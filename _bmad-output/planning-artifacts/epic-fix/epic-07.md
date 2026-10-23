## Epic 7: Kitchen tickets and display

Kitchen staff get KOTs and a LAN display that survives LAN loss, and order status reaches the seat.

> Notes: this epic builds on the Epic 1 LAN envelope and class scope table (Story 1.9: `paired`, `kds`, `phone`), idempotent intents and `base_rev` (Story 1.8), the Kitchen display SvelteKit `adapter-static` skeleton (Story 1.2), the entitlement API `has_flag()` (Story 2.12), per-device HC (Story 1.21), the printer adapter and paper width (Story 2.27), the fulfilment setting (Story 2.26), the pairing rate limiter (Story 2.19) and the alert service (Story 3.25). From Epic 5 it uses, by title: "Products with categories, barcodes, images and the kitchen flag", "Sell at the counter — a walk-up sale settles and takes stock", "Attach an order to a running session so it settles with the bill", "Take stock at served, mark items served, and cancel unserved lines into waste" (`order.line_served`, cancel with "Was it already made?") and "Choose print or none at settle, and print through the side-effect outbox" (the `side_effect_outbox`, Reprint, "Printer offline" alert). From Epic 6 it uses, by title: "Build a cart and add the order to the session bill from the seat" (kiosk `order.place`, new-order sound, ORDER badge), "Pay now from the wallet, Pay at counter, and unpaid order lines at the end of a prepaid session" (wallet-paid seat orders and `open_unpaid` counter orders) and "Order status reaches the HUD with delivery or pick-up copy" (the Hub "Mark ready" action applying `order.ready`, the delivered vs pick-up HUD copy, the cancelled-line toast). **Ticket model (decided here):** each placement that contains kitchen-flagged lines opens one kitchen ticket holding only those lines; the ticket's stage (New → Preparing → Ready → Served) is the single source for the Hub sheet, the KOT, the display and the READY badge; reaching Ready applies the Epic 6 `order.ready` effects for that ticket, and reaching Served applies `order.line_served` to its lines. **Kitchen display transport (decided):** the Hub's `:7401` listener serves both TLS (paired Tauri apps) and plain HTTP by path — `/kitchen` for the board (and `/console` for the phone page in Epic 12) — each plain-HTTP path accepting only its own scoped token; the spine amendment to AD-42 / Story 1.9 is logged separately. The board uses a scoped code-paired token (AD-42 class `kds`) whose scope is the board feed, `kitchen.advance` and `kitchen.advance.reverted` only — it can never cancel. **Seat only:** tickets on the board and on the KOT identify the order by seat label (PC/TV number or "Counter") and ticket number; no player name appears on either. **No cancel slip:** a counter cancellation prints nothing; the board greys the ticket out only. **Known limit:** plain HTTP rules out a service worker, so a tablet reloaded during LAN loss cannot load the board until the LAN returns; its IndexedDB queue survives and replays when the LAN is back (Story 7.10). **Out of this epic:** prep stations and item-level timing (FR102, Epic 14), owner-side kitchen reports (Epic 8), Bluetooth KOT on the Android Hub beyond the existing adapter interface (Epic 4 plugin already plugs in). **Paid tiers before Epic 10:** the `kitchen` flag (Basic and above) comes from the Story 2.8 operator-issued comp entitlement until billing exists, so every story here is tested end to end on a comp branch and on a Free branch. **Tier gate:** Story 7.1 builds the gate; each later story registers its own new kitchen intents with it and adds its own Free-refusal acceptance criterion. **Upgrade link:** the Owner app Subscription screen does not exist until Epic 10, so every `tier-lock` in this epic shows "Available on Basic" with no "Upgrade" link; the link is switched on centrally when Epic 10 ships (its `tier-lock` Upgrade story). Every story adds its new intents and their `.reverted` twins to the Story 1.8 `proptest` generator.

### Story 7.1: Kitchen settings at the Hub, locked on Free

As a café owner,
I want a Kitchen section in Hub settings that only opens on Basic and above,
So that I can set up KOTs and kitchen displays when my plan includes them and know clearly when it doesn't.

**Covers:** FR97 (tier-locked on Free), AD-29, AD-30, UX-DR24, UX-DR88

