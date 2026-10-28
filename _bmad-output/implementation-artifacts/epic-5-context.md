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

## Story 5.12 (Hub-B lane): attach an order to a running session

- **Decisions taken.** `order.place {session_id, lines, stock_override?}` / `order.place.reverted {order_id, placement_id}` (permission `pos.sell`; a placement takes no `base_rev`, its Undo takes the order's `rev`). The intent id is the `placement_id`, stored on the lines (`order_lines.placement_id/placed_by/placed_at`) and on the `sale` / `sale_reverted` stock moves (`stock_moves.placement_id`), so an Undo takes back that placement alone. Supported: a running postpaid session outside a group bill, or a block session whose bill is still open. The first placement opens the session's one `session` order; product lines sit at position 1000 and up so the time lines sort first. End appends its time lines to that order (`orders_one_per_session`); Undo of End deletes the time lines only and keeps the order while product lines remain. `validate_settle` refuses the bill of a running non-block session; a session with an order cannot join a group bill. Migration `0048_session_orders.sql` (`ALTER TABLE ... ADD COLUMN`).
- **Snapshot.** A running postpaid session shows its products on its own live bill (`SnapshotBill.items`, `order_id` stays empty), so the floor never shows a settle bar for it; an ended session or a block session shows the open order with `lines` (time) and `items` (products). `SessionScreen.ordered_items` feeds the kiosk's "Ordered: n items".
- **API.** `UiIntentResult.placed` (`PlaceOutcome`: order id and rev, placement id, `placed_at`, `undo_until`, order total); the `order.placed` UI event; `bill.reverted` also fires when an Undo deletes order lines and leaves the order.
- **For later stories.** 5.13 adds served state on these lines (today every placed line is complete); 5.14 numbers the combined bill at settle; session bills carry no tax yet (lines record `tax_rate_bp` for it).

## Story 5.13 (Hub-B lane): take stock at served, mark served, cancel into waste

- **Decisions taken.** Setting `inventory.take_stock_at` (`served` default, `order`; Settings → Inventory). Placed product lines carry `served` (migration `0049_stock_holds.sql`: `order_lines.served/served_at/served_by`, default 1, so existing and counter-sale lines are served) and the new change-log table `stock_holds (id, order_line_id, item_id, qty, released_at, rev, created_at, updated_at)`, with no foreign key on the line (a closed hold outlives a cancelled line). In `served` mode a kitchen-flagged line is placed unserved with one hold per tracked item (a bundle's parts × qty) and no stock move; every other line is served at once as in 5.12. In `order` mode stock is taken at placement and a kitchen line is still unserved until marked. A line is "held" when it has open holds, so a later setting change never strands one. Available stock = on hand − expired lots − open holds (`load_held`, catalogue `sellable`); holds touch the item's `stock` row so `catalog.changed` follows them. `order.line_served {line_id}` takes the holds' stock as `sale` moves (tagged with the line's placement and order) and closes them; `order.line_cancel {line_id, made}` (reason `changed_mind`, `mistake`, `other`; `made` required) releases the holds, or in `order` mode returns the units through a `return` move into a fresh no-expiry lot at the average cost; `made = true` then records waste `cancelled_order` at the average cost, capped at the stock on hand. Both take `pos.sell` and no `base_rev`; neither has an Undo twin; a served line cannot be cancelled. A cancelled line leaves the bill, the total shrinks, and the order goes with its last line. Undo of a placement deletes its open holds and is `UNDO_NOT_ALLOWED` once a `return` move exists for it. `validate_settle` refuses a session bill with an unserved line.
- **Wire and Hub.** `BillItem.line_id/served`; `OrderLineServed`, `OrderLineCancel`, `OrderLineEvent`; the UI event `order.line_changed {line_id, cancelled}` (a cancel sends no `bill.reverted`); `ReasonCode::{ChangedMind, Mistake}`. The tile sheet's "Items on this bill" section lists placed lines with Mark served and Cancel item (reason, then "Was it already made?"); the settle bar is blocked while a line is unserved.
- **For later stories.** 5.23 voids an unsettled order (and its holds); 5.24 refunds; Epic 7 adds kitchen tickets that would call Mark served.

## Story 5.14 (Hub-B lane): epoch-segmented invoice numbers

- **Decisions taken.** `orders.invoice_no` (unique) and the change-log table `invoice_sequences (fy, segment, next)` (migration `0050_invoices.sql`). The system intent `invoice.issue {order_id}` (no `base_rev`) numbers a settled `session`, `plan_sale` or `pos_sale` order with a non-zero total once `now - settled_at > UNDO_WINDOW_MS`; otherwise `STALE`. The number is `<branch code>/<FY>/E<counter><tag>/<seq:06>` (`DEV01/26-27/E1k/000001`): the FY is the order's stamped `fy` with its `FY20` prefix dropped, the tag one letter from the Hub instance id. A new FY or epoch is a new segment starting at 1. Issuing advances the sequence by one, locks the branch code (`lock_branch_code`) and records the irreversible effect `invoice_number_assigned`, so every Undo path (which already checks `effect_recorded`) refuses afterwards. `InvoiceTask` (arena-tasks) scans unnumbered sales oldest settle first at start, on order writes and when the next window closes.
- **For later stories.** 5.17 (Print at settle) must call `apply::invoice_issue::issue_number` inside the settle transaction, and should number any older unnumbered sale first so the series stays in `settled_at` order; 5.16 prints `orders.invoice_no`; 5.24 credit notes need their own series (same segment shape); 5.20 B2B invoices reuse this number; 5.29 GSTR-1 reads `invoice_no`. Orphaned old-epoch invoices (Epics 8-9) are never dropped.

## Story 5.16 (Hub-B lane): receipt layouts

- **Decisions taken.** `arena_core::printer::receipt` is a pure renderer: `render_sale(&ReceiptSettings, &SaleReceipt)` and `render_payment(&ReceiptSettings, &PaymentReceipt)` give ESC/POS bytes (Font A, PC437) at 32 or 48 columns. Money prints with an ASCII symbol (`Rs`, else the ISO code). 80 mm items sit in columns; 58 mm items stack. GST adds title "Tax Invoice", GSTIN, HSN/SAC and CGST/SGST per rate. Payment receipts (top-up, collection) have no invoice number. Goldens live in `crates/arena-core/src/printer/golden/` (`UPDATE_GOLDEN=1` rewrites).
- **For later stories.** 5.17 builds `SaleReceipt`/`PaymentReceipt` from committed rows (`orders.invoice_no`, `tax_breakup`, payments; `ClearedSpend` to `ClearedBill`) and sends the bytes through the outbox; 5.24 credit notes reuse the layout with negative amounts and a different title; 5.28 prints the Z-report in the same style.
