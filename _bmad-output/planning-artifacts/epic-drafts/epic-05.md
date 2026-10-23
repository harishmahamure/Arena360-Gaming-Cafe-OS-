## Epic 5: POS, inventory, tax-compliant receipts and shift close

Staff sell products and bundles, keep stock, issue GST invoices, refunds and receipts, and close a shift with a Z-report; reports and CSV/template imports are included.

> Draft notes: this epic builds on the Epic 3 settle path — `orders` / `order_lines` / `payments`, the `settle-bar` and `payment-split`, the `bill.settled{order_id}` hook with `settled_at` and `undo_until` (settled_at + 8 s), and the shared irreversible-effect guard of Story 3.14 — and on the Story 2.26 printer adapter, the Story 2.12 `lock_branch_code()` function and the Story 3.25 alert service. **Invoice timing (decided):** when the receipt choice is Print (offered only while a printer is available) the invoice number is issued inside the settle intent, so a printed bill never shows Undo and is corrected by Refund; otherwise a system `invoice.issue` assigns the number when the 8 s Undo window closes. "Invoice number issued", "receipt printed" and "drawer kicked" are all registered irreversible effects. **Cash:** from Story 5.10 a cash payment row (settle, top-up, collection, refund) needs an open shift (`SHIFT_NOT_OPEN`) and writes `cash_events`; non-cash methods never need a shift. **Tax:** Epic 3 bills were tax-inclusive with no tax line; from Story 5.2 every bill carries a per-rate tax breakup. GST logic applies only when `branch.tax_mode = 'gst_in'`; other branches use a simple tax with the same invoice series shape. Wallet top-ups and credit collections are payments, not sales: they get a payment receipt, never an invoice number. **Out of this epic:** KOT and the kitchen display (Epic 7 — this epic only stores the kitchen flag and offers "Mark served" at the Hub), the kiosk Shop and the HUD "ordered" status (Epic 6 — this epic publishes availability and shows "Ordered" on the Epic 3 minimal session screen), Bluetooth printing (the Android plugin, Epic 4 — the adapter here exposes the transport interface with USB and LAN 9100), sending WhatsApp receipts and owner alerts for variance and low stock (Epic 8 drains the outbox and sends), the yearly GST archive and orphaned-invoice reconciliation (Epics 8–9). Every story adds its new intents and their `.reverted` twins to the Story 1.6 `proptest` generator.

### Story 5.1: Tax settings — GST registration, rates, inclusive or exclusive, and simple tax outside India

As a café owner,
I want to set whether my branch charges GST, at what default rates and whether prices include tax,
So that every bill and invoice follows the tax rules that apply to my café.

**Covers:** FR112, FR76 (GST on/off, GSTIN, inclusive/exclusive), FR78 (simple tax for non-GST tenants), AD-3, AD-26, AD-39, UX-DR18, UX-DR88 (Settings)

**Acceptance Criteria:**

**Given** the `branch.bootstrap` profile from Story 2.12
**When** this story's migration runs
**Then** the profile also carries the tenant's `country` (ISO 3166-1), stored on `branch`, and `tax_mode` stays `gst_in` only for a GST-registered branch in India, else `simple`

**Given** a Manager or Owner on Settings → Tax in an Indian branch
**When** the screen opens
**Then** it shows GST registered (yes/no), GSTIN (required and format-checked, 15 characters with a valid state code and check digit, only when yes), prices include tax (inclusive/exclusive), default product rate (5%), default play-time rate (18%), an optional SAC code for play time, and the invoice prefix = branch code, shown read-only with "Locked after the first invoice" once `branch.code_locked_at` is set

**Given** a branch outside India
**When** Settings → Tax opens
**Then** no GST fields are shown; it offers a tax label (e.g. "VAT", "Tax"), a default rate (0% until set), inclusive/exclusive, and the same read-only invoice prefix

**Given** valid changes
**When** they are saved
**Then** `settings.set` stores them with `base_rev` (`STALE` on conflict), the audit shows before/after, and the new values apply to bills opened after the save, while open bills keep the values they were opened with

**Given** GST turned on with a missing or invalid GSTIN, or a rate outside 0–28% (GST) or 0–100% (simple)
**When** saved
**Then** the field shows an inline error (`GSTIN_INVALID` or a range error) and nothing is written

**Given** a Counter
**When** they open Settings → Tax
**Then** the fields are read-only and Save opens `inline-pin` for manager elevation

### Story 5.2: Tax is computed once per rate on every bill, in minor units

As a café owner,
I want every bill to show its tax per rate, rounded the same way everywhere,
So that receipts, reports and tax filings always agree to the paisa.

**Covers:** FR78, FR76 (rates per item), FR80 (plans carry their own rate), AD-7, AD-26, NFR10, UX-DR73

**Acceptance Criteria:**

**Given** the `invoice::tax` module in `arena-core`
**When** a bill's lines are taxed
**Then** line amounts are summed per tax rate, tax is computed once per rate and rounded once to the currency's minor unit half-up — inclusive: tax = round(gross × rate / (10 000 + rate)); exclusive: tax = round(net × rate / 10 000) added to the total — and the result is stored on `orders` as `subtotal_minor`, `tax_minor`, `total_minor` and `tax_breakup` (`[{rate_bp, taxable_minor, tax_minor}]`)

**Given** `tax_mode = 'gst_in'`
**When** the breakup is built
**Then** each rate also carries CGST = round-half-up(tax / 2) and SGST = tax − CGST, at half the rate each; in `simple` mode one tax line per rate is shown under the branch tax label

**Given** golden tests
**When** they run
**Then** ₹310.00 inclusive at 5% gives tax ₹14.76 (CGST ₹7.38, SGST ₹7.38); mixed 5% and 18% lines round once per rate, never per line; currencies with 0 and 3 minor-unit exponents (JPY, KWD) round at their own minor unit

**Given** a session bill (Story 3.11), a plan sale (Story 3.22) or any order
**When** its total is computed
**Then** session lines use the play-time rate in force when the bill opened, product lines use the product's rate, and plan lines use a new `plans.tax_rate_bp` (default = play-time rate, editable in Settings → Plans)

**Given** an inclusive branch
**When** a bill is computed
**Then** the total equals the Epic 3 amount and the breakup shows "GST 5% (included)"; in an exclusive branch the tax is added after the subtotal

**Given** `bill-breakdown` and the settle-bar
**When** any bill is shown
**Then** they list subtotal, one line per tax rate (with CGST/SGST for GST branches), and the total in `hub-numeric`, all in tabular figures

**Given** a settled order
**When** its stored totals are read later
**Then** they never change, even if rates or settings change afterwards

### Story 5.3: Products with categories, barcodes, images and the kitchen flag

As a branch manager,
I want to add and edit the products we sell with their price, tax and barcode,
So that the counter can sell them and stock can be tracked.

**Covers:** FR87, FR69 (availability feed), FR4 (wizard step 6 completes), AD-7, AD-39, UX-DR18, UX-DR88

**Acceptance Criteria:**