**Acceptance Criteria:**

**Given** a branch whose verified licence has the `kitchen` flag (Basic and above)
**When** a Manager or Owner opens Settings → Kitchen
**Then** it shows "KOT printer" (Receipt printer — default, Another printer, or None — display only), and an empty "Kitchen displays" list with "Pair a kitchen display" (enabled in Story 7.6)

**Given** "Another printer"
**When** the Manager picks it
**Then** the Story 2.27 "Find printer" flow runs for a second ESC/POS printer with its own paper width (58 or 80 mm) and "Test print", stored as the branch KOT printer; the receipt printer is unchanged

**Given** a Free branch (no `kitchen` flag)
**When** Settings → Kitchen opens
**Then** it shows `tier-lock` — lock glyph, "Available on Basic" and the line "Ask Arena 360 to change your plan." — and no editable fields; the "Upgrade" link is not shown until the Owner app Subscription screen exists (Epic 10)

**Given** the kitchen tier gate in `apply()`'s permission step
**When** an intent registered as kitchen-gated reaches a branch without the `kitchen` flag
**Then** it is rejected `TIER_LOCKED` and nothing is written; registration is one line per intent in `arena-proto`, and this story registers the Settings → Kitchen `settings.set` keys and is tested with those keys and a fixture intent registered only in the test build
**And** an intent marked "existing work" (acting on a kitchen ticket created while the flag was present) is let through after a lapse, so open tickets finish (AD-29)

**Given** a Counter or Kitchen role
**When** they open Settings → Kitchen
**Then** the fields are read-only and Save opens `inline-pin` for manager elevation

**Given** valid changes
**When** they are saved
**Then** `settings.set` stores them with `base_rev` (`STALE` on conflict) and the audit shows before/after

**Given** a branch that upgrades to Basic
**When** the next heartbeat delivers the new licence
**Then** Settings → Kitchen unlocks without a restart

**Given** a branch whose licence lapses to Free limits
**When** the Hub applies the new limits
**Then** no new kitchen tickets or KOTs are created from then on, open tickets keep working until served or cancelled, and nothing running is ended (AD-29)

### Story 7.2: Kitchen notes on order lines from the POS and the kiosk cart

As counter staff or a player,
I want to add "Less sugar" or "No onion" to a kitchen item,
So that the kitchen makes it the way the customer asked.

**Covers:** FR97 (modifiers), UX-DR69, UX-DR47, UX-DR103

**Acceptance Criteria:**

**Given** a kitchen-flagged product in Catalog
**When** a Manager edits it
**Then** an optional "Kitchen notes" list takes up to 8 preset notes of up to 40 characters each, saved with `product.upsert` and audited

**Given** a kitchen-flagged line in the Hub POS cart
**When** staff tap the line
**Then** the product's preset notes show as toggle chips plus one free-text note field (up to 60 characters), and the chosen notes show indented under the line in the cart

**Given** a kitchen-flagged line in the kiosk `cart` on a Basic+ branch
**When** the player opens the line
**Then** only the product's preset notes are offered as toggles (no free text on the kiosk), shown indented under the line

**Given** an order placed with notes
**When** `order.place` applies
**Then** the notes are stored on the `order_lines` row (new `kitchen_notes` column, JSON array of strings) and never change price, tax or stock

**Given** two cart lines of the same product with different notes
**When** the cart shows them
**Then** they stay separate lines; lines with identical notes merge with their quantities added

**Given** a free-text note over 60 characters or a preset over 40
**When** it is entered
**Then** input stops at the limit with a counter, and `apply()` rejects an over-long note `FIELD_TOO_LONG`

**Given** a Free branch or a non-kitchen product
**When** the cart shows the line
**Then** no notes control is offered

**Given** a Free branch
**When** an `order.place` carrying `kitchen_notes`, or a `product.upsert` setting a "Kitchen notes" list, reaches `apply()`
**Then** the kitchen-notes part is registered with the Story 7.1 gate and the intent is rejected `TIER_LOCKED`; the same order without notes is accepted

### Story 7.3: Every order with kitchen items opens a kitchen ticket that the Hub can move

As counter staff,
I want each kitchen order to become a ticket I can see and move from the tile sheet,
So that the counter and the kitchen work from one state from the start.

