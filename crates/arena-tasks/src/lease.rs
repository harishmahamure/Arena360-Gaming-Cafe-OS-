//! The writer lease: decides at start whether this Hub is primary or follower, renews the
//! lease while primary and demotes on supersession.
//!
//! Epochs are `(counter, hub_instance_id)`, compared lexicographically. No cloud is contacted
//! to take a lease: a Hub that finds none mints a provisional `(max seen + 1, own_id)`, and one that
//! takes another instance's expired row mints `max(row counter, max seen) + 1` (Story 9.3, the
//! highest counter seen is `lease.max_seen_counter`). The first heartbeat confirms the provisional
//! epoch or answers with the higher fence. A follower never
//! takes over on its own; promotion is a manual step (later story).
//!
//! Beyond the table row, the process-level writer lock (`<db>.lock`, see
//! [`Store::try_lock_writer`]) guards against two processes on one data dir: they share one
//! `hub_instance_id`, so the row alone cannot tell them apart. A process that cannot take the
//! lock is a follower and never writes the lease.

use std::sync::Arc;
use std::time::Duration;

use arena_core::{Clock, Epoch, HubStatus, LeaseInfo, LeaseRenewal, Role, Writer, WriterHandle};
use arena_proto::{AppError, ErrorCode};
use arena_store::{LeaseRow, Store, read_lease, read_max_seen_counter, write_lease};
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior};

/// How often the primary renews its lease.
pub const RENEW_INTERVAL: Duration = Duration::from_secs(3);
/// How long a lease stays valid after each renewal.
pub const LEASE_TTL_MS: i64 = 15_000;

/// What one renewal tick did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tick {
    /// The lease was renewed.
    Renewed,
    /// A higher epoch holds the lease; the writer is closed and this Hub is now a follower.
    Demoted,
    /// This Hub is a follower; nothing to do.
    Idle,
}

#[derive(Debug)]
enum Decision {
    Take(LeaseRow),
    Follow(Option<LeaseRow>),
}

/// The start-up rule for a process that holds the writer lock.
fn decide(existing: Option<LeaseRow>, max_seen: i64, own_id: &str, now: i64) -> Decision {
    let expires_at = now + LEASE_TTL_MS;
    match existing {
        None => Decision::Take(LeaseRow {
            counter: max_seen.max(0) + 1,
            hub_instance_id: own_id.to_owned(),
            provisional: true,
            renewed_at: now,
            expires_at,
        }),
        // Our own row (a restart): renew it, keeping the counter.
        Some(row) if row.hub_instance_id == own_id => Decision::Take(LeaseRow {
            renewed_at: now,
            expires_at,
            ..row
        }),
        // Someone else's expired row: take it with a strictly higher epoch, so the old holder
        // demotes if it ever wakes up and re-reads the row.
        Some(row) if row.expires_at <= now => Decision::Take(LeaseRow {
            counter: row.counter.max(max_seen) + 1,
            hub_instance_id: own_id.to_owned(),
            provisional: true,
            renewed_at: now,
            expires_at,
        }),
        // A fresh row held by another instance: follow, never write.
        Some(row) => Decision::Follow(Some(row)),
    }
}

/// The highest epoch this Hub has been told of by somebody else (Story 9.22): the new main Hub's
/// `epoch.changed` announcement, its mDNS record or a device that has seen it. The lease task reads
/// it at every tick, so an old main Hub that is still running stops writing within one renewal
/// once a higher epoch exists, without waiting for the cloud.
#[derive(Debug, Clone, Default)]
pub struct SeenEpochs(Arc<std::sync::Mutex<Option<Epoch>>>);

impl SeenEpochs {
    /// Remembers `epoch` when it is higher than the one held.
    pub fn note(&self, epoch: Epoch) {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.as_ref().is_none_or(|h| epoch > *h) {
            *held = Some(epoch);
        }
    }

    /// Remembers an epoch written `<counter>:<instance>`; text that is not one changes nothing.
    pub fn note_text(&self, epoch: &str) -> bool {
        match arena_proto::epoch_parts(epoch) {
            Some((counter, instance)) => {
                self.note(Epoch {
                    counter,
                    hub_instance_id: instance.to_owned(),
                });
                true
            }
            None => false,
        }
    }