**Given** Inventory → Products
**When** a Manager creates a product
**Then** `product.upsert` (permission `catalog.edit`) stores name, category, price, tax rate (default from Story 5.1), HSN, track-stock flag (default on), image, barcode, kitchen flag, favourite flag and active flag in new `products` and `stock` tables (stock qty 0)

**Given** a GST branch and a product without HSN, or an HSN that is not 4, 6 or 8 digits
**When** saved
**Then** it is rejected `HSN_REQUIRED` on that field

**Given** a barcode already used by another active product
**When** saved
**Then** it is rejected `BARCODE_TAKEN` and nothing is written

**Given** an image
**When** it is uploaded
**Then** it is resized to at most 512 px and 200 KB before it enters the change log; a larger or non-image file shows an inline error

**Given** a product edit
**When** saved
**Then** it carries `base_rev` (`STALE` on conflict); a deactivated product leaves the grid but stays on past orders; delete is soft (`deleted_at`)

**Given** any product, stock or expiry change
**When** it commits
**Then** a `catalog.changed` event carries each product's `available` flag and reason (inactive, out of stock) so kiosks can disable items (the kiosk Shop is Epic 6)

**Given** the Hub heartbeat `setup` block (Story 2.27)
**When** the branch has at least one active product
**Then** it reports `products_count`, and wizard step 6 completes by itself

**Given** a Counter
**When** they try to edit a product
**Then** `inline-pin` opens for manager elevation

### Story 5.4: Bundles priced on their own

As a branch manager,
I want to sell combos like burger + cola at a combo price,
So that combos are easy to sell and every component is still counted in stock.

**Covers:** FR68 (definition), AD-39, UX-DR18

**Acceptance Criteria:**

**Given** a product marked "Bundle"
**When** the Manager adds components with quantities
**Then** `product.upsert` stores them in a new `bundle_components` table; the bundle has its own price and tax rate, independent of its components' prices

**Given** a component that is itself a bundle, the bundle as its own component, or a quantity ≤ 0
**When** saved
**Then** it is rejected with a field error and nothing is written

**Given** a bundle
**When** its stock availability is computed
**Then** it is the minimum over components of floor(component available ÷ component qty), and a bundle never has its own stock row

**Given** a component that is deactivated
**When** the bundle is shown in the product list
**Then** it shows "Component inactive: <name>" and is not available for sale

### Story 5.5: Stock-in with expiry lots and weighted average cost

As counter staff,
I want to record stock arriving with its cost, supplier and expiry date,
So that stock levels and stock value are right and expiring items can be tracked.

**Covers:** FR88, NFR10 (stock ≥ 0), AD-2, UX-DR18

**Acceptance Criteria:**

**Given** Inventory → Stock-in for a tracked, non-bundle product
**When** staff enter quantity, unit cost, supplier (free text) and an optional expiry date
**Then** `stock.in` (permission `stock.in`) appends a `stock_moves` row (new table, append-only, reason `stock_in`), adds a `stock_lots` row (new table: product, qty remaining, unit cost, expiry, received at), raises `stock.qty`, and updates `avg_cost_minor` as the weighted average of the existing quantity and the new lot

**Given** golden tests
**When** 10 units at ₹20 are on hand and 30 arrive at ₹24
**Then** the average cost is ₹23.00, computed in integer minor units with half-up rounding

**Given** quantity ≤ 0, negative cost, an expiry date in the past, or a bundle or untracked product
**When** submitted
**Then** a field error shows and nothing is written

**Given** any later decrement (sale, waste, write-off)
**When** it applies
**Then** it consumes lots earliest-expiry first (lots without expiry last), never takes `stock.qty` or any lot below zero, and records the lots it used on the `stock_moves` row

**Given** a stock-in
**When** the Undo toast shows
**Then** `stock.in.reverted` within 8 s removes the lot and restores quantity and average cost, under the Story 3.14 rules

### Story 5.6: Record waste with a reason

As counter staff,
I want to record spoiled, broken or given-away items,
So that stock matches the shelf and the owner can see what is lost.

**Covers:** FR89 (recording), AD-30, UX-DR66

**Acceptance Criteria:**

**Given** Inventory → Waste for a tracked product
**When** staff enter quantity and pick a reason (Spoiled · Broken · Free · Cancelled order)
**Then** `stock.waste` (permission `stock.adjust`) appends a `stock_moves` row with reason `waste`, the waste reason, the staff member and the value at average cost, and lowers stock using the Story 5.5 lot order

**Given** a quantity greater than stock on hand
**When** submitted
**Then** it is rejected `OUT_OF_STOCK` and nothing is written

**Given** a Counter without `stock.adjust`
**When** they record waste
**Then** `inline-pin` opens in place, and the audit records both actors

**Given** a waste entry
**When** the Undo toast shows
**Then** `stock.waste.reverted` within 8 s restores the quantity to the same lots

### Story 5.7: Expiry alerts, and expired stock held from sale until written off

As counter staff,
I want to be warned before stock expires and stopped from selling expired items,
So that we never sell expired food and can clear it quickly.

**Covers:** FR90, FR69 (expired disabled), UX-DR101 (expiring badge), UX-DR107, AD-12

**Acceptance Criteria:**

**Given** a branch setting "Warn before expiry" (default 3 days, 0–30)
**When** the daily system check runs at business-day start, or a stock-in records an expiry inside the window
**Then** an "Expiring soon" alert is raised in the header alert list (Story 3.25) with a sound and persistent badge, and Inventory → Expiring lists each lot with product, quantity and expiry date

**Given** a lot past its expiry date with quantity remaining
**When** availability is computed
**Then** that quantity is excluded from sale; if no unexpired quantity remains, the product is disabled in the POS grid with an "Expired" tag and published as unavailable on `catalog.changed`

**Given** Inventory → Expired, refreshed at each business-day start
**When** staff choose "Write off" on a lot
**Then** `stock.write_off` (permission `stock.adjust`) appends a `stock_moves` row with reason `expired` and value at cost, the lot reaches zero, and the alert for that lot clears

**Given** the WAN is down
**When** the daily check runs
**Then** it runs from the Hub's local clock and business day without any network call

### Story 5.8: Low-stock thresholds and alerts

As counter staff,
I want an alert when a product runs low,
So that we reorder before it runs out.

**Covers:** FR91 (Hub half), UX-DR101 (low-stock badge), UX-DR107

**Acceptance Criteria:**

**Given** a product
**When** a Manager sets a low-stock threshold (whole units, optional)
**Then** it is stored as `products.low_stock_qty`

**Given** any decrement that takes available stock to or below the threshold
**When** it commits
**Then** a `stock.low` event raises a "Low stock: <product> (<qty> left)" alert in the header alert list with a sound and badge, once per crossing, and Inventory → Low stock lists the product

**Given** a stock-in that lifts the product above its threshold
**When** it commits
**Then** the alert clears and the product leaves the low-stock list

**Given** a product without a threshold, or an untracked product
**When** stock changes
**Then** no low-stock alert is raised

