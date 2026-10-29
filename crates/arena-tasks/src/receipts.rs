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
//! A KOT (Story 7.5) is the same kind of job for a kitchen ticket: a `kot` row is marked
//! `sending` before its bytes go out, so a restart that finds one half-sent records
//! `kot.print_failed` for Reprint and never sends it twice, and the result is `kot.printed` or
//! `kot.print_failed`. The slip goes to the printer the KOT mode names.
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
    KEY_PRINTER_SELECTED, KotPrintFailed, KotPrinted, PrinterErrorKind, PrinterJobFailure,
    PrinterSelected, ReceiptPrintFailed, ReceiptPrinted, printer_error_text,
};
use arena_store::{KIND_DRAWER, KIND_KOT, OutboxRow, STATE_SENDING};
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
        (Outcome::Printed, _) if row.kind == KIND_KOT => (
            IntentKind::KotPrinted,
            IntentPayload::KotPrinted(KotPrinted { outbox_id }),
        ),
        (Outcome::Failed(error_kind, error), _) if row.kind == KIND_KOT => (
            IntentKind::KotPrintFailed,
            IntentPayload::KotPrintFailed(KotPrintFailed {
                outbox_id,
                error_kind: *error_kind,
                error: error.clone(),
            }),
        ),
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

/// The refund whose credit note a `print` row carries (Story 5.24), when it is one.
fn credit_note(row: &OutboxRow) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(&row.payload)
        .ok()
        .and_then(|v| {
            v.get("credit_note")
                .and_then(|c| c.as_str().map(str::to_owned))
        })
}

