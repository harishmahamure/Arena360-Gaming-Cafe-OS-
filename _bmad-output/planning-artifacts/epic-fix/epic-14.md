## Epic 14: Deeper back office, owner depth and platform ops

Managers run stock counts, purchase orders, petty cash, attendance and prep stations; owners get scheduled reports, cohorts, team logins and approvals; the platform gains off-site standby, auto-updates, a demo and compare pages.

> Notes: this epic builds on the Epic 1 `apply()`, change log, idempotent intents and `base_rev` (Stories 1.7–1.8), the LAN envelope and class scope table (Story 1.9), roles and permissions (Story 1.11), locale and money formatting (Story 1.12), inline PIN (Story 1.13), the audit viewer (Story 1.15) and per-device HC (Story 1.21); on the Epic 2 cloud, owner sign-in with TOTP (Stories 2.4–2.6), the heartbeat with `cloud_time` and licence re-sign (Story 2.11), the entitlement API and `trusted_now` (Story 2.12), the setup-command mailbox (Story 2.15), the Hub installer (Story 2.10), device pairing and the limiter (Stories 2.18–2.19), device groups and maintenance (Stories 2.20, 2.24), opening hours (Story 2.26), printer detection (Story 2.27) and Account → Branches (Story 2.30); on Epic 3 sessions and their "open" states (active, paused, grace), the tile sheet (Story 3.5), the alert service (Story 3.25) and pricing rules with the resolver (Stories 3.29–3.31); on the Epic 4 Android Hub and its Play rollout (Story 4.16, unchanged here); on Epic 5 stock-in with lots (Story 5.5), waste (Story 5.6), the shift (Stories 5.10, 5.25–5.26, 5.28), refunds with credit notes (Story 5.24), Hub reports and exports (Stories 5.27, 5.30–5.32); on Epic 6 staff messages to every PC (Story 6.16) and the kiosk Hub-unreachable rules (Story 6.36); on the Epic 7 kitchen ticket model, KOT, board and pairing (Stories 7.1, 7.3, 7.5–7.7). From Epic 8 it uses, by title: "Rollup schema v1 and the daily rollup at close", "Rollups regenerate after a schema bump, and the cloud accepts only the current and previous schema", "The owner read API serves the live summary and rollup ranges from files", "The Hub publishes a branch summary every cadence" (the summary `catalogue` and `attention` items), "The Owner app shell — navigation, themes, install and Free limits", "Reports — revenue and cash reconciliation", "Owner reports export to CSV and PDF and match the Hub's Z-reports exactly", "The cloud accepts owner command kinds, checks them, and fans "all branches" out", "The Hub applies owner commands exactly once, and the owner's change wins", "Commands are swept, expire after 7 days, and are fenced by epoch", "Preview the target branches and follow each command row to Applied", "Step-up MFA for sensitive commands", "Manage → Pricing — plans and rates, pricing rules and coupons", "Manage → Devices and groups, and revoking a device", "The cloud sends player and owner messages through a swappable messaging adapter, exactly once", "The Hub drains its notification outbox after the undo window", "Owner alerts by web push and WhatsApp, and the Alerts tab", "The Hub raises owner alerts for cash variance, low stock, overdue credit and large refunds or discounts", "The cloud alerts the owner when a branch goes offline during opening hours", "A Cloudflare Tunnel for entitled branches, run by the Windows Hub" and "Remote kiosks connect through the Tunnel with their device token". From Epic 9 it uses, by title: "The cloud issues epoch counters by compare-and-set on the branch fence", "Owner issues a standby code and a second PC pairs as standby", "The standby bootstraps from the main Hub and follows it live", "The standby keeps its own catalogue, media and licence and shows its status", "Promote the standby to main with a manager PIN", "Force promote after a data-loss warning", "A returning old main Hub locks itself" and "One-tap full database export to a folder or USB drive". From Epic 10 it uses, by title: "The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans" (`addon_owner_login` already priced), "Wizard step 2 takes payment through Razorpay Checkout", "Upgrade links and Add branch reuse the same checkout" and "Change tier and add-ons mid-cycle with proration" (all show Owner login as "Available soon" until Story 14.25). From Epic 11 it uses, by title: "Marketing site shell — navigation, footer, light and dark themes and high contrast", "Pricing — Cafés tab with Monthly · Yearly, equal plan cards, add-ons and FAQ", "Downloads with installers, system requirements and checksums", "Get started hands off to sign-up, and funnel events are counted without personal data" (funnel events reused by the Demo page), "Admin access through Cloudflare Access, with staff accounts and every admin action audited" and "The admin app shell, the Audit viewer and Staff management" (Story 14.53 adds a Status section there for incidents). From Epic 12 it uses the signed read-only Pi image ("A signed, read-only Pi image is built in CI") and the Pi agent's paired connection (Pi health frames). From Epic 13 it uses "Rollup schema v2 carries tournament income, deposits, loyalty and bookings" (v2 and the per-tier `rollup_schema` pin with `arena-cloud ops rollup-pin`) and the ggLeap and Smartlaunch importers (named on the Compare pages). **Invariants kept:** every Hub business change goes through `apply()` — the one allowed exception (decided) is device health telemetry (`device_health` from Story 14.13, `device_outages` from Story 14.14), which is not business state and is never synced as batches, snapshotted, exported or audited; every owner write is a command through the mailbox and applies once; promotion and restore open a new epoch through the fence; paid features are gated only by licence flags (or, for owner-side features, by `gz_control` entitlements); the cloud never runs SQL over branch rows — every owner figure in this epic is a rollup or summary field. **Tier gating (decided):** stock counts, suppliers and POs, petty cash, attendance and device health at the Hub are on every tier; prep stations need the `kitchen` flag (Basic+); scheduled reports, deeper dashboards, team logins, refund approvals, message to all kiosks, tenant-wide pricing and the device-offline owner alert need a paid branch (Owner app); off-site standby needs the new `offsite_standby` flag (Premium only, Windows Hub only). **Ticket model amended:** Epic 7 opened one kitchen ticket per placement; from Story 14.10 a placement opens one ticket per prep station that has kitchen lines in it; a branch with no stations defined behaves exactly as Epic 7. **Scheduled reports (decided):** WhatsApp only, through the existing messaging adapter; there is no email channel or email provider. **Rollup schema v3 (decided):** Epic 13 defines rollup v2 and the per-tier `rollup_schema` pin. This epic defines v3 = Epic 13's v2 plus the utilisation, staff, kitchen, devices, games (play hours per game, FR137) and player-cohort blocks (Stories 14.15–14.16); the cloud accepts v3 and v2 (N and N−1) (Story 14.17) and the pin is raised to 3 per tier with Epic 13's `ops rollup-pin`, so v2 Hubs keep reporting. **Refund approvals:** the Hub table is `refund_approvals` (intents `refund.approval_*`), distinct from Epic 10's cloud `refund_requests` for subscription refunds. **Updates (decided by the spine):** Windows Hub and Kiosk use the Tauri updater with ed25519-signed manifests staged by control-plane percentage; the Pi agent is ed25519-signed; kiosks and Pis update only from the Hub's verified cache; nothing installs while any session at the branch (Hub) or on the device (kiosk, Pi) is active, paused or in grace; the standby updates before the primary; rollback is a roll-forward hotfix once a migration has committed. The Android Hub keeps Play staged rollout (Story 4.16) and is out of this epic except as a kiosk-update cache. **Out of this epic:** central purchasing across branches, payroll hints and accounting exports (v2, SPEC non-goals), smart-plug vendors, and the Epic 13 bookings, tournaments, loyalty and vault. Every Hub story adds its new intents and their `.reverted` twins to the Story 1.8 `proptest` generator.

### Story 14.1: Start a stock count and fill in the count sheet

As a branch manager,
I want to count what is on the shelf against a count sheet without seeing the expected quantity first,
So that the count is honest and I can do it during a quiet hour without closing the counter.

**Covers:** FR93 (count sheet), AD-2, AD-30, NFR10, UX-DR18, UX-DR66, UX-DR88 (Inventory), UX-DR100

**Acceptance Criteria:**

**Given** Inventory → Stock count
**When** a Manager (permission `stock.count`; Counter elevates with `inline-pin`) chooses "New count" with scope All tracked products or one or more categories
**Then** `stock.count_start` creates a `stock_counts` row (new table: id, scope, state `open|applied|cancelled`, started_by, started_at, `rev`) and one `stock_count_lines` row (new table: count_id, product_id, counted_qty null, expected_at_count null, counted_by, counted_at) per tracked, non-bundle product in scope, sorted by category then name

**Given** an open count
**When** the count sheet renders
**Then** each line shows product name, category and barcode with an empty count field and no expected quantity ("Expected shows after you enter the count."); a barcode scan focuses that product's line, and the sheet can be printed on the receipt printer as a blank checklist (name and an underscore field per line)

**Given** a count entered for a line
**When** it is saved
**Then** `stock.count_line` stores `counted_qty`, the counter and the time, and stores `expected_at_count` = the product's `stock.qty` at that commit, so sales and stock-in during the count never distort that line; the line then shows Expected and Difference (counted − expected, signed "over" or "short")

**Given** a line already counted
**When** it is counted again
**Then** the new figure replaces the old one with a fresh `expected_at_count`, and the audit keeps both

**Given** a second "New count" whose scope overlaps an open count
**When** it is started
**Then** it is refused `COUNT_OPEN` naming the open count; counts with disjoint categories may run at once

**Given** a negative or non-integer count, or a product deactivated after the count started
**When** it is entered
**Then** a field error shows and nothing is written; deactivated products stay on the sheet marked "Inactive" and can still be counted

**Given** "Cancel count" (Manager)
**When** it is confirmed in place
**Then** the count becomes `cancelled`, stock is untouched, and it stays listed in history

### Story 14.2: Apply count adjustments with reasons and see variance by value

As a branch manager,
I want to post the differences from a finished count with a reason for each, and see what they cost,
So that stock matches the shelf and I know where stock value is going.

**Covers:** FR93 (variance per item, adjustment with reason, variance-by-value report), NFR10 (stock ≥ 0), AD-2, AD-7, AD-30, UX-DR66, UX-DR88 (Reports)

**Acceptance Criteria:**