**Given** a low-stock event
**When** it is written
**Then** it is recorded so the owner alert (Epic 8) can be sent from it, with product id, quantity and threshold

### Story 5.9: POS screen — product grid, search or scan, and cart

As counter staff,
I want a POS screen where I can find any product by tapping, typing or scanning and build a cart,
So that I can ring up a sale in seconds.

**Covers:** FR66 (grid, search or scan), FR69 (POS grid), UX-DR68, UX-DR69, UX-DR70, UX-DR87 (POS layout), UX-DR100 (Cart's empty), UX-DR105 (Ctrl+2, Ctrl+K), UX-DR115 (key-hub-pos)

**Acceptance Criteria:**

**Given** POS on the rail (Ctrl+2)
**When** it opens
**Then** it shows `pos-product-grid` on the left (category strip with ★ Favourites first, one "Search or scan barcode" field, `hub-tile` product tiles with photo, name and tabular price, 16px gaps) and the `cart` on the right 40px apart, with the `settle-bar` pinned at the bottom, matching the POS mock

**Given** a product that is inactive, out of stock or expired
**When** the grid renders
**Then** its tile is disabled with a readable tag ("Out of stock", "Expired") at full ink, and it cannot be added

**Given** typing in the search field
**When** characters are entered
**Then** the grid filters by name (case-insensitive) as they type; Ctrl+K focuses the field when focus is not in another text field

**Given** a USB barcode scanner (keyboard wedge)
**When** a code ending in Enter arrives in the search field, or arrives as a burst of keystrokes under 50 ms apart while focus is on the POS outside a text field
**Then** the matching product is added to the cart with quantity 1 (or +1) and the field clears; an unknown code shows "No product with barcode <code>." and adds nothing

**Given** a cart with lines
**When** staff use the ± steppers
**Then** quantities change with a live availability check (a step past available stock is refused with the reason inline), bundle components show indented under the bundle, and the cart header shows "Walk-up" or the attached player with a "Change" link (`player-lookup`)

**Given** an empty cart
**When** it renders
**Then** it shows "Cart's empty." and the settle bar shows no total

**Given** a Hub window narrower than 840px
**When** the cart has an item
**Then** `phone-cart-bar` ("3 items · ₹310 · View", tabular) sits above the bottom bar; tapping opens cart + settle bar as a full-height sheet, and Esc/Back closes it with the cart kept

### Story 5.10: Open a shift with a confirmed float, and cash needs an open shift

As counter staff,
I want to open my shift by confirming the cash in the drawer,
So that every rupee of cash from then on is counted against my shift.

**Covers:** FR82, FR83 (sales and refunds change expected cash), AD-2, AD-7, UX-DR88 (Shift), UX-DR101

**Acceptance Criteria:**

**Given** Shift on the rail (Ctrl+4) with no open shift at the branch
**When** staff choose "Open shift"
**Then** the float field is prefilled with the last closing count minus any deposit recorded at that close (₹0 for the first shift), and confirming applies `shift.open` (permission `shift.open`), creating a `shifts` row (new table) owned by the signed-in staff and a `cash_events` row (new table) of kind `float_in`

**Given** a float different from the prefilled amount
**When** confirmed
**Then** the shift opens with the entered amount and the audit records both the carried and the entered amounts

**Given** an open shift
**When** another `shift.open` is attempted
**Then** it is rejected `SHIFT_ALREADY_OPEN` naming whose shift is open (the branch has one drawer, so one open shift at a time); handover is close then open

**Given** no open shift
**When** any intent carries a cash payment row (session or POS settle, wallet top-up, credit collection)
**Then** `apply()` rejects it `SHIFT_NOT_OPEN`, the settle bar shows "Open a shift to take cash" with a link to Shift, and UPI, card, wallet and credit rows still settle

**Given** an open shift
**When** a cash payment commits from any signed-in staff
**Then** a `cash_events` row of kind `sale` for the cash kept (cash tendered minus change) is written against the open shift with that staff member's id, and its Undo writes the compensating row

**Given** the Shift screen during an open shift
**When** it opens
**Then** it shows who opened it, when, the float and a running count of cash events, but never the expected drawer amount (blind until close)

### Story 5.11: Sell at the counter — a walk-up sale settles and takes stock

As counter staff,
I want to settle a cart with any payment mix and have stock taken off automatically,
So that a product sale takes under 10 seconds and stock stays right.

**Covers:** FR66 (1 scan + 1 tap, < 10 s), FR68 (decrement components, shortage override), FR92 (served at sale), NFR6, NFR10, AD-13, UX-DR66, UX-DR71, UX-DR72, UX-DR21

**Acceptance Criteria:**

**Given** a walk-up cart and an open shift
**When** the settle bar shows
**Then** `payment-split` is prefilled with one Cash row for the full total, so Settle (or Ctrl+Enter) sells it; staff can change or add rows as in Story 3.13, and wallet and credit appear only once a player is attached

**Given** Settle
**When** `pos.sale` (permission `pos.sell`) applies
**Then** in one transaction an `orders` row (state settled, no session) with `order_lines`, the Story 5.2 tax breakup and `payments` is written, every tracked product (and each component of a bundle × quantity) is decremented through `stock_moves` reason `sale` using the lot order, `bill.settled` is emitted, and the undo toast shows

**Given** a tracked product or bundle component short at settle (e.g. sold from another window meanwhile)
**When** the intent reaches `apply()`
**Then** a plain product is rejected `OUT_OF_STOCK`; a bundle is rejected `BUNDLE_SHORT` naming the component and the cart offers `inline-pin` override (permission `stock.override`)

**Given** a manager override on a short bundle
**When** the sale applies
**Then** a `count_adjust` stock move first raises the short component to the needed quantity with reason "Override at sale" and `elevated_by`, so stock never goes below zero, and the adjustment appears in stock movements and the audit

**Given** the sale's Undo within 8 s (no invoice, receipt or drawer effect recorded)
**When** `pos.sale.reverted` applies
**Then** payments, cash event and stock moves are reversed by compensating rows and the cart is restored on screen

**Given** a Playwright run on reference hardware
**When** staff scan one item and press Settle with an open shift
**Then** the sale completes in under 10 s from the scan, with 1 scan + 1 tap

### Story 5.12: Attach an order to a running session so it settles with the bill

As counter staff,
I want to add food and drinks to a player's running session from the tile or the POS,
So that the player pays for everything once at the end.

**Covers:** FR67, FR24 (order action), FR68 (shortage at order), AD-13, UX-DR65, UX-DR69, UX-DR73, UX-DR97

**Acceptance Criteria:**

**Given** an in-session tile's sheet
**When** staff press Order
**Then** the POS opens with the cart attached to that session (cart header "In session · <player> · <device>" with "Change"); in POS, "Change" can attach any running session by player lookup or device

**Given** an attached cart
**When** staff press "Add to session bill"
**Then** `order.place` (permission `pos.sell`) adds the lines to the session's open bill order (created at the first attached order; Story 3.11's End now appends its segment lines to that order), `order.placed` is pushed, and the kiosk's session screen shows "Ordered: <n> items"

