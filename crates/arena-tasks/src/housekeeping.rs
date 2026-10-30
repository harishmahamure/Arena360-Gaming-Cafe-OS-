//! Daily housekeeping: prunes the `applied_intents` dedupe records older than
//! [`DEDUPE_RETENTION_MS`] (30 days).
//!
//! It runs at start, every [`HOUSEKEEPING_INTERVAL`] (24 h), and whenever this Hub becomes
//! primary, but writes only while primary. It goes through the Writer
//! ([`WriterHandle::prune_applied_intents`], which re-checks the epoch inside its
//! transaction), in batches of [`PRUNE_BATCH`] records, one writer job per batch, so a large
//! backlog never holds the write lock for long. It stops once the writer closes.
//!
//! It also prunes the change log (Story 9.15): rows older than [`CHANGES_RETENTION_MS`] (90 days) that
//! the upload cursor has passed, in batches of [`CHANGES_PRUNE_BATCH`]. Rows of invoices, credit
//! notes, payments and tax records go only once [`MONEY_RETENTION_MS`] (8 years) old; rows not yet
//! uploaded never go, so a Free Hub (no upload cursor) prunes none.
//!
//! Pruning is a maintenance write outside `apply()`: dedupe records are not branch rows, so it
//! writes no change rows and is not audited. Intents expire after 7 days, so a pruned key can
//! no longer be applied anyway.
//!
//! Each time this Hub becomes primary (and at start, if it already is), the task also runs
//! [`WriterHandle::seed_defaults`] once: an upgraded branch gets the default settings and
//! built-in roles it lacks through `apply()` (`branch.seed_defaults`); with nothing missing no
//! intent is submitted.

use std::sync::Arc;
use std::time::Duration;

use arena_core::{Clock, HubStatus, Role, WriterHandle};
use arena_proto::{AppError, ErrorCode};
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior};

/// How long a dedupe record is kept: 30 days.
pub const DEDUPE_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// How long an uploaded `changes` row is kept: 90 days.
pub const CHANGES_RETENTION_MS: i64 = 90 * 24 * 60 * 60 * 1000;
/// How long a money or tax `changes` row is kept: 8 years.
pub const MONEY_RETENTION_MS: i64 = 8 * 366 * 24 * 60 * 60 * 1000;
/// The most `changes` rows one prune job deletes.
pub const CHANGES_PRUNE_BATCH: usize = 1_000;
/// How often housekeeping runs after the first run at start.
pub const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the credit auto-pause check runs, so a pause lands within an hour of the business
/// day (or the 15-day mark) turning over.
pub const CREDIT_SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// The most records one prune job deletes.
pub const PRUNE_BATCH: usize = 500;

/// What one housekeeping run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Housekeeping {
    /// Primary: this many dedupe records were deleted.
    Pruned(usize),
    /// Not primary (or the writer is closed): nothing was written.
    Idle,
}

/// The housekeeping task for one Hub process.
pub struct HousekeepingTask {
    clock: Arc<dyn Clock>,
    status: watch::Receiver<HubStatus>,
    writer: WriterHandle,
    interval: Duration,
}

impl HousekeepingTask {
    pub fn new(
        writer: WriterHandle,
        clock: Arc<dyn Clock>,
        status: watch::Receiver<HubStatus>,
    ) -> Self {
        Self {
            clock,
            status,
            writer,
            interval: HOUSEKEEPING_INTERVAL,
        }
    }

    fn is_primary(&self) -> bool {
        !self.writer.is_closed() && self.status.borrow().role == Role::Primary
    }

