//! `arena-hub`: the branch Hub service, run by the `arena-hub` binary and, on Android, in-process
//! by the Hub app.
//!
//! Start-up: parse args, init logs, open the store (exit 2 on `SCHEMA_TOO_NEW`), load or mint
//! the instance id, load or generate the LAN TLS certificate, start the lease task and the WAN connectivity probe, `/healthz` + `/ui/snapshot` + the staff auth routes
//! and the LAN listeners (`:7400`, `:7401`), then run until Ctrl+C or SIGTERM.
//!
//! Logs carry ids, codes and counts only — no PII (see `logs.rs`).

mod config;
#[cfg(feature = "dev-fixtures")]
mod dev;
mod logs;

use std::process::ExitCode;
use std::sync::Arc;

use std::net::{IpAddr, Ipv4Addr};

use arena_api::lan::{HubInfoSource, LanHub, bind_lan, serve_lan, tls};
use arena_api::{bind_health, serve_router};
use arena_core::{
    Clock, DeviceDirectory, HubStatus, PairGate, Role, SnapshotReader, StaffDirectory, SystemClock,
    load_or_mint_instance_id, record_tls_fingerprint, status_channel,
};
use arena_proto::{AppError, ErrorCode};
use arena_store::{LATEST_SCHEMA_VERSION, Store};
use arena_tasks::{
    Advertiser, AndroidSecrets, CloudClient, CloudEndpoint, CloudPairer, CommandTask,
    ConnectivityTask, HeartbeatTask, HousekeepingTask, Keystore, LeaseTask, MdnsAdvertiser,
    MdnsTask, NetProber, NetworkName, SecretWrapper, SystemInterfaces, platform_keystore,
};
use tokio::sync::{Notify, oneshot};
use tracing_subscriber::EnvFilter;

pub use arena_core::setup::BackupFolder;

static STOP: std::sync::Mutex<Option<Arc<Notify>>> = std::sync::Mutex::new(None);

pub fn stop() {
    let slot = STOP.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(n) = slot {
        n.notify_one();
    }
}

const EXIT_FAILURE: u8 = 1;
const EXIT_SCHEMA_TOO_NEW: u8 = 2;
const EXIT_USAGE: u8 = 64;

/// How often the Hub re-checks that the chosen backup folder can still be written.
const BACKUP_CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(3600);

/// What the host process injects into the Hub.
#[derive(Default)]
pub struct StartOptions {
    /// The Android Keystore wrapper (the Android shell supplies the JNI-backed one). With it the
    /// device token and branch key are stored wrapped; without it the platform keystore is used.
    pub secret_wrapper: Option<Arc<dyn SecretWrapper>>,
    /// Advertises the Hub on the LAN in place of the `mdns-sd` daemon (the Android shell
    /// supplies one backed by `NsdManager`). Taken by the first start.
    pub advertiser: Option<Box<dyn Advertiser>>,
    /// The shell notifies this when the network changes, so the Hub re-reads its addresses now.
    pub network_wake: Option<Arc<Notify>>,
    /// Names the network the Hub is on (shown in Settings → Devices).
    pub network_name: Option<Arc<dyn NetworkName>>,
    /// The Android shell's Bluetooth SPP bridge. With it the Hub prints through Bluetooth and
    /// LAN (no USB); without it the platform adapter is used.
    pub bluetooth: Option<Arc<dyn arena_tasks::printer::BluetoothLink>>,
    /// The Android shell's folder access (the document tree the owner picked). With it the Hub
    /// accepts a picked folder as the backup location and checks it at start and hourly.
    pub backup_folder: Option<Arc<dyn BackupFolder>>,
}

pub async fn start(args: impl Iterator<Item = std::ffi::OsString>) -> ExitCode {
    start_with(args, StartOptions::default()).await
}