**Given** lines placed in this story
**When** they are placed
**Then** tracked stock (and bundle components) is decremented at placement through `stock_moves` reason `sale`, and every placed line is treated as served

**Given** the session bill
**When** `bill-breakdown` shows
**Then** order lines appear after the session segments, the settle covers both, and Undo of End (Story 3.14) removes only the segment lines while placed orders stay on the bill

**Given** a placement that would exceed available stock
**When** it reaches `apply()`
**Then** it is rejected `OUT_OF_STOCK` or `BUNDLE_SHORT` (with the Story 5.11 override for bundles)

**Given** a placement
**When** the Undo toast shows
**Then** `order.place.reverted` within 8 s removes the lines and returns their stock

### Story 5.13: Take stock at served, mark items served, and cancel unserved lines into waste

As counter staff,
I want stock taken only when food is actually served, and a clean way to cancel an item,
So that stock stays right when orders change and anything already made is counted as waste.

**Covers:** FR92, AD-13, UX-DR65, UX-DR97

**Acceptance Criteria:**

**Given** the branch setting "Take stock at" (Served — default, or Order) in Settings → Inventory
**When** lines are placed on a session bill (Story 5.12)
**Then** with "Order" stock is decremented at placement as before; with "Served" it is reserved (excluded from available stock) and decremented when the line is served

**Given** "Served" mode
**When** lines are placed
**Then** non-kitchen lines are served at placement, and kitchen-flagged lines stay unserved until staff press "Mark served" in the sheet's order section, which applies `order.line_served` and decrements stock

**Given** a placed, unserved line
**When** staff cancel it with a reason and answer "Was it already made?"
**Then** "No" releases the reservation (or returns stock with reason `return` in "Order" mode) and "Yes" records waste with reason "Cancelled order" at cost; the line leaves the bill and the audit records reason and staff

**Given** a served line
**When** staff try to cancel it
**Then** cancel is not offered; the correction is a void (unsettled bill) or a refund (settled bill)

**Given** a walk-up POS sale (Story 5.11)
**When** it settles
**Then** all its lines are served at settle in either mode

### Story 5.14: Invoice numbers in an epoch-segmented series that never repeat

As a café owner,
I want every sale to get the next invoice number in my branch's series, never reused,
So that my invoices are legally valid even after a Hub change or restore.

**Covers:** FR77, FR76 (invoice per sale), AD-3 (branch code lock), AD-13, AD-26, AD-38 (epoch), NFR9, NFR10

**Acceptance Criteria:**

**Given** the `invoice` module and a new `invoice_sequences(fy, segment, next)` table
**When** an invoice number is issued inside `apply()`
**Then** it is `<branch code>/<FY>/E<epoch counter><instance tag>/<seq>` with a 6-digit zero-padded seq (e.g. `KOR/26-27/E3a/000142`), written to `orders.invoice_no`, and the sequence advances by exactly one in the same transaction

**Given** a settled order with a non-zero total
**When** `undo_until` passes without Undo
**Then** a Hub task applies the system intent `invoice.issue{order_id}`, the number is issued and recorded as the irreversible effect "invoice number issued" (Story 3.14 guard); an order reverted inside the window never receives a number, so the series has no gaps

**Given** the Hub restarts with settled orders past `undo_until` and no number
**When** it starts
**Then** they are numbered in `settled_at` order before new numbers are issued, and none is numbered twice

**Given** a new epoch (fixture: provisional epoch change) or a new financial year
**When** the next number is issued
**Then** it starts at 000001 in the new `(fy, segment)`; the FY is the settle's stamped `fy`, and numbers in older segments are never reused

**Given** the first invoice ever issued at the branch
**When** it commits
**Then** `lock_branch_code()` (Story 2.12) sets `code_locked_at` in the same transaction

**Given** an order with a zero total (e.g. a pack session) or a wallet top-up or credit collection
**When** it settles
**Then** no invoice number is issued

**Given** concurrent settles from two Hub windows
**When** both issue numbers
**Then** the numbers are consecutive and unique (the writer actor serialises them)

### Story 5.15: Receipt and notification settings

As a branch manager,
I want to set up the printer, paper width, receipt text and which service messages we send,
So that receipts look right for my café without any provider keys at the branch.

**Covers:** FR114, AD-27 (no provider keys), AD-28, AD-34, UX-DR88 (receipts/printer), UX-DR95

**Acceptance Criteria:**

**Given** Settings → Receipts
**When** it opens
**Then** it shows the printer chosen in Story 2.26 (with Find printer and Test print), paper width 58 or 80 mm, header lines (branch name, address, phone; GSTIN added automatically for GST branches), footer text (up to 3 lines), default receipt choice (Print when a printer is available · None), and a live text preview at the chosen width

**Given** a header or footer line longer than the paper width (32 or 48 characters)
**When** previewed
**Then** it wraps at word boundaries in the preview exactly as it will print

**Given** Settings → Notifications
**When** it opens
**Then** it lists the service messages (WhatsApp receipt, credit reminder, low balance, session ending) each with on/off and a preview of the current template, states "Messages are sent by Arena 360 — no provider keys needed", and asks for no keys anywhere

**Given** a Free branch
**When** Settings → Notifications opens
**Then** each message shows `tier-lock` "Available on Basic"

**Given** valid changes
**When** saved
**Then** `settings.set` stores them, they are audited, and `settings.changed` updates open views

### Story 5.16: Receipt, tax invoice and payment receipt layouts for 58 and 80 mm

As a café owner,
I want receipts that read cleanly on both paper widths and carry every field the law needs,
So that customers get a proper invoice and staff never fix layouts by hand.

**Covers:** FR76 (GSTIN, HSN on invoice), FR71 (references printed), FR54 (collection receipt), UX-DR95, UX-DR96 (receipt content), UX-DR103 (formal register)

**Acceptance Criteria:**

**Given** the `receipt` renderer (pure function from a settled order and settings to ESC/POS commands)
**When** it renders a sale
**Then** it prints in Font A: text header (branch name bold double-width/height, then address, GSTIN and phone centred), title "Tax Invoice" (GST) or "Invoice" (simple), invoice no., date/time, staff, player or "Guest", session lines (device, minutes × rate), item lines, subtotal, tax breakup per rate (CGST/SGST for GST), total in bold double-height, payments by method with UPI last-4 and card references, remainder to credit, footer

**Given** 80 mm paper
**When** item lines render
**Then** they show name, qty, rate and amount in columns within 48 characters, amounts right-aligned, HSN under the name for GST branches, rules made of `-`

**Given** 58 mm paper
**When** item lines render
**Then** the name takes its own line and "qty × rate" and the amount go on the line below within 32 characters

**Given** a wallet top-up or a credit collection
**When** a payment receipt renders
**Then** it has no invoice number and shows amount, method and references; a collection lists the settled bills it cleared and outstanding after, from the Story 3.20 `bill.settled` data