**Given** an open count with at least one counted line
**When** the Manager chooses "Review and apply"
**Then** a review lists only lines with a non-zero difference, each with expected, counted, difference and value (difference × the product's `avg_cost_minor`), the total over and short value, and the count of uncounted lines (which are never adjusted)

**Given** each non-zero line
**When** the review is completed
**Then** a reason is required per line (Miscount earlier · Theft or loss · Damaged, not recorded · Sale not recorded · Stock-in not recorded · Other with required text), and "Apply" stays disabled until every line has one

**Given** "Apply" with a manager PIN in place (permission `stock.count_apply`)
**When** `stock.count_apply` commits
**Then** in one transaction each line writes a `stock_moves` row with reason `count_adjust`, the reason text and the count id; a shortage consumes lots earliest-expiry first (Story 5.5); a surplus adds a lot at the current average cost with no expiry; the count becomes `applied`; no line takes `stock.qty` or a lot below zero

**Given** stock moved between a line's count and Apply
**When** Apply runs
**Then** the adjustment is the line's recorded difference (counted − expected at count), not a re-computed one, and the review warns "Stock changed for 3 items since they were counted" so the Manager can recount them first

**Given** an applied count
**When** Undo is offered
**Then** no Undo is offered; a mistaken count is corrected by a new count, and the audit records the applying manager, every line and its reason

**Given** Reports → Stock → Count variance for a range
**When** it renders
**Then** it lists applied counts with date, scope, manager, lines adjusted and net value, and per product the quantity and value over and short, sorted by absolute value, with the reason breakdown; it exports to CSV and PDF like every Hub report (Story 5.32)

### Story 14.3: Keep supplier records and pick a supplier at stock-in

As a branch manager,
I want a list of my suppliers with their contact and tax details,
So that stock-in and purchase orders name the same supplier every time.

**Covers:** FR94 (supplier records), AD-2, AD-39, UX-DR18, UX-DR88 (Inventory), UX-DR100

**Acceptance Criteria:**

**Given** Inventory → Suppliers
**When** a Manager adds a supplier
**Then** `supplier.upsert` (permission `inventory.manage`) stores it in a new `suppliers` table (id, name, contact person, phone, email, GSTIN optional, address, notes, active, `rev`)

**Given** a GSTIN entered
**When** it is saved
**Then** it is checked for the 15-character GSTIN format and state code; an invalid value shows a field error and nothing is written

**Given** two suppliers with the same name (case-insensitive)
**When** the second is saved
**Then** it is refused `DUPLICATE_NAME` with a link to the existing one

**Given** the Stock-in form (Story 5.5)
**When** staff type in the supplier field
**Then** it suggests active suppliers by name; choosing one stores `supplier_id` on the `stock_moves` row as well as the name; free text is still accepted and stored as name only

**Given** a supplier with stock-in history
**When** a Manager deactivates it
**Then** it disappears from suggestions, keeps its history, and can be reactivated; suppliers are never deleted

**Given** a branch with no suppliers
**When** Suppliers opens
**Then** it shows "No suppliers yet." with "Add supplier"

### Story 14.4: Raise a purchase order and send it to the supplier

As a branch manager,
I want to write a purchase order with quantities and agreed costs and share it,
So that the supplier knows exactly what to deliver and I can check the delivery against it.

**Covers:** FR94 (purchase orders), AD-2, AD-7, AD-39, UX-DR18, UX-DR88 (Inventory)

**Acceptance Criteria:**

**Given** Inventory → Purchase orders → New
**When** a Manager picks an active supplier and adds lines (tracked, non-bundle product, quantity, unit cost) with an optional expected date and note
**Then** `po.create` (permission `inventory.manage`) writes a `purchase_orders` row (new table: id, number `PO-<n>` from a per-branch counter allocated in `apply()`, supplier_id, state `draft`, expected_on, note, total_minor, created_by, `rev`) and `purchase_order_lines` (po_id, product_id, qty_ordered, qty_received 0, unit_cost_minor)

**Given** "Suggest from low stock"
**When** pressed
**Then** every product at or below its low-stock threshold that this supplier last delivered is added with quantity = threshold × 2 − current quantity (at least 1), editable before saving

**Given** a draft PO
**When** the Manager chooses "Mark as sent"
**Then** `po.send` moves it to `sent`, after which lines can no longer be edited, only cancelled; Export → PDF produces a print-styled PO with branch name and GSTIN, supplier, number, date, lines and total, and "Share" uses the device share sheet where available

**Given** a `draft` or `sent` PO with nothing received
**When** the Manager cancels it with a reason
**Then** it becomes `cancelled` and stays in the list

**Given** quantity ≤ 0, negative cost, an inactive supplier or a bundle line
**When** submitted
**Then** a field error shows and nothing is written

**Given** the PO list
**When** it renders
**Then** it lists POs newest first with number, supplier, state as a `status-badge` (Draft · Sent · Part received · Received · Closed · Cancelled), expected date and total, filterable by state and supplier

### Story 14.5: Receive a delivery against a purchase order

As counter staff taking in a delivery,
I want to tick off what arrived against the PO,
So that stock-in rows are written for me and short deliveries are visible.

**Covers:** FR94 (receive against PO creates stock-in rows), FR88 (lots, weighted average cost), NFR10, AD-2, UX-DR18, UX-DR21

**Acceptance Criteria:**

**Given** a `sent` or `part received` PO
**When** staff choose "Receive" (permission `stock.in`)
**Then** each open line shows ordered, already received and a received-now field prefilled with the outstanding quantity, the PO unit cost (editable) and an optional expiry date

**Given** received quantities entered
**When** staff confirm
**Then** `po.receive` applies, in one transaction, one Story 5.5 stock-in per line with a non-zero quantity (lot, weighted average cost, `stock_moves` reason `stock_in` with `ref_id` = PO id and `supplier_id`), adds to `qty_received`, and moves the PO to `part received` or `received`

**Given** a received quantity above the outstanding quantity
**When** entered
**Then** the line shows "More than ordered" as a warning and it is accepted only with a manager PIN in place

**Given** a unit cost that differs from the PO cost
**When** confirmed
**Then** the received cost is used for the lot and average, and the line records both costs for the PO report

**Given** a `part received` PO the Manager does not expect to be completed
**When** they choose "Close" with a reason
**Then** it becomes `closed` with the outstanding quantities listed as short

**Given** a receipt
**When** the Undo toast shows
**Then** `po.receive.reverted` within 8 s reverses every stock-in it wrote and the PO's received quantities, under the Story 3.14 rules; after any of the received stock is sold or wasted, Undo is not offered

**Given** Reports → Stock → Purchase orders for a range
**When** it renders
**Then** it lists POs by supplier with ordered, received and short quantities and value, and cost differences

### Story 14.6: Keep a petty-cash ledger separate from the drawer

As a branch manager,
I want a small cash float for daily purchases with its own running balance,
So that petty spending is tracked without muddling the drawer count.

**Covers:** FR95, NFR10 (balance never negative), AD-2, AD-7, AD-30, UX-DR18, UX-DR21, UX-DR66, UX-DR88 (Shift)

**Acceptance Criteria:**

**Given** Shift → Petty cash
**When** it opens
**Then** it shows the petty-cash balance in `hub-numeric` and the ledger newest first (time, kind, category, amount, staff, balance after), from a new append-only `petty_cash_entries` table (id, kind `top_up|spend|return|count_adjust`, amount_minor, source, category, note, receipt_blob, staff_id, at, `business_day`)

**Given** "Top up" with an amount and source Drawer or Owner
**When** `petty.top_up` applies (permission `cash.manage`; Counter elevates with `inline-pin`)
**Then** the balance rises; a Drawer top-up needs an open shift and also writes a `cash_events` row of new kind `petty_top_up` that the expected-cash function (Story 5.25) subtracts; an Owner top-up touches no drawer

**Given** "Spend" with category (the Story 5.25 list), amount, note and a receipt photo (resized to at most 200 KB)
**When** `petty.spend` applies
**Then** the balance falls and the drawer is unaffected; the Story 5.25 Expense form gains "Petty cash" as a third "Paid from" choice that writes the same entry

**Given** a spend larger than the balance
**When** it reaches `apply()`
**Then** it is rejected `PETTY_SHORT` and nothing is written

**Given** "Return to drawer" with an open shift
**When** `petty.return` applies
**Then** the balance falls and a `cash_events` row of new kind `petty_return` adds to expected drawer cash

**Given** any petty entry
**When** the Undo toast shows
**Then** `.reverted` within 8 s removes it (and its `cash_events` row) under the Story 3.14 rules, unless that would take the balance below zero

**Given** the Z-report (Story 5.28)
**When** a shift with petty movements closes
**Then** it lists drawer top-ups to and returns from petty cash as their own lines

### Story 14.7: Count petty cash and report its spending

As a branch manager,
I want to count the petty cash box and see what it was spent on,
So that the box matches the ledger and I can show the owner where the money went.

**Covers:** FR95 (ledger report and reconciliation), AD-30, UX-DR74, UX-DR75, UX-DR66, UX-DR88 (Reports)

**Acceptance Criteria:**

**Given** Petty cash → Count
**When** a Manager opens it
**Then** the Story 5.26 `denomination-counter` and `variance-readout` run in the blind state, and after the count shows Counted, Ledger and Difference ("Counted ₹1,450. Ledger ₹1,500. Difference −₹50 short.")

**Given** a non-zero difference
**When** the Manager confirms with a reason and their PIN in place
**Then** `petty.count` writes a `count_adjust` entry for the difference so the balance equals the count, and records counted, ledger and reason in the audit

**Given** a zero difference
**When** confirmed
**Then** a count entry is recorded with no adjustment and "Last counted" updates

**Given** Reports → Petty cash for a range
**When** it renders
**Then** it shows opening balance, top-ups by source, spending by category, returns, count adjustments and closing balance, with each spend's receipt photo viewable, and exports to CSV and PDF

**Given** a petty balance that was not counted for 7 days with any spending in that time
**When** the Manager signs in
**Then** a Hub alert "Petty cash not counted for 7 days" is raised once until the next count

### Story 14.8: Staff clock in and out, tied to their shift

As counter staff,
I want to clock in when I arrive and out when I leave, in the same place I open and close my shift,
So that my hours are recorded without a second system.

**Covers:** FR96 (clock in/out tied to shift), AD-2, AD-30, UX-DR66, UX-DR88 (Shift), UX-DR103, UX-DR104

**Acceptance Criteria:**

**Given** a signed-in staff member who is not clocked in
**When** they choose Shift → "Clock in"
**Then** `attendance.clock_in` writes an `attendance` row (new table: id, staff_id, clock_in_at, clock_out_at null, source `self|manager`, note, `business_day`, `rev`) and the header shows "Clocked in 9:02 am" beside their name

**Given** "Open shift" (Story 5.10) by someone not clocked in
**When** it opens
**Then** the panel offers one action "Clock in and open shift" that applies both intents in one transaction; the shift row stores the `attendance_id`

**Given** a clocked-in staff member with an open shift
**When** they close it (Story 5.26)
**Then** after the close the panel asks in place "Clock out too?" with Clock out and Stay clocked in; Clock out applies `attendance.clock_out`

**Given** a clock-out
**When** the staff member still has an open shift
**Then** it is refused `SHIFT_STILL_OPEN` with "Close your shift first, or ask a manager to hand it over."

**Given** a second clock-in while clocked in, or a clock-out while not clocked in
**When** attempted
**Then** it is refused `ALREADY_CLOCKED_IN` or `NOT_CLOCKED_IN` and nothing changes

**Given** an attendance row still open when the business day rolls
**When** the roll happens
**Then** it is left open and flagged "Missing clock-out" on the Shift screen for Managers; nobody is clocked out by the system

**Given** a sign-in with PIN at the Hub
**When** the staff member clocks in or out
**Then** the actor is the signed-in staff member; nobody can clock in for someone else

### Story 14.9: Hours per staff in reports, with manager corrections

As a café owner,
I want to see how many hours each person worked and fix forgotten clock-outs,
So that pay and rotas are based on real hours.

**Covers:** FR96 (hours per staff in reports), AD-30, UX-DR66, UX-DR88 (Reports), UX-DR100

**Acceptance Criteria:**

**Given** Reports → Staff (Story 5.30) for a range
**When** it renders
**Then** each staff member also shows clocked hours (sum of closed attendance rows overlapping the range, split at range edges), number of attendances and missing clock-outs, and the report's total hours

**Given** Reports → Staff → Attendance
**When** it opens
**Then** it lists every attendance row in the range (staff, in, out, duration, shifts linked, source) with open rows flagged "Missing clock-out" first

**Given** a Manager editing an attendance row (permission `attendance.edit`)
**When** they set or change clock-in or clock-out with a required reason and their PIN in place
**Then** `attendance.correct` stores the new times with `source = manager`, and the audit keeps the original times, the editor and the reason

**Given** a correction where out is before in, or a row overlapping another row of the same person
**When** saved
**Then** it is refused `BAD_TIMES` and nothing changes

**Given** a Manager adding a missed attendance for someone
**When** saved
**Then** `attendance.correct` creates a closed row with `source = manager`

**Given** the export
**When** Attendance exports to CSV
**Then** it has one row per attendance with times in branch local time and duration in hours and minutes

### Story 14.10: Prep stations route kitchen items into a ticket per station

As a café owner with a bar and a kitchen,
I want drinks to go to the bar and food to the kitchen as separate tickets,
So that each station sees only its own work.

**Covers:** FR102 (prep stations, separate ticket per station), AD-2, AD-29 (`kitchen` flag), AD-39, UX-DR24, UX-DR65, UX-DR88 (Settings)

**Acceptance Criteria:**

**Given** Settings → Kitchen on a branch with the `kitchen` flag
**When** a Manager adds a prep station
**Then** `station.upsert` stores it in a new `prep_stations` table (id, name, sort, active, kot_printer null, `rev`) with at most 8 active stations; names are unique per branch

**Given** a kitchen-flagged product (Story 5.3)
**When** a Manager edits it
**Then** a "Prep station" picker lists the active stations plus "Default" and stores `products.station_id`; the Owner app product edit (Manage → Products) shows the same picker filled from the summary `catalogue`

**Given** a placement with kitchen lines for more than one station
**When** `order.place` commits
**Then** one `kitchen_tickets` row per station is created in the same transaction, each holding only that station's lines, sharing the order id and seat label, with ticket numbers `K-<n>` allocated in order and the station id stored on the ticket; lines whose product has no station go to the "Default" ticket

**Given** a branch with no stations defined
**When** an order is placed
**Then** exactly one ticket per placement is created, as in Story 7.3

**Given** the tile sheet's order section
**When** an order has several station tickets
**Then** each ticket row shows its station name; "Mark ready" acts per ticket, and the Epic 6 `order.ready` HUD copy fires when every ticket of that placement is Ready

**Given** a station deactivated while it has open tickets
**When** the change applies
**Then** open tickets stay on it until served or cancelled, and its products fall back to "Default" for new orders

**Given** the `proptest` generator
**When** orders with mixed stations, advances and cancels run
**Then** every kitchen line belongs to exactly one ticket and every ticket's lines share one station

### Story 14.11: Each station gets its own KOT printer and its own kitchen board

As a bartender,
I want only bar tickets to print at the bar and show on the bar tablet,
So that I'm not sorting through food orders.

**Covers:** FR102 (per-station tickets reach their station), FR97 (KOT ≤ 3 s), FR98, AD-28, AD-42 (`kds` scope), NFR3, UX-DR77, UX-DR78, UX-DR95

**Acceptance Criteria:**

**Given** a station's settings
**When** the Manager sets its KOT printer (Receipt printer, the branch KOT printer, Another printer via the Story 2.27 flow, or None)
**Then** it is stored on the station; "Default" uses the branch KOT printer from Story 7.1

**Given** a station ticket created
**When** its order commits
**Then** a `kot` side-effect is queued to that station's printer and the KOT prints the station name under the "KOT" title, within 3 s on reference hardware; a station set to None prints nothing

**Given** one station's printer is offline
**When** its KOT fails
**Then** the "Printer offline" alert names the station's printer, other stations still print, and Reprint is offered on the failed ticket only

**Given** a paired kitchen display's entry in Settings → Kitchen
**When** the Manager sets "Shows" to All stations or one or more stations
**Then** the setting is stored on the `kitchen_displays` row, the board feed sends only those stations' tickets, and the `kitchen-top-bar` shows the station names after the branch name

**Given** a display limited to stations
**When** it calls `kitchen.advance` on a ticket of another station
**Then** it is refused `FORBIDDEN` and logged

**Given** the KOT golden tests
**When** station tickets render at 58 and 80 mm
**Then** the byte output matches stored golden files with the station line within the width

### Story 14.12: Item-level prep timing and the prep-time report

As a kitchen manager,
I want to know how long each item takes from order to ready and served,
So that I can fix slow items and staff the busy stations.

**Covers:** FR102 (item-level timing), AD-2, AD-7, UX-DR78, UX-DR88 (Reports), UX-DR110

**Acceptance Criteria:**

**Given** a kitchen ticket line
**When** its ticket is created, moves to Preparing, Ready and Served
**Then** the line records `placed_at`, `started_at`, `ready_at` and `served_at` on `kitchen_ticket_lines` (stamped from Hub time in `apply()`), and a `.reverted` advance clears the stamp it set

**Given** a ticket in Preparing on the board or the tile sheet
**When** kitchen staff tap one line
**Then** `kitchen.line_ready` stamps that line's `ready_at`, the line shows a check glyph and "Done" (not colour alone), and the ticket stays in Preparing; when the last line is done the ticket moves to Ready by itself through the normal `kitchen.advance`

**Given** a ticket advanced to Ready with lines not individually marked
**When** it advances
**Then** those lines take the ticket's ready time

**Given** a line tapped by mistake
**When** Undo is pressed within 8 s on the board
**Then** `kitchen.line_ready.reverted` clears its `ready_at`

**Given** Reports → Kitchen for a range
**When** it renders
**Then** it shows per station and per product the count of items, median and 90th-percentile order-to-ready and ready-to-served times, and the slowest hour of the day, excluding cancelled lines; it exports to CSV and PDF

**Given** the `kds` scope table
**When** this story ships
**Then** it gains `kitchen.line_ready` and its revert only, and the CI scope test is updated

### Story 14.13: Kiosks and Pis report device health to the Hub

As a branch manager,
I want each PC and TV Pi to report its version and hardware to the Hub,
So that the Hub always holds a current health record for every device.

**Covers:** FR165 (last seen, kiosk version, GPU/RAM — data), AD-25, AD-8, NFR2

**Acceptance Criteria:**

**Given** the kiosk service
**When** it sends `hello` and then every 5 minutes while connected
**Then** it reports `health{kiosk_version, os_build, cpu_model, ram_total_mb, ram_used_pct, gpu_model, gpu_driver, disk_free_gb, uptime_s}` collected by the service (WMI) without measurable GPU use; the Pi agent reports `health{agent_version, board, ram_total_mb, ram_used_pct, cec}` the same way

**Given** a health frame
**When** the Hub receives it
**Then** it upserts a `device_health` row (new table, one per device, not part of the change log: device_id, last_seen_at, fields above, updated_at) outside `apply()` and the change log: device health is an allowed exception to the single-writer rule because it is telemetry, not business state — it is never synced to the cloud as batches, never in snapshots or exports, and never audited

**Given** a kiosk from the previous two releases that does not send `health`
**When** it connects
**Then** last seen still updates from the connection and the other health fields stay empty

**Given** a health frame from a device class other than paired kiosks and Pis, or with fields outside the schema
**When** the Hub receives it
**Then** it is dropped and logged; the `kds` and `phone` classes cannot send `health`

**Given** a kiosk running a game
**When** the service collects health every 5 minutes for an hour
**Then** the collection adds no measurable GPU load and under 1% CPU on the reference PC

### Story 14.14: The Hub shows device health and records device outages

As a branch manager,
I want to see each PC's last contact, kiosk version and hardware at a glance, and how long devices were offline,
So that I can spot a failing or outdated PC before a player complains.

**Covers:** FR165 (last seen, kiosk version, GPU/RAM shown; offline time), AD-25, NFR2, UX-DR23, UX-DR88 (Settings → Devices), UX-DR97

**Acceptance Criteria:**

**Given** Settings → Devices
**When** it opens
**Then** each device row shows name, group, kind, last seen ("Online" or "Last seen 8:42 pm"), version, RAM and GPU summary, with a warning `status-badge` "Old version" when the kiosk is more than one release behind the Hub and "Low disk" under 5 GB free

**Given** a device sheet on the floor (Story 3.5)
**When** opened
**Then** a "Health" line shows version, last seen, RAM used and GPU, and a device in maintenance (Story 2.24) is marked as such

**Given** the branch summary
**When** it is published (Story 8.4)
**Then** it gains a `devices` block per device (id, name, group, kind, online, last_seen_at, version, ram_total_mb, gpu_model, maintenance) with no player data

**Given** a device whose `device_health` row has no health fields (an older kiosk)
**When** Settings → Devices or the sheet renders
**Then** those fields show "—"

**Given** a paired kiosk or Pi
**When** its connection drops and later returns
**Then** the Hub records the interval in a new `device_outages` telemetry table (device_id, from, to null while open; kept 400 days, outside `apply()` and the change log like `device_health`), from which offline time can be reported

### Story 14.15: Rollup schema v3 adds device utilisation, staff, kitchen, device and game-hours fields

As a café owner on a paid tier,
I want my Hub to put the numbers behind the deeper dashboards into its daily rollup,
So that heatmaps and staff rankings come from my branch's own data and never from a cloud copy of it.

**Covers:** FR160 (utilisation, staff data), FR96 (hours), FR137 (hours per game feed owner reports — data), AD-35 (versioned rollups, writers for N and N−1), AD-7, AD-24, AD-8, NFR5 (rollup < 5 s), NFR10, NFR13

**Acceptance Criteria:**

**Given** `arena-proto`
**When** rollup schema v3 is defined
**Then** it is Epic 13's v2 (Story 13.29) plus `utilisation: [{group_id, group_name, hour (0–23 local), device_minutes, capacity_minutes}]`, `staff: [{staff_id, name, sales, sessions_started, discounts: {count, value}, voids, refunds, shifts, variance_total, variance_abs_max, hours}]`, `kitchen: [{station_id?, station_name, items, median_ready_s, p90_ready_s}]` `devices: {offline_minutes_open_hours: [{device_id, minutes}]}` (from `device_outages` clipped to opening hours) and `games: [{game_id, name, minutes, sessions}]` (play hours per library game from the Story 6.27 `game_plays` rows of the business day, with the game's name so owners never need a cloud lookup), in integer minor units, with no phone numbers or names of players (player ids appear nowhere in the new fields)

