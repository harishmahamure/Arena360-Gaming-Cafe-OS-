---
title: 'Story 5.17: Choose print or none at settle, and print through the side-effect outbox'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: 'd2169b7'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-14-invoice-numbers-in-an-epoch-segmented-series-that-never-repe.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-16-receipt-tax-invoice-and-payment-receipt-layouts-for-58-and-8.md'
---

## Intent

**Problem:** A sale settles but nothing prints. The renderer (5.16) and the invoice series (5.14) exist, but counter staff cannot ask for a receipt, a printed bill can still be undone after it is in the customer's hand, and a missing printer has no defined effect on the sale.

**Approach:**

- The settle bar for a sale (a session bill, a counter sale, a plan sale) offers **Print · None**. The Hub answers `GET /ui/printer/ready` (any staff who may sell): with a printer chosen and no recent failure the bar shows "Printer ready" and preselects the branch default (`receipt.default`); otherwise Print is not offered and the bar says "No printer connected — nothing will print. Settle is never blocked."
- `session.settle`, `pos.sale` and `plan.sell` take `receipt: "print" | "none"` (absent means none). With `print`, `apply()` numbers the sale inside the settle transaction (`issue_number`, first numbering any older unnumbered sale whose Undo window has closed so the series stays in `settled_at` order), records the irreversible effects "invoice number issued" and "receipt printed" (so no Undo toast is shown and a correction is a Refund) and inserts a `print` row into the new `side_effect_outbox` table. No I/O runs inside `apply()`.
- `ReceiptPrintTask` (arena-tasks) reads queued rows at start and whenever an order changes, builds the receipt from the committed rows (`arena_core::printer::job`), sends it through the `Printer` adapter (USB, LAN 9100, Bluetooth; the recorded fake drives the tests) and applies the system intent `receipt.printed`, or `receipt.print_failed` with the error. A failure never touches the sale: it raises the "Printer offline" alert (sound and badge, from Story 3.25) once while one is open, and the order offers Reprint.
- `receipt.reprint {order_id}` (permission `pos.sell`) queues the same receipt marked "Reprint" for a settled order that has a number. The audit names who asked.

## Boundaries & Constraints

**Always:**

- The printer never blocks a settle: the server accepts `print` whatever the printer is doing, and a bar with no answer from `/ui/printer/ready` offers no Print and sends `none`.
- The outbox row is written in the settle transaction; the task sends it after commit. A restart prints what a crash left. A job that printed but could not be recorded is not sent again (the task keeps its outcome until the result intent applies).
- `receipt.printed` and `receipt.print_failed` are system-only, take no `base_rev`, and refuse (`STALE`) a row that is not queued. `receipt.reprint` refuses `NOT_FOUND` for no such order and `STALE` for an unsettled or unnumbered one.
- A result or a reprint touches the order row so the change log (and the audit) carries the actor and intent kind; the order is already irreversible then.
- Print is not offered for a wallet top-up or credit collection (payment receipts are built by the 5.16 renderer; their settle bars do not pass `receiptChoice`).
- `GET /ui/printer/ready` reports `offline` for `PRINTER_OFFLINE_HOLD_MS` (2 min) after the last failed job, then offers Print again; a job that works clears the failure. The failure state and the one-job-at-a-time lock are shared by the Settings pages and the task (`PrinterJobs`).

**Never:**

- Drawer kicks (5.18), WhatsApp receipts (5.19), B2B invoices (5.20), refunds and credit notes (5.24), the Z-report (5.28), payment receipts at the top-up and collection settle.
- I/O inside `apply()`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Print settle | `pos.sale` / `session.settle` / `plan.sell` with `receipt: print` | number issued in the same transaction, effects recorded, one queued `print` row | none |
| Print then Undo | Undo inside the window | refused | `UNDO_NOT_ALLOWED` |
| None / absent | `receipt: none` or no field | no number, no row, Undo works | none |
| Older sale due | unnumbered sale past its window | numbered first | none |
| Older sale in window | unnumbered, inside window | left alone (numbered later by the task) | none |
| Printer ready | printer chosen, no failure in 2 min | "Printer ready", Print · None, default preselected | none |
| No printer / offline | not chosen, or failed < 2 min ago | message, no choice, sends `none` | none |
| Print succeeds | queued row | `receipt.printed`, row `done` | none |
| Print fails | refused / timeout / no printer chosen | `receipt.print_failed`, row `failed`, one `printer_offline` alert | the sale stays settled |
| Reprint | numbered settled order | queued row `reprint: true`, receipt marked "Reprint" | `STALE` no number, `NOT_FOUND` |
| Reprint not allowed | staff without `pos.sell` | refused | `PERMISSION_DENIED` |
| Result twice | closed row | refused | `STALE` |

## Code Map