**Given** golden tests
**When** fixture orders render at both widths (GST and simple, multi-rate, long names, ₹ and a 0-exponent currency)
**Then** the byte output matches the stored golden files, and no line exceeds the width

### Story 5.17: Choose print or none at settle, and print through the side-effect outbox

As counter staff,
I want to print a receipt when the customer wants one, and never be blocked when the printer is missing,
So that every sale finishes fast whatever the printer is doing.

**Covers:** FR72 (print, none, missing printer never blocks), FR74 (no Undo once printed), AD-13, AD-28, AD-34, NFR18, UX-DR71 (receipt choice, no printer state), UX-DR101 (printer offline), UX-DR107

**Acceptance Criteria:**

**Given** the settle bar with a printer available
**When** it renders
**Then** it shows "Printer ready" and the receipt choice Print · None with the branch default selected

**Given** a settle with Print
**When** it applies
**Then** the invoice number (Story 5.14) is issued inside the settle transaction itself, so the bill is never undoable: the undo toast is not shown and a correction is a Refund

**Given** no printer configured or the printer reporting offline
**When** the settle bar renders
**Then** Print is not offered and it shows "No printer connected — nothing will print. Settle is never blocked."

**Given** a settle with Print
**When** it commits
**Then** after commit a `side_effect_outbox` row (new table, kind `print`) is queued after commit, the printer task sends the receipt through the adapter (USB or LAN 9100), and records `receipt.printed` (or `receipt.print_failed` with the error); no I/O runs inside `apply()`

**Given** a print failure (paper out, offline)
**When** it is recorded
**Then** a "Printer offline" alert with sound and badge is raised in the header list, the sale stays settled, and the order shows Reprint

**Given** any settled order with a number
**When** staff press Reprint (Sessions screen, POS history or the order)
**Then** the same receipt prints marked "Reprint", and the audit records who reprinted it

**Given** the adapter interface
**When** a transport other than USB or LAN is configured
**Then** it is selected through the same interface (the Android Bluetooth plugin plugs in here) and the recorded fake drives all tests

### Story 5.18: Cash drawer kicks on cash sales

As counter staff,
I want the drawer to open by itself when I take cash,
So that I can give change straight away and every drawer opening is logged.

**Covers:** FR73, AD-13, AD-28, NFR18

**Acceptance Criteria:**

**Given** Settings → Receipts with "Cash drawer on printer" on
**When** a settle with a cash row commits
**Then** a `side_effect_outbox` row of kind `drawer` sends the ESC/POS kick pulse through the printer, and `drawer.kicked` is recorded with a `cash_events` row of kind `drawer_open` against the open shift

**Given** the drawer was kicked for a settle
**When** the undo toast would show
**Then** the irreversible-effect guard closes it and Undo is rejected `UNDO_NOT_ALLOWED`; a correction is a Refund

**Given** a settle with no cash row, or the drawer setting off
**When** it commits
**Then** no kick is sent

**Given** the printer is offline
**When** a kick fails
**Then** the sale stays settled, `drawer.kick_failed` is recorded, and the "Printer offline" alert is raised

### Story 5.19: WhatsApp receipt queued to the outbox

As a player,
I want my receipt on WhatsApp when I've agreed to service messages,
So that I have a copy without paper.

**Covers:** FR72 (WhatsApp, consent only), FR187 (queued, never for an undone action), AD-27, NFR13, UX-DR71 (WhatsApp choice)

**Acceptance Criteria:**

**Given** the settle bar for a profiled player with current consent on a paid tier
**When** the receipt choice renders
**Then** WhatsApp is added beside Print and None

**Given** a settle with receipt choice WhatsApp for a profiled player with current consent on a paid tier
**When** it commits
**Then** a `notify.requested` intent writes a `notify_outbox` row (new table: purpose transactional, consent policy version, kind receipt, order id, `not_before` = `undo_until`, state queued) with no phone number in clear in the change log

**Given** a guest, a player without consent, or a Free branch
**When** the receipt choice renders
**Then** WhatsApp is not offered, and an API attempt is rejected `CONSENT_REQUIRED` or `FEATURE_NOT_ENTITLED`

**Given** the settle is undone within the window
**When** the reverted intent applies
**Then** the outbox row is marked skipped and will never be sent

**Given** queued rows
**When** this epic ships
**Then** they stay queued for the Epic 8 drain, which sends after `not_before` once the invoice number exists; the order shows "WhatsApp receipt queued"

### Story 5.20: B2B invoice with the customer's GSTIN

As counter staff,
I want to put a business customer's GSTIN on their invoice,
So that they can claim input tax credit.

**Covers:** FR76 (B2B), UX-DR18, UX-DR95

**Acceptance Criteria:**

**Given** a GST branch's settle bar
**When** staff choose "Business invoice"
**Then** fields for customer GSTIN (format-checked with check digit) and legal name appear, and are stored on the order at settle

**Given** an invalid GSTIN
**When** staff settle
**Then** the field shows `GSTIN_INVALID` inline and Settle stays disabled until it is fixed or cleared

**Given** a settled B2B order
**When** its receipt renders
**Then** it prints the customer's legal name and GSTIN under the title, and the order is flagged B2B for GSTR-1

**Given** a simple-tax branch
**When** the settle bar renders
**Then** "Business invoice" is not offered

### Story 5.21: Coupons applied at settle

As a branch manager,
I want to create coupon codes with limits and let staff apply them at settle,
So that promotions are priced exactly as advertised.

**Covers:** FR64, AD-39, UX-DR71 (coupon), UX-DR73 (coupon line), UX-DR95

**Acceptance Criteria:**

**Given** Settings → Coupons
**When** a Manager creates a coupon
**Then** `coupon.upsert` (permission `catalog.edit`) stores code (unique, case-insensitive), percent or amount, valid from/to, usage limit, minimum bill and allowed groups, plans and products in new `coupons` and `coupon_targets` tables

**Given** a bill in the settle bar
**When** staff enter a valid code
**Then** the discount applies only to eligible lines (all lines if no targets), before tax, spread across tax rates in proportion to eligible amounts (largest remainder in minor units), and `bill-breakdown` and the receipt layout gain a coupon line

**Given** an expired, exhausted, unknown or not-applicable code, or a bill under the minimum
**When** applied
**Then** it is refused `COUPON_INVALID` with the reason inline and the bill is unchanged

**Given** a settle with a coupon
**When** it commits
**Then** `coupons.used` increments in the same transaction, the receipt shows the code and amount, and Undo decrements it

**Given** two settles racing for the last use of a coupon
**When** both reach `apply()`
**Then** only one succeeds; the other is rejected `COUPON_INVALID`

### Story 5.22: Manual discount with manager PIN and reason

As a branch manager,
I want any manual discount to need my PIN and a reason,
So that discounts are rare, approved and visible in the shift report.

**Covers:** FR65, AD-30, UX-DR66, UX-DR71 (discount)

**Acceptance Criteria:**

**Given** the settle bar
**When** staff choose Discount and enter a percentage (1–100) or an amount (≤ bill after coupon)
**Then** `inline-pin` (permission `discount.manual`) and a reason picker (Regular customer · Service issue · Staff meal · Other with text) open in place

