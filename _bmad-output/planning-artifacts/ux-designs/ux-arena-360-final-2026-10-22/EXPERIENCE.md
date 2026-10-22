---
name: Arena 360 Experience
status: draft
updated: 2026-10-23
sources:
  - docs/Arena 360 - Product.md
  - docs/Arena360 — User Journeys.md
---

# Arena 360 — Experience Spine

`DESIGN.md` owns how it looks; this spine owns how it works. Token references use `{path.to.token}` and resolve in `DESIGN.md`. Requirements, personas, tiers and acceptance criteria live in the sources and are referenced, not restated. **The spines win on conflict with any mock, wireframe or import** (`.working/`, `imports/`). Currency is shown as ₹ in examples and is locale-formatted in the product.

## Foundation

This is a multi-surface product: one brand in two moods (see `DESIGN.md` Brand & Style). All app surfaces share **one web UI + design-token codebase**. Kiosk and Windows Hub ship it inside **Tauri** (WebView2). The Android Hub ships it inside **Tauri 2 mobile**. Dark-only, plus a high-contrast theme. English only.

| Surface | Form factor / platform | Mood | Primary user |
|---|---|---|---|
| Kiosk | Tauri desktop app (WebView2); shell replacement on each gaming PC; Windows 10–11; fixed 1920×1080 landscape | Player | Player |
| Overlay HUD | Always-on-top Tauri window over games, same PC | Player | Player |
| Hub | Tauri on Windows 10+ desktop (paid tiers). Tauri 2 mobile on Android tablet **and** phone (the only Hub on Free; also allowed on paid tiers). Same tile grid on all; on phone the rail becomes a bottom bar | Staff | Counter staff, manager |
| Kitchen display | Browser page on any LAN tablet | Staff | Kitchen staff |
| Owner app | One responsive web app, installable as a PWA (phone + desktop), with web push for alerts | Staff | Owner |
| Onboarding wizard | Web, resumable, 10 steps | Staff | Owner |
| Partner portal | Separate web app with its own login (a partner is not a café owner) | Staff | Partner / installer |
| Marketing site | Static web | Player | Prospective owner, partner |
| Pi TV idle/lock + phone QR start page | TV via Pi; the player's phone browser. **v1.1** | Player | Player |
| Receipts, prints & messages | Thermal print, WhatsApp/SMS, web push | Formal | Player, owner, kitchen |

Two principles from the sources frame every player state. **"The kiosk only asks; the Hub decides."** **"The Kiosk owns the PC between sessions and stays out of the way during one."** Staff screens follow the PRD's simplicity posture: owners compose roles from a fixed catalogue, and inventory is deliberately simple.

## Information Architecture

| Surface | Structure | Reached from |
|---|---|---|
| **Kiosk** | **Flow:** Attract → Login → Plan picker → Waiting for Hub → **Main shell**. Session end: `lock-screen` → per-branch time-up mode → Attract.<br>**Login:** `login-panel` with username + password. Alternative: staff starts the session from the Hub. Also "Create account" → `signup-form`, and "Forgot password?" → "Ask staff to reset it".<br>**Main shell, top bar:** `logo-slot`; `pill-nav` with Home · Games · Apps · Shop (Shop on Basic+ only); right cluster of `reconnecting-indicator` (when shown), `volume-control`, `messages-bell`, `call-staff-button`, `time-chip`, account chip.<br>**Home:** video `hero` (Play now · Game details) + `video-control`; featured `game-tile` row ending in `all-games-tile`.<br>**Games:** `launcher-card` row, `most-played-carousel`, `filter-rail` + search + grid.<br>**Apps:** same structure as Games.<br>**Shop:** food & drink only; `shop-item-card` grid → `cart` → checkout.<br>**`account-menu`:** balance, Extend, Messages, Account, PC settings, Lock, Log out. **Account page:** `account-subnav` with Details · Wallet & packs · Order history · Change password.<br>**In-game:** Overlay HUD. **Admin exit:** manager PIN or Hub remote unlock → desktop | Power-on / session end |
| **Hub** | **Navigation:** persistent `hub-rail`: **Floor view (home)** · POS · Players · Sessions · Inventory · Shift · Reports · Settings · System. On phone, `hub-bottom-bar`: Floor · POS · Players · Shift · More (Inventory, Reports, Settings, System). [NOTE FOR UX] Where Sessions lives on phone is not stated (assumed under More).<br>**Floor view:** tiles grouped by zone; zones collapsible.<br>**Floor tile** → `tile-sheet`: start · extend · order · transfer · pause/resume · lock · end.<br>**POS:** `pos-product-grid` · `cart` · `settle-bar`.<br>**Players:** `player-lookup` → profile (outstanding first; credit enable; collect; reset password).<br>**Shift:** open with float · close (`denomination-counter`, `variance-readout`) · Z-report.<br>**Settings** includes the layout editor, time-up mode, high-contrast per device, receipts/printer, opening hours and ads consent (Free).<br>**System:** backup, standby promote, audit, diagnostics | App launch / idle-lock unlock |
| **Kitchen display** | Single screen, four columns: New → Preparing → Ready → Served | LAN URL |
| **Owner app** | **Navigation:** `owner-nav`: Today · Reports · Alerts · Manage · Account (bottom tab bar on phone, left sidebar on desktop). Live view sits in the sidebar, locked below Premium. [NOTE FOR UX] Live view's entry point on phone is not decided.<br>**Today:** per branch, as `metric-figure`s: open/closed, sessions live, revenue so far, cash in drawer, cash variance (counted − expected) [ASSUMPTION — confirm at mock walkthrough], staff on shift, last sync time; then a "Needs attention" list.<br>**Reports:** Revenue · Cash reconciliation · Stock · Credit · Audit · Exports.<br>**Other:** Alerts · Manage (`command-row` list; "Waiting for branch") · Live view (Premium) · Account (MFA, Subscription, Branches).<br>**On Free:** Subscription, pairing and support only; everything else shows `tier-lock` | PWA / web login; web push |
| **Onboarding wizard** | `wizard-progress` beside the step content. 10 steps per the PRD: tenant → tier → install Hub (`pairing-code`) → devices → plans → products → staff → printer & payments → test session → go live | Marketing "Get started" |
| **Marketing site** | **MVP:** Home · Product · Pricing (`tab-control`: Cafés · Partners) · Get started · Downloads · Help · FAQ · Legal.<br>**v1.1:** Demo · Compare · Status · Partners | Public |
| **Pi / QR (v1.1)** | TV idle · TV lock · phone QR start page | TV / QR scan |
| **Partner portal** | Referred cafés · Payouts · Referral link | Partner login |

