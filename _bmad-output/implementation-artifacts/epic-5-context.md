# Epic 5 Context: POS, inventory, tax-compliant receipts and shift close

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Staff sell products and bundles at the Hub, keep stock, issue GST invoices, refunds and receipts, and close a shift with a blind count and Z-report. The epic also delivers the Reports screen, CSV/PDF exports and CSV/template/branch-setup imports. It turns the Epic 3 settle path into a tax-compliant, cash-accountable point of sale.

## Stories

- Story 5.1: Tax settings (GST registration, rates, inclusive/exclusive, simple tax outside India)
- Story 5.2: Tax computed once per rate on every bill, in minor units
- Story 5.3: Products with categories, barcodes, images and kitchen flag
- Story 5.4: Bundles priced on their own
- Story 5.5: Stock-in with expiry lots and weighted average cost
- Story 5.6: Record waste with a reason
- Story 5.7: Expiry alerts; expired stock held from sale until written off
- Story 5.8: Low-stock thresholds and alerts
- Story 5.9: POS screen (product grid, search or scan, cart)
- Story 5.10: Open a shift with confirmed float; cash needs an open shift
- Story 5.11: Counter walk-up sale settles and takes stock
- Story 5.12: Attach an order to a running session
- Story 5.13: Take stock at served; mark served; cancel unserved lines into waste
- Story 5.14: Epoch-segmented invoice numbers that never repeat
- Story 5.15: Receipt and notification settings
- Story 5.16: Receipt, tax invoice and payment receipt layouts (58/80 mm)
- Story 5.17: Print or none at settle, via side-effect outbox
- Story 5.18: Cash drawer kicks on cash sales
- Story 5.19: WhatsApp receipt queued to outbox
- Story 5.20: B2B invoice with customer GSTIN
- Story 5.21: Coupons applied at settle
- Story 5.22: Manual discount with manager PIN and reason
- Story 5.23: Void an unsettled order with manager PIN
- Story 5.24: Refund a settled order with a credit note
- Story 5.25: Cash deposits and expenses
- Story 5.26: Close a shift with blind count and signed variance
- Story 5.27: Reports screen with daily sales and sessions
- Story 5.28: Z-report at shift close
- Story 5.29: GSTR-1 export
- Story 5.30: Players and staff reports
- Story 5.31: Stock reports with waste by reason
- Story 5.32: Export every Hub report to CSV and PDF
- Story 5.33: CSV import with dry-run, starting with products
- Story 5.34: Import players and plans by CSV
- Story 5.35: Seed plans and products from a starter template
- Story 5.36: Export and import a branch setup file
- Story 5.37: Paid Hubs keep setup template in the cloud
- Story 5.38: Second branch starts from tenant's existing setup
- Story 5.39: POS and money proven under load, offline and on real hardware

## Requirements & Constraints

- **Invoice timing:** when the receipt choice is Print (offered only while a printer is available), the invoice number is issued inside the settle intent, so a printed bill never shows Undo and is corrected by Refund. Otherwise a system `invoice.issue` assigns the number when the 8 s Undo window closes. Invoice number issued, receipt printed and drawer kicked are all registered irreversible effects.
- **Cash/shift rule:** from Story 5.10, any cash payment row (settle, top-up, collection, refund) needs an open shift (`SHIFT_NOT_OPEN`) and writes `cash_events`. Non-cash methods never need a shift.
- **Tax mode:** Epic 3 bills were tax-inclusive with no tax line; from Story 5.2 every bill carries a per-rate tax breakup. GST logic (series, rate split, rounding, GSTIN) applies only when `branch.tax_mode = 'gst_in'` (GST-registered Indian branch); other branches use a simple tax with a configurable label and rate, the same invoice series shape and a generic receipt layout.
- Wallet top-ups and credit collections are payments, not sales: they get a payment receipt, never an invoice number.
- GSTIN must be format-checked (15 chars, valid state code and check digit). The invoice prefix is the branch code, locked after the first invoice.
- A missing printer never blocks a sale. Out-of-stock and expired items cannot be sold; a short bundle refuses unless overridden with a manager PIN at POS.
- **Out of scope for this epic:** KOT and kitchen display (Epic 7; here only the kitchen flag and Mark served), kiosk Shop and HUD "ordered" (Epic 6; here availability is published and "Ordered" shows on the Epic 3 minimal session screen), Bluetooth printing (Epic 4; adapter exposes the transport interface with USB and LAN 9100), sending WhatsApp receipts and owner alerts for variance/low stock (Epic 8 drains the outbox), yearly GST archive and orphaned-invoice reconciliation (Epics 8-9), player erasure (Epic 9; the players report only shows how an erased player appears).

## Technical Decisions

- Builds on the Epic 3 settle path: `orders`, `order_lines`, `payments`, `settle-bar`, `payment-split`, and the `bill.settled{order_id}` hook with `settled_at` and `undo_until` = settled_at + 8 s; the Story 3.14 irreversible-effect guard; the Story 2.27 printer adapter; the Story 2.13 `lock_branch_code()` function and `branch.bootstrap` profile; the Story 3.25 alert service.
- Invoices and credit notes use the series `<branch code>/<FY>/E<counter><instance tag>/<seq>`, issued inside `apply()`, never reused across epochs. Orphaned old-epoch invoices are never dropped; they are listed as pending reconciliation.
- Tax: sum lines per rate, compute tax once per rate, round to minor units once, half-up. Totals must match across all surfaces.
- Side effects (print, drawer kick, notifications) run after commit from a local outbox and record result intents (`receipt.printed`, `drawer.kicked`, `notify.sent`); no I/O inside `apply()`. Notify requests carry `not_before` = commit + undo window and are skipped if the forward intent is reverted.
- Every story adds its new intents and their `.reverted` twins to the Story 1.8 `proptest` generator.
- The Reports screen is created by Story 5.27; each later report story adds its entry to it.