pub async fn start_with(
    args: impl Iterator<Item = std::ffi::OsString>,
    options: StartOptions,
) -> ExitCode {
    let cfg = match config::parse(args, |k| std::env::var_os(k), cfg!(windows)) {
        Ok(config::Command::Run(cfg)) => cfg,
        #[cfg(feature = "dev-fixtures")]
        Ok(config::Command::DevSeed(cfg, opts)) => return dev_seed(cfg, opts).await,
        #[cfg(feature = "dev-fixtures")]
        Ok(config::Command::DevRevokeDevice {
            device_id,
            health_port,
        }) => return dev_revoke(&device_id, health_port),
        Ok(config::Command::Help) => {
            println!("{}", config::USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(config::Command::Version) => {
            println!("arena-hub {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("arena-hub: {e}\n{}", config::USAGE);
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let ring = match std::fs::create_dir_all(&cfg.data_dir)
        .and_then(|()| logs::RingLog::open(cfg.logs_dir(), Arc::clone(&clock)))
    {
        Ok(ring) => ring,
        Err(e) => {
            eprintln!("arena-hub: cannot open logs: {e}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    let _ = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(Arc::new(ring))
        .try_init();

    match run(cfg, clock, options).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.code == ErrorCode::SchemaTooNew => {
            tracing::error!(
                code = e.code.as_str(),
                binary_schema_version = LATEST_SCHEMA_VERSION,
                detail = %e.message,
                "refusing to start"
            );
            eprintln!("arena-hub: {e}");
            ExitCode::from(EXIT_SCHEMA_TOO_NEW)
        }
        Err(e) => {
            tracing::error!(code = e.code.as_str(), detail = %e.message, "hub failed");
            eprintln!("arena-hub: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Runs `dev seed` and prints the outcome: exit 0 on success, 2 on `SCHEMA_TOO_NEW` and 1
/// otherwise (including `ALREADY_BOOTSTRAPPED` and `NOT_PRIMARY`).
#[cfg(feature = "dev-fixtures")]
async fn dev_seed(cfg: config::Config, opts: config::SeedOptions) -> ExitCode {
    match dev::seed(&cfg, &opts, Arc::new(SystemClock)).await {
        Ok(seeded) => {
            println!(
                "dev seed: bootstrapped branch DEV01 in {} ({} changes)",
                cfg.data_dir.display(),
                seeded.changes
            );
            println!(
                "dev seed: registered paired device \"{}\" {}",
                dev::DEV_KIOSK_LABEL,
                seeded.device_id
            );
            println!("dev seed: device token (shown once): {}", seeded.token);
            println!(
                "dev seed: paired as {} for tenant {} (tier {}, dev licence)",
                seeded.identity.hub_id,
                seeded.identity.tenant_id,
                seeded.identity.tier.as_str()
            );
            // Dev-fixtures builds only: release builds have no dev module.
            println!("dev seed: staff credentials (shown once, dev only):");
            for s in &seeded.staff {
                println!(
                    "dev seed:   {:<8} role {:<8} password {:<20} pin {}  ({})",
                    s.login, s.role, s.password, s.pin, s.staff_id
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dev seed: {e}");
            if e.code == ErrorCode::SchemaTooNew {
                ExitCode::from(EXIT_SCHEMA_TOO_NEW)
            } else {
                ExitCode::from(EXIT_FAILURE)
            }
        }
    }
}

/// Runs `dev revoke-device` against the running Hub: exit 0 once it applied the revoke.
#[cfg(feature = "dev-fixtures")]
fn dev_revoke(device_id: &str, health_port: u16) -> ExitCode {
    match dev::revoke_device(device_id, health_port) {
        Ok(body) => {
            println!("dev revoke-device: revoked {device_id} {body}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dev revoke-device: {e}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Resolves on Ctrl+C. If the handler cannot be installed, it logs and never resolves, so a
/// handler failure is not mistaken for a shutdown request.
async fn ctrl_c() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %e, "ctrl-c handler failed");
        std::future::pending::<()>().await;
    }
}

/// Resolves on SIGTERM (Unix only; never resolves elsewhere or if the handler fails).
async fn terminate() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
                return;
            }
            Err(e) => tracing::error!(error = %e, "sigterm handler failed"),
        }
    }
    std::future::pending::<()>().await;
}

fn internal(message: String) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

async fn run(
    cfg: config::Config,
    clock: Arc<dyn Clock>,
    mut options: StartOptions,
) -> Result<(), AppError> {
    let stop_signal = Arc::new(Notify::new());
    *STOP.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&stop_signal));
    let mut store = Store::open(cfg.db_path(), clock.now_ms())?;
    let schema_version = store.schema_version()?;
    let instance_id = load_or_mint_instance_id(&mut store)?;
    tracing::info!(
        hub_instance_id = %instance_id,
        schema_version,
        writer_lock = store.holds_writer_lock(),
        "store open"
    );
    let identity = store.hub_identity()?;
    let secrets = options
        .secret_wrapper
        .as_ref()
        .map(|w| AndroidSecrets::new(&cfg.data_dir, w));
    // Before anything can mint a branch key: has the key that wrapped the secrets been lost?
    let keys_lost = identity.is_some() && secrets.as_ref().is_some_and(|s| s.lost(true));
    let pair_gate = if keys_lost {
        PairGate::needing_repair()
    } else {
        PairGate::new(identity.is_some())
    };
    if keys_lost {
        tracing::warn!("the keystore key is lost; the hub needs to be paired again");
    } else if pair_gate.is_paired() {
        tracing::info!("hub paired; pairing screen skipped");
    } else {
        tracing::info!("hub not paired yet; serving the pairing screen");
    }

    // The LAN certificate: generated once by the writer, reused afterwards; a follower never
    // writes it. Its fingerprint goes into hub_meta like the instance id.
    let hostname = tls::hostname();
    let hub_name = arena_proto::sanitize_hub_name(hostname.as_deref().unwrap_or_default());
    let cert = tls::load_or_create(
        &cfg.data_dir,
        hostname.as_deref(),
        store.holds_writer_lock(),
    )?;
    record_tls_fingerprint(&mut store, &cert.fingerprint)?;
    tracing::info!(tls_fingerprint = %cert.fingerprint, "lan certificate ready");
    let server_tls = cert.server_config()?;
    let devices = Arc::new(DeviceDirectory::open(cfg.db_path())?);
    let snapshots = Arc::new(SnapshotReader::open(cfg.db_path())?);
    let staff = Arc::new(StaffDirectory::open(cfg.db_path())?);

    let mut initial = HubStatus::opened(schema_version, instance_id.clone());
    initial.tls_fingerprint = Some(cert.fingerprint.clone());
    initial.lan_ports = Some((cfg.lan_port, cfg.web_port));
    // The effective tier is unknown until the heartbeat task has evaluated the cached licence
    // against trusted time (Story 4.15): the tier recorded at pairing is not it. A paid Android
    // Hub that restarts with the WAN down must not report Free (and show ads) in between.
    initial.tier = None;
    initial.platform = cfg.platform;
    if cfg.platform == arena_proto::HubPlatform::Android {
        // Read the addresses before the first snapshot, so the header does not flash
        // "No LAN connection" while the mDNS task starts.
        initial.lan_addresses = arena_tasks::lan_ipv4_addresses();
    }
    let (status_tx, status_rx) = status_channel(initial);
    let listener = bind_health(cfg.health_port)
        .await
        .map_err(|e| internal(format!("bind 127.0.0.1:{}: {e}", cfg.health_port)))?;
    let any = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let lan_listeners = bind_lan(any, cfg.lan_port, cfg.web_port)
        .await
        .map_err(|e| {
            internal(format!(
                "bind 0.0.0.0:{} / 0.0.0.0:{}: {e}",
                cfg.lan_port, cfg.web_port
            ))
        })?;

    let lease = LeaseTask::start(
        store,
        instance_id.clone(),
        Arc::clone(&clock),
        status_tx.clone(),
    )
    .await?;
    let writer = lease.writer();
    // Housekeeping (dedupe prune) runs at start and daily while primary; it ends once the
    // writer closes.
    let housekeeping = writer.clone().map(|w| {
        tokio::spawn(HousekeepingTask::new(w, Arc::clone(&clock), status_tx.subscribe()).run())
    });
    let mut lease_job = Some(tokio::spawn(lease.run()));
    // The WAN probe: the branch's only outbound traffic. It runs on its own task, publishes
    // into the status and never touches the writer, so no intent waits on it.
    let prober = NetProber::new(cfg.probe_internet.clone(), cfg.cloud_host.clone())
        .map_err(|e| internal(format!("connectivity probe: {e}")))?
        .plain_http(cfg.cloud_plain_http);
    let probe_job =
        tokio::spawn(ConnectivityTask::new(prober, Arc::clone(&clock), status_tx.clone()).run());
    tracing::info!(
        cloud_host = %format!("{}:{}", cfg.cloud_host.host, cfg.cloud_host.port),
        internet_endpoints = cfg.probe_internet.len(),
        "connectivity probe started"
    );
    let (mdns_stop_tx, mdns_stop_rx) = oneshot::channel::<()>();
    let advertiser: Box<dyn Advertiser> = options
        .advertiser
        .take()
        .unwrap_or_else(|| Box::new(MdnsAdvertiser::default()));
    let mdns_job = tokio::spawn(
        MdnsTask::new(
            advertiser,
            Arc::clone(&snapshots),
            SystemInterfaces,
            status_tx.clone(),
            hub_name.clone(),
            cfg.lan_port,
            cfg.web_port,
        )
        .with_applied(writer.as_ref().map(|w| w.subscribe()))
        .with_wake(options.network_wake.take())
        .with_network(options.network_name.take())
        .run(async {
            let _ = mdns_stop_rx.await;
        }),
    );

    // The game media cache (Story 6.2): its own database and files under `reference/`, outside the
    // branch store. It is not critical: without it the Hub runs and serves no media.
    let media_cache = {
        let cap_source = Arc::clone(&snapshots);
        let cap: arena_core::media_cache::CapFn = Arc::new(move || {
            arena_core::media_cache::media_cap_bytes(&cap_source.settings().unwrap_or_default())
        });
        match arena_core::media_cache::MediaCache::open(cfg.data_dir.join("reference"), cap) {
            Ok(cache) => Some(Arc::new(cache)),
            Err(e) => {
                tracing::error!(code = e.code.as_str(), detail = %e.message, "the game media cache could not be opened");
                None
            }
        }
    };
    let pairing = Arc::new(arena_api::pairing::Pairing::new(Arc::clone(&clock))?);
    // The kitchen display code (Story 7.6) is a second instance: its own key, its own live code, and
    // no owner check, so a used or expired code is not renewed.
    let kds_pairing = Arc::new(arena_api::pairing::Pairing::new(Arc::clone(&clock))?);
    // The built Kitchen display app is served from a directory (`ARENA_KITCHEN_DIR`, default
    // `kitchen/` beside the Hub executable); without it the Hub serves a built-in pairing page.
    let kitchen_dir = std::env::var_os("ARENA_KITCHEN_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("kitchen")))
        });
    // The player key: opens profiles for the Hub UI and a player's name for a kiosk login.
    let player_kek: Arc<dyn arena_core::players::Kek> = Arc::new(
        arena_tasks::KeystoreKek::new(match &secrets {
            Some(s) => Box::new(Arc::clone(&s.kek)),
            None => arena_tasks::named_keystore(&cfg.data_dir, arena_tasks::KEK_FILE),
        })
        .refusing_while_lost(pair_gate.clone()),
    );
    let mut lan_hub = LanHub::new(
        writer.clone(),
        status_rx.clone(),
        Arc::clone(&clock),
        devices,
        server_tls,
        env!("CARGO_PKG_VERSION"),
    )
    .with_staff(Arc::clone(&staff))
    .with_player_kek(Arc::clone(&player_kek))
    .with_pairing(Arc::clone(&pairing))
    .with_kds_pairing(Arc::clone(&kds_pairing))
    .with_info(HubInfoSource {
        reader: Arc::clone(&snapshots),
        hub_name,
        lan_port: cfg.lan_port,
        web_port: cfg.web_port,
    });
    if let Some(cache) = &media_cache {
        lan_hub = lan_hub.with_media(Arc::clone(cache));
    }
    if let Some(dir) = kitchen_dir {
        lan_hub = lan_hub.with_kitchen_dir(dir);
    }
    let lan_hub = Arc::new(lan_hub);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    #[cfg(feature = "dev-fixtures")]
    let fake_printer = (cfg.printer == config::PrinterChoice::Fake)
        .then(|| Arc::new(arena_tasks::printer::FakePrinter::dev()));
    let printer: Arc<dyn arena_core::printer::Printer> = match cfg.printer {
        config::PrinterChoice::System => match options.bluetooth.take() {
            Some(link) => Arc::new(arena_tasks::printer::AndroidPrinter::new(link)),
            None => Arc::new(arena_tasks::printer::SystemPrinter::new()),
        },
        #[cfg(feature = "dev-fixtures")]
        config::PrinterChoice::Fake => match &fake_printer {
            Some(fake) => Arc::clone(fake) as Arc<dyn arena_core::printer::Printer>,
            None => return Err(internal("the fake printer was not created".to_owned())),
        },
    };
    tracing::info!(adapter = ?cfg.printer, "printer adapter ready");
    let printer_jobs = Arc::new(arena_core::printer::PrinterJobs::default());
    let receipt_printer = Arc::clone(&printer);
    let backup_folder = options.backup_folder.take();
    let mut ui_auth = arena_api::UiAuth::new(
        Arc::clone(&staff),
        Arc::clone(&snapshots),
        writer.clone(),
        Arc::clone(&clock),
    )
    .with_status(status_rx.clone())
    .with_presence(lan_hub.presence())
    .with_printer(printer)
    .with_printer_jobs(Arc::clone(&printer_jobs))
    .with_pairing(pairing)
    .with_kds_pairing(kds_pairing)
    .with_pair_gate(pair_gate.clone())
    .with_player_kek(Arc::clone(&player_kek));
    if let Some(folder) = &backup_folder {
        ui_auth = ui_auth.with_backup_folder(Arc::clone(folder));
    }
    if let Some(cache) = &media_cache {
        ui_auth = ui_auth.with_media_cache(Arc::clone(cache));
    }
    let ui_auth = Arc::new(ui_auth);
    let cloud = CloudClient::new(&CloudEndpoint {
        host: cfg.cloud_host.clone(),
        plain_http: cfg.cloud_plain_http,
    })
    .map_err(|e| internal(format!("cloud client: {e}")))?;
    let keystore: Arc<dyn Keystore> = match &secrets {
        Some(s) => Arc::clone(&s.token),
        None => Arc::from(platform_keystore(&cfg.data_dir)),
    };
    let verify_keys = [
        arena_proto::licence::LICENCE_VERIFY_KEYS,
        #[cfg(feature = "dev-fixtures")]
        arena_proto::licence::DEV_LICENCE_VERIFY_KEYS,
    ]
    .concat();
    let command_poke = Arc::new(Notify::new());
    let setup_templates = arena_tasks::SetupTemplateInbox::new();
    let resend = arena_tasks::ResendInbox::new();
    let heartbeat_job = tokio::spawn(
        HeartbeatTask::new(
            cloud.clone(),
            Arc::clone(&keystore),
            writer.clone(),
            pair_gate.clone(),
            status_tx.clone(),
            verify_keys.clone(),
            env!("CARGO_PKG_VERSION").to_owned(),
        )
        .with_poke(Arc::clone(&command_poke))
        .with_templates(Arc::clone(&setup_templates))
        .with_resend(Arc::clone(&resend))
        .with_presence({
            let presence = lan_hub.presence();
            Arc::new(move |id| presence.status(id))
        })
        .run(),
    );
    // `backup.now` (Story 8.19) asks the upload task for an immediate cycle.
    let (backup_requester, backup_responder) = arena_tasks::backup_channel();
    // The change-log upload (Story 8.2): primary and paid only, reads through the snapshot reader.
    let upload_job = tokio::spawn(
        arena_tasks::UploadTask::new(
            cloud.clone(),
            Arc::clone(&keystore),
            writer.clone(),
            Arc::clone(&snapshots),
            pair_gate.clone(),
            status_tx.clone(),
            verify_keys,
        )
        .with_resend(resend)
        .with_backup(backup_responder)
        .run(),
    );
    let command_job = tokio::spawn(
        CommandTask::new(
            cloud.clone(),
            Arc::clone(&keystore),
            writer.clone(),
            pair_gate.clone(),
            status_tx.clone(),
            command_poke,
        )
        .with_backup(backup_requester)
        .with_presence({
            let presence = lan_hub.presence();
            Arc::new(move |id| presence.status(id))
        })
        .run(),
    );
    let template_job = tokio::spawn(
        arena_tasks::SetupTemplateTask::new(
            cloud.clone(),
            Arc::clone(&keystore),
            writer.clone(),
            Arc::clone(&snapshots),
            pair_gate.clone(),
            status_tx.clone(),
            Arc::clone(&clock),
        )
        .run(),
    );
    let template_apply_job = tokio::spawn(
        arena_tasks::SetupApplyTask::new(
            cloud.clone(),
            Arc::clone(&keystore),
            writer.clone(),
            pair_gate.clone(),
            status_tx.clone(),
            Arc::clone(&clock),
            Arc::clone(&setup_templates),
        )
        .run(),
    );
    // The catalogue and media pull (Story 6.2): primary only, never through the writer.
    let catalog_job = media_cache.clone().map(|cache| {
        tokio::spawn(
            arena_tasks::CatalogPullTask::new(
                cloud.clone(),
                Arc::clone(&keystore),
                pair_gate.clone(),
                status_tx.clone(),
                cache,
                Arc::clone(&clock),
            )
            .run(),
        )
    });
    let test_session_job = writer.clone().map(|w| {
        tokio::spawn(
            arena_tasks::TestSessionTask::new(w, Arc::clone(&snapshots), Arc::clone(&clock)).run(),
        )
    });
    let timer_job = writer.clone().map(|w| {
        let (events, mut forward) =
            tokio::sync::broadcast::channel(arena_tasks::TIMER_EVENTS_DEPTH);
        let presence = lan_hub.presence();
        let relay = tokio::spawn(async move {
            loop {
                match forward.recv().await {
                    Ok(event) => {
                        presence.push_timer_event(event);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "timer events dropped");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        (
            tokio::spawn(
                arena_tasks::SessionTimerTask::new(
                    w,
                    Arc::clone(&snapshots),
                    Arc::clone(&clock),
                    events,
                )
                .run(),
            ),
            relay,
        )
    });
    let invoice_job = writer.clone().map(|w| {
        tokio::spawn(
            arena_tasks::InvoiceTask::new(w, Arc::clone(&snapshots), Arc::clone(&clock)).run(),
        )
    });
    let receipt_job = writer.clone().map(|w| {
        tokio::spawn(
            arena_tasks::ReceiptPrintTask::new(
                w,
                Arc::clone(&snapshots),
                Arc::clone(&clock),
                receipt_printer,
                printer_jobs,
            )
            .run(),
        )
    });
    // The notification drain (Story 8.29): primary and paid only, sends through the cloud.
    let notify_job = writer.clone().map(|w| {
        tokio::spawn(
            arena_tasks::NotifyDrainTask::new(
                cloud.clone(),
                Arc::clone(&keystore),
                w,
                Arc::clone(&snapshots),
                Arc::clone(&clock),
                pair_gate.clone(),
                status_tx.clone(),
            )
            .with_player_kek(Arc::clone(&player_kek))
            .run(),
        )
    });
    // The session messages (Story 8.30): low balance, session ending, console 5 minutes. They are
    // queued in the outbox the drain above sends from.
    let notify_trigger_job = writer.clone().map(|w| {
        tokio::spawn(
            arena_tasks::NotifyTriggerTask::new(
                w,
                Arc::clone(&snapshots),
                Arc::clone(&clock),
                status_tx.clone(),
            )
            .run(),
        )
    });
    // The daily credit reminders (Story 8.31): from 11:00 branch time, one per player on a cycle
    // day, queued in the same outbox.
    let credit_reminder_job = writer.clone().map(|w| {
        tokio::spawn(
            arena_tasks::CreditReminderTask::new(
                w,
                Arc::clone(&snapshots),
                Arc::clone(&clock),
                status_tx.clone(),
            )
            .run(),
        )
    });
    let mut cloud_pairer = CloudPairer::new(
        cloud,
        writer.clone(),
        keystore,
        pair_gate.clone(),
        Arc::clone(&clock),
        cfg.platform,
        instance_id,
        env!("CARGO_PKG_VERSION").to_owned(),
    );
    cloud_pairer = cloud_pairer.with_templates(setup_templates);
    if let Some(s) = &secrets {
        cloud_pairer = cloud_pairer.with_kek_store(Arc::clone(&s.kek));
    }
    let pairer: Arc<dyn arena_core::HubPairer> = Arc::new(cloud_pairer);
    #[cfg_attr(not(feature = "dev-fixtures"), allow(unused_mut))]
    let setup_job = tokio::spawn(arena_core::setup::publish_setup(
        Arc::clone(&snapshots),
        writer.as_ref().map(|w| w.subscribe()),
        status_tx.clone(),
    ));
    let backup_job = backup_folder.map(|folder| {
        tokio::spawn(arena_core::setup::watch_backup_folder(
            Arc::clone(&snapshots),
            writer.as_ref().map(|w| w.subscribe()),
            status_tx.clone(),
            folder,
            BACKUP_CHECK_EVERY,
        ))
    });
    let router = arena_api::router(status_rx.clone(), Arc::clone(&clock))
        .merge(arena_api::ui_router(
            snapshots,
            Arc::clone(&clock),
            lan_hub.presence(),
            Some(status_rx.clone()),
            Some(pair_gate.clone()),
        ))
        .merge(arena_api::ui_auth_router(ui_auth))
        .merge(arena_api::ui_pair_router(pairer, pair_gate));
    #[cfg(feature = "dev-fixtures")]
    let router = router.merge(arena_api::health::dev_router(
        writer.clone(),
        Arc::clone(&clock),
    ));
    #[cfg(feature = "dev-fixtures")]
    let router = match fake_printer {
        Some(fake) => {
            let jobs: arena_api::health::PrintedJobs = Arc::new(move || {
                fake.sent()
                    .into_iter()
                    .map(|job| {
                        let target = match job.target {
                            arena_core::printer::PrinterTarget::Usb(name) => format!("usb:{name}"),
                            arena_core::printer::PrinterTarget::Lan(addr) => format!("lan:{addr}"),
                            arena_core::printer::PrinterTarget::Bluetooth(addr) => {
                                format!("bt:{addr}")
                            }
                        };
                        (target, job.bytes)
                    })
                    .collect()
            });
            router.merge(arena_api::health::dev_printer_router(jobs))
        }
        None => router,
    };
    let mut server = tokio::spawn(serve_router(listener, router, async {
        let _ = stop_rx.await;
    }));
    tracing::info!(
        port = cfg.health_port,
        "healthz, ui snapshot and ui auth listening on loopback"
    );

    let (lan_stop_tx, lan_stop_rx) = oneshot::channel::<()>();
    let lan = tokio::spawn(serve_lan(lan_listeners, lan_hub, async {
        let _ = lan_stop_rx.await;
    }));
    tracing::info!(
        lan_port = cfg.lan_port,
        web_port = cfg.web_port,
        "lan listening"
    );

    let outcome = loop {
        tokio::select! {
            () = ctrl_c() => {
                tracing::info!(signal = "ctrl_c", "shutting down");
                break Ok(());
            }
            () = terminate() => {
                tracing::info!(signal = "sigterm", "shutting down");
                break Ok(());
            }
            () = stop_signal.notified() => {
                tracing::info!(signal = "stop", "shutting down");
                break Ok(());
            }
            res = &mut server => {
                let msg = match res {
                    Ok(Ok(())) => "healthz server stopped unexpectedly".to_owned(),
                    Ok(Err(e)) => format!("healthz server failed: {e}"),
                    Err(e) => format!("healthz server task failed: {e}"),
                };
                tracing::error!(detail = %msg, "early exit");
                break Err(internal(msg));
            }
            res = async { lease_job.as_mut().expect("guarded").await }, if lease_job.is_some() => {
                lease_job = None;
                match res {
                    // Demotion (or a follower start) ends the lease task; keep serving healthz.
                    Ok(()) => tracing::info!("lease task ended"),
                    Err(e) => {
                        let msg = format!("lease task failed: {e}");
                        tracing::error!(detail = %msg, "early exit");
                        break Err(internal(msg));
                    }
                }
            }
        }
    };

    // Healthz must not claim primary or an open database once the writer goes away.
    status_tx.send_modify(|s| {
        s.role = Role::Follower;
        s.db_open = false;
    });
    if let Some(job) = lease_job {
        job.abort();
        let _ = job.await;
    }
    if let Some(job) = housekeeping {
        job.abort();
        let _ = job.await;
    }
    probe_job.abort();
    let _ = probe_job.await;
    heartbeat_job.abort();
    let _ = heartbeat_job.await;
    command_job.abort();
    let _ = command_job.await;
    upload_job.abort();
    let _ = upload_job.await;
    template_job.abort();
    let _ = template_job.await;
    template_apply_job.abort();
    let _ = template_apply_job.await;
    if let Some(job) = catalog_job {
        job.abort();
        let _ = job.await;
    }
    setup_job.abort();
    let _ = setup_job.await;
    if let Some(job) = backup_job {
        job.abort();
        let _ = job.await;
    }
    if let Some(job) = test_session_job {
        job.abort();
        let _ = job.await;
    }
    if let Some(job) = invoice_job {
        job.abort();
        let _ = job.await;
    }
    if let Some(job) = notify_job {
        job.abort();
        let _ = job.await;
    }
    if let Some(job) = notify_trigger_job {
        job.abort();
        let _ = job.await;
    }
    if let Some(job) = credit_reminder_job {
        job.abort();
        let _ = job.await;
    }
    if let Some(job) = receipt_job {
        job.abort();
        let _ = job.await;
    }
    if let Some((job, relay)) = timer_job {
        job.abort();
        let _ = job.await;
        relay.abort();
        let _ = relay.await;
    }
    let _ = mdns_stop_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), mdns_job).await;
    // Close every LAN session before the writer goes away.
    let _ = lan_stop_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), lan).await;
    if let Some(writer) = writer {
        writer.close().await;
    }
    let _ = stop_tx.send(());
    if !server.is_finished() {
        let _ = server.await;
    }
    tracing::info!("stopped");
    outcome
}