/// Whether a `print` row carries a shift's Z-report (Story 5.28): its `ref_id` is the shift id.
fn z_report(row: &OutboxRow) -> bool {
    serde_json::from_str::<serde_json::Value>(&row.payload)
        .ok()
        .and_then(|v| v.get("z_report").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
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

    /// Builds the KOT of a ticket and sends it to the printer the KOT mode names.
    async fn send_kot(&self, row: &OutboxRow) -> Outcome {
        let (ticket_id, reprint) = (row.ref_id.clone(), reprint(row));
        let now = self.clock.now_ms();
        let job = self
            .blocking("kot", move |r| r.kot_job(&ticket_id, reprint, now))
            .await;
        let Some(job) = job else {
            return Self::fail(PrinterErrorKind::Other, "the KOT could not be built");
        };
        let Some(job) = job else {
            return Self::fail(PrinterErrorKind::Other, "the ticket of this KOT is gone");
        };
        self.transmit(&job.selected, &job.bytes).await
    }

    /// Builds the receipt and sends it; the outcome is what the result intent records.
    async fn send(&self, row: &OutboxRow) -> Outcome {
        if row.kind == KIND_KOT {
            return self.send_kot(row).await;
        }
        let reprint = reprint(row);
        let credit_note = credit_note(row);
        let z = z_report(row);
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
                let bytes = if let Some(refund_id) = &credit_note {
                    r.credit_note_bytes(refund_id, now)?
                } else if z {
                    r.z_report_bytes(&order_id, reprint, now)?
                } else if drawer {
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
            return Self::fail(
                PrinterErrorKind::Other,
                "the order or shift of this receipt is gone",
            );
        };
        self.transmit(&selected, &bytes).await
    }

    /// Sends `bytes` to the printer saved as `selected`, one job at a time.
    async fn transmit(&self, selected: &str, bytes: &[u8]) -> Outcome {
        let selected = match PrinterSelected::parse(selected) {
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
        match self.printer.send(&target, bytes).await {
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
                rows.extend(r.queued_kots(PRINT_BATCH)?);
                rows.extend(r.sending_kots(PRINT_BATCH)?);
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
                None if row.kind == KIND_KOT && row.state == STATE_SENDING => {
                    // A send a restart cut short: the slip may or may not have come out, so it is
                    // never sent again; the ticket offers Reprint.
                    let o = Self::fail(
                        PrinterErrorKind::Other,
                        "the Hub restarted while this KOT was printing; reprint it if it did not come out",
                    );
                    self.unrecorded
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(row.id.clone(), o.clone());
                    o
                }
                None => {
                    if row.kind == KIND_KOT {
                        match self.writer.mark_outbox_sending(row.id.clone()).await {
                            Ok(true) => {}
                            Ok(false) => continue,
                            Err(e) => {
                                tracing::warn!(code = e.code.as_str(), "KOT not marked sending");
                                return true;
                            }
                        }
                    }
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
                        if a
                            .changes
                            .iter()
                            .any(|c| {
                                c.tbl == "orders"
                                    || c.tbl == "shifts"
                                    || c.tbl == "kitchen_tickets"
                            })
                        {
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

    /// A ticket with one line, and a KOT row on the outbox in `state`.
    async fn kot_queued(f: &Fixture, row: &str, state: &str, payload: &str) {
        let sql = format!(
            "INSERT OR IGNORE INTO kitchen_tickets (id, order_id, device_id, seat, number, stage,
                 placed_at, rev, created_at, updated_at)
             VALUES ('T1', '01K8Z3Y7QXW2R5T9V1B4N9MZD4Q', NULL, 'PC 07', 3, 'new', {T0}, 1, 0, 0);
             INSERT OR IGNORE INTO kitchen_ticket_lines (id, ticket_id, order_line_id, label, qty,
                 notes, rev, created_at, updated_at)
             VALUES ('TL1', 'T1', 'L1', 'Tea', 2, '[\"No sugar\"]', 1, 0, 0);
             INSERT INTO side_effect_outbox (id, kind, ref_id, payload, state, at)
             VALUES ('{row}', 'kot', 'T1', '{payload}', '{state}', {T0});"
        );
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&sql).unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_queued_kot_prints_the_slip_and_is_recorded_without_touching_the_ticket() {
        let f = fixture(Some(LAN)).await;
        kot_queued(
            &f,
            "01J000000000000000000000K1",
            "queued",
            r#"{"reprint":false,"by":""}"#,
        )
        .await;
        assert!(!task(&f).drain().await);

        let sent = f.fake.sent();
        assert_eq!(sent.len(), 1);
        let text = String::from_utf8_lossy(&sent[0].bytes).to_string();
        assert!(text.contains("KOT") && text.contains("PC 07") && text.contains("K-3"));
        assert!(text.contains("2 x Tea") && text.contains("No sugar"));
        assert!(text.contains("Order 9MZD4Q") && !text.contains("REPRINT"));
        assert_eq!(
            state(&f, "01J000000000000000000000K1"),
            ("done".to_owned(), None)
        );
        assert!(alert_kinds(&f).is_empty());
    }

    #[tokio::test]
    async fn a_kot_reprint_is_marked_and_a_second_drain_sends_nothing_more() {
        let f = fixture(Some(LAN)).await;
        kot_queued(
            &f,
            "01J000000000000000000000K1",
            "queued",
            r#"{"reprint":true,"by":"s"}"#,
        )
        .await;
        let t = task(&f);
        t.drain().await;
        t.drain().await;
        assert_eq!(f.fake.sent().len(), 1);
        assert!(String::from_utf8_lossy(&f.fake.sent()[0].bytes).contains("REPRINT"));
    }

    #[tokio::test]
    async fn a_refused_kot_is_recorded_failed_with_the_offline_alert() {
        let f = fixture(Some(LAN)).await;
        let addr = "192.168.1.50:9100".parse().unwrap();
        f.fake.fail_with(Some(PrinterError::Refused(addr)));
        kot_queued(
            &f,
            "01J000000000000000000000K1",
            "queued",
            r#"{"reprint":false,"by":""}"#,
        )
        .await;
        task(&f).drain().await;
        let (s, error) = state(&f, "01J000000000000000000000K1");
        assert_eq!(s, "failed");
        assert!(error.unwrap().contains("refused the connection"));
        assert_eq!(alert_kinds(&f), ["printer_offline"]);
    }

    #[tokio::test]
    async fn a_kot_a_restart_found_half_sent_is_marked_failed_and_never_sent_again() {
        let f = fixture(Some(LAN)).await;
        kot_queued(
            &f,
            "01J000000000000000000000K1",
            "sending",
            r#"{"reprint":false,"by":""}"#,
        )
        .await;
        task(&f).drain().await;
        assert!(f.fake.sent().is_empty());
        let (s, error) = state(&f, "01J000000000000000000000K1");
        assert_eq!(s, "failed");
        assert!(error.unwrap().contains("restarted"));
        assert_eq!(alert_kinds(&f), ["printer_offline"]);
    }

    #[tokio::test]
    async fn another_kot_printer_takes_the_slip_at_its_own_paper_width() {
        let f = fixture(Some(LAN)).await;
        let other = r#"{"kind":"lan","id_or_addr":"192.168.1.60:9100","name":"Kitchen"}"#;
        let sql = format!(
            "UPDATE settings SET value = 'another' WHERE key = 'kitchen.kot_mode';
             UPDATE settings SET value = '{other}' WHERE key = 'kitchen.kot_printer';
             UPDATE settings SET value = '58' WHERE key = 'kitchen.kot_paper_mm';"
        );
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&sql).unwrap();
                Ok(())
            })
            .await
            .unwrap();
        kot_queued(
            &f,
            "01J000000000000000000000K1",
            "queued",
            r#"{"reprint":false,"by":""}"#,
        )
        .await;
        task(&f).drain().await;
        let sent = f.fake.sent();
        assert_eq!(
            sent[0].target,
            PrinterTarget::Lan("192.168.1.60:9100".parse().unwrap())
        );
        let text = String::from_utf8_lossy(&sent[0].bytes).to_string();
        assert!(text.contains(&"-".repeat(32)) && !text.contains(&"-".repeat(33)));
    }

    /// A closed shift `SH1` with a settled cash sale in its window and a queued Z-report row.
    async fn queued_z(f: &Fixture, row: &str, payload: &str) {
        queued(
            f,
            "01J000000000000000000000R0",
            "O1",
            r#"{"reprint":false,"by":"s"}"#,
        )
        .await;
        let sql = format!(
            "INSERT INTO shifts (id, staff_id, opened_at, float_minor, carried_minor, state,
                 closed_at, counted_minor, expected_minor, variance_minor, closed_by, rev,
                 created_at, updated_at)
             VALUES ('SH1', 's', {open}, 1000, 0, 'closed', {close}, 5000, 5000, 0, 's', 1, 0, 0);
             INSERT INTO payments (id, order_id, method, amount_minor, reference, staff_id,
                 position, business_day, fy, rev, created_at, updated_at)
             VALUES ('P1', 'O1', 'cash', 4000, NULL, 's', 0, 'd', 'FY2026-27', 1, 0, 0);
             INSERT INTO side_effect_outbox (id, kind, ref_id, payload, state, at)
             VALUES ('{row}', 'print', 'SH1', '{payload}', 'queued', {T0});
             UPDATE side_effect_outbox SET state = 'done' WHERE id = '01J000000000000000000000R0';",
            open = T0 - 1000,
            close = T0 + 1000,
        );
        f.writer
            .submit(move |tx| {
                tx.execute_batch(&sql).unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_queued_z_report_prints_the_shift_report_and_is_recorded_on_the_shift() {
        let f = fixture(Some(LAN)).await;
        queued_z(
            &f,
            "01J000000000000000000000Z1",
            r#"{"z_report":true,"reprint":false,"by":"s"}"#,
        )
        .await;
        assert!(!task(&f).drain().await);

        let sent = f.fake.sent();
        assert_eq!(sent.len(), 1);
        let text = String::from_utf8_lossy(&sent[0].bytes).to_string();
        assert!(
            text.contains("Z-Report") && !text.contains("Reprint"),
            "{text}"
        );
        assert!(
            text.contains("40.00"),
            "the shift's cash sale is on it: {text}"
        );
        assert_eq!(
            state(&f, "01J000000000000000000000Z1"),
            ("done".to_owned(), None)
        );
        assert!(alert_kinds(&f).is_empty());
    }

    #[tokio::test]
    async fn a_reprinted_z_report_is_marked_and_a_failed_one_alerts_without_touching_the_shift() {
        let f = fixture(Some(LAN)).await;
        queued_z(
            &f,
            "01J000000000000000000000Z1",
            r#"{"z_report":true,"reprint":true,"by":"s"}"#,
        )
        .await;
        task(&f).drain().await;
        assert!(String::from_utf8_lossy(&f.fake.sent()[0].bytes).contains("Reprint"));

        let addr = "192.168.1.50:9100".parse().unwrap();
        f.fake.fail_with(Some(PrinterError::Refused(addr)));
        f.writer
            .submit(|tx| {
                tx.execute_batch(
                    "INSERT INTO side_effect_outbox (id, kind, ref_id, payload, state, at)
                     VALUES ('01J000000000000000000000Z2', 'print', 'SH1',
                             '{\"z_report\":true,\"reprint\":false}', 'queued', 1800000000000)",
                )
                .unwrap();
                Ok(())
            })
            .await
            .unwrap();
        task(&f).drain().await;
        assert_eq!(state(&f, "01J000000000000000000000Z2").0, "failed");
        assert_eq!(alert_kinds(&f), ["printer_offline"]);
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
    async fn a_drain_sends_the_kick_before_a_waiting_receipt() {
        let f = fixture(Some(LAN)).await;
        queued_drawer(&f, "01J000000000000000000000D1").await;
        queued(
            &f,
            "01J000000000000000000000P2",
            "O1",
            r#"{"reprint":false,"by":"s"}"#,
        )
        .await;
        task(&f).drain().await;

        let sent = f.fake.sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].bytes, b"\x1bp\x00\x19\xfa", "the pulse goes first");
        assert_ne!(sent[1].bytes, sent[0].bytes);
        assert_eq!(
            state(&f, "01J000000000000000000000P2"),
            ("done".to_owned(), None)
        );
        assert_eq!(drawer_events(&f), 1);
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

    /// Story 5.39: the rows a killed Hub left queued print once after the restart, and a second
    /// restart finds nothing to print again.
    #[tokio::test]
    async fn rows_left_queued_by_a_crash_print_once_across_restarts() {
        let f = fixture(Some(LAN)).await;
        for (row, order) in [
            ("01J000000000000000000000R1", "O1"),
            ("01J000000000000000000000R2", "O1"),
        ] {
            queued(&f, row, order, r#"{"reprint":false,"by":"s"}"#).await;
        }
        task(&f).drain().await;
        assert_eq!(f.fake.sent().len(), 2);
        // A restart is a new task over the same database.
        task(&f).drain().await;
        task(&f).drain().await;
        assert_eq!(f.fake.sent().len(), 2);
        for row in ["01J000000000000000000000R1", "01J000000000000000000000R2"] {
            assert_eq!(state(&f, row).0, "done");
        }
    }

    /// Story 5.39, the monthly hardware job (`.github/workflows/hardware-bench.yml`): a sale
    /// receipt, the Z-report and a drawer kick go through the real adapter to every printer in
    /// `ARENA_HW_PRINTERS`, a JSON list of `{kind, id_or_addr, name, paper}` (`paper` is the roll
    /// loaded, "58" or "80"). Each job must come back `done`. The outcome per printer is written
    /// to `ARENA_HW_OUT` (default `target/hardware-bench/printers.json`) for the compatibility
    /// list; whoever loads the paper checks the slips and the drawer by eye.
    #[tokio::test]
    #[ignore = "needs the test-café printers: set ARENA_HW_PRINTERS"]
    async fn hardware_bench_prints_receipts_and_a_z_report_and_kicks_the_drawer() {
        let list = std::env::var("ARENA_HW_PRINTERS").expect("ARENA_HW_PRINTERS is not set");
        let printers: Vec<serde_json::Value> = serde_json::from_str(&list).unwrap();
        assert!(!printers.is_empty(), "ARENA_HW_PRINTERS lists no printer");
        let mut results = Vec::new();
        let mut failed = Vec::new();
        for p in &printers {
            let text = |k: &str| p[k].as_str().unwrap_or_default().to_owned();
            assert!(
                matches!(text("paper").as_str(), "58" | "80"),
                "paper must be the string \"58\" or \"80\": {p}"
            );
            let selected = serde_json::json!({
                "kind": text("kind"),
                "id_or_addr": text("id_or_addr"),
                "name": text("name"),
            })
            .to_string();
            let mut outcome = serde_json::Map::new();
            for job in ["receipt", "z_report", "drawer"] {
                let f = fixture(Some(&selected)).await;
                let paper = text("paper");
                let sql =
                    format!("UPDATE settings SET value = '{paper}' WHERE key = 'printer.paper_mm'");
                f.writer
                    .submit(move |tx| {
                        tx.execute_batch(&sql).unwrap();
                        Ok(())
                    })
                    .await
                    .unwrap();
                let row = "01J000000000000000000000H1";
                match job {
                    "receipt" => {
                        queued(&f, row, "O1", r#"{"reprint":false,"by":"s"}"#).await;
                    }
                    "z_report" => queued_z(&f, row, r#"{"reprint":false,"by":"s"}"#).await,
                    _ => queued_drawer(&f, row).await,
                }
                let task = ReceiptPrintTask::new(
                    f.writer.clone(),
                    Arc::new(SnapshotReader::open(&f.path).unwrap()),
                    f.clock.clone(),
                    Arc::new(crate::printer::SystemPrinter::new()),
                    f.jobs.clone(),
                );
                task.drain().await;
                let (s, error) = state(&f, row);
                if s != "done" {
                    failed.push(format!(
                        "{} {job}: {}",
                        text("name"),
                        error.unwrap_or(s.clone())
                    ));
                }
                outcome.insert(job.into(), serde_json::Value::String(s));
            }
            results.push(serde_json::json!({
                "name": text("name"),
                "kind": text("kind"),
                "paper": text("paper"),
                "jobs": outcome,
            }));
        }
        let out = std::env::var("ARENA_HW_OUT")
            .unwrap_or_else(|_| "target/hardware-bench/printers.json".into());
        if let Some(dir) = std::path::Path::new(&out).parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&out, serde_json::to_string_pretty(&results).unwrap()).unwrap();
        assert!(failed.is_empty(), "printer jobs failed: {failed:?}");
    }
}
