//! The Android Hub hands the branch to a Windows Hub over the LAN (Story 10.17): a real TLS
//! listener on the tablet side, the `arena-tasks` puller on the PC side, both over loopback.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use arena_api::lan::{HubCert, HubInfoSource, LanHub, bind_lan, serve_lan};
use arena_core::handover::{Handover, HandoverJoinControl};
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, ManualClock, Role, SnapshotReader, Writer, WriterHandle,
    status_channel,
};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ErrorCode, HANDOVER_FAILURES_PER_CODE, HandoverJoinRequest,
    HandoverPhase, Intent, IntentPayload, MigrationNotice, ProductUpsert,
};
use arena_store::{HubIdentity, LATEST_SCHEMA_VERSION, LeaseRow, Store, write_lease};
use arena_tasks::HandoverPuller;
use tokio::sync::{oneshot, watch};

const T0: i64 = 1_800_000_000_000;
const PC: &str = "Front desk";

fn intent(actor: Actor, payload: IntentPayload) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: T0,
        base_rev: None,
        actor,
        elevated_by: None,
        reason: None,
        payload,
    }
}

fn owner() -> Actor {
    Actor {
        kind: ActorKind::Owner,
        id: "test-owner".into(),
    }
}

fn lease(hub: &str) -> LeaseRow {
    LeaseRow {
        counter: 1,
        hub_instance_id: hub.into(),
        provisional: true,
        renewed_at: T0,
        expires_at: i64::MAX,
    }
}

struct Tablet {
    _dir: tempfile::TempDir,
    path: PathBuf,
    web: SocketAddr,
    lan: SocketAddr,
    fingerprint: String,
    writer: WriterHandle,
    handover: Arc<Handover>,
    status: watch::Sender<HubStatus>,
    branch_id: String,
    _stop: oneshot::Sender<()>,
}

async fn tablet() -> Tablet {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, T0).unwrap();
    store
        .write_tx(|tx| write_lease(tx, &lease("01JHUBAAAAAAAAAAAAAAAAAAAA")))
        .unwrap();
    let mut initial = HubStatus::opened(
        store.schema_version().unwrap(),
        "01JHUBAAAAAAAAAAAAAAAAAAAA",
    );
    initial.role = Role::Primary;
    initial.lease = Some(LeaseInfo::from(&lease("01JHUBAAAAAAAAAAAAAAAAAAAA")));
    let (status_tx, status_rx) = status_channel(initial);
    let clock = Arc::new(ManualClock::new(T0));
    let writer = Writer::spawn(store, clock.clone(), status_rx.clone()).unwrap();
    writer
        .submit_intent(intent(
            Actor {
                kind: ActorKind::System,
                id: "test".into(),
            },
            IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "Dev Branch".into(),
                branch_code: "DEV01".into(),
                currency: "INR".into(),
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
                onboarding: None,
            }),
        ))
        .await
        .unwrap();
    // The branch id the snapshot header states, which is also the tablet's cloud identity.
    let probe = tempfile::tempdir().unwrap();
    let branch_id = arena_store::write_export(&path, probe.path(), "probe.a360", T0)
        .unwrap()
        .header
        .branch_id;
    {
        let branch_id = branch_id.clone();
        writer
            .submit(move |tx| {
                arena_store::insert_hub_identity(
                    tx,
                    &HubIdentity {
                        tenant_id: "T".into(),
                        branch_id,
                        hub_id: "H1".into(),
                        tier: arena_proto::Tier::Free,
                        paired_at: T0,
                    },
                )
                .map(|_| ())
            })
            .await
            .unwrap();
    }
    let reader = Arc::new(SnapshotReader::open(&path).unwrap());
    let cert = HubCert::generate(None).unwrap();
    status_tx.send_modify(|s| {
        s.tls_fingerprint = Some(cert.fingerprint.clone());
        s.move_notice = Some(MigrationNotice {
            id: "M1".into(),
            target_hub_name: PC.into(),
        });
    });
    let handover = Arc::new(
        Handover::open(
            writer.clone(),
            Arc::clone(&reader),
            status_tx.clone(),
            clock.clone(),
            None,
        )
        .unwrap(),
    );
    let hub = Arc::new(
        LanHub::new(
            Some(writer.clone()),
            status_rx,
            clock,
            Arc::new(DeviceDirectory::open(&path).unwrap()),
            cert.server_config().unwrap(),
            "0.1.0",
        )
        .with_handover(Arc::clone(&handover), dir.path().to_path_buf())
        .with_signer(cert.clone())
        .with_info(HubInfoSource {
            reader: Arc::clone(&reader),
            hub_name: "Tablet".into(),
            lan_port: 7400,
            web_port: 7401,
        }),
    );
    let listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let web = listeners.web_addr().unwrap();
    let lan = listeners.lan_addr().unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, hub, async {
        let _ = stop_rx.await;
    }));
    Tablet {
        _dir: dir,
        path,
        web,
        lan,
        fingerprint: cert.fingerprint.clone(),
        writer,
        handover,
        status: status_tx,
        branch_id,
        _stop: stop_tx,
    }
}