**Covers:** FR97 (seat/device, order id, time, items), FR99 (only the counter cancels, with reason), FR92, AD-2, AD-13, AD-39, UX-DR65, UX-DR97

**Acceptance Criteria:**

**Given** a Basic+ branch
**When** any `order.place` (Hub POS walk-up, session order, kiosk order) commits with at least one kitchen-flagged line
**Then** in the same transaction a `kitchen_tickets` row is created (new table: id ULID, `order_id`, `device_id` or null, seat label, ticket number, stage `new`, `placed_at`, `rev`) with `kitchen_ticket_lines` for only the kitchen-flagged lines and their notes
**And** the ticket number is `K-<n>` from a per-branch counter allocated in `apply()`, and a placement with no kitchen lines creates no ticket

**Given** the seat label
**When** the ticket is created
**Then** it is the device label for a session or kiosk order ("PC 07", "TV 3") and "Counter" for a walk-up POS sale

**Given** an in-session tile's sheet with open tickets
**When** the order section shows
**Then** each ticket is a row with its number, lines with notes, stage tag and elapsed time, and the Epic 6 "Mark ready" and Epic 5 "Mark served" actions now act on that ticket

**Given** a ticket in New or Preparing
**When** staff press "Mark ready" (or `kitchen.advance{ticket_id, to: ready, base_rev}` applies)
**Then** the stage moves to `ready` and the Epic 6 `order.ready` effects run for that ticket's order, so the HUD shows the delivered or pick-up copy within 5 s

**Given** a ticket in Ready
**When** it advances to Served (Hub "Mark served" or `kitchen.advance{to: served}`)
**Then** `order.line_served` applies to each of the ticket's lines in the same transaction (taking stock in "Served" mode); for a walk-up ticket, whose lines were served at settle, only the stage changes

**Given** `kitchen.advance` (permission `kitchen.advance`, granted to Kitchen, Counter, Manager and Owner and to the `kds` class)
**When** `to` is not exactly the next stage, or `base_rev` differs
**Then** it is rejected `BAD_STAGE` or `STALE` and nothing changes; `kitchen.advance.reverted` within its window returns the ticket to the previous stage and reverses the stage's effects (Ready: clears the ready state; Served: `order.line_served.reverted` restores the reservation)

**Given** staff cancel unserved lines of a ticket (Epic 5 cancel, reason required, "Was it already made?")
**When** the last open line of the ticket is cancelled
**Then** the ticket's stage becomes `cancelled` with the reason and staff in the audit; cancelling some lines removes only those lines from the ticket
**And** no intent in the `kds` scope can cancel (a `kds`-class `order.line_cancel` returns `FORBIDDEN`)

**Given** a Free branch (no `kitchen` flag)
**When** an `order.place` with kitchen-flagged lines commits
**Then** no kitchen ticket is created and the order behaves exactly as in Epics 5 and 6 (Epic 6 "Mark ready", Epic 5 "Mark served"); `kitchen.advance` and `kitchen.advance.reverted` are registered with the Story 7.1 gate as "existing work" intents, so on Free they return `TIER_LOCKED` except for a ticket created before a lapse

**Given** the `proptest` generator with `order.place`, `kitchen.advance`, the cancels and their reverts
**When** it runs in CI
**Then** a ticket's stage only moves one step at a time, Served implies every live line is served, and a cancelled ticket has no live lines

### Story 7.4: The floor tile shows READY and the Hub chimes when food is ready

As counter staff,
I want the tile to switch from ORDER to READY with a sound when the kitchen is done,
So that I deliver it or hand it over without asking the kitchen.

**Covers:** FR101 (tile ORDER/READY per fulfilment), FR115, UX-DR64, UX-DR107, UX-DR110

**Acceptance Criteria:**

**Given** a device with a kitchen ticket in Ready
**When** the floor renders
**Then** its `tile-order-badge` reads "READY" (replacing "ORDER") within 2 s of the stage change, and reverts to "ORDER" if another of its tickets is still New or Preparing after this one is served

**Given** a ticket reaching Ready
**When** it commits
**Then** the Hub plays the bundled order-ready sound once and raises an alert on that tile, acknowledged by opening its sheet (Story 3.25); a screen reader hears "PC 07 order ready" once

