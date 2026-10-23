## Epic 13: Bookings, tournaments, loyalty and game accounts

Cafés grow revenue with reservations, tournaments, loyalty and referrals, memberships, a launcher-account vault, game requests and migration from competitors.

> Notes: this epic builds on the Epic 1 shell, `apply()`, idempotent intents and the `proptest` generator (Stories 1.7–1.8), inline PIN (Story 1.13) and the audit viewer (Story 1.15); on Epic 2 devices, groups, zones and opening hours (Stories 2.20, 2.23, 2.26) and the wizard's Hub-progress steps (Story 2.28); on the Epic 3 session engine: tile sheet (Story 3.5), player profiles and lookup (Stories 3.6–3.7), postpaid start (Story 3.9), settle and `settle-bar` (Stories 3.12–3.13), Undo (Story 3.14), wallet (Story 3.16), plans and plan sales (Stories 3.21–3.22), the timer engine (Story 3.23), alerts with sound and badge (Story 3.25), time-up (Story 3.26), pass sessions (Story 3.27), transfer (Story 3.32) and forced end (Story 3.34); on Epic 5 tax per rate (Story 5.2), invoice numbering (Story 5.14), notification settings (Story 5.15), print layouts and the side-effect outbox (Stories 5.16–5.17), refunds with credit notes (Story 5.24), the Z-report (Story 5.28), Hub reports and exports (Stories 5.27, 5.32) and the CSV import engine (Stories 5.33–5.34); on Epic 6 the Hub catalogue cache (Story 6.2), the branch library (Story 6.3), the plan picker (Story 6.9), kiosk sign-up (Story 6.10), session-end cleanup and launcher sign-out (Stories 6.11–6.12), the Games tab (Story 6.23), launcher cards (Story 6.26), the kiosk Account page (Story 6.34) and the Hub-unreachable rules (Story 6.36); on Epic 8 rollups and schema bumps (Stories 8.5–8.7), owner reports (Story 8.15), the command catalogue and Hub apply (Stories 8.18–8.19), the command preview (Story 8.21), Manage → Pricing (Story 8.23) and the messaging adapter and outbox drain (Stories 8.28–8.29); and on Epic 9 erasure, key backup, export, standby replication and promotion (Stories 9.1, 9.2, 9.4, 9.19, 9.21). **Where screens live (decided for this draft):** the spine-only Sessions screen gains tabs Sessions · Bookings · Tournaments, so `hub-rail` keeps its nine items; café accounts and game requests are tabs of Settings → Games; importers sit in Settings → Import beside the Epic 5 CSV importers. **Money model (decided for this draft):** a booking deposit is an advance shown under "Payments received, not sales" with a payment receipt and no invoice; at seating it moves to the player's wallet; a forfeited deposit becomes a "No-show fee" sale with a tax invoice; tournament entry fees are a separate revenue type "Tournaments" — the "tournament income account" is that revenue type, not a general ledger; loyalty redemptions and referral bonuses are promotional wallet credit, reported apart from payments and sales. **Tiers:** bookings, tournaments, loyalty, referrals, memberships, the vault, game requests and importers run on every tier; WhatsApp reminders and owner commands stay paid-only as in Epic 8. **PII:** bookings, entrants, referrals and loyalty rows reference `player_id` only; screens show the username or "Erased player". **Out of this epic:** public online booking and assisted migration (SPEC non-goals), prize payouts (recorded as expenses with Story 5.25), team tournaments and formats other than single elimination, loyalty point expiry, and any coins, quests or points on the kiosk top bar (gamification stays banned). **Gates and sign-offs:** the launcher-vault stories (13.16–13.21) are gated — legal sign-off on Steam, Epic and Riot terms for shared café accounts must be recorded before build starts, and each carries that precondition AC; the Steam adapter is additionally gated on its own spike story, which must record a go; the tax treatment of booking deposits, no-show fees and promotional wallet credit needs CA sign-off before GA; the ggLeap and Smartlaunch source profiles (Stories 13.25–13.26) need real export samples from beta cafés before they are built. Every story adds its new intents to the Story 1.8 `proptest` generator.

### Story 13.1: Book a device or a group slot for a player

As counter staff,
I want to book a specific PC or "any PC in a group" for a player at a time,
So that regulars and groups can plan their visit and we never double-book a seat.

**Covers:** FR42 (reserve a device or group slot), AD-2, AD-7, AD-13, AD-14, AD-39, UX-DR18, UX-DR21, UX-DR66, UX-DR104

**Acceptance Criteria:**

**Given** Sessions → Bookings on the Hub (permission `booking.manage`, held by Counter, Manager and Owner)
**When** it opens
**Then** it shows a day view in branch local time with a date picker, one row per device grouped by group, existing bookings as blocks labelled with username, time and state, and a "New booking" action; empty days show "No bookings for this day."

**Given** "New booking"
**When** staff pick a profiled player with `player-lookup`, a device or a group, a start time and a length
**Then** `booking.create` writes a `bookings` row (new table: id, player, device or null, group, start, end, deposit 0, state `confirmed`, note, `rev`, `business_day` of the start) and the block appears in the day view within 2 s

**Given** a device booking that overlaps another booking on the same device, or a group booking that would make overlapping bookings in that group exceed the number of its devices not in maintenance
**When** it reaches `apply()`
**Then** it is refused `BOOKING_CONFLICT` naming the clashing booking or "No free PC in <group> at that time", and nothing is written

**Given** a start in the past, a length outside 30 min–12 h or not in 15-min steps, a start more than 30 days ahead, or a slot outside opening hours (Story 2.26)
**When** saved
**Then** a field error shows and nothing is written

**Given** a guest (no profile)
**When** staff try to book
**Then** the form asks to create or link a profile first (Story 3.6)

**Given** an existing booking
**When** staff edit its time, device or note, or cancel it
**Then** `booking.update` or `booking.cancel` applies with `base_rev` (`STALE` on conflict), the same conflict rules apply to the new slot, and `undo-toast` offers Undo for 8 s

**Given** the WAN is down
**When** bookings are created, edited or cancelled
**Then** they work identically with no network call

### Story 13.2: Take a booking deposit as a prepayment

As counter staff,
I want to take a deposit when someone books,
So that the café is covered if they don't turn up.

**Covers:** FR42 (deposit), FR70 (methods), AD-2, AD-13, AD-26, NFR10, UX-DR71, UX-DR72, UX-DR95

**Acceptance Criteria:**

**Given** Settings → Branch → Bookings (Manager)
**When** it opens
**Then** it shows "Default deposit" (amount, default ₹0 = no deposit) and "Deposit required" (off by default), validated as non-negative integers in minor units