**Given** the utilisation block
**When** it is built
**Then** `device_minutes` sums session minutes per device group per local hour, split at hour edges, and `capacity_minutes` is 60 × devices in the group not in maintenance during that hour within opening hours

**Given** the games block
**When** it is built
**Then** each game's `minutes` equals the sum of that day's `game_plays` minutes split at the business-day edge, guests count toward minutes and sessions, and v1's `play_hours` is still written unchanged

**Given** golden tests with the Story 13.29 fixture extended with a 24-hour day, attendance, kitchen timings and game plays
**When** the v3 rollup is built
**Then** the JSON equals the expected file exactly, and every v2 field is byte-identical to the v2 build of the same day

**Given** a licence pinning `rollup_schema = 2` (the Story 13.29 pin)
**When** a rollup is built
**Then** the Hub emits v2 (it keeps writers for v2 and v3), and when the pin rises to 3 the Story 8.7 regeneration rebuilds past days oldest first at most 10 per minute; until a licence pins 3, v3 is built only in tests

**Given** the reference PC and the Story 8.5 busy day
**When** the v3 rollup runs
**Then** it completes in under 5 s, and the Story 4.17 device-farm job checks the same on a 3 GB Android device

### Story 14.16: Rollup schema v3 gains the player cohort block

As a café owner on a paid tier,
I want my Hub to compute new, returning and weekly retained players into the rollup,
So that cohort charts come from my branch's own data, with no player names leaving the café.

**Covers:** FR160 (player retention cohorts — data), AD-35, AD-7, AD-24, NFR5, NFR13

**Acceptance Criteria:**

**Given** rollup schema v3 from Story 14.15
**When** this story ships
**Then** v3 gains `players: {new, returning, active, cohorts: [{cohort_week, size, retained: [w1..w11]}]}` with counts only — no player id, name or phone

**Given** the cohort block
**When** it is built for a business day
**Then** it covers the 12 ISO weeks ending in that day's week, where a player's cohort week is the week of their first settled session or sale and `retained[k]` counts the cohort's players with a settled session or sale in cohort week + k

**Given** guests, erased players and players whose only activity was undone
**When** cohorts are built
**Then** they are excluded, and an erasure later lowers the cohort sizes of regenerated days only

**Given** golden tests with the Story 14.15 fixture extended with 12 weeks of players (new, returning, lapsed, erased)
**When** the block is built
**Then** it equals the expected file exactly, and new + returning = active for the day

**Given** the reference PC with 12 weeks of history and 5,000 players
**When** the full v3 rollup runs
**Then** the full v3 rollup still completes in under 5 s, and the 3 GB Android device-farm check passes

### Story 14.17: The cloud accepts rollup v3 and the owner API serves the new fields

As a café owner on a paid tier,
I want the Owner app to read the new rollup fields as soon as my branch sends them,
So that new reports appear after one Hub update, while branches that haven't updated yet keep working.

**Covers:** FR160 (data path), FR137 (game hours data path), AD-35 (cloud accepts N and N−1), AD-8, AD-10 (`rollup_schema` claim), NFR7, NFR25

**Acceptance Criteria:**

**Given** the cloud release that knows v3
**When** it is deployed (cloud first)
**Then** the accepted set becomes {3, 2}; a rollup with any other schema counts as missing with `ROLLUP_SCHEMA_OLD`

**Given** the per-tier `rollup_schema` pin from Story 13.29
**When** an operator raises it to 3 for a tier with the audited `arena-cloud ops rollup-pin --tier <t> --schema 3 --reason <text>`
**Then** the next heartbeat of each branch on that tier signs a licence with `rollup_schema = 3`; a Hub whose heartbeat reports it cannot write v3 keeps `rollup_schema = 2`, and raising the pin to 3 is refused while the cloud's accepted set lacks 3

**Given** `GET /v1/branches/{id}/summary?from=&to=&fields=players,utilisation,staff,kitchen,devices,games`
**When** it is called
**Then** the cloud sums `utilisation`, `staff`, `kitchen` counts, `devices` and `games` minutes and sessions across the range in memory, takes `players.cohorts` from the latest v3 day in the range, and lists in `pre_v3_days` any day whose file is older than v3 (so the app can say the new fields are missing for those days)