**Given** fulfilment is deliver-to-seat
**When** the sheet's order section shows a Ready ticket
**Then** it reads "Ready · deliver to PC 07"; for pick-up it reads "Ready · waiting for pick-up"

**Given** a walk-up "Counter" ticket reaching Ready
**When** it commits
**Then** no tile changes; the POS screen's open-tickets strip shows "K-2043 READY" with the same sound and alert

**Given** every ticket of a device is served or cancelled
**When** the last one closes
**Then** the order badge clears from the tile

**Given** a Ready advance that is undone
**When** `kitchen.advance.reverted` commits
**Then** the tile returns to "ORDER", the open alert is closed without sound, and the kiosk removes the ready message from `messages-bell` and the HUD "Ordered" tag returns, with no new toast

**Given** a Free branch
**When** an order is marked ready through the Epic 6 "Mark ready" action
**Then** no READY badge, open-tickets strip entry or order-ready chime is produced (they need a kitchen ticket, which Free never creates), and a test asserts the Free tile keeps the Epic 6 ORDER badge behaviour unchanged

### Story 7.5: A KOT prints within 3 s and can be reprinted from the Hub

As kitchen staff,
I want a printed ticket for every kitchen order within seconds,
So that I can start cooking even when I'm not looking at a screen.

**Covers:** FR97 (KOT ≤ 3 s, reprint), FR100 (printer offline alert, display still receives), NFR3, NFR18, AD-28, AD-34, UX-DR95, UX-DR101, UX-DR107

**Acceptance Criteria:**

**Given** the `kot` renderer (pure function from a kitchen ticket and settings to ESC/POS commands)
**When** it renders
**Then** it prints in Font A: "KOT" title, seat label double-width/height, ticket number, the order id (short form: last 6 characters of the order ULID, e.g. "Order 9MZD4Q", the same short id the Hub shows on the order), branch-local time, then each line as "qty × name" with each note indented below, a `-` rule, no prices and no player name; at 80 mm within 48 characters and at 58 mm within 32 with long names wrapped

**Given** a ticket created on a branch with a KOT printer (Story 7.1)
**When** its order commits
**Then** a `side_effect_outbox` row of kind `kot` is queued after commit and the printer task sends it, recording `kot.printed` or `kot.print_failed` with the error; the bytes reach the printer ≤ 3 s after commit on reference hardware

**Given** KOT printer set to None
**When** a ticket is created
**Then** nothing is queued and the ticket flows only to the Hub and kitchen displays

**Given** a KOT print failure (offline, paper out)
**When** it is recorded
**Then** the Hub raises the "Printer offline" alert with sound and header badge, the ticket and order are unaffected, the kitchen display still receives the ticket, and the ticket row shows "KOT not printed" with Reprint

**Given** any ticket in the tile sheet or the POS open-tickets strip
**When** staff press Reprint KOT
**Then** `kot.reprint` applies and the same KOT prints marked "REPRINT" with the reprint time, and the audit records who reprinted it

**Given** a ticket cancelled at the counter
**When** the cancellation commits
**Then** no KOT or cancel slip is printed

**Given** golden tests
**When** fixture tickets render at both widths (one line, many lines, long names, several notes, "Counter", "TV 3")
**Then** the byte output matches the stored golden files, every file carries the seat, ticket number and order id, and no line exceeds the width

**Given** a Free branch, or a branch after a lapse
**When** `kot.reprint` reaches `apply()`
**Then** it is registered with the Story 7.1 gate as an "existing work" intent, so it is rejected `TIER_LOCKED` except for a ticket created before the lapse, and no new `kot` outbox row is ever queued on Free

**Given** the Hub restarts with a queued KOT
**When** the printer task resumes
**Then** the KOT prints once or is marked failed for Reprint, never twice

### Story 7.6: Pair a kitchen tablet by code as a scoped display

As a café manager,
I want to open the kitchen display on any tablet's browser and pair it with a code from the Hub,
So that the kitchen gets a live board without installing anything or getting staff access.

**Covers:** FR98 (LAN tablet browser, paired by code), AD-42, AD-25, AD-23, AD-29, NFR11, NFR23, UX-DR24, UX-DR113

