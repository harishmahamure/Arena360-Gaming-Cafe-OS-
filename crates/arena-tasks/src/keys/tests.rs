use super::*;
use crate::cloud::CloudEndpoint;
use crate::lease::LeaseTask;
use arena_core::key_backup::open_key_table;
use arena_core::{Clock, ManualClock, status_channel};
use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, LicenceClaims, dev_sign};
use arena_proto::{Actor, ActorKind, Intent, IntentPayload, PlayerErase, PlayerForm};
use arena_store::Store;
use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const T0: i64 = 1_800_000_000_000;
const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";
/// The epoch the fixture's lease gives its writer.
const EPOCH: &str = "1:01JAAAAAAAAAAAAAAAAAAAAAAA";
const KEK: [u8; KEK_LEN] = [9; KEK_LEN];

const NAME: &str = "Asha Rao";
const PHONE: &str = "+919876543210";
const EMAIL: &str = "asha@example.com";

struct Mem(Mutex<Option<Vec<u8>>>);

impl Keystore for Mem {
    fn put(&self, secret: &[u8]) -> std::io::Result<()> {
        *self.0.lock().unwrap() = Some(secret.to_vec());
        Ok(())
    }
    fn get(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().clone())
    }
}

struct TestKek;

impl Kek for TestKek {
    fn kek(&self) -> Result<[u8; KEK_LEN], arena_proto::AppError> {
        Ok(KEK)
    }
}

/// (request line, body bytes).
type Seen = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
type Handler = Box<dyn FnMut(&str, &[u8]) -> (u16, String) + Send>;

/// A one-request-per-connection HTTP server; `make` gets the port to build links to itself.
async fn serve(make: impl FnOnce(u16) -> Handler) -> (u16, Seen) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut handler = make(port);
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 65536];
            let (head_end, length) = loop {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                raw.extend_from_slice(&buf[..n]);
                if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..at]).into_owned();
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    break (at + 4, length);
                }
                if n == 0 {
                    break (raw.len(), 0);
                }
            };
            while raw.len() < head_end + length {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
            }
            let line = String::from_utf8_lossy(&raw)
                .lines()
                .next()
                .unwrap_or("")
                .to_owned();
            let body = raw.get(head_end..).unwrap_or(&[]).to_vec();
            log.lock().unwrap().push((line.clone(), body.clone()));
            let (status, reply) = handler(&line, &body);
            let head = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                reply.len()
            );
            let _ = sock.write_all(format!("{head}{reply}").as_bytes()).await;
        }
    });
    (port, seen)
}

/// A cloud that takes escrows, issues one key link and takes the PUT unless `put_fails`.
fn cloud(put_fails: Arc<AtomicBool>) -> impl FnOnce(u16) -> Handler {
    move |port| {
        Box::new(move |line, _| {
            if line.starts_with("POST /v1/kek/escrow") {
                (200, r#"{"stored":true}"#.into())
            } else if line.starts_with("POST /v1/presign") {
                (
                    200,
                    serde_json::json!({"url": format!("http://127.0.0.1:{port}/put/keys")})
                        .to_string(),
                )
            } else if line.starts_with("PUT ") && put_fails.load(Ordering::SeqCst) {
                (503, "{}".into())
            } else {
                (200, "{}".into())
            }
        })
    }
}

fn lines(seen: &Seen, prefix: &str) -> Vec<(String, Vec<u8>)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|(l, _)| l.starts_with(prefix))
        .cloned()
        .collect()
}

struct Fx {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    task: KeyBackupTask,
    writer: WriterHandle,
    identity: age::x25519::Identity,
}

async fn fx(port: u16, tier: Tier) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let store = Store::open(&path, T0).unwrap();
    let (status, _rx) = status_channel(HubStatus::opened(83, OWN));
    let manual = Arc::new(ManualClock::new(T0));
    let clock: Arc<dyn Clock> = manual;
    let lease = LeaseTask::start(store, OWN.into(), Arc::clone(&clock), status.clone())
        .await
        .unwrap();
    let writer = lease.writer().unwrap();
    std::mem::forget(lease);
    let claims = LicenceClaims::for_tier("T", "B", tier, T0 / 1000);
    writer
        .record_pairing(
            arena_store::HubIdentity {
                tenant_id: "T".into(),
                branch_id: "B".into(),
                hub_id: "H".into(),
                tier,
                paired_at: T0,
            },
            dev_sign(&claims).unwrap(),
            None,
        )
        .await
        .unwrap();
    status.send_modify(|s| s.tier = Some(tier));
    let cloud =
        CloudClient::new(&CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap())
            .unwrap();
    let keystore: Arc<dyn Keystore> = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
    let identity = age::x25519::Identity::generate();
    let task = KeyBackupTask::new(
        cloud,
        keystore,
        Arc::new(TestKek),
        Some(writer.clone()),
        Arc::new(SnapshotReader::open(&path).unwrap()),
        PairGate::new(true),
        status,
        DEV_LICENCE_VERIFY_KEYS.to_vec(),
        vec![identity.to_public().to_string()],
    );
    Fx {
        _dir: dir,
        path,
        task,
        writer,
        identity,
    }
}