Modal depth is one level, never stacked. Kiosk Shop is food & drink only; time packs and Extend live in the account menu and HUD.

→ Structure references:
- [imports/ggleap-02-home.png](imports/ggleap-02-home.png): hero, featured row, pill nav, time chip.
- [imports/ggleap-03-account-menu.png](imports/ggleap-03-account-menu.png): account menu, minus the gamification balances.
- [imports/ggleap-04-games-library.png](imports/ggleap-04-games-library.png): launcher cards and the most-played carousel.
- [imports/ggleap-05-apps.png](imports/ggleap-05-apps.png): apps grid.
- [imports/ggleap-06-shop.png](imports/ggleap-06-shop.png): shop and cart.
- [imports/ggleap-07-account.png](imports/ggleap-07-account.png): account sub-nav.
- [.working/direction-cinema.html](.working/direction-cinema.html): Hub floor view and HUD composition. Layout only; its palette and glass are superseded (see `DESIGN.md`).

## Voice and Tone

Brand posture lives in `DESIGN.md`. There are three registers:

| Register | Where | Do | Don't |
|---|---|---|---|
| Player: casual, gamer-friendly | Kiosk, HUD, Pi/QR, marketing | "10 min left. Extend?" · "More time added. GG." · "Order's ready — grab it at the counter." | "Your session will terminate in 600 seconds." |
| Staff: plain, neutral, no blame | Hub, Kitchen, Owner app, Onboarding, Partner portal | "Counted ₹4,850. Expected ₹4,900. Difference −₹50 short." · "Waiting for branch" · "Printer offline" | "Cash shortage detected!" · "You entered the wrong amount" |
| Money / legal: formal | Receipts, GST invoices, terms, subscription, payouts | "Invoice No. 000123 · GST 18% included" | Slang, emoji, abbreviations |

Refusals state the fact and the next step. Error copy on the Hub never names a person. Fixed player-facing lines:

| Situation | Copy |
|---|---|
| Login failure | "Username or password didn't match." (never says which) |
| Login locked | "Too many tries. Try again in 0:60." |
| Forgot password | "Ask staff at the counter to reset it." |
| Sign-up duplicate | "That username's taken." · "That phone number already has an account — ask staff if it's yours." |
| Insufficient balance | "Balance ₹40. Top up at counter." |
| Credit limit | "Credit limit reached, see counter." |
| Low balance | "Balance running low — ₹40 left." |
| Extend past closing | "Can't extend past closing time (11:00 pm)." |
| Blocked app | "That app isn't allowed on this PC." |
| Forced end | "Your session was ended by staff. Please see the counter." |
| Call staff | "Staff notified." |
| Hub unreachable | "Reconnecting…" |
| Hub unreachable, new login / order / Extend | "Counter is reconnecting — try again shortly." |
| Tier-gated (staff) | "Available on Basic" · "Upgrade" |

## Component Patterns

These rows are behavioral. Visual specs live in `DESIGN.md` Components. Names are identical across both spines.