**Acceptance Criteria:**

**Given** Settings → Kitchen on a Basic+ branch
**When** a Manager or Owner presses "Pair a kitchen display"
**Then** the Hub shows the display address `http://<hub-lan-ip>:7401/kitchen` and a 6-digit single-use code with a 10-minute countdown

**Given** a tablet browser on the café LAN
**When** it opens that address
**Then** the Hub's `:7401` listener, which serves TLS to paired Tauri apps, serves the Kitchen display app on the `/kitchen` path over plain HTTP, showing a pairing screen with a 6-digit field and a display-name field (default "Kitchen")

**Given** a valid code
**When** it is entered
**Then** the Hub mints a scoped `kds` token (Argon2id hash in a new `kitchen_displays` table: id, name, created_by, last_seen, revoked), returns it once, the page stores it in browser storage, and connects by WS with `hello{class: kds}`; the display appears in the Hub list with name, connected state and last seen

**Given** a `kds`-class connection
**When** it calls anything outside the scope table (board feed, `kitchen.advance`, `kitchen.advance.reverted`)
**Then** the Hub returns `FORBIDDEN` and logs it; a CI test asserts the `kds` scope contains no staff, money, cancel or device intent

**Given** a plain-HTTP request to `:7401`
**When** it targets any path other than `/kitchen` (and its WS endpoint) or presents a non-`kds` token there, or a paired-device token is sent over plain HTTP
**Then** the Hub rejects it with `FORBIDDEN`; TLS clients on the same port are unaffected

**Given** a wrong or expired code
**When** it is entered
**Then** the field clears with "That code didn't work. Get a new one from the counter."; 5 failures invalidate the code and more than 20 attempts per IP per hour return `RATE_LIMITED` (Story 2.19 limiter)

**Given** 4 non-revoked displays
**When** a fifth pairing is attempted
**Then** it is refused with "This branch already has 4 kitchen displays — remove one at the counter."; kitchen displays never count toward the device cap

**Given** a paired display in the list
**When** a Manager presses Revoke
**Then** its socket closes with `TOKEN_REVOKED`, the page clears its token and returns to the pairing screen

**Given** a Free branch
**When** the address is opened
**Then** the page shows "Kitchen display is available on Basic. Ask the owner to upgrade." and the pairing intents (`kds.pair_code`, `kds.pair`) are registered with the Story 7.1 gate and return `TIER_LOCKED`

**Given** the tablet's stored token is missing or invalid
**When** the page loads
**Then** it shows the pairing screen

### Story 7.7: The kitchen board shows open tickets in four columns

As kitchen staff,
I want every open ticket on one board by stage, with the seat and how long it has waited,
So that I always know what to cook next and nothing gets forgotten.

**Covers:** FR98 (columns), FR99 (cancelled ticket greys and leaves), FR202 (HC, keyboard, non-colour cues), UX-DR77, UX-DR78, UX-DR89, UX-DR13, UX-DR4, UX-DR100, UX-DR110, UX-DR113

**Acceptance Criteria:**

**Given** a paired display connected to the Hub over plain HTTP/WS at `:7401/kitchen`
**When** the board renders in tablet landscape
**Then** it shows `kitchen-top-bar` (64px: branch name, clock, "<n> open · oldest <m> min", LAN status-badge "CONNECTED") over four columns New, Preparing, Ready, Served with counts in their headers, using kitchen tokens (gutter 24, column gap 24, ticket padding 16, ticket gap 16)

**Given** a ticket on the board
**When** it renders as a `kitchen-ticket`
**Then** it shows the seat label in `hub-numeric`, each line as "qty× name" with notes indented below in muted ink, the ticket number and tabular elapsed time since placement; no player name is shown; Ready adds a `state-free` edge and "READY" tag; Served uses muted ink

**Given** columns with more tickets than fit
**When** the board renders
**Then** tickets are newest-first and each column scrolls on its own; an empty column shows its header only

**Given** the open count
**When** it is computed
**Then** it counts New, Preparing and Ready tickets (never Cancelled or Served), and "oldest" is the oldest of those; elapsed times use Hub time from the board feed, not the tablet's clock