**Given** a new or existing confirmed booking and a deposit amount above zero
**When** staff choose "Take deposit"
**Then** `settle-bar` opens with one "Booking deposit" line and the methods cash, UPI (last 4 digits), card reference or wallet; credit is not offered

**Given** the deposit is settled
**When** `booking.deposit_take` applies
**Then** an `orders` row with a `booking_deposit` line linked to the booking and its `payments` rows are written in one transaction, the booking stores `deposit_minor` and `order_id`, and no invoice number is assigned

**Given** "Deposit required" is on
**When** a booking is saved without a settled deposit
**Then** it is stored with state `held`, shows "Deposit due" in the day view, and does not count as confirmed until the deposit settles

**Given** a printer and the default receipt choice "Print"
**When** the deposit settles
**Then** a payment receipt prints in the Story 5.16 layout with "Booking deposit", booking date and time, device or group, and amount

**Given** Daily sales and the Z-report
**When** they cover a day with deposits taken
**Then** deposits appear under "Payments received, not sales" as "Booking deposits", never as sales, and the shift's expected cash includes cash deposits

**Given** Undo within 8 s of the deposit settle with no receipt printed
**When** staff press Undo
**Then** the deposit payments and order are reversed and the booking returns to its previous state

### Story 13.3: A booked device shows Reserved 15 minutes before the slot

As counter staff,
I want a booked PC to turn Reserved shortly before the booking,
So that nobody else sits there and I can see at a glance who is expected and when.

**Covers:** FR42 (Reserved 15 min before), FR18, FR23, AD-2, AD-12, UX-DR2, UX-DR22, UX-DR61, UX-DR65, UX-DR97, UX-DR101, UX-DR107

**Acceptance Criteria:**

**Given** a confirmed device booking and a free device
**When** the timer engine reaches 15 minutes before the start
**Then** a system `booking.window_opened` sets the device state `reserved`, and within 2 s the tile shows the reserved token with "Reserved", the username and the start time

**Given** a confirmed group booking
**When** its window opens
**Then** the Hub picks a free, online device in that group (lowest sort order first), stores it on the booking and marks it reserved; if none is free, the header alert list shows "No free PC for <username>'s booking at <time>" with `alert-badge`

**Given** the booked device is in session when the window opens
**When** the tile updates
**Then** it stays "In session" with the secondary line "Booked <time> · <username>" and raises `alert-badge` "BOOKED" with the alert sound once, so staff can transfer the session (Story 3.32) or move the booking to another free device

**Given** a reserved device
**When** anyone starts a session on it, or a kiosk self-start arrives
**Then** `apply()` refuses `DEVICE_RESERVED`; the Hub shows "Reserved for <username> at <time>" and the kiosk shows "This PC is reserved. Ask at the counter."

**Given** a reserved tile
**When** staff open its sheet
**Then** it shows the booking (username, time, deposit) and a secondary "Release"

**Given** "Release" on a reserved tile
**When** applied
**Then** the device returns to free, a device booking stays confirmed but is unassigned from the device (a group booking keeps its group), and the action is audited

**Given** a booking cancelled or moved after its window opened
**When** the change applies
**Then** the old device returns to free within 2 s and a new device in the window becomes reserved

### Story 13.4: Seat the booking when the player arrives

As counter staff,
I want to start the booked player's session from the reserved tile in one tap,
So that the player is playing within seconds and their deposit counts toward the bill.

**Covers:** FR42 (seating), FR27, FR31, AD-2, AD-13, NFR10, UX-DR65, UX-DR97, UX-DR104

**Acceptance Criteria:**

**Given** a reserved tile
**When** staff open its sheet
**Then** it offers "Start for reservation" (primary) above "Release"; Start opens the normal start area with the booked player pre-selected and every start mode valid for the group

**Given** "Start for reservation"
**When** `session.start{booking_id}` applies
**Then** the booking becomes `seated`, its settled deposit is credited to the player's wallet as a `wallet_txns` row with reason `booking_deposit` in the same transaction, the deposit order is marked applied, and the tile shows "In session"

**Given** the booked player logs in at the reserved kiosk and picks a plan
**When** the start arrives
**Then** it is accepted despite the reservation and seats the booking exactly as from the Hub

**Given** another player logs in at the reserved kiosk
**When** they pick a plan
**Then** the Story 13.3 refusal applies

**Given** the player arrives before the window opens
**When** staff choose "Seat now" on the booking in Sessions → Bookings with the booked device free
**Then** the session starts and the booking is seated as above

**Given** Undo within 8 s of seating
**When** staff press Undo
**Then** the session and the wallet credit are reversed and the booking returns to `confirmed` with the device reserved

### Story 13.5: No-show and late-cancellation rules release the slot

As a café owner,
I want no-shows to free the PC automatically and late cancellations to be flagged,
So that empty reserved PCs don't sit idle on a busy evening.

**Covers:** FR42 (no-show rules), AD-2, AD-13, UX-DR21, UX-DR66, UX-DR101

**Acceptance Criteria:**

**Given** Settings → Branch → Bookings
**When** a Manager edits it
**Then** it also shows "No-show after" (default 15 min past start, 5–60) and "Free cancellation until" (default 2 h before start, 0–48 h), saved with `settings.set` and applying to bookings created after the save

**Given** a confirmed booking not seated by start + "No-show after"
**When** the timer engine reaches that time
**Then** a system `booking.no_show` sets the booking `no_show` and returns a reserved device to free within 2 s

**Given** a cancellation after the free-cancellation cutoff
**When** staff cancel
**Then** the booking is stored `cancelled` with `late = true`, and the sheet shows "Late cancellation"

**Given** a no-show or a cancellation
**When** the player profile opens
**Then** it shows "No-shows: n" and "Late cancellations: n"

**Given** a no-show applied while the player arrives a minute later
**When** staff Undo it within 8 s, or a Manager reverses it with `inline-pin`
**Then** the booking returns to confirmed and the device to reserved

**Given** Reports → Bookings (added to the Story 5.27 list)
**When** it renders for a range
**Then** it shows bookings made, seated, no-shows and cancellations (late and free) by group

### Story 13.6: Deposits are kept or returned by the no-show and cancellation rules

As a café owner,
I want the deposit kept on a no-show or late cancellation and returned otherwise,
So that the café is covered without anyone settling it by hand.

**Covers:** FR42 (deposit and no-show rules), FR75, AD-2, AD-13, AD-26, NFR10, UX-DR66, UX-DR95

**Acceptance Criteria:**

**Given** Settings → Branch → Bookings
**When** a Manager edits it
**Then** it also shows "Deposit kept on no-show or late cancellation" (default 100%, 0–100% in 5% steps), applying to bookings created after the save