    /// One run: while primary, deletes dedupe records with `applied_at < now - 30 days`, one
    /// batch per writer job, until none remain.
    pub async fn tick(&self) -> Result<Housekeeping, AppError> {
        if !self.is_primary() {
            return Ok(Housekeeping::Idle);
        }
        let cutoff = self.clock.now_ms().saturating_sub(DEDUPE_RETENTION_MS);
        let mut total = 0;
        loop {
            match self.writer.prune_applied_intents(cutoff, PRUNE_BATCH).await {
                Ok(n) => {
                    total += n;
                    if n < PRUNE_BATCH {
                        return Ok(Housekeeping::Pruned(total));
                    }
                }
                Err(e) if e.code == ErrorCode::NotPrimary => {
                    return Ok(if total == 0 {
                        Housekeeping::Idle
                    } else {
                        Housekeeping::Pruned(total)
                    });
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// One run of the change-log prune (see the module docs), while primary: batches until fewer
    /// than [`CHANGES_PRUNE_BATCH`] go. Returns how many rows were deleted.
    pub async fn prune_changes(&self) -> Result<usize, AppError> {
        if !self.is_primary() {
            return Ok(0);
        }
        let now = self.clock.now_ms();
        let cutoff = now.saturating_sub(CHANGES_RETENTION_MS);
        let money_cutoff = now.saturating_sub(MONEY_RETENTION_MS);
        let mut total = 0;
        loop {
            match self
                .writer
                .prune_changes(cutoff, money_cutoff, CHANGES_PRUNE_BATCH)
                .await
            {
                Ok(n) => {
                    total += n;
                    if n < CHANGES_PRUNE_BATCH {
                        return Ok(total);
                    }
                }
                Err(e) if e.code == ErrorCode::NotPrimary => return Ok(total),
                Err(e) => return Err(e),
            }
        }
    }

    async fn tick_and_log(&self) {
        match self.tick().await {
            Ok(Housekeeping::Pruned(n)) => {
                tracing::info!(pruned = n, "housekeeping: dedupe records pruned");
            }
            Ok(Housekeeping::Idle) => {}
            Err(e) => tracing::warn!(code = e.code.as_str(), "housekeeping failed"),
        }
        match self.prune_changes().await {
            Ok(0) => {}
            Ok(n) => tracing::info!(pruned = n, "housekeeping: uploaded change rows pruned"),
            Err(e) => tracing::warn!(code = e.code.as_str(), "housekeeping: change prune failed"),
        }
    }

    /// Story 3.20: pauses credit for players 15+ days overdue (and reopens it when the branch
    /// rule is off), while primary; logs the outcome.
    pub async fn credit_sweep_and_log(&self) {
        if !self.is_primary() {
            return;
        }
        match self.writer.credit_auto_pause(None).await {
            Ok(Some(applied)) => tracing::info!(
                rows = applied.changes.len(),
                "credit.auto_pause: credit pauses updated"
            ),
            Ok(None) => {}
            Err(e) if e.code == ErrorCode::NotPrimary => {}
            Err(e) => tracing::warn!(code = e.code.as_str(), "credit.auto_pause failed"),
        }
    }

    /// Story 5.7: raises the "Expiring soon" alert for stock that is expired or about to expire
    /// (once per business day, from the Hub's own clock) and clears it when none is, while
    /// primary; logs the outcome.
    pub async fn stock_expiry_sweep_and_log(&self) {
        if !self.is_primary() {
            return;
        }
        match self.writer.stock_expiry_check().await {
            Ok(Some(applied)) => tracing::info!(
                rows = applied.changes.len(),
                "stock.expiry_check: expiry alert updated"
            ),
            Ok(None) => {}
            Err(e) if e.code == ErrorCode::NotPrimary => {}
            Err(e) => tracing::warn!(code = e.code.as_str(), "stock.expiry_check failed"),
        }
    }

    /// Seeds missing default rows (see the module docs); logs the outcome.
    pub async fn seed_and_log(&self) {
        if !self.is_primary() {
            return;
        }
        match self.writer.seed_defaults().await {
            Ok(Some(applied)) => tracing::info!(
                rows = applied.changes.len(),
                "branch.seed_defaults: missing default rows added"
            ),
            Ok(None) => {}
            Err(e) if e.code == ErrorCode::NotPrimary => {}
            Err(e) => tracing::warn!(code = e.code.as_str(), "branch.seed_defaults failed"),
        }
    }

    /// Runs at once, then every interval and on each promotion to primary, until the writer
    /// closes (or the status channel does).
    pub async fn run(mut self) {
        let mut interval = tokio::time::interval_at(Instant::now(), self.interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut sweep = tokio::time::interval_at(Instant::now(), CREDIT_SWEEP_INTERVAL);
        sweep.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut was_primary = self.is_primary();
        self.seed_and_log().await;
        while !self.writer.is_closed() {
            tokio::select! {
                _ = interval.tick() => self.tick_and_log().await,
                _ = sweep.tick() => {
                    self.credit_sweep_and_log().await;
                    self.stock_expiry_sweep_and_log().await;
                }
                changed = self.status.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let primary = self.is_primary();
                    if primary && !was_primary {
                        self.seed_and_log().await;
                        self.tick_and_log().await;
                        self.credit_sweep_and_log().await;
                        self.stock_expiry_sweep_and_log().await;
                    }
                    was_primary = primary;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::{LeaseInfo, ManualClock, Writer, status_channel};
    use arena_store::{
        AppliedIntentRecord, LeaseRow, Store, find_applied_intent, insert_applied_intent,
        write_lease,
    };
    use std::path::{Path, PathBuf};

    const NOW: i64 = 1_800_000_000_000;
    const DAY: i64 = 24 * 60 * 60 * 1000;

    struct Fixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
        clock: Arc<ManualClock>,
        status: watch::Sender<HubStatus>,
        task: HousekeepingTask,
    }

    fn rec(key: &str, applied_at: i64) -> AppliedIntentRecord {
        AppliedIntentRecord {
            idempotency_key: key.into(),
            intent_id: "I".into(),
            intent_kind: "settings.set".into(),
            fingerprint: "F".into(),
            outcome: vec![0x80],
            applied_at,
        }
    }

    fn lease(counter: i64, id: &str) -> LeaseRow {
        LeaseRow {
            counter,
            hub_instance_id: id.into(),
            provisional: true,
            renewed_at: NOW,
            // Never expires, so the manual clock can move by days.
            expires_at: i64::MAX,
        }
    }

    /// A writer holding lease `(1, H)` with dedupe records aged 31 days (`OLD`), 29 days
    /// (`NEW`) and 29.5 days (`MID`, which passes 30 days a day later).
    fn fixture(role: Role) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, NOW).unwrap();
        store
            .write_tx(|tx| {
                write_lease(tx, &lease(1, "H"))?;
                insert_applied_intent(tx, &rec("OLD", NOW - 31 * DAY))?;
                insert_applied_intent(tx, &rec("NEW", NOW - 29 * DAY))?;
                insert_applied_intent(tx, &rec("MID", NOW - 30 * DAY + DAY / 2))?;
                Ok(())
            })
            .unwrap();
        let mut initial = HubStatus::opened(store.schema_version().unwrap(), "H");
        initial.role = role;
        initial.lease = Some(LeaseInfo::from(&lease(1, "H")));
        let (status, rx) = status_channel(initial);
        let clock = Arc::new(ManualClock::new(NOW));
        let writer = Writer::spawn(store, clock.clone(), rx.clone()).unwrap();
        let task = HousekeepingTask::new(writer, clock.clone(), rx);
        Fixture {
            _dir: dir,
            path,
            clock,
            status,
            task,
        }
    }

    fn has(path: &Path, key: &str) -> bool {
        let mut s = Store::open(path, NOW).unwrap();
        s.write_tx(|tx| Ok(find_applied_intent(tx, key)?.is_some()))
            .unwrap()
    }

    fn count(path: &Path) -> i64 {
        Store::open(path, NOW)
            .unwrap()
            .applied_intent_count()
            .unwrap()
    }

    /// Polls `cond` for up to 5 s.
    async fn eventually(mut cond: impl FnMut() -> bool) -> bool {
        for _ in 0..500 {
            if cond() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cond()
    }

    #[tokio::test]
    async fn prunes_the_31_day_record_and_keeps_the_29_day_one() {
        let f = fixture(Role::Primary);
        let changes_before = Store::open(&f.path, NOW).unwrap().changes().unwrap();
        assert_eq!(f.task.tick().await.unwrap(), Housekeeping::Pruned(1));
        assert!(!has(&f.path, "OLD"));
        assert!(has(&f.path, "NEW") && has(&f.path, "MID"));
        assert_eq!(
            Store::open(&f.path, NOW).unwrap().changes().unwrap(),
            changes_before,
            "pruning writes no change rows"
        );
    }

    #[tokio::test]
    async fn exactly_30_days_is_kept() {
        let f = fixture(Role::Primary);
        f.clock.set(NOW - 29 * DAY + DEDUPE_RETENTION_MS);
        assert_eq!(f.task.tick().await.unwrap(), Housekeeping::Pruned(2));
        assert!(has(&f.path, "NEW"));
    }

    #[tokio::test]
    async fn a_large_backlog_is_pruned_in_batches_one_job_each() {
        let f = fixture(Role::Primary);
        Store::open(&f.path, NOW)
            .unwrap()
            .write_tx(|tx| {
                for i in 0..1_201 {
                    insert_applied_intent(tx, &rec(&format!("B{i}"), NOW - 40 * DAY))?;
                }
                Ok(())
            })
            .unwrap();
        let jobs = f.task.writer.jobs_submitted();
        // 1 202 old records: batches of 500, 500 and 202.
        assert_eq!(f.task.tick().await.unwrap(), Housekeeping::Pruned(1_202));
        assert_eq!(f.task.writer.jobs_submitted() - jobs, 3);
        assert_eq!(count(&f.path), 2);
        // Exactly one full batch left over still takes a second (empty) job to confirm.
        Store::open(&f.path, NOW)
            .unwrap()
            .write_tx(|tx| {
                for i in 0..PRUNE_BATCH {
                    insert_applied_intent(tx, &rec(&format!("C{i}"), NOW - 40 * DAY))?;
                }
                Ok(())
            })
            .unwrap();
        let jobs = f.task.writer.jobs_submitted();
        assert_eq!(
            f.task.tick().await.unwrap(),
            Housekeeping::Pruned(PRUNE_BATCH)
        );
        assert_eq!(f.task.writer.jobs_submitted() - jobs, 2);
    }

    fn seed_changes(path: &Path, cursor_seq: Option<i64>) {
        use arena_store::{
            ChangeOp, ChangeRecord, UploadCursor, insert_change, write_upload_cursor,
        };
        let mut store = Store::open(path, NOW).unwrap();
        store
            .write_tx(|tx| {
                // Seqs 1-4 are 100 days old, 5 is a 100-day-old order, 6 is a day old.
                for seq in 1..=6_i64 {
                    insert_change(
                        tx,
                        &ChangeRecord {
                            epoch: "1:H".into(),
                            seq,
                            tbl: if seq == 5 { "orders" } else { "settings" }.into(),
                            row_id: format!("r{seq}"),
                            op: ChangeOp::Insert,
                            row: vec![0x80],
                            intent_id: format!("I{seq}"),
                            intent_kind: "settings.set".into(),
                            actor_kind: "staff".into(),
                            actor_id: "S".into(),
                            elevated_by_kind: None,
                            elevated_by_id: None,
                            elevated_by_mode: None,
                            elevated_by_verifier_version: None,
                            at: if seq == 6 { NOW - DAY } else { NOW - 100 * DAY },
                            business_day: None,
                            fy: None,
                            reason: None,
                        },
                    )?;
                }
                if let Some(last) = cursor_seq {
                    write_upload_cursor(
                        tx,
                        &UploadCursor {
                            epoch: "1:H".into(),
                            last_seq_uploaded: last,
                            uploaded_at: NOW,
                        },
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    fn change_seqs(path: &Path) -> Vec<i64> {
        Store::open(path, NOW)
            .unwrap()
            .changes()
            .unwrap()
            .iter()
            .map(|c| c.seq)
            .collect()
    }

    #[tokio::test]
    async fn uploaded_change_rows_older_than_90_days_are_pruned_but_money_rows_stay() {
        let f = fixture(Role::Primary);
        seed_changes(&f.path, Some(5));
        assert_eq!(f.task.prune_changes().await.unwrap(), 4);
        // The order row (money, 100 days old) and the recent row stay.
        assert_eq!(change_seqs(&f.path), vec![5, 6]);
        assert_eq!(f.task.prune_changes().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn unuploaded_change_rows_are_never_pruned() {
        let f = fixture(Role::Primary);
        seed_changes(&f.path, Some(2));
        assert_eq!(f.task.prune_changes().await.unwrap(), 2);
        assert_eq!(change_seqs(&f.path), vec![3, 4, 5, 6]);
    }

    #[tokio::test]
    async fn a_free_hub_with_no_upload_cursor_prunes_no_change_rows() {
        let f = fixture(Role::Primary);
        seed_changes(&f.path, None);
        assert_eq!(f.task.prune_changes().await.unwrap(), 0);
        assert_eq!(change_seqs(&f.path), vec![1, 2, 3, 4, 5, 6]);
    }

    #[tokio::test]
    async fn a_follower_prunes_no_change_rows() {
        let f = fixture(Role::Follower);
        seed_changes(&f.path, Some(6));
        assert_eq!(f.task.prune_changes().await.unwrap(), 0);
        assert_eq!(change_seqs(&f.path).len(), 6);
    }
    #[tokio::test]
    async fn a_follower_prunes_nothing() {
        let f = fixture(Role::Follower);
        assert_eq!(f.task.tick().await.unwrap(), Housekeeping::Idle);
        assert_eq!(count(&f.path), 3);
    }

    #[tokio::test]
    async fn a_superseded_epoch_prunes_nothing() {
        let f = fixture(Role::Primary);
        Store::open(&f.path, NOW)
            .unwrap()
            .write_tx(|tx| write_lease(tx, &lease(2, "OTHER")))
            .unwrap();
        assert_eq!(f.task.tick().await.unwrap(), Housekeeping::Idle);
        assert_eq!(count(&f.path), 3);
    }

    #[tokio::test]
    async fn a_closed_writer_is_idle_and_run_returns() {
        let f = fixture(Role::Primary);
        f.task.writer.close().await;
        assert_eq!(f.task.tick().await.unwrap(), Housekeeping::Idle);
        tokio::time::timeout(Duration::from_secs(5), f.task.run())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_upgraded_branch_is_seeded_at_start_and_its_settings_are_editable() {
        use arena_core::SnapshotReader;
        use arena_proto::{Actor, ActorKind, Intent, IntentKind, IntentPayload, SettingsSet};
        use arena_store::{NewBranch, insert_branch};
        // A branch from before 0005/0006: the branch row only, no settings, no roles.
        let f = fixture(Role::Primary);
        Store::open(&f.path, NOW)
            .unwrap()
            .write_tx(|tx| {
                insert_branch(
                    tx,
                    &NewBranch {
                        branch_id: "B",
                        name: "Old Branch",
                        branch_code: "OLD01",
                        currency: "USD",
                        tz: "Asia/Dubai",
                        day_start_min: 300,
                        fy_start_month: 1,
                    },
                )
            })
            .unwrap();
        let path = f.path.clone();
        let writer = f.task.writer.clone();
        let _runner = tokio::spawn(f.task.run());
        let reader = SnapshotReader::open(&path).unwrap();
        assert!(
            eventually(|| reader
                .settings()
                .unwrap()
                .iter()
                .any(|s| s.key == "branch.tz" && s.value == "Asia/Dubai"))
            .await
        );
        let settings = reader.settings().unwrap();
        for key in arena_core::LOCALE_KEYS {
            assert!(settings.iter().any(|s| s.key == key), "{key}");
        }
        let store = Store::open(&path, NOW).unwrap();
        assert_eq!(store.rows(arena_store::Table::Roles).unwrap().len(), 5);
        assert!(
            store
                .changes()
                .unwrap()
                .iter()
                .all(|c| c.intent_kind == "branch.seed_defaults")
        );
        // Settings → Branch can now save.
        let currency = settings
            .iter()
            .find(|s| s.key == "branch.currency")
            .unwrap();
        assert_eq!(currency.value, "USD");
        let intent = Intent {
            intent_id: "01JAAAAAAAAAAAAAAAAAAAAAAA".into(),
            kind: IntentKind::SettingsSet,
            idempotency_key: "01JAAAAAAAAAAAAAAAAAAAAAAB".into(),
            issued_at: NOW,
            base_rev: Some(currency.rev),
            actor: Actor {
                kind: ActorKind::Owner,
                id: "owner".into(),
            },
            elevated_by: None,
            reason: None,
            payload: IntentPayload::SettingsSet(SettingsSet {
                key: "branch.currency".into(),
                value: "EUR".into(),
            }),
        };
        writer.submit_intent(intent).await.unwrap();
        assert_eq!(
            reader.locale(NOW).unwrap().unwrap().currency,
            "EUR",
            "the edit applied"
        );
    }

    #[tokio::test]
    async fn run_prunes_at_start_then_each_interval() {
        let mut f = fixture(Role::Primary);
        f.task.interval = Duration::from_millis(200);
        let path = f.path.clone();
        let writer = f.task.writer.clone();
        let runner = tokio::spawn(f.task.run());
        // At start: only the 31-day record goes.
        assert!(eventually(|| !has(&path, "OLD")).await);
        assert!(has(&path, "MID") && has(&path, "NEW"));
        // A day later, the next interval prunes the record that aged past 30 days meanwhile.
        f.clock.advance(DAY);
        assert!(eventually(|| !has(&path, "MID")).await);
        assert!(has(&path, "NEW"));
        assert!(!runner.is_finished());
        // Demotion: the writer closes and the status changes; the task ends.
        writer.close().await;
        f.status.send_modify(|s| s.role = Role::Follower);
        tokio::time::timeout(Duration::from_secs(5), runner)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn promotion_to_primary_runs_a_tick_at_once() {
        let f = fixture(Role::Follower);
        let path = f.path.clone();
        let writer = f.task.writer.clone();
        // The default 24 h interval: only the start tick (idle as follower) would run.
        let runner = tokio::spawn(f.task.run());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(has(&path, "OLD"), "a follower prunes nothing");
        f.status.send_modify(|s| s.role = Role::Primary);
        assert!(eventually(|| !has(&path, "OLD")).await);
        assert!(!runner.is_finished());
        writer.close().await;
        f.status.send_modify(|s| s.role = Role::Follower);
        tokio::time::timeout(Duration::from_secs(5), runner)
            .await
            .unwrap()
            .unwrap();
    }
}