**Given** the Served column
**When** tickets are served
**Then** it keeps tickets served in the last 30 minutes, then they leave the board

**Given** the counter cancels a ticket (Story 7.3)
**When** the change reaches the board
**Then** the ticket drops to muted ink with struck-through lines and a "CANCELLED" status-badge in place of its stage tag, cannot be tapped, and leaves the board after 60 s; no cancel slip prints, and the board has no cancel control at any time

**Given** the display set to high contrast from its Hub list entry (per device, Story 1.21)
**When** the board renders
**Then** it uses the `-hc` tokens with 2px outlines and the yellow focus ring

**Given** a branch that lapses to Free while a display is paired
**When** the display requests the board feed
**Then** the feed still serves tickets created before the lapse until they are served or cancelled, and serves no new tickets (none are created on Free); a display on a branch that was never paid cannot hold a token (Story 7.6)

**Given** a screen reader or keyboard
**When** the board is used
**Then** it has one `h1`, each column is a labelled region, tickets are focusable with ≥ 44px targets, and new tickets and status-badge changes are announced in a polite live region

### Story 7.8: One tap advances a ticket, with Undo

As kitchen staff,
I want to move a ticket to the next stage with one tap and take it back if I tapped the wrong one,
So that the board keeps up with the kitchen without slowing me down.

**Covers:** FR98 (tap advances one stage with Undo), FR101 (HUD ≤ 5 s via Ready), AD-13, AD-14, AD-39, UX-DR21

**Acceptance Criteria:**

**Given** a ticket in New, Preparing or Ready
**When** staff tap it (or press Enter on it)
**Then** the display sends `kitchen.advance{ticket_id, to: next, base_rev, idempotency_key}` with a ULID minted at the tap, the ticket moves to the top of the next column at once, and `undo-toast` shows at the bottom ("PC 12 · #K-2041 moved to Preparing", "8 s", Undo)

**Given** the toast is showing
**When** staff press Undo within 8 s
**Then** `kitchen.advance.reverted` carrying the advance's resulting `rev` returns the ticket to its previous column with its previous effects reversed (Story 7.3)

**Given** a toast is showing
**When** staff advance another ticket
**Then** the new toast replaces it and the previous advance is final

**Given** a Served ticket
**When** it is tapped
**Then** nothing changes; Served is the last stage

**Given** the Hub or another display moved or cancelled the ticket first
**When** the advance returns `STALE` or `BAD_STAGE`
**Then** the ticket snaps to the Hub's state and the toast reads "Already moved at the counter" with no Undo

**Given** an advance to Ready
**When** it commits
**Then** the player's HUD shows the fulfilment-specific ready copy within 5 s and the tile shows READY (Story 7.4)

**Given** a branch that lapsed to Free
**When** a display advances a ticket created before the lapse
**Then** the advance applies as normal (Story 7.1 "existing work"); an advance naming any other ticket is rejected `TIER_LOCKED` and the board snaps to the Hub's state

**Given** an Undo after the window or after a later advance of the same ticket
**When** it reaches the Hub
**Then** it is rejected `EXPIRED` or `STALE` and the board keeps the Hub's state

### Story 7.9: New tickets chime and hold a NEW badge, with a mute toggle

As kitchen staff,
I want a chime and a badge I can't miss for each new order, and a way to silence the chime,
So that I notice orders during a rush and can still work quietly when I need to.

**Covers:** FR98 (chime, persistent NEW badge until advanced), UX-DR77 (mute toggle), UX-DR78, UX-DR107, UX-DR110, NFR26 (bundled sounds)

**Acceptance Criteria:**

**Given** a new ticket arrives on the board
**When** it is added to New
**Then** it shows at the top with the "NEW" `alert-badge` and a danger edge, the bundled chime plays once, and the arrival is announced in the live region

**Given** a ticket with the NEW badge
**When** it is advanced to Preparing
**Then** the badge and danger edge clear; nothing else clears it, and Undo back to New does not restore the badge or chime

**Given** the 44px mute toggle in `kitchen-top-bar`
**When** staff mute it
**Then** the chime stops for new tickets, the toggle shows a struck-through bell and "Chime off", and NEW badges still show; unmuting restores the chime for later tickets