**Given** a booking with a settled deposit becomes `no_show`, or is cancelled late
**When** the Story 13.5 transition applies
**Then** in the same transaction the kept share becomes a "No-show fee" sale with a tax invoice at the branch default service rate (Stories 5.2, 5.14), paid by method `deposit`, and any remainder is credited to the player's wallet

**Given** a cancellation before the free-cancellation cutoff
**When** `booking.cancel` applies
**Then** the deposit is returned to the wallet by default, or by its original method if staff choose (cash needs an open shift), as negative `payments` against the deposit order

**Given** a late cancellation
**When** a Manager waives the fee with `inline-pin` and a reason
**Then** the whole deposit is returned as above, audited with both actors

**Given** a no-show reversed under Story 13.5 before the fee invoice prints
**When** the reversal applies
**Then** the fee sale and wallet remainder are reversed; once the invoice has printed, the fee can only be refunded with Story 5.24

**Given** Reports → Bookings
**When** it renders for a range
**Then** it also shows deposits taken, applied, refunded and kept, and kept + applied + refunded + held equals deposits taken to the minor unit; no-show fees appear in Daily sales and the Z-report as sales type "No-show fees"

### Story 13.7: Create a tournament that blocks devices for the event

As a branch manager,
I want to set up a tournament with its time and the PCs it uses,
So that those PCs are held for the event and nobody books or starts on them.

**Covers:** FR43 (devices blocked for the event), AD-2, AD-7, AD-39, UX-DR2, UX-DR18, UX-DR66, UX-DR97

**Acceptance Criteria:**

**Given** Sessions → Tournaments (permission `tournament.manage`, Manager and Owner)
**When** a Manager creates a tournament
**Then** `tournament.upsert` stores name, game (optional library entry), start and end, entry fee, maximum entrants (2–128) and the devices it uses in new `tournaments` and `tournament_devices` tables, with state `draft`, and the list shows it with its state

**Given** a draft tournament
**When** the Manager chooses "Open entries"
**Then** it becomes `open` and its devices are blocked from start to end; a clash with a confirmed booking or another tournament on any of them is refused `TOURNAMENT_CONFLICT` listing each clash, and nothing changes

**Given** an open tournament
**When** the timer engine reaches 15 minutes before its start
**Then** each free blocked device becomes `reserved` and its tile shows "Reserved", "Tournament" and the tournament name; a blocked device still in session gets the "BOOKED" alert as in Story 13.3

**Given** a blocked device during the event window
**When** a normal session start or a booking for it arrives
**Then** it is refused `DEVICE_RESERVED` or `BOOKING_CONFLICT` with "Held for <tournament> until <time>"

**Given** a tournament end time
**When** the timer engine reaches it
**Then** every remaining block lifts and those devices return to free within 2 s

**Given** an edit to an open tournament's time or devices
**When** saved with `base_rev`
**Then** the same conflict rules apply to the new window; a Counter attempting any edit gets `inline-pin`

### Story 13.8: Register entrants and post entry fees to tournament income

As counter staff,
I want to sign players up and take their entry fee in one step,
So that every entry is paid, receipted and counted as tournament income.

**Covers:** FR43 (entry fee to tournament income), AD-2, AD-13, AD-26, NFR10, UX-DR67, UX-DR71, UX-DR72, UX-DR95

**Acceptance Criteria:**

**Given** an open tournament with free places
**When** staff choose "Add entrant" and pick a profiled player
**Then** `settle-bar` opens with one "Entry — <tournament>" line at the entry fee with its tax rate (the branch default service rate unless the Manager set another on the tournament), payable by any method including wallet and credit

**Given** the entry is settled
**When** `tournament.enter` applies
**Then** a `tournament_entrants` row (tournament, player, order, state `registered`) and the order with a `tournament_entry` line are written in one transaction, the invoice follows Story 5.14, and the entrant list shows the username and "Paid"

**Given** a tournament with a zero entry fee
**When** an entrant is added
**Then** the entrant row is written with no order

**Given** a full tournament, a player already registered, a tournament not open, or a guest
**When** an entry is attempted
**Then** it is refused `TOURNAMENT_FULL`, `ALREADY_ENTERED`, `TOURNAMENT_NOT_OPEN` or asked to create a profile, and nothing is written

**Given** Daily sales, the Z-report and Reports → Tournaments
**When** they cover entry fees
**Then** entry fees appear as their own sales type "Tournaments", never inside play time or products, and Reports → Tournaments lists each tournament with entrants, entry income, tax and refunds

**Given** an entrant withdraws before the start
**When** a Manager refunds the entry with Story 5.24 (credit note) or `undo-toast` within 8 s
**Then** the entrant becomes `withdrawn`, the place is freed, and tournament income nets the refund; after the start the refund action is disabled with "The tournament has started"

**Given** a Manager cancels the tournament
**When** they confirm with `inline-pin` and a reason
**Then** each paid entry is refunded by its own refund intent with its own idempotency key (to wallet by default), the blocks lift, and the tournament shows `cancelled`

### Story 13.9: Generate a single-elimination bracket and record results

As a branch manager,
I want the Hub to draw the bracket and move winners on as I enter results,
So that the event runs on time and everyone can see who plays next.

**Covers:** FR43 (bracket), AD-2, AD-13, AD-39, UX-DR18, UX-DR21, UX-DR105, UX-DR110

**Acceptance Criteria:**

**Given** an open tournament with at least 2 registered entrants
**When** the Manager chooses "Close entries and draw" with seeding Random or Manual order
**Then** `tournament.draw` writes `tournament_matches` rows (round, slot, entrant A, entrant B, winner, `rev`) for a single-elimination bracket of the next power of two, gives byes to the top seeds, advances bye winners, stores the random seed so the draw can be reproduced, and sets the tournament `running`

**Given** fewer than 2 entrants
**When** a draw is attempted
**Then** it is refused `TOURNAMENT_TOO_SMALL`

**Given** the bracket screen
**When** it renders
**Then** rounds show as columns of matches with usernames (or "Erased player"), bye slots read "Bye", the current round's open matches are marked "Next" with text, not colour alone, and it works by keyboard alone

**Given** an open match
**When** staff record a winner
**Then** `tournament.result` sets the winner and places them in the next round's slot, and `undo-toast` offers Undo for 8 s

**Given** a recorded result whose next match already has a result
**When** staff try to change it
**Then** it is refused `RESULT_LOCKED` with "Clear the later result first"

**Given** the final's result
**When** it is recorded
**Then** the tournament becomes `completed` and shows the champion and runner-up

### Story 13.10: Tournament play on blocked devices, and releasing them early

As counter staff,
I want to start entrants on the tournament PCs without charging play time,
So that the entry fee covers the event and the PCs go back on sale as soon as they're done.

