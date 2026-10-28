//! The receipt print task (Story 5.17): sends what the side-effect outbox holds to the printer,
//! after the sale has committed.
//!
//! A settle with the receipt choice Print, and a Reprint, queue a `print` row inside their own
//! transaction. This task finds queued rows (at start, so a restart prints what a crash left, and
//! whenever an order changes), builds the receipt from the committed rows, sends it through the
//! printer adapter (USB, LAN 9100 or Bluetooth, all behind the same interface) and applies the
//! result intent: `receipt.printed`, or `receipt.print_failed` with the error. A failure never
//! touches the sale; it raises the "Printer offline" alert and the order shows Reprint.
//!
//! The cash drawer kick (Story 5.18) is the same kind of job: a cash settle queues a `drawer` row,
//! the task sends the pulse through the same printer and applies `drawer.kicked` or
//! `drawer.kick_failed`. Kicks go first: the cash is already in the customer's hand.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arena_core::printer::{Printer, PrinterJobs, PrinterTarget, drawer_kick};
use arena_core::{Clock, SnapshotReader, WriterHandle};
use arena_proto::{
    Actor, ActorKind, DrawerKickFailed, DrawerKicked, ErrorCode, Intent, IntentKind, IntentPayload,
    KEY_PRINTER_SELECTED, PrinterErrorKind, PrinterJobFailure, PrinterSelected, ReceiptPrintFailed,
    ReceiptPrinted, printer_error_text,
};
use arena_store::{KIND_DRAWER, OutboxRow};
use tokio::sync::broadcast::error::RecvError;

pub const PRINT_TICK: Duration = Duration::from_millis(500);
/// How many queued receipts one pass takes.
pub const PRINT_BATCH: i64 = 20;

/// How a sent job came out, kept until its result intent is applied so a receipt is never sent
/// twice because the Hub could not record the first.
#[derive(Debug, Clone)]
enum Outcome {
    Printed,
    Failed(PrinterErrorKind, String),
}

pub struct ReceiptPrintTask {
    writer: WriterHandle,
    reader: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    printer: Arc<dyn Printer>,
    jobs: Arc<PrinterJobs>,
    unrecorded: std::sync::Mutex<HashMap<String, Outcome>>,
}