**Given** the mute choice
**When** the page reloads
**Then** it is kept in browser storage for this tablet, and if storage is unavailable the board starts unmuted

**Given** a browser that blocks audio until a user gesture
**When** the board loads
**Then** it shows a one-time "Tap to turn on the chime" bar until the first tap, and badges work either way

**Given** several tickets arriving within one second
**When** they are added
**Then** the chime plays once for the burst and each ticket gets its own NEW badge

### Story 7.10: The board keeps working through LAN loss and syncs on reconnect

As kitchen staff,
I want to keep moving tickets when the tablet drops off the Wi-Fi,
So that a flaky connection never stops the kitchen.

**Covers:** FR99 (survives LAN loss, advances synced on reconnect), FR166, AD-14, AD-39, AD-11 (jittered backoff), NFR1, UX-DR98

**Acceptance Criteria:**

**Given** the display loses the Hub (no reply within 5 s or the socket closes)
**When** the board is open
**Then** the LAN status-badge shows "RECONNECTING" in warning, every ticket stays visible, elapsed times keep counting on the tablet's monotonic clock, and no per-ticket marker appears

**Given** the Hub is unreachable
**When** staff advance or Undo tickets
**Then** each advance moves the ticket at once and is queued in IndexedDB with its ULID `idempotency_key`, the predicted `base_rev` and a 24-hour TTL; an Undo of a not-yet-sent advance removes it from the queue

**Given** the page is reloaded during LAN loss
**When** the Hub is unreachable
**Then** the board cannot load (no service worker on plain HTTP); the IndexedDB queue is kept

**Given** the page is reloaded while the queue is not empty
**When** it loads again after the Hub is reachable
**Then** the stored queue replays before the board accepts new taps

**Given** the Hub returns
**When** the display reconnects with jittered backoff (2–30 s) and re-syncs
**Then** queued intents replay in creation order, apply once by idempotency key, the board is redrawn from the Hub's state, new tickets arrive with chime and NEW badges, and the badge returns to "CONNECTED" only after this reconcile

**Given** a queued advance that is now invalid (the counter moved or cancelled the ticket, `STALE`, `BAD_STAGE`, `EXPIRED`)
**When** it is replayed
**Then** it is dropped silently and the ticket shows the Hub's state

**Given** the Hub is unreachable
**When** orders are placed at the counter
**Then** they do not appear on the display until reconnect; the KOT still prints at the Hub

**Given** an integration test that cuts the display's LAN for 10 minutes while it advances 20 tickets and the counter cancels one of them
**When** the LAN returns
**Then** every valid advance is applied exactly once, the cancelled ticket shows Cancelled, and the board matches the Hub

### Story 7.11: The kitchen flow proven on reference hardware

As a café owner,
I want the order-to-kitchen-to-seat flow measured on real printers and tablets before release,
So that I can trust it on a busy night.

**Covers:** NFR3 (KOT ≤ 3 s), NFR1, FR97, FR98, FR100, FR101

**Acceptance Criteria:**

**Given** the reference Hub, a kiosk, a kitchen tablet browser and the listed USB and LAN ESC/POS printers in the hardware lab
**When** the scripted Journey 4 runs (kiosk order → KOT → board New → Preparing → Ready → HUD toast → Served)
**Then** the KOT prints ≤ 3 s after commit, the ticket appears on the board ≤ 2 s, the HUD ready copy shows ≤ 5 s after the Ready tap and the tile shows READY ≤ 2 s, each timing recorded in the release report

**Given** the printer unplugged during the run
**When** an order is placed
**Then** the Hub shows the "Printer offline" alert and the board still receives the ticket

**Given** 5 orders per second for 1 minute across 20 kiosks with 2 displays
**When** the burst runs
**Then** every KOT prints once, both boards match the Hub, and KOT latency stays ≤ 3 s at the 95th percentile

**Given** the 8-hour WAN-outage soak
**When** 500 orders flow through the kitchen
**Then** no ticket, advance or KOT is lost and nothing differs from a WAN-up run

**Given** the Kitchen display on current Chrome on Android and Safari on iPad
**When** the board, chime, mute, Undo and LAN-loss suite runs
**Then** it passes on both, with results published to the compatibility list