impl Tablet {
    async fn serves(&self) -> bool {
        let mut i = intent(
            owner(),
            IntentPayload::ProductUpsert(ProductUpsert {
                id: None,
                name: format!("Tea {}", ulid::Ulid::generate()),
                category: String::new(),
                price_minor: 100,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: true,
                image: String::new(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active: true,
                bundle: false,
                components: Vec::new(),
                kitchen_notes: None,
            }),
        );
        i.issued_at = T0;
        match self.writer.submit_intent(i).await {
            Ok(_) => true,
            Err(e) => {
                assert_eq!(e.code, ErrorCode::HubHandingOver);
                false
            }
        }
    }

    async fn code(&self) -> String {
        match self.handover.view().await.and_then(|v| v.code) {
            Some(c) => c,
            None => self.handover.begin().await.unwrap().code.unwrap(),
        }
    }
}

struct Pc {
    _dir: tempfile::TempDir,
    puller: HandoverPuller,
    handover: Arc<Handover>,
    folder: PathBuf,
}

async fn pc(branch_id: &str) -> Pc {
    pc_with_status(branch_id).await.0
}

async fn pc_with_status(branch_id: &str) -> (Pc, watch::Sender<HubStatus>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, T0).unwrap();
    store
        .write_tx(|tx| write_lease(tx, &lease("01JHUBBBBBBBBBBBBBBBBBBBBB")))
        .unwrap();
    store
        .write_tx(|tx| {
            arena_store::insert_hub_identity(
                tx,
                &HubIdentity {
                    tenant_id: "T".into(),
                    branch_id: branch_id.into(),
                    hub_id: "H2".into(),
                    tier: arena_proto::Tier::Basic,
                    paired_at: T0,
                },
            )
            .map(|_| ())
        })
        .unwrap();
    store
        .write_tx(|tx| arena_store::set_meta(tx, arena_store::MIGRATION_WAIT_KEY, "Dev Branch"))
        .unwrap();
    let mut initial = HubStatus::opened(LATEST_SCHEMA_VERSION, "01JHUBBBBBBBBBBBBBBBBBBBBB");
    initial.tls_fingerprint = Some("cd".repeat(32));
    initial.lan_addresses = vec![Ipv4Addr::new(192, 168, 1, 20)];
    initial.lan_ports = Some((7400, 7401));
    initial.role = Role::Primary;
    initial.lease = Some(LeaseInfo::from(&lease("01JHUBBBBBBBBBBBBBBBBBBBBB")));
    initial.migration_wait = Some("Dev Branch".into());
    let (status_tx, status_rx) = status_channel(initial);
    let clock = Arc::new(ManualClock::new(T0));
    let writer = Writer::spawn(store, clock.clone(), status_rx.clone()).unwrap();
    let reader = Arc::new(SnapshotReader::open(&path).unwrap());
    let handover = Arc::new(
        Handover::open(writer, Arc::clone(&reader), status_tx.clone(), clock, None).unwrap(),
    );
    let folder = dir.path().join("handover");
    let made = Pc {
        puller: HandoverPuller::new(
            Arc::clone(&handover),
            reader,
            status_rx,
            PC.into(),
            folder.clone(),
        ),
        handover,
        folder,
        _dir: dir,
    };
    (made, status_tx)
}

fn join(t: &Tablet, code: &str) -> HandoverJoinRequest {
    HandoverJoinRequest {
        address: t.web.to_string(),
        code: code.into(),
    }
}

