---
title: "Story 5.20: B2B invoice with the customer's GSTIN"
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '836ec62'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-19-whatsapp-receipt-queued-to-the-outbox.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-16-receipt-tax-invoice-and-payment-receipt-layouts-for-58-and-8.md'
---

## Intent

**Problem:** A business customer cannot claim input tax credit on a Tax Invoice that carries no GSTIN of theirs, and nothing marks the sale as B2B for the GSTR-1 export.

**Approach:**

- `session.settle`, `pos.sale` and `plan.sell` take an optional `business` (`BusinessBuyer { gstin, legal_name }`). The Hub stores both on the order inside the settle transaction (`orders.buyer_gstin`, `orders.buyer_name`, migration 0053); an order with a `buyer_gstin` is B2B for GSTR-1 (Story 5.29), every other order is B2C.
- The settle bar offers a "Business invoice" toggle when `GET /ui/printer/ready` says `business: true` (the branch is `TaxMode::GstIn`). Turning it on shows Customer GSTIN and Legal name; Settle stays disabled, with the reason on the button, until both are valid or the toggle is off.
- The receipt (print and reprint) prints the legal name and `GSTIN <number>` directly under the "Tax Invoice" title (and "Reprint" mark), before the invoice number.

## Boundaries & Constraints

**Always:**

- The Hub re-checks everything: a branch that is not on GST refuses a `business` (`VALIDATION_FAILED`, field `business`); a GSTIN that is not 15 characters with a valid state code and check digit is `VALIDATION_FAILED` with field `business.gstin` and message `GSTIN_INVALID`; a legal name that is blank, padded, over 100 characters or has a control character is `VALIDATION_FAILED` with field `business.legal_name`. A refused settle rolls back whole (no sale, no stock move, no payment row, no change-log row).
- The same GSTIN rule as Settings → Tax (`gstin_valid`); the settle bar holds a TypeScript port of it (`apps/hub/src/lib/business.ts`) only to keep Settle disabled.
- A bill with no total takes no invoice number, so the buyer is accepted and not stored.
- The buyer is not an irreversible effect: Undo stays open, and a printed bill (number issued in the settle) is corrected by Refund as before.

**Never:**

- A buyer on a simple-tax branch, or printed on its receipt.
- The customer's GSTIN in a log line or telemetry.
- The GSTR-1 export itself (Story 5.29) and credit notes (Story 5.24).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Valid business, GST branch | settle with `business` | buyer stored on the order; receipt prints name + GSTIN under the title | none |
| Invalid GSTIN | check digit or shape wrong | refused, nothing written | `VALIDATION_FAILED`, `business.gstin`, `GSTIN_INVALID` |
| Bad legal name | blank, padded, over 100, control char | refused, nothing written | `VALIDATION_FAILED`, `business.legal_name` |
| Simple-tax branch | settle with `business` | refused | `VALIDATION_FAILED`, `business` |
| Settle bar, simple tax | `ready` has no `business` | no "Business invoice" | none |
| Settle bar, GST, toggle on, field invalid | typed GSTIN fails the check | inline `GSTIN_INVALID` text, Settle disabled | none |
| Toggle turned off | | fields hidden, Settle enabled, no buyer sent | none |
| Zero-total bill | `business` | settles, nothing stored | none |

## Code Map

- `crates/arena-proto/src/tax.rs` -- `BusinessBuyer` (+ `check()`), `BUYER_NAME_MAX_CHARS`; `session.rs`, `pos.rs`, `plans.rs` -- `business` on the three settle payloads; `printer.rs` -- `PrinterReady.business`.
- `crates/arena-store/migrations/0053_business_invoice.sql`, `src/invoices.rs` (`set_order_buyer`), `src/outbox.rs` (`ReceiptHead.buyer`, read by `load_receipt_source`).
- `crates/arena-core/src/apply/business_invoice.rs` -- `record_on_settle`, called first by `receipt_print::queue_on_settle` for every settle; `src/printer/receipt.rs` (`Buyer`, `SaleReceipt.buyer`, layout), `printer/job.rs` (buyer only when `gst`).
- `crates/arena-api/src/ui_printer.rs` -- `ready` adds `business`.
- `apps/hub/src/lib/SettleBar.svelte`, `business.ts`, `tile-actions.svelte.ts`, `TileSheet.svelte`, `players/SellPlan.svelte`, `routes/pos/+page.svelte`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/receipt.rs` (5), `printer/receipt.rs` (2), proto `tax.rs` (1), store `migrations.rs` (1), `ui_printer.rs` (1), Playwright `settle.spec.ts` (3) and `business.unit.spec.ts` (3).

## Tasks & Acceptance

- [x] Protocol: `BusinessBuyer` and its check, `business` on the three settle payloads, `PrinterReady.business`.
- [x] Store: migration 0053, `set_order_buyer`, buyer on the receipt head.
- [x] Core: validate and store the buyer at settle (GST only); print it under the title.
- [x] API: `ready` answers `business`.
- [x] Hub UI: Business invoice toggle, GSTIN and legal name fields, Settle gated; POS, tile sheet and plan sale pass the buyer.
- [x] Tests: Rust matrix, receipt layout, migration, Playwright, TS unit.
- [x] Verification gate.

**Acceptance Criteria:**

- Given a GST branch's settle bar, when staff choose "Business invoice", then fields for the customer GSTIN (format-checked with check digit) and legal name appear and are stored on the order at settle.
- Given an invalid GSTIN, when staff settle, then the field shows the `GSTIN_INVALID` text inline and Settle stays disabled until it is fixed or cleared; the Hub refuses the same intent with `GSTIN_INVALID`.
- Given a settled B2B order, when its receipt renders, then it prints the customer's legal name and GSTIN under the title, and the order carries `buyer_gstin` (the B2B flag for GSTR-1).
- Given a simple-tax branch, when the settle bar renders, then "Business invoice" is not offered and the Hub refuses a `business`.

## Verification

- `cargo test` for `arena-proto`, `arena-store`, `arena-core` (lib and `proptest_apply` at the default cases), `arena-api`, `arena-replay`, `arena-hub` pass; `cargo clippy -D warnings` (workspace except `arena-e2e`, which does not build on Windows, and the existing `arena-kiosk-svc` `discovery` test warnings) and `cargo fmt --check` are clean; `pnpm gen:types`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, prettier and Hub `svelte-check` pass; Playwright `settle` and `business.unit` pass.

## Notes for later stories

- Story 5.29 (GSTR-1) reads B2B from `orders.buyer_gstin IS NOT NULL` (indexed by `orders_b2b`); place of supply for a B2B row is the first two characters of the buyer's GSTIN. Story 5.24 (credit note) should copy the buyer onto the credit note.
- The `proptest_apply` generator does not yet draw a `business` (it would need a GST branch); the Rust matrix covers all three settle intents.
