---
title: 'Story 5.16: Receipt, tax invoice and payment receipt layouts for 58 and 80 mm'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: '0fe1b46'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-15-receipt-and-notification-settings.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-27-printer-detection-and-test-print-at-the-hub.md'
---

## Intent

**Problem:** The Hub can print a test page but has no renderer that turns a settled sale or a payment into a receipt that carries what the law needs on 58 and 80 mm paper.

**Approach:** A pure `arena_core::printer::receipt` module. `render_sale(&ReceiptSettings, &SaleReceipt)` and `render_payment(&ReceiptSettings, &PaymentReceipt)` return ESC/POS bytes. No database, clock or printer is touched; Story 5.17 builds the inputs from committed rows and sends the bytes through the side-effect outbox.

## Decisions

- **Font and text.** Font A (`ESC M 0`), code page PC437 (`ESC t 0`). Anything outside printable ASCII prints as `?`, so money uses an ASCII symbol: `Rs` for the rupee, the ISO code where the symbol has no ASCII form (`JPY`, `AED`). Amounts keep the branch's grouping (`1,23,456.78`) and symbol side; a negative amount has a leading `-`.
- **Sale layout.** Branch name bold double-width/height (wrapped at half the width), then address, `GSTIN <no>` (GST branches only) and phone centred; a `-` rule; title "Tax Invoice" (GST) or "Invoice"; invoice no. (left off while none is issued); date and time in the branch zone; staff (left off when empty); customer or "Guest"; play-time lines (`device`, `SAC` on GST, `N min x rate/h`, amount); item lines; subtotal; tax per rate (GST: `Taxable r%`, `CGST r/2%`, `SGST r/2%`; simple: `<label> r%`); TOTAL bold double-height with the symbol; payments by method (`Cash`, `UPI ****1234`, `Card ref <ref>`, `Wallet`, `On credit` for the remainder); footer wrapped and centred.
- **80 mm items** sit in columns (name, Qty, Rate, Amount; amounts right-aligned) inside 48 characters, with `HSN` under the name on GST branches. Number columns grow to fit the widest amount; when that leaves the name column under 12 characters the item stacks instead.
- **58 mm items** stack: the name on its own line (wrapped at 32), `HSN`, then `qty x rate` and the amount right-aligned below. A line that cannot fit left and right text puts the right text on the line below.
- **Payment receipt** (wallet top-up or credit collection): "Payment Receipt", date, staff, player, kind, AMOUNT, payments by method; a collection adds "Bills cleared" (invoice no. or bill date and the amount cleared, from the Story 3.19 `bill.settled` data) and "Outstanding" after. It has no invoice number and no GSTIN.
- **Wrapping** matches the Story 5.15 preview (`apps/hub/src/lib/receipt.ts`): word boundaries, a word longer than the line is cut.
- **Shared formatter.** `locale::format_number` is the grouping code extracted from `format_minor`; both use it.
- **Out of scope.** Loading rows into `SaleReceipt`/`PaymentReceipt`, the print choice, the outbox and the drawer (5.17, 5.18); credit notes (5.24, which reuse this layout with negative amounts); the Z-report (5.28).

## Tasks & Acceptance

- [x] `arena-core/src/printer/receipt.rs`: settings, input types, layout, ESC/POS encoding.
- [x] `locale.rs`: `format_number` extracted.
- [x] Golden tests: GST multi-rate, simple tax and a 0-exponent currency (`JPY`) sales, a top-up and a collection, each at 58 and 80 mm (`src/printer/golden/*.escpos.txt`, control bytes spelled out; `UPDATE_GOLDEN=1` rewrites them); every row of every fixture is checked against the width (name rows at half) and for PC437 safety.
- [x] Unit tests: tax invoice fields, simple invoice with none of them, no GSTIN off GST, columns versus stacked, very long amounts and names, payment receipt without invoice number, bold/size control bytes, money formatting, percent, wrapping, branch-local date.
- [x] Verification gate.

## Verification

- `cargo test -p arena-core` (825 unit tests pass), `cargo clippy -p arena-core --all-targets -D warnings`, `cargo fmt --check`. No TypeScript or protocol change, so the Hub checks are unaffected.

## Review Triage Log

- Not yet reviewed.