#[tokio::test]
async fn the_pc_takes_the_final_snapshot_and_the_tablet_steps_down() {
    let t = tablet().await;
    let pc = pc(&t.branch_id).await;
    let code = t.code().await;
    assert!(t.serves().await, "the tablet serves while the code shows");

    let view = pc.puller.join(join(&t, &code)).await.unwrap();
    assert_eq!(view.phase, HandoverPhase::Received);

    // The PC holds a file whose hash, branch and seq were checked.
    let (file, facts) = pc.handover.received().await.unwrap();
    assert!(file.starts_with(&pc.folder) && file.exists());
    assert_eq!(facts.branch_id, t.branch_id);
    let last = Store::open_reader(&t.path)
        .unwrap()
        .changes()
        .unwrap()
        .last()
        .unwrap()
        .seq;
    assert_eq!(facts.last_seq, last);

    // The tablet serves nothing more and says the branch moved; the heartbeat will report it.
    assert!(!t.serves().await);
    assert_eq!(t.handover.view().await.unwrap().phase, HandoverPhase::Moved);
    let reported = t.handover.handed_over().await.unwrap();
    assert_eq!(
        (reported.migration_id.as_str(), reported.last_seq),
        ("M1", last)
    );
    assert!(
        Store::open_reader(&t.path)
            .unwrap()
            .changes()
            .unwrap()
            .last()
            .unwrap()
            .seq
            == last,
        "nothing was written after the snapshot"
    );
    // A restart of the PC remembers what it holds.
    assert_eq!(
        pc.handover.view().await.unwrap().phase,
        HandoverPhase::Received
    );
}

#[tokio::test]
async fn a_wrong_code_changes_nothing_and_the_sixth_attempt_replaces_it() {
    let t = tablet().await;
    let pc = pc(&t.branch_id).await;
    let code = t.code().await;
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..HANDOVER_FAILURES_PER_CODE {
        let e = pc.puller.join(join(&t, wrong)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::PairCodeInvalid);
        assert!(t.serves().await);
    }
    let e = pc.puller.join(join(&t, &code)).await.unwrap_err();
    assert!(e.message.contains("new code"), "{}", e.message);
    let fresh = t.code().await;
    assert_ne!(fresh, code);
    assert!(t.serves().await);
    assert_eq!(
        pc.puller.join(join(&t, &fresh)).await.unwrap().phase,
        HandoverPhase::Received
    );
}

#[tokio::test]
async fn a_pc_of_another_branch_is_refused_and_the_tablet_can_resume() {
    let t = tablet().await;
    let pc = pc("01OTHERBRANCH").await;
    let code = t.code().await;
    let e = pc.puller.join(join(&t, &code)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::RestoreWrongBranch);
    assert!(pc.handover.view().await.is_none());
    // The claim stopped the tablet; it stays stopped until the manager cancels.
    assert!(!t.serves().await);
    let e = pc.puller.join(join(&t, &code)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::HubHandingOver);
    t.handover.cancel().await.unwrap();
    assert!(t.serves().await);
    assert!(t.status.borrow().handover.is_none());
}

#[tokio::test]
async fn an_address_nobody_answers_is_a_plain_message_and_nothing_stops() {
    let t = tablet().await;
    let pc = pc(&t.branch_id).await;
    let code = t.code().await;
    let e = pc
        .puller
        .join(HandoverJoinRequest {
            address: "127.0.0.1:9".into(),
            code,
        })
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NoHub);
    assert!(t.serves().await);
    let e = pc
        .puller
        .join(HandoverJoinRequest {
            address: "not a host".into(),
            code: "123456".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("address"));
}

// ---- Story 10.18: the PC imports what it pulled ----

#[derive(Default)]
struct MemKey(std::sync::Mutex<Option<Vec<u8>>>);

impl arena_tasks::Keystore for MemKey {
    fn put(&self, secret: &[u8]) -> std::io::Result<()> {
        *self.0.lock().unwrap() = Some(secret.to_vec());
        Ok(())
    }
    fn get(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().clone())
    }
}

type Seen = Arc<std::sync::Mutex<Vec<(String, String)>>>;

