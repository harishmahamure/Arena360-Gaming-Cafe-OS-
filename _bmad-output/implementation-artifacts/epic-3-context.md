# Epic 3 Context: Counter staff run the floor

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let counter staff run the whole café floor from the Hub: see every device live, manage players and their wallet, credit and plans, start, time, pause, extend, transfer, group and end sessions on PCs and consoles, apply dynamic pricing, and settle bills with mixed payments and a short Undo. Everything works offline on the LAN. It builds on Epic 1 (shell, `apply()`, inline PIN, audit) and Epic 2 (devices, groups, zones, rates, opening hours, kiosk unlock/lock from the test session).

## Stories

- Story 3.1: Branch billing rules — rounding, minimum, grace and time-up mode
- Story 3.2: Floor view shows every device as a live tile grouped by zone
- Story 3.3: Floor view performance proven on reference hardware
- Story 3.4: Collapsible zones with per-state counts
- Story 3.5: Non-modal tile sheet with remote PC actions
- Story 3.6: Create a player profile with protected personal data and consent
- Story 3.7: Player lookup by phone, Guest, and the Players screen
- Story 3.8: Edit a player profile and reset their password
- Story 3.9: Start a postpaid session for a player or a guest
- Story 3.10: The PC unlocks to a session screen when a session starts
- Story 3.11: End a postpaid session and see the bill to the minute
- Story 3.12: Settle a bill into payment records with reference rules
- Story 3.13: Settle bar and payment split at the counter
- Story 3.14: Undo for End and Settle
- Story 3.15: Pause and resume a session with a reason
- Story 3.16: Wallet top-up and paying from the wallet
- Story 3.17: Convert a guest to a profile, including at settle, and list sessions
- Story 3.18: Enable credit for a player and settle the remainder on credit
- Story 3.19: Collect credit with partial payments and Undo
- Story 3.20: Credit aging shown first, and auto-pause at 15+ days overdue
- Story 3.21: Time packs, blocks and passes per device group
- Story 3.22: Sell a pack or pass at the counter and see owned plans on the profile
- Story 3.23: Session timer engine with time warnings
- Story 3.24: Start a prepaid session on a pack or the wallet
- Story 3.25: Hub alerts with sound and a persistent badge
- Story 3.26: Time-up on PCs — grace, then clean up or staff handles
- Story 3.27: Pass and block sessions with fixed windows
- Story 3.28: Extend a session from the Hub, charged to wallet, pack or credit
- Story 3.29: Dynamic pricing rules by weekday hours and by date
- Story 3.30: The active rule prices new sessions and shows in the header
- Story 3.31: A rule change mid-session opens a new rate segment
- Story 3.32: Transfer a session to another device
- Story 3.33: Group sessions under one bill and one payer
- Story 3.34: Forced end with manager PIN and reason
- Story 3.35: Console sessions timed from the Hub, with warnings on the tile
- Story 3.36: Console time-up alert and controller check
- Story 3.37: Floor invariants, throughput and offline proof

## Requirements & Constraints

**Performance (reference hardware: 2015 i3 + HDD, 60 devices)**
- Floor state reaches every open view within 2 s via pushed events, never polling. Session start is under 10 s from first tap, settle under 10 s, profile create under 15 s. Kiosk unlock is within 2 s of commit.
- Hub UI stays under 250 MB and the Hub service under 50 MB. Throughput is 5 intents/s sustained and 50/s burst with apply p95 under 100 ms. A per-release benchmark fails the release on a miss.

**Consistency invariants (enforced in `apply()`, proven by proptest)**
- Wallet is never negative and equals the sum of its ledger deltas. Credit outstanding is never negative and never above the limit, except one recorded manager override per player per business day.
- At most one active, paused or grace session per device, backed by a unique partial index.
- A settled order's payments equal its total exactly. Every UPI payment carries a 4-digit reference and every card payment a terminal reference.
- The change log is gap-free and replay reproduces the rows.

**Offline**
- Every flow behaves identically with WAN down and makes no outbound call except the connectivity probe.
- A killed Hub loses no committed row, and it rebuilds the timer heap on restart.

**Billing**
- Money is integer minor units, and bills are tax-inclusive with no tax line.
- Rules snapshot at session start: rounding unit (default 15 min), minimum (30 min), grace (2 min) and time-up mode ("Clean up" or "Staff handles").
- Minutes per segment are the ceiling of unpaused time. The minimum applies to the first segment and rounding to the last. A rule change affects only sessions started afterwards.

**Privacy**
- Player name, phone and email are encrypted per player and never appear in change rows or logs, only the player id.
- Consent rows are append-only with policy version and timestamp.
- Passwords are Argon2id hashes only.

**Accessibility**
- Targets are at least 44 px, and the counter flow is fully keyboard-operable.
- Every state, badge and alert uses text or a glyph and never colour or sound alone. Zero serious or critical axe violations in dark and HC themes.

## Technical Decisions

**Intent model**
- Every mutation is an intent through `apply()` with a client idempotency key, `base_rev` on edits (`STALE` on conflict) and typed actors. Manager elevation is recorded as `elevated_by`.
- Each story adds its intents to the Story 1.8 proptest generator, including each `.reverted` twin.
- Errors are stable `AppError` codes; user text comes from the catalogue.

**Undo**
- Undo is a compensating `*.reverted` intent in an 8 s window, from the same staff and Hub window, carrying the forward `rev`. Otherwise it is rejected `UNDO_NOT_ALLOWED` or `STALE`.
- One shared irreversible-effect guard blocks Undo. Epic 3 records no effect, and Epic 5 registers `receipt.printed`, `drawer.kicked` and "invoice number assigned".
- Forced end needs the same PIN elevation to undo.

