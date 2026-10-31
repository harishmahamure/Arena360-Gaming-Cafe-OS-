---
title: 'Story 10.7: Every subscription charge gets a GST tax invoice from Arena 360'
type: 'feature'
created: '2026-10-31'
status: 'in-review'
baseline_revision: '9cfda95c71efdd68db9a8226300d07c419ac9be7'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-6-a-paid-licence-reaches-the-hub-within-one-heartbeat-and-staf.md'
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** A `subscription.charged` (Story 10.3) moves the subscription and entitles the branch, but the owner gets no tax invoice: nothing records GST, the buyer's GSTIN or a number they can claim input credit against.

**Approach:** The webhook/reconcile transaction also issues one immutable `billing_invoices` row per charge (number `A360/<FY>/<seq:06>`, gap-free per FY), priced from the subscription's locked price book. A jobs task renders each new invoice to a PDF under the billing R2 prefix and tells the owner "Invoice ready" through the owner-alert (messaging adapter) path.

## Boundaries & Constraints

**Always:** the invoice row is written in the same transaction as the billing event and the transition, keyed by the event, so a repeat delivery issues nothing. FY is Apr-Mar in Asia/Kolkata. GST is computed once on the subtotal (18%, half-up to paise, from the quote); CGST + SGST (tax split in two, the odd paisa to SGST) when buyer state equals Arena 360's state, else IGST. Band MY/SG/US: band currency, no GST lines, note "Export of services - zero-rated under LUT". PDFs live under `billing/invoices/`, never a branch prefix. Money is minor units; the invoice never changes after issue (only `pdf_key`, `pdf_at`, `notified_at` are set later).

**Never:** no credit notes (Story 10.15), no proration lines (10.9), no Owner-app invoice list or download link (10.8), no change to the webhook transitions or the heartbeat/licence wire shapes, no Razorpay amounts read from events.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| First charge, band IN, same state | GSTIN state 27 = seller 27 | `A360/2026-27/000001`: lines, subtotal, CGST 9% + SGST 9%, buyer GSTIN/name/state, SAC 998314 |
| Different state | GSTIN state 29 | IGST 18%, no CGST/SGST |
| No GSTIN | branch unregistered | place of supply = seller's state (CGST+SGST); buyer GSTIN blank |
| Band MY/SG/US | charged | band currency, no GST, LUT note |
| Same event twice, or reconcile after webhook | duplicate event id / period not newer | exactly one invoice |
| Renewal | later period | next number; no gap |
| FY rollover | 31 Mar 23:59 IST vs 1 Apr 00:00 IST | `2025-26` vs `2026-27`, sequence restarts at 000001 |
| Add-ons | devices_20 x2 + tunnel | one line per item with quantity |
| Jobs run | invoice with no `pdf_key` | PDF written, one `invoice_ready` alert; a re-run sends nothing |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0036_billing_events.sql` -- model for the new `0037_billing_invoices.sql`; register it in `crates/arena-store/src/control.rs` (migration list) and the schema-version test.
- `crates/arena-store/src/control/billing.rs` -- `apply_billing_event` / `change_stmts`: add an optional `NewInvoice` statement set (seq via `INSERT .. SELECT MAX(seq)+1`), a unique-violation on `billing_invoices` maps to `Conflict`; new `control/invoice.rs` for reads (`invoice`, `invoices_for_event`, `invoices_to_render`, `set_invoice_pdf`, `mark_invoice_notified`).
- `crates/arena-cloud/src/webhook.rs` -- `Decision.charged`; `handle` builds the draft (quote from `subscription::quote_for` / `price_book_items`, branch + tenant for buyer) before the transaction.
- `crates/arena-cloud/src/subscription.rs` -- `quote_items`, `quote_for`, `renews_at` reused; `gstin.rs` for the state code.
- `crates/arena-cloud/src/invoice.rs` (new) -- `fy_of`, `Seller` (env `ARENA_INVOICE_SELLER_*`, dev default), `draft`, minimal text PDF writer, `render_job`.
- `crates/arena-cloud/src/{config.rs,backup.rs}`, `tests/config.rs` -- `Task::InvoiceRender` (`invoice-render`).
- `crates/arena-cloud/src/owner_alerts.rs`, `crates/arena-proto/src/owner_alert.rs`, `packages/ui/src/generated/OwnerAlertKind.ts` -- cloud-sourced `InvoiceReady` kind (`number`, `amount`).
- `crates/arena-cloud/src/offline_watch.rs` -- pattern for a job that builds `AuthState` and raises alerts.
- Tests: store invoice tests, cloud `tests/billing_events.rs` additions, `invoice` unit tests, `tests/owner_alerts.rs`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store/control-migrations/0037_billing_invoices.sql` -- `billing_invoices` (spec columns plus `seq`, `tenant_id`, `billing_event_id` unique, `buyer_name`, `buyer_state_code`, `seller_gstin`, `period_start`, `period_end`, `pdf_at`, `notified_at`), unique `number` and `(fy, kind, seq)`, triggers refusing edits/deletes of issued fields
- [x] `crates/arena-store/src/control/{billing,invoice}.rs`, `control.rs` -- transactional issue and reads
- [x] `crates/arena-cloud/src/invoice.rs`, `webhook.rs` -- build and issue the invoice on a charge
- [x] `crates/arena-proto/src/owner_alert.rs` (+ generated TS) -- `InvoiceReady`
- [x] `crates/arena-cloud/src/{invoice.rs,config.rs,backup.rs}` -- `invoice-render` job: PDF to R2, owner alert once per invoice
- [x] Tests for every I/O row; README billing section

**Acceptance Criteria:**
- Given the migration ran, when the schema is read, then `billing_invoices` exists with unique `number`, and an update of money, number or buyer columns is refused.
- Given an issued invoice, when the jobs task runs twice, then one PDF exists under `billing/invoices/` and exactly one "Invoice ready" alert was raised.
- Given a charge whose price book is missing a price, when the event is handled, then nothing is recorded and the call errors so the webhook is retried.

## Design Notes

Unknown buyer state (no GSTIN) falls to the supplier's state, the IGST Act default when the recipient's address is not on record. `cgst = tax / 2`, `sgst = tax - cgst` keeps the pair equal to the once-computed 18%. If `ARENA_INVOICE_SELLER_GSTIN` is unset the dev seller is used with an error log; production sets all `ARENA_INVOICE_SELLER_*`.

## Verification

**Commands:**
- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud --all-targets -- -D warnings` -- clean
- `cargo test -p arena-proto -p arena-store -p arena-cloud` -- pass apart from documented baseline failures

## Auto Run Result

Status: review. fmt and clippy clean; `cargo test -p arena-proto -p arena-store -p arena-cloud` passes except baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token` (`backup::a_hung_run_times_out_with_its_own_code` is flaky on Windows and passed on re-run). Every I/O matrix row is covered in `tests/billing_events.rs` and `invoice.rs` unit tests.

Risks to know: `arena-store` sqld tests skip locally (no `sqld`); `OwnerAlertKind.ts` was edited by hand (run `check:generated`); `control` and `billing-reconcile` roles both need the `ARENA_INVOICE_SELLER_*` variables (not yet in `deployment.yaml`); a payment held for review is invoiced, its "Invoice ready" alert waits for entitlement; the alert links to `/alerts` until Story 10.8 adds the invoice list; `seller_name`/`seller_address` columns were added so the PDF re-renders from the row alone.