/// A cloud that answers every call with `reply` and remembers the request lines and bodies.
async fn cloud(status: u16, reply: &'static str) -> (u16, Seen) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                raw.extend_from_slice(&buf[..n]);
                if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..at]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    while raw.len() < at + 4 + len {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                    }
                    break;
                }
                if n == 0 {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&raw).into_owned();
            let line = text.lines().next().unwrap_or("").to_owned();
            let body = text
                .split_once("\r\n\r\n")
                .map(|(_, b)| b.to_owned())
                .unwrap_or_default();
            log.lock().unwrap().push((line, body));
            let head = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                reply.len()
            );
            let _ = sock.write_all(format!("{head}{reply}").as_bytes()).await;
        }
    });
    (port, seen)
}

struct Importing {
    _t: Tablet,
    pc: Pc,
    importer: arena_tasks::MigrationImporter,
    status: watch::Sender<HubStatus>,
    restarted: Arc<std::sync::atomic::AtomicBool>,
    seen: Seen,
}

impl Importing {
    fn db(&self) -> PathBuf {
        self.pc._dir.path().join("branch.db")
    }
}

/// A PC that pulled the snapshot, with a cloud that answers the claim with `status`/`reply`.
async fn importing(status_code: u16, reply: &'static str) -> Importing {
    let t = tablet().await;
    let (pc, status) = pc_with_status(&t.branch_id).await;
    let code = t.code().await;
    pc.puller.join(join(&t, &code)).await.unwrap();
    let (port, seen) = cloud(status_code, reply).await;
    let cloud = arena_tasks::CloudClient::new(
        &arena_tasks::CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
    )
    .unwrap();
    let restarted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&restarted);
    let token = Arc::new(MemKey::default());
    arena_tasks::Keystore::put(token.as_ref(), b"hub_x.y").unwrap();
    let importer = arena_tasks::MigrationImporter::new(
        Arc::clone(&pc.handover),
        status.clone(),
        cloud,
        token,
        "01JHUBBBBBBBBBBBBBBBBBBBBB".into(),
        pc._dir.path().join("branch.db"),
        pc.folder.clone(),
        Arc::new(move || flag.store(true, std::sync::atomic::Ordering::SeqCst)),
    );
    Importing {
        _t: t,
        pc,
        importer,
        status,
        restarted,
        seen,
    }
}