## UX & Interaction Patterns

- POS uses `pos-product-grid` (categories and favourites first, one combined search-or-scan field, a keyboard-wedge scan adds the item: 1 scan + 1 tap), `cart`, and `settle-bar`. Below 840 px wide, `phone-cart-bar` opens a full-height cart and settle sheet; Esc or Back closes it with the cart kept.
- A cart can attach to a running session. The attached player shows in the cart header with a "Change" link; "Guest" is one tap away.
- Bill breakdown order: session lines, orders, coupon, discount, subtotal, tax (inclusive/exclusive per branch), total, then Paid so far and Remaining.
- Shift close is a blind count with a signed variance. Manager/Owner PIN (`inline-pin`) gates overrides, discounts, voids and refunds.

## Cross-Story Dependencies

- 5.2 (tax breakup) underpins 5.14 to 5.20, 5.24 and 5.29; 5.1 settings feed 5.2.
- 5.3/5.4 (products, bundles) and 5.5 (stock lots) precede 5.6-5.8 and 5.9-5.13.
- 5.10 (open shift) gates all cash handling in 5.11, 5.18, 5.24, 5.25, 5.26 and the Z-report in 5.28.
- 5.14 (invoice series) precedes 5.16, 5.17, 5.20, 5.24 and 5.29; 5.17 and 5.18 use the outbox.
- 5.27 (Reports screen) precedes 5.28-5.32; 5.32 exports all reports.
- 5.33 (CSV import) precedes 5.34-5.36; 5.37 and 5.38 build on 5.36.
- 5.39 verifies the whole epic end to end.

## Story 5.10 (Hub-B lane): open a shift with a confirmed float, and cash needs an open shift

- **Decisions taken.** One open shift per branch (partial unique index plus `SHIFT_ALREADY_OPEN`). `shift.open` stores the confirmed float and the carried one (the last shift's opening float) on the `shifts` row so the audit shows both. `cash_events` is append-only; kinds `float_in`, `sale`, `sale_reverted` (a compensating row, `reverses` the sale, against the sale's own shift). The gate lives where each intent validates its payments (`SettleTarget.shift_open`, `TopupTarget`, `CollectTarget`); sale events are written next to the payment rows. The snapshot carries `shift {open, carried_minor}` and never an expected drawer amount. Migration `0046_shifts.sql`.
- **For later stories.** 5.11 `pos.sale` must go through `shift::gate`/`record_sales` and its Undo through `reverse_sales`; 5.24 refunds and 5.25 deposits and expenses add cash event kinds against the open shift; 5.26 closes a shift (`state = 'closed'`) and must settle how an Undo after close writes its compensating row.
- **Fixtures.** Core test fixtures open a shift (`primary()`, `DEFAULT_SHIFT`); `primary_closed()` and `bootstrapped_closed()` do not. The proptest bootstrap opens a shift and the new `ShiftOpen` op and `assert_cash_holds` invariant check the ledger against the payments.

## Story 5.11 (Hub-B lane): sell at the counter

- **Decisions taken.** `pos.sale` / `pos.sale.reverted` (permission `pos.sell`; the Undo takes `base_rev` = the order's `rev`, a sale takes none) write a settled `pos_sale` order with `tax_minor`/`tax_breakup` (`invoice::tax::compute`, once per rate; GST mode uses each product's `tax_rate_bp`, simple mode the branch `rate_bp`), product `order_lines`, payments (5.10 shift gate and cash events), wallet/credit rows and one `sale` stock move per product (lots in expiry order, expired lots never taken). Short plain stock is `OUT_OF_STOCK`; a short bundle part is `BUNDLE_SHORT` (names the component), which `stock_override` sells by first raising each short part with a `count_adjust` move (new lot, note "Override at sale", `elevated_by`); the raise survives an Undo. `stock.override` is checked on the actor or the elevating manager, and `/ui/elevate` asks an approver of `pos.sale` for `pos.sell` and `stock.override`. Migration `0047_pos_sales.sql` rebuilds `orders` (kind check, tax columns) and adds product columns to `order_lines` and `order_id`/`note`/`elevated_by` to `stock_moves` (no foreign key: the ledger outlives an undone sale).
- **For later stories.** 5.12 attaches an order to a session instead of settling it; 5.13 moves stock at "served" (a POS sale takes stock at settle); 5.14 fills `invoice_no` (Recent sales shows "Pending" until then); 5.24 refunds a settled `pos_sale` and must settle how it relates to the Undo window.
- **Hub.** `GET /ui/pos/sales` (Recent sales), `SettleBar` props `prefillCash` and `hotkey` (Ctrl+Enter), `undo.svelte.ts` kind `pos` with a `restore` payload the POS screen takes back into its cart, and `UndoDock` now also mounts on POS.
