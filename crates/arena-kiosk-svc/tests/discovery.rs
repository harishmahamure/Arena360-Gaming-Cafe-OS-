use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arena_api::lan::{HubCert, HubInfoSource, LanHub, bind_lan, serve_lan};
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, Role, SnapshotReader, SystemClock, Writer,
    status_channel,
};
use arena_kiosk_svc::discovery::{
    Browse, BrowsedService, Browser, Discovery, MdnsBrowser, NetProber, probe_ip,
};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ErrorCode, HubInfo, Intent, IntentPayload, KioskHubState,
    PROTOCOL_VERSION,
};
use arena_store::{LeaseRow, Store, write_lease};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

fn now_ms() -> i64 {
    arena_core::Clock::now_ms(&SystemClock)
}

struct RealHub {
    _dir: tempfile::TempDir,
    web: SocketAddr,
    fingerprint: String,
    _stop: oneshot::Sender<()>,
    _status: tokio::sync::watch::Sender<HubStatus>,
}

async fn real_hub(claimed_fingerprint: Option<String>) -> RealHub {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, now_ms()).unwrap();
    let lease = LeaseRow {
        counter: 1,
        hub_instance_id: HUB_ID.into(),
        provisional: true,
        renewed_at: now_ms(),
        expires_at: i64::MAX,
    };
    store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
    let mut initial = HubStatus::opened(store.schema_version().unwrap(), HUB_ID);
    initial.role = Role::Primary;
    initial.lease = Some(LeaseInfo::from(&lease));
    let cert = HubCert::generate(None).unwrap();
    initial.tls_fingerprint = Some(claimed_fingerprint.unwrap_or_else(|| cert.fingerprint.clone()));
    let (status_tx, status) = status_channel(initial);
    let writer = Writer::spawn(store, Arc::new(SystemClock), status.clone()).unwrap();
    let payload = IntentPayload::BranchBootstrap(BranchBootstrap {
        name: "Koramangala".into(),
        branch_code: "KOR01".into(),
        currency: "INR".into(),
        tz: "Asia/Kolkata".into(),
        day_start_min: 360,
        fy_start_month: 4,
    });
    writer
        .submit_intent(Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now_ms(),
            base_rev: None,
            actor: Actor {
                kind: ActorKind::System,
                id: "test".into(),
            },
            elevated_by: None,
            reason: None,
            payload,
        })
        .await
        .unwrap();
    let hub = Arc::new(
        LanHub::new(
            Some(writer),
            status,
            Arc::new(SystemClock),
            Arc::new(DeviceDirectory::open(&path).unwrap()),
            cert.server_config().unwrap(),
            env!("CARGO_PKG_VERSION"),
        )
        .with_info(HubInfoSource {
            reader: Arc::new(SnapshotReader::open(&path).unwrap()),
            hub_name: "front-desk".into(),
            lan_port: 7400,
            web_port: 7401,
        }),
    );
    let listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let web = listeners.web_addr().unwrap();
    let (stop, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, hub, async {
        let _ = stop_rx.await;
    }));
    RealHub {
        _dir: dir,
        web,
        fingerprint: cert.fingerprint,
        _stop: stop,
        _status: status_tx,
    }
}

async fn fake_hub(info: serde_json::Value) -> SocketAddr {
    fake_hub_with_cert(|_| info).await
}