fn result_intent(row: &OutboxRow, outcome: &Outcome, now: i64) -> Intent {
    let drawer = row.kind == KIND_DRAWER;
    let outbox_id = row.id.clone();
    let (kind, payload) = match (outcome, drawer) {
        (Outcome::Printed, false) => (
            IntentKind::ReceiptPrinted,
            IntentPayload::ReceiptPrinted(ReceiptPrinted { outbox_id }),
        ),
        (Outcome::Failed(error_kind, error), false) => (
            IntentKind::ReceiptPrintFailed,
            IntentPayload::ReceiptPrintFailed(ReceiptPrintFailed {
                outbox_id,
                error_kind: *error_kind,
                error: error.clone(),
            }),
        ),
        (Outcome::Printed, true) => (
            IntentKind::DrawerKicked,
            IntentPayload::DrawerKicked(DrawerKicked { outbox_id }),
        ),
        (Outcome::Failed(error_kind, error), true) => (
            IntentKind::DrawerKickFailed,
            IntentPayload::DrawerKickFailed(DrawerKickFailed {
                outbox_id,
                error_kind: *error_kind,
                error: error.clone(),
            }),
        ),
    };
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind,
        // The row id (a ULID) makes a retried result the same intent.
        idempotency_key: row.id.clone(),
        issued_at: now,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: "printer".into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

fn reprint(row: &OutboxRow) -> bool {
    serde_json::from_str::<serde_json::Value>(&row.payload)
        .ok()
        .and_then(|v| v.get("reprint").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

impl ReceiptPrintTask {
    pub fn new(
        writer: WriterHandle,
        reader: Arc<SnapshotReader>,
        clock: Arc<dyn Clock>,
        printer: Arc<dyn Printer>,
        jobs: Arc<PrinterJobs>,
    ) -> Self {
        Self {
            writer,
            reader,
            clock,
            printer,
            jobs,
            unrecorded: std::sync::Mutex::new(HashMap::new()),
        }
    }

    async fn blocking<T: Send + 'static>(
        &self,
        what: &'static str,
        read: impl FnOnce(&SnapshotReader) -> Result<T, arena_proto::AppError> + Send + 'static,
    ) -> Option<T> {
        let reader = Arc::clone(&self.reader);
        match tokio::task::spawn_blocking(move || read(&reader)).await {
            Ok(Ok(v)) => Some(v),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), what, "receipt print read failed");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, what, "receipt print read panicked");
                None
            }
        }
    }

    fn fail(kind: PrinterErrorKind, message: &str) -> Outcome {
        Outcome::Failed(kind, printer_error_text(message))
    }

    /// Builds the receipt and sends it; the outcome is what the result intent records.
    async fn send(&self, row: &OutboxRow) -> Outcome {
        let reprint = reprint(row);
        let drawer = row.kind == KIND_DRAWER;
        let order_id = row.ref_id.clone();
        let now = self.clock.now_ms();
        let read = self
            .blocking("receipt", move |r| {
                let selected = r
                    .settings()?
                    .into_iter()
                    .find(|s| s.key == KEY_PRINTER_SELECTED)
                    .map(|s| s.value)
                    .unwrap_or_default();
                let bytes = if drawer {
                    Some(drawer_kick())
                } else {
                    r.receipt_bytes(&order_id, reprint, now)?
                };
                Ok((selected, bytes))
            })
            .await;
        let Some((selected, bytes)) = read else {
            return Self::fail(PrinterErrorKind::Other, "the receipt could not be built");
        };
        let Some(bytes) = bytes else {
            return Self::fail(PrinterErrorKind::Other, "the order of this receipt is gone");
        };
        let selected = match PrinterSelected::parse(&selected) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return Self::fail(PrinterErrorKind::Other, "no printer is set up");
            }
            Err(_) => {
                return Self::fail(PrinterErrorKind::Other, "the saved printer is not valid");
            }
        };
        let target = match PrinterTarget::from_selected(&selected) {
            Ok(t) => t,
            Err(e) => return Self::fail(e.kind(), &e.to_string()),
        };
        let _single = self.jobs.lock().lock().await;
        match self.printer.send(&target, &bytes).await {
            Ok(()) => {
                self.jobs.record(None);
                Outcome::Printed
            }
            Err(e) => {
                let text = printer_error_text(&e.to_string());
                self.jobs.record(Some(PrinterJobFailure {
                    error_kind: e.kind(),
                    error: text.clone(),
                    at: self.clock.now_ms(),
                }));
                Outcome::Failed(e.kind(), text)
            }
        }
    }

    /// Applies the result of a job; false when the Hub could not (it stays queued for a retry).
    async fn record(&self, row: &OutboxRow, outcome: Outcome) -> bool {
        let intent = result_intent(row, &outcome, self.clock.now_ms());
        match self.writer.submit_intent(intent).await {
            Ok(_) => true,
            // The row was already closed (a replayed result): nothing is left to do.
            Err(e) if e.code == ErrorCode::Stale || e.code == ErrorCode::NotFound => true,
            Err(e) => {
                tracing::warn!(code = e.code.as_str(), field = ?e.field, message = %e.message, "receipt result not recorded");
                false
            }
        }
    }

    /// Prints every queued receipt, oldest first. Returns whether any row is still waiting.
    pub async fn drain(&self) -> bool {
        let Some(rows) = self
            .blocking("queue", |r| {
                let mut rows = r.queued_drawers(PRINT_BATCH)?;
                rows.extend(r.queued_prints(PRINT_BATCH)?);
                Ok(rows)
            })
            .await
        else {
            return true;
        };
        for row in &rows {
            let held = self
                .unrecorded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&row.id)
                .cloned();
            let outcome = match held {
                Some(o) => o,
                None => {
                    let o = self.send(row).await;
                    self.unrecorded
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(row.id.clone(), o.clone());
                    o
                }
            };
            if self.record(row, outcome).await {
                self.unrecorded
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&row.id);
            } else {
                return true;
            }
        }
        rows.len() as i64 >= PRINT_BATCH
    }

    pub async fn run(self) {
        let mut applied = self.writer.subscribe();
        let mut due = true;
        let mut tick = tokio::time::interval(PRINT_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if due {
                        due = self.drain().await;
                    }
                }
                ev = applied.recv() => match ev {
                    Ok(a) => {
                        if a.changes.iter().any(|c| c.tbl == "orders") {
                            due = true;
                        }
                    }
                    Err(RecvError::Lagged(_)) => due = true,
                    Err(RecvError::Closed) => return,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printer::{FakePrinter, PrinterError};
    use arena_core::{LeaseInfo, ManualClock, Role, Writer, status_channel};
    use arena_proto::BranchBootstrap;
    use arena_store::{LeaseRow, Store, write_lease};

    const T0: i64 = 1_800_000_000_000;
    const LAN: &str = r#"{"kind":"lan","id_or_addr":"192.168.1.50:9100","name":"Counter"}"#;

    struct Fixture {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        clock: Arc<ManualClock>,
        writer: WriterHandle,
        fake: Arc<FakePrinter>,
        jobs: Arc<PrinterJobs>,
    }

    async fn fixture(printer: Option<&str>) -> Fixture {
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
        let boot = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::BranchBootstrap,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: T0,
            base_rev: None,
            actor: Actor {
                kind: ActorKind::System,
                id: "hub".into(),
            },
            elevated_by: None,
            reason: None,
            payload: IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "Arena".into(),
                branch_code: "B01".into(),
                currency: "INR".into(),
                tz: "UTC".into(),
                day_start_min: 0,
                fy_start_month: 1,
                onboarding: None,
            }),
        };
        writer.submit_intent(boot).await.unwrap();
        if let Some(value) = printer {
            let sql =
                format!("UPDATE settings SET value = '{value}' WHERE key = 'printer.selected'");
            writer
                .submit(move |tx| {
                    tx.execute_batch(&sql).unwrap();
                    Ok(())
                })
                .await
                .unwrap();
        }
        Fixture {
            _dir: dir,
            path,
            clock,
            writer,
            fake: Arc::new(FakePrinter::new(Vec::new())),
            jobs: Arc::default(),
        }
    }

    /// A numbered settled sale (made once per order) and a queued print row for it.
    async fn queued(f: &Fixture, row: &str, order: &str, payload: &str) {
        let sql = format!(
            "INSERT OR IGNORE INTO orders (id, status, subtotal_minor, total_minor, business_day, fy,
                 rev, created_at, updated_at, settled_at, settled_by, kind, invoice_no)
             VALUES ('{order}', 'settled', 4000, 4000, 'd', 'FY2026-27', 1, 0, 0, {T0}, 's',
                     'pos_sale', 'B01/26-27/E1h/000001');
             INSERT OR IGNORE INTO order_lines (id, order_id, from_at, to_at, minutes,
                 rate_minor_per_hour, amount_minor, position, rev, created_at, updated_at, label,
                 qty, unit_price_minor)
             VALUES ('L-{order}', '{order}', 0, 0, 0, 0, 4000, 0, 1, 0, 0, 'Cola', 1, 4000);
             INSERT INTO side_effect_outbox (id, kind, ref_id, payload, state, at)
             VALUES ('{row}', 'print', '{order}', '{payload}', 'queued', {T0});"
        );
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&sql).unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    fn task(f: &Fixture) -> ReceiptPrintTask {
        ReceiptPrintTask::new(
            f.writer.clone(),
            Arc::new(SnapshotReader::open(&f.path).unwrap()),
            f.clock.clone(),
            f.fake.clone(),
            f.jobs.clone(),
        )
    }

    /// The state and error of an outbox row.
    fn state(f: &Fixture, row: &str) -> (String, Option<String>) {
        let mut store = Store::open(&f.path, T0).unwrap();
        let id = row.to_owned();
        let mut out = None;
        store
            .write_tx(|tx| {
                out = arena_store::find_outbox(tx, &id)?.map(|r| (r.state, r.error));
                Ok(())
            })
            .unwrap();
        out.unwrap()
    }

    fn alert_kinds(f: &Fixture) -> Vec<String> {
        use arena_store::{RowValue, Table};
        Store::open(&f.path, T0)
            .unwrap()
            .rows(Table::Alerts)
            .unwrap()
            .iter()
            .map(|a| match &a["kind"] {
                RowValue::Text(t) => t.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[tokio::test]
    async fn a_queued_receipt_prints_after_commit_and_is_recorded_as_printed() {
        let f = fixture(Some(LAN)).await;
        queued(
            &f,
            "01J000000000000000000000R1",
            "O1",
            r#"{"reprint":false,"by":"s"}"#,
        )
        .await;
        assert!(!task(&f).drain().await);

        let sent = f.fake.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].target,
            PrinterTarget::Lan("192.168.1.50:9100".parse().unwrap())
        );
        let text = String::from_utf8_lossy(&sent[0].bytes).to_string();
        assert!(text.contains("B01/26-27/E1h/000001") && text.contains("Cola"));
        assert!(!text.contains("Reprint"));
        assert_eq!(
            state(&f, "01J000000000000000000000R1"),
            ("done".to_owned(), None)
        );
        assert!(alert_kinds(&f).is_empty());
        assert_eq!(f.jobs.last_failure(), None);
    }

    #[tokio::test]
    async fn a_reprint_row_prints_the_marked_copy() {
        let f = fixture(Some(LAN)).await;
        queued(
            &f,
            "01J000000000000000000000R1",
            "O1",
            r#"{"reprint":true,"by":"s"}"#,
        )
        .await;
        task(&f).drain().await;
        let text = String::from_utf8_lossy(&f.fake.sent()[0].bytes).to_string();
        assert!(text.contains("Reprint"));
    }

    #[tokio::test]
    async fn a_refused_job_is_recorded_as_failed_with_an_alert_and_the_next_job_clears_it() {
        let f = fixture(Some(LAN)).await;
        let addr = "192.168.1.50:9100".parse().unwrap();
        f.fake.fail_with(Some(PrinterError::Refused(addr)));
        queued(
            &f,
            "01J000000000000000000000R1",
            "O1",
            r#"{"reprint":false,"by":"s"}"#,
        )
        .await;
        task(&f).drain().await;

        let (first, error) = state(&f, "01J000000000000000000000R1");
        assert_eq!(first, "failed");
        assert!(error.unwrap().contains("refused the connection"));
        assert_eq!(alert_kinds(&f), ["printer_offline"]);
        let failure = f.jobs.last_failure().expect("the failure is kept");
        assert_eq!(failure.error_kind, PrinterErrorKind::Refused);

        f.fake.fail_with(None);
        queued(
            &f,
            "01J000000000000000000000R2",
            "O1",
            r#"{"reprint":true,"by":"s"}"#,
        )
        .await;
        task(&f).drain().await;
        assert_eq!(state(&f, "01J000000000000000000000R2").0, "done");
        assert_eq!(f.jobs.last_failure(), None, "a job that works clears it");
        assert_eq!(alert_kinds(&f).len(), 1, "the alert is not raised twice");
    }

    #[tokio::test]
    async fn no_printer_set_up_fails_the_row_without_blocking_anything() {
        let f = fixture(None).await;
        queued(
            &f,
            "01J000000000000000000000R1",
            "O1",
            r#"{"reprint":false,"by":"s"}"#,
        )
        .await;
        task(&f).drain().await;
        let (s, error) = state(&f, "01J000000000000000000000R1");
        assert_eq!(s, "failed");
        assert_eq!(error.as_deref(), Some("no printer is set up"));
        assert!(f.fake.sent().is_empty());
        assert_eq!(alert_kinds(&f), ["printer_offline"]);
    }

    #[tokio::test]
    async fn a_receipt_is_sent_once_even_when_drained_twice() {
        let f = fixture(Some(LAN)).await;
        queued(
            &f,
            "01J000000000000000000000R1",
            "O1",
            r#"{"reprint":false,"by":"s"}"#,
        )
        .await;
        let t = task(&f);
        t.drain().await;
        t.drain().await;
        assert_eq!(f.fake.sent().len(), 1);
    }

    /// A cash sale of `O1` against an open shift and a queued `drawer` row for it.
    async fn queued_drawer(f: &Fixture, row: &str) {
        queued(
            f,
            "01J000000000000000000000R0",
            "O1",
            r#"{"reprint":false,"by":"s"}"#,
        )
        .await;
        let sql = format!(
            "INSERT INTO shifts (id, staff_id, opened_at, float_minor, carried_minor, state, rev,
                 created_at, updated_at)
             VALUES ('SH1', 's', 0, 0, 0, 'open', 0, 0, 0);
             INSERT INTO cash_events (id, shift_id, kind, amount_minor, staff_id, order_id, rev,
                 created_at, updated_at)
             VALUES ('CE1', 'SH1', 'sale', 4000, 's', 'O1', 0, 0, 0);
             INSERT INTO side_effect_outbox (id, kind, ref_id, payload, state, at)
             VALUES ('{row}', 'drawer', 'O1', '{{\"reprint\":false,\"by\":\"s\"}}', 'queued', {T0});
             UPDATE side_effect_outbox SET state = 'done' WHERE id = '01J000000000000000000000R0';"
        );
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&sql).unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    fn drawer_events(f: &Fixture) -> usize {
        use arena_store::{RowValue, Table};
        Store::open(&f.path, T0)
            .unwrap()
            .rows(Table::CashEvents)
            .unwrap()
            .iter()
            .filter(|e| matches!(&e["kind"], RowValue::Text(k) if k == "drawer_open"))
            .count()
    }

    #[tokio::test]
    async fn a_queued_drawer_kick_sends_the_pulse_and_writes_a_drawer_open_cash_event() {
        let f = fixture(Some(LAN)).await;
        queued_drawer(&f, "01J000000000000000000000D1").await;
        task(&f).drain().await;

        let sent = f.fake.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].bytes, b"\x1bp\x00\x19\xfa");
        assert_eq!(
            state(&f, "01J000000000000000000000D1"),
            ("done".to_owned(), None)
        );
        assert_eq!(drawer_events(&f), 1);
        assert!(alert_kinds(&f).is_empty());
    }

    #[tokio::test]
    async fn a_failed_kick_is_recorded_with_the_alert_and_no_cash_event() {
        let f = fixture(Some(LAN)).await;
        let addr = "192.168.1.50:9100".parse().unwrap();
        f.fake.fail_with(Some(PrinterError::Refused(addr)));
        queued_drawer(&f, "01J000000000000000000000D1").await;
        task(&f).drain().await;

        let (s, error) = state(&f, "01J000000000000000000000D1");
        assert_eq!(s, "failed");
        assert!(error.unwrap().contains("refused the connection"));
        assert_eq!(alert_kinds(&f), ["printer_offline"]);
        assert_eq!(drawer_events(&f), 0);
    }

    #[tokio::test]
    async fn a_kick_with_no_printer_set_up_fails_the_row_and_sends_nothing() {
        let f = fixture(None).await;
        queued_drawer(&f, "01J000000000000000000000D1").await;
        task(&f).drain().await;
        assert_eq!(state(&f, "01J000000000000000000000D1").0, "failed");
        assert!(f.fake.sent().is_empty());
    }
}