fn intent_as(kind: ActorKind, payload: IntentPayload) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: T0,
        base_rev: None,
        actor: Actor {
            kind,
            id: "o".into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

fn intent(payload: IntentPayload) -> Intent {
    intent_as(ActorKind::Owner, payload)
}

async fn bootstrap(f: &Fx) {
    f.writer
        .submit_intent(intent_as(
            ActorKind::System,
            IntentPayload::BranchBootstrap(arena_proto::BranchBootstrap {
                name: "Arena".into(),
                branch_code: "ARN".into(),
                currency: "INR".into(),
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
                onboarding: None,
            }),
        ))
        .await
        .unwrap();
}

async fn add_player(f: &Fx, username: &str, phone: &str, name: &str, email: &str) -> String {
    let id = ulid::Ulid::generate().to_string();
    let form = PlayerForm {
        username: username.into(),
        password: "secret1".into(),
        phone: phone.into(),
        name: name.into(),
        email: email.into(),
        dob: "2000-01-02".into(),
        notes: String::new(),
        tags: Vec::new(),
        consent: true,
        policy_version: "1".into(),
    };
    let create = arena_core::players::seal(&KEK, &form, &id, "$argon2id$stub".into()).unwrap();
    f.writer
        .submit_intent(intent(IntentPayload::PlayerCreate(create)))
        .await
        .unwrap();
    id
}

async fn add_asha(f: &Fx) -> String {
    add_player(f, "asha", PHONE, NAME, EMAIL).await
}

async fn add_ravi(f: &Fx) -> String {
    add_player(f, "ravi", "+919876543211", "Ravi", "").await
}

async fn erase_player(f: &Fx, id: &str) {
    f.writer
        .submit_intent(intent(IntentPayload::PlayerErased(PlayerErase {
            player_id: id.into(),
            wallet: None,
            note: None,
        })))
        .await
        .unwrap();
}

fn state(f: &Fx) -> arena_store::BackupState {
    Store::open_reader(&f.path)
        .unwrap()
        .read_with(arena_store::read_backup_state)
        .unwrap()
}

fn unwrap_escrow(f: &Fx, body: &[u8]) -> Vec<u8> {
    let req: KekEscrowRequest = serde_json::from_slice(body).unwrap();
    let bytes = STANDARD.decode(req.wrapped_kek).unwrap();
    let d = age::Decryptor::new_buffered(&bytes[..]).unwrap();
    let mut r = d
        .decrypt(std::iter::once(&f.identity as &dyn age::Identity))
        .unwrap();
    let mut out = Vec::new();
    r.read_to_end(&mut out).unwrap();
    out
}

#[tokio::test]
async fn a_free_hub_escrows_its_branch_key_once_and_uploads_no_table() {
    let (port, seen) = serve(cloud(Arc::default())).await;
    let mut f = fx(port, Tier::Free).await;
    bootstrap(&f).await;
    add_asha(&f).await;
    f.task.cycle().await;
    let escrows = lines(&seen, "POST /v1/kek/escrow");
    assert_eq!(escrows.len(), 1);
    assert_eq!(
        unwrap_escrow(&f, &escrows[0].1),
        KEK,
        "only the escrow key opens it"
    );
    assert!(state(&f).kek_escrowed_at.is_some());

    // Nothing more is sent, and the table never leaves a Free Hub.
    f.task.cycle().await;
    add_ravi(&f).await;
    f.task.cycle().await;
    assert_eq!(lines(&seen, "POST /v1/kek/escrow").len(), 1);
    assert!(lines(&seen, "POST /v1/presign").is_empty());
    assert!(lines(&seen, "PUT ").is_empty());
    assert!(state(&f).keys_dirty(), "the table was never backed up");
}

#[tokio::test]
async fn a_paid_hub_backs_up_the_table_when_it_changes_and_an_erased_key_leaves_it() {
    let (port, seen) = serve(cloud(Arc::default())).await;
    let mut f = fx(port, Tier::Basic).await;
    bootstrap(&f).await;
    let first = add_asha(&f).await;
    f.task.cycle().await;
    let puts = lines(&seen, "PUT ");
    assert_eq!(puts.len(), 1);
    let presign: serde_json::Value =
        serde_json::from_slice(&lines(&seen, "POST /v1/presign")[0].1).unwrap();
    assert_eq!(presign["kind"], "keys");
    assert_eq!(presign["epoch"], EPOCH);
    let table = open_key_table(&KEK, &puts[0].1).unwrap();
    assert_eq!(
        table
            .iter()
            .map(|k| k.player_id.as_str())
            .collect::<Vec<_>>(),
        [first.as_str()]
    );
    let s = state(&f);
    assert!(!s.keys_dirty() && s.keys_backed_up_at.is_some() && s.keys_error.is_none());

    // Nothing changed: nothing is sent.
    f.task.cycle().await;
    assert_eq!(lines(&seen, "PUT ").len(), 1);

    // A second player, then an erasure: the one object is overwritten each time.
    add_ravi(&f).await;
    f.task.cycle().await;
    let puts = lines(&seen, "PUT ");
    assert_eq!(puts.len(), 2);
    assert_eq!(open_key_table(&KEK, &puts[1].1).unwrap().len(), 2);

    erase_player(&f, &first).await;
    f.task.cycle().await;
    let puts = lines(&seen, "PUT ");
    assert_eq!(puts.len(), 3);
    let table = open_key_table(&KEK, &puts[2].1).unwrap();
    assert_eq!(table.len(), 1);
    assert!(
        table.iter().all(|k| k.player_id != first),
        "the erased key is gone"
    );
    assert!(!state(&f).keys_dirty());
}

#[tokio::test]
async fn a_failed_backup_is_recorded_and_the_retry_sends_only_the_latest_table() {
    let put_fails = Arc::new(AtomicBool::new(true));
    let (port, seen) = serve(cloud(Arc::clone(&put_fails))).await;
    let mut f = fx(port, Tier::Basic).await;
    bootstrap(&f).await;
    add_asha(&f).await;
    let wait = f.task.cycle().await;
    assert!(wait <= clamp_interval(Tier::Basic.defaults().upload_interval_s));
    let s = state(&f);
    assert!(s.keys_dirty() && s.keys_backed_up_at.is_none());
    assert!(s.keys_error.is_some() && s.keys_error_at.is_some());

    // More changes while the WAN is down; connectivity returns.
    add_ravi(&f).await;
    put_fails.store(false, Ordering::SeqCst);
    f.task.cycle().await;
    let puts = lines(&seen, "PUT ");
    let last = puts.last().unwrap();
    assert_eq!(
        open_key_table(&KEK, &last.1).unwrap().len(),
        2,
        "the latest table"
    );
    let s = state(&f);
    assert!(!s.keys_dirty() && s.keys_error.is_none() && s.keys_backed_up_at.is_some());
}

#[tokio::test]
async fn a_new_branch_key_after_a_re_pair_is_escrowed_again() {
    let (port, seen) = serve(cloud(Arc::default())).await;
    let mut f = fx(port, Tier::Free).await;
    f.task.cycle().await;
    assert_eq!(lines(&seen, "POST /v1/kek/escrow").len(), 1);
    let claims = LicenceClaims::for_tier("T", "B", Tier::Free, T0 / 1000);
    assert!(
        f.writer
            .record_repairing(
                arena_store::HubIdentity {
                    tenant_id: "T".into(),
                    branch_id: "B".into(),
                    hub_id: "H2".into(),
                    tier: Tier::Free,
                    paired_at: T0,
                },
                dev_sign(&claims).unwrap(),
            )
            .await
            .unwrap()
    );
    assert_eq!(state(&f).kek_escrowed_at, None);
    f.task.cycle().await;
    assert_eq!(lines(&seen, "POST /v1/kek/escrow").len(), 2);
}

#[tokio::test]
async fn a_build_without_an_escrow_key_sends_nothing() {
    let (port, seen) = serve(cloud(Arc::default())).await;
    let mut f = fx(port, Tier::Free).await;
    f.task.recipients.clear();
    f.task.cycle().await;
    assert!(lines(&seen, "POST ").is_empty());
    assert_eq!(state(&f).kek_escrowed_at, None);
}

#[test]
fn an_escrow_recipient_that_is_not_a_public_key_is_refused_without_echoing_it() {
    let private = "AGE-SECRET-KEY-1QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ";
    let e = wrap_kek(&KEK, &[private.to_owned()]).unwrap_err();
    assert!(!e.contains("SECRET"));
    assert!(wrap_kek(&KEK, &[]).is_err());
}

/// Collects the log lines written to it.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Story 9.2's privacy test: after creating and erasing a player, no change-log batch, key backup
/// object, escrow body or log line holds the name, phone or email.
#[tokio::test]
async fn no_plaintext_reaches_a_batch_a_key_backup_the_escrow_or_the_log() {
    let log = captured_log();
    let (port, seen) = serve(cloud(Arc::default())).await;
    let mut f = fx(port, Tier::Basic).await;
    bootstrap(&f).await;
    let id = add_asha(&f).await;
    f.task.cycle().await;
    erase_player(&f, &id).await;
    f.task.cycle().await;

    let mut haystacks: Vec<(String, Vec<u8>)> = lines(&seen, "PUT ")
        .into_iter()
        .map(|(_, b)| ("key backup".to_owned(), b))
        .chain(
            lines(&seen, "POST ")
                .into_iter()
                .map(|(_, b)| ("request body".to_owned(), b)),
        )
        .collect();
    // The change-log batch the uploader would send for these intents.
    let snapshots = SnapshotReader::open(&f.path).unwrap();
    let arena_store::RangeRead::Batch(batch) = snapshots
        .range_batch(EPOCH, 1, i64::MAX / 2, 5_000)
        .unwrap()
    else {
        panic!("the log holds rows");
    };
    let encoded = arena_store::encode_batch(83, &batch).unwrap();
    haystacks.push(("batch".into(), zstd::decode_all(&encoded[..]).unwrap()));
    haystacks.push(("log".into(), log.0.lock().unwrap().clone()));
    assert!(
        haystacks.iter().all(|(_, b)| !b.is_empty()),
        "every place was filled: {:?}",
        haystacks
            .iter()
            .map(|(p, b)| (p.clone(), b.len()))
            .collect::<Vec<_>>()
    );
    for (place, bytes) in &haystacks {
        let text = String::from_utf8_lossy(bytes);
        for plain in [NAME, PHONE, "9876543210", EMAIL, "asha@"] {
            assert!(!text.contains(plain), "{plain:?} found in the {place}");
        }
    }
    assert!(state(&f).keys_backed_up_at.is_some());
}

/// Every log line of every test in this process, from one global subscriber installed once: a
/// per-thread subscriber can lose events to the callsite cache while other tests run.
fn captured_log() -> Capture {
    static LOG: std::sync::OnceLock<Capture> = std::sync::OnceLock::new();
    let log = LOG.get_or_init(Capture::default);
    let _ = tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish(),
    );
    log.clone()
}