**Covers:** FR43 (blocked devices in use), FR140, AD-2, AD-8 (additive enum), AD-12, UX-DR65, UX-DR97

**Acceptance Criteria:**

**Given** a reserved tournament device during its event window
**When** staff open its sheet
**Then** it offers "Start tournament play" with a picker limited to that tournament's registered entrants

**Given** an entrant is picked
**When** `session.start{mode: event, tournament_id}` applies
**Then** a session with mode `event` (added to the session mode enum by an additive migration) and a zero rate starts, the kiosk unlocks to the normal session screen, and the session's `ends_at` is the tournament end

**Given** a non-entrant or a tournament outside its window
**When** an event start is attempted
**Then** it is refused `NOT_AN_ENTRANT` or `TOURNAMENT_NOT_RUNNING`

**Given** an event session reaching the tournament end
**When** the timer engine ticks
**Then** grace and the branch time-up mode apply as for any session (Story 3.26), and no bill is created

**Given** an event session ended by staff
**When** it ends
**Then** no bill opens, the device stays reserved for the tournament, and the session counts in Reports → Sessions under group minutes with revenue zero

**Given** a Manager chooses "Release from tournament" on a blocked device with no event session
**When** they confirm
**Then** the block lifts for that device only, it returns to free within 2 s, and the action is audited

### Story 13.11: Loyalty settings and points earned on settled bills

As a café owner,
I want players to earn points on what they spend, at a rate I choose,
So that regulars have a reason to come back.

**Covers:** FR55 (points per rupee, rates per branch), AD-2, AD-7, AD-13, NFR10, UX-DR18, UX-DR38, UX-DR95

**Acceptance Criteria:**

**Given** Settings → Branch → Loyalty (Manager)
**When** it opens
**Then** it shows Loyalty on/off (default off), "Points per ₹100 spent" (default 5), "Value of 100 points" (default ₹10) and "Minimum to redeem" (default 100 points), using the branch currency symbol, saved with `settings.set` and applying to bills settled after the save

**Given** loyalty is on and a bill for a profiled player settles
**When** the settle applies
**Then** in the same transaction a `loyalty_txns` row (new append-only table: player, points, reason `earn`, order, at) adds floor(eligible amount × rate) points and `loyalty_balances` (new table, points ≥ 0) is updated; the eligible amount is the bill total minus the part put on credit, and top-ups, deposits and credit collections earn nothing

**Given** a guest bill or loyalty off
**When** it settles
**Then** no points are written

**Given** Undo of a settle, or a refund (Story 5.24)
**When** it applies
**Then** a `reversal` row removes the points earned on the undone or refunded amount; if the balance is smaller, it goes to zero and the shortfall is recorded on the row, never below zero

**Given** a printed receipt for a player who earned points
**When** it prints
**Then** it shows "Points earned: n · Balance: n" after the totals

**Given** the Hub player profile and the kiosk Account → Wallet & packs
**When** they open for a player with loyalty on
**Then** they show the points balance and its wallet value ("340 points · worth ₹34 · Redeem at the counter."); the kiosk top bar and HUD show no points

**Given** a player erased with Story 9.1
**When** the tombstone applies
**Then** their points balance is set to zero by a `reason: erased` row

### Story 13.12: Redeem loyalty points as wallet credit

As counter staff,
I want to turn a player's points into wallet money when they ask,
So that the reward is real and easy to spend on play or food.

**Covers:** FR55 (redeem as wallet credit), FR47, AD-2, AD-13, NFR10, UX-DR21, UX-DR67

**Acceptance Criteria:**

**Given** a player profile with points at or above the minimum
**When** staff choose "Redeem points" and enter a number of points
**Then** the panel shows the wallet value at the current rate and a primary "Redeem"

**Given** Redeem
**When** `loyalty.redeem` (permission `loyalty.redeem`, held by Counter) applies
**Then** in one transaction a `redeem` row removes the points and a `wallet_txns` row with reason `loyalty_redeem` adds the value; no payment, cash event or invoice is written

**Given** points above the balance or below the minimum
**When** submitted
**Then** it is refused `POINTS_INSUFFICIENT` or `BELOW_MIN_REDEEM` with the numbers shown, and nothing is written

**Given** Undo within 8 s
**When** the wallet still holds the redeemed value
**Then** both rows are reversed; if part was already spent, Undo is refused `INSUFFICIENT_BALANCE`

**Given** Daily sales
**When** it covers redemptions
**Then** a "Promotional wallet credit" section shows loyalty redemptions as a value, separate from sales and from payments received

### Story 13.13: Referral bonus on a referred player's first paid session

As a café owner,
I want players who bring a friend to get a bonus once the friend actually plays and pays,
So that word of mouth grows the café without giving away free money.

**Covers:** FR55 (referral bonus on first paid session), FR44, FR121, AD-2, AD-24, NFR13, UX-DR18, UX-DR29

**Acceptance Criteria:**

**Given** Settings → Branch → Loyalty
**When** a Manager edits it
**Then** it also shows Referrals on/off (default off), "Bonus to the referrer" (default ₹50) and "Bonus to the new player" (default ₹0)

**Given** referrals on
**When** staff create a profile (Story 3.6) or a player signs up at the kiosk (Story 6.10)
**Then** the form has an optional "Referred by" field (phone or username at the Hub, username at the kiosk), and a valid, profiled, non-erased referrer other than the player is stored as `players.referred_by`

**Given** an unknown, erased or self referrer
**When** the form is submitted at the Hub
**Then** the field shows "No player found" and the profile is not saved until the field is fixed or cleared; at the kiosk the profile is created and the referrer is silently not recorded, so the kiosk never reveals whether a player exists

**Given** a referred player's first session bill settles with an amount paid above zero
**When** its 8 s Undo window closes without Undo
**Then** a system `referral.rewarded` writes the bonuses as `wallet_txns` rows with reason `referral_bonus` to the referrer and, if above zero, the new player, and records the reward on the referred player so it never repeats

**Given** a guest session, a zero bill, a bill reverted within its window, or referrals off at the moment of reward
**When** it settles
**Then** no bonus is written

**Given** the referrer has been erased before the reward
**When** the reward runs
**Then** only the new player's bonus (if any) is written

**Given** Daily sales
**When** it covers rewards
**Then** referral bonuses appear in "Promotional wallet credit" beside loyalty redemptions, and Reports → Players shows referrals made and rewarded per referrer

### Story 13.14: Monthly membership plans — sell, start and renew

As a café owner,
I want to sell a monthly membership that players renew at the counter,
So that my regulars pay every month and play within their window.

**Covers:** FR56 (monthly recurring pass), FR58, FR59, FR33, AD-7, AD-39, UX-DR48, UX-DR72

**Acceptance Criteria:**

