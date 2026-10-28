//! The invoice task (Story 5.14): gives every settled sale its number once the Undo window has
//! closed, by applying the system intent `invoice.issue` one order at a time in `settled_at`
//! order. It starts with a scan, so sales settled before a restart are numbered first and none is
//! numbered twice (`invoice.issue` refuses an order that has a number).

use std::sync::Arc;
use std::time::Duration;

use arena_core::{Clock, SnapshotReader, WriterHandle};
use arena_proto::{
    Actor, ActorKind, ErrorCode, Intent, IntentKind, IntentPayload, InvoiceIssue, UNDO_WINDOW_MS,
};
use tokio::sync::broadcast::error::RecvError;

pub const INVOICE_TICK: Duration = Duration::from_secs(1);
/// How many unnumbered sales one read takes.
pub const INVOICE_BATCH: i64 = 100;
/// How long to wait before trying again after a failure.
pub const INVOICE_RETRY_MS: i64 = 5_000;

pub struct InvoiceTask {
    writer: WriterHandle,
    reader: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
}

fn issue_intent(order_id: String, now: i64) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::InvoiceIssue,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: "hub".into(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::InvoiceIssue(InvoiceIssue { order_id }),
    }
}

impl InvoiceTask {
    pub fn new(writer: WriterHandle, reader: Arc<SnapshotReader>, clock: Arc<dyn Clock>) -> Self {
        Self {
            writer,
            reader,
            clock,
        }
    }