**Given** a valid PIN and reason
**When** the settle applies
**Then** the discount is applied before tax across rates as in Story 5.21, stored on the order with reason and `elevated_by`, shown as a "Discount" line on the bill and receipt, and listed in the audit

**Given** a discount and a coupon on the same bill
**When** both apply
**Then** the coupon applies first and the discount to the remainder

**Given** no reason, or a PIN without the permission
**When** submitted
**Then** nothing is applied and the reason is shown inline

### Story 5.23: Void an unsettled order with manager PIN

As a branch manager,
I want to cancel an order that hasn't been paid yet, with my approval,
So that mistakes before payment are removed cleanly and show in the staff report.

**Covers:** FR75 (void), AD-30, UX-DR66

**Acceptance Criteria:**

**Given** a placed, unsettled order (e.g. on a session bill)
**When** staff choose Void
**Then** `inline-pin` (permission `pos.void`) and a reason picker (Entered by mistake · Customer changed mind · Other with text) open in place

**Given** a valid PIN and reason
**When** `order.void` applies
**Then** the order is marked voided, reserved stock is released and decremented stock is returned with reason `return`, the lines leave the session bill, and the audit records reason and approver

**Given** a settled or already-voided order
**When** a void is attempted
**Then** it is rejected `ORDER_SETTLED` (settled orders are refunded) or `STALE`, and nothing changes

**Given** voided orders
**When** reports and the shift report are computed
**Then** voids are counted per staff and excluded from sales

### Story 5.24: Refund a settled order with a credit note

As a branch manager,
I want to refund a paid bill against its original invoice,
So that mistakes after a receipt are fixed properly and appear in the shift report.

**Covers:** FR75 (refund), FR79, FR74 (Refund after print), AD-26, AD-30, NFR10, UX-DR66, UX-DR95 (credit note)

**Acceptance Criteria:**

**Given** Refund
**When** staff enter or scan the original invoice number
**Then** the invoice opens with its lines; an unknown number is refused `INVOICE_NOT_FOUND`

**Given** selected lines and quantities, a reason, a manager PIN (permission `refund.issue`), a refund method (cash with open shift, UPI last-4, card reference, or wallet for a profiled player; a credit portion of the original is reduced from outstanding first) and per tracked line "Return to stock"
**When** `refund.create` applies
**Then** a `refunds` row (new table) and negative `payments` are written, tax is recomputed per rate for the refunded lines (Story 5.2), a credit note number is issued from the same series counter and stored, stock is returned with reason `return` where ticked, and a cash refund writes a `cash_events` row of kind `refund`

**Given** a refund total above the amount paid minus earlier refunds
**When** it reaches `apply()`
**Then** it is rejected `REFUND_EXCEEDS_PAID` and nothing is written

**Given** a refunded plan line
**When** the owned pack or pass is unused
**Then** it is deactivated; if partly used, the line cannot be refunded and shows why

**Given** a credit note
**When** it prints (if a printer is available)
**Then** it shows "Credit Note", its number, the original invoice number and date, negative amounts and tax per rate, in the Story 5.16 layout

### Story 5.25: Cash deposits and expenses

As counter staff,
I want to record cash I move to the safe and expenses paid from the drawer,
So that the expected drawer cash at close is right.

**Covers:** FR83, AD-30, UX-DR18, UX-DR66

**Acceptance Criteria:**

**Given** an open shift and Shift → Deposit
**When** staff enter amount and slip reference (required)
**Then** `cash.deposit` (permission `cash.manage`; Counter elevates with `inline-pin`) writes a `cash_events` row of kind `deposit`

**Given** Shift → Expense
**When** staff enter category (Supplies · Utilities · Repairs · Food stock · Other), amount, paid from (Drawer or Owner) and a receipt photo
**Then** `expense.create` writes a `cash_events` row of kind `expense`; only "Drawer" expenses reduce expected cash; the photo is resized to at most 200 KB before storage

**Given** a deposit or drawer expense larger than the expected drawer cash
**When** it reaches `apply()`
**Then** it is rejected `DRAWER_SHORT` without revealing the expected amount

**Given** no open shift
**When** a deposit or expense is attempted
**Then** it is rejected `SHIFT_NOT_OPEN`

**Given** the expected-cash function
**When** it is computed for a shift
**Then** it equals float + cash sales − cash refunds − deposits − drawer expenses, from `cash_events` only, with reversed pairs netted

### Story 5.26: Close a shift with a blind count and a signed variance

As counter staff,
I want to count the drawer before I see what's expected and get a plain difference,
So that the count is honest and nobody is blamed for a number.

**Covers:** FR84, FR85, NFR6 (shift close < 10 s), AD-30, UX-DR74, UX-DR75, UX-DR66, UX-DR103 (staff voice), UX-DR115 (key-hub-shift-close)

**Acceptance Criteria:**

**Given** Shift → Close shift
**When** it opens
**Then** `denomination-counter` shows one row per note for the branch currency (INR: ₹500, ₹200, ₹100, ₹50, ₹20, ₹10, plus a coins total) with count fields, tabular subtotals and a running total, a "Single total" toggle, and `variance-readout` in the blind state ("Expected cash shows after you enter the count."); currencies without a denomination set use Single total only

**Given** counted cash entered
**When** staff confirm the count
**Then** Counted, Expected and Difference appear as three `hub-numeric` figures 32px apart, Difference = counted − expected, signed with "over" or "short", in the line "Counted ₹4,850. Expected ₹4,990. Difference −₹140 short."; "Breakdown" lists float, cash sales, cash refunds, deposits and expenses

**Given** |difference| ≤ the branch threshold (default ₹100, Settings → Shift)
**When** staff press "Close shift"
**Then** `shift.close` (permission `shift.close`) stores counted, expected and variance and closes the shift with no PIN

**Given** |difference| > the threshold
**When** the readout renders
**Then** it shows a warning border and "Over threshold" (never red), and close needs `inline-pin` (permission `shift.override_variance`) plus a reason (Counting error · Unrecorded expense · Change float mismatch · Other with required text) in place; otherwise it is rejected `VARIANCE_OVER_THRESHOLD`

**Given** an over-threshold close
**When** it commits
**Then** `shift.closed` carries the variance, reason, staff and approver for the owner alert (sent in Epic 8), and the audit records both actors

**Given** an optional "Deposit at close" amount with slip reference
**When** the shift closes
**Then** the deposit is recorded after the count and the next shift's suggested float is counted minus that deposit

**Given** the API
**When** a `shift.close` arrives without a counted amount
**Then** it is rejected and the expected amount is never returned before counted is stored

**Given** a Playwright run on reference hardware
**When** staff close a within-threshold shift using Single total
**Then** it completes in under 10 s from the first tap

### Story 5.27: Z-report at shift close

As a café owner,
I want a one-tap end-of-shift report of everything that happened to the money,
So that I can check each shift at a glance and it matches what I see remotely.