**Given** `branch=all`
**When** the new fields are requested
**Then** each branch is returned separately plus a total for counts and minutes; cohorts are never merged across branches

**Given** a Free branch or a branch not in the session's permitted branches
**When** the new fields are requested
**Then** it returns `FEATURE_NOT_ENTITLED` or `NOT_FOUND` as in Story 8.9

**Given** the jobs role's daily metrics pass
**When** it runs
**Then** the Story 13.29 "rollup missing-day count by schema" metric includes v3, so a stuck v2 fleet is visible

### Story 14.18: Reports → Players with new, returning and weekly retention cohorts

As a café owner,
I want to see how many new players come back week after week,
So that I can tell whether my promotions build regulars or one-time visits.

**Covers:** FR160 (player retention cohorts, weekly chart), AD-35, NFR7, NFR15, UX-DR25, UX-DR81, UX-DR90 (Reports), UX-DR100, UX-DR110, UX-DR113

**Acceptance Criteria:**

**Given** Reports in the Owner app
**When** it opens on a paid branch
**Then** a new Players tab sits after Credit, with the range and branch pickers of Story 8.15

**Given** Players for one branch
**When** it renders
**Then** `metric-figure`s show new, returning and active players for the range, and a cohort chart shows one row per cohort week (week label, cohort size) and one cell per following week with the retained percentage, shaded by a single sequential scale and always printed as a number, so colour is never the only cue

**Given** the cohort chart
**When** used with a keyboard or screen reader
**Then** it is a real table with row and column headers, each cell's accessible name reads "Week of 6 Oct, 42 players, week 3: 31%", and a "Show as table" toggle is not needed because the table is the chart

**Given** All branches
**When** chosen
**Then** each branch's cohort chart is shown one after another (never merged), each with its own data age

**Given** days in the range from before v3 (`pre_v3_days`)
**When** the tab renders
**Then** a muted line says "Player cohorts start from 14 Nov, when this branch updated." and figures cover only v3 days

**Given** a branch with fewer than 5 players in every cohort
**When** it renders
**Then** it shows "Not enough players yet to show cohorts." instead of the chart

**Given** Export
**When** chosen
**Then** CSV and PDF exports of the tab are produced as in Story 8.17

### Story 14.19: Reports → Devices with a utilisation heatmap and device health

As a café owner,
I want to see which device groups are busy at which hours and which PCs are failing,
So that I can set happy hours and fix hardware before it costs me sessions.

**Covers:** FR160 (device utilisation heatmap per group per hour), FR165 (owner view of health), AD-35, NFR15, UX-DR23, UX-DR81, UX-DR90 (Reports), UX-DR100, UX-DR110, UX-DR113

**Acceptance Criteria:**

**Given** Reports → Devices for one branch and a range
**When** it renders
**Then** a heatmap shows device groups as rows and the 24 local hours as columns, each cell the utilisation % (device minutes ÷ capacity minutes over the range), shaded on one sequential scale with the number printed in every cell; hours outside opening hours show "—"

**Given** the heatmap
**When** used with a keyboard or screen reader
**Then** it is a table with headers, each cell's name reads "Gold PCs, 7 pm: 86%", and arrow keys move between cells

**Given** a phone width
**When** the heatmap does not fit
**Then** the hour columns scroll inside the card with the group column fixed; the page itself never scrolls sideways

**Given** the Devices tab's health section
**When** it renders
**Then** it lists devices from the live summary `devices` block with name, group, online or last seen, version and GPU/RAM, offline devices first, plus each device's offline minutes during opening hours over the range from rollups

**Given** All branches
**When** chosen
**Then** each branch is shown in turn with its own heatmap and health list

**Given** pre-v3 days in the range
**When** it renders
**Then** the muted "starts from" line of Story 14.18 is shown and the heatmap covers v3 days only

### Story 14.20: Reports → Staff sorted by variance, and branch comparison

As a café owner with several branches,
I want to rank staff by cash variance and put my branches side by side,
So that I see who needs help with the drawer and which branch lags.

**Covers:** FR160 (staff sorted by variance, branch comparison), FR96 (hours), AD-35, NFR10, UX-DR25, UX-DR81, UX-DR90 (Reports), UX-DR103 (no-blame voice)

**Acceptance Criteria:**

**Given** Reports → Staff for a range
**When** it renders
**Then** a table lists each staff member with sales, sessions started, discounts (count and value), voids, refunds, shifts, total signed variance, largest single variance and hours, sorted by absolute total variance by default, with sortable column headers exposed as such to screen readers

**Given** a staff member over the branch threshold on any shift
**When** the row renders
**Then** it shows a warning edge and "Over threshold on 2 shifts" in text; copy never says "short by <name>" or blames a person

**Given** Reports → Compare branches (shown when the tenant has two or more paid branches)
**When** it opens for a range
**Then** a `card` per branch shows revenue, revenue per device, sessions, average utilisation, new players, cash variance total and stock waste value side by side, each with its data age, and branches in different currencies are shown in their own currency with no cross-currency ranking

**Given** the comparison
**When** the owner picks a metric to sort by
**Then** branches are ordered by it with a rank number in text

**Given** both reports
**When** exported
**Then** CSV and PDF are produced as in Story 8.17

### Story 14.21: Reports → Games shows play hours per game

As a café owner,
I want to see which games my players spend the most hours on at each branch,
So that I keep the right games installed and promote what my players love.

**Covers:** FR137 (hours per game feed owner dashboards), FR160, AD-35 (rollups only), NFR7, NFR15, UX-DR25, UX-DR81, UX-DR90 (Reports), UX-DR100, UX-DR113

**Acceptance Criteria:**

**Given** Reports in the Owner app on a paid branch
**When** it opens
**Then** a Games tab sits after Devices, with the range and branch pickers of the Epic 8 "Reports — revenue and cash reconciliation" story

**Given** Games for one branch and a range
**When** it renders
**Then** it lists the top games by play hours from the rollup `games` block (Story 14.17 API), each with name, hours and minutes in tabular figures, sessions and share of all play hours, sorted by hours with a rank number in text, top 20 shown with "Show all"

**Given** All branches
**When** chosen
**Then** each branch is listed separately with its own data age, plus an all-branches list that sums hours by game id across the permitted branches

**Given** pre-v3 days in the range
**When** it renders
**Then** the muted "starts from" line of Story 14.18 is shown and the figures cover v3 days only

**Given** a branch with no game plays in the range
**When** it renders
**Then** it shows "No game plays recorded yet."

**Given** a Free branch, or a Read-only or Full member without that branch
**When** Games is requested
**Then** the Free branch shows the upgrade path and the API returns `FEATURE_NOT_ENTITLED` or `NOT_FOUND`

**Given** Export
**When** chosen
**Then** CSV and PDF are produced as for the other owner reports, with the same figures as the screen

### Story 14.22: The owner is alerted when a PC goes offline during opening hours

As a café owner,
I want to know when a PC has dropped off the network while the café is open,
So that a dead PC is fixed before the evening rush.

**Covers:** FR165 (device-offline alert during opening hours), AD-27, AD-5 (`settings.set` thresholds), UX-DR96, UX-DR103

**Acceptance Criteria:**

**Given** a kiosk or Pi not in maintenance that has had no connection for 10 minutes
**When** the current branch time is inside opening hours
**Then** the Hub writes one `notify.requested` owner_alert of kind `device_offline` (branch, device name, group, last seen) once per outage, and a Hub alert "PC-07 offline for 10 min" appears with the Story 3.25 badge

**Given** a device that comes back
**When** it reconnects
**Then** the outage ends, a later outage alerts again, and the owner's Alerts list gets a "PC-07 back online" entry (push only)

**Given** five or more devices go offline within the same minute
**When** the Hub raises alerts
**Then** it sends one alert "6 PCs offline at <branch>" listing them instead of six, since a switch or power fault is the likely cause

**Given** a device placed in maintenance or revoked
**When** it is offline
**Then** no alert is raised

**Given** Alerts → Thresholds in the Owner app
**When** the owner changes "Device offline after" (5–60 minutes) or turns the alert off
**Then** a non-sensitive `settings.set` updates `owner_alert_thresholds` at the targeted branches

**Given** the whole branch offline
**When** the Hub itself is down
**Then** device alerts are not raised (the Hub cannot see them) and the Story 8.35 branch-offline alert covers it

### Story 14.23: Owners choose daily or weekly scheduled reports on WhatsApp

As a café owner,
I want to choose which branches' reports I get on WhatsApp each morning or each week,
So that the numbers come to me without opening the app.

**Covers:** FR159 (schedule settings), AD-27, AD-34 (adapter), UX-DR18, UX-DR25, UX-DR90 (Reports → Exports), UX-DR113

**Acceptance Criteria:**

**Given** Reports → Exports → Scheduled reports on a tenant with a paid branch
**When** the owner adds a schedule
**Then** they pick frequency (Daily · Weekly, `tab-control`), branches (one, several or All), contents (Revenue, Cash reconciliation, Stock, Players, Staff; Revenue and Cash on by default) and it is sent to the owner's verified phone on WhatsApp only, stored in a new `report_schedules` table in `gz_control` (id, tenant, owner_id, frequency, branch ids or all, sections, timezone from the first branch, active)

**Given** the owner has turned WhatsApp off in Alerts → Notification settings (Story 8.33)
**When** they save a schedule
**Then** the form says "Scheduled reports are sent on WhatsApp. Turn WhatsApp on to receive them." and the schedule saves paused

**Given** a schedule saved
**When** the list renders
**Then** it shows each schedule with "Next: tomorrow by 8:00 am" in the owner's timezone and Pause, Edit and Delete

**Given** a Free branch
**When** branches are listed
**Then** it is not selectable and shows `tier-lock` "Available on Basic"

**Given** a branch that leaves the tenant's paid set (lapse or downgrade to Free)
**When** the next send runs
**Then** it is left out with a line "Not included: <branch> is on Free", and the schedule is not deleted

**Given** "Send a test now"
**When** pressed
**Then** the latest available report for the schedule is sent at once on WhatsApp, rate-limited to 3 per hour

### Story 14.24: Scheduled reports are built from rollups and arrive by 8 am local

As a café owner,
I want yesterday's or last week's figures on WhatsApp before 8 in the morning,
So that I can check the café with my morning tea.

**Covers:** FR159 (arrive by 8 am local), AD-35 (rollups only), AD-27, AD-34, NFR10, NFR20, NFR25, UX-DR96

**Acceptance Criteria:**

**Given** the jobs role
**When** it is 07:00 in a schedule's timezone (daily), or 07:00 on Monday (weekly, covering the previous Monday–Sunday)
**Then** it builds the report from the same owner read API sums as Story 8.17 (rollups only, never branch rows) and queues one WhatsApp send with an idempotency key of (schedule, period); there is no email channel and no email provider

**Given** a send
**When** it goes out through the messaging adapter (Gupshup, recorded fake in tests)
**Then** it uses the approved `scheduled_report` WhatsApp template with the period, branch count, and revenue, sessions and cash variance per branch (at most 5 branches, then "+3 more") and a link to Reports for the full range, with Export there for CSV or PDF; every figure equals the Owner app's report for the same range to the minor unit

**Given** a branch whose rollup for the period is missing at 07:00
**When** the report is built
**Then** the job retries every 15 minutes until 07:45, then sends with that branch marked "No data yet — the branch hasn't sent yesterday's figures", never delaying past 08:00

**Given** a provider failure
**When** a send fails
**Then** it is retried with backoff until 08:00; a send still failing at 08:00 is recorded as late and the owner sees "Yesterday's report didn't arrive — open Reports" in Alerts

**Given** the metrics
**When** sends complete
**Then** "scheduled reports delivered by 8 am" is exported as a percentage per day, and a duplicate run for the same (schedule, period) never sends twice

### Story 14.25: The owner-login add-on can be bought for a branch

As a café owner,
I want to buy extra owner logins for a branch,
So that my partner or manager can see that branch in the Owner app with their own account.

**Covers:** FR161 (owner-login add-on), FR178 (add-on ₹50 per branch per month), FR179 (prorated mid-cycle), AD-20, AD-29, UX-DR93, UX-DR24

**Acceptance Criteria:**