#[tokio::test]
async fn the_import_waits_for_the_owners_key_and_then_claims_stages_and_restarts() {
    use arena_core::handover::MigrationImportControl;
    let i = importing(200, r#"{"epoch":"9:01JHUBBBBBBBBBBBBBBBBBBBBB"}"#).await;

    // Without the branch key nothing is asked of the cloud and nothing is staged.
    let refused = i.importer.import().await.unwrap_err();
    assert_eq!(refused.code, ErrorCode::StepNotReady);
    assert!(i.seen.lock().unwrap().is_empty());
    assert!(!arena_store::has_pending(&i.db()));

    i.status.send_modify(|s| s.restore_key_released = true);
    let done = i.importer.import().await.unwrap();
    assert!(done.restarting);

    // The claim named the snapshot's epoch and last seq and this Hub's instance.
    let (line, body) = i.seen.lock().unwrap()[0].clone();
    assert!(line.starts_with("POST /v1/migration/claim"), "{line}");
    let sent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["instance_id"], "01JHUBBBBBBBBBBBBBBBBBBBBB");
    let (_, facts) = i.pc.handover.received().await.unwrap();
    assert_eq!(sent["fork"]["parent_epoch"], facts.epoch);
    assert_eq!(sent["fork"]["parent_seq"], facts.last_seq);

    // The snapshot is staged for the next start and the Hub is told to restart (after a beat).
    assert!(arena_store::has_pending(&i.db()));
    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
    assert!(i.restarted.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn a_fenced_claim_stages_nothing_and_says_the_branch_moved_on() {
    use arena_core::handover::MigrationImportControl;
    let i = importing(
        409,
        r#"{"code":"FENCED","message":"A newer Hub has taken over this branch (epoch 5:X)."}"#,
    )
    .await;
    i.status.send_modify(|s| s.restore_key_released = true);
    let e = i.importer.import().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Fenced);
    assert!(e.message.contains("moved on"), "{}", e.message);
    assert!(!arena_store::has_pending(&i.db()));
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    assert!(!i.restarted.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn a_pc_that_is_not_waiting_for_a_move_imports_nothing() {
    use arena_core::handover::MigrationImportControl;
    let i = importing(200, r#"{"epoch":"9:X"}"#).await;
    i.status.send_modify(|s| {
        s.restore_key_released = true;
        s.migration_wait = None;
    });
    let e = i.importer.import().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::StepNotReady);
    assert!(i.seen.lock().unwrap().is_empty());
}

/// A paired kiosk registered on the tablet and connected to it over pinned TLS.
async fn kiosk_on(t: &Tablet) -> arena_kiosk_core::HubClient {
    use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
    use arena_proto::{ClientClass, DeviceRegister, Hello, Versions};
    let secret = new_token_secret().unwrap();
    let applied = t
        .writer
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceRegister(DeviceRegister {
                device_id: None,
                device_kind: Default::default(),
                class: ClientClass::Paired,
                label: "PC 1".into(),
                token_hash: hash_token_secret(&secret).unwrap(),
                tv_number: None,
                group_id: None,
            }),
        ))
        .await
        .unwrap();
    let token = format_token(&applied.changes[0].row_id, &secret);
    arena_kiosk_core::HubClient::connect(
        &format!("wss://{}/", t.lan),
        &t.fingerprint,
        Hello {
            credential: token,
            class: ClientClass::Paired,
            versions: Versions {
                app: "0.1.0".into(),
            },
            capabilities: Vec::new(),
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn the_tablet_tells_each_kiosk_where_the_branch_goes_and_closes_it_at_the_acknowledgement() {
    use arena_proto::HubEvent;
    let t = tablet().await;
    let pc = pc(&t.branch_id).await;
    let kiosk = kiosk_on(&t).await;
    assert_eq!(
        kiosk.hello_ok().branch_id.as_deref(),
        Some(t.branch_id.as_str()),
        "the kiosk learns the branch at hello"
    );
    let mut events = kiosk.events().unwrap();
    let code = t.code().await;
    pc.puller.join(join(&t, &code)).await.unwrap();

    let signed = loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), events.next_event())
            .await
            .expect("the successor arrives")
        {
            Some(HubEvent::Successor(s)) => break s,
            Some(_) => {}
            None => panic!("the link closed before the successor"),
        }
    };
    assert_eq!(signed.successor.address, "192.168.1.20:7400");
    assert_eq!(signed.successor.fingerprint, "cd".repeat(32));
    assert_eq!(signed.successor.branch_id, t.branch_id);
    assert_eq!(
        signed.successor.expires_at,
        T0 + arena_proto::SUCCESSOR_TTL_MS
    );
    // It checks out for the kiosk that pins the tablet, and for nobody else.
    assert_eq!(
        arena_kiosk_core::successor::verify(&signed, &t.fingerprint, Some(&t.branch_id), T0),
        Ok(())
    );
    assert_eq!(
        arena_kiosk_core::successor::verify(&signed, &"ef".repeat(32), Some(&t.branch_id), T0),
        Err(arena_kiosk_core::successor::Rejection::Certificate)
    );

    // The acknowledgement has happened: the tablet ends the link with HUB_HANDING_OVER.
    let closed = tokio::time::timeout(std::time::Duration::from_secs(5), kiosk.closed())
        .await
        .expect("the tablet closes the link");
    assert_eq!(closed.error.unwrap().code, ErrorCode::HubHandingOver);
}

#[tokio::test]
async fn a_pc_that_names_no_address_leaves_the_kiosks_to_pair_again() {
    use arena_proto::HubEvent;
    let t = tablet().await;
    let (pc, status) = pc_with_status(&t.branch_id).await;
    status.send_modify(|s| s.lan_addresses.clear());
    let kiosk = kiosk_on(&t).await;
    let mut events = kiosk.events().unwrap();
    let code = t.code().await;
    pc.puller.join(join(&t, &code)).await.unwrap();
    // Still closed at the acknowledgement, but nothing was ever pushed.
    while let Some(ev) =
        tokio::time::timeout(std::time::Duration::from_secs(5), events.next_event())
            .await
            .expect("the link ends")
    {
        assert!(!matches!(ev, HubEvent::Successor(_)), "{ev:?}");
    }
}