**Given** Settings → Plans
**When** a Manager creates a membership
**Then** `plan.upsert{kind: membership}` stores name, price, allowed groups and either "All day" or a play window that may wrap midnight, with the period fixed at one calendar month; console and PC groups cannot be mixed (`GROUP_KIND_MISMATCH`)

**Given** a profiled player with no active membership of that plan
**When** staff sell it (Story 3.22 flow)
**Then** a `player_packs` row is created with expiry at the end of the day before the same date next month in branch local time (clamped to the month's last day, e.g. 31 Jan → 28 Feb)

**Given** a player whose membership of that plan is still active
**When** staff sell it again
**Then** the sale is a renewal: the expiry moves one calendar month on from the current expiry, not from today, and the profile shows "Renewed to <date>"

**Given** an owned, active membership allowed for the group
**When** staff start it, or the player picks it in the kiosk `plan-picker` (card "Membership · until <date>")
**Then** the session runs as a pass session (Story 3.27): inside the window it ends at the window end, "All day" ends at closing time, and outside the window it is refused `PASS_OUTSIDE_WINDOW`

**Given** a membership and a pass held together
**When** either is sold
**Then** `PASS_ALREADY_ACTIVE` does not apply between them; it still applies between two passes

**Given** the player profile and kiosk Account → Wallet & packs
**When** a membership is within 3 days of expiry, or has expired
**Then** it shows "Renews by <date>" with a warning tag, or "Lapsed" muted

### Story 13.15: WhatsApp renewal reminder for memberships

As a member,
I want a WhatsApp before my membership runs out,
So that I can renew without losing a day.

**Covers:** FR56 (WhatsApp renewal reminder), FR52 (consent and paid-tier pattern), AD-27, AD-29, AD-34, UX-DR24, UX-DR96

**Acceptance Criteria:**

**Given** Settings → Notifications (Story 5.15)
**When** it opens
**Then** it also lists "Membership renewal" with on/off and a template preview ("Hi <name>, your <plan> membership at <branch> ends on <date>. Renew at the counter."), and on Free it shows `tier-lock` "Available on Basic"

**Given** a paid branch with the message on and a member with current consent
**When** the membership is 3 days from expiry, and again on the expiry day, at the branch opening time
**Then** one `notify.requested` (kind membership_renewal, plan, expiry date) is written for each point, never twice for the same point

**Given** the membership is renewed before a queued row drains
**When** the drain (Story 8.29) re-checks the row
**Then** it is marked `skipped` with reason `renewed` and nothing is sent

**Given** a guest, a player without consent, an erased player or the message off
**When** the trigger fires
**Then** no outbox row is written, or the queued row is skipped

**Given** a Free branch
**When** Players opens
**Then** a "Renewals due" list shows memberships ending in the next 3 days or lapsed in the last 7, with username, plan and date, instead of messages

### Story 13.16: Café-owned launcher accounts kept in an encrypted Hub vault

As a branch manager,
I want to store the café's Steam, Epic and Riot accounts once at the Hub and say which games each owns,
So that players can play our licensed games without anyone ever seeing the passwords.

**Covers:** FR138 (Hub holds shared launcher accounts), AD-22, AD-24 (keys outside the change log), AD-5 (no secrets in commands), NFR11, NFR13, UX-DR18, UX-DR66

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Steam, Epic and Riot shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** Settings → Games → Café accounts (permission `vault.manage`, Manager and Owner)
**When** a Manager adds an account
**Then** they enter launcher (Steam · Epic · Riot), label, login name, password, an optional authenticator secret, and the library entries (Story 6.3) with that launcher that the account owns

**Given** the account is saved
**When** `vault.account_upsert` applies
**Then** the change log gets a `vault_accounts` row with id, launcher, label, owned entries and `rev` only; the login name, password and authenticator secret are encrypted with a vault data key and stored in `vault_secrets` (new table outside the change log), and the vault data key is kept wrapped by the branch KEK beside `player_keys`

**Given** a saved account
**When** anyone views it
**Then** the password and secret are never shown again; the login name shows masked except its first 2 characters, and "Replace password" writes a new ciphertext; every add, replace and remove is audited without any secret

**Given** a paid-tier Hub and a standby
**When** `vault_secrets` changes
**Then** the Story 9.2 key backup includes the wrapped vault data key and ciphertext, and Story 9.19 replication carries them to the standby over LAN

**Given** the command catalogue, change-log batches, snapshots, the export file (Story 9.4), the Owner API and the Hub log
**When** a CI privacy test creates an account and scans all of them
**Then** no plaintext login name, password or secret appears, and no command kind can carry vault fields

**Given** a library entry owned by at least one café account
**When** the library list renders
**Then** it shows "Café accounts: n"

### Story 13.17: A secure sign-in helper signs a café account into a launcher and reports sign-out at session end

As a branch manager,
I want the kiosk to fetch a café login only for the seat that is playing, keep it out of the player's reach, and tell the Hub when the launcher is signed out again,
So that our passwords never touch disk, logs or the screen and the Hub knows when an account is free.

**Covers:** FR138 (kiosk injects login at launch; sign-out at session end), AD-31, AD-34, AD-42, NFR11, NFR12, NFR13, UX-DR40, UX-DR101, UX-DR103

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Steam, Epic and Riot shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** the Games tab and a library entry with café accounts on a PC where its launcher is not signed in to a player's own account
**When** it renders
**Then** the `game-tile` shows a "Café account" text tag

**Given** the player activates that tile during an active session
**When** the kiosk handles it
**Then** the UI asks the kiosk service to launch; the service makes a one-use `vault.secret{session_id, entry_id}` call over the pinned paired-device connection, and the Hub answers only for the token of the device that holds that active session, picking the least recently used account owning the entry; any other caller gets `FORBIDDEN` and every answer is audited without the secret (exclusive leases are added to this call by the next story)

**Given** the secret in the service
**When** it passes it to a launcher adapter
**Then** it goes through a short-lived helper started in the player's desktop session behind a `LauncherSignIn` interface with a recorded fake; the secret is held in memory only, zeroed after use, and never sent to the UI process, written to disk or logged

**Given** the adapter reports a sign-in failure
**When** the kiosk handles it
**Then** the service sends `vault.signin_failed{session_id, account_id, reason}` to the Hub, which records it as an audited intent, and the player sees "Couldn't sign in — ask at the counter."

**Given** session end by any path
**When** Story 6.12 cleanup signs the launcher out
**Then** the service sends `vault.signed_out{session_id, account_id}` or `vault.signout_failed{session_id, account_id, reason}` over the pinned connection, retried with backoff until the Hub acknowledges it, and the Hub records each as an audited intent with no secret

**Given** the Hub is unreachable
**When** a café-account tile is activated
**Then** nothing is sent and the player sees "Counter is reconnecting — try again shortly."

**Given** an automated test with the recorded fake adapter
**When** a launch, play and session end run
**Then** the Hub holds a `vault.signed_out` record for the session, and a scan of kiosk logs, temp folders, crash dumps and the UI process memory finds no secret

### Story 13.18: The Hub lends one vault account per seat and takes it back at session end

As a branch manager,
I want each café account used by only one PC at a time and returned when the session ends,
So that two players never fight over one login and nobody keeps our account.

**Covers:** FR138 (one account per concurrent seat, released at session end), AD-2, AD-12, NFR10, UX-DR22, UX-DR65, UX-DR101, UX-DR107

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Steam, Epic and Riot shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** a player in an active session asks to play a library entry with café accounts
**When** `vault.checkout{session_id, entry_id}` (player self-service permission `vault.self_checkout`) applies
**Then** it picks the least recently used account owning that entry that is neither leased nor quarantined and writes a `vault_leases` row (new table: account, session, device, state `active`, at), enforced by a unique partial index of one active lease per account and one per session per launcher

**Given** the Story 13.17 `vault.secret` call
**When** this story ships
**Then** it requires a `lease_id` and is answered only for the leased device's token within 60 s of checkout, replacing the Story 13.17 per-session account pick; the kiosk service checks out before fetching the secret

**Given** the session already holds an active lease for that launcher on an account that owns the entry
**When** another checkout arrives
**Then** the existing lease is returned and no second lease is created

**Given** every account owning the entry is leased or quarantined
**When** a checkout arrives
**Then** it is refused `VAULT_BUSY` and the player sees "All café accounts for this game are in use — try again soon."

**Given** a session with an active lease ends by any path (player, staff, timer, forced end, transfer, orphaned)
**When** the end applies
**Then** in the same transaction its leases move to `releasing`, and the Story 13.17 `vault.signed_out` report for that session and account moves them to `released`

**Given** a lease in `releasing` that gets a Story 13.17 `vault.signout_failed` report, or no report within 5 minutes, or an active lease that gets `vault.signin_failed`
**When** the timer engine or the report applies
**Then** the lease becomes `quarantined` with the reason, the account is not lent again, and the tile raises `alert-badge` "CHECK PC" with "Café account <label> may still be signed in"

**Given** Settings → Games → Café accounts
**When** it renders
**Then** each account shows "Free", "In use · <device> · since <time>" or "Needs check", and a Manager can "Mark signed out" a quarantined account with `inline-pin`, audited

**Given** two checkouts for the last free account arriving together
**When** both reach `apply()`
**Then** exactly one gets the account and the other gets `VAULT_BUSY`

### Story 13.19: Spike — prove a Steam sign-in method for café accounts

As a developer on Arena 360,
I want a time-boxed spike that proves how a café account can be signed into current Steam,
So that the Steam adapter is built only on a method we know works and we know what breaks it.

**Covers:** FR138 (Steam — feasibility), FR134, AD-34, NFR12

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** the spike starts
**Then** legal sign-off on Steam shared café accounts is recorded in the decision log; without it the spike stays blocked

**Given** a time box of 3 working days on Windows 10 and 11 with a real test Steam account
**When** the spike ends
**Then** a written result in the repository names the sign-in method that works on current Steam (login window automation, command line or other), how Steam Guard codes are produced from the authenticator secret, how "Remember me" is kept off, how a signed-out start is guaranteed, and what breaks it (Steam client updates, extra verification, captcha)

**Given** the result
**When** it is reviewed
**Then** it records a go or no-go for the Story 13.17 `LauncherSignIn` interface; a no-go blocks the Steam adapter story and is raised as a risk on FR138, and no production adapter code is merged from the spike

### Story 13.20: The Steam adapter signs a café account into Steam

As a player,
I want to play a café-owned Steam game with one click,
So that I can play games I don't own without typing any login.

**Covers:** FR138 (Steam), FR134, AD-34, NFR12, UX-DR101

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Steam shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** the Story 13.19 spike result
**When** build of this story starts
**Then** it records a go, and the adapter uses only the sign-in method the spike proved; with a no-go the story stays blocked

**Given** the Story 13.17 helper and a Steam lease
**When** the Steam adapter runs
**Then** it starts Steam signed out, fills the login and any Steam Guard code, turns off "Remember me", then starts the game by its Steam id

**Given** a wrong password, extra verification or Steam offline
**When** sign-in is attempted
**Then** the adapter gives up within 30 s and reports the failure reason to the Story 13.17 failure path

**Given** an automated Windows 10 and 11 test with a real test Steam account
**When** a player launches, plays, and the session ends
**Then** the game starts signed in, the next session opens Steam signed out, and no password is found by the Story 13.17 scan

### Story 13.21: Epic Games and Riot Client sign-in from the vault

As a player,
I want café-owned Epic and Riot games to work the same way as Steam,
So that every café game is one click whatever launcher it uses.

**Covers:** FR138 (Epic, Riot), AD-31, AD-34, NFR12, UX-DR40, UX-DR101

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Epic and Riot shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** the Story 13.17 `LauncherSignIn` interface
**When** this story ships
**Then** it adds Epic Games and Riot Client adapters, each with a recorded fake, that sign in with the leased login and an authenticator code where the account has one, start the entry by its launcher URI, and never persist the login

**Given** an Epic or Riot account whose launcher asks for an emailed code or a captcha
**When** sign-in is attempted
**Then** the adapter gives up within 30 s, the account is quarantined with "Needs a code we can't enter — switch the account to an authenticator app", and the player sees the Story 13.17 failure line

**Given** session end
**When** Story 6.12 cleanup runs
**Then** Epic and Riot sign-out is confirmed to the Hub with `vault.signed_out`

**Given** the same automated test as Story 13.20 with Epic and Riot test accounts
**When** it runs on Windows 10 and 11
**Then** the game starts signed in and the next session opens each launcher signed out with no residual credential

### Story 13.22: Players request a game from the kiosk and the Hub collects the requests

As a player,
I want to ask the café to add a game I want to play,
So that the café knows what its players are looking for.

**Covers:** FR139 (request from kiosk, appears on Hub), AD-2, AD-17, UX-DR18, UX-DR40, UX-DR100, UX-DR103

**Acceptance Criteria:**

**Given** the Games tab
**When** it renders
**Then** a "Request a game" action sits after the grid and in the empty state ("No games here yet — ask at the counter." gains the action)

**Given** "Request a game"
**When** the panel opens
**Then** the player can search the catalogue the Hub has cached (Story 6.2) by name, or choose "Not listed? Type the name" (2–60 characters), and submit with one primary button

**Given** a submit
**When** `game.request{catalogue_id | name}` applies (player self-service permission `game.self_request`, or the device actor for a guest session)
**Then** a `game_requests` row (new table: catalogue id or normalised name, player or null, device, at, status `open`) is written and the player sees "Request sent — thanks!"

**Given** the same player requests the same game again within 30 days, or makes a fourth request in a business day (or a guest session a fourth)
**When** it arrives
**Then** a repeat shows "You've already asked for this one." and the daily cap shows "That's enough requests for today."; no row is written

**Given** Settings → Games → Requests on the Hub
**When** it opens
**Then** it lists requested games grouped by catalogue id or case-insensitive name, with request count, distinct players, last requested and status, newest first, and the tab shows the number of open games

**Given** a Manager chooses "Mark added" (optionally linking a library entry) or "Dismiss"
**When** `game_request.resolve` applies
**Then** every open request for that game takes the new status, and new requests for it start a fresh open group

**Given** the Hub is unreachable
**When** the player submits
**Then** nothing is sent and the panel shows "Counter is reconnecting — try again shortly."

### Story 13.23: Game request counts in Hub reports

As a café owner,
I want to see how often each game was requested over a period,
So that I buy or install the games my players actually want.

**Covers:** FR139 (request counts per game in reports), FR108, UX-DR88

**Acceptance Criteria:**

**Given** Reports → Game requests (added to the Story 5.27 list, permission `report.view`)
**When** it renders for a range
**Then** it lists each requested game with request count, distinct players and current status, sorted by count, with ties by most recent

**Given** requests from guests and erased players
**When** counted
**Then** they count toward the totals and no player names appear

**Given** the report
**When** exported
**Then** CSV and PDF follow Story 5.32 with the same figures as the screen

**Given** a 90-day range with 10,000 requests on reference hardware
**When** the report renders
**Then** it appears within 2 s without blocking `apply()`

### Story 13.24: Import framework — source detection, field mapping and a dry-run report

As a café owner moving from another system,
I want to upload my old system's export, check how its columns map, and see a full report before anything changes,
So that my regulars and their balances come across without surprises.

**Covers:** FR22 (field mapping, dry-run), FR7, AD-2, AD-14, AD-24, NFR6, NFR13, UX-DR18, UX-DR84

**Acceptance Criteria:**

**Given** Settings → Import → "From another system" (Manager)
**When** a Manager picks an export (CSV, UTF-8 or UTF-16, up to 20 MB)
**Then** the Hub matches its header against versioned source profiles in `arena-proto`; a match shows "<source> export detected", and no match offers "Map columns yourself"

**Given** a file
**When** the mapping screen opens
**Then** each source column is listed with a sample value and a target (username, name, phone, email, DOB, wallet balance, credit limit, notes, or Ignore), pre-filled from a matched profile, required targets (phone or username, name) are marked, and the Manager can change any mapping and save it for this branch under a name for re-use

**Given** "Run dry-run"
**When** it runs
**Then** every row is checked with the Story 5.34 rules plus phone normalisation to E.164 with the branch country code, and the report shows new, matched (by phone, then username), skipped and failed counts with each failure's row and reason; nothing is written

**Given** the dry-run report
**When** the Manager chooses "Download report"
**Then** a CSV of every row's outcome is saved with phones masked except the last 4 digits

**Given** commit
**When** it applies
**Then** players are created through the Story 5.34 path in chunks of 500, each with its own idempotency key, with personal data encrypted, balances as `wallet_txns` reason `import`, consent recorded as not granted, the source username kept when free or suffixed "-1", "-2" otherwise, and no password (players set one at the counter with Story 3.8)

**Given** commit or cancel
**When** it finishes
**Then** the uploaded file is deleted from the Hub and was never sent to the cloud

**Given** a 1,000-row file on reference hardware
**When** dry-run plus commit runs
**Then** it completes in under 30 s

**Given** onboarding steps 5 and 6 (Story 2.28)
**When** they render before go-live
**Then** they show "Moving from another system? Import at the Hub: Settings → Import."

### Story 13.25: Import a ggLeap players export

As a café owner moving from ggLeap,
I want the Hub to recognise my ggLeap export and map it for me,
So that switching takes minutes, not an afternoon of matching columns.

**Covers:** FR22 (ggLeap), FR7, AD-8 (versioned profiles), NFR6

**Acceptance Criteria:**

**Given** real ggLeap user exports collected from beta cafés
**When** the ggLeap source profile is built
**Then** it is added to `arena-proto` with its header signature, column mapping and a fixture-based test per export variant

**Given** a ggLeap user export
**When** it is picked in the Story 13.24 importer
**Then** it shows "ggLeap players export detected" and the mapping is pre-filled for every known column

**Given** a ggLeap export with an extra or renamed column
**When** detected
**Then** the known columns stay mapped, the unknown column defaults to Ignore, and the screen says "1 column we don't recognise — check the mapping"

**Given** ggLeap date and phone formats in the fixtures
**When** the dry-run runs
**Then** every fixture row parses with no failures, and the commit result equals the fixture's expected players and balances

**Given** a 1,000-row ggLeap export on reference hardware
**When** dry-run plus commit runs
**Then** it completes in under 30 s
### Story 13.26: Import a Smartlaunch export through the same mapping and dry-run

As a café owner moving from Smartlaunch,
I want to bring my Smartlaunch members across the same way,
So that switching doesn't depend on which system I used before.

**Covers:** FR22 (Smartlaunch), FR7, AD-24, NFR6, NFR13

**Acceptance Criteria:**

**Given** the Story 13.24 importer
**When** a Manager picks a Smartlaunch member export as CSV or Excel (.xlsx, first sheet)
**Then** a Smartlaunch source profile built from real Smartlaunch exports collected from beta cafés, with a fixture test per export variant, detects it and pre-fills the mapping

**Given** Smartlaunch-specific fields (member group, separate first and last name, balance as a decimal with a currency sign)
**When** they are mapped
**Then** first and last name join into name, the balance is parsed into minor units of the branch currency, and member group maps to player tags

**Given** a balance with a currency other than the branch currency, or more decimals than the currency allows
**When** the dry-run runs
**Then** that row fails with "Currency doesn't match the branch" or "Amount has too many decimals"

**Given** the dry-run, report download, commit, deletion and performance rules
**When** used with a Smartlaunch file
**Then** they behave exactly as in Story 13.24

### Story 13.27: Migrated time balances and products from ggLeap and Smartlaunch

As a café owner moving from another system,
I want players' remaining time and my product list to come across too,
So that nobody loses prepaid hours and the shop is ready on day one.

**Covers:** FR22 (exports beyond players), FR7, FR55, AD-2, AD-14, NFR6

**Acceptance Criteria:**

**Given** a players export with a time-balance column (ggLeap or Smartlaunch)
**When** it is mapped to "Time balance"
**Then** the mapping screen asks for the allowed groups and a validity (default 90 days), and the dry-run shows how many players get migrated time and its total hours

**Given** commit
**When** it applies
**Then** each player with time above zero gets a `player_packs` row on a system plan "Migrated time" (created once, inactive for sale) with those minutes, groups and expiry, and the pack shows on the profile and kiosk like any pack

**Given** a column of points or coins
**When** it is mapped to "Loyalty points" and loyalty is on (Story 13.11)
**Then** points are imported as `loyalty_txns` with reason `import`; with loyalty off the target is unavailable

**Given** a ggLeap or Smartlaunch product export
**When** it is detected
**Then** its columns map to the Story 5.33 product fields with the same dry-run, conflict and commit rules, and offers or time plans in the export map to packs with the Story 5.34 plan rules

**Given** a time value in a format the profile doesn't know
**When** the dry-run runs
**Then** that row fails with "Can't read this time value" and the sample is shown

### Story 13.28: Owner commands for loyalty, referral and booking rules and memberships

As a café owner,
I want to set loyalty, referral and booking rules and membership plans for one branch or all of them from my phone,
So that every branch runs the same rewards without me visiting each one.

**Covers:** FR55 (rates per branch), FR56, FR42 (rules), FR154, AD-5, AD-8 (`schema_version`), AD-29, AD-39, UX-DR83, UX-DR90

**Acceptance Criteria:**

**Given** the command catalogue in `arena-proto`
**When** this story ships
**Then** `settings.set` accepts the loyalty, referral and booking keys from Stories 13.2, 13.5, 13.6, 13.11 and 13.13, and `plan.upsert` accepts `kind: membership`, each under a raised `schema_version`; no command kind exists for bookings, tournaments, café accounts or game requests

**Given** a branch whose heartbeat reports an older command `schema_version`
**When** one of these commands is posted
**Then** it is refused `BRANCH_UPDATE_REQUIRED` for that branch only

**Given** Manage → Pricing in the Owner app
**When** it opens
**Then** Plans & rates also lists memberships, and a new "Loyalty & bookings" tab shows each branch's current values with data age; Send opens the Story 8.21 preview with no MFA prompt

**Given** a command applied at the Hub (Story 8.19)
**When** a local manager changed the same setting earlier
**Then** the owner's value wins and the Hub shows "Updated by owner" naming the overwritten change

**Given** a Free branch
**When** the owner opens these tabs
**Then** they follow Story 8.12 and show only the upgrade path

### Story 13.29: Rollup schema v2 carries tournament income, deposits, loyalty and bookings

As a café owner on a paid tier,
I want my remote reports to show tournament income, deposits, rewards and bookings,
So that what I see on my phone matches the Hub's own reports line for line.

**Covers:** FR43 (tournament income in reports), FR42, FR55, FR139, AD-35 (versioned rollups, cloud accepts N and N−1, pin per tier), AD-8, AD-10 (`rollup_schema` licence claim), NFR7, NFR10, NFR25

**Acceptance Criteria:**

**Given** `arena-proto`
**When** rollup schema v2 is defined
**Then** it is v1 plus `revenue.by_type.tournaments` and `revenue.by_type.no_show_fees`, `deposits {taken, applied, refunded, kept}`, `promo_credit {loyalty, referral}`, `loyalty {points_earned, points_redeemed}`, `bookings {made, seated, no_show, cancelled}`, `memberships {sold, renewed}` and `game_requests [{game_key, name, count}]`, in integer minor units

**Given** the control plane
**When** this story ships
**Then** it gains a per-tier `rollup_schema` pin (default 1 for every tier) and the audited `arena-cloud ops rollup-pin --tier <t> --schema <n> --reason <text>`, and the heartbeat signs the pinned value into each branch's licence as the `rollup_schema` claim; a Hub whose heartbeat reports it cannot write the pinned schema keeps its highest writable schema in the claim

**Given** a Hub that knows v2
**When** its licence pins `rollup_schema = 1`
**Then** it emits v1 (it keeps writers for v1 and v2); when the pin rises to 2 it emits v2, and the Story 8.7 regeneration rebuilds past days oldest first at most 10 per minute; until a licence pins 2, v2 is built only in tests

**Given** the cloud release that knows v2
**When** it is deployed (cloud first, before any pin is raised)
**Then** the accepted set becomes {2, 1}; a rollup with any other schema counts as missing with `ROLLUP_SCHEMA_OLD`

**Given** the jobs role's daily metrics pass
**When** it runs
**Then** the "rollup missing-day count" metric is broken down by schema so a stuck fleet on an old schema is visible

**Given** Owner Reports → revenue (Story 8.15)
**When** a range includes v2 days
**Then** Tournaments and No-show fees show as their own rows, deposits and promotional credit show outside revenue, and v1 days show those rows as "—"

**Given** golden tests over a fixture with bookings, deposits, no-shows, tournaments with refunds, loyalty, referrals and memberships
**When** the v2 rollup is built
**Then** it equals the expected file exactly, and revenue, payments and cash equal the fixture's Z-reports for the same day

### Story 13.30: Bookings, tournaments, loyalty and the vault proven against their invariants

As a café owner,
I want these features tested under load and failure like the rest of the counter,
So that a busy tournament night or a WAN outage never double-books a PC, loses a deposit or leaks a password.

**Covers:** NFR1, NFR3, NFR4, NFR10, NFR12, NFR13, AD-2, AD-14

**Acceptance Criteria:**

**Given** the Story 1.8 `proptest` generator with every intent from this epic
**When** it runs on every push
**Then** no device ever has overlapping confirmed bookings, group bookings never exceed capacity, points and wallets never go negative, an account never has two active leases, deposits taken always equal applied + refunded + kept + held, and replaying the change log reproduces the database

**Given** a 60-device branch on reference hardware with 200 bookings and a 32-entrant tournament in one day
**When** windows open, no-shows fire and results are entered
**Then** every Reserved and free transition shows on the floor within 2 s, and `apply()` stays under 100 ms at the 5 intents/s sustained load

**Given** the WAN blocked for the whole day
**When** the same run executes
**Then** every step completes with no outbound connection, and queued renewal reminders send once after the WAN returns

**Given** a failover to the standby (Story 9.21 path) during an active café-account lease
**When** the new main Hub takes over
**Then** the lease and vault ciphertext are present, the account is not lent twice, and session end still releases it

**Given** a privacy scan after the full run
**When** it reads batches, snapshots, exports, key backups, the Owner API and all logs
**Then** no vault secret or player name, phone or email appears in plaintext

**Given** any target missed
**When** the suite finishes
**Then** the release is blocked and the report names the invariant or step