**Covers:** FR86, FR108 (Shift/Z), FR73 (drawer opens in report), FR75 (negative line), UX-DR95, UX-DR75

**Acceptance Criteria:**

**Given** a shift closes
**When** the close commits
**Then** a Z-report is built from stored rows for that shift: sales by type (play time, products, plans), payments by method with UPI and card reference counts, expected vs counted, variance and reason, waste (quantity and value by reason), expenses, deposits, refunds as negative lines, discounts, voids and drawer opens

**Given** the close screen
**When** this story ships
**Then** the close action reads "Close shift · print Z-report"

**Given** a printer is available
**When** the close commits
**Then** the Z-report prints through the side-effect outbox in the Story 5.16 layout; with no printer, nothing prints and the close still completes

**Given** Reports → Shifts
**When** staff open a past shift
**Then** its Z-report shows on screen with Reprint, and the figures equal those printed at close

**Given** golden tests over a fixture shift with undone sales, refunds and an override sale
**When** the Z-report is built
**Then** reversed pairs net to zero and totals equal the sum of the shift's payments and `cash_events`

### Story 5.28: GSTR-1 export

As a café owner,
I want to download a month's GST return data from the Hub,
So that my accountant can file GSTR-1 without retyping invoices.

**Covers:** FR81, AD-26, NFR1

**Acceptance Criteria:**

**Given** a GST branch and Reports → GST
**When** a Manager picks a month and exports
**Then** the Hub writes a ZIP of CSVs in the GST Offline Tool template format — B2B (per invoice with customer GSTIN), B2CS (by rate, place of supply from the GSTIN state code), CDNR (credit notes to registered customers), HSN summary and Documents issued (invoice and credit note ranges per segment) — to a chosen folder or USB, with no network call

**Given** a fixture month with B2B and B2C invoices, two tax rates, a credit note and an epoch change
**When** the export is checked
**Then** taxable value, CGST and SGST per section reconcile to the Hub's invoices minus credit notes for that month, and Documents issued lists each segment's first and last number with none missing

**Given** a simple-tax branch
**When** Reports opens
**Then** the GST export is not shown

**Given** a month with no invoices
**When** exported
**Then** the files contain headers only and the screen says "No invoices in <month>."

### Story 5.29: Reports screen with daily sales and sessions

As a café owner,
I want daily sales and session reports on the Hub even with the internet down,
So that I can see how the day went and how busy my PCs were.

**Covers:** FR108 (daily sales, sessions), FR80 (plans reported separately), AD-7, AD-13 (net reversed pairs), NFR1, UX-DR88 (Reports)

**Acceptance Criteria:**

**Given** Reports on the rail (permission `report.view`)
**When** it opens
**Then** it lists Shifts, Daily sales, Sessions and GST (GST branches only) with a business-day or date-range picker in branch local time, and later report stories add their entries to the same list

**Given** Daily sales for a business day
**When** it renders
**Then** it shows totals by hour, by device group and by product category, and sales by type with play time, products and plans (packs and passes) as separate rows; top-ups and credit collections appear in a separate "Payments received, not sales" section

**Given** Sessions for a range
**When** it renders
**Then** it shows count, billed minutes and revenue by device and by group, and utilisation per hour (minutes in session ÷ minutes available) for each group

**Given** undone actions in the range
**When** reports are computed
**Then** reversed pairs net to zero and voided orders are excluded, with all figures from stamped `business_day` columns and stored invoice totals

**Given** a 90-day range on a seeded branch with 60 devices
**When** a report renders on reference hardware
**Then** it appears within 2 s without blocking `apply()`

### Story 5.30: Players and staff reports

As a café owner,
I want to see my best players and what each staff member sold, discounted and voided,
So that I can reward regulars and spot problems early.

**Covers:** FR108 (players, staff), AD-24, UX-DR88

**Acceptance Criteria:**

**Given** Reports → Players for a range
**When** it renders
**Then** it shows top spenders (name, visits, spend), new vs returning players (first settle in range vs earlier), guest totals as "Guest", and credit outstanding aged 0–7, 8–14 and 15+ days

**Given** an erased player
**When** reports list players
**Then** their rows show "Erased player" with no personal data

**Given** Reports → Staff for a range
**When** it renders
**Then** each staff member shows sales total, sessions started, discounts given (count and value), voids, refunds approved and variance per shift

**Given** a Counter without `report.view`
**When** they open Reports
**Then** `inline-pin` opens in place

### Story 5.31: Stock reports with waste by reason

As a branch manager,
I want to see stock value, movements, waste and expiring items,
So that I know what I'm holding and what I'm losing.

**Covers:** FR108 (stock), FR89 (waste report), FR90 (expiring), UX-DR88

**Acceptance Criteria:**

**Given** Reports → Stock
**When** it renders
**Then** it shows current stock per tracked product with quantity, average cost and value, and the total stock value

**Given** Movements for a range
**When** it renders
**Then** it lists every `stock_moves` row (product, reason, quantity, value, staff, time), filterable by product and reason

**Given** Waste for a range
**When** it renders
**Then** it shows totals by reason (Spoiled, Broken, Free, Cancelled order, Expired write-off) with quantity and value at cost, and per product

**Given** Expiring
**When** it renders
**Then** it lists lots expiring within the warning window and expired lots not yet written off

### Story 5.32: Export every Hub report to CSV and PDF

As a café owner,
I want to save any report as CSV or PDF,
So that I can share it with my accountant or keep it offline.

**Covers:** FR109, FR199 (reports exportable), NFR1, NFR16

**Acceptance Criteria:**

**Given** any report on screen (including a Z-report and the stock and GST views)
**When** staff choose Export → CSV
**Then** the Hub writes a UTF-8 CSV with a header row, amounts in major units with the currency code column, dates in branch local time, to a chosen folder or USB

**Given** Export → PDF
**When** chosen
**Then** the Hub service renders an A4 PDF with the branch name, report title, range, generated-at time and the same figures, using the bundled Sora font with tabular numerals and no network fetch

**Given** a report figure on screen
**When** it is compared with its CSV and PDF
**Then** all three match exactly

**Given** an unwritable destination
**When** export runs
**Then** an inline error names the problem and offers another location

### Story 5.33: CSV import with dry-run, starting with products

As a café owner moving from another system,
I want to import my product list from a spreadsheet and see problems before anything changes,
So that I'm ready to sell in minutes without typing every item.

**Covers:** FR7 (CSV engine, products), FR87 (import with dry-run), NFR6 (1,000 rows < 30 s), AD-2, AD-14

**Acceptance Criteria:**

**Given** Settings → Import → Products
**When** a Manager picks a CSV (UTF-8, header row: name, category, price, tax_rate, hsn, barcode, track_stock, kitchen, opening_qty, unit_cost)
**Then** a dry-run validates every row with the Story 5.3 rules and shows counts of new, updated (matched by barcode, else exact name) and failed rows, with each failure's row number and reason, and writes nothing

**Given** conflicts (duplicate barcode within the file, unknown column, bad number)
**When** the dry-run shows them
**Then** commit is disabled until the file is fixed, or the Manager chooses "Import valid rows only"