    async fn unnumbered(&self) -> Option<Vec<(String, i64)>> {
        let reader = Arc::clone(&self.reader);
        match tokio::task::spawn_blocking(move || reader.unnumbered_orders(INVOICE_BATCH)).await {
            Ok(Ok(rows)) => Some(rows),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "unnumbered sales not read");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "unnumbered sales read panicked");
                None
            }
        }
    }

    /// Numbers every sale whose Undo window has closed, oldest settle first, and returns when to
    /// look again (`None` when nothing is waiting).
    pub async fn number_due(&self) -> Option<i64> {
        loop {
            let Some(rows) = self.unnumbered().await else {
                return Some(self.clock.now_ms() + INVOICE_RETRY_MS);
            };
            if rows.is_empty() {
                return None;
            }
            let mut numbered = false;
            for (order_id, settled_at) in rows {
                let now = self.clock.now_ms();
                let ready_at = settled_at + UNDO_WINDOW_MS + 1;
                if now < ready_at {
                    // Every later sale settled later still: wait for the oldest.
                    return Some(ready_at);
                }
                match self.writer.submit_intent(issue_intent(order_id, now)).await {
                    Ok(_) => numbered = true,
                    Err(e) if e.code == ErrorCode::Stale || e.code == ErrorCode::NotFound => {}
                    Err(e) if e.code == ErrorCode::NotPrimary => {
                        return Some(now + INVOICE_RETRY_MS);
                    }
                    Err(e) => {
                        tracing::warn!(code = e.code.as_str(), "invoice number not issued");
                        return Some(now + INVOICE_RETRY_MS);
                    }
                }
            }
            if !numbered {
                return Some(self.clock.now_ms() + INVOICE_RETRY_MS);
            }
        }
    }

    pub async fn run(self) {
        let mut applied = self.writer.subscribe();
        let mut next_at = Some(self.clock.now_ms());
        let mut tick = tokio::time::interval(INVOICE_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if next_at.is_some_and(|at| self.clock.now_ms() >= at) {
                        next_at = self.number_due().await;
                    }
                }
                ev = applied.recv() => match ev {
                    Ok(a) => {
                        if next_at.is_none() && a.changes.iter().any(|c| c.tbl == "orders") {
                            // A sale may have settled: look again when its window has closed.
                            next_at = Some(self.clock.now_ms());
                        }
                    }
                    Err(RecvError::Lagged(_)) => next_at = Some(self.clock.now_ms()),
                    Err(RecvError::Closed) => return,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::{LeaseInfo, ManualClock, Role, Writer, status_channel};
    use arena_proto::BranchBootstrap;
    use arena_store::{LeaseRow, Store, write_lease};

    const T0: i64 = 1_800_000_000_000;

    struct Fixture {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        clock: Arc<ManualClock>,
        writer: WriterHandle,
    }

    async fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, T0).unwrap();
        let lease = LeaseRow {
            counter: 1,
            hub_instance_id: "H".into(),
            provisional: true,
            renewed_at: T0,
            expires_at: i64::MAX,
        };
        store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
        let mut initial = arena_core::HubStatus::opened(store.schema_version().unwrap(), "H");
        initial.role = Role::Primary;
        initial.lease = Some(LeaseInfo::from(&lease));
        let (_status, rx) = status_channel(initial);
        let clock = Arc::new(ManualClock::new(T0));
        let writer = Writer::spawn(store, clock.clone(), rx).unwrap();
        let mut boot = issue_intent(String::new(), T0);
        boot.kind = IntentKind::BranchBootstrap;
        boot.payload = IntentPayload::BranchBootstrap(BranchBootstrap {
            name: "B".into(),
            branch_code: "B01".into(),
            currency: "INR".into(),
            tz: "UTC".into(),
            day_start_min: 0,
            fy_start_month: 1,
            onboarding: None,
        });
        writer.submit_intent(boot).await.unwrap();
        Fixture {
            _dir: dir,
            path,
            clock,
            writer,
        }
    }

    /// A settled order straight into the database: `(id, kind, total, settled_at)`.
    async fn settled(f: &Fixture, id: &str, kind: &str, total: i64, settled_at: i64) {
        let sql = format!(
            "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                 created_at, updated_at, settled_at, settled_by, kind)
             VALUES ('{id}', 'settled', {total}, {total}, 'd', 'FY2026-27', 1, 0, 0, {settled_at},
                     's', '{kind}')"
        );
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&sql).unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    fn task(f: &Fixture) -> InvoiceTask {
        InvoiceTask::new(
            f.writer.clone(),
            Arc::new(SnapshotReader::open(&f.path).unwrap()),
            f.clock.clone(),
        )
    }

    fn numbers(f: &Fixture) -> Vec<(String, Option<String>)> {
        use arena_store::{RowValue, Table};
        let text = |v: &RowValue| match v {
            RowValue::Text(t) => Some(t.clone()),
            _ => None,
        };
        let mut rows: Vec<_> = Store::open(&f.path, T0)
            .unwrap()
            .rows(Table::Orders)
            .unwrap()
            .iter()
            .map(|o| (text(&o["id"]).unwrap(), text(&o["invoice_no"])))
            .collect();
        rows.sort();
        rows
    }

    #[tokio::test]
    async fn sales_are_numbered_oldest_settle_first_once_their_window_has_closed() {
        let f = fixture().await;
        // Ids sort against the settle order, to prove the series follows `settled_at`.
        settled(&f, "O3", "pos_sale", 500, T0).await;
        settled(&f, "O2", "plan_sale", 900, T0 + 1_000).await;
        settled(&f, "O1", "session", 700, T0 + 2_000).await;
        settled(&f, "T1", "topup", 1_000, T0).await;
        settled(&f, "Z1", "pos_sale", 0, T0).await;
        let t = task(&f);

        let ready = T0 + UNDO_WINDOW_MS + 1;
        assert_eq!(t.number_due().await, Some(ready), "nothing is due yet");
        assert!(numbers(&f).iter().all(|(_, n)| n.is_none()));

        f.clock.set(ready);
        assert_eq!(
            t.number_due().await,
            Some(ready + 1_000),
            "O3 is numbered; O2 waits for its own window"
        );
        f.clock.set(ready + 2_000);
        assert_eq!(t.number_due().await, None);
        let got: Vec<_> = numbers(&f)
            .into_iter()
            .map(|(id, n)| (id, n.map(|n| n.rsplit('/').next().unwrap().to_owned())))
            .collect();
        assert_eq!(
            got,
            [
                ("O1".to_owned(), Some("000003".to_owned())),
                ("O2".to_owned(), Some("000002".to_owned())),
                ("O3".to_owned(), Some("000001".to_owned())),
                ("T1".to_owned(), None),
                ("Z1".to_owned(), None),
            ],
            "a top-up and a zero total are never numbered"
        );
    }

    #[tokio::test]
    async fn a_restart_numbers_the_backlog_in_settle_order_and_none_twice() {
        let f = fixture().await;
        settled(&f, "B", "pos_sale", 100, T0 + 10).await;
        settled(&f, "A", "pos_sale", 100, T0 + 20).await;
        f.clock.set(T0 + 60_000);
        assert_eq!(task(&f).number_due().await, None);
        let first = numbers(&f);
        assert!(first[1].1.as_deref().unwrap().ends_with("/000001"));
        assert!(first[0].1.as_deref().unwrap().ends_with("/000002"));

        // A new task instance (the Hub restarted) finds nothing left to number.
        assert_eq!(task(&f).number_due().await, None);
        assert_eq!(numbers(&f), first);
    }
}