**Given** the control-plane flag `addon_owner_login_enabled`
**When** it is switched on
**Then** the add-ons panel in wizard step 2, Upgrade and "Change plan" (Stories 10.4, 10.5, 10.9) shows "Owner login" with its band price and a quantity of 0–10 instead of "Available soon"

**Given** a paid branch and a quantity above 0
**When** the owner pays or confirms the change
**Then** the subscription carries `addon_owner_login × n` with the Story 10.9 proration, and the branch's `entitlements` row records `owner_logins = n` when the webhook is applied

**Given** a Free branch
**When** the add-ons panel renders
**Then** Owner login is not offered, since the Owner app on Free shows only Subscription, pairing and support

**Given** the subscription invoice (Story 10.7)
**When** it is issued
**Then** the add-on appears as its own line with quantity

**Given** a lapse or downgrade to Free
**When** the entitlement drops
**Then** `owner_logins` becomes 0 for that branch and no command or Hub state changes

### Story 14.26: Invite a team member with a read-only or full role

As a café owner,
I want to invite someone by phone, choose which branches they can see and whether they can change things,
So that my partner can manage prices while my accountant only reads reports.

**Covers:** FR161 (additional owner logins, read-only or full roles), AD-10, AD-23, NFR11, UX-DR18, UX-DR83, UX-DR90 (Account), UX-DR113

**Acceptance Criteria:**