- `crates/arena-store/migrations/0051_side_effect_outbox.sql` -- NEW. `src/outbox.rs` -- NEW: `insert_outbox`, `list_queued_outbox`, `finish_outbox`, `latest_print_state`, `load_receipt_source` (order, lines, payments). `lib.rs` -- `Store::queued_prints`, `Store::receipt_source`; `sessions.rs` -- `PosSaleView.print_state`.
- `crates/arena-proto/src/{receipt,intent,printer,session,pos,plans}.rs` -- `ReceiptChoice`, `ReceiptReprint`, `ReceiptPrinted`, `ReceiptPrintFailed`, the three intent kinds, `receipt` on the three settle payloads, `PrinterReady`, `PrinterUnready`, `PRINTER_OFFLINE_HOLD_MS`.
- `crates/arena-core/src/apply/receipt_print.rs` -- NEW: load/validate/mutate of the three intents and `queue_on_settle`; wired in `mod.rs`, `mutate.rs`, `validate.rs`, `guards.rs`. `src/printer/job.rs` -- NEW: rows and settings to `SaleReceipt` and bytes. `src/printer/receipt.rs` -- `render_sale_copy` (the "Reprint" line). `src/printer/mod.rs` -- `PrinterJobs`. `src/snapshot.rs` -- `queued_prints`, `receipt_bytes`. `src/invoice/tax.rs` -- `RateTax` deserialises.
- `crates/arena-tasks/src/receipts.rs` -- NEW `ReceiptPrintTask`; `crates/arena-hub/src/lib.rs` spawns it and shares `PrinterJobs` with the UI router; `crates/arena-api/src/{ui_printer,ui_auth}.rs` -- `GET /ui/printer/ready`.
- `apps/hub/src/lib/{SettleBar.svelte,printer.ts,pos-sales.ts,tile-actions.svelte.ts,TileSheet.svelte,players/SellPlan.svelte,audit/labels.ts}`, `routes/pos/+page.svelte` (choice, Reprint in Recent sales), `routes/sessions/+page.svelte` (Reprint), `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/receipt.rs` (NEW), `printer/job.rs`, `arena-store` `outbox.rs`, `arena-tasks` `receipts.rs`, `ui_printer.rs` (`ready`), `proptest_apply.rs` (`PosSale.print`, `assert_prints_hold`), `tests/pos.spec.ts`, `tests/settle.spec.ts`, `tests/controllers-list.spec.ts`.

## Tasks & Acceptance

- [x] Store: migration 0051, outbox and receipt-source readers.
- [x] Protocol: the choice, the three intents, `ready`.
- [x] Core: print at settle inside the transaction; reprint; result intents; receipt builder; "Reprint" mark.
- [x] Task and Hub: print task, shared printer lock and failure state, `/ui/printer/ready`.
- [x] Hub UI: choice on the settle bar, no-printer message, no Undo toast for a printed bill, Reprint in Recent sales, strings, audit labels.
- [x] Tests: Rust matrix, task tests with the recorded fake printer, proptest op and invariant, Playwright.
- [x] Verification gate.

**Acceptance Criteria:**

- Given the settle bar with a printer available, then it shows "Printer ready" and Print · None with the branch default selected.
- Given a settle with Print, then the invoice number is issued inside the settle transaction, the bill is never undoable and no Undo toast shows.
- Given no printer or an offline one, then Print is not offered and the bar says "No printer connected — nothing will print. Settle is never blocked."
- Given a settle with Print, then after commit the print task sends the receipt and records `receipt.printed` or `receipt.print_failed`; no I/O runs in `apply()`.
- Given a print failure, then a "Printer offline" alert is raised, the sale stays settled and the order shows Reprint.
- Given a numbered order, then Reprint prints the same receipt marked "Reprint" and the audit records who.
- Given a transport other than USB or LAN, then it is selected through the same `Printer` interface and the recorded fake drives the tests.

## Verification

- `cargo test` for `arena-store`, `arena-proto`, `arena-core` (incl. `proptest_apply` at 200 cases), `arena-tasks` (the new `receipts` tests), `arena-api`, `arena-replay` pass; `cargo clippy -D warnings` on the touched crates and `cargo fmt --check` are clean; `pnpm gen:types`, `check:strings`, `check:protocol`, prettier and Hub `svelte-check` pass; Playwright `pos`, `settle`, `controllers-list` and ten more settle-touching specs pass (320+ tests).
- Failures that are not from this story and are unchanged by it: `arena-kiosk-svc` Windows lockdown, pipe and DPAPI tests (need admin), `arena-tasks` `a_printer_that_stops_reading_times_out` and `trusted_time_is_cloud_time_plus_monotonic_elapsed`, `arena-cloud` `a_hung_run_times_out_with_its_own_code`, `arena-e2e` (Unix sockets, does not build on Windows) and clippy on `arena-kiosk-svc/tests/discovery.rs`. `ui_auth::lock_and_unlock_log_the_staff_ids_and_never_the_pin` is flaky on the baseline too (global tracing subscriber).

## Notes for later stories

- Reprint is on POS Recent sales and the Sessions screen (`SessionListRow.invoice_no` / `receipt_order_id`); there is no open order left to reprint from once a bill settles, so the "open order" place in the epic text is the Sessions row of that bill.
- Top-up and credit-collection payment receipts are rendered (5.16) but not yet printed at their settle.
- 5.18 (drawer) adds a `drawer` outbox kind and its own result intent; the table already allows it.
- 5.19 queues a `notify_outbox` row (change-log table) separately.
