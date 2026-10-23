## Epic 3: Counter staff run the floor

Staff run postpaid and prepaid sessions on PCs and consoles, manage players, wallet, credit, plans and dynamic pricing, and settle bills with mixed payments and Undo.

> Notes: this epic builds on the Epic 1 shell, `apply()`, inline PIN and audit, and on the Epic 2 devices, groups, zones, rates, opening hours and kiosk unlock/lock mechanism (Story 2.29). **Settle path:** Epic 3 settles session, top-up and plan-sale bills **without a receipt and without an invoice number**. A bill is an `orders` row with session/plan/top-up `order_lines` and `payments`; totals are tax-inclusive and no tax line is shown. Settle emits a `bill.settled` event, which is the hook Epic 5 uses to add receipts (print/WhatsApp/none), tax breakup, invoice numbering, cash drawer and shift binding. Undo checks one shared "irreversible effect recorded?" guard. In Epic 3 nothing records an effect, so Undo is always allowed inside the window; Epic 5 registers `receipt.printed`, `drawer.kicked` and "invoice number assigned" with that guard. Decided invoice timing (for Epic 5): with no printer the invoice number is assigned when the 8 s Undo window closes; with a printer it is assigned at print, so printed bills are never undoable. Cash is accepted with no open shift until Epic 5 adds `SHIFT_NOT_OPEN`. **Kiosk side:** on session start the kiosk unlocks to a minimal session screen (time + device name, shared tokens), and it locks with a reason line (Pay at counter, ended by staff, time's up, locked by staff). Epic 6 replaces that screen with the player shell, HUD, grace lock, launcher logout and kiosk self-start. **Out of this epic:** the tile sheet's Order action and the order-pending tile state (Epics 5–6), the reserved state's data (bookings, Epic 13), credit reminders and the console 5-min message (Epic 8), Pi/QR (Epic 12), and the kiosk halves of FR31, FR35, FR48 and FR63 (player self-start, kiosk Extend, kiosk account page, kiosk attract/plan-picker chip; Epic 6). Hiding erased players from the Story 3.7 lookup index and search is handed off to Epic 9: Story 9.1 adds `players.erased_at` and filters erased rows out. Every story adds its new intents to the Story 1.8 `proptest` generator.

### Story 3.1: Branch billing rules — rounding, minimum, grace and time-up mode

As a café owner,
I want to set how session time is rounded and billed and what happens when a player's time runs out,
So that every bill and every time-up at my branch follows one clear rule.

**Covers:** FR113, AD-7, AD-12 (`time_up_mode`), UX-DR88 (Settings → time-up mode), UX-DR18

**Acceptance Criteria:**

**Given** a Manager or Owner on Settings → Branch → Billing
**When** the screen opens
**Then** it shows rounding unit (default 15 min), minimum billing (default 30 min), grace period (default 2 min) and time-up mode ("Clean up" default, or "Staff handles"), each with its current value and a one-line explanation from the catalogue

**Given** valid values (rounding 1–60 min, minimum 0–240 min, grace 0–10 min)
**When** they save
**Then** `settings.set` stores them, the change appears in the audit, and `settings.changed` updates every open Hub view and connected kiosk without a reload

**Given** an out-of-range or non-numeric value
**When** they save
**Then** the field shows an inline error and nothing is written

**Given** a billing-rules change saved while sessions are running
**When** those sessions are billed
**Then** they use the rules in force when they started, and only sessions started after the save use the new rules

**Given** a Counter
**When** they change a billing rule
**Then** the action stays visible and opens `inline-pin` for manager elevation, and the change is audited with both actors

### Story 3.2: Floor view shows every device as a live tile grouped by zone

As counter staff,
I want the floor view to show every device's current state as a tile in its zone, updating by itself,
So that I can see the whole café at a glance and never act on stale information.

**Covers:** FR23, NFR3 (floor ≤ 2 s), NFR15, UX-DR2, UX-DR61, UX-DR63, UX-DR87, UX-DR97, UX-DR105 (arrow keys, Enter), UX-DR110, UX-DR116 (group-chip on tiles)

**Acceptance Criteria:**

**Given** the shared UI package
**When** the component gallery opens
**Then** `floor-tile` renders all eight states (free, in session, paused, reserved, order pending, maintenance, offline, unassigned) in dark and HC themes: 1px `border-strong`, radius md, ≥ 128px tall, a top row with device name and `status-badge`, a middle time slot in `hub-numeric`, a bottom player/secondary line, a 3px state bar, `group-chip` bottom-right, and an `alert-badge` slot top-right; unassigned shows a 2px dashed border with no bar and no group-chip
**And** every state carries a text label, never colour alone

**Given** a branch with zones, groups and devices from Epic 2
**When** Floor view opens
**Then** tiles render from the local database immediately with no skeleton, grouped by zone in sort order with 6 columns at 1920px, each zone header showing name and tile count, and the Unassigned section last
**And** a free tile's secondary line shows its group's active hourly rate ("₹100/hr · tap to start"), an offline tile shows "Last seen <time>", and a maintenance tile shows its note

**Given** a device's state changes at the Hub (socket connect/disconnect, maintenance, group assignment)
**When** the change commits
**Then** every open Floor view shows it within 2 s through a pushed event, with no polling

**Given** a tile has keyboard focus
**When** live updates arrive for it or other tiles
**Then** focus does not move and the focused tile is not reordered
**And** arrow keys move focus between tiles in grid order and Enter activates the focused tile

**Given** a Hub window narrower than 840px
**When** the floor renders
**Then** it shows 2 tile columns with a 16px gutter and 12px gap, and all targets stay ≥ 44px

**Given** the Floor view in dark and HC themes
**When** an axe scan runs
**Then** it has zero serious or critical violations, and each tile's accessible name includes device, state and time

### Story 3.3: Floor view performance proven on reference hardware

As a café owner with a large floor,
I want the floor view proven to stay fast with 60 devices on an old counter PC,
So that staff never wait on the screen during a rush.

**Covers:** NFR2, NFR3 (floor ≤ 2 s), NFR4 (60 devices without lag), AD-33

**Acceptance Criteria:**

**Given** the reference hardware runner (2015 i3 + HDD) with 60 seeded devices across 4 zones
**When** Floor view cold-opens
**Then** all 60 tiles are painted within 1 s of the view opening, with no skeleton

**Given** the same branch
**When** the benchmark changes 10 device states per second for 60 s
**Then** each change paints within 2 s of commit and no frame takes longer than 100 ms

**Given** the floor with 60 tiles
**When** the benchmark scrolls the floor and moves keyboard focus across all tiles
**Then** no input-to-paint delay exceeds 100 ms

**Given** the floor has been open with 60 live tiles for 30 minutes
**When** memory is measured
**Then** the Hub UI stays under 250 MB and the Hub service under 50 MB (NFR2 budgets)

**Given** any of these measurements misses its budget
**When** the per-release benchmark job runs
**Then** the job fails the release and reports the failing metric and its value

### Story 3.4: Collapsible zones with per-state counts

As counter staff in a large café,
I want to collapse zones I'm not watching while still seeing how many devices are in each state,
So that I can focus on one part of the floor without missing anything.

**Covers:** FR26 (collapse, counts), UX-DR87 (zone header collapse)

**Acceptance Criteria:**

**Given** a zone header
**When** staff activate its collapse control (click, tap, or Enter/Space with focus)
**Then** the zone shrinks to one row showing zone name, tile count, a count per state (e.g. "5 in session · 2 free · 1 offline")

**Given** a collapsed zone
**When** a device in it changes state
**Then** the counts update within 2 s without expanding the zone

**Given** zones collapsed on one Hub PC
**When** the Hub UI restarts on that PC
**Then** the same zones are collapsed; another Hub window on a different PC keeps its own collapse state, and nothing is written through `apply()`

**Given** a fresh Hub install
**When** Floor view first opens
**Then** every zone is expanded and nothing collapses automatically at any device count

**Given** a screen reader
**When** focus is on a zone header
**Then** the control announces the zone name and expanded/collapsed state

### Story 3.5: Non-modal tile sheet with remote PC actions

As counter staff,
I want to tap a tile to open a sheet beside the floor and lock, unlock, message, restart, shut down or admin-unlock that PC from it,
So that I can handle a device in two taps while the floor stays live.

**Covers:** FR24 (sheet shell, lock), FR39, FR118 (Hub remote unlock from the tile sheet), AD-25, AD-31, NFR3 (remote result ≤ 5 s), UX-DR65, UX-DR66, UX-DR97, UX-DR101 (Failed — retry), UX-DR104, UX-DR105 (Esc)

**Acceptance Criteria:**

**Given** Floor view
**When** staff tap a tile or press Enter on it
**Then** a non-modal `tile-sheet` docks right, the floor reflows to 4 columns at 1920px and stays live and operable behind it, focus moves into the sheet, and Esc closes it and returns focus to the tile

**Given** the sheet for a device
**When** it opens
**Then** it shows device name, group, zone, group rate and status, and only the actions valid for that state (UX-DR97): free/in-session PCs show Lock, Unlock, Message, Restart, Shut down and "Admin unlock (maintenance)"; maintenance shows "Return to service" and "Admin unlock (maintenance)"; unassigned shows "Assign to group" (Story 2.20 intents); offline shows last seen

**Given** a Counter on an online PC
**When** they choose Lock, Unlock or Message (with text up to 140 characters)
**Then** a `device.screen` intent (permission held by Counter) sends `pc.lock`, `pc.unlock` or `pc.message` to that kiosk; the kiosk covers the screen with "Locked by staff", removes that cover, or shows the message as a banner until dismissed, and confirms to the Hub

**Given** Restart or Shut down
**When** chosen by a role without `device.remote`
**Then** `inline-pin` opens in the sheet; with elevation the kiosk service restarts or shuts down Windows, and the action is audited with both actors

**Given** an online PC's sheet
**When** staff choose "Admin unlock (maintenance)" and a manager PIN is entered in the sheet's `inline-pin`
**Then** the Hub applies `device.admin_unlock` (Story 1.20), the kiosk drops to the Windows desktop for 15 minutes with its countdown badge and then re-locks, and the audit records the kiosk, the requesting staff member, the approving manager and mode = remote
**And** a wrong PIN, or a PIN of staff without the admin-exit permission, applies nothing and clears the cells; the Remote unlock entry on the floor placeholder and the Unassigned entry (Stories 1.20 and 2.18) now open this sheet action

**Given** any remote action
**When** the kiosk confirms within 5 s
**Then** the sheet shows the result inline; otherwise the tile's secondary line shows "Failed — retry", and a Retry action resends with the same idempotency key

**Given** the sheet's action layout
**When** it renders
**Then** each action is one tap from the open sheet (≤ 2 taps from the floor), Lock and other destructive actions are secondary buttons placed ≥ 24px from primary actions, and the whole flow works by keyboard alone

### Story 3.6: Create a player profile with protected personal data and consent

As counter staff,
I want to create a player's profile in under 15 seconds and record whether they agreed to service messages,
So that regulars get an account quickly and their personal data and consent are handled lawfully.

**Covers:** FR44, FR201, AD-24, AD-7, NFR6 (profile < 15 s), NFR11, NFR13, UX-DR18, UX-DR103 (consent text)

**Acceptance Criteria:**

**Given** the Players screen (Ctrl+3) or the "Create profile" action
**When** the form opens
**Then** it shows username, password (show/hide, paste allowed), phone (default +91), name, and optional email, DOB, notes and tags, with focus in the first field, plus a consent checkbox, unticked by default, with the catalogue text "Player agrees to the Terms and Privacy Policy and to receive service messages on WhatsApp/SMS."

**Given** valid input
**When** staff submit
**Then** `player.create` (permission `player.manage`) creates rows in the new `players` and `consents` tables (consents: purpose transactional, current policy version from the catalogue, granted 1 or 0, timestamp) in one intent
**And** name, phone and email are stored encrypted with a new per-player data key in `player_keys` (outside the change log, wrapped by a branch KEK held in the OS keystore), `phone_lookup` holds a keyed hash, and the change rows contain only ciphertext

**Given** a username or phone already used by a non-deleted player at the branch
**When** staff submit
**Then** it fails `USERNAME_TAKEN` or `PHONE_TAKEN` on that field and nothing is written

**Given** a password shorter than 6 characters or an invalid phone
**When** staff submit
**Then** the field shows an inline error, focus moves to it, and nothing is written; the password is stored only as an Argon2id hash

**Given** the Hub log and the audit viewer
**When** a profile is created
**Then** the log carries only the player id (no name, phone or email), and the audit shows the player id and consent purpose, version and state

**Given** a Playwright run on the reference hardware
**When** a profile with the required fields is typed and submitted
**Then** it completes in under 15 s from opening the form

### Story 3.7: Player lookup by phone, Guest, and the Players screen

As counter staff,
I want to find a player by typing their phone number, or tap Guest,
So that I can attach the right person to a session in one tap.

**Covers:** FR45, AD-24, UX-DR67, UX-DR88 (Players), UX-DR105 (Ctrl+3, Ctrl+K)

**Acceptance Criteria:**

**Given** the `player-lookup` component (on the Players screen and inside the tile sheet's start area)
**When** staff type digits
**Then** matching players appear directly below as they type (name + phone), Enter picks the top result, and a "Guest" secondary button sits beside the field

**Given** the Hub service starts
**When** it builds the lookup index
**Then** it decrypts phones and names into an in-memory index only (never written to disk in clear), adds each new player on create, and returns results for a 3,000-player branch within 100 ms server-side

**Given** no player matches
**When** the field holds a complete number
**Then** it shows "No player with that number." with **Create profile** (opens Story 3.6 with the phone prefilled) and **Guest**

**Given** a picked player
**When** the Players screen opens their profile
**Then** it shows name, username, phone, tags and consent state

**Given** focus is not in a text field
**When** staff press Ctrl+K
**Then** the lookup field on the current screen (or the Players screen if none) gets focus; Ctrl+3 opens Players

**Given** the lookup index and search built here
**When** player erasure is needed
**Then** it is handed off to Epic 9: Story 9.1 adds `players.erased_at` and filters erased players out of this index and search, so this story adds no erasure filter of its own

### Story 3.8: Edit a player profile and reset their password

As counter staff,
I want to update a player's details and set a new password when they forget it,
So that players never lose their account because they forgot a password.

**Covers:** FR46, FR44 (edit), FR201 (consent change), AD-24, AD-39, UX-DR18

**Acceptance Criteria:**

**Given** a player's profile
**When** staff edit phone, name, email, DOB, notes or tags and save
**Then** `player.update` re-encrypts changed personal fields with the player's key, carries `base_rev`, and is rejected `STALE` with current values shown if the profile changed meanwhile; uniqueness rules from Story 3.6 still apply
**And** the Story 3.7 lookup index is updated in the same commit, so a search by the new phone or name finds the player and the old phone no longer does

**Given** "Reset password"
**When** the player types a new password twice at the counter
**Then** `player.password_reset` stores a new Argon2id hash, the audit records staff, player id and time, and no password appears in any log or change row in clear

**Given** the two entries differ or the policy fails
**When** submitted
**Then** an inline error shows and nothing changes

**Given** the consent checkbox on the profile
**When** staff change it at the player's request
**Then** a new `consents` row is appended with the current policy version, state and timestamp; earlier rows are kept

**Given** a Counter without `player.manage`
**When** they try to edit
**Then** `inline-pin` opens in place

### Story 3.9: Start a postpaid session for a player or a guest

As counter staff,
I want to start an open postpaid session from a free tile in one tap, for a looked-up player or a guest,
So that a walk-in is on the clock within seconds.

**Covers:** FR27 (Hub start), FR30 (guest sessions), FR24 (start), NFR10 (one session per device), AD-2, AD-7, AD-14, UX-DR65, UX-DR97, UX-DR104

**Acceptance Criteria:**

**Given** the sheet of a free, online PC in a group with an active rate
**When** it opens
**Then** its start area shows `player-lookup`, Guest, the group's hourly rate and a primary "Start postpaid"

**Given** a picked player or Guest
**When** staff press "Start postpaid"
**Then** `session.start{mode: postpaid}` (permission `session.start`) creates a `sessions` row (new table: player or null + guest number, device, group, rate, billing-rules snapshot from Story 3.1, state, `business_day`, `fy`, `epoch`, `rev`) and an open `session_segments` row (new table) at the group's active rate, sets the device in session, and pushes `session.started`
**And** the tile shows "In session" with elapsed time and the player line within 2 s of commit

**Given** a guest session
**When** it shows on the tile and in the audit
**Then** the player line reads "Guest <n>", where n increments per business day

**Given** a device that is in session, in maintenance, offline, or in a group with no active rate
**When** a start is attempted
**Then** it is refused `PC_BUSY`, `PC_UNAVAILABLE` or `NO_RATE` with the plain reason inline, and nothing is written

**Given** two Hub windows start a session on the same PC at the same moment
**When** both intents reach `apply()`
**Then** exactly one succeeds and the other gets `PC_BUSY` (one active/paused/grace session per device, enforced in `apply()` and by a unique partial index)

**Given** a start retried after a timeout
**When** the same idempotency key arrives again
**Then** the original outcome is returned and no second session exists

**Given** a PC with an active or paused session
**When** Restart or Shut down (Story 3.5) is requested from its sheet
**Then** it is refused `PC_BUSY` with the inline reason "End or pause the session first", and nothing is sent to the kiosk

### Story 3.10: The PC unlocks to a session screen when a session starts

As a player at a café PC,
I want the PC to unlock within two seconds of the counter starting my session and show my time,
So that I can start playing straight away and always know my session is running.

**Covers:** FR27 (kiosk unlock ≤ 2 s), NFR3, NFR6 (start < 10 s), AD-12, AD-25, UX-DR8, UX-DR103

**Acceptance Criteria:**

**Given** a session starts on a PC
**When** the kiosk service receives `session.started`
**Then** the kiosk removes its locked cover and shows a minimal session screen with the device name and elapsed (postpaid) or remaining (prepaid) time in tabular figures, within 2 s of the tap at the Hub

**Given** a running session
**When** the Hub sends `session.tick` frames
**Then** each carries remaining or elapsed time, `lock_after_ms` (postpaid: until branch closing time) and `time_up_mode`, and the session screen shows the Hub's time while connected

**Given** the Hub sends a lock with a reason (Pay at counter, ended by staff, time's up, locked by staff, paused)
**When** the kiosk receives it
**Then** the kiosk covers the screen with that reason's catalogue line, and returns to its idle locked screen when the Hub clears it

**Given** the Hub becomes unreachable during a session
**When** `lock_after_ms` counts down on the kiosk's monotonic clock
**Then** the session screen keeps showing time with "Reconnecting…", the kiosk locks at zero, writes no state, and re-syncs to the Hub's record on reconnect

**Given** a Playwright run on reference hardware
**When** staff go from tapping a free tile to the kiosk reporting unlocked (Guest path)
**Then** it takes under 10 s

**Given** the kiosk does not confirm unlock within 5 s
**When** the timeout passes
**Then** the tile shows "Failed — retry" and Retry resends the unlock with the same idempotency key

### Story 3.11: End a postpaid session and see the bill to the minute

As counter staff,
I want to end a session in one tap and see an itemised bill that matches the clock,
So that there's never an argument about what the player owes.

**Covers:** FR28, FR24 (end), AD-7, UX-DR73, UX-DR97, UX-DR104

**Acceptance Criteria:**

**Given** an in-session tile's sheet
**When** it opens
**Then** it shows a live `bill-breakdown`: one line per rate segment as "<device> · <from>–<to> · <minutes> min × <rate>/hr" with its amount, then subtotal, "Paid so far" and "Remaining", all in tabular figures on `hub-tile`

**Given** staff press End on a postpaid session
**When** `session.end` applies
**Then** the session ends, the bill is written as an open `orders` row with one `order_lines` row per segment (new tables `orders`, `order_lines`), the device becomes free, and the kiosk locks with "Pay at counter" until the bill is settled or a new session starts on it
**And** the tile shows "Free" with an "UNPAID <amount>" secondary line until settled

**Given** the bill engine
**When** it computes a postpaid bill
**Then** minutes per segment = ceil((to − from − paused in segment) / 60 000); the branch minimum (from the session's snapshot) applies to the first segment and rounding up to the rounding unit applies to the total on the last segment; amounts are integer minor units (golden tests: 41 min at ₹100/hr with 15-min rounding and 30-min minimum = 45 min = ₹75.00; 10 min = 30 min minimum = ₹50.00)

**Given** End is pressed twice quickly or retried after a timeout
**When** the second intent arrives
**Then** the idempotency key returns the first outcome and no second bill exists

**Given** a session already ended
**When** another End arrives with an older `base_rev`
**Then** it is rejected `STALE` and nothing changes

### Story 3.12: Settle a bill into payment records with reference rules

As a café owner,
I want every settled bill recorded as payments that exactly match the total, with a reference on every UPI and card payment,
So that the money is traceable to the rupee, works offline, and is ready for receipts and invoices later.

**Covers:** FR70 (cash, UPI, card, split), FR71, NFR1, NFR10, AD-2, AD-7, AD-13, AD-34 (no provider)

**Acceptance Criteria:**

**Given** an unpaid bill (`orders` row from Story 3.11)
**When** `session.settle` (permission `pos.sell`) is submitted with one or more payment rows (cash, UPI, card)
**Then** a `payments` row per method (new table: order, method, amount, reference, staff, `business_day`, `fy`) is written and the order is marked settled in one transaction, and `bill.settled{order_id}` is emitted (the receipt hook)

**Given** payment rows whose sum differs from the bill total (cash change is recorded as cash = remainder)
**When** they reach `apply()`
**Then** the intent is rejected `PAYMENT_SHORT` and nothing is written

**Given** a UPI row without exactly 4 digits of the transaction reference, or a card row without a terminal reference
**When** it reaches `apply()`
**Then** the intent is rejected `REFERENCE_REQUIRED` naming the row

**Given** a settle
**When** it commits
**Then** no invoice number and no receipt is produced in this epic; the outcome carries `settled_at` and `undo_until` (settled_at + 8 s), so a later invoice step can assign the number when the Undo window closes, or at print when a printer is used

**Given** the settle commits
**When** the device's kiosk shows "Pay at counter"
**Then** it returns to its idle locked screen and the tile's UNPAID line clears

**Given** a settle retried with the same idempotency key, or a second settle on a settled order
**When** applied
**Then** the first outcome is returned, or the second is rejected `STALE`; no payment is duplicated

**Given** the WAN is down
**When** a bill is settled
**Then** it succeeds identically, with no network call

### Story 3.13: Settle bar and payment split at the counter

As counter staff,
I want to take payment in any mix of cash, UPI and card with a live remainder, all from the keyboard,
So that I can close a bill in seconds.

**Covers:** FR70 (live remainder, split), FR71 (UI), NFR6, UX-DR71, UX-DR72, UX-DR101, UX-DR104, UX-DR105 (Ctrl+Enter, Tab/arrows)

**Acceptance Criteria:**

**Given** an unpaid bill (open in the tile sheet of the device it was played on)
**When** the `settle-bar` shows
**Then** it shows the total in `hub-numeric` and methods cash · UPI · card, with no QR anywhere, and a primary Settle disabled with the inline reason "Remaining ₹x" while the remainder is not zero

**Given** `payment-split`
**When** staff add methods and amounts
**Then** each row has a label, amount field and reference field; the remainder recomputes live in `hub-numeric`, shows warning "Remaining" while non-zero and ok "Paid in full" at zero; cash may exceed the remainder and shows "Change ₹x"

**Given** a UPI row without 4 digits, or a card row without a reference
**When** staff try to save the row or settle
**Then** the row cannot be saved and the field shows the inline error ("Enter the last 4 digits of the UPI reference")

**Given** the remainder is zero
**When** staff press Settle or Ctrl+Enter
**Then** the Story 3.12 settle is submitted and the bar shows the settled state

**Given** keyboard-only use
**When** staff settle
**Then** they choose a method with Tab/arrows + Enter, type amounts and references, and settle with Ctrl+Enter without a mouse; Ctrl+Enter does nothing while the remainder is non-zero

**Given** a Playwright run on reference hardware
**When** staff settle a two-method split from opening the bar
**Then** it completes in under 10 s

### Story 3.14: Undo for End and Settle

As counter staff,
I want a few seconds to undo a mis-tapped End or Settle,
So that one wrong tap never costs the café or the player money.

**Covers:** FR74 (End, Settle), AD-13, AD-28 (guard), AD-39, UX-DR21, UX-DR104

**Acceptance Criteria:**

**Given** the shared UI package
**When** the `undo-toast` component renders
**Then** it is a `surface-2` bar with `border-strong` edge at the Hub bottom-centre (above the settle bar or bottom bar) showing what happened with the tabular amount ("Session ended · PC-12 · Aman · Bill ₹150.00"), seconds remaining, and an accent Undo; it never blocks the floor

**Given** End or Settle commits
**When** the toast shows
**Then** it counts down 8 s; Undo submits `session.end.reverted` or `session.settle.reverted` carrying the forward `rev`, from the same staff and Hub window

**Given** `session.end.reverted` applies
**When** the session was postpaid
**Then** the session returns to its prior state with its segments reopened, the open bill is deleted by compensating rows, the device is back in session, and the kiosk unlocks to the session screen again; time between End and Undo is billed

**Given** `session.settle.reverted` applies
**When** the bill was settled
**Then** the payments are reversed by compensating rows, the order returns to unpaid, and the tile's UNPAID line returns

**Given** a new money action (another End or Settle)
**When** it commits while a toast is showing
**Then** the new toast replaces the old one and the earlier action is final

**Given** an Undo arrives after the window, from another staff member or Hub window, or with a stale `rev`
**When** it reaches `apply()`
**Then** it is rejected `UNDO_NOT_ALLOWED` or `STALE` and nothing changes

**Given** the shared irreversible-effect guard
**When** a fixture records an irreversible effect against the forward intent — receipt printed, drawer kicked, or invoice number assigned
**Then** the toast is not shown (or closes) and Undo is rejected `UNDO_NOT_ALLOWED`; with no effect recorded, Undo is allowed

**Given** the invoice-number timing decided for later invoicing (number assigned when the 8 s Undo window closes if no printer is used, or immediately at print when a printer is used)
**When** this story's guard and toast are built
**Then** "invoice number assigned" is a registered irreversible effect kind, and a fixture test shows that a bill numbered at print shows no Undo while a bill numbered at window close could be undone only before that moment; no invoice numbering is implemented in this epic

**Given** the audit
**When** an action was undone
**Then** both the forward and the reverted intent are listed and linked

### Story 3.15: Pause and resume a session with a reason

As counter staff,
I want to pause a session with a reason and resume it later,
So that players aren't charged while they step away or while a PC is broken.

**Covers:** FR34, FR24 (pause/resume), FR28 (paused excluded), UX-DR61 (paused tile), UX-DR65, UX-DR97

**Acceptance Criteria:**

**Given** an in-session tile's sheet
**When** staff press Pause
**Then** a reason picker appears in place with Player stepped away · Hardware issue · Power cut · Other (free text required for Other), and choosing one applies `session.pause`

**Given** a paused session
**When** the floor renders
**Then** the tile shows the paused state token, a pause icon, a frozen timer and the reason as its secondary line, and the kiosk covers the screen with "Session paused — see the counter"

**Given** staff press Resume
**When** `session.resume` applies
**Then** the timer continues from where it stopped, `paused_total_ms` grows by the paused duration, and the kiosk unlocks within 2 s

**Given** a session paused and resumed, then ended
**When** the bill is computed
**Then** paused time is excluded per segment (golden test: 60 min with a 20-min pause bills 40 min before minimum and rounding)

**Given** pause on a paused session, or resume on an active one
**When** applied
**Then** it is rejected with a plain reason and nothing changes

### Story 3.16: Wallet top-up and paying from the wallet

As counter staff,
I want to top up a player's wallet by cash, UPI or card and let them pay bills from it,
So that regulars can prepay and every rupee in the wallet is traceable.

**Covers:** FR47, FR70 (wallet), NFR10 (wallet ≥ 0), AD-2, AD-7, UX-DR72

**Acceptance Criteria:**

**Given** a player profile
**When** staff choose "Top up" and enter an amount
**Then** a top-up bill opens in the same `settle-bar` with cash · UPI · card only (UPI last-4 and card reference rules from Story 3.12)

**Given** the top-up is settled
**When** `wallet.topup` applies
**Then** a `wallet_txns` row (new table: player, kind wallet, delta, reason topup, method, staff, ref to the order) is appended, `wallets.balance_minor` increases (the `wallets` table is created by this story's migration with a zero-balance row backfilled for every existing player, and `player.create` now also creates one), the payments are recorded, and `wallet.changed` updates every open view; the toast offers Undo as in Story 3.14 (`wallet.topup.reverted`)

**Given** a bill for a profiled player with a wallet balance
**When** staff add a wallet row in `payment-split`
**Then** the row shows the balance and accepts at most min(balance, remainder); settling appends a negative `wallet_txns` row in the same transaction

**Given** a wallet payment larger than the balance (e.g. a concurrent spend)
**When** it reaches `apply()`
**Then** it is rejected `INSUFFICIENT_BALANCE` and nothing is written; the balance never goes below zero

**Given** a player's profile on the Players screen
**When** it opens
**Then** it shows the wallet balance beside the Story 3.7 profile fields

**Given** the player profile
**When** staff open Wallet
**Then** it lists every wallet transaction newest first with time, amount, reason, method and staff, and the running balance equals the sum of deltas

**Given** a guest bill
**When** `payment-split` shows
**Then** no wallet option is offered

### Story 3.17: Convert a guest to a profile, including at settle, and list sessions

As counter staff,
I want to turn a guest into a profiled player at any time, even while settling,
So that a walk-in who becomes a regular keeps their history.

**Covers:** FR30, UX-DR72 (conversion at settle), UX-DR88 (Sessions screen)

**Acceptance Criteria:**

**Given** a running, ended or unpaid guest session
**When** staff choose "Convert to profile" in the sheet or in `payment-split`
**Then** the Story 3.6 form opens in place; on save, `player.create` plus `session.assign_player` attach the session (and its bill) to the new player in one intent, and the tile and bill show the player's name

**Given** an existing player
**When** staff choose "Link to existing player" through `player-lookup` instead
**Then** the guest session and bill are attached to that player

**Given** the Sessions screen on the rail
**When** it opens
**Then** it lists today's sessions (device, player or "Guest <n>", mode, start, end, bill, state: running / unpaid / settled) with filters for state, and selecting an unpaid row opens its settle bar
**And** converted sessions show the player's name, with the original guest number kept in the audit

**Given** a guest bill after conversion
**When** `payment-split` refreshes
**Then** the wallet option becomes available for the linked player, and the bill's payer-dependent options follow that player

### Story 3.18: Enable credit for a player and settle the remainder on credit

As a branch manager,
I want to give trusted players a credit limit and let staff put a bill's remainder on it,
So that regulars can play now and pay later without going over the limit I set.

**Covers:** FR49, FR50, FR70 (credit), NFR10 (credit ≤ limit), AD-30, UX-DR66, UX-DR72, UX-DR101 (credit block)

**Acceptance Criteria:**

**Given** a Manager on a player's profile
**When** they enable credit with a limit and a reminder cycle (default 3, 7, 14 days; editable list of days)
**Then** `credit.set_limit` (permission `credit.manage`) stores them in a `credit_balances` row (new table created by this story's migration: player, limit, outstanding, reminder cycle, `credit_paused`, `last_override_day`; a row is created on first enable); a limit below current outstanding is rejected `CREDIT_BELOW_OUTSTANDING`

**Given** a bill for a credit-enabled player
**When** staff add a credit row in `payment-split`
**Then** the row shows outstanding, limit and headroom, and settling appends a `wallet_txns` row of kind credit and increases `credit_balances.outstanding_minor` in the same transaction

**Given** outstanding + new > limit
**When** settle is attempted
**Then** `apply()` rejects `CREDIT_LIMIT`, the row shows "Credit limit reached, see counter." and offers `inline-pin` override

**Given** a manager PIN entered for the override
**When** no override was used for this player in the current business day
**Then** `credit.override` records `last_override_day` and the settle applies with `elevated_by`; a second override that day is rejected `OVERRIDE_USED_TODAY`

**Given** a guest or a player without credit
**When** `payment-split` shows
**Then** no credit option exists; a guest bill converted or linked to a credit-enabled player (Story 3.17) shows the credit option

**Given** concurrent credit spends from two Hub windows
**When** both reach `apply()`
**Then** the limit check runs inside each transaction and the second is rejected if it would exceed the limit

### Story 3.19: Collect credit with partial payments and Undo

As counter staff,
I want to collect any part of a player's outstanding credit by any method in one tap,
So that players can clear their tab whenever they come in.

**Covers:** FR54, FR74 (Collect), AD-13, UX-DR21, UX-DR72

**Acceptance Criteria:**

**Given** a player with outstanding credit
**When** staff choose "Collect" on the profile
**Then** a collection opens in `settle-bar` prefilled with the full outstanding, editable down to any positive amount, payable by cash, UPI (last-4), card (reference) or wallet

**Given** an amount ≤ outstanding
**When** staff settle it
**Then** `credit.collect` (permission `credit.collect`) appends a credit `wallet_txns` row with negative delta, records payments, lowers outstanding, and allocates the amount to the oldest unpaid spends first
**And** the screen shows a collection summary listing the settled spends it cleared (date, bill reference, amount) and outstanding after

**Given** an amount greater than outstanding
**When** submitted
**Then** it is rejected with an inline error and nothing is written

**Given** a collection commits
**When** the `undo-toast` shows
**Then** Undo (`credit.collect.reverted`) within 8 s restores outstanding and the allocation exactly, under the Story 3.14 rules

**Given** the collection summary
**When** Epic 5 adds receipts
**Then** the summary data (cleared spends, outstanding after) is exposed on the `bill.settled` event so it can be printed without recomputation

### Story 3.20: Credit aging shown first, and auto-pause at 15+ days overdue

As counter staff,
I want to see a player's overdue credit first when I look them up, and have credit stop for players far overdue,
So that I can collect before giving more credit.

**Covers:** FR51, FR53, UX-DR67 (outstanding first), UX-DR88

**Acceptance Criteria:**

**Given** a player with outstanding credit
**When** their row appears in `player-lookup` results
**Then** the outstanding amount shows first in warning text, before name and phone

**Given** the player profile
**When** it opens
**Then** an outstanding section comes first with the total and buckets 0–7, 8–14 and 15+ days, computed from the Story 3.19 oldest-first (FIFO) allocation of collections, plus the oldest unpaid date

**Given** a player whose oldest unpaid credit is 15+ days old and the branch rule "Auto-pause credit at 15+ days" is on (default on, in Settings → Billing)
**When** the daily system check runs at business-day start, or the player is looked up
**Then** a `credit.auto_pause` system intent sets `credit_paused`, the profile shows "Credit paused — 15+ days overdue", and new credit rows are refused `CREDIT_PAUSED` (the once-a-day override does not lift a pause)

**Given** a paused player
**When** a collection (Story 3.19) brings their oldest unpaid credit under 15 days or clears it
**Then** credit reopens automatically in the same transaction (`credit_paused = 0`)
**And** an Undo of that collection within its window restores the paused state exactly

**Given** the branch rule is off
**When** the check runs
**Then** no player is paused, and players paused earlier are reopened

### Story 3.21: Time packs, blocks and passes per device group

As a café owner,
I want to define packs, blocks and passes with prices and allowed groups,
So that I can sell prepaid play the way my café works, with console plans kept apart from PCs.

**Covers:** FR58, FR59, FR60, AD-7, AD-39, UX-DR18

**Acceptance Criteria:**

**Given** Settings → Plans
**When** a Manager creates a time pack
**Then** `plan.upsert` stores name, hours (as minutes), price, validity days and allowed groups in new `plans` and `plan_groups` tables (e.g. "5 h, ₹300, 7 days, Basic only")

**Given** a block
**When** it is created
**Then** it stores price and a start window (start and end minutes from local midnight), and the block's session will end at the window end regardless of start time

**Given** a pass
**When** it is created
**Then** it stores price, validity days and a play window that may wrap midnight (e.g. 22:00–06:00)

**Given** allowed groups that mix console and PC kinds
**When** the plan is saved
**Then** it is rejected `GROUP_KIND_MISMATCH`; a "combo" is created as two separate plans

**Given** zero/negative price or hours, a window with start = end, or no allowed group
**When** saved
**Then** a field error shows and nothing is written

**Given** a plan edit
**When** saved
**Then** it carries `base_rev` (`STALE` on conflict); packs and passes already sold keep the terms they were sold with; a deactivated plan stops being offered but owned copies stay usable

**Given** a Counter
**When** they edit plans
**Then** `inline-pin` opens for manager elevation

### Story 3.22: Sell a pack or pass at the counter and see owned plans on the profile

As counter staff,
I want to sell a pack or pass to a player and see what they own,
So that prepaid players can buy time and know what's left.

**Covers:** FR58 (sale), FR48 (Hub profile), FR33 (one active pass), UX-DR72

**Acceptance Criteria:**

**Given** a profiled player
**When** staff choose "Sell plan" and pick an active pack or pass
**Then** a bill with one plan line opens in `settle-bar` with all methods from Stories 3.13, 3.16 and 3.18

**Given** the bill is settled
**When** `plan.sell` applies with the settle
**Then** a `player_packs` row (new table: player, plan, minutes left for packs, expiry = settle time + validity days, bought at, order) is created, and Undo reverts both the sale and the owned plan

**Given** the player already holds an unexpired pass
**When** a second pass is sold
**Then** it is refused `PASS_ALREADY_ACTIVE`

**Given** the player profile
**When** it opens
**Then** "Packs & passes" lists each owned plan with hours remaining (packs), window (passes), expiry date and allowed groups; expired ones show greyed with an "Expired" tag and unused time ("0h 45m unused")

**Given** a guest
**When** staff try to sell a plan
**Then** the action asks to create or link a profile first

### Story 3.23: Session timer engine with time warnings

As counter staff,
I want the Hub to track every session's end time, warn ahead of it and end it on time,
So that timed play stops exactly when it should, even after a Hub restart.

**Covers:** FR31 (countdown), NFR3, NFR9, AD-12, UX-DR110

**Acceptance Criteria:**

**Given** sessions with an `ends_at` (prepaid modes) or a `closing_at` from opening hours (postpaid)
**When** the Hub service runs
**Then** a timer engine keeps a min-heap of end times, ticking every 1 s, rebuilt from active/paused/grace sessions at start

**Given** a session with an `ends_at`
**When** the engine reaches `ends_at`
**Then** it applies `session.end{reason: expired, actor: system}` exactly once and the kiosk locks within 2 s

**Given** a running session
**When** 10, 5 and 1 min remain to `ends_at` (or to `closing_at` for postpaid)
**Then** a warning is emitted to the kiosk (shown on the session screen) and the tile's time shows a warning border and ⚠ glyph from 10 min; warnings are not persisted and are not resent after a restart for marks already passed

**Given** a paused session
**When** it resumes
**Then** its heap entry is reinserted with `ends_at` moved by the paused duration

**Given** the Hub restarts with sessions past their `ends_at`
**When** the heap is rebuilt
**Then** overdue sessions are ended at once in `ends_at` order, and none ends twice

**Given** a test with an injected `Clock`
**When** 500 sessions with random end times run through the engine
**Then** each ends within 1 s of its `ends_at` and the engine never blocks `apply()`

### Story 3.24: Start a prepaid session on a pack or the wallet

As counter staff,
I want to start a session that counts down from the player's pack or wallet,
So that prepaid players play exactly the time they've paid for.

**Covers:** FR31 (staff start), FR32, NFR10, UX-DR48 (plan options, Hub side), UX-DR101

**Acceptance Criteria:**

**Given** a free PC's sheet with a profiled player picked
**When** the start area shows
**Then** it lists only options valid for this device's group: each owned unexpired pack allowed for the group (hours left, expiry) and Wallet (balance, hourly rate); expired packs show greyed and cannot be selected

**Given** a pack is chosen
**When** `session.start{mode: pack}` applies
**Then** the session gets `ends_at` = start + minutes left, the timer engine tracks it, and the kiosk unlocks counting down

**Given** Wallet is chosen
**When** `session.start{mode: wallet}` applies
**Then** `ends_at` is the moment the postpaid bill formula would reach the balance; a balance below the minimum bill is refused `INSUFFICIENT_BALANCE` with the typed reason "Balance ₹40. Top up at counter." naming the rate

**Given** a pack not allowed for the group, or expired
**When** start is attempted
**Then** it is refused `PLAN_NOT_FOR_GROUP` or `PACK_EXPIRED` with the reason inline

**Given** a pack or wallet session ends (by time-up or by staff)
**When** `session.end` applies
**Then** used minutes (rounded up to the whole minute, paused time excluded) are deducted from the pack, or the wallet is debited by the bill (never below zero), and the bill is auto-settled with no unpaid state

**Given** a concurrent wallet spend that leaves less than the session's reserved amount
**When** it reaches `apply()`
**Then** it is rejected `INSUFFICIENT_BALANCE`, so the wallet never goes below zero at session end

### Story 3.25: Hub alerts with sound and a persistent badge

As counter staff,
I want time-critical events to play a sound and leave a badge on the tile and header until I open them,
So that I never miss an alert because I was looking away or the room was loud.

**Covers:** FR26 (collapsed zones never hide an alert), UX-DR22, UX-DR60 (alert count), UX-DR87 (zone alert count), UX-DR107, UX-DR110, NFR15, NFR26 (bundled sounds)

**Acceptance Criteria:**

**Given** the Hub alert service
**When** an alert is raised for a device (any feature can raise a kind; a test fixture kind is used here)
**Then** the tile shows the `alert-badge` tag top-right, the header `alert-badge` count increments, and a bundled sound plays once and repeats every 30 s until the alert is acknowledged

**Given** an alert on a tile
**When** staff open that tile's sheet, or the alert row in the header list
**Then** the alert is acknowledged, the sound stops, and the badge clears; acknowledgement is written to the audit with staff and time

**Given** the header badge
**When** staff open it
**Then** a list shows each open alert with device, kind and age, and choosing one opens that tile's sheet

**Given** the Story 2.12 header `status-badge` slot (licence couldn't be verified, PC clock is wrong)
**When** this story's alert list ships
**Then** those two conditions are raised as branch-level alerts in the list (no device, no repeating sound) and the separate status-badge slot is removed; each alert clears when its condition clears

**Given** a collapsed zone (Story 3.4)
**When** a device in it raises or clears an alert
**Then** the zone row shows the zone's `alert-badge` count, updated within 2 s without expanding the zone, so a collapsed zone never hides an alert

**Given** the Hub UI is locked by idle lock or reloaded
**When** alerts are open
**Then** they persist (held by the Hub service) and reappear with their badges after unlock

**Given** a screen reader
**When** an alert is raised
**Then** it is announced once in a polite live region; sound is never the only cue

### Story 3.26: Time-up on PCs — grace, then clean up or staff handles

As a café owner,
I want a player's PC to get a short grace period at zero and then follow my branch's time-up mode,
So that nobody plays unpaid and nobody loses a game without warning.

**Covers:** FR31 (countdown to zero), FR113 (grace, time-up mode applied), AD-12, NFR3

**Acceptance Criteria:**

**Given** a prepaid session reaches `ends_at`
**When** the timer engine ticks
**Then** the session enters `grace` for the session's grace period (snapshot), the tile shows "Grace m:ss", and the kiosk shows "Time's up" with the grace countdown

**Given** grace ends in "Clean up" mode
**When** the timer engine ticks
**Then** `session.end{reason: expired, actor: system}` applies and the kiosk locks to its idle screen within 2 s

**Given** grace ends in "Staff handles" mode
**When** the timer engine ticks
**Then** the session becomes `expired` (no further time is charged), the kiosk holds a lock screen "Time's up — see the counter.", the tile raises a "TIME UP" alert (Story 3.25), and the session ends only when staff press End

**Given** the Hub is unreachable from the kiosk
**When** `lock_after_ms` (which already includes grace) reaches zero on the kiosk's monotonic clock
**Then** the kiosk performs the mode's local action, writes no state, and re-syncs to the Hub's record on reconnect

**Given** the Hub restarts while sessions are in grace or near time-up
**When** the timer heap is rebuilt
**Then** overdue sessions are processed at once in `ends_at` order, and none ends twice

### Story 3.27: Pass and block sessions with fixed windows

As counter staff,
I want to start a pass session that runs only in its window and a block that ends at its fixed time,
So that night passes and block deals work exactly as sold.

**Covers:** FR33, FR59, FR31 (pass, block), AD-7

**Acceptance Criteria:**

**Given** a player with an owned, unexpired pass allowed for this group
**When** staff start it inside the pass window
**Then** `session.start{mode: pass}` sets `ends_at` to the window end in branch local time (correct across a midnight wrap, e.g. 23:30 start on a 22:00–06:00 pass ends 06:00 next day)

**Given** the current time is outside the pass window
**When** start is attempted
**Then** it is refused `PASS_OUTSIDE_WINDOW` naming the window

**Given** a block plan allowed for the group whose start window contains now
**When** staff start it for a player or guest
**Then** the session ends at the block window's end regardless of start time, and a bill with the block price opens at once, payable now or at end through `settle-bar`

**Given** a block outside its start window
**When** start is attempted
**Then** it is refused with the window shown

**Given** a pass or block reaching its end
**When** the timer engine ticks
**Then** Story 3.26's grace and time-up mode apply

**Given** a DST-free IANA timezone and a branch day start
**When** golden tests run windows across midnight and business-day boundaries
**Then** start validity and `ends_at` match the expected local times

### Story 3.28: Extend a session from the Hub, charged to wallet, pack or credit

As counter staff,
I want to add time to a prepaid session from the tile and charge it to the player's wallet, a pack or credit,
So that players can keep playing without stopping their game.

**Covers:** FR35 (Hub extend), FR50, NFR10, UX-DR65, UX-DR101 (extend refusals)

**Acceptance Criteria:**

**Given** an active, paused or grace prepaid session
**When** staff press Extend in the sheet
**Then** options show +30 min, +1 h and +2 h with live prices at the resolved rate, and the payment sources available to the player: wallet (balance), each owned pack allowed for the group (hours left) and credit (headroom)

**Given** an option and source chosen
**When** `session.extend` applies
**Then** `ends_at` moves, a ledger row is written (wallet debit, pack minutes reserved, or credit spend with the Story 3.18 limit check and override), a grace session returns to active, and the kiosk shows the new time within 2 s

**Given** the new end would pass branch closing time (opening hours, Story 2.26)
**When** extend is attempted
**Then** it is refused `PAST_CLOSING` and the options stay open with the reason

**Given** insufficient wallet, pack minutes, or credit headroom (or paused credit)
**When** extend is attempted
**Then** it is refused with the typed reason and other options stay available

**Given** a postpaid session
**When** the sheet renders
**Then** Extend is not offered (the session is open)

### Story 3.29: Dynamic pricing rules by weekday hours and by date

As a branch manager,
I want to set happy hours and special-date prices per group that never stack,
So that pricing follows my promotions automatically.

**Covers:** FR61, FR62, AD-7 (`days_mask`), AD-39, UX-DR18

**Acceptance Criteria:**

**Given** Settings → Pricing rules
**When** a Manager creates a weekly rule
**Then** `rule.upsert` stores name, target group, weekdays (`days_mask`, bit 0 = Monday), hour range (may wrap midnight), and either an override rate or a percentage (negative = discount, in basis points) in a new `pricing_rules` table

**Given** a date rule
**When** it is created
**Then** it stores specific dates or an inclusive range in branch local time and outranks weekly rules

**Given** a new or edited rule that overlaps another active rule for the same group and time
**When** it is saved
**Then** the save succeeds with a visible "Conflicts with <rule>" flag listing the overlap and which rule wins (higher priority, then date over weekly, then latest edit)

**Given** the resolver `pricing::resolve(group, at_local)`
**When** golden tests run
**Then** exactly one rule applies at any instant (never stacking), wrap-around windows apply to the day they started, and the result includes `next_change_at`

**Given** invalid input (no days, start = end, percentage below −100%)
**When** saved
**Then** a field error shows and nothing is written

**Given** a Counter
**When** they edit rules
**Then** `inline-pin` opens for manager elevation

### Story 3.30: The active rule prices new sessions and shows in the header

As counter staff,
I want the current pricing rule shown in the header and used when I start or extend a session,
So that what I tell players matches what they're billed.

**Covers:** FR25, FR63 (Hub header, sheet), NFR3, UX-DR60, UX-DR76 (`chip-rule`)

**Acceptance Criteria:**

**Given** a rule is active for any group
**When** the `hub-header` renders
**Then** `chip-rule` shows the rule name, adjustment and end time ("⚡ Happy hour −20% · until 6:00 pm"); with several groups affected it names each; with none it is absent

**Given** a rule starts, ends or is edited
**When** the change happens
**Then** the chip, free-tile rates and sheet rates update within 1 min

**Given** a session start or extend
**When** the rate is resolved through `pricing::resolve`
**Then** the sheet shows the resolved rate and the active `chip-rule` before staff confirm, and the new segment records the `rule_id`

**Given** a wallet session start during a rule
**When** `ends_at` is computed
**Then** it uses the resolved rate, and the refusal text names that rate

**Given** the WAN is down
**When** a rule boundary passes
**Then** the chip and rates still change on time from the local clock

### Story 3.31: A rule change mid-session opens a new rate segment

As a café owner,
I want a happy hour that starts mid-session to change the rate from that minute,
So that players get exactly the price on the board at every moment.

**Covers:** FR63 (mid-session segment), FR28 (segment per rule flip), NFR10, AD-12, UX-DR73

**Acceptance Criteria:**

**Given** a running session when a rule begins or ends for its group
**When** `next_change_at` is reached
**Then** the session engine closes the current segment and opens a new one at the new rate from that minute, recording the `rule_id`

**Given** a session that crossed a rule boundary
**When** its bill is shown or computed
**Then** `bill-breakdown` shows one line per segment ("11 min × ₹80/hr (Happy hour)"), and golden tests match the expected totals with minimum on the first segment and rounding on the last

**Given** a wallet session when the rate changes
**When** the segment flips
**Then** `ends_at` is recomputed so the wallet still never goes below zero, and the kiosk receives the new `lock_after_ms` within 2 s

**Given** a paused session when a boundary passes
**When** it resumes
**Then** the segment split still happens at the boundary and paused time is excluded from both segments

**Given** the Hub restarts
**When** the timer engine is rebuilt
**Then** pending flips are rescheduled from `next_change_at`, and a boundary missed while down is applied at its true time

### Story 3.32: Transfer a session to another device

As counter staff,
I want to move a session to another free PC in two taps,
So that a player with a broken PC keeps playing without losing time.

**Covers:** FR36, FR24 (transfer), FR28 (segment per transfer), NFR3, UX-DR65

**Acceptance Criteria:**

**Given** an in-session or paused tile's sheet
**When** staff press Transfer
**Then** free, online devices of the same kind are highlighted on the floor, and picking one (tap or arrows + Enter) applies `session.transfer`

**Given** the transfer applies
**When** it commits
**Then** the old device locks and the new one unlocks within 5 s, the current segment closes, a new segment opens on the new device at its group's resolved rate, and both tiles update

**Given** a target in another group of the same kind
**When** transferred
**Then** the rate changes from that moment; a prepaid pack not allowed for the new group is refused `PLAN_NOT_FOR_GROUP`

**Given** a target that is busy, offline, in maintenance, or a different kind (PC ↔ console)
**When** picked
**Then** it is refused (`PC_BUSY`, `PC_UNAVAILABLE`, `GROUP_KIND_MISMATCH`) with the reason inline and nothing changes

**Given** the new kiosk does not confirm unlock within 5 s
**When** the timeout passes
**Then** the new tile shows "Failed — retry" and the session stays on the new device record so a retry resends the unlock

### Story 3.33: Group sessions under one bill and one payer

As counter staff,
I want several friends on several PCs billed together to one payer,
So that a group pays once and I don't juggle separate bills.

**Covers:** FR37, UX-DR73 (group split), UX-DR72

**Acceptance Criteria:**

**Given** a postpaid session on a tile
**When** staff choose "Add to group" and pick other running postpaid sessions (or start new ones from free tiles with "Start in group")
**Then** a `group_bills` row (new table: payer, state open) links them, and each tile shows a group marker naming the payer

**Given** a group bill
**When** any member's sheet opens
**Then** `bill-breakdown` shows each device's segments and minutes with a subtotal per device, and one total

**Given** staff change the payer to another member or a looked-up player before settlement
**When** saved
**Then** the payer changes and the settle options (wallet, credit) follow the new payer

**Given** members end at different times
**When** each ends
**Then** their devices lock and free individually, and the group bill stays unpaid until the last member ends and staff settle it once

**Given** a prepaid session
**When** staff try to add it to a group
**Then** it is refused with the reason "Only postpaid sessions can share a bill"

**Given** Undo of a group settle
**When** applied in the window
**Then** the whole group bill returns to unpaid

### Story 3.34: Forced end with manager PIN and reason

As a branch manager,
I want ending a player's paid time early to need my PIN and a reason, and to tell the player why,
So that early ends are rare, accountable and never a surprise.

**Covers:** FR38, AD-30, UX-DR66, UX-DR101 (forced end)

**Acceptance Criteria:**

**Given** a prepaid session with time remaining (or any session where staff choose "Force end")
**When** staff press End
**Then** `inline-pin` and a reason picker (Rule violation · Hardware issue · Closing · Other with text) open in the sheet

**Given** a correct manager PIN and a reason
**When** `session.end{forced}` applies
**Then** it records reason and `elevated_by`, deducts only the time actually used from the pack or wallet, and the kiosk shows "Your session was ended by staff. Please see the counter."

**Given** no reason, or a PIN without the force-end permission
**When** submitted
**Then** it is rejected `REASON_REQUIRED` or `PIN_REQUIRED` and the session continues

**Given** the audit
**When** a forced end is viewed
**Then** it shows the staff member, the approving manager, the reason and the minutes left at the time

**Given** a forced end
**When** the Undo toast shows
**Then** Undo needs the same elevation (AD-13), and with it restores the session and its remaining time

### Story 3.35: Console sessions timed from the Hub, with warnings on the tile

As counter staff,
I want to start, time and end PS5 and Xbox sessions from their tiles with the same plans as PCs,
So that consoles earn properly even without any hardware on the TV.

**Covers:** FR140, FR60, AD-12, UX-DR62, UX-DR97

**Acceptance Criteria:**

**Given** a console device (Story 2.21) in a console group
**When** its tile renders
**Then** it uses `floor-tile-console`: controller glyph, "TV n" on the secondary line, and the same states as a PC tile

**Given** the console's sheet
**When** staff start a session
**Then** postpaid, pack (console packs only), wallet, pass and block start exactly as for PCs, the timer runs on the Hub, and no kiosk unlock is sent

**Given** a running console session
**When** remaining time reaches 10, 5 and 1 min
**Then** the tile time shows the warning border and ⚠ glyph with "10 min left", "5 min left", "1 min left"

**Given** a PC plan or a PC pack
**When** offered for a console
**Then** it never appears; attempting it via the API is refused `PLAN_NOT_FOR_GROUP`

**Given** the console sheet
**When** it renders
**Then** remote PC actions (lock, message, restart, shut down) are absent

### Story 3.36: Console time-up alert and controller check

As counter staff,
I want a console that runs out of time to alert me until I deal with it, and to record whether the controllers came back,
So that nobody overruns unnoticed and missing controllers are traced to a session.

**Covers:** FR141, FR146, AD-13, UX-DR62, UX-DR107

**Acceptance Criteria:**

**Given** a prepaid console session reaching `ends_at`
**When** the timer engine ticks
**Then** there is no grace: the session becomes `expired` (no further charge), the tile shows "Time's up" with a "TIME UP" `alert-badge`, and the Story 3.25 sound plays until acknowledged

**Given** staff walk over and press End
**When** `session.end` applies
**Then** the audit records who ended it and when, and Undo works until the window closes

**Given** any console session has ended
**When** the tile renders
**Then** it shows a "CHECK CONTROLLERS" warning `status-badge` and the sheet offers **Returned** and **Missing**, and Start stays disabled with the reason "Mark controllers first"

**Given** staff tap Returned or Missing
**When** `controller.check` applies
**Then** the result is logged against the session with staff and time, the badge clears, and Missing also adds an entry to the header alert list for the manager

**Given** the controller check
**When** the Sessions screen lists that session
**Then** it shows Returned or Missing

### Story 3.37: Floor invariants, throughput and offline proof

As a café owner,
I want the money and session rules proven under load, under random sequences and with the internet down,
So that a busy Saturday or an outage can never corrupt a wallet, a credit balance or a session.

**Covers:** NFR1, NFR4, NFR6, NFR10, AD-2, AD-14

**Acceptance Criteria:**

**Given** the `proptest` suite with every Epic 3 intent and its `.reverted` twin
**When** it runs in CI on every push
**Then** after every step: wallet balances ≥ 0 and equal the sum of wallet ledger deltas; credit outstanding ≥ 0 and ≤ limit except where a same-day override is recorded; at most one active/paused/grace session per device; settled orders' payments equal their totals; every UPI payment has a 4-digit reference; undo pairs net to the prior state; `changes.seq` is gap-free and replay reproduces the rows

**Given** the reference hardware runner with 60 devices
**When** a load test drives a realistic mix of start, pause, extend, transfer, end and settle intents at 5/s for 10 min, with 50/s bursts of 10 s
**Then** server-side apply stays under 100 ms at p95, floor updates arrive within 2 s, and no intent is lost or duplicated; the job fails the release otherwise

**Given** a Playwright run on reference hardware
**When** it times session start (tap to kiosk unlocked) and settle (tap Settle to toast)
**Then** each is under 10 s, and profile creation is under 15 s

**Given** all WAN egress blocked
**When** the Epic 3 flows run (profiles, lookup, every session mode, pause, extend, transfer, group, forced end, console time-up, top-up, credit, settle, undo, rules, plans)
**Then** results match a WAN-up run and no outbound connection other than the connectivity probe is attempted

**Given** the Hub process killed during a burst
**When** it restarts
**Then** every committed session, payment and ledger row is present, the timer heap is rebuilt, and kiosks re-sync to the Hub's record