**Given** Account → Team (shown to the tenant's primary owner only)
**When** it opens
**Then** it lists members with name, phone, role (Owner · Full · Read-only), branches and status (Invited · Active · Removed), and each branch's seats as "2 of 3 owner logins used"

**Given** "Invite" with name, phone, role Full or Read-only and one or more branches, and step-up MFA passed
**When** sent
**Then** a `tenant_members` row (new table: id, tenant, owner_id null, phone, name, role `full|read_only`, branch ids, status `invited`, invited_by) is written and one seat is reserved on each chosen branch; an invite link goes by WhatsApp through the messaging adapter, valid 72 hours

**Given** a branch with no free owner-login seat
**When** it is chosen
**Then** it is shown disabled with "No free owner login — add one in Subscription"

**Given** the owner lowers a branch's Owner login quantity below the seats in use there
**When** they try to save it
**Then** it is refused `SEATS_IN_USE` with "3 people use this branch. Remove 1 before lowering to 2."

**Given** a phone already a member of the tenant
**When** it is entered
**Then** the invite is refused `ALREADY_MEMBER`

**Given** an invite not yet accepted
**When** the primary owner chooses Resend or Cancel
**Then** a new 72-hour link replaces the old one, or the invite is cancelled and its seats freed

**Given** the primary owner changes a member's role or branches, or removes them, with step-up MFA
**When** saved
**Then** the member row is updated (or marked `removed`, and a pending invite is cancelled), the seat is freed on removal, and the change is in the admin-visible tenant audit

**Given** the tenant's primary owner
**When** any member tries to change Team, Subscription or the primary owner
**Then** it is refused `FORBIDDEN`; only the primary owner can, and the primary owner cannot be removed

### Story 14.27: An invited team member signs up with OTP, password and TOTP

As someone invited to help run a café,
I want to accept the invite from my phone and secure my own login,
So that I get into the Owner app with my own account and never use the owner's.

**Covers:** FR161 (additional owner logins), FR2 (password and TOTP for every login), AD-10, AD-23, NFR11, NFR15, UX-DR18, UX-DR112, UX-DR113

**Acceptance Criteria:**

**Given** the invitee opens the link
**When** they confirm their phone by OTP
**Then** they set a password and enrol TOTP with backup codes exactly as in Stories 2.4–2.5 (an existing `owners` account signs in instead), the member becomes `active`, and the primary owner sees "Neha joined"

**Given** an expired or cancelled invite link
**When** it is opened
**Then** it shows "This invite has expired. Ask the owner for a new one." and nothing is created

**Given** wrong OTP codes
**When** 5 are entered
**Then** the challenge is void as in Story 2.4, and per-phone send limits apply

**Given** a member who joined
**When** they sign in on a new device later
**Then** the Story 2.6 TOTP, backup-code and SMS-fallback rules apply to them exactly as to the primary owner

**Given** the sign-up pages
**When** an axe scan runs in both themes
**Then** there are zero serious or critical violations, paste is allowed in every code field, and logs hold no phone number in clear

### Story 14.28: Team roles are enforced on every owner read and command

As a café owner,
I want each team member to see only their branches and a read-only member to never send a change,
So that giving someone access is safe.

**Covers:** FR161 (read-only vs full enforced), AD-5, AD-23, AD-30 (actor), NFR11, UX-DR24, UX-DR83, UX-DR90

**Acceptance Criteria:**

**Given** a signed-in member
**When** any owner API handler runs
**Then** the session's permitted branch ids are the member's branches that still have the owner-login entitlement, and every read for another branch returns `NOT_FOUND`, as for another tenant's branch

**Given** a Read-only member
**When** they post any command
**Then** the cloud refuses it `FORBIDDEN`, and the app hides Send, Manage edits and command actions, showing "Read-only access" in their place

**Given** a Full member
**When** they send a command
**Then** it carries `issued_by` = the member and the Hub applies it with `actor{kind: owner, id: <member id>}`, so the Hub audit, toasts and owner audit search name the member; sensitive kinds still need their own step-up MFA

**Given** a Full member choosing "All branches"
**When** the preview opens
**Then** "All" means only their permitted branches

**Given** alerts and scheduled reports
**When** a member is active
**Then** they can turn on push and WhatsApp alerts and schedules for their own branches only

**Given** a branch that lapses or drops to Free
**When** its `owner_logins` entitlement becomes 0
**Then** every member loses that branch on their next request, seats are kept on the member rows, and access returns by itself if the branch is upgraded again with enough seats

**Given** a member whose role or branches were changed in Account → Team (Story 14.26)
**When** their next request arrives
**Then** the new role and branches apply to it

**Given** a member removed in Account → Team
**When** the removal is saved
**Then** all of that member's sessions are revoked at once

**Given** a removed member or a branch that lost its seat mid-session
**When** the next request arrives
**Then** it is refused `UNAUTHORIZED` or `NOT_FOUND`, cached screens on their device are cleared on that response, and queued offline commands are dropped with "Your access changed — this change wasn't sent"

**Given** an integration test with three members (Full on A, Read-only on A and B, removed)
**When** it calls every owner endpoint and command kind
**Then** each response matches the role and branch matrix, and the test fails on any leak

### Story 14.29: Refunds above the owner's threshold wait for approval at the Hub

As a branch manager,
I want a large refund to be held for the owner instead of paid out at once,
So that big refunds get a second look and I'm covered.

**Covers:** FR163 (threshold, pending refund), AD-2, AD-5 (`settings.set`), AD-30, UX-DR66, UX-DR101, UX-DR103

**Acceptance Criteria:**

**Given** Alerts → Thresholds in the Owner app on a paid branch
**When** the owner sets "Refunds above this need my approval" (off by default) to an amount
**Then** a non-sensitive `settings.set` stores `refund_approval_minor` at the targeted branches

**Given** a refund (Story 5.24) whose total is above `refund_approval_minor`
**When** a manager submits it with their PIN
**Then** `refund.create` is rejected `APPROVAL_REQUIRED` and, in its place, `refund.approval_request` writes a `refund_approvals` row (new Hub table, distinct from Epic 10's cloud `refund_requests` for subscription refunds: id, order_id, lines and quantities, method, return-to-stock flags, amount_minor, reason, requested_by, state `pending`, `rev`) with no payment, credit note, stock or cash change

**Given** a pending request
**When** it commits
**Then** a `notify.requested` owner_alert of kind `refund_pending` (branch, invoice number, amount, reason, staff name; no player data) is written with `not_before` = commit + 8 s, and the refund screen and the order show "Waiting for owner approval"

**Given** a pending request
**When** the manager cancels it
**Then** `refund.approval_cancel` moves it to `cancelled`, the owner alert is followed by a "cancelled at branch" entry, and nothing else changes

**Given** a second refund request for the same invoice while one is pending
**When** submitted
**Then** it is refused `REFUND_PENDING`

**Given** the branch offline or on Free
**When** a refund over the threshold is needed
**Then** on paid it still waits (the request reaches the owner when the WAN returns); on Free the threshold cannot be set and Story 5.24 is unchanged

**Given** a request pending for 7 days
**When** the business day rolls
**Then** it becomes `expired` and the Hub shows "Approval expired — ask again"

### Story 14.30: The owner approves or declines a pending refund from the Owner app

As a café owner,
I want to see pending refunds and approve or decline each one from my phone,
So that the counter isn't stuck waiting for me to visit.

**Covers:** FR163 (owner approval command, pending alerts), FR154 (approve refund command), AD-5, AD-39, UX-DR23, UX-DR83, UX-DR90 (Today, Alerts), UX-DR96

**Acceptance Criteria:**

**Given** the Hub summary
**When** a branch has pending requests
**Then** the summary's `attention` items include each (request id, invoice number, amount, reason, staff name, requested_at) and Today's "Needs attention" shows "Refund of ₹1,800 waiting for you at <branch>" with Approve and Decline

**Given** the owner alert of kind `refund_pending`
**When** it is delivered
**Then** push and WhatsApp say "Refund of ₹1,800 at <branch> needs your approval" and deep-link to Alerts → Pending approvals, which lists every pending request across permitted branches

**Given** Approve, or Decline with a required reason, by the primary owner or a Full member
**When** sent
**Then** a `refund.approval_grant` or `refund.approval_decline` command (new non-sensitive kinds with `base_rev` = the approval row's `rev`) goes through the Story 8.21 preview to one branch and follows its `command-row` to Applied

**Given** the command at the Hub
**When** it applies
**Then** the request moves to `approved` or `declined` with the owner as actor; a request no longer `pending` (cancelled, expired or already decided) is rejected `REQUEST_CLOSED`, shown as "Couldn't apply at branch"

**Given** the Hub
**When** a decision applies
**Then** the alert service shows "Refund approved by owner — complete it" or "Refund declined by owner: <reason>" with sound and badge, on the refund screen and the order

**Given** a Read-only member
**When** they open Pending approvals
**Then** they see the list without Approve or Decline

### Story 14.31: Staff complete an approved refund at the counter

As a branch manager,
I want to pay out an approved refund in one step,
So that the customer gets their money as soon as the owner says yes.

**Covers:** FR163 (refund completes after approval), FR75, FR79, AD-26, AD-30, NFR10, UX-DR66, UX-DR95

**Acceptance Criteria:**

**Given** an `approved` request
**When** a manager opens it and presses "Complete refund" with their PIN
**Then** `refund.create{approval_id}` applies exactly the approved lines, quantities, method and return-to-stock choices, skips the threshold check, issues the credit note, and moves the request to `completed`

**Given** an approved cash refund
**When** no shift is open, or the drawer holds less than the amount
**Then** it is refused `SHIFT_NOT_OPEN` or `DRAWER_SHORT` (without revealing expected cash) and the request stays `approved`

**Given** the order received another refund since the approval
**When** completion would exceed what was paid
**Then** it is rejected `REFUND_EXCEEDS_PAID` and the manager must ask again

**Given** a completed refund
**When** the Undo toast shows
**Then** the Story 5.24 rules apply (no Undo once a credit note number is issued or a receipt printed)

**Given** the owner alerts for large refunds (Story 8.34)
**When** an approved refund completes
**Then** no second "large refund" alert is sent for it, since the owner already approved it

### Story 14.32: The owner sends a message to every kiosk of a branch

As a café owner,
I want to send a short message to every player in a branch from my phone,
So that I can announce closing time or an offer without calling the counter.

**Covers:** FR164, FR40, AD-5, AD-25, NFR3 (staff message ≤ 2 s at the branch), UX-DR35, UX-DR50, UX-DR83, UX-DR90 (Manage)

**Acceptance Criteria:**

**Given** Manage → Message kiosks on a paid branch
**When** the owner (primary or Full) writes up to 140 characters and picks one branch
**Then** a non-sensitive `kiosk.message` command (new kind; one branch only, "All branches" not offered) is sent through the preview with the branch's online state

**Given** the command at the Hub
**When** it applies
**Then** it applies `device.message{scope: all}` (Story 6.16) with `actor{kind: owner}`, every PC with an active session shows it as an info `hud-toast` within 2 s and keeps it in the bell, and the result reports how many PCs received it

**Given** the command row
**When** it reaches Applied
**Then** it reads "Delivered to 14 PCs" (or "No PCs in session — nothing shown")

**Given** a message command fetched more than 15 minutes after it was queued
**When** the Hub applies it
**Then** it is rejected `EXPIRED` and shown as "Couldn't apply at branch — message too old", because a late announcement would mislead players

**Given** text with a URL, phone number or more than 140 characters
**When** sent
**Then** the URL and phone are allowed as plain text (no links are clickable on the kiosk) and over-length text is refused in the form

**Given** a Read-only member
**When** they open Manage
**Then** Message kiosks is not shown

### Story 14.33: Tenant-wide pricing rules with branch overrides flagged

As a café owner with several branches,
I want one pricing rule I push to every branch, and to see where a branch has changed it,
So that my prices stay consistent and local exceptions are visible, not hidden.

**Covers:** FR162 (tenant-wide push, branch override flagged), AD-5, AD-39, FR156, UX-DR23, UX-DR76, UX-DR83, UX-DR90 (Manage → Pricing)

**Acceptance Criteria:**

**Given** Manage → Pricing → Pricing rules
**When** the owner creates a rule with scope "All branches (tenant rule)"
**Then** `rule.upsert` is sent to every permitted, entitled branch with a `tenant_rule_id` (ULID minted by the cloud) and the target group matched by group name at each branch; branches without a group of that name are listed in the preview as skipped with the reason

**Given** the rule at a Hub
**When** it applies
**Then** `pricing_rules` stores `tenant_rule_id` and `overridden = 0`, and the Hub's rule list shows it with a "From owner" tag

**Given** branch staff edit or disable a tenant rule at the Hub (Story 3.29, manager PIN)
**When** the edit commits
**Then** the row is marked `overridden = 1` with who and when, the Hub shows "This rule was set by the owner for all branches. Your change applies here only.", and the summary `catalogue` carries the flag

**Given** the Owner app rule list
**When** a tenant rule renders
**Then** it shows "Overridden at 2 branches" as a warning `status-badge`, expandable to each branch's differences, with "Re-apply to these branches"

**Given** "Re-apply"
**When** sent
**Then** a `rule.upsert` with the tenant values goes to those branches, the owner's change wins as in Story 8.19, `overridden` returns to 0, and the Hub toasts "Pricing rule updated by owner"

**Given** a tenant rule edited in the Owner app later
**When** pushed
**Then** branches with `overridden = 1` are excluded by default and the preview offers "Also overwrite 2 overridden branches"

### Story 14.34: Happy-hour automation across branches

As a café owner,
I want to set one happy hour that starts and ends by itself at every branch in its own local time and tells players when it begins,
So that quiet hours fill up without staff remembering to do anything.

**Covers:** FR162 (happy-hour automation), FR61, FR157, AD-5, AD-7 (branch local time), UX-DR50, UX-DR76, UX-DR90 (Manage → Pricing)

**Acceptance Criteria:**

**Given** Manage → Pricing → "New happy hour"
**When** the owner sets name, weekdays, start and end time, discount percentage, device groups (by name) and branches (one, several or All)
**Then** it is sent as a tenant rule (Story 14.33) with `kind = happy_hour` and `announce = 1`, resolved at each branch in that branch's timezone

**Given** a happy hour at a branch
**When** its start boundary passes (the resolver's `next_change_at`, Story 3.31)
**Then** running sessions in those groups open a new rate segment, the header `chip-rule` shows it, and the Hub applies a system `device.message{scope: all}` "Happy hour now — 20% off until 6 pm" to PCs in those groups with active sessions

**Given** its end boundary
**When** it passes
**Then** rates return without any message to players

**Given** a happy hour that overlaps an existing branch rule
**When** it is pushed
**Then** the branch keeps the Story 3.29 no-stacking resolution, and the preview shows "Conflicts with <rule> at <branch>" before sending

**Given** a branch offline when the happy hour was pushed
**When** the command arrives after the start time
**Then** the rule applies from that moment with the announcement only if more than 15 minutes of it remain

**Given** the kiosk attract screen (Story 6.7)
**When** a happy hour is active or starts within 60 minutes
**Then** the attract rates panel shows "Happy hour 4–6 pm, 20% off"

### Story 14.35: The cloud relays WebRTC signalling and issues TURN credentials for Premium branches

As a café owner on Premium,
I want my main Hub and an off-site PC to find each other through the cloud without opening any port,
So that an off-site standby can connect from anywhere.

**Covers:** FR172 (STUN/TURN path), AD-29 (`offsite_standby` flag), AD-22, AD-23, NFR11, NFR14, NFR20; Spine §Deferred "TURN credential lifetime", "Off-site standby (WebRTC) detail"

**Acceptance Criteria:**

**Given** the tier table in `arena-proto`
**When** this story ships
**Then** Premium licences gain the `offsite_standby` flag (no add-on sells it), and the heartbeat signs it from the Story 10.6 claims

**Given** two Hub devices of the same branch, both with valid tokens and the flag
**When** either calls `POST /v1/signal/{pair_id}` with an SDP offer, answer or ICE candidate
**Then** the control role stores it in a new short-lived `signal_messages` table (pair_id, from_hub, kind, body, created_at; rows deleted after 10 minutes), and the other Hub receives it from `GET /v1/signal/{pair_id}?since=` held open up to 25 s

**Given** a caller that is not one of the pair's two Hub devices, a branch without the flag, or a revoked token
**When** it calls the signalling endpoints
**Then** it is refused `FORBIDDEN`, `FEATURE_NOT_ENTITLED` or `TOKEN_REVOKED`

**Given** `POST /v1/turn/credentials`
**When** an entitled Hub calls it
**Then** the cloud requests a credential from Cloudflare Realtime TURN through an adapter (recorded fake in tests) and returns ICE servers (Cloudflare STUN and TURN over UDP, TCP and TLS 443) valid for 1 hour; nothing media-related runs on the VPS

**Given** per-tenant limits
**When** signalling exceeds 600 messages per hour per pair or TURN credentials exceed 30 per hour per branch
**Then** further calls return `RATE_LIMITED`

**Given** the cost guard
**When** TURN relay bytes per branch are reported by Cloudflare
**Then** the jobs role exports "TURN GB per branch per month" and alerts ops when a branch exceeds the recorded budget

### Story 14.36: Pair an off-site PC as the branch's off-site standby

As a café owner on Premium,
I want to add a PC at my home or another site as a standby for my branch,
So that a fire, theft or power failure at the café can't take my data and my Hub at once.

**Covers:** FR172 (pairing), FR177 (standby codes), AD-10, AD-11, AD-42, UX-DR24, UX-DR66, UX-DR102

**Acceptance Criteria:**

**Given** Account → Branches on a branch with the `offsite_standby` flag
**When** the owner chooses "Add off-site standby"
**Then** a single-use 8-character code is shown as in Story 9.18, and at most one LAN standby and one off-site standby can exist per branch (`STANDBY_EXISTS` otherwise)

**Given** the Windows Hub installer on a PC anywhere with internet
**When** the code is entered
**Then** `POST /v1/pair` registers it with `role: offsite_standby`, its own `hub_instance_id`, token and licence; it never runs `branch.bootstrap`

**Given** the new off-site standby
**When** it starts
**Then** it asks the cloud to introduce it to the branch's main Hub, and the main Hub shows an alert "Approve off-site standby '<PC name>'?" in System → Standby

**Given** the alert at the main Hub
**When** a Manager approves with their PIN in place
**Then** the main Hub mints a paired-device token with `kind = standby`, and the two Hubs exchange and pin each other's Hub certificate fingerprints through the signed signalling channel, so any later connection between them is authenticated by those fingerprints, never by the cloud

**Given** a Manager declines, or no approval within 24 hours
**When** that happens
**Then** the off-site PC shows "Not approved at the café" and the code must be reissued

**Given** kiosks and Pis paired to the main Hub
**When** the off-site standby is approved
**Then** each receives its fingerprint (as in Story 9.18) and the branch tunnel hostname, so they can reach it after a promotion

**Given** an Android Hub or a branch without the flag
**When** an off-site standby is attempted
**Then** it is refused `PLATFORM_UNSUPPORTED` or `TIER_LOCKED`, and Account → Branches shows `tier-lock` "Available on Premium"

### Story 14.37: Two Hubs open an authenticated WebRTC data channel and replicate over it

As a café owner on Premium,
I want the off-site PC to receive the branch's changes live over the internet,
So that it stays seconds behind the café without any open port.

**Covers:** FR172 (follows over STUN/TURN), AD-4 (follow), AD-8 (`SCHEMA_BEHIND`), AD-24 (keys), AD-1, NFR9, NFR11

**Acceptance Criteria:**

**Given** an approved off-site standby
**When** it connects
**Then** both Hubs fetch ICE servers (Story 14.35), negotiate one reliable, ordered WebRTC data channel through the signalling relay, verify the DTLS fingerprint against the pinned Hub certificate (a mismatch closes the channel and alerts both Hubs), and run the Story 9.19 replication protocol unchanged inside it (device token in the first frame, `from_seq`)

**Given** a standby that already holds a database at some `seq` (in tests, seeded from a LAN snapshot fixture)
**When** the channel is up
**Then** the main Hub streams closed batches then live rows from that `seq`, applied with `arena-replay` and written verbatim into `changes`, and `player_keys` changes arrive on the same channel, an erased key being deleted within one message

**Given** the channel drops (WAN loss at either end, ICE failure, TURN credential expiry)
**When** it happens
**Then** the standby reconnects with jittered backoff 2–30 s, fetching a fresh TURN credential before expiry, and resumes from its last applied `seq` with no gap and no duplicate; trading at the café is never affected

**Given** a direct path is not possible (symmetric NAT at both ends)
**When** ICE runs
**Then** the channel falls back to TURN relay over UDP, then TCP, then TLS on 443

**Given** the off-site standby's schema is below the main Hub's
**When** it connects
**Then** it is refused `SCHEMA_BEHIND` and shows "Update this PC to follow the main Hub"

**Given** the off-site standby
**When** any intent reaches it
**Then** it is rejected `NOT_PRIMARY` and nothing is written

### Story 14.38: An empty off-site standby bootstraps over the data channel in resumable chunks

As a café owner on Premium,
I want a brand-new off-site PC to copy the whole branch over the internet even on a shaky line,
So that it becomes a ready standby without anyone carrying a USB drive.

**Covers:** FR172 (bootstrap), AD-4 (standby bootstrap), AD-24, NFR9, UX-DR23

**Acceptance Criteria:**

**Given** an approved off-site standby with an empty database
**When** it first connects over the Story 14.37 channel
**Then** it requests a snapshot consistent at a stated `seq`, received in 1 MB chunks each with a SHA-256, then follows from that `seq` by the Story 14.37 rules

**Given** the channel drops mid-bootstrap
**When** it reconnects
**Then** it resumes from the last verified chunk of the same snapshot; if the main Hub no longer holds that snapshot (more than 24 hours old) it starts a fresh one

**Given** a chunk whose hash does not match
**When** it is received
**Then** it is requested again, and three failures in a row abandon the snapshot with "Copy failed — retrying"

**Given** the snapshot
**When** it is built and sent
**Then** player PII is ciphertext only and `player_keys` travel only inside the authenticated channel, never through the cloud

**Given** both standby screens (Story 9.20)
**When** they render
**Then** the off-site one is labelled "Off-site standby", shows "Copying branch — 42%" during bootstrap and then lag in rows and seconds, the path (Direct or Relayed) and last contact; the main Hub's System → Standby lists both standbys

**Given** the main Hub serving a bootstrap
**When** staff keep trading
**Then** `apply()` latency stays within NFR3 and the upload of chunks is throttled to leave the café's uplink usable (at most 50% of measured upstream)

### Story 14.39: Promote the off-site standby to main

As a café owner whose café Hub PC has died,
I want to make the off-site PC the main Hub with a PIN,
So that the branch has a working main Hub again within seconds.

**Covers:** FR172 (promotes with the same guarantees), FR169, FR170, FR171, AD-11, AD-38, AD-24, NFR8, UX-DR66, UX-DR102

**Acceptance Criteria:**

**Given** the off-site standby screen and no replication frame from the main Hub for 15 s
**When** a Manager or the Owner taps "Promote to main" and enters their PIN
**Then** the Story 9.21 steps run (schema check, epoch claim through the fence with `fork = {old epoch, last replicated seq}`, erasures re-applied, writer opened); because the off-site PC cannot see the café LAN, a provisional epoch is never used: promotion waits for the cloud claim and fails `CLOUD_REQUIRED` without it

**Given** the main Hub is still sending replication frames
**When** "Promote to main" is tapped
**Then** it fails `LEASE_FRESH`, and force promote (Story 9.22) is offered only after its data-loss warning

**Given** promotion
**When** it is measured from PIN confirmation to the first accepted intent at the off-site Hub (from its own UI)
**Then** it takes under 10 s on the reference PC

**Given** the old café Hub comes back
**When** it sees the higher epoch by heartbeat
**Then** it locks itself (Story 9.24) and offers "Make this the standby", which re-links it as the LAN standby of the off-site main; moving the main back to the café is a normal promotion of that standby later

**Given** every promotion of an off-site standby
**When** it completes
**Then** the audit records it with path and lag, and the owner receives an alert "Off-site standby is now the main Hub for <branch>"

### Story 14.40: The café's devices reach a promoted off-site main through the Tunnel

As counter staff at a café whose main Hub is now off-site,
I want the PCs, TVs and kitchen tablet to find the new main by themselves,
So that players keep playing and we keep selling without re-pairing anything.

**Covers:** FR172 (same guarantees for devices), FR169 (kiosks reconnect by themselves), AD-6 (Tunnel), AD-12, AD-42, NFR8, UX-DR98

**Acceptance Criteria:**

**Given** an off-site Hub that has just been promoted (Story 14.39)
**When** it opens the writer
**Then** it starts the branch's Cloudflare Tunnel connector with the branch credential fetched from the cloud, so the branch tunnel hostname now reaches it

**Given** café kiosks and Pis that lost the old main
**When** mDNS finds no Hub for 30 s
**Then** they try the branch tunnel hostname learned in Story 14.36, connect with their device token over TLS pinned to the off-site fingerprint, present the highest epoch they have seen, and reconcile sessions by the Story 6.36 rules

**Given** a Kitchen display
**When** it loses the old Hub
**Then** it tries its Hub address list, which now includes the tunnel hostname, and reconnects with its existing scoped token by the Story 7.10 rules

**Given** the café's WAN is up
**When** the off-site Hub is promoted
**Then** café kiosks are connected within 60 s of promotion

**Given** the café's WAN is down
**When** the off-site Hub is promoted
**Then** café kiosks keep counting down locally (AD-12), and the owner is told "The café's devices will reconnect when its internet returns"

**Given** a device reconnecting through the tunnel
**When** it is admitted
**Then** it is only the paired-device class (Story 8.41 rules), and a device revoked at any time is refused `TOKEN_REVOKED`

### Story 14.41: Off-site standby proven across NAT, relay and failover

As the Arena 360 team,
I want the off-site standby tested against real network conditions,
So that it meets the same guarantees as the LAN standby before Premium cafés rely on it.

**Covers:** FR172 (same guarantees as LAN), NFR8, NFR9, NFR10, NFR19; Spine §Operational Envelope "Branch performance"

**Acceptance Criteria:**

**Given** a CI network lab with two Hubs behind configurable NATs (full cone, port restricted, symmetric) and the Cloudflare TURN test account
**When** the replication suite runs for each combination, including TURN-only over TLS 443
**Then** bootstrap of a 1 GB database completes, a 5 intents/s load replicates with median lag under 2 s on direct paths and under 5 s relayed, and the follower's database equals the primary's by checksum

**Given** link faults injected (30% loss, 500 ms latency, 10-minute outage, TURN credential expiry mid-stream)
**When** the suite runs
**Then** replication resumes without gaps or duplicates and no intent at the primary is slowed beyond NFR3

**Given** a failover drill
**When** the primary is killed under load and the off-site standby is promoted
**Then** no row up to the last replicated `seq` is lost, café kiosks reconnect through the tunnel, and a returning old primary self-locks with zero intents accepted at the old epoch after the fence moved

**Given** a split brain attempt (primary alive but partitioned from the off-site standby, force promote at the off-site)
**When** the partition heals
**Then** exactly one Hub accepts intents, the old one's later rows are orphaned (Story 9.23), and the builder lists them

**Given** the results
**When** the suite finishes
**Then** a run report (NAT pair, path, lag percentiles, bootstrap time, failover time) is stored as a release artefact, and any target missed fails the release

### Story 14.42: A release registry with signed manifests and staged rollout by percentage

As the Arena 360 team,
I want every Hub, Kiosk and Pi build published once with a signed manifest and rolled out to a growing share of branches,
So that a bad build reaches few cafés and can be stopped at once.

**Covers:** FR176 (staged rollout by percentage from the control plane), AD-32, AD-8, NFR19, NFR25; Additional Requirements "Signing and distribution", "Rollout order"; LLD release train

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `releases` (artifact `hub_windows|kiosk|pi`, version, channel `stable`, manifest R2 key, ed25519 signature, min_hub_version, schema_version, notes_md, rollout_percent, state `draft|rolling|paused|complete|withdrawn`, published_at)

**Given** a release tag
**When** the CI release job runs after cloud release N is live
**Then** it uploads the Authenticode-signed Windows Hub and Kiosk installers and the Pi agent bundle to R2 behind the CDN, writes a Tauri-updater manifest per artifact (version, URL, SHA-256, ed25519 signature from the SOPS-held update key) and registers each as `draft`

**Given** the audited ops commands `arena-cloud ops release roll --artifact <a> --version <v> --percent <p> --reason <text>`, `pause` and `withdraw`
**When** an operator runs them
**Then** the percentage follows 1 → 10 → 50 → 100 (no skipping without `--force`), each step is recorded, and `pause` or `withdraw` stops new installs at the next check

**Given** a branch heartbeat (Hub) or update check
**When** the cloud decides whether this branch gets the release
**Then** the branch is in the cohort when `hash(branch_id, artifact) mod 100 < rollout_percent`, so a branch stays in the cohort as the percentage grows, and the response carries `update{artifact, version, manifest_url}` only for an artifact the Hub can run (`min_hub_version`, schema)

**Given** Hubs reporting update outcomes (`update.installed`, `update.failed` with stage and error)
**When** more than 2% of the cohort for a version reports `failed` within 24 hours
**Then** the jobs role sets the release `paused` and pages the on-call

**Given** a release in any state
**When** an operator views `arena-cloud ops release status`
**Then** it shows cohort size, installed, pending, failed and the fleet version spread per artifact

### Story 14.43: The Windows Hub checks daily and installs on restart, never during an open session

As a café owner,
I want my Hub to update itself when nobody is playing,
So that I get fixes without staff doing anything and without any session being cut.

**Covers:** FR176 (daily check, install on restart with rollback, never during an open session), AD-32, AD-8 (rollback = roll-forward hotfix), NFR19, UX-DR23, UX-DR102

**Acceptance Criteria:**

**Given** a primary Windows Hub in the cohort for a newer version
**When** its daily check runs (at a random minute in the first opening hour, and on a heartbeat `update` hint)
**Then** it downloads the installer in the background at low priority, verifies the ed25519 manifest signature and the Authenticode signature, and stages it; a failed verification deletes the file, reports `update.failed{stage: verify}` and never installs

**Given** a staged update
**When** System → Updates opens
**Then** it shows "Update ready: 1.8.2" with the release notes and "Restart to update" (Manager), and the header shows a muted "Update ready" label

**Given** "Restart to update", or the branch's closing time + 30 minutes after the daily rollup has published
**When** any session at the branch is active, paused or in grace
**Then** the install does not start, the button shows "Waiting for 3 sessions to end", and it is retried every 5 minutes; nothing ever ends a session to install

**Given** no open session
**When** the install starts
**Then** the Hub stops accepting new intents with "Updating — back in about a minute", writes a local pre-update copy with `VACUUM INTO` to `backups/pre-update-<version>.db` (two kept), restarts into the new build, runs forward-only migrations and resumes; kiosks count down locally meanwhile (AD-12)

**Given** the new build fails to reach `healthz` ready within 120 s before any migration committed
**When** the updater's watchdog sees it
**Then** it restores the previous build, starts it, and reports `update.failed{stage: start}`; once a migration has committed the previous build is never started (`SCHEMA_TOO_NEW`), the Hub stays on the new build and the fix is a roll-forward hotfix

**Given** a 24-hour branch with no closing time
**When** an update is staged
**Then** it installs only at the first moment with no open session after `day_start_min`, or by "Restart to update"

**Given** a Free branch on Android
**When** an update exists
**Then** this story does not apply (Story 4.16 covers Play)

### Story 14.44: The standby updates before the primary

As a café owner with a standby PC,
I want the standby to take an update first,
So that it can always follow the main Hub and still be promoted if an update goes wrong.

**Covers:** FR176, NFR19 (standby before primary), AD-8 (`SCHEMA_BEHIND`), AD-11, AD-32

**Acceptance Criteria:**

**Given** a branch with a LAN or off-site standby and a version in its cohort
**When** the update check runs
**Then** the cloud offers the version to the standby first, and offers it to the primary only after the standby has reported `update.installed` for that version or 48 hours have passed with the standby offline

**Given** a standby (which never has sessions of its own)
**When** it has staged the update
**Then** it installs at once, restarts, reconnects and resumes following from its last `seq`

**Given** the standby on the newer build and the primary not yet updated
**When** they replicate
**Then** following works (standby schema ≥ primary), and a promotion in this window is allowed

**Given** the standby's update failed
**When** the primary's check runs
**Then** the primary is not offered the update, System → Updates on both shows "Standby update failed — fix the standby first", and the failure is reported to the cloud

**Given** a primary that updated while its standby was offline (48-hour rule)
**When** the standby returns
**Then** it gets `SCHEMA_BEHIND`, updates itself from the cloud at once, and resumes following

### Story 14.45: The Hub caches signed kiosk updates and kiosks install them between sessions

As a café owner,
I want every PC to take the new kiosk version from my Hub when nobody is on it,
So that 60 PCs update without 60 downloads or a single interrupted player.

**Covers:** FR176 (kiosks from the Hub's signed cache, between sessions only), AD-32, AD-8 (Hub accepts kiosks from the previous two releases), NFR17, NFR19, UX-DR97

**Acceptance Criteria:**

**Given** a primary Hub (Windows or Android) whose heartbeat offers a kiosk release
**When** it downloads it
**Then** it verifies the ed25519 manifest signature and the Authenticode signature, keeps the current and previous kiosk installers in its cache, and serves the verified one only to paired kiosks over TLS on `GET /kiosk/update`

**Given** the kiosk service
**When** it checks every 6 hours and on `hello`
**Then** it learns the Hub's offered version, downloads it in the background, verifies both signatures again, and stages it

**Given** a staged kiosk update
**When** the PC has no active, paused or grace session and is on the attract screen
**Then** the service installs it, the Hub tile shows "Updating" with a `status-badge`, and the Hub refuses a session start on that PC with `DEVICE_UPDATING` ("This PC is updating — about 2 minutes") until it reconnects

**Given** a session starts while an update is downloading or staged but not yet installing
**When** it starts
**Then** the install waits until that session ends

**Given** the new kiosk build fails to reach the Hub within 3 minutes of install
**When** the service watchdog sees it
**Then** it reinstalls the previous version from its local copy and reports `kiosk.update_failed` to the Hub, which raises a Hub alert and reports it in the next heartbeat

**Given** a kiosk more than two releases behind the Hub
**When** it connects
**Then** it is admitted only to fetch the update, its tile shows "Update needed", and sessions cannot start on it until it updates

**Given** the Hub's cache
**When** it serves more than 10 kiosks at once
**Then** further kiosks wait in a queue so the Hub's LAN serving never slows floor updates beyond NFR3

### Story 14.46: Pi agents update from the Hub's signed cache between console sessions

As a café owner with console TVs,
I want each Pi to update itself from the Hub when its console is free,
So that TV features improve without anyone reflashing a card.

**Covers:** FR176 (Pis from the Hub's signed cache, between sessions only), AD-32 (Pi ed25519), AD-12, NFR18, NFR19

**Acceptance Criteria:**

**Given** a primary Hub offered a Pi agent release
**When** it downloads it
**Then** it verifies the ed25519 signature, caches current and previous, and serves the bundle to paired Pis on `GET /pi/update`

**Given** the Pi agent
**When** it checks every 6 hours and on connect
**Then** it downloads the bundle (agent binary and TV page) into the inactive of two slots on `/data`, verifies the signature again, and marks it pending

**Given** a pending update
**When** the linked console has no active, paused or grace session and the TV shows the idle screen
**Then** the agent switches the active slot and restarts; the TV shows "Updating…" for at most 60 s and returns to idle

**Given** the new slot fails to reach the Hub within 2 minutes or crashes three times
**When** the watchdog sees it
**Then** it switches back to the previous slot and reports `pi.update_failed` to the Hub

**Given** a power cut during the switch
**When** the Pi boots
**Then** it boots the last slot marked good, never a half-written one

**Given** the read-only root (Story 12.1)
**When** updates run
**Then** only `/data` slots are written; a root filesystem change still needs a new image

### Story 14.47: Upgrade safety is proven on every release

As the Arena 360 team,
I want every release to prove it upgrades cleanly and that a hotfix can roll forward over it,
So that no café is left on a broken build or loses a session to an update.

**Covers:** NFR19 (rollback tested each release, never during an open session), FR176, AD-8, AD-32; Additional Requirements "CI/CD"

**Acceptance Criteria:**

**Given** the release candidate
**When** the CI upgrade job runs
**Then** it installs release N−1 and N−2 on Windows runners with a populated branch database (a 30-day synthetic history), upgrades to N through the real updater path, and checks that migrations run, `healthz` is ready, the change log is gap-free across the upgrade and the 30-day Z-reports are unchanged

**Given** a deliberately broken build N+1 that fails at start before migrations
**When** it is installed over N
**Then** the watchdog restores N and reports `update.failed{stage: start}`

**Given** N with a committed migration and a hotfix N+1
**When** the hotfix installs over N
**Then** it opens the current schema and the branch keeps trading; a test asserts that starting N−1 against the migrated database fails `SCHEMA_TOO_NEW`

**Given** a test LAN with a Hub, a standby, 5 kiosks and a Pi with sessions running
**When** updates for all four artifacts are offered at once
**Then** no install starts while any session is open, the standby updates before the primary, and every device ends on the new version after its sessions end

**Given** kiosks of the previous two releases
**When** the new Hub runs
**Then** they connect and run sessions (compatibility suite)

**Given** any check failing
**When** the job ends
**Then** the release cannot be registered as `rolling` (Story 14.42)

### Story 14.48: Spike — the Hub core compiles to WebAssembly with an in-memory SQLite

As a developer on Arena 360,
I want to prove that the real Hub core and store run in a browser before we build the demo on them,
So that the demo uses the same `apply()` as cafés and we know its size and speed up front.

**Covers:** FR197 (same core, in-memory DB — feasibility), AD-41, AD-8

**Acceptance Criteria:**

**Given** the Rust workspace
**When** the spike build runs
**Then** `arena-core` and `arena-store` compile to `wasm32-unknown-unknown` with SQLite in memory and an injected browser `Clock`, using the same migrations and `apply()`, behind a cargo feature that the native builds never enable

**Given** platform code (network, files, printing, OS)
**When** it is reached from the wasm build
**Then** it is behind traits whose demo fakes return "Not available in the demo", and a CI check fails if `arena-core` gains a direct platform dependency

**Given** a CI parity test
**When** the same scripted intent sequence runs against the native Hub and the wasm core in headless Chromium
**Then** the resulting change logs and Z-report totals are identical

**Given** the spike's build
**When** it is measured
**Then** the compressed bundle size and the time to seed and apply 1,000 intents are recorded in the repository, and the story is done only if the bundle is under 8 MB compressed; otherwise the record names what is too large

### Story 14.49: The Hub UI runs in a browser tab on the wasm core

As a prospect evaluating Arena 360,
I want to try the real Hub screens without installing anything,
So that I can judge the product before I sign up.

**Covers:** FR197 (same web UI, in-memory DB), AD-15, AD-41, NFR26

**Acceptance Criteria:**

**Given** the Hub UI SvelteKit app
**When** built in demo mode
**Then** it loads the Story 14.48 wasm core in a Web Worker and replaces its LAN transport with a worker transport carrying the same intent and event frames; no screen code is forked, and a CI check fails if a component imports a demo-only module

**Given** the demo build
**When** it opens on a mid-range laptop over a 20 Mbps link
**Then** an interactive floor appears in under 4 s

**Given** an action that needs a platform service (print, drawer kick, backup to folder, pairing)
**When** it is used in the demo
**Then** it shows "Not available in the demo" in place and nothing else fails

**Given** browsers without WebAssembly or Web Workers
**When** the demo is opened
**Then** a page says "The demo needs a recent browser" with a link to the product video

**Given** the demo
**When** it runs
**Then** it makes no network request after load other than static assets, verified by a CI test that fails on any other request

### Story 14.50: The demo opens on a sample café with players coming and going

As a prospect,
I want the demo to feel like a real evening at a café,
So that I see sessions, orders and alerts happen without setting anything up.

**Covers:** FR197 (sample data), UX-DR61, UX-DR97, UX-DR103

**Acceptance Criteria:**

**Given** the demo core starting
**When** it seeds
**Then** it loads a fixed sample branch "Demo Gaming Café" (24 PCs in 3 groups, 4 consoles, plans, a happy-hour rule, 30 products, 40 players with invented names and numbers in the reserved test range, 2 staff) and signs in a demo Manager, with no real personal data

**Given** a simulator in the worker
**When** the demo runs
**Then** it starts and ends sessions on about half the PCs, places kiosk orders, raises a low-time warning and a "Call staff" alert every few minutes through ordinary intents, at a pace the visitor can change (Calm · Busy)

**Given** the visitor acts
**When** they start, extend, end or settle a session, sell at the POS or close a shift
**Then** it works exactly as in the real Hub, and the simulator never acts on a device the visitor has open in a sheet

**Given** "Reset demo"
**When** pressed
**Then** the database is rebuilt from the seed in under 1 s

**Given** the tab is closed or reloaded
**When** the visitor returns
**Then** the demo starts fresh; nothing is stored beyond the theme

### Story 14.51: The marketing Demo page with a guided tour

As a prospect,
I want a Demo page that shows me what to try and leads me to sign up,
So that ten minutes in the demo turn into a decision.

**Covers:** FR197 (marketing Demo page), FR196 (site navigation), NFR15, NFR27, UX-DR5, UX-DR11, UX-DR91, UX-DR113

**Acceptance Criteria:**

**Given** the Astro marketing site
**When** this story ships
**Then** a Demo page appears in the main navigation and the phone full-screen nav, and its hero says "Try the Hub — sample café, nothing to install" with a primary "Open the demo"

**Given** "Open the demo"
**When** pressed
**Then** the demo Hub opens full-window at `/demo/hub` with a persistent strip "Demo — nothing is saved · Reset · Get started", which never covers the floor, cart or settle bar

**Given** the guided tour
**When** the visitor chooses "Show me around"
**Then** five steps (start a session, sell a drink, see a time warning, settle with a split payment, close a shift) each highlight one control with a short line and Next/Skip; Esc ends the tour; focus moves to each step's text

**Given** a phone visitor
**When** they open the demo
**Then** the Hub phone layout is used, and the strip collapses to "Demo · Get started"

**Given** the funnel instrumentation (NFR27)
**When** visitors use the demo
**Then** demo opened, tour completed and "Get started from demo" are counted without cookies or personal data

**Given** an axe scan of the Demo page and the demo strip
**When** it runs in light and dark
**Then** it has zero serious or critical violations

### Story 14.52: Compare pages against ggLeap, Smartlaunch and iCafe

As a café owner using another café system,
I want an honest side-by-side comparison,
So that I can see what I'd gain and what moving would take.

**Covers:** FR198 (compare: offline, price, hardware, migration), FR196, NFR15, NFR16, UX-DR5, UX-DR11, UX-DR25, UX-DR91, UX-DR113

**Acceptance Criteria:**

**Given** the marketing site
**When** this story ships
**Then** it has `/compare` (an index) and `/compare/ggleap`, `/compare/smartlaunch`, `/compare/icafe`, linked from the footer and the Pricing FAQ

**Given** a compare page
**When** it renders
**Then** it has the same four sections in order — Works offline, Price, Hardware needed, Moving to Arena 360 — each as a table with an Arena 360 column and the competitor column, followed by a "What they do better" list

**Given** every competitor claim
**When** the page is built
**Then** it comes from a content file holding the claim, its public source URL and the date checked; the build fails if any claim lacks a source or was checked more than 180 days ago

**Given** prices
**When** shown
**Then** Arena 360 prices come from the generated pricing table (Story 10.1, GST-exclusive) and competitor prices are shown in their published currency with the date, never converted

**Given** "Moving to Arena 360"
**When** it renders
**Then** it describes what can be imported today — the Epic 5 CSV imports of products, players and plans, and the Epic 13 importers from another system (ggLeap and Smartlaunch players exports with field mapping and dry-run, migrated time balances and products) — and links Get started; on `/compare/ggleap` and `/compare/smartlaunch` it names that competitor's importer specifically; each importer line is shown only while its feature flag in the site content file is on, so the page promises nothing that is not shipped

**Given** the compare pages
**When** scanned with axe in both themes and at 320 px width
**Then** tables reflow to stacked rows on phone with headers kept, and there are zero serious or critical violations

### Story 14.53: A public status page for the cloud

As a café owner,
I want one page that tells me whether Arena 360's cloud is working,
So that I know whether a problem is mine or theirs — while my café keeps running either way.

**Covers:** FR198 (status page), NFR21, NFR25, UX-DR5, UX-DR23, UX-DR91, UX-DR113

**Acceptance Criteria:**

**Given** synthetic checks run every 5 minutes from outside the VPS (Grafana Cloud synthetic monitoring) against sign-in, the Owner API, the command relay, heartbeat, presign and the marketing site
**When** results are collected
**Then** a GitHub Actions job writes `status.json` (component, state `operational|degraded|outage`, last checked, 90-day daily uptime) to a public R2 object behind the CDN, so the page works when the VPS is down

**Given** `/status`
**When** it renders
**Then** it lists each component with a `status-badge` (text plus colour), the 90-day uptime per component as a row of daily cells with each day's state in its accessible name, and a fixed line "Your café keeps running when our cloud is down — sessions, sales and kiosks work offline."

**Given** `gz_control`
**When** the migration runs
**Then** it adds `status_incidents` (id, title, components, severity `degraded|outage`, state `open|resolved`, started_at, resolved_at, created_by) and `status_incident_updates` (incident_id, body, posted_at, posted_by)

**Given** the admin app
**When** a staff member with the `support` or `superadmin` role opens the new Status section in the navigation
**Then** they can open an incident (title, affected components, severity, first update), add updates and mark it resolved through audited `/v1/admin/status/*` endpoints, each writing one `admin_audit` row, and other roles do not see the section

**Given** an incident opened, updated or resolved
**When** it is saved
**Then** the status job merges open and recent incidents into `status.json`, it appears on `/status` within 5 minutes with its updates newest first, and resolved incidents stay listed for 90 days

**Given** `status.json` older than 15 minutes
**When** the page renders
**Then** it shows "Status checks are delayed" rather than claiming everything is fine

### Story 14.54: A public changelog built from release notes

As a café owner,
I want to read what changed in each Hub, Kiosk, Pi and app release,
So that I know what's new and what my staff will notice.

**Covers:** FR198 (changelog), AD-32, NFR16, UX-DR91, UX-DR113

**Acceptance Criteria:**

**Given** the release registry (Story 14.42)
**When** a release moves from `draft` to `rolling`
**Then** its `notes_md` (written in the staff register, no internal ticket ids) is published to `/changelog` at the next site build, grouped by month with artifact and version

**Given** a release still rolling
**When** shown
**Then** it is labelled "Rolling out — your café gets it automatically over the next week"; a `withdrawn` release is removed and replaced by its hotfix's entry

**Given** the Owner app and web releases (which ship immediately)
**When** they deploy
**Then** their notes appear under "Owner app" from the same pipeline

**Given** `/changelog`
**When** it renders
**Then** each entry has a heading per release, a filter by artifact (Hub · Kiosk · Console TV · Owner app) as a `tab-control` reflected in the URL, and an RSS feed at `/changelog.xml`

**Given** System → Updates on the Hub
**When** an update is ready (Story 14.43)
**Then** its "What's new" link opens the matching changelog entry