    fn get(&self) -> Option<Epoch> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// The lease task for one Hub process.
pub struct LeaseTask {
    clock: Arc<dyn Clock>,
    status: watch::Sender<HubStatus>,
    writer: Option<WriterHandle>,
    held: Option<LeaseRow>,
    /// Higher epochs other Hubs announced (Story 9.22).
    seen: SeenEpochs,
    /// Test hook: make every renewal job fail.
    #[cfg(test)]
    fail_renewals: bool,
}

impl LeaseTask {
    /// Decides the role at start. When primary, `store` becomes the writer's connection;
    /// otherwise it is closed without writing.
    pub async fn start(
        store: Store,
        instance_id: String,
        clock: Arc<dyn Clock>,
        status: watch::Sender<HubStatus>,
    ) -> Result<Self, AppError> {
        let job_clock = Arc::clone(&clock);
        let (store, decision) = tokio::task::spawn_blocking(move || {
            let mut store = store;
            // The writer lock is taken by `Store::open`; without it this process never writes.
            if !store.holds_writer_lock() {
                let row = store.lease()?;
                return Ok((store, Decision::Follow(row)));
            }
            let decision = store.write_tx(|tx| {
                let now = job_clock.now_ms();
                let decision = decide(
                    read_lease(tx)?,
                    read_max_seen_counter(tx)?,
                    &instance_id,
                    now,
                );
                if let Decision::Take(row) = &decision {
                    write_lease(tx, row)?;
                }
                Ok(decision)
            })?;
            Ok::<_, AppError>((store, decision))
        })
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("lease start: {e}")))??;