**Settle path**
- A bill is an `orders` row with `order_lines` (session, plan or top-up) and `payments`. Settle emits `bill.settled{order_id}`, the hook for Epic 5 receipts, tax, invoice numbering and shift binding. The outcome carries `settled_at` and `undo_until`.
- Epic 3 produces no receipt, no invoice number and no `SHIFT_NOT_OPEN` check.
- Credit collection exposes cleared spends and outstanding-after on the event.

**Data**
- New tables as stories land: `players`, `consents`, `player_keys` (keys outside the change log, wrapped by a branch KEK in the OS keystore), `phone_lookup` (keyed hash), `wallets`, `wallet_txns`, `credit_balances`, `sessions`, `session_segments`, `orders`, `order_lines`, `payments`, `plans`, `plan_groups`, `player_packs`, `pricing_rules`, `group_bills`.
- Sessions carry a billing-rules snapshot, `business_day`, `fy`, `epoch` and `rev`.
- Segments record the rate and `rule_id`. Guest numbers increment per business day.
- The lookup index is in-memory only, built from decrypted data, and answers within 100 ms for 3,000 players. Erasure filtering is deferred to Story 9.1.

**Timer engine**
- A min-heap of `ends_at` times ticks every 1 s with an injected `Clock`. It is rebuilt from active, paused and grace sessions at start.
- It ends sessions exactly once with `actor: system`, fires warnings at 10, 5 and 1 min (not persisted), and never blocks `apply()`.
- Pause shifts `ends_at`. Grace precedes time-up. Rule flips and wallet `ends_at` are recomputed from `next_change_at`.
- Ticks carry `lock_after_ms` and `time_up_mode`, so kiosks count down on a monotonic clock and lock locally while the Hub is down. Kiosks write no state and re-sync on reconnect.

**Pricing**
- `pricing::resolve(group, at_local)` returns exactly one rule (never stacking) plus `next_change_at`. Priority is higher priority, then date over weekly, then latest edit. `days_mask` bit 0 is Monday, and windows wrap midnight.

**Kiosk and remote actions**
- Session screen is minimal (device name plus time in tabular figures) with a reason line on lock. Epic 6 replaces it.
- `device.screen` intents (`pc.lock`, `pc.unlock`, `pc.message` up to 140 chars) are held by Counter, and restart, shut down and admin unlock need elevation.
- Remote results confirm within 5 s or show "Failed — retry", which resends with the same key.
- Console sessions are Hub-timed with no kiosk unlock and no remote PC actions.
- Plans never mix console and PC groups (`GROUP_KIND_MISMATCH`, `PLAN_NOT_FOR_GROUP`).

**Local UI state**
- Zone collapse state is stored per Hub PC and never goes through `apply()`.
- Alerts are held by the Hub service and persist across idle lock and reload.

## UX & Interaction Patterns

- Floor view is the home screen. It is grouped by zone with the Unassigned section last, at 6 columns at 1920 px, 4 beside the open sheet and 2 below 840 px.
- `floor-tile` has 8 states (free, in session, paused, reserved, order pending, maintenance, offline, unassigned). Console tiles add a controller glyph and "TV n".
- The tile sheet is non-modal and docked right. Focus moves in and Esc returns it to the tile. Live updates never move focus or reorder a focused tile.
- Every common action is at most 2 taps from the floor. Money actions are one tap plus an `undo-toast` with no confirm modals. Manager PIN is always inline (`inline-pin`). Modal depth is at most one.
- Keymap: Ctrl+1 Floor, Ctrl+3 Players, Ctrl+K search, Ctrl+Enter settle. Arrows and Enter move through tiles, and shortcuts never fire inside a text field.
- `player-lookup` shows outstanding credit first in warning text, with a Guest button and "No player with that number."
- `settle-bar` and `payment-split` offer cash, UPI, card, wallet and credit, never a QR. Settle stays disabled with "Remaining ₹x" until the remainder is zero.
- `bill-breakdown` has one line per rate segment.
- Alerts pair a bundled sound, repeated every 30 s, with a persistent `alert-badge` on the tile, header and collapsed zone row until acknowledged.
- Refusals keep the user in place with the plain reason inline (for example "Balance ₹40. Top up at counter.").

## Cross-Story Dependencies

- Chain: 3.1 (rules) feeds 3.9 snapshots. 3.2 to 3.5 give the floor, then the sheet. 3.6 to 3.8 give players. 3.9 to 3.11 give start, kiosk unlock and end. 3.12 and 3.13 settle, then 3.14 Undo.
- Wallet (3.16), credit (3.18 to 3.20) and plans (3.21 and 3.22) extend settle and `payment-split`.
- 3.23 timer engine precedes 3.24, 3.26, 3.27, 3.28 and 3.31. 3.25 alerts precede 3.26 and 3.36. 3.29 resolver precedes 3.30 and 3.31, and 3.30 switches Extend (3.28) to the resolver.
- 3.17 conversion needs 3.6 and 3.13. 3.33 groups and 3.34 forced end build on 3.14 and 3.24. 3.35 and 3.36 reuse 3.21 to 3.27 for consoles. 3.37 proves everything last.
- 3.5 uses Story 1.20 admin unlock. 3.9 enforces `DEVICE_UNASSIGNED` from 2.20 and 2.18. 3.10 reuses the Story 2.29 unlock/lock mechanism. 3.25 absorbs the Story 2.12 header status-badge slot. 3.1 and 3.28 read the Story 2.26 opening hours.
- Hand-offs: Epic 5 (receipts, invoice numbering, shift, tax, tile Order action), Epic 6 (player shell and kiosk Extend), Epic 8 (credit reminders, console 5-min message), Epic 9 (Story 9.1 erasure filter), Epic 12 (Pi/QR), Epic 13 (reserved state data).
