---
title: 'Story 5.24: Refund a settled order with a credit note'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '498b3de2fbc1ee3dc5053cb2109a9a57a6dccb3f'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-23-void-an-unsettled-order-with-manager-pin.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A paid, numbered bill can only be corrected by hand. A settled order cannot be voided (Story 5.23), the invoice number is final once issued, and nothing records who approved paying money back or what it did to tax, stock, cash and credit.

**Approach:** `refund.create {invoice_no, lines[{line_id, qty, return_to_stock?}], method?, reference?, note?, receipt?}` (permission `refund.issue` on the actor or on the manager whose PIN elevated the intent; `reason`: `changed_mind`, `mistake`, `hardware_issue`, `other`) refunds selected lines of a settled, invoiced order against a credit note. The credit note takes the next number of the invoice series (same `(fy, segment)` counter). The refund is stored as a `refunds` row and as negative `payments` rows tied to it; the cash leaves the drawer as a negative `refund` cash event; ticked tracked lines return their units to stock (`return` moves); an unused pack or pass is deactivated. The Refund panel on the POS screen looks the invoice up (`GET /ui/refund/invoice?no=`) and submits.

## Boundaries & Constraints

**Always:**

- The order is found by its invoice number: an unknown number, or a sale still inside its Undo window (it has none), is `INVOICE_NOT_FOUND` (new code, HTTP 404). A refund takes no `base_rev`; the reason is required (`REASON_REQUIRED`) and `other` needs a note of 1 to 140 plain characters.
- What a line is worth is its share of what the order charged: the order total (or, on a sale with a per-rate tax breakup, each rate's pre-tax amount) is split over the lines by largest remainder in proportion to their gross amounts, so a coupon or manual discount is refunded pro rata. A part of a product line takes the half-up share; the last units take the rest. Every line refunded is exactly what was paid.
- Tax: a sale with a tax breakup (counter sales) recomputes tax once per rate on the refunded amounts (negative, Story 5.2); session and plan orders carry no tax and so does their refund. The credit note stores `tax_minor` (magnitude) and the negative per-rate `tax_breakup`.
- `REFUND_EXCEEDS_PAID` (new, 409) when the total is above the money taken on the invoice (positive payment rows) less earlier refunds; nothing is written. Quantities are limited by what is left of each line; play-time and plan lines are refunded whole.
- Credit first: the part of the original paid on credit and not yet cut by an earlier refund, capped at the player's outstanding credit, reduces outstanding (a negative `credit` ledger row, a negative `credit` payment). Only the rest goes back by `method`; `method` is not needed when credit absorbs everything.
- Methods: `cash` needs an open shift (`SHIFT_NOT_OPEN`) and writes a negative `refund` cash event; `upi` takes a 4-digit reference and `card` a terminal reference (the reference rules of Story 3.12); `wallet` needs a profiled player and adds to the wallet (`bill` ledger row, method `refund`); `credit` is refused.
- Plan lines: refused (`plan_used`, `plan_not_refundable`) when the owned pack or pass has been played on, otherwise refunded whole and the pack expires now with its minutes cleared. "Return to stock" applies to tracked product lines and bundles' tracked parts only.
- `receipt: print` queues the credit note on the side-effect outbox in the same transaction (a `print` row for the order carrying `credit_note: <refund id>`); `whatsapp` is refused. The credit note prints "Credit Note", its number, the original invoice number and date, negative amounts and tax per rate in the Story 5.16 layout.

**Never:**

- An Undo twin: a refund is final (the order's invoice number already makes its own Undo impossible).
- Refunding to credit, refunding a free line, or refunding more units than were sold.

## I/O & Edge-Case Matrix

| Scenario                        | Input / State                              | Expected                                              | Error                                                         |
| ------------------------------- | ------------------------------------------ | ----------------------------------------------------- | ------------------------------------------------------------- |
| Part of a line, cash, to stock  | 2 of 3 colas, cash, ticked                 | 2 units back, `refund` cash event, credit note number | none                                                          |
| Unknown / unnumbered invoice    | number not issued                          | refused                                               | `INVOICE_NOT_FOUND`                                           |
| Over-refund                     | total above paid less earlier refunds      | nothing written                                       | `REFUND_EXCEEDS_PAID`                                         |
| Cash, no shift                  | shift closed                               | refused                                               | `SHIFT_NOT_OPEN`                                              |
| Counter, no PIN                 | actor without `refund.issue`               | nothing written                                       | `PERMISSION_DENIED`                                           |
| Counter + manager PIN           | elevated                                   | `approved_by` = manager, `staff_id` = actor           | none                                                          |
| Credit sale                     | 8,000 of 12,000 on credit, refund 4,000    | outstanding down 4,000, no cash                       | none                                                          |
| GST sale                        | 5% and 18% lines                           | tax per rate, negative breakup                        | none                                                          |
| Discounted sale                 | 10% off 10,000                             | cola refunds 3,600, chips 5,400                       | none                                                          |
| Unused pack                     | plan line, pack unused                     | refunded 30,000, pack expired                         | none                                                          |
| Played pack                     | minutes used                               | refused                                               | `VALIDATION_FAILED` (`plan_used`)                             |
| Bad reference / method / reason | UPI "12", no method, credit, no reason     | refused                                               | `REFERENCE_REQUIRED` / `VALIDATION_FAILED` / `REASON_REQUIRED` |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0057_refunds.sql` -- rebuilds `payments` (`amount_minor <> 0`, `refund_id`), new `refunds` table, `order_lines.refunded_qty/refunded_minor`; `src/refunds.rs` -- invoice lookup, line and payment reads, inserts, `load_refund_invoice`, `load_credit_note_source`; `branch.rs` -- `Table::Refunds`, `RowKey::Refunds`; `outbox.rs` and `sessions.rs` -- receipt and Recent sales read positive payments only.
- `crates/arena-proto/src/refund.rs` -- `RefundCreate`, `RefundLineReq`, `RefundInvoice`, `RefundLineView`, `RefundSummary`; `intent.rs` -- kind `refund.create`, requirement `refund.issue`, `ReasonCode::REFUND`; `error.rs` -- `INVOICE_NOT_FOUND`, `REFUND_EXCEEDS_PAID`; `shift.rs` -- `CASH_REFUND`.
- `crates/arena-core/src/apply/refund.rs` -- plan (shared by validate and mutate), credit note number, rows, stock, credit, wallet, cash, plan; `printer/receipt.rs` and `job.rs` -- credit note layout and builder; `snapshot.rs` -- `refund_invoice`, `credit_note_bytes`.
- `crates/arena-tasks/src/receipts.rs` -- the print task builds a credit note when the row carries `credit_note`. `crates/arena-api/src/ui_refund.rs` -- `GET /ui/refund/invoice` (`pos.sell`).
- Hub: `lib/refund.ts`, `lib/RefundPanel.svelte`, `routes/pos/+page.svelte` (Refund beside Recent sales), `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/refund.rs` (13), store migration test, API test in `ui_auth.rs`, proptest `Op::Refund` and `assert_refunds_hold` (the invoice-series and cash invariants now count credit notes and refunds), replay seed refunds one sale, Playwright `refund.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] Store: migration 0057, refund reads and writes, `Table::Refunds`
- [x] Protocol: `refund.create`, reasons, error codes, views, `CASH_REFUND`, generated TS
- [x] Core: `refund.create` (plan, number, rows, stock, credit, wallet, cash, plan deactivation, print queue); credit note layout
- [x] API and print task: invoice lookup, credit note print
- [x] Hub UI: Refund panel with lookup, lines, reason, method, inline manager PIN
- [x] Tests: Rust matrix, migration, API, proptest op and invariants, replay, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given Refund and an invoice number, the invoice opens with its lines; an unknown number is refused `INVOICE_NOT_FOUND`.
- Given lines, quantities, a reason, a manager PIN, a method and per tracked line "Return to stock", when `refund.create` applies, then a `refunds` row and negative `payments` are written, tax is recomputed per rate, a credit note number is issued from the invoice series, stock returns with reason `return` where ticked, and a cash refund writes a `cash_events` row of kind `refund`.
- Given a total above paid minus earlier refunds, it is rejected `REFUND_EXCEEDS_PAID` and nothing is written.
- Given a refunded plan line with an unused pack or pass, it is deactivated; if partly used, it cannot be refunded and shows why.
- Given a credit note and a printer, it prints with "Credit Note", its number, the original invoice number and date, negative amounts and tax per rate.

## Spec Change Log

## Review Triage Log
Code review (2026-10-28): no blocking findings. fmt, clippy -D warnings, cargo test (proto, store, core, api, replay, kiosk-core) and Hub svelte-check pass.

## Design Notes

`payments` is rebuilt (no table references it) rather than given a second table so the existing per-order sums work with one filter: readers that mean "what was paid" take `amount_minor > 0` (receipt, Recent sales, the refund cap); the proptest's "payments sum to the total" does the same. A credit note shares the invoice counter, so `assert_invoices_hold` now collects credit note numbers with invoices into the same gap-free segments.

The refund's lines are stored as JSON on the credit note (label, quantity, net amount, HSN) so the print never depends on rows that may change; `order_lines.refunded_qty/minor` hold the running totals that cap later refunds.

The proptest also needed one fix that is not about refunds: its Undo-of-End check assumed the order always leaves the session, which stopped being true when Story 5.12 let products stay on it. It now accepts an order that holds only placed products.

## Verification

**Commands:**

- `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay`, `arena-tasks`, `arena-hub`, `arena-cloud`, `arena-kiosk-core`; `cargo test` for the same crates -- expected: pass. Failures that also occur on a clean checkout: `arena-cloud` `unreachable_at_start_retries_then_exits_1_without_logging_the_token` and the flaky `a_hung_run_times_out_with_its_own_code`; `arena-tasks` `printer::lan::tests::a_printer_that_stops_reading_times_out` and `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` (Windows timing). `arena-e2e` does not build on Windows (Unix sockets).
- `pnpm gen:types` (then `check:generated` once committed), `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`, Hub `pnpm check` (svelte-check); Playwright `refund.spec.ts`, `pos.spec.ts`, `audit.spec.ts`, `tile-sheet.spec.ts` in `apps/hub` -- expected: pass.