        let mut task = Self {
            clock,
            status,
            writer: None,
            held: None,
            seen: SeenEpochs::default(),
            #[cfg(test)]
            fail_renewals: false,
        };
        match decision {
            Decision::Take(row) => {
                task.writer = Some(Writer::spawn(
                    store,
                    Arc::clone(&task.clock),
                    task.status.subscribe(),
                )?);
                tracing::info!(
                    counter = row.counter,
                    hub_instance_id = %row.hub_instance_id,
                    provisional = row.provisional,
                    expires_at = row.expires_at,
                    "lease taken; primary"
                );
                task.publish(Role::Primary, Some(&row));
                task.held = Some(row);
            }
            Decision::Follow(row) => {
                drop(store);
                tracing::info!(
                    counter = row.as_ref().map(|r| r.counter),
                    hub_instance_id = row.as_ref().map(|r| r.hub_instance_id.as_str()),
                    "lease held elsewhere; follower"
                );
                task.publish(Role::Follower, row.as_ref());
            }
        }
        Ok(task)
    }

    fn publish(&self, role: Role, lease: Option<&LeaseRow>) {
        self.status.send_modify(|s| {
            s.role = role;
            s.lease = lease.map(LeaseInfo::from);
        });
    }

    /// Where other parts of the Hub (the LAN listener, mDNS) report a higher epoch they have seen.
    pub fn seen_epochs(&self) -> SeenEpochs {
        self.seen.clone()
    }

    /// The writer handle while primary.
    pub fn writer(&self) -> Option<WriterHandle> {
        self.writer.clone()
    }

    /// The current role.
    pub fn role(&self) -> Role {
        if self.writer.is_some() {
            Role::Primary
        } else {
            Role::Follower
        }
    }

    /// One renewal: re-reads the row inside the renewing transaction and either renews it or,
    /// when the row is missing or holds any other epoch, closes the writer and becomes a
    /// follower. A primary whose renewals keep failing demotes once its own lease has expired.
    pub async fn tick(&mut self) -> Result<Tick, AppError> {
        let (Some(writer), Some(held)) = (self.writer.clone(), self.held.clone()) else {
            return Ok(Tick::Idle);
        };
        // A higher epoch announced on the LAN ends this Hub's writing at once: the new main Hub
        // owns the branch, whatever this Hub's own lease row still says. The counter is kept so a
        // later start mints above it.
        if let Some(seen) = self.seen.get()
            && seen > LeaseInfo::from(&held).epoch()
        {
            if let Err(e) = writer.note_epoch_seen(seen.counter).await {
                tracing::warn!(code = e.code.as_str(), "could not keep the epoch seen");
            }
            let now = self.clock.now_ms();
            let row = LeaseRow {
                counter: seen.counter,
                hub_instance_id: seen.hub_instance_id,
                provisional: false,
                renewed_at: now,
                expires_at: now + LEASE_TTL_MS,
            };
            self.demote(Some(row), "a higher epoch was announced").await;
            return Ok(Tick::Demoted);
        }
        let held_expires_at = held.expires_at;
        #[cfg(test)]
        let fail = self.fail_renewals;
        #[cfg(not(test))]
        let fail = false;
        let result = if fail {
            Err(AppError::new(
                ErrorCode::Internal,
                "injected renewal failure",
            ))
        } else {
            writer
                .renew_lease(held, LEASE_TTL_MS, Arc::clone(&self.clock))
                .await
        };
        match result {
            Ok(LeaseRenewal::Renewed(row)) => {
                self.publish(Role::Primary, Some(&row));
                self.held = Some(row);
                Ok(Tick::Renewed)
            }
            Ok(LeaseRenewal::Superseded(row)) => {
                self.demote(row, "lease superseded").await;
                Ok(Tick::Demoted)
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                let held = self.held.clone();
                self.demote(held, "writer closed").await;
                Ok(Tick::Demoted)
            }
            Err(e) if !writer.is_alive() => {
                tracing::error!(code = e.code.as_str(), "writer thread stopped");
                let held = self.held.clone();
                self.demote(held, "writer thread stopped").await;
                self.status.send_modify(|s| s.db_open = false);
                Ok(Tick::Demoted)
            }
            Err(e) if self.clock.now_ms() >= held_expires_at => {
                tracing::warn!(code = e.code.as_str(), "lease renewal failed");
                let held = self.held.clone();
                self.demote(held, "own lease expired").await;
                Ok(Tick::Demoted)
            }
            Err(e) => Err(e),
        }
    }

    async fn demote(&mut self, seen: Option<LeaseRow>, reason: &'static str) {
        if let Some(writer) = self.writer.take() {
            writer.close().await;
        }
        self.held = None;
        tracing::warn!(
            reason,
            counter = seen.as_ref().map(|r| r.counter),
            hub_instance_id = seen.as_ref().map(|r| r.hub_instance_id.as_str()),
            "demoted; writer closed; follower"
        );
        self.publish(Role::Follower, seen.as_ref());
    }

    /// Shuts the task down for a one-shot run (`dev seed`): while primary, expires the lease
    /// now through the writer (only if the row is still ours) so another Hub can start at once,
    /// then closes the writer. A follower has nothing to release.
    pub async fn release(mut self) -> Result<(), AppError> {
        let (Some(writer), Some(held)) = (self.writer.take(), self.held.take()) else {
            return Ok(());
        };
        let result = writer.renew_lease(held, 0, Arc::clone(&self.clock)).await;
        writer.close().await;
        self.publish(Role::Follower, None);
        result.map(|_| ())
    }

    /// Renews every [`RENEW_INTERVAL`] until demoted. Returns at once for a follower.
    pub async fn run(mut self) {
        let mut interval =
            tokio::time::interval_at(Instant::now() + RENEW_INTERVAL, RENEW_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        while self.writer.is_some() {
            interval.tick().await;
            match self.tick().await {
                Ok(Tick::Renewed) => {}
                Ok(Tick::Demoted | Tick::Idle) => break,
                Err(e) => tracing::warn!(code = e.code.as_str(), "lease renewal failed"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::{ManualClock, status_channel};
    use std::path::{Path, PathBuf};

    const T0: i64 = 1_800_000_000_000;
    const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";
    const OTHER: &str = "ZZZZZZZZZZZZZZZZZZZZZZZZZZ";

    struct Fixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
        clock: Arc<ManualClock>,
        tx: watch::Sender<HubStatus>,
        rx: watch::Receiver<HubStatus>,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, T0).unwrap();
        let (tx, rx) = status_channel(HubStatus::opened(store.schema_version().unwrap(), OWN));
        Fixture {
            _dir: dir,
            path,
            clock: Arc::new(ManualClock::new(T0)),
            tx,
            rx,
        }
    }

    fn seed(path: &Path, row: &LeaseRow) {
        let mut s = Store::open(path, T0).unwrap();
        s.write_tx(|tx| write_lease(tx, row)).unwrap();
    }

    fn row_in(path: &Path) -> Option<LeaseRow> {
        Store::open(path, T0).unwrap().lease().unwrap()
    }

    async fn start(f: &Fixture) -> LeaseTask {
        LeaseTask::start(
            Store::open(&f.path, T0).unwrap(),
            OWN.into(),
            f.clock.clone(),
            f.tx.clone(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn empty_lease_mints_provisional_epoch_and_becomes_primary() {
        let f = fixture();
        let task = start(&f).await;
        assert_eq!(task.role(), Role::Primary);
        assert!(task.writer().is_some());
        let row = row_in(&f.path).unwrap();
        assert_eq!(
            row,
            LeaseRow {
                counter: 1,
                hub_instance_id: OWN.into(),
                provisional: true,
                renewed_at: T0,
                expires_at: T0 + LEASE_TTL_MS,
            }
        );
        let s = f.rx.borrow().clone();
        assert_eq!(s.role, Role::Primary);
        let lease = s.lease.unwrap();
        assert_eq!((lease.counter, lease.provisional), (1, true));
        assert_eq!(lease.expires_at, T0 + 15_000);
        assert_eq!(lease.epoch().to_string(), format!("1:{OWN}"));
    }

    #[tokio::test]
    async fn renewal_advances_renewed_at_and_expires_at() {
        let f = fixture();
        let mut task = start(&f).await;
        f.clock.advance(3_000);
        assert_eq!(task.tick().await.unwrap(), Tick::Renewed);
        let row = row_in(&f.path).unwrap();
        assert_eq!(row.counter, 1);
        assert_eq!(row.renewed_at, T0 + 3_000);
        assert_eq!(row.expires_at, T0 + 3_000 + LEASE_TTL_MS);
        assert_eq!(
            f.rx.borrow().lease.as_ref().unwrap().expires_at,
            T0 + 18_000
        );
    }

    #[tokio::test]
    async fn own_fresh_row_is_renewed_keeping_the_counter() {
        let f = fixture();
        seed(
            &f.path,
            &LeaseRow {
                counter: 3,
                hub_instance_id: OWN.into(),
                provisional: false,
                renewed_at: T0 - 1_000,
                expires_at: T0 + 14_000,
            },
        );
        let task = start(&f).await;
        assert_eq!(task.role(), Role::Primary);
        let row = row_in(&f.path).unwrap();
        assert_eq!((row.counter, row.provisional), (3, false));
        assert_eq!(row.expires_at, T0 + LEASE_TTL_MS);
    }

    #[tokio::test]
    async fn fresh_higher_epoch_elsewhere_makes_a_follower_that_never_writes() {
        let f = fixture();
        let theirs = LeaseRow {
            counter: 5,
            hub_instance_id: OTHER.into(),
            provisional: false,
            renewed_at: T0 - 1_000,
            expires_at: T0 + 10_000,
        };
        seed(&f.path, &theirs);
        let mut task = start(&f).await;
        assert_eq!(task.role(), Role::Follower);
        assert!(task.writer().is_none());
        assert_eq!(row_in(&f.path), Some(theirs.clone()));
        f.clock.advance(60_000);
        assert_eq!(task.tick().await.unwrap(), Tick::Idle);
        assert_eq!(row_in(&f.path), Some(theirs));
        let s = f.rx.borrow().clone();
        assert_eq!(s.role, Role::Follower);
        assert_eq!(s.lease.unwrap().epoch().to_string(), format!("5:{OTHER}"));
    }

    #[tokio::test]
    async fn expired_row_elsewhere_is_taken() {
        let f = fixture();
        seed(
            &f.path,
            &LeaseRow {
                counter: 5,
                hub_instance_id: OTHER.into(),
                provisional: false,
                renewed_at: T0 - 20_000,
                expires_at: T0 - 1,
            },
        );
        let task = start(&f).await;
        assert_eq!(task.role(), Role::Primary);
        let row = row_in(&f.path).unwrap();
        assert_eq!(row.hub_instance_id, OWN);
        assert_eq!(row.counter, 6);
        assert_eq!(row.expires_at, T0 + LEASE_TTL_MS);
    }

    #[tokio::test]
    async fn an_offline_take_mints_above_the_highest_counter_seen() {
        let f = fixture();
        seed(
            &f.path,
            &LeaseRow {
                counter: 2,
                hub_instance_id: OTHER.into(),
                provisional: false,
                renewed_at: T0 - 20_000,
                expires_at: T0 - 1,
            },
        );
        // The Hub heard of a fence at counter 5 (a heartbeat answer) before the lease expired.
        {
            let mut s = Store::open(&f.path, T0).unwrap();
            s.write_tx(|tx| arena_store::raise_max_seen_counter(tx, 5))
                .unwrap();
        }
        let task = start(&f).await;
        let row = row_in(&f.path).unwrap();
        assert_eq!((row.counter, row.provisional), (6, true));
        assert_eq!(row.hub_instance_id, OWN);
        // Renewing keeps what was seen, and the writer reads it back.
        assert_eq!(task.writer().unwrap().max_seen_counter().await.unwrap(), 5);
    }

    #[tokio::test]
    async fn superseded_primary_closes_writer_within_one_tick() {
        let f = fixture();
        let mut task = start(&f).await;
        let writer = task.writer().unwrap();
        let higher = LeaseRow {
            counter: 2,
            hub_instance_id: OTHER.into(),
            provisional: false,
            renewed_at: T0,
            expires_at: T0 + LEASE_TTL_MS,
        };
        seed(&f.path, &higher);
        f.clock.advance(3_000);
        assert_eq!(task.tick().await.unwrap(), Tick::Demoted);
        assert_eq!(task.role(), Role::Follower);
        assert!(writer.is_closed());
        let err = writer.submit(|_| Ok(())).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotPrimary);
        assert_eq!(row_in(&f.path), Some(higher));
        assert_eq!(f.rx.borrow().role, Role::Follower);
        assert_eq!(task.tick().await.unwrap(), Tick::Idle);
    }

    #[tokio::test]
    async fn a_higher_epoch_announced_on_the_lan_stops_the_old_main_within_one_tick() {
        let f = fixture();
        let mut task = start(&f).await;
        let writer = task.writer().unwrap();
        let seen = task.seen_epochs();
        // Our own epoch, a lower one and text that is no epoch change nothing.
        assert!(seen.note_text(&format!("1:{OWN}")));
        assert!(seen.note_text("0:ZZZ"));
        assert!(!seen.note_text("nonsense"));
        f.clock.advance(3_000);
        assert_eq!(task.tick().await.unwrap(), Tick::Renewed);

        // The new main announces epoch 2; the next tick (one renewal) closes the writer.
        assert!(seen.note_text(&format!("2:{OTHER}")));
        f.clock.advance(3_000);
        assert_eq!(task.tick().await.unwrap(), Tick::Demoted);
        assert_eq!(task.role(), Role::Follower);
        assert!(writer.is_closed());
        let err = writer.submit(|_| Ok(())).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotPrimary);
        let s = f.rx.borrow();
        assert_eq!(s.role, Role::Follower);
        assert_eq!(
            s.lease.as_ref().unwrap().epoch().to_string(),
            format!("2:{OTHER}")
        );
        drop(s);
        assert_eq!(task.tick().await.unwrap(), Tick::Idle);
        // The counter was kept, so a restart mints above it; the Hub's own row is untouched.
        assert_eq!(row_in(&f.path).unwrap().hub_instance_id, OWN);
        let mut store = Store::open(&f.path, T0).unwrap();
        assert_eq!(store.write_tx(|tx| read_max_seen_counter(tx)).unwrap(), 2);
    }

    #[tokio::test]
    async fn second_process_on_same_db_is_a_follower() {
        let f = fixture();
        let first = start(&f).await;
        assert_eq!(first.role(), Role::Primary);
        let before = row_in(&f.path);
        let (tx2, rx2) = status_channel(HubStatus::opened(1, OWN));
        let second = LeaseTask::start(
            Store::open(&f.path, T0).unwrap(),
            OWN.into(),
            f.clock.clone(),
            tx2,
        )
        .await
        .unwrap();
        assert_eq!(second.role(), Role::Follower);
        assert_eq!(rx2.borrow().role, Role::Follower);
        assert_eq!(row_in(&f.path), before);
    }

    #[tokio::test]
    async fn missing_row_demotes() {
        let f = fixture();
        let mut task = start(&f).await;
        let mut s = Store::open(&f.path, T0).unwrap();
        s.write_tx(|tx| arena_store::delete_lease(tx)).unwrap();
        assert_eq!(task.tick().await.unwrap(), Tick::Demoted);
        assert_eq!(task.role(), Role::Follower);
        assert_eq!(row_in(&f.path), None);
        assert_eq!(f.rx.borrow().lease, None);
    }

    #[tokio::test]
    async fn lower_epoch_elsewhere_demotes_and_is_not_overwritten() {
        let f = fixture();
        let mut task = start(&f).await;
        // Same counter, lower instance id: a lower epoch, but not ours.
        let lower = LeaseRow {
            counter: 1,
            hub_instance_id: "00000000000000000000000000".into(),
            provisional: true,
            renewed_at: T0,
            expires_at: T0 + LEASE_TTL_MS,
        };
        seed(&f.path, &lower);
        assert_eq!(task.tick().await.unwrap(), Tick::Demoted);
        assert_eq!(task.role(), Role::Follower);
        assert_eq!(row_in(&f.path), Some(lower));
    }

    #[tokio::test]
    async fn failing_renewals_demote_once_own_lease_expires() {
        let f = fixture();
        let mut task = start(&f).await;
        task.fail_renewals = true;
        f.clock.advance(3_000);
        assert_eq!(task.tick().await.unwrap_err().code, ErrorCode::Internal);
        assert_eq!(task.role(), Role::Primary);
        f.clock.set(T0 + LEASE_TTL_MS);
        assert_eq!(task.tick().await.unwrap(), Tick::Demoted);
        assert_eq!(task.role(), Role::Follower);
        assert_eq!(f.rx.borrow().role, Role::Follower);
    }

    #[tokio::test]
    async fn writer_closed_from_outside_demotes_on_next_tick() {
        let f = fixture();
        let mut task = start(&f).await;
        task.writer().unwrap().close().await;
        assert_eq!(task.tick().await.unwrap(), Tick::Demoted);
        assert_eq!(task.role(), Role::Follower);
        assert!(task.writer().is_none());
        let s = f.rx.borrow().clone();
        assert_eq!(s.role, Role::Follower);
        assert!(s.db_open);
    }

    #[tokio::test]
    async fn renewal_reads_the_clock_inside_the_job() {
        let f = fixture();
        let mut task = start(&f).await;
        let writer = task.writer().unwrap();
        // Hold the writer busy so the clock moves after tick() is called but before its job runs.
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let blocker = tokio::spawn(async move {
            writer
                .submit(move |_| {
                    let _ = started_tx.send(());
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .await
        });
        started_rx.await.unwrap();
        let clock = f.clock.clone();
        let mover = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            clock.set(T0 + 5_000);
            release_tx.send(()).unwrap();
        });
        assert_eq!(task.tick().await.unwrap(), Tick::Renewed);
        mover.join().unwrap();
        blocker.await.unwrap().unwrap();
        assert_eq!(row_in(&f.path).unwrap().renewed_at, T0 + 5_000);
    }

    #[tokio::test(start_paused = true)]
    async fn run_renews_every_interval() {
        let f = fixture();
        let mut rx = f.rx.clone();
        let task = start(&f).await;
        rx.borrow_and_update();
        let runner = tokio::spawn(task.run());
        f.clock.advance(3_000);
        tokio::time::advance(RENEW_INTERVAL).await;
        rx.changed().await.unwrap();
        assert_eq!(
            rx.borrow().lease.as_ref().unwrap().expires_at,
            T0 + 3_000 + LEASE_TTL_MS
        );
        assert_eq!(row_in(&f.path).unwrap().renewed_at, T0 + 3_000);
        runner.abort();
    }

    #[tokio::test]
    async fn release_expires_the_lease_now_and_closes_the_writer() {
        let f = fixture();
        let task = start(&f).await;
        let writer = task.writer().unwrap();
        f.clock.set(T0 + 1_000);
        task.release().await.unwrap();
        assert!(writer.is_closed());
        let row = row_in(&f.path).unwrap();
        assert_eq!((row.renewed_at, row.expires_at), (T0 + 1_000, T0 + 1_000));
        assert_eq!(f.rx.borrow().role, Role::Follower);
        // Another instance may take the released lease immediately.
        assert!(matches!(
            decide(Some(row), 0, OTHER, T0 + 1_000),
            Decision::Take(_)
        ));
    }

    #[test]
    fn decide_rules() {
        let other = |counter, expires_at| LeaseRow {
            counter,
            hub_instance_id: OTHER.into(),
            provisional: false,
            renewed_at: 0,
            expires_at,
        };
        assert!(
            matches!(decide(None, 0, OWN, 10), Decision::Take(r) if r.counter == 1 && r.provisional)
        );
        assert!(matches!(
            decide(Some(other(5, 11)), 9, OWN, 10),
            Decision::Follow(Some(_))
        ));
        assert!(
            matches!(decide(Some(other(5, 10)), 0, OWN, 10), Decision::Take(r) if r.counter == 6)
        );
        // An offline take mints above the highest counter seen, whichever is higher.
        assert!(
            matches!(decide(Some(other(2, 10)), 5, OWN, 10), Decision::Take(r) if r.counter == 6 && r.provisional)
        );
        assert!(
            matches!(decide(Some(other(7, 10)), 5, OWN, 10), Decision::Take(r) if r.counter == 8)
        );
    }
}