| Component | Use | Behavioral rules |
|---|---|---|
| `focus-ring` | Every focusable element, every surface | Shown on keyboard focus, never suppressed. Focus order follows reading order. Sticky bars and open sheets never cover the focused element. |
| `button-primary` | All | One per view. Disabled state shows why, inline (never a silent disabled). |
| `button-secondary` / `button-warning` | All / HUD | Secondary for non-primary actions (Dismiss, Cancel, End, Lock). Warning only for Extend in `hud-toast` and `extend-options`. |
| `text-field` | Login, sign-up, search, lookup, PIN reason, cash count, payment refs, onboarding | Label above, always programmatically linked. Validation runs on submit and on blur. On failure, focus moves to the first invalid field and the error is announced. Password variant has show/hide. Paste allowed except on the kiosk password field [NOTE FOR UX: confirm paste policy on kiosk]. |
| `scrim` / `tile-scrim` | All media-backed surfaces | Always rendered when media is present. Removed with media in high-contrast. |
| `video-control` | Kiosk login, Kiosk Home hero | Always visible while video plays. Keyboard-reachable. Pause holds the current frame for the rest of the login or session view. |
| `login-panel` | Kiosk login | Username + password; Enter submits. 3 failures lock login for 60 s with a visible countdown, and the Hub is notified. Non-memory alternative: **staff starts from Hub** (satisfies WCAG 3.3.8 by alternative). QR-from-Hub login is dropped for now. While the Hub is unreachable, submit is disabled with "Counter is reconnecting — try again shortly." "Forgot password?" shows the ask-staff line; staff reset it from the Players profile. No language picker. |
| `signup-form` | Kiosk "Create account" | Fields: username, password, phone, name. **No OTP.** A consent checkbox for WhatsApp/SMS is stored per player. It gates WhatsApp receipts and credit reminders and is unticked by default. Duplicates are blocked by username **and** by phone, with the copy in Voice. On success the profile is created on the Hub and the player continues to the plan picker. |
| `pill-nav` / `pill-nav-item-active` | Kiosk shell | Home · Games · Apps · Shop. Shop is hidden on Free (player surfaces hide gated features). Exactly one item is active. Arrow keys move; selection persists per session and resets on logout. |
| `time-chip` | Kiosk top bar | Always visible while logged in. Counts down (prepaid) or up (postpaid). At 10 min: warning dot, border and glyph. Click → `account-menu`. |
| `reconnecting-indicator` | Kiosk top bar | Appears only while the Hub is unreachable. The running session keeps going on the last known time; the player is never locked out. New login, Shop orders and Extend are disabled meanwhile, each saying "Counter is reconnecting — try again shortly." Disappears on reconnect once the session has reconciled. |
| `volume-control` | Kiosk top bar | Sets Windows system volume for this session. [NOTE FOR UX] Whether volume resets between players is not decided. |
| `messages-bell` | Kiosk top bar | Lists staff broadcast and per-seat messages and order-status updates for this session. Opening the bell marks them read. The list clears at session end. |
| `call-staff-button` | Kiosk top bar | One tap pings the Hub: an `alert-badge` "CALL" on that tile plus the alert sound. The kiosk confirms "Staff notified". |
| `account-menu` | Kiosk | Balance, Extend (opens `extend-options`), Messages, Account, PC settings, Lock, Log out. Log out confirms only if time remains on a prepaid plan. [NOTE FOR UX] Contents of PC settings are not defined. |
| `account-subnav` | Kiosk Account page | Details · Wallet & packs (balance, packs with hours left and expiry) · Order history (this branch) · Change password (current + new). Single-select; arrow keys move. |
| `hero` | Kiosk Home, attract, login | Plays muted game video for the featured title and follows the media fallback chain. Stops entirely when a game launches. Play now launches the title. **Game details** (secondary) opens a panel with genre, age rating and launcher; Esc closes it and returns focus. |
| `game-tile` / `game-tile-featured` | Home row, Games/Apps grid | Focus = `focus-ring`; activate launches (Games) or opens (Apps). The featured tile drives the hero media. Being featured is not focus. |
| `all-games-tile` | End of Home featured row | Activates Games with no filter. |
| `most-played-carousel` | Games tab | The branch's **top 10 games by session hours over the last 7 days**, ranked. Branch-level, not per player; not gamification. Arrow keys scroll. |
| `launcher-card` | Games tab | Opens Steam / Epic / Riot for the player's **own** account. Logout is enforced at session end. |
| `filter-rail` | Games, Apps, Shop | Single-select category + search. Filters apply instantly (no Apply button). An empty result shows an underlined "Clear filters" link. |
| `shop-item-card` | Kiosk Shop, Hub POS | Add to `cart`; stock checked live against the Hub. Out of stock is disabled with "Out of stock". Bundles show their components and refuse if any is short (override only at POS). |
| `popular-badge` | Kiosk Shop | Marks the branch's **top 3 shop items by units sold over the last 7 days**. |
| `cart` | Kiosk Shop checkout; Hub POS | **Kiosk:** ±1 steppers; live stock check on each change; "Place order" attaches to the running session (shows "ordered" on the HUD). The player can instead pay now by wallet. With no session: "Pay at counter". **Hub:** see `pos-product-grid` / `settle-bar`; a cart can attach to a running session. Empty cart: "Cart's empty." |
| `card` | Balances, account sections | Generic container; no behavior of its own. |
| `plan-picker` | Kiosk after login | Shows only plans valid for this device's group. Packs show hours left and expiry; expired packs are greyed and not selectable. The wallet option shows balance and hourly rate, plus any active dynamic pricing rule. Selecting sends a request; the kiosk shows "Waiting for the counter…" until the Hub approves or refuses with a typed reason. |
| `extend-options` | Expanded HUD; account menu; grace `lock-screen` | Opened from the `hud-toast` Extend, the **global Extend hotkey Ctrl+Shift+E** (opens the expanded HUD with focus on the first option), account menu → Extend, or Extend on the grace countdown. Disabled while the Hub is unreachable ("Counter is reconnecting — try again shortly."). Options are priced live by the Hub and charged to wallet, pack or credit. Refusals: past closing time, credit limit, insufficient balance. Each uses Voice copy and the panel stays open. |
| `hud-pill` | Overlay HUD | Time left/elapsed + balance; hotkey expands. Never takes focus on its own. **Hidden in fullscreen-exclusive games**; warnings then arrive as `hud-toast` + sound (PRD), and at 5 and 1 min the kiosk briefly **forces a visible warning** (minimise/flash, or a lock-screen-style banner) so warnings are never sound-only. [NOTE FOR UX] Which mechanism ships depends on testing against exclusive-mode titles. |
| `hud-toast` | Warnings, low balance, order status, staff messages, forced end | Appears top-right with sound and never steals focus. **Warning toasts (10 / 5 / 1 min, low balance) persist until dismissed or time-up.** Info toasts (order status, messages) auto-dismiss after 8 s and are kept in `messages-bell`. Each toast shows the Extend hotkey (Ctrl+Shift+E). Warning timings are configurable per branch (PRD). |
| `lock-screen` | Time-up, pay at counter, forced end | Grace countdown (default 2 min) runs over the still-visible game, with **Extend** available alongside the countdown (opens `extend-options`; a successful Extend dismisses the lock). After grace, the **per-branch time-up mode** applies: (a) kill allow-listed processes, clean up, return to attract; or (b) keep the lock screen over the paused game for staff to handle. Postpaid end shows "Pay at counter". Forced end shows the forced-end line. |
| `undo-toast` | Hub (End, Settle, Collect), Kitchen (stage advance) | Money actions run **in one tap, with no confirm modal**, then show Undo for 5–10 s. Undo restores the prior state exactly. The toast never blocks the floor. A new money action replaces the toast and commits the previous one. The receipt prints at once if a printer is available. Undo after a receipt has printed **voids that invoice with an audit entry** [ASSUMPTION — confirm]. [NOTE FOR UX] Whether kiosk cleanup waits for the Undo window is not decided. |
| `alert-badge` | Hub header, floor tiles, kitchen tickets | The visual twin of every alert sound. It persists until acknowledged by opening the item it points to. The header shows the count, and opening the header badge lists the alerts. |
| `hub-rail` / `hub-bottom-bar` | Hub | Floor view is home and the default after unlock. Items are always labelled. On phone the bottom bar replaces the rail: Floor · POS · Players · Shift · More; More opens a list of Inventory, Reports, Settings, System. |
| `hub-header` | Hub | Always shows: branch, active `chip-rule`, connectivity (`chip-offline`), cloud backup status (paid: last upload, pending batches), `alert-badge` count, clock, signed-in staff. Tapping backup status opens System. |
| `floor-tile` | Hub floor view | Tiles group by zone; each zone header can collapse its zone (see State Patterns, High device count). Tap or Enter → `tile-sheet`. Updates ≤2 s from source events. Live updates never move focus or reorder a focused tile. Shows the `status-badge`, time, player, `tile-order-badge` and `alert-badge`. |
| `floor-tile-console` | Hub floor view (consoles) | Same as `floor-tile`. Sessions are started and ended **from the Hub only**, and the timer runs on the Hub. Warnings show on the tile. Time-up raises `alert-badge` "TIME UP" + sound until staff walk over and end it. The audit records who ended it. |
| `tile-order-badge` | Floor tile | "ORDER" while the kitchen is working; "READY" when ready for runner delivery. Tapping the tile opens the sheet at the order section. |
| `tile-sheet` | Hub | Start / extend / order / transfer / pause-resume / lock / end; each is one tap. Pause asks for a reason from a list; paused time isn't billed. Transfer: pick a target tile; the old device locks and the new one unlocks within 5 s. Shows `bill-breakdown`. Non-modal: the floor stays live behind it. On open, focus moves into the sheet; Esc closes it and returns focus to the tile. |
| `inline-pin` | Manager-gated actions (forced end, manual discount, refund/void, variance over threshold, credit override, promote standby, kiosk admin exit) | Renders in place inside the current sheet, row or kiosk panel. Numeric keypad + keyboard. Never navigates away. A reason picker appears in the same place when required. |
| `player-lookup` | Hub floor sheet, POS, Players | One phone field; results as you type; Enter picks the top result. "Guest" is one tap/key away. Credit outstanding is shown first on the profile. No match: "No player with that number." with **Create profile** (≤15 s) and **Guest**. |
| `pos-product-grid` | Hub POS | Categories and favourites first. A barcode scan (keyboard wedge) adds the item: 1 scan + 1 tap to sell. Out-of-stock and expired items are disabled. A short bundle refuses with an `inline-pin` override. |
| `settle-bar` | Hub POS, session sheet | Shows the total and the payment methods: cash · UPI · card · wallet · credit. Settle is disabled until the remainder is zero or moved to credit; the reason is shown inline. Coupon and manual discount (PIN + reason) apply here. Receipt choice: print · WhatsApp (only with consent) · none. Print happens only if a printer is available; with none, nothing prints and Settle is never blocked. Settle → `undo-toast`. |
| `payment-split` | Settle | Add a method, enter an amount, add more. The remainder recomputes live. **UPI:** no dynamic QR and no webhook; staff record the payment by entering the **last 4 digits of the transaction reference** (required — the UPI row can't be saved without it). **Card:** external terminal; enter the reference. **Credit:** only for a profiled player with credit enabled; blocked at the limit with an `inline-pin` override once per player per day. A guest can be converted to a profile here. |
| `bill-breakdown` | Session sheet, POS | Lines in order: session (device, minutes × rate after rounding unit and minimum; paused time excluded; one line per rate segment after a transfer or dynamic pricing rule), orders, coupon, discount, subtotal, GST (inclusive/exclusive per branch), total, paid so far, remaining. A group session splits by device minutes, and the payer can be changed before settlement. |
| `denomination-counter` | Shift close | One count field per denomination with a live total; or a "Single total" toggle. Expected cash is **never shown until counted cash is entered.** |
| `variance-readout` | Shift close | Shows counted, expected and the signed difference. "Breakdown" is one tap away and lists opening float, cash sales, cash refunds, deposits and expenses. Within threshold (default ₹100), close completes with no PIN. Over threshold: `inline-pin` + a reason from a list + free text, in place. Close → Z-report prints with one tap. **Difference = counted − expected**: + means extra cash ("over"), − means short ("short"). |
| `chip-rule` | Hub header, kiosk attract, plan picker | Shows the active dynamic pricing rule while active. The change is reflected within 1 min. |
| `chip-offline` | Hub header | Appears when internet or cloud is unreachable and names which. Nothing else in the UI changes. |
| `status-badge` | Floor tiles, Owner commands, kitchen | Always colour + text. Owner command states: pending ("Waiting for branch" + time queued) → applied / rejected (reason + Retry) / expired (Resend). Changes are announced politely on web. |
| `tier-lock` | Hub, Owner app, Onboarding (staff/owner only) | A gated feature or limit (Kitchen on Free, Owner dashboards on Free, Live view below Premium, device limit 20/60/200, Free's 3 users) shows locked with "Available on <tier>" + Upgrade, which leads to Owner app Subscription. **Player surfaces hide gated features entirely.** |
| `kitchen-ticket` | Kitchen display | New tickets appear at the top with elapsed time, a chime and the "NEW" `alert-badge` (held until advanced). Tap advances one stage → `undo-toast`. Cancel after preparing needs a reason and lands in the waste report. |
| `owner-nav` | Owner app | Today · Reports · Alerts · Manage · Account. Bottom tab bar on phone, left sidebar on desktop; one destination active; Today is home. Live view is in the sidebar, `tier-lock` below Premium. |
| `tab-control` | Marketing Pricing (Cafés · Partners) | Switches peer panels in place without a page load; one tab selected. Arrow keys move between tabs; Tab moves into the panel (ARIA tabs). [ASSUMPTION] The selected tab is reflected in the URL so it can be linked. |
| `wizard-progress` | Onboarding wizard | Lists all 10 steps with done / current / upcoming state and "Can skip" tags on Products, Staff, and Printer & payments. The current step is `aria-current="step"`; on self-advance focus moves to the new step heading. [ASSUMPTION] Done steps link back for edits, upcoming steps open only in order, and resuming lands on the first incomplete step. |
| `metric-figure` | Owner app | Every figure carries its data age. Loads from the mirror in under 2 s. Never shows a broken state when the branch is offline; it just ages. |
| `command-row` | Owner app Manage | One row per target branch. Status per `status-badge`. Expires after 7 days → Resend. |
| `pairing-code` | Onboarding step 3 | 8-character single-use code with QR, 15-min countdown (announced at milestones, not every tick). The page advances by itself on the first Hub heartbeat and moves focus to the new step heading. Expired → "Generate a new code". |
| `logo-slot` | Kiosk, Pi | Branch logo only; falls back to the Arena 360 mark. |
| `ad-banner` | Free-tier Android Hub | See Ads. Never covers the floor grid, cart or settle bar. Never on the kiosk in Phase 1. |

## State Patterns

**Floor tile states** (Hub; colour + label, `{colors.state-*}`):

| State | Token | Tile shows | Tap opens |
|---|---|---|---|
| Free | `{colors.state-free}` | "Free", group rate | Start (postpaid / prepaid / guest) |
| In session | `{colors.state-in-session}` | Time left or elapsed, player | Extend · order · transfer · pause · lock · end |
| Paused | `{colors.state-paused}` | In-session colour dimmed, pause icon, "Paused", frozen timer | Resume · end |
| Reserved | `{colors.state-reserved}` | "Reserved", for whom and when. Bookings (v1.1) show 15 min before the slot | Start for reservation / release |
| Order pending | `{colors.state-order-pending}` | `tile-order-badge` over the session state ("ORDER" / "READY") | Order section / mark served |
| Maintenance | `{colors.state-maintenance}` | "Maintenance" | Return to service |
| Offline | `{colors.state-offline}` | "Offline", last seen | Device diagnostics / remote actions |
| Unassigned | `{colors.state-unassigned}` | Dashed outline, "Unassigned", PC name | Assign to group / zone |

Consoles use the same states on `floor-tile-console`.

| State | Surface | Treatment |
|---|---|---|
| Cloud / internet offline | Hub | `chip-offline` in the header; **nothing else changes**. Every branch action keeps working. |
| Hub unreachable | Kiosk | Running sessions keep going on the last known time locally. `reconnecting-indicator` shows; the player is never locked out. New login, Shop orders and Extend are disabled with "Counter is reconnecting — try again shortly." The kiosk reconciles with the Hub on reconnect (Journey 8). |
| High device count | Hub floor view | The airy spacing shows fewer tiles per screen (accepted tradeoff). The floor scrolls vertically, and each zone header can collapse its zone to one row showing the zone name, tile count, counts per state, and the zone's `alert-badge` count, so a collapsed zone never hides an alert. Collapse state is remembered per device. [ASSUMPTION] Zones start expanded; collapsing is manual (no auto-collapse threshold). |
| No printer | Hub | Settle, Z-report and KOT complete without printing; no blocking error. |
| Waiting for Hub approval | Kiosk plan picker | "Waiting for the counter…" with the selected plan; returns to the picker with a typed reason on refusal. |
| Data age / pending command | Owner app | Each figure shows data age. Remote changes show "Waiting for branch" with time queued. Expired after 7 days → "Expired" + Resend. |
| Media | Kiosk, Pi, marketing | Video → still cover (video missing, unapproved, or the PC can't play it) → solid `{colors.bg}`. Never a broken player. High-contrast skips straight to solid. |
| Cold load | Kiosk | The attract screen renders from cached branding and rates; media streams in after. |
| Cold load | Hub floor view | Tiles render from the local DB immediately. No skeletons (local-first). |
| Empty | Games/Apps grid | "No games here yet — ask at the counter." |
| Empty | Kiosk Shop | "Kitchen's closed right now." when the menu is empty or unavailable. |
| Empty | Kiosk / POS cart | "Cart's empty." |
| Empty | Kitchen display column | Column header only; no filler copy. |
| Empty | Owner Alerts | "All quiet." |
| Empty | Partner portal | "No referred cafés yet." + referral link. |
| No match | Hub `player-lookup` | "No player with that number." + Create profile / Guest. |
| Refusal | Kiosk plan picker | Balance + "Top up at counter"; stays on the picker. |
| Refusal | Extend | Past closing time / credit limit / insufficient balance: Voice copy; options stay open. |
| Credit block | Kiosk / Hub | "Credit limit reached, see counter." The Hub offers an `inline-pin` override, once per player per day. |
| Sign-up duplicate | Kiosk | Field-level error on username or phone, using the Voice copy. |
| Login lock | Kiosk | 60 s countdown on the panel after 3 failures; the Hub is notified with an `alert-badge`. |
| Blocked app | Kiosk | `hud-toast` "That app isn't allowed on this PC."; logged on the Hub. |
| Forced end | Kiosk | `lock-screen` with the forced-end line. |
| Low balance | Kiosk HUD | Persistent warning `hud-toast` with Extend / top-up guidance. |
| Remote action pending / failed | Hub | Result shown within 5 s, otherwise marked "Failed — retry" on the tile. |
| Printer offline | Hub | `alert-badge` + red badge on the Hub. The kitchen display still receives the order. |
| Low stock / expiring | Hub | `alert-badge` in the header; the item is listed in Inventory. Expired items are excluded from sale. |
| Idle lock | Hub | After 5 min idle (default), PIN unlock in place; returns to the same screen. |
| Error (save/apply) | Any | Inline, next to the control: plain cause + retry. Never a blocking modal during a sale. |
| Tier-gated | Staff / owner | `tier-lock`. Player surfaces hide the feature. |
| Not permitted (role) | Hub | [NOTE FOR UX] Treatment for an action the role lacks and that is not PIN-elevatable is not designed. |
| LAN loss | Kitchen display | [NOTE FOR UX] Not designed. |
| No network | Owner PWA | [NOTE FOR UX] Not designed beyond data age. |

## Interaction Primitives

- **Hub speed rules.** Every common action is one tap from floor view, and no action takes more than two taps. Start and settle each finish under 10 s from the first tap.
- **No modal blocks a sale.** Sheets are non-modal; the floor view stays live and tappable behind them.
- **Money actions: one tap + Undo.** End, Settle, Collect and the kitchen stage advance execute immediately and show `undo-toast` for 5–10 s. There is never a confirm modal.
- **Manager PIN is inline.** Entry never leaves the current screen.
- **Keyboard.**
  - The counter flow can be completed without a mouse.
  - Floor grid: arrow keys move between tiles and Enter opens the sheet; Esc closes it and returns focus to the tile.
  - Shortcuts never fire while a text field has focus.
  - The kiosk has a **global Extend hotkey, Ctrl+Shift+E**, that opens the expanded HUD with focus, and HUD toasts print it.
  - [NOTE FOR UX] The POS key map, the rest of the kiosk key map (nav, search, account menu, return-to-game) and the kiosk admin-exit trigger are not defined.
- **Sound + visual.**
  - The Hub plays a sound on alerts and on new orders. The Kitchen display chimes on each new order. HUD warnings use toast + sound.
  - **Every sound has a persistent `alert-badge` twin** until acknowledged.
  - Hub alerts: new kiosk order, order ready, call staff, console time-up, kiosk login lockout, printer offline, low stock, expiring stock.
  - Background videos are **always muted**.
- **Video lifecycle.** Background video plays on kiosk login, the kiosk Home hero and Pi idle TV. It stops entirely while a game runs. The kiosk has a visible pause/play `video-control`.
- **HUD never steals focus** from the game on any event. Focus moves to it only when the player presses the hotkey.
- **Motion.** Short, subtle transitions only; cut to instant in high-contrast. OS reduced-motion is **not** honoured separately (user decision).
- **Banned:** blocking modals during a sale, confirm modals on money actions, stacked modals, live blur, autoplay audio, gamification (coins, quests, prize vault, social feed), ads on any kiosk screen in Phase 1.

## Accessibility Floor

Visual contrast and token ratios live in `DESIGN.md`.

- **Targets.** ≥ `{spacing.touch-min}` (44px) on every surface, including the Hub on an Android phone (two tile columns via `hub-bottom-bar`). Airy spacing never shrinks a target.
- **Focus.** `focus-ring` on every focusable element, distinct from the featured-tile outline. In HC it is the thicker yellow ring.
- **Boundaries.** Inputs, PIN cells and focusable edges use `{colors.border-strong}` (≥3:1).
- **Links** are always underlined.
- **High-contrast theme** (`*-hc` tokens) is a **branch/device setting**: an owner or manager toggles it per device from Hub settings. There is no player toggle. HC removes art, video and translucency, adds 2px outlines, and cuts motion. It applies to Kiosk, Hub, Kitchen and Owner surfaces.
- **Text over media.**
  - Always sits on `{components.scrim}` inside its copy zone, verified against a white frame.
  - Never in accent.
  - Game-tile labels sit on `{components.tile-scrim}`.
- **Colour is never the only cue.** Floor tiles, badges, tickets, the time-chip warning and field errors always carry text or a glyph.
- **Sound is never the only cue.** Every alert sound has a persistent `alert-badge` (Hub, Kitchen).
- **Moving media.**
  - Visible pause/play on kiosk login and Home.
  - Only curated control-plane library videos, flash-screened before publishing, autoplay.
  - Owner local uploads show as a still cover unless approved.
  - Marketing autoplay is not restricted (user decision).
- **Timed warnings.**
  - Warning toasts persist until dismissed or time-up.
  - Extend is reachable by hotkey and from the account menu, not only from the toast.
  - **Fullscreen-exclusive games:** per PRD, `hud-pill` hides and warnings arrive as `hud-toast` + sound. At 5 and 1 min the kiosk also briefly forces a visible warning (minimise/flash or a lock-screen-style banner), so no warning is sound-only.
  - Extend stays available during the 2-min grace, on the lock screen.
- **Authentication (WCAG 3.3.8).**
  - The kiosk password login has a non-memory alternative: staff starts the session from the Hub.
  - Show-password is available, and the error copy never reveals which field failed.
- **Web screen-reader baseline** (Owner app, onboarding, partner portal, marketing, Kitchen, Hub web UI):
  - Landmarks (`header`, `nav`, `main`); one `h1` per view.
  - Every control programmatically labelled.
  - Polite live regions for `status-badge` changes, `chip-offline`, `alert-badge` counts, `hud-toast` and new kitchen tickets.
  - Countdowns announced at milestones, not every tick.
  - Self-advancing wizard steps move focus to the new heading.
- **Numbers.** Timers use tabular numerals so counting text doesn't shift.

## Key Flows

Step-level "what must be true" lives in the journeys source. The flows below are the experience path.

### Journey 1 — Owner onboarding (Vikram, owner of two Pune cafés, moving off ggLeap)

1. Vikram taps Get started on the marketing site: name, phone, email, café name, city. Phone OTP.
2. The wizard creates the tenant and branch, and he picks a tier (Free needs no card).
3. The wizard shows the `pairing-code`: 8 characters, large, with a QR and a 15-min countdown.
4. On the counter PC he installs the Hub; the installer asks only for the code.
5. The Hub pairs, and the wizard advances by itself on the first heartbeat.
6. On each gaming PC the kiosk finds the Hub and asks for the 6-digit code shown on the Hub. Each PC appears on the floor view as **Unassigned** (dashed tile) until he drags it into a group.
7. Groups and plans (template or CSV dry-run), products, and inviting Neha as manager with a PIN. **Products, staff invite, and printer & payments can be skipped and finished later from the Hub.**
8. Printer & payments: detect the ESC/POS printer and run a test print. Card and UPI references are recorded manually, so there are no UPI QR keys to enter in this phase.
9. Test session: a 2-minute session starts and the HUD counts down.
10. **Climax:** the kiosk locks at zero exactly on cue, so Vikram knows the floor is live. The go-live checklist (opening hours, backup location, standby PC on paid, ad consent on Free) unlocks the Owner app dashboard.

Failures:
- An expired or mistyped code shows a plain message on the Hub and "Generate a new code" on the wizard.
- A kiosk that can't see the Hub shows a field to type the Hub's IP.

### Journey 2 — Walk-in postpaid session (Rohit, 19, counter staff; a walk-in guest)

1. A walk-in arrives. Rohit types the phone number into `player-lookup` or taps Guest.
2. Rohit taps a free tile. The sheet shows the group rate and any active `chip-rule`, and one tap starts the session.
3. The kiosk unlocks within 2 s. The HUD shows elapsed time and the rate.
4. The player orders from the kiosk Shop (Basic+); the order attaches to the session.
5. The player taps "I'm done" on the HUD, or Rohit ends from the tile (`undo-toast`). The PC locks with "Pay at counter".
6. The sheet shows the `bill-breakdown` (minutes × rate, orders, GST).
7. Rohit settles with any mix in `payment-split`: cash, UPI (last 4 digits of the reference), card reference, or wallet. A remainder can go to credit only for a profiled credit player. He can convert the guest to a profile here.
8. **Climax:** the receipt prints (if a printer is available), the PC cleans up and returns to attract, and the tile turns `{colors.state-free}`. The bill matched the clock to the minute, so there's no argument.

Failures:
- Settle with a remainder: Settle stays disabled, with the reason shown.
- Credit at the limit: `inline-pin` override (once per player per day).
- A mis-tapped End or Settle: Undo.

### Journey 3 — Prepaid player at the kiosk (Aman, 17, prepaid regular)

1. The attract screen loops game video, rates and the active dynamic pricing rule. There are no ads in Phase 1.
2. Aman logs in with **username + password** (show-password available). Staff-start from the Hub would also work.
3. The `plan-picker` shows only plans valid for this PC; his pack shows hours left and expiry.
4. "Waiting for the counter…", then the Hub approves. The shell opens on Home with his time in the `time-chip`.
5. He launches a game from the hero, and the background video stops.
6. HUD warning toasts + sound arrive at 10, 5 and 1 min and stay until he dismisses them. The chip turns amber with a ⚠ at 10.
7. He presses Ctrl+Shift+E (or taps Extend on the toast). `extend-options` are priced live by the Hub: "More time added."
8. Later, time is up. The grace countdown runs over his still-visible ranked match, with Extend still on offer.
9. **Climax:** the match finishes inside grace and the lock lands after it. In this branch's time-up mode (clean up), the next player never sees Aman's Steam or Discord: accounts are signed out and the PC returns to attract.

Failures:
- Insufficient balance: balance + "Top up at counter"; stays on the picker.
- 3 bad logins: 60 s lock.
- Extend past closing time: refused with copy.
- The branch uses the "staff handles" mode: the lock holds over the paused game until staff arrive.
- The Hub drops mid-session: "Reconnecting…", and play continues; Extend and Shop orders wait with "Counter is reconnecting — try again shortly."

**3a — Sign-up (Aman's first visit).** Aman taps Create account. He enters username, password, phone and name, and ticks or leaves the WhatsApp/SMS consent box. No OTP. Duplicate username or phone gets field-level copy. On success he lands on the plan picker.

### Journey 4 — Kiosk order to kitchen (Aman orders; Sunita, kitchen). Basic+ only; on Free, orders go through the counter POS

1. Aman opens Shop mid-session. Out-of-stock items are greyed "Out of stock", and top sellers carry `popular-badge`.
2. He builds a `cart`. Stock is checked live, and a bundle refuses if a component is short.
3. He places the order. It attaches to his session, or he pays now by wallet. With no session, he pays at the counter.
4. The KOT prints within 3 s. The Hub plays the new-order sound with an `alert-badge`. Sunita's display chimes and the ticket appears at the top with the seat number and "NEW".
5. Sunita taps New → Preparing → Ready (each with Undo). The ticket gains a green edge and "READY".
6. Aman's HUD toasts "Order's ready" (also in `messages-bell`). Rohit sees a "READY" `tile-order-badge`.
7. **Climax:** the food arrives at seat 7 with nobody shouting across the room, and the line is already on Aman's bill.

Failures:
- Printer offline: `alert-badge` on the Hub; the display still receives the order.
- Cancel after preparing: a reason is required, and it lands in the waste report.

### Journey 5 — Shift close and variance (Rohit closes; Neha, branch manager)

1. Rohit opens the shift with a confirmed float.
2. Sales, deposits (slip reference) and expenses (receipt photo) record through the day.
3. Close shift: `denomination-counter` or a single total. Expected cash stays hidden until counted cash is entered.
4. `variance-readout` shows the signed difference (counted − expected) with "short" or "over". The breakdown is one tap away.
5. Within the threshold (default ₹100), the shift closes with no PIN.
6. Over the threshold, Neha enters `inline-pin` + a reason in place.
7. Vikram is alerted (web push + WhatsApp) under "Needs attention". The Z-report prints.
8. **Climax:** Rohit sees a number, not an accusation, and the Z-report matches what Vikram's Owner app shows.

Closing cash becomes the next float unless a deposit was made.

### Journey 6 — Owner changes a price remotely (Vikram)

1. In the Owner app, Vikram edits a rate and chooses one branch or all. A preview lists the affected branches.
2. He confirms (MFA session). Each branch shows a `command-row` with "Waiting for branch" and the time queued.
3. The Hub polls, applies once and reports.
4. **Climax:** the badge flips to Applied, and the new rate is on floor view and the kiosk attract screen within a minute. Vikram never touched the branch.

Failures:
- Rejected: reason + Retry.
- Branch offline for 7 days: Expired + Resend.

### Journey 7 — Credit player (Karan, 22, credit player)

1. Neha enables credit on Karan's profile with a limit and a reminder cycle.
2. Karan starts a session on credit, and the Hub shows remaining headroom.
3. A later order would exceed the limit, so the kiosk shows "Credit limit reached, see counter."
4. Neha overrides with `inline-pin`, **once per player per day**, logged.
5. Reminders go by WhatsApp/SMS on cycle days, only with consent. They are off on Free, where the Hub shows an overdue list instead.
6. At 15+ days overdue, Vikram's Owner home alerts and credit auto-pauses.
7. Karan returns, and Rohit looks him up: outstanding is shown first.
8. **Climax:** a one-tap collect (`undo-toast`), partial allowed. Outstanding drops and credit reopens automatically, blocked at the limit and not after.

### Journey 8 — Hub fails, standby promoted (Rohit, Saturday rush)

1. The main Hub PC dies mid-rush. The kiosks show "Reconnecting…", and every session keeps running.
2. Rohit opens the Hub on the standby PC.
3. He taps "Promote to main", and Neha's (or any manager's) PIN is entered inline.
4. **Climax:** the kiosks reconnect by themselves, sessions reconcile, and no player notices.

Failure: [NOTE FOR UX] Promote failure and split-brain (the old Hub returns) are not defined.

### Journey 9 — Partner refers a café (Imran, partner / installer)

1. Imran finds Partners on the marketing site and applies.
2. His application is approved, and he logs into the partner portal (own login).
3. The portal shows his referral link and code.
4. Vikram signs up using Imran's link.
5. **Climax:** the partner portal lists Vikram's café as referred, with the monthly 10% payout accruing.

[NOTE FOR UX] The approval notification and the co-branded kiosk screen (source v1.1) are undefined. Application fields and payout terms: see Open Questions.

### Journey 10 — Console session (Rohit; PS5, TV 3)

1. Two friends ask for the PS5. Rohit taps the "PS5 · TV 3" `floor-tile-console`, looks them up or picks Guest, and starts. The same postpaid, pack and pass rules apply.
2. The Hub runs the timer. If the profile has consent, an optional WhatsApp/SMS goes out at 5 min.
3. The tile shows the warning states as time runs low.
4. At time-up the tile raises `alert-badge` "TIME UP" + sound, and the badge holds until it is acknowledged.
5. **Climax:** Rohit walks over, ends the session from the tile (Undo available), and settles. The audit records who ended it, and nobody overran unnoticed.

v1.1 (Pi with QR start, TV-input warnings, controller check) is deferred. [NOTE FOR UX] The Pi/QR flow is not designed.

## Receipts, Prints & Messages

Structure only; register is formal. Visual print treatment: see `DESIGN.md` Layout (open).

| Output | Contents (in order) |
|---|---|
| Receipt / GST invoice | Branch logo + name, GSTIN · invoice no. (sequential per branch and financial year, never reused) · date/time · staff · player or "Guest" · session line (device, minutes × rate) · item lines (name, HSN, qty, rate, GST rate, amount) · coupon · discount · subtotal · GST breakdown · total · payments by method (with UPI last-4 / card refs) · remainder to credit · footer text. B2B adds the customer GSTIN. Delivery: print · WhatsApp (consent) · none |
| Credit note | As invoice, negative, referencing the original invoice no. |
| Credit collection receipt | Amount, method, references to the settled transactions, outstanding after |
| KOT | Seat/device no. · order id · time · items + modifiers. Reprint from the Hub |
| Z-report | Sales by type · payments by method · expected vs counted · variance · waste · expenses · deposits · refunds · drawer opens |
| WhatsApp / SMS | Credit reminder (amount, branch) · low balance · session ending · console 5-min alert. Each requires consent |
| Owner alerts (web push + WhatsApp) | Branch offline · cash variance (branch, staff, amount, reason) · low stock · credit overdue · large refund/discount |

## Game Media

- Media appears on tiles, the hero and Pi idle when the control-plane library has a match for an auto-detected game.
- Only curated library videos (flash-screened before publishing) autoplay.
- An owner may upload media locally for unmatched games. It stays on that device and is not synced, and it shows as a still cover unless approved. [NOTE FOR UX] Who approves a local upload, and where the owner matches/uploads media (Hub Settings vs Owner app), are not specified.
- Fallback: video → still cover → solid `{colors.bg}`. Muted always, stopped while any game runs, skipped in high-contrast. Offline lookup failures fall straight through the chain with no player-facing error.

## Offline & Sync

- The branch is local-first. Hub, Kiosk, Kitchen and POS work fully with the internet down (see State Patterns, Cloud offline).
- The kiosk survives losing its Hub (see State Patterns, Hub unreachable).
- The Owner app reads the cloud mirror: it shows data age, and remote commands sit as "Waiting for branch".
- UPI and card payments are always recorded manually by reference, so settling never depends on the internet.

## Ads

- **Phase 1 has no kiosk ads.** The attract screen exists without an ad slot, and the PRD's "every 15 min on Free" is deferred.
- The Free-tier Android Hub carries an `ad-banner` and optional rewarded ads (PRD). Basic and Premium show none. Upgrading removes ads within one heartbeat. There is never an ad on a player screen during a paid session.
- [NOTE FOR UX] The rewarded-ad player flow (who triggers it, where the player watches) is not designed.

## Tier Gating in UI

The tier matrix lives in the PRD.

- **Staff and owner surfaces** show gated features as `tier-lock` with "Available on <tier>" + Upgrade.
- **Player surfaces hide** gated features entirely.
- Free:
  - Hub on Android only.
  - Kiosk Shop hidden; orders at the counter POS.
  - Kitchen locked.
  - Owner app shows Subscription, pairing and support, with dashboards locked.
  - Credit reminders off (Hub overdue list instead).
  - 3 users and 20 devices; limits show `tier-lock` when reached.
- Live view is locked below Premium.

## Responsive & Platform

| Surface | Behavior |
|---|---|
| Kiosk | Fixed 1920×1080 landscape on Windows 10–11 via Tauri/WebView2. [NOTE FOR UX] Other resolutions and ultrawide behaviour (letterbox vs reflow) are undecided. |
| Hub, Windows desktop | Rail + header + reflowing tile grid with airy spacing (`{spacing.floor-grid-gap}`, `{spacing.hub-section-gap}` between zones). Fewer tiles fit per screen; the floor scrolls and zones collapse at high device counts. |
| Hub, Android tablet | Same layout, touch-first (Tauri 2 mobile). |
| Hub, Android phone | Same tile grid and screens. The rail becomes `hub-bottom-bar` (Floor · POS · Players · Shift · More), and tiles fit 2 columns (`{spacing.floor-grid-gap-phone}` gap) and scroll vertically. 44px targets are kept. (Breakpoint and POS arrangement: see `DESIGN.md` Layout note.) |
| Kitchen display | Tablet landscape browser. Airy columns mean fewer tickets show per column; each column scrolls independently, newest on top. |
| Owner app | One responsive web app from phone to desktop, installable as a PWA, with web push alerts. Full parity across widths. `owner-nav` is a bottom tab bar on phone and a left sidebar on desktop. |
| Web (marketing, onboarding, partner) | Responsive web. [NOTE FOR UX] The phone treatment of onboarding's `wizard-progress` column and the marketing phone navigation are not decided (see `DESIGN.md` Layout note). |
| Pi / QR (v1.1) | TV 16:9 idle/lock; phone portrait QR start page. |

## Inspiration & Anti-patterns

- **Kept from the ggLeap kiosk** ([imports/](imports/)):
  - The art-backed login panel, now a solid panel over video with no blur.
  - Hero Home + featured row + Play CTA.
  - Top pill nav + always-visible time chip.
  - Library/shop grid with left filter rail + search.
  - Also: show-password and Forgot password; volume control; messages bell; a "?" button that became Call staff; the All-games tile; launcher cards; the "Most played here" carousel; Popular badges; cart + checkout; PC settings; the Account page with sub-nav.
- **Dropped from ggLeap:**
  - Coins, prize vault, quests/challenges, social feed.
  - Language picker, social login.
  - Frosted translucency.
  - In-kiosk "Add money" (top-up is at the counter).
  - GamePass / merch categories.
- **Explored and set aside:** the Chai, Graphite and Overdrive directions ([.working/directions-index.html](.working/directions-index.html)). Cinema was chosen, then re-paletted to Electric.

## Upstream Updates Required

Already applied on 2026-10-22, with no action needed:
- Brand renamed to Arena 360.
- Kiosk login is username + password, with the staff-start path kept.
- English only.
- Tauri, Windows 10+.

Outstanding:

| Spine decision | Source to update |
|---|---|
| **No UPI dynamic QR this phase**; UPI and card recorded manually by reference (last 4 digits) | PRD POS "UPI QR" row (l.152), cross-cutting Payments (l.426), Money scope "UPI QR" (l.69), onboarding step 8 "UPI provider keys"; Journeys §2 Settle "UPI QR" |
| **No kiosk attract-screen ads in Phase 1** | PRD Ads decision (l.35), Attract screen row (l.270), Ads cross-cutting (l.428), tier table "Ads on attract screen" (l.464), placement rule 1 (l.473); Journeys §3 Attract (l.77) |
| **Kiosk orders are Basic+**; Free orders at the counter POS | PRD tier table "Kiosk app … orders" on Free (l.448) |
| **Credit override once per player per day** | PRD Credit row "override once" (l.90); Journeys §2 "Rest to credit" (l.61) |
| **No player OTP** | PRD Sign-up row "OTP optional per branch" (l.272), against l.37 and l.525 |
| **Owner app is one responsive web app + PWA with web push** | PRD Owner app intro "phone app and a web app" (l.325); Notifications "push for owner app" (l.427) |
| **Android Hub on Tauri 2 mobile** | PRD Free-tier Hub note (l.469) and architecture: confirm AdMob banner and rewarded ads inside Tauri 2 mobile |
| **Time-up mode is a per-branch setting** | PRD Configuration "auto-lock behaviour" (l.221): name the two modes |
| Architecture doc title | PRD summary (l.7) still cites "Gamezone SaaS — Low-Level Architecture" |
| **QR-from-Hub kiosk login dropped**; login = username + password or staff-start | PRD kiosk login/sign-up rows (QR path kept on 2026-10-22); Journeys §3 Login |
| **Shift variance = counted − expected** (+ over, − short) | PRD Shift close / variance rows ("expected minus counted") |
| **Kiosk while Hub unreachable**: running sessions continue; new login, orders and Extend disabled | PRD Kiosk offline behaviour; Journeys §8 |
| **Extend allowed during the 2-min grace** | PRD time-up / grace row |
| Journeys doc | Title "Arena360" (no space); add a Journey 8 section; add Journey 9 (partner) and Journey 10 (console) |

## Open Questions

1. **Consent scope.** Does the single sign-up checkbox cover WhatsApp receipts, credit reminders and the console 5-min alert? What is the exact wording?
2. **GST display on Pricing.** Are subscription prices shown GST-inclusive or exclusive on the marketing Pricing page and in the Owner app Subscription?
3. **Yearly billing.** Is there a yearly plan, and at what discount versus monthly?
4. **Add-on prices.** Prices for the Pricing page add-ons are not set in the PRD.
5. **Cancellation and refunds.** Subscription cancellation terms and refund policy (pro-rata, notice period) for the Pricing FAQ and Subscription screen.
6. **Partner payouts.** Payout method, schedule, minimum threshold and tax handling (TDS/GST, invoices) behind the 10% monthly payout in the partner portal.
7. **Partner application fields.** What the Partners application form collects (identity, business, location, bank/KYC) and what approval requires.