**Given** commit
**When** it applies
**Then** one `import.products` intent per 500 rows creates or updates products and records opening stock as stock-in lots, each chunk with its own idempotency key so a retry never duplicates

**Given** a 1,000-row file on reference hardware
**When** dry-run plus commit runs
**Then** it completes in under 30 s

**Given** a template CSV
**When** the Manager chooses "Download template"
**Then** a file with the exact header and two sample rows is saved

### Story 5.34: Import players and plans by CSV

As a café owner moving from another system,
I want to bring my players with their balances and my plans across from a spreadsheet,
So that regulars keep their money and plans from day one.

**Covers:** FR7 (players, plans), AD-24, NFR6, NFR13

**Acceptance Criteria:**

**Given** Settings → Import → Players with columns phone, name, balance, credit_limit
**When** the dry-run runs
**Then** each row is checked for a valid phone, duplicates within the file and against existing players (by phone), non-negative balance and limit, and results show as in Story 5.33

**Given** commit
**When** it applies
**Then** each player is created with encrypted personal data and a generated username, the balance is added as a `wallet_txns` row with reason `import` (no cash event, no invoice), credit is enabled with the limit if above zero, and consent is recorded as not granted

**Given** Settings → Import → Plans with columns name, type (pack, block, pass), hours, price, validity_days, window_start, window_end, groups
**When** the dry-run runs
**Then** each plan is checked with the Story 3.21 rules and its group names are matched to existing groups; an unknown group or a mix of console and PC groups fails that row

**Given** a 1,000-row players file on reference hardware
**When** dry-run plus commit runs
**Then** it completes in under 30 s

### Story 5.35: Seed plans and products from a starter template

As a new café owner,
I want to start from a ready-made menu and plan list for my kind of café,
So that I can open with sensible defaults and edit them later.

**Covers:** FR7 (templates), UX-DR103

**Acceptance Criteria:**

**Given** Settings → Import → Templates
**When** a Manager opens it
**Then** it offers Indian café, Console lounge and VR arcade, each listing the plans and products it will add

**Given** a template chosen
**When** the dry-run runs
**Then** template plans are mapped to existing groups of the matching kind (PC, console, VR); plans with no matching group, and products whose name or barcode already exists, are listed as skipped

**Given** commit
**When** it applies
**Then** plans and products are created through the Story 5.33 import intents, with prices in the branch currency and tax rates from Story 5.1 defaults, and the wizard's step 6 completes once products exist

**Given** a template already applied
**When** it is applied again
**Then** nothing is duplicated

### Story 5.36: Export and import a branch setup file

As a café owner with more than one branch,
I want to copy one branch's groups, rates, plans and products to another branch as a file,
So that a new branch matches the first one even without the internet.

**Covers:** FR12 (file path), AD-8 (shared format in `arena-proto`)

**Acceptance Criteria:**

**Given** Settings → Import → Branch setup
**When** a Manager chooses Export
**Then** the Hub writes a versioned JSON file (format defined in `arena-proto`) with groups (name, swatch, kind), rates, plans, pricing rules, coupons, product categories, products (without stock or images) and bundles, and no players, devices, money or stock rows

**Given** a setup file
**When** a Manager imports it at another Hub
**Then** a dry-run lists what will be created and what is skipped because it already exists (by name), then commit creates them through `apply()`

**Given** a file from a newer format version
**When** imported
**Then** it is refused `UNSUPPORTED` with "Update this Hub to import this file."

### Story 5.37: A second branch starts from the tenant's existing setup

As a café owner opening another branch,
I want the new branch's wizard to reuse my existing groups, plans and products by default,
So that setting up the second café takes minutes.

**Covers:** FR12, FR174 (Free uploads nothing), AD-3, AD-5, AD-9, AD-29, AD-36

**Acceptance Criteria:**

**Given** a paired Hub on a paid tier (licence entitlement)
**When** its catalogue changes (groups, rates, plans, products, bundles, rules, coupons)
**Then** within 5 min it sends the Story 5.36 setup file (≤ 1 MB) to `PUT /v1/branches/{id}/setup-template` with its device token, and the cloud stores it under the branch's R2 prefix

**Given** a Hub on Free
**When** its catalogue changes
**Then** it uploads nothing, and the cloud endpoint rejects any upload from a Free branch with `FEATURE_NOT_ENTITLED`, checked from `gz_control` entitlements

**Given** an owner creating another branch (Story 2.29)
**When** the new branch's wizard reaches step 4 and at least one paid branch of the tenant has a stored file
**Then** "Start from <branch>'s setup" is on by default with a picker of those branches, and can be turned off

**Given** the option is on
**When** the new Hub pairs
**Then** `branch.bootstrap` carries a short-lived link to the chosen file, the Hub applies it after bootstrap through the Story 5.36 import, the copied groups appear in wizard step 4 for device assignment, and step 5 completes if every group has a rate

**Given** the tenant's other branches are all on Free, or none has a stored file
**When** the wizard shows step 4
**Then** it explains the file path instead: "Export the setup file at your other branch's Hub (Settings → Import → Branch setup) and import it at the new Hub", and the owner can continue without it

**Given** the download fails
**When** the new Hub applies the bootstrap
**Then** the wizard offers Retry or the file path, and the branch still bootstraps without the template

### Story 5.38: POS and money proven under load, offline and on real hardware

As a café owner,
I want stock, tax, invoices and the drawer proven correct under random sequences, outages and real printers,
So that the books close right on the busiest night.

**Covers:** NFR1, NFR6, NFR10, NFR18, AD-2, AD-14, AD-26

**Acceptance Criteria:**

**Given** the `proptest` suite with every Epic 5 intent and its reverse
**When** it runs in CI on every push
**Then** after every step: stock quantity ≥ 0 and equals the sum of lots and of `stock_moves` deltas; each order's total equals lines − discounts + tax (exclusive) with tax computed once per rate; payments on settled orders equal totals; invoice and credit note numbers are unique and gap-free per `(fy, segment)`; every invoice is on a settled order; expected drawer cash equals the sum of the shift's cash events; reverted pairs net to the prior state

**Given** all WAN egress blocked
**When** the Epic 5 flows run (products, stock, POS sale, session orders, coupons, discounts, refunds, invoices, printing to the fake, shifts, reports, exports, imports)
**Then** results match a WAN-up run and no outbound connection other than the connectivity probe is attempted

**Given** the Hub process killed during a burst of sales
**When** it restarts
**Then** every committed sale, stock move and invoice is present, pending invoice numbers are issued once, and queued print jobs either print once or are marked failed for Reprint

**Given** the test-café hardware bench
**When** the monthly hardware job runs
**Then** receipts and a Z-report print correctly at 58 and 80 mm on the listed USB and LAN ESC/POS printers, the drawer kicks, and a USB keyboard-wedge scanner adds items, with results published to the compatibility list

**Given** a Playwright run on reference hardware
**When** it times a scanned product sale and a within-threshold shift close
**Then** each is under 10 s