#[tokio::test]
async fn a_player_change_wakes_the_backup_before_the_upload_interval() {
    let (port, seen) = serve(cloud(Arc::default())).await;
    let f = fx(port, Tier::Basic).await;
    bootstrap(&f).await;
    let writer = f.writer.clone();
    let job = tokio::spawn(f.task.with_min_gap(Duration::from_millis(100)).run());
    let wait_for_puts = |n: usize| {
        let seen = Arc::clone(&seen);
        async move {
            tokio::time::timeout(Duration::from_secs(10), async {
                while lines(&seen, "PUT ").len() < n {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("the backup ran")
        }
    };
    // The first cycle backs up the table as it is (always due once), then sleeps a full interval.
    wait_for_puts(1).await;
    let form = PlayerForm {
        username: "asha".into(),
        password: "secret1".into(),
        phone: PHONE.into(),
        name: NAME.into(),
        email: EMAIL.into(),
        dob: String::new(),
        notes: String::new(),
        tags: Vec::new(),
        consent: true,
        policy_version: "1".into(),
    };
    let id = ulid::Ulid::generate().to_string();
    let create = arena_core::players::seal(&KEK, &form, &id, "$argon2id$stub".into()).unwrap();
    writer
        .submit_intent(intent(IntentPayload::PlayerCreate(create)))
        .await
        .unwrap();
    wait_for_puts(2).await;
    let puts = lines(&seen, "PUT ");
    assert_eq!(open_key_table(&KEK, &puts[1].1).unwrap().len(), 1);
    job.abort();
}
