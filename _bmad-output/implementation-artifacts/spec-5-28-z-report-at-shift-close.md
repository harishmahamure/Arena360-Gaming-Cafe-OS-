---
title: 'Story 5.28: Z-report at shift close'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
baseline_commit: '60e0f1227d9d9e608376c9e3cfabd607872f53e9'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-26-close-a-shift-with-a-blind-count-and-a-signed-variance.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-27-reports-screen-with-daily-sales-and-sessions.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A shift closes with a count (Story 5.26) but leaves no end-of-shift report, so nobody can see at a glance what the shift did to the money, and nothing prints at close.

**Approach:** `shift.close` builds a Z-report from the stored rows of that shift inside the close transaction and stores it on the `shifts` row (`z_report`, JSON), so the screen, the print and every later view show the same figures. When a printer is chosen the close queues a `print` outbox row for it (Story 5.17 outbox, Story 5.16 layout); with none, nothing is queued and the close completes. Reports gains a Shifts entry (`GET /ui/reports/shifts`, `GET /ui/reports/shifts/{id}`, `report.view`) with Reprint (`shift.reprint_z`, `report.view`).

## Boundaries & Constraints

**Always:**

- A shift's rows are: its `cash_events` (by `shift_id`), and, by time between `opened_at` and the close time, settled `session`/`plan_sale`/`pos_sale` orders (`settled_at`), their positive payments, top-ups and credit collections (payments, not sales), refunds (`refunds.created_at`), waste moves (`stock_moves.waste_reason` set, `at`) and voids (`voided_at`). An undone settle is not `settled`, so reversed pairs net to zero.
- Sales by type (play time, products, plans, discounts, tax added) reuse the Story 5.27 fold so they add up to the stored totals. Payments by method give amount and count; a "UPI and card references" section after them lists each UPI and card payment row (time, invoice number or empty, amount, reference) grouped by method. Refunds are negative lines (credit note no., method, amount). Expenses, deposits and drawer opens come from `cash_events`; expected, counted, variance, reason and approver from the closed shift row.
- The stored Z-report is final. A shift closed before this story has none: the read builds it from the rows at that moment (marked by no `closed_at` change) and the print does the same.
- Printing never blocks or fails the close: queued only when `printer.selected` parses to a printer; the print task's `receipt.printed` / `receipt.print_failed` close the row and a failure raises the one `printer_offline` alert. The row's `ref_id` is the shift id and its payload carries `"z_report": true`; the result intents touch the `shifts` row.
- The close action reads "Close shift · print Z-report".

**Never:**

- Writing from the report read routes, changing `cash_events`, putting the expected cash anywhere before the count (the Z-report exists only after the close), I/O inside `apply()`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fixture shift | cash + UPI sales, an undone sale, a cash refund, an expense, a deposit, waste | sales net of the undo; refunds negative; cash payments equal the cash sale events; expected = sum of `cash_events` | none |
| UPI and card | UPI ref `1234`, card ref `T-9` | each listed with time, invoice no., amount, reference under its method | none |
| Printer chosen | close | one `print` row, `ref_id` = shift id, printed in the 5.16 layout | printer down: row failed, alert, close stays |
| No printer | `printer.selected` empty | no row, no alert | none |
| Past shift | open Reports → Shifts → row | stored figures; Reprint queues a new row | unknown shift: `NOT_FOUND`; open shift: `STALE` |
| Old shift | closed before 5.28 | built from rows on read | none |
| No `report.view` | counter role | API 403, list says so | `PERMISSION_DENIED` |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0060_z_report.sql` -- `shifts.z_report TEXT NULL`; `src/shifts.rs` -- `ShiftClosing.z_report`, `list_shifts`, `find_shift`; `src/zreport.rs` (new) -- `ZReportRows` loader over a shift and a time window; `src/outbox.rs` -- unchanged kinds (`print`).
- `crates/arena-proto/src/report.rs` -- `ZReport` and parts, `ShiftListRow`; `intent.rs` -- kind `shift.reprint_z` (copy every `ReceiptReprint` site); `receipt.rs` -- `ZReportReprint`.
- `crates/arena-core/src/reports.rs` -- `z_report(rows, tz)`; `apply/shift.rs` -- build + store + queue in `mutate_close`; `apply/receipt_print.rs` -- reprint, result intents touch the shift for a Z row; `printer/receipt.rs` -- `render_z_report`; `printer/job.rs` -- `z_report_bytes`; `snapshot.rs` -- `shift_list`, `z_report`, `z_report_bytes`.
- `crates/arena-tasks/src/receipts.rs` -- print task renders a Z row via `z_report_bytes`.
- `crates/arena-api/src/ui_report.rs` + `ui_auth.rs` routes -- the two shift routes.
- Hub: `lib/reports.ts` (Shifts entry, loaders), `routes/reports/shifts/+page.svelte` (list), `routes/reports/shifts/[id]/+page.svelte` (Z-report + Reprint), `lib/CloseShiftPanel.svelte` (label), `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: core `apply/tests/z_report.rs` (golden fixture), `reports.rs` unit, `receipt.rs` golden, tasks print test, API test, Playwright `reports.spec.ts` + `shift-close.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] Store: migration 0060, loader, shift list/find, store the report at close
- [x] Protocol: `ZReport`, `ZReportReprint`, `shift.reprint_z`, generated TS
- [x] Core: `z_report` fold, build/store/queue at close, reprint intent, result intents, renderer + golden
- [x] Tasks: print task renders Z rows
- [x] API: shift list and Z-report routes
- [x] Hub UI: Shifts entry, list, Z-report screen with Reprint, close label
- [x] Tests: golden fixture, edge matrix, print task, API, proptest op, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given a shift closes, then its stored Z-report holds sales by type, payments by method with counts, the UPI and card references section, expected vs counted with variance and reason, waste by reason, expenses, deposits, refunds as negative lines, discounts, voids and drawer opens.
- Given a printer is chosen, when the close commits, then the Z-report prints through the outbox in the 5.16 layout; with none, nothing prints and the close completes.
- Given Reports, then it lists Shifts; opening a past shift shows its Z-report with Reprint and the same figures as printed.
- Given a fixture shift with undone sales, refunds and an override sale, then reversed pairs net to zero and the totals equal the shift's payments and `cash_events`.

## Spec Change Log

## Review Triage Log

## Design Notes

The report is stored at close because an Undo after close writes its compensating row against the sale's own shift (Story 5.10), which would silently move a live read; the stored JSON keeps "equals what was printed" true. Windows never overlap (one open shift), so time attribution of non-cash payments is unambiguous.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-tasks`, `arena-replay`, `arena-kiosk-core`; `pnpm gen:types`, `check:generated`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `reports.spec.ts shift-close.spec.ts` in `apps/hub`.

## Review Triage Log (code review)

- Fixed: undone waste was counted as a second positive waste line (store query now signed, net-zero rows dropped; test added).
- Fixed: Z-report page did not reload when the route id changed (now an `$effect`); refunds keyed by index; a variance note with no reason now shows on screen; note asserted end to end.
- Deferred: inclusive `BETWEEN` window edges at a same-millisecond close/open; Shifts list capped at 100 with no pagination; reprint error mapping is coarse; print-task wake-up on `shifts` has no `run()`-level test.