async fn fake_hub_with_cert(make: impl FnOnce(&str) -> serde_json::Value) -> SocketAddr {
    let cert = HubCert::generate(None).unwrap();
    let info = make(&cert.fingerprint);
    let acceptor = tokio_rustls::TlsAcceptor::from(cert.server_config().unwrap());
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = info.to_string();
    tokio::spawn(async move {
        loop {
            let Ok((mut tcp, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            let body = body.clone();
            tokio::spawn(async move {
                let mut first = [0u8; 1];
                if tcp.peek(&mut first).await.is_err() {
                    return;
                }
                if first[0] == 0x16 {
                    let _ = acceptor.accept(tcp).await;
                    return;
                }
                let mut buf = [0u8; 2048];
                let _ = tcp.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tcp.write_all(resp.as_bytes()).await;
                let _ = tcp.shutdown().await;
            });
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_typed_ip_of_a_real_hub_is_found_with_its_branch_and_fingerprint() {
    let hub = real_hub(None).await;
    let found = probe_ip(&NetProber, "127.0.0.1", hub.web.port())
        .await
        .unwrap();
    assert_eq!(found.addr, "127.0.0.1:7400");
    assert_eq!(found.branch_name, "Koramangala");
    assert_eq!(found.hub_name, "front-desk");
    assert_eq!(found.fingerprint, hub.fingerprint);
    assert_eq!(found.protocol_version, PROTOCOL_VERSION);
    assert!(found.compatible);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hub_info_fingerprint_that_is_not_the_certificate_is_refused() {
    let hub = real_hub(Some("ee".repeat(32))).await;
    let err = probe_ip(&NetProber, "127.0.0.1", hub.web.port())
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NoHub);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http_server_that_is_not_a_hub_is_no_hub() {
    let addr = fake_hub(serde_json::json!({"hello": "world"})).await;
    let err = probe_ip(&NetProber, "127.0.0.1", addr.port())
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NoHub);
    let addr = fake_hub_with_cert(|fp| {
        serde_json::to_value(HubInfo {
            product: "other-product".into(),
            ..HubInfo::new("B", "H", fp, 7400, 7401)
        })
        .unwrap()
    })
    .await;
    let err = probe_ip(&NetProber, "127.0.0.1", addr.port())
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NoHub);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hub_on_another_protocol_version_is_incompatible() {
    let addr = fake_hub_with_cert(|fp| {
        serde_json::to_value(HubInfo {
            protocol_version: PROTOCOL_VERSION + 1,
            ..HubInfo::new("B", "H", fp, 7400, 7401)
        })
        .unwrap()
    })
    .await;
    let err = probe_ip(&NetProber, "127.0.0.1", addr.port())
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::HubIncompatible);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_listening_is_no_hub_within_about_two_seconds() {
    let port = {
        let l = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        l.local_addr().unwrap().port()
    };
    let started = Instant::now();
    let err = probe_ip(&NetProber, "127.0.0.1", port).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NoHub);
    assert!(started.elapsed() < Duration::from_millis(2500));
    let started = Instant::now();
    let err = probe_ip(&NetProber, "192.0.2.1", port).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NoHub);
    assert!(started.elapsed() < Duration::from_millis(2500));
}

struct ScriptedBrowser {
    rounds: Mutex<Vec<Vec<BrowsedService>>>,
    started: Arc<std::sync::atomic::AtomicUsize>,
    stopped: Arc<std::sync::atomic::AtomicUsize>,
}

impl Browser for ScriptedBrowser {
    fn browse(&self) -> Result<Browse, String> {
        let (tx, rx) = mpsc::channel(8);
        self.started
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut rounds = self.rounds.lock().unwrap();
        if !rounds.is_empty() {
            for svc in rounds.remove(0) {
                tx.try_send(svc).unwrap();
            }
        }
        let stopped = Arc::clone(&self.stopped);
        Ok(Browse::new(rx, move || {
            drop(tx);
            stopped.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }))
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unpaired_kiosk_discovers_probes_and_selects_without_pairing() {
    use arena_kiosk_core::ipc::{IpcMsg, read_msg, write_msg};
    use arena_kiosk_svc::paths::DataDir;
    use arena_kiosk_svc::{Runtime, Timings};

    let hub = real_hub(None).await;
    let other_fp = "cd".repeat(32);
    let txt: HashMap<String, String> = arena_proto::txt_records(
        "Koramangala",
        "front-desk",
        PROTOCOL_VERSION,
        &other_fp,
        hub.web.port(),
    )
    .into_iter()
    .collect();
    let spoof = BrowsedService {
        addrs: vec![IpAddr::from([127, 0, 0, 1])],
        port: 7400,
        txt,
    };
    let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stopped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let browser = ScriptedBrowser {
        rounds: Mutex::new(vec![vec![spoof.clone()], vec![spoof.clone(), spoof]]),
        started: Arc::clone(&started),
        stopped: Arc::clone(&stopped),
    };
    let dir = tempfile::tempdir().unwrap();
    let data = DataDir(dir.path().join("kiosk"));
    std::fs::create_dir_all(data.root()).unwrap();
    let ipc = arena_kiosk_svc::ipc::UnixIpcServer::bind(&data.socket()).unwrap();
    let discovery = Discovery::new(Arc::new(browser), Arc::new(NetProber))
        .with_window(Duration::from_millis(600))
        .with_web_port(hub.web.port());
    let runtime = Runtime::start_with_discovery(
        data.clone(),
        arena_kiosk_svc::secret::platform_store(data.token()),
        Arc::new(arena_kiosk_svc::clock::SystemClock),
        Timings::default(),
        ipc,
        Arc::new(arena_kiosk_svc::launcher::NoopLauncher),
        discovery,
    )
    .unwrap();
    let mut state = runtime.hub_state();
    tokio::time::timeout(
        Duration::from_secs(5),
        state.wait_for(|s| *s == KioskHubState::Unpaired),
    )
    .await
    .unwrap()
    .unwrap();

    let stream = tokio::net::UnixStream::connect(data.socket())
        .await
        .unwrap();
    let (r, mut w) = stream.into_split();
    let mut r = tokio::io::BufReader::new(r);
    let mut next = async |want: &dyn Fn(&IpcMsg) -> bool| -> IpcMsg {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), read_msg(&mut r))
                .await
                .unwrap()
                .unwrap();
            if want(&msg) {
                return msg;
            }
        }
    };
    assert_eq!(
        next(&|m| matches!(m, IpcMsg::HubStatus { .. })).await,
        IpcMsg::HubStatus {
            state: KioskHubState::Unpaired
        }
    );
    let request = |rid: &str, kind: &str, body: serde_json::Value| IpcMsg::Request {
        rid: rid.into(),
        kind: kind.into(),
        body,
    };
    write_msg(&mut w, &request("d1", "discover", serde_json::Value::Null))
        .await
        .unwrap();
    write_msg(&mut w, &request("d2", "discover", serde_json::Value::Null))
        .await
        .unwrap();
    let mut seen = Vec::new();
    let done = next(&|m| matches!(m, IpcMsg::DiscoveryDone { search: 2, .. })).await;
    let IpcMsg::DiscoveryDone { hubs, .. } = done else {
        unreachable!()
    };
    assert_eq!(hubs.len(), 1, "deduplicated by fingerprint");
    assert_eq!(hubs[0].addr, "127.0.0.1:7400");
    assert_eq!(hubs[0].web_port, hub.web.port());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while stopped.load(std::sync::atomic::Ordering::SeqCst)
        < started.load(std::sync::atomic::Ordering::SeqCst)
        && std::time::Instant::now() < deadline
    {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let ran = started.load(std::sync::atomic::Ordering::SeqCst);
    assert!((1..=2).contains(&ran), "{ran} browses started");
    assert_eq!(
        stopped.load(std::sync::atomic::Ordering::SeqCst),
        ran,
        "every started browse was stopped"
    );
    tokio::time::sleep(Duration::from_millis(800)).await;
    write_msg(&mut w, &request("x", "hub_status", serde_json::Value::Null))
        .await
        .unwrap();
    loop {
        let msg = next(&|_| true).await;
        if matches!(&msg, IpcMsg::Reply { rid, .. } if rid == "x") {
            break;
        }
        seen.push(msg);
    }
    assert!(
        !seen
            .iter()
            .any(|m| matches!(m, IpcMsg::DiscoveryDone { search: 1, .. })),
        "no done for the aborted search: {seen:?}"
    );

    write_msg(
        &mut w,
        &request(
            "s0",
            "select_hub",
            serde_json::json!({"addr": "127.0.0.1:7400", "fingerprint": other_fp}),
        ),
    )
    .await
    .unwrap();
    let IpcMsg::Reply { error, .. } =
        next(&|m| matches!(m, IpcMsg::Reply { rid, .. } if rid == "s0")).await
    else {
        unreachable!()
    };
    assert_eq!(
        error.unwrap().code,
        ErrorCode::NoHub,
        "an advert whose fingerprint is not the listener's certificate is refused"
    );
    assert!(!data.selected_hub().exists());

    write_msg(
        &mut w,
        &request("p1", "probe_ip", serde_json::json!({"ip": "999.1.1.1"})),
    )
    .await
    .unwrap();
    let IpcMsg::Reply { error, .. } =
        next(&|m| matches!(m, IpcMsg::Reply { rid, .. } if rid == "p1")).await
    else {
        unreachable!()
    };
    assert_eq!(error.unwrap().code, ErrorCode::NoHub);

    write_msg(
        &mut w,
        &request("p2", "probe_ip", serde_json::json!({"ip": "127.0.0.1"})),
    )
    .await
    .unwrap();
    let IpcMsg::Reply { body, error, .. } =
        next(&|m| matches!(m, IpcMsg::Reply { rid, .. } if rid == "p2")).await
    else {
        unreachable!()
    };
    assert_eq!(error, None);
    assert_eq!(body["hub"]["fingerprint"], hub.fingerprint);
    assert_eq!(body["hub"]["branch_name"], "Koramangala");

    write_msg(
        &mut w,
        &request(
            "s1",
            "select_hub",
            serde_json::json!({"addr": "127.0.0.1:7400", "fingerprint": hub.fingerprint}),
        ),
    )
    .await
    .unwrap();
    let IpcMsg::Reply { body, error, .. } =
        next(&|m| matches!(m, IpcMsg::Reply { rid, .. } if rid == "s1")).await
    else {
        unreachable!()
    };
    assert_eq!(error, None);
    assert_eq!(body["branch_name"], "Koramangala");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(data.selected_hub()).unwrap()).unwrap();
    assert_eq!(
        saved,
        serde_json::json!({
            "addr": "127.0.0.1:7400",
            "fingerprint": hub.fingerprint,
            "branch_name": "Koramangala",
            "web_port": hub.web.port(),
        })
    );
    assert!(!data.config().exists(), "no config.json");
    assert!(!data.token().exists(), "no device token");
    assert_eq!(*runtime.hub_state().borrow(), KioskHubState::Unpaired);

    write_msg(&mut w, &request("d3", "discover", serde_json::Value::Null))
        .await
        .unwrap();
    let IpcMsg::Reply { body, .. } =
        next(&|m| matches!(m, IpcMsg::Reply { rid, .. } if rid == "d3")).await
    else {
        unreachable!()
    };
    assert_eq!(body["search"], 3);
    assert!(
        !data.selected_hub().exists(),
        "a new search clears the old choice"
    );
    runtime.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_mdns_advert_is_browsed_back_or_skipped() {
    use arena_tasks::{Advert, Advertiser, MdnsAdvertiser};
    let fp = "9a".repeat(32);
    let name = format!("arena-test-{}", ulid::Ulid::generate());
    let mut advertiser = MdnsAdvertiser::default();
    if let Err(e) = advertiser.register(&Advert::new(&name, 7400, 7401, "Round Trip", &fp)) {
        println!("skipped: multicast unavailable ({e})");
        return;
    }
    let browse = MdnsBrowser.browse();
    let mut browse = match browse {
        Ok(b) => b,
        Err(e) => {
            println!("skipped: mdns browse unavailable ({e})");
            advertiser.shutdown();
            return;
        }
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut seen = false;
    while let Ok(Some(svc)) = tokio::time::timeout_at(deadline, browse.rx.recv()).await {
        if svc.txt.get("fp") == Some(&fp) {
            assert_eq!(
                svc.txt.get("branch").map(String::as_str),
                Some("Round Trip")
            );
            assert_eq!(svc.port, 7400);
            seen = true;
            break;
        }
    }
    drop(browse);
    let _ = advertiser.unregister();
    advertiser.shutdown();
    if !seen {
        println!("skipped: no multicast round trip on this machine within 8 s");
    }
}
