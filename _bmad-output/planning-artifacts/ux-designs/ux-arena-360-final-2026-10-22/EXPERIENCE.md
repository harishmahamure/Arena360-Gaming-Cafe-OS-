---
name: Arena 360 Experience
status: final
updated: 2026-10-23
sources:
  - docs/Arena 360 - Product.md
  - docs/Arena360 — User Journeys.md
---

# Arena 360 — Experience Spine

`DESIGN.md` owns how it looks; this spine owns how it works. Token references use `{path.to.token}` and resolve in `DESIGN.md`. Requirements, personas, tiers and acceptance criteria live in the sources and are referenced, not restated. Currency is shown as ₹ in examples and is locale-formatted in the product.

**The spines (`DESIGN.md` and this file) win on conflict with any mock, wireframe, or import.** The mock index is under Visual References.

## Foundation

This is a multi-surface product: one brand in two moods (see `DESIGN.md` Brand & Style). All app surfaces share **one web UI + design-token codebase**. Kiosk and Windows Hub ship it inside **Tauri** (WebView2). The Android Hub ships it inside **Tauri 2 mobile**. Kiosk, Hub, Kitchen display and Pi/QR screens are dark only, plus a high-contrast (HC) theme; the Owner app and all web surfaces also have a light theme (follows the OS, with a manual toggle). English only.

| Surface | Form factor / platform | Mood | Primary user |
|---|---|---|---|
| Kiosk | Tauri desktop app (WebView2); shell replacement on each gaming PC; Windows 10–11; landscape, reflowing at 1366 / 1920 / 2560 / ultrawide | Player | Player |
| Overlay HUD | Always-on-top Tauri window over games, same PC | Player | Player |
| Hub | Tauri on Windows 10+ desktop (paid tiers). Tauri 2 mobile on Android tablet **and** phone (the only Hub on Free; also allowed on paid tiers). Same tile grid on all; below 840px wide the rail becomes a bottom bar | Staff | Counter staff, manager |
| Kitchen display | Browser page on any LAN tablet | Staff | Kitchen staff |
| Owner app | One responsive web app, installable as a PWA (phone + desktop), with web push for alerts | Staff | Owner |
| Onboarding wizard | Web, resumable, 10 steps | Staff | Owner |
| Partner portal | Separate web app at partners.arena360.in with its own login (a partner is not a café owner) | Staff | Partner / installer |
| Marketing site | Static web | Player | Prospective owner, partner |
| Pi TV idle/warning/lock + phone QR start page and live timer | TV via Pi; the player's phone browser (no login). **v1.1** | Player | Player |
| Receipts, prints & messages | Thermal print, WhatsApp/SMS, web push | Formal | Player, owner, kitchen |

Two principles from the sources frame every player state. **"The kiosk only asks; the Hub decides."** **"The Kiosk owns the PC between sessions and stays out of the way during one."** Staff screens follow the PRD's simplicity posture: owners compose roles from a fixed catalogue, and inventory is deliberately simple.

## Visual References

- [Key-screen index](mockups/mocks-index.html): 19 key-screen mocks, linked below at the IA surface, component and Key Flow step each one illustrates.
  - Kiosk: [Login](mockups/key-kiosk-login.html) · [Home](mockups/key-kiosk-home.html) · [Games](mockups/key-kiosk-games.html) · [Shop & cart](mockups/key-kiosk-shop.html) · [Account](mockups/key-kiosk-account.html) · [HUD & time-up](mockups/key-kiosk-hud-lock.html)
  - Hub: [Floor view](mockups/key-hub-floor.html) · [Tile sheet](mockups/key-hub-tile-sheet.html) · [POS](mockups/key-hub-pos.html) · [Shift close](mockups/key-hub-shift-close.html) · [Phone](mockups/key-hub-phone.html)
  - Owner + Kitchen display: [Owner Today](mockups/key-owner-today.html) · [Price change](mockups/key-owner-price-change.html) · [Kitchen board](mockups/key-kitchen-board.html)
  - Pi / QR console: [TV, phone and controller check](mockups/key-pi-console.html)
  - Web: [Marketing home](mockups/key-web-marketing-home.html) · [Pricing](mockups/key-web-pricing.html) · [Onboarding install Hub](mockups/key-web-onboarding-install-hub.html) · [Partner portal](mockups/key-web-partner-portal.html)
- [Cinema direction](mockups/direction-cinema.html): composition reference only; what it supersedes is listed in `DESIGN.md` Visual references.
- [Colour themes](mockups/color-themes.html): dark and HC palette source (see `DESIGN.md` Colors).
- [Light theme variants](mockups/color-themes-light.html): light palette source; only "1 · Cool paper" is adopted (see `DESIGN.md` Colors, Light theme).
- [ggLeap kiosk screenshots](imports/): structure only (see Information Architecture).

Every mock annotation marked as an assumption or open question has been resolved in these spines. **Mocks that predate later decisions** (the spines win; re-render pending):
- [Floor view](mockups/key-hub-floor.html): no `group-chip`.
- [Hub phone](mockups/key-hub-phone.html): no phone POS (`phone-cart-bar` and full-height cart sheet).
- [Owner Today](mockups/key-owner-today.html) and [Price change](mockups/key-owner-price-change.html): dark only (no light theme); Live view as a locked Today section; no "Queued on this device", MFA or Manage → Pricing.
- [Onboarding install Hub](mockups/key-web-onboarding-install-hub.html): still shows a pairing QR; no phone `wizard-progress` dots; dark only.
- [Pricing](mockups/key-web-pricing.html): no Monthly · Yearly toggle, GST-exclusive note, add-on prices or refund FAQ; still lists rewarded ads.
- [Marketing home](mockups/key-web-marketing-home.html): hero not on `web-hero`; dark only.
- [HUD & time-up](mockups/key-kiosk-hud-lock.html): annotation still describes a forced visible warning in fullscreen-exclusive games.

**Spine-only (no mock; build from this spine and `DESIGN.md`):**
- Kiosk: attract, sign-up (`signup-form`), `plan-picker`, Apps tab.
- Hub: Players, Sessions, Inventory, Reports, Settings, System.
- Owner app: Reports, Alerts, Account, and Manage beyond the command list.
- Web: onboarding steps other than Install Hub; marketing pages other than Home and Pricing.
- Receipts, prints and messages.
- Hub `ad-interstitial`; Owner app Manage → Pricing and Subscription.

## Information Architecture

### Kiosk

Reached from power-on or session end.

- **Flow:** Attract → Login → Plan picker → Waiting for Hub → **Main shell**. Session end: `lock-screen` → per-branch time-up mode → Attract.
- **Login:** `login-panel` with username + password.
  - Alternative: staff starts the session from the Hub.
  - "Create account" → `signup-form`.
  - "Forgot password?" → the Forgot password line in Voice and Tone.
- **Main shell, top bar:** `logo-slot`; `pill-nav` with Home · Games · Apps · Shop (Shop on Basic+ only); right cluster of `reconnecting-indicator` (when shown), `volume-control`, `messages-bell`, `call-staff-button`, `time-chip`, account chip.
- **Home:** video `hero` (Play now · Game details) + `video-control`; featured `game-tile` row ending in `all-games-tile`.
- **Games:** `launcher-card` row, `most-played-carousel`, `filter-rail` + search + grid.
- **Apps:** same structure as Games.
- **Shop:** food & drink only; `shop-item-card` grid → `cart` → checkout.
- **`account-menu`:** balance, Extend, Messages, Account, PC settings (mouse sensitivity, display resolution and refresh from an allowed list, audio output device, keyboard layout), Lock, Log out.
- **Account page:** `account-subnav` with Details · Wallet & packs · Order history · Change password. The `pill-nav` tab the player came from stays active.
- **In-game:** Overlay HUD.
- **Admin exit:** hidden combo **Ctrl+Alt+Shift+A** then manager `inline-pin`, or Hub remote unlock from the `tile-sheet` → desktop. Both are logged.
- **Mocks:** [Login](mockups/key-kiosk-login.html) · [Home](mockups/key-kiosk-home.html) · [Games](mockups/key-kiosk-games.html) · [Shop & cart](mockups/key-kiosk-shop.html) · [Account](mockups/key-kiosk-account.html) · [HUD & time-up](mockups/key-kiosk-hud-lock.html).

### Hub

Reached from app launch or idle-lock unlock.

- **Navigation:** persistent `hub-rail`: **Floor view (home)** · POS · Players · Sessions · Inventory · Shift · Reports · Settings · System.
  - Below 840px wide, `hub-bottom-bar`: Floor · POS · Players · Shift · More (Sessions first, then Inventory, Reports, Settings, System).
- **Floor view:** tiles grouped by zone; zones collapsible. Each tile shows its device group as a `group-chip`.
- **Floor tile** → `tile-sheet`: start · extend · order · transfer · pause/resume · lock · end.
- **POS:** `pos-product-grid` · `cart` · `settle-bar`. On phone: grid + `phone-cart-bar` → full-height cart and settle sheet.
- **Players:** `player-lookup` → profile (outstanding first; credit enable; collect; reset password).
- **Shift:** open with float · close (`denomination-counter`, `variance-readout`) · Z-report.
- **Settings:** layout editor (groups, zones, group colour from six swatches), time-up mode, order fulfilment (delivered to seat or pick up at counter), high-contrast per device, receipts/printer (58mm or 80mm), Kiosk default volume, opening hours and ads consent (Free).
- **System:** backup, standby promote, audit, diagnostics.
- **Mocks:** [Floor view](mockups/key-hub-floor.html) · [Tile sheet](mockups/key-hub-tile-sheet.html) · [POS](mockups/key-hub-pos.html) · [Shift close](mockups/key-hub-shift-close.html) · [Phone](mockups/key-hub-phone.html).

### Other surfaces

| Surface | Structure | Reached from |
|---|---|---|
| **Kitchen display** | Single screen: `kitchen-top-bar` (branch · clock · "5 open · oldest 9 min" · LAN status · mute chime) over four columns: New → Preparing → Ready → Served ([Kitchen board mock](mockups/key-kitchen-board.html)) | LAN URL |
| **Owner app** | **Navigation:** `owner-nav`: Today · Reports · Alerts · Manage · Account (bottom tab bar on phone, left sidebar on desktop). Live view sits in the sidebar on desktop and as a **Live view** button on each Today branch card on phone; both locked below Premium.<br>**Today:** per branch, as `metric-figure`s: open/closed, sessions live, revenue so far, cash in drawer, per-shift cash variance (counted − expected; over threshold flagged amber), staff on shift, last sync time; then a "Needs attention" list.<br>**Reports:** Revenue · Cash reconciliation · Stock · Credit · Audit · Exports.<br>**Manage:** Pricing (Plans & rates · Pricing rules · Coupons; rule rows show schedule, branches and status) · Products · Devices & groups · Staff users & roles · `command-row` list ("Queued on this device" → "Waiting for branch").<br>**Account:** MFA · Subscription · Branches · Theme (`theme-toggle`).<br>**Subscription:** tier per branch, add-ons, billing cycle (Monthly · Yearly `tab-control`), invoices, payment method, Cancel. Prices as on the marketing Pricing page, shown GST-exclusive.<br>**Other:** Alerts · Live view (Premium).<br>**On Free:** Subscription, pairing and support only; everything else shows `tier-lock`.<br>**Mocks:** [Today](mockups/key-owner-today.html) · [Price change](mockups/key-owner-price-change.html). | PWA / web login; web push |
| **Onboarding wizard** | `wizard-progress` beside the step content (horizontal dots + step name on phone). 10 steps per the PRD: tenant → tier → install Hub (`pairing-code`, typed code, no QR) → devices → plans → products → staff → printer & payments → test session → go live. Done steps link back for edits. Only step 3 is mocked ([Onboarding install-Hub mock](mockups/key-web-onboarding-install-hub.html)) | Marketing "Get started" |
| **Marketing site** | **MVP:** Home · Product · Pricing (`tab-control`: Cafés · Partners) · Get started · Downloads · Help · FAQ · Legal.<br>**v1.1:** Demo · Compare · Status · Partners.<br>**Home:** sections, in order: hero (`web-hero` copy left in the scrim zone, a Hub floor-view product shot right), "Install. Pair. Play." three steps, "What you get" three cards, closing Get started band, footer with `theme-toggle` ([Marketing home mock](mockups/key-web-marketing-home.html)).<br>**Pricing, Cafés tab:** one primary "Get started" above the table; Monthly · Yearly `tab-control`; three equal tier `card`s with no emphasised plan; a plan comparison table; add-ons; FAQ. Displayed values are listed under Pricing and partner terms below.<br>**Pricing, Partners tab:** programme terms, a worked payout example, payout terms and Apply ([Pricing mock](mockups/key-web-pricing.html)).<br>**Phone navigation:** a hamburger opens a full-screen nav (Product · Pricing · Downloads · Help · FAQ); "Get started" stays visible in the header. | Public |
| **Pi / QR (v1.1)** | **TV:** `tv-idle` → console (during play) → `tv-warning` at 5 and 1 min → `tv-lock` at time-up → `tv-idle`.<br>**Phone (no login):** `qr-start-page` (pick a console plan → Request → "Waiting for the counter") → `live-timer` after staff start. | TV / QR scan |
| **Partner portal** | Referred cafés · Payouts · Referral link · KYC.<br>**Home:** referral link + code card, a metric row (payout this month, active paying cafés, commission rate), a referred-cafés table and a payouts table showing accrued, pending (below minimum), paid, TDS and GST-invoice status ([Partner portal mock](mockups/key-web-partner-portal.html): approved and under-review states).<br>**Before approval:** application status only ("Under review" tracker).<br>**After approval, before first payout:** KYC (PAN, bank details, GSTIN optional). | Partner login (link from approval WhatsApp) |

Modal depth is one level, never stacked. Kiosk Shop is food & drink only; time packs and Extend live in the account menu and HUD.

### Pricing and partner terms (displayed values)

These are the values the marketing Pricing page, the Owner app Subscription screen and the partner portal display. India pricing; other countries follow the PRD's regional (PPP) bands.

| Item | Displayed as |
|---|---|
| Free | ₹0 · 1 branch · ads on the Android Hub |
| Basic | ₹200/branch/month + 18% GST · yearly ₹2,000/branch/year + 18% GST |
| Premium | ₹1,999/branch/month + 18% GST · yearly ₹19,990/branch/year + 18% GST |
| Yearly | 10× monthly ("2 months free"); paid tiers only; switchable in Owner app Subscription |
| GST | Prices GST-exclusive with the note "+ 18% GST"; GST itemised on the invoice |
| Add-ons (per branch/month, + GST) | +20 devices ₹50 · Tunnel for remote devices ₹200 · Additional owner login ₹50 |
| Refunds | Full refund within 7 days of the first payment. After that, cancel anytime: the plan stays active to the end of the paid period (monthly or yearly), then the branch drops to Free. No partial refunds. Shown in the Pricing FAQ and the Subscription cancel flow |
| Partner commission | 10% of subscription revenue for every month the referred café stays active and paying |
| Partner payouts | Monthly bank transfer by the 10th for the previous month · ₹500 minimum, a lower balance rolls over · TDS deducted at 2% (subject to tax advice), shown on each payout · GST-registered partners upload a GST invoice |
| Partner application | Name, phone (OTP), email, city, business name, type (reseller / installer), cafés served. KYC after approval: PAN, bank details, GSTIN (optional). Approval notified by WhatsApp with the portal login link |

→ Structure references:
- [imports/ggleap-02-home.png](imports/ggleap-02-home.png): hero, featured row, pill nav, time chip.
- [imports/ggleap-03-account-menu.png](imports/ggleap-03-account-menu.png): account menu, minus the gamification balances.
- [imports/ggleap-04-games-library.png](imports/ggleap-04-games-library.png): launcher cards and the most-played carousel.
- [imports/ggleap-05-apps.png](imports/ggleap-05-apps.png): apps grid.
- [imports/ggleap-06-shop.png](imports/ggleap-06-shop.png): shop and cart.
- [imports/ggleap-07-account.png](imports/ggleap-07-account.png): account sub-nav.
- [mockups/direction-cinema.html](mockups/direction-cinema.html): Hub floor view and HUD composition; layout only.

## Voice and Tone

Brand posture lives in `DESIGN.md`. There are three registers:

| Register | Where | Do | Don't |
|---|---|---|---|
| Player: casual, gamer-friendly | Kiosk, HUD, Pi/QR, marketing | "10 min left. Extend?" · "More time added. GG." · "Order's ready — grab it at the counter." | "Your session will terminate in 600 seconds." |
| Staff: plain, neutral, no blame | Hub, Kitchen display, Owner app, Onboarding, Partner portal | "Counted ₹4,850. Expected ₹4,900. Difference −₹50 short." · "Waiting for branch" · "Printer offline" | "Cash shortage detected!" · "You entered the wrong amount" |
| Money / legal: formal | Receipts, GST invoices, terms, subscription, payouts | "Invoice No. 000123 · GST 18% included" · "₹200/branch/month + 18% GST" | Slang, emoji, abbreviations |

Refusals state the fact and the next step. Error copy on the Hub never names a person. Fixed player-facing lines:

| Situation | Copy |
|---|---|
| Login failure | "Username or password didn't match." (never says which) |
| Login locked | "Too many tries. Try again in 0:60." |
| Forgot password | "Ask staff at the counter to reset it." |
| Sign-up duplicate | "That username's taken." · "That phone number already has an account — ask staff if it's yours." |
| Sign-up consent (required checkbox) | "I agree to the Terms and Privacy Policy and to receive service messages on WhatsApp/SMS." |
| Insufficient balance | "Balance ₹40. Top up at counter." |
| Credit limit | "Credit limit reached, see counter." |
| Low balance | "Balance running low — ₹40 left." |
| Extend past closing | "Can't extend past closing time (11:00 pm)." |
| Extended | "More time added. GG." |
| Order ready, delivered to seat | "Order's ready — on its way to you." |
| Order ready, pick up at counter | "Order's ready — grab it at the counter." |
| Blocked app | "That app isn't allowed on this PC." |
| Forced end | "Your session was ended by staff. Please see the counter." |
| Call staff | "Staff notified." |
| Hub unreachable | "Reconnecting…" |
| Hub unreachable, new login / order / Extend | "Counter is reconnecting — try again shortly." |
| Tier-gated (staff) | "Available on Basic" · "Upgrade" |
| TV warning (Pi) | "5 min left — ask at the counter to extend" · "1 min left — ask at the counter to extend" |
| TV time-up (Pi) | "Time's up — see the counter." |
| QR start, waiting | "Waiting for the counter — pay at the counter to start." |
| QR live timer | "Time left on TV 3" · "Extend at the counter" |

Fixed staff and owner lines:

| Situation | Copy |
|---|---|
| Owner command overwrote a Hub edit | Hub toast: "Price updated by owner" (or the matching entity: "Product updated by owner") |
| Command rejected | "Device/Hub revoked" · otherwise "Couldn't apply at branch" + Retry |
| Command queued offline | "Queued on this device" |
| Old main Hub returns | "This PC is no longer the main Hub" + "Make this the standby" |
| Forced promote warning | "Data since the last sync may be lost. Promote anyway?" |
| Installer bad or expired code | "That code didn't work. Get a new code from your browser." |
| Partner approval (WhatsApp) | "Your Arena 360 partner application is approved. Log in: <link>" |

## Component Patterns

These rows are behavioural. Visual specs live in `DESIGN.md` Components, under the same five groups. Names are identical across both spines.

### Shared

| Component | Use | Behavioural rules |
|---|---|---|
| `focus-ring` | Every focusable element, every surface | Shown on keyboard focus on every focusable element, never suppressed. Focus order follows reading order. Sticky bars (`settle-bar`, `hub-bottom-bar`, `ad-banner`) and the open `tile-sheet` never cover the focused element: scroll padding equals their height. |
| `text-field` | Login, sign-up, search, lookup, PIN reason, cash count, payment refs, onboarding | Label above, always programmatically linked. Validation runs on submit and on blur. On failure, focus moves to the first invalid field and the error is announced. Password variant has show/hide. Paste is allowed everywhere, including the Kiosk password field (WCAG 3.3.8). |
| `interaction-states` | Every button, tile, card and row | Hover and pressed feedback on every pointer surface; touch shows pressed only. Disabled controls never respond and always show the reason inline. HC swaps tints for outline changes. |
| `theme-toggle` | Owner app Account; web footer | System · Light · Dark. System (the default) follows `prefers-color-scheme` live. The choice is remembered per device. Not on Kiosk, Hub, Kitchen display or Pi. |
| `button-primary` | All | One per view. Disabled state shows why, inline (never a silent disabled). |
| `button-secondary` / `button-warning` | All / HUD | Secondary for non-primary actions (Dismiss, Cancel, End, Lock). Warning only for Extend in `hud-toast` and `extend-options`. |
| `undo-toast` | Hub (End, Settle, Collect), Kitchen display (stage advance) | ([Tile sheet](mockups/key-hub-tile-sheet.html), [POS](mockups/key-hub-pos.html), [Kitchen board](mockups/key-kitchen-board.html) mocks) Money actions run **in one tap, with no confirm modal**, then show Undo for 5–10 s. Undo restores the prior state exactly. The toast never blocks the floor. A new money action replaces the toast and commits the previous one. The receipt prints at once if a printer is available. **No Undo once a receipt has printed**: the toast is not shown and staff use Refund instead. Kiosk cleanup runs **immediately** on End; Undo restores the session record, but the player must log in again. |
| `alert-badge` | Hub header, floor tiles, kitchen tickets | The visual twin of every alert sound. It persists until acknowledged by opening the item it points to. The header shows the count, and opening the header badge lists the alerts. |
| `status-badge` | Floor tiles, Owner commands, Kitchen display | Always colour + text. Owner command states: pending ("Waiting for branch" + time queued) → applied / rejected (reason + Retry) / expired (Resend). Changes are announced in a polite live region on web. |
| `tier-lock` | Hub, Owner app, Onboarding (staff/owner only) | A gated feature or limit (Kitchen display on Free, Owner dashboards on Free, Live view below Premium, the device limit (Free 20, Basic 60, Premium 200), Free's 3 users) shows locked with "Available on <tier>" + Upgrade, which leads to Owner app Subscription. **Player surfaces hide gated features entirely.** |

### Player (Kiosk, HUD)

| Component | Use | Behavioural rules |
|---|---|---|
| `scrim` / `tile-scrim` | All media-backed surfaces | Always rendered when media is present. Removed with media in high-contrast. |
| `video-control` | Kiosk login, Kiosk Home hero | ([Home mock](mockups/key-kiosk-home.html)) Always visible while video plays. Keyboard-reachable. Pause holds the current frame for the rest of the login or session view. |
| `login-panel` | Kiosk login | ([Login mock](mockups/key-kiosk-login.html): at rest, failed submit, Hub unreachable) Username + password; Enter submits. 3 failures lock login for 60 s with a visible countdown, and the Hub is notified. Non-memory alternative: **staff starts from Hub** (satisfies WCAG 3.3.8 by alternative). QR-from-Hub login is dropped for now. While the Hub is unreachable, submit is disabled (see State Patterns, Hub unreachable). "Forgot password?" shows the Forgot password line from Voice; staff reset it from the Players profile. No language picker. |
| `signup-form` | Kiosk "Create account" | (Spine-only.) Fields: username, password, phone, name. **No OTP.** One **required** consent checkbox, unticked by default, covering Terms, Privacy Policy and service messages only (WhatsApp/SMS receipts, credit reminders, session and console alerts); copy in Voice. There is no marketing consent. Account creation is blocked until it is ticked, with the reason inline. Duplicates are blocked by username **and** by phone, with the copy in Voice. On success the profile is created on the Hub and the player continues to the plan picker. |
| `pill-nav` / `pill-nav-item-active` | Kiosk shell | Home · Games · Apps · Shop. Shop is hidden on Free (player surfaces hide gated features). Exactly one item is active; on the Account page the tab the player came from stays active. Arrow keys move; selection persists per session and resets on logout. |
| `time-chip` | Kiosk top bar | Always visible while logged in. Counts down (prepaid) or up (postpaid). At ≤10 min the chip gains a 2px warning border and a ⚠ glyph replaces the dot. Click → `account-menu`. |
| `reconnecting-indicator` | Kiosk top bar | Appears only while the Hub is unreachable and disappears on reconnect once the session has reconciled. What stays working and what is disabled: State Patterns, Hub unreachable. |
| `volume-control` | Kiosk top bar | Sets Windows system volume for this session. Resets to the branch default at logout. |
| `messages-bell` | Kiosk top bar | Lists staff broadcast and per-seat messages and order-status updates for this session. Opening the bell marks them read. The list clears at session end. |
| `call-staff-button` | Kiosk top bar | One tap pings the Hub: an `alert-badge` "CALL" on that tile plus the alert sound. The Kiosk confirms "Staff notified." |
| `account-menu` | Kiosk | ([Account mock](mockups/key-kiosk-account.html)) Balance, Extend (opens `extend-options`), Messages, Account, PC settings, Lock, Log out. Log out confirms only if time remains on a prepaid plan. **PC settings:** mouse sensitivity, display resolution and refresh (from the branch's allowed list), audio output device, keyboard layout. They save to the player's profile and re-apply when that player logs in on any PC; other players get branch defaults. |
| `account-subnav` | Kiosk Account page | Details · Wallet & packs (balance, packs with hours left and expiry) · Order history (this branch) · Change password (current + new). Single-select; arrow keys move. |
| `hero` | Kiosk Home, attract, login | ([Home mock](mockups/key-kiosk-home.html)) Plays muted game video for the featured title and follows the media fallback chain. The featured title is the branch's most-played game this week, and the meta line includes the player's own last-played time ("You last played 2 days ago"). Stops entirely when a game launches. Play now launches the title. **Game details** (secondary) opens a panel with genre, age rating and launcher; Esc closes it and returns focus. |
| `game-tile` / `game-tile-featured` | Home row, Games/Apps grid | Focus = `focus-ring`; activate launches (Games) or opens (Apps). The featured tile drives the hero media. Being featured is not focus. |
| `all-games-tile` | End of Home featured row | Activates Games with no filter. |
| `most-played-carousel` | Games tab | ([Games mock](mockups/key-kiosk-games.html)) The branch's **top 10 games by session hours over the last 7 days**, ranked. Branch-level, not per player; not gamification. Arrow keys scroll. |
| `launcher-card` | Games tab | Opens Steam / Epic / Riot for the player's **own** account. Logout is enforced at session end. |
| `filter-rail` | Games, Apps, Shop | Single-select category + search. Filters apply instantly (no Apply button). An empty result shows an underlined "Clear filters" link. |
| `shop-item-card` | Kiosk Shop, Hub POS | Add to `cart`; stock checked live against the Hub. Out of stock is disabled with "Out of stock". Bundles show their components and refuse if any is short (override only at POS). |
| `popular-badge` | Kiosk Shop | Marks the branch's **top 3 shop items by units sold over the last 7 days**. |
| `cart` | Kiosk Shop checkout; Hub POS | **Kiosk** ([Shop & cart mock](mockups/key-kiosk-shop.html)): ±1 steppers; live stock check on each change; "Place order" attaches to the running session (shows "ordered" on the HUD). The player can instead pay now by wallet. With no session: "Pay at counter". **Hub:** see `pos-product-grid` / `settle-bar`; a cart can attach to a running session. Empty cart: "Cart's empty." |
| `card` | Balances, account sections | Generic container; no behaviour of its own. |
| `plan-picker` | Kiosk after login | (Spine-only.) Shows only plans valid for this device's group. Packs show hours left and expiry; expired packs are greyed and not selectable. The wallet option shows balance and hourly rate, plus any active dynamic pricing rule. Selecting sends a request; the Kiosk shows "Waiting for the counter…" until the Hub approves or refuses with a typed reason. |
| `extend-options` | Expanded HUD; account menu; grace `lock-screen` | (Entry points in the [HUD & time-up](mockups/key-kiosk-hud-lock.html) and [Account](mockups/key-kiosk-account.html) mocks; the panel itself is spine-only.) Opened from the `hud-toast` Extend, the **global Extend hotkey Ctrl+Shift+E** (opens the expanded HUD with focus on the first option), account menu → Extend, or Extend on the grace countdown. Disabled while the Hub is unreachable (see State Patterns). Options are priced live by the Hub and charged to wallet, pack or credit. Refusals: past closing time, credit limit, insufficient balance. Each uses Voice copy and the panel stays open. |
| `hud-pill` | Overlay HUD | Shows time left or elapsed, and balance. The Extend hotkey (Ctrl+Shift+E) expands it. Never takes focus on its own. **Hidden in fullscreen-exclusive games**; at 5 and 1 min the Kiosk then **flashes the taskbar button and plays the warning sound**. No overlay is forced over the game. Known limitation: see Accessibility Floor. |
| `hud-toast` | Warnings, low balance, order status, staff messages, forced end | ([HUD & time-up mock](mockups/key-kiosk-hud-lock.html)) Appears top-right with sound and never steals focus. **Warning toasts (10, 5 and 1 min; low balance) persist until dismissed or time-up.** Info toasts (order status, messages) auto-dismiss after 8 s and are kept in `messages-bell`. Each toast shows the Extend hotkey (Ctrl+Shift+E). Warning timings are configurable per branch (PRD). |
| `lock-screen` | Time-up, pay at counter, forced end | ([HUD & time-up mock](mockups/key-kiosk-hud-lock.html), grace variant) Grace countdown (default 2 min) runs over the still-visible game, with **Extend** available alongside the countdown (opens `extend-options`; a successful Extend dismisses the lock). After grace, the **per-branch time-up mode** applies. **Clean up:** end all non-system processes except allow-listed (protected) ones, sign out launchers, clear temp files and the clipboard, reset volume, then return to attract. **Staff handles:** keep the lock screen over the paused game until staff handle it. Postpaid end shows "Pay at counter". Forced end shows the forced-end line. |
| `logo-slot` | Kiosk, Pi | Branch logo only; falls back to the Arena 360 mark. |

### Player (Pi / QR)

| Component | Use | Behavioural rules |
|---|---|---|
| `tv-idle` | TV between console sessions | ([Pi console mock](mockups/key-pi-console.html)) Shown on the Pi input whenever the console tile is Free. Muted game video via the Kiosk media pipeline (fallback chain applies), branch logo, TV number, console rates and a QR that opens `qr-start-page` for this TV. No ads in Phase 1. On session start the Pi switches the TV to the console input. |
| `tv-warning` | 5 and 1 min left | The Pi switches the TV to its own input for 5 s, shows the warning with the Voice copy, then switches back to the console. |
| `tv-lock` | Time-up | **No grace.** At time-up the Pi immediately switches the TV to its input and holds "Time's up — see the counter." (no countdown, no other copy) until staff end the session, then returns to `tv-idle`. Staff handle the overrun at the counter; powering the TV down is per branch (HDMI-CEC standby or a smart plug). |
| `qr-start-page` | Player's phone after scanning | ([Pi console mock](mockups/key-pi-console.html)) **No login.** Lists the console plans valid for this TV with live prices. Request sends a start request to the Hub (tile `alert-badge` "REQUEST" + sound) and shows the waiting line. The player **pays at the counter**; staff confirm there and start from the tile. A request staff refuse shows "Ask at the counter." |
| `live-timer` | Player's phone after start | Replaces the start page: time left on this TV, updated over the branch LAN, with "Extend at the counter". **No actions.** If the phone leaves the LAN it shows the last value with "Reconnect to café Wi-Fi to update". |

### Staff (Hub, Kitchen)

| Component | Use | Behavioural rules |
|---|---|---|
| `hub-rail` / `hub-bottom-bar` | Hub | Floor view is home and the default after unlock. Items are always labelled. On phone the bottom bar replaces the rail: Floor · POS · Players · Shift · More; More opens a bottom sheet listing Sessions (first), Inventory, Reports, Settings, System ([Hub phone mock](mockups/key-hub-phone.html)); closing it returns to the current screen. The switch happens below 840px wide. |
| `hub-header` | Hub | Always shows: branch, active `chip-rule`, connectivity (`chip-offline`), cloud backup status on paid tiers (last upload, pending batches), `alert-badge` count, clock, signed-in staff. Tapping backup status opens System. |
| `floor-tile` | Hub floor view | ([Floor view mock](mockups/key-hub-floor.html)) Tiles group by zone; each zone header can collapse its zone (see State Patterns, High device count). Tap or Enter → `tile-sheet`. Updates ≤2 s from source events. Live updates never move focus or reorder a focused tile. Shows the `status-badge`, time, player, `group-chip`, `tile-order-badge` and `alert-badge`. |
| `group-chip` | Floor tile corner | Read-only. Name + colour dot of the tile's device group; the owner or manager sets group colours from six swatches in Settings → Layout. Hidden on Unassigned tiles. |
| `floor-tile-console` | Hub floor view (consoles) | Same as `floor-tile`. Sessions are started and ended **from the Hub only**, and the timer runs on the Hub. Warnings show on the tile. Time-up raises `alert-badge` "TIME UP" + sound until staff walk over and end it. The audit records who ended it. After the end the tile shows "CHECK CONTROLLERS" until staff tap **Returned** or **Missing** in the sheet (logged against the session). A QR start request raises `alert-badge` "REQUEST"; the sheet opens with the requested plan pre-selected and Start. |
| `tile-order-badge` | Floor tile | "ORDER" while the kitchen is working; "READY" when the order is ready to deliver to the seat or to pick up at the counter, per the branch's order fulfilment setting. Tapping the tile opens the sheet at the order section. |
| `tile-sheet` | Hub | ([Tile sheet mock](mockups/key-hub-tile-sheet.html)) Start / extend / order / transfer / pause-resume / lock / end; each is one tap. Pause asks for a reason: Player stepped away · Hardware issue · Power cut · Other; paused time isn't billed, and the tile's secondary line shows the reason. The sheet docks to the right and the floor reflows while it is open. Remote unlock (for Kiosk admin exit) needs `inline-pin` and is logged. Transfer: pick a target tile; the old device locks and the new one unlocks within 5 s. Shows `bill-breakdown`. Non-modal: the floor stays live behind it. On open, focus moves into the sheet; Esc closes it and returns focus to the tile. |
| `inline-pin` | Manager-gated actions (forced end, manual discount, refund/void, variance over threshold, credit override, promote standby and forced promote, Kiosk admin exit, any action the signed-in role lacks) | ([Tile sheet](mockups/key-hub-tile-sheet.html) and [Shift close](mockups/key-hub-shift-close.html) mocks) Renders in place inside the current sheet, row or Kiosk panel. Numeric keypad + keyboard. Never navigates away. A reason picker appears in the same place when required. |
| `player-lookup` | Hub floor sheet, POS, Players | One phone field; results as you type; Enter picks the top result. "Guest" is one tap or key away. Credit outstanding is shown first on the profile. In POS, the attached player shows in the cart header with a "Change" link. No match: "No player with that number." with **Create profile** (completes in ≤15 s) and **Guest**. |
| `pos-product-grid` | Hub POS | ([POS mock](mockups/key-hub-pos.html)) Categories and favourites first. One combined "Search or scan barcode" field above the grid. A barcode scan (keyboard wedge) adds the item: 1 scan + 1 tap to sell. Out-of-stock and expired items are disabled. A short bundle refuses with an `inline-pin` override. |
| `settle-bar` | Hub POS, session sheet | ([POS mock](mockups/key-hub-pos.html): remainder open, UPI ref missing, no printer, settled) Shows the total and the payment methods: cash · UPI · card · wallet · credit. Settle is disabled until the remainder is zero or moved to credit; the reason is shown inline. Keys: **Ctrl+Enter** settles; the payment method is chosen with Tab/arrows + Enter. No dedicated payment-method shortcut. Coupon and manual discount (PIN + reason) apply here. Receipt choice: print · WhatsApp (only with consent) · none. Print happens only if a printer is available; with none, nothing prints and Settle is never blocked. Settle → `undo-toast`. |
| `phone-cart-bar` | Hub POS below 840px | Appears once the cart has an item: "3 items · ₹310 · View". Tap opens `cart` + `settle-bar` as a full-height sheet; Esc or Back closes it with the cart kept. |
| `payment-split` | Settle | Add a method, enter an amount, add more. The remainder recomputes live. **UPI:** no dynamic QR and no webhook; staff record the payment by entering the **last 4 digits of the transaction reference** (required — the UPI row can't be saved without it). **Card:** external terminal; enter the reference. **Credit:** only for a profiled player with credit enabled; blocked at the limit with an `inline-pin` override once per player per day. A guest can be converted to a profile here. |
| `bill-breakdown` | Session sheet, POS | ([Tile sheet mock](mockups/key-hub-tile-sheet.html)) Lines in order: session (device, minutes × rate after rounding unit and minimum; paused time excluded; one line per rate segment after a transfer or dynamic pricing rule), orders, coupon, discount, subtotal, GST (inclusive/exclusive per branch), total. Then "Paid so far" and "Remaining" lines. A group session splits by device minutes, and the payer can be changed before settlement. |
| `denomination-counter` | Shift close | ([Shift close mock](mockups/key-hub-shift-close.html), including the blind state) One count field per denomination with a live total; or a "Single total" toggle. Expected cash is **never shown until counted cash is entered.** |
| `variance-readout` | Shift close | Shows counted, expected and the signed difference. "Breakdown" is one tap away and lists opening float, cash sales, cash refunds, deposits and expenses. Within threshold (default ₹100), close completes with no PIN. Over threshold: `inline-pin` + a reason from a list + free text, in place. Reason list: Counting error · Unrecorded expense · Change float mismatch · Other (+ free text). Close → Z-report prints with one tap. **Difference = counted − expected**: + means extra cash ("over"), − means short ("short"). |
| `chip-rule` | Hub header, Kiosk attract, plan picker | Shows the active dynamic pricing rule while that rule is in effect. A rule change reaches the chip within 1 min. |
| `chip-offline` | Hub header | Appears when internet or cloud is unreachable and names which. Nothing else in the UI changes. |
| `kitchen-ticket` | Kitchen display | ([Kitchen board mock](mockups/key-kitchen-board.html)) New tickets appear at the top with elapsed time, a chime and the "NEW" `alert-badge` (held until advanced). Tap advances one stage → `undo-toast`. **The Kitchen display cannot cancel**: only the Hub/counter cancels (after preparing, with a reason, landing in the waste report). A cancelled ticket shows "Cancelled", greys out, then leaves the board. During LAN loss tickets stay visible and can still be advanced, with no per-ticket marker (see State Patterns). |
| `kitchen-top-bar` | Kitchen display | Branch name · clock · open tickets with the oldest age · LAN status (Connected / Reconnecting) · mute-chime toggle. Muting silences the chime only; the "NEW" `alert-badge` and a "Chime off" label stay visible. |
| `ad-banner` | Free-tier Android Hub | See Ads. Never covers the floor grid, cart or settle bar. Never on the Kiosk in Phase 1. |
| `ad-interstitial` | Free-tier Android Hub | After every 2nd completed settle (receipt done). Skippable after 5 s. Never shown while a cart or `tile-sheet` is open; deferred if another sale starts. |

### Owner and Web

| Component | Use | Behavioural rules |
|---|---|---|
| `metric-figure` | Owner app | ([Owner Today mock](mockups/key-owner-today.html)) Every figure carries its data age. Loads from the mirror in under 2 s. Never shows a broken state when the branch is offline; it just ages. An offline branch card drops its figures to muted ink, shows "Branch offline", and lists its queued commands ("Waiting for branch") on the card. |
| `owner-nav` | Owner app | ([Owner Today mock](mockups/key-owner-today.html), phone and desktop) Today · Reports · Alerts · Manage · Account. Bottom tab bar on phone, left sidebar on desktop; one destination active; Today is home. Live view is in the sidebar on desktop and a button on each Today branch card on phone, `tier-lock` below Premium. |
| `command-row` | Owner app Manage | ([Price change mock](mockups/key-owner-price-change.html)) One row per target branch. Status per `status-badge`: Queued on this device (offline) → Waiting for branch → Applied / Rejected / Expired. Expires after 7 days → Resend. The change list groups rows under Today / Earlier. **Only valid commands can be sent**: the app offers only valid targets and tier-locked items can't be sent. Rejection reasons: "Device/Hub revoked"; any residual failure shows "Couldn't apply at branch" + Retry. **Conflicts:** the owner's command wins and overwrites; the Hub shows a toast ("Price updated by owner") and the audit records both changes. **MFA:** fresh MFA only for sensitive commands (staff users and roles, revoke device/Hub, backup restore); prices, products and plans send without an MFA prompt. |
| `wizard-progress` | Onboarding wizard | ([Onboarding install-Hub mock](mockups/key-web-onboarding-install-hub.html)) Lists all 10 steps with done / current / upcoming state and "Can skip" tags on Products, Staff, and Printer & payments. The current step is `aria-current="step"`; on self-advance focus moves to the new step heading. Done steps link back for edits, upcoming steps open only in order, and resuming lands on the first incomplete step. Below 840px: horizontal dots + current step name. |
| `tab-control` | Marketing Pricing (Cafés · Partners; Monthly · Yearly); Owner app Subscription cycle | ([Pricing mock](mockups/key-web-pricing.html)) Switches peer panels in place without a page load; one tab selected. Arrow keys move between tabs; Tab moves into the panel (ARIA tabs). The selected tab is reflected in the URL so it can be linked. Monthly · Yearly swaps every displayed price. |
| `pairing-code` | Onboarding step 3 | ([Onboarding install-Hub mock](mockups/key-web-onboarding-install-hub.html): waiting and expired) 8-character single-use code, **typed** into the Hub installer (no QR), 15-min countdown (announced at milestones, not every tick). A "Copy code" action sits beside the code. The page advances by itself on the first Hub heartbeat and moves focus to the new step heading. Expired → "Generate a new code". A bad or expired code in the installer retries in place: inline error, field cleared, "Get a new code from your browser"; no restart. |

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
| Cloud / internet offline | Hub | `chip-offline` in the header; **nothing else changes** (shown in every Hub mock, for example [Floor view](mockups/key-hub-floor.html)). Every branch action keeps working. |
| Hub unreachable | Kiosk | Running sessions keep going on the last known time locally. `reconnecting-indicator` shows; the player is never locked out. New login, Shop orders and Extend are disabled with "Counter is reconnecting — try again shortly." ([Login mock](mockups/key-kiosk-login.html), state 3.) The Kiosk reconciles with the Hub on reconnect (Journey 8). |
| High device count | Hub floor view | The airy spacing shows fewer tiles per screen (accepted tradeoff). The floor scrolls vertically, and zones can collapse. A zone header collapses its zone to one row showing the zone name, tile count, counts per state and the zone's `alert-badge` count, so a collapsed zone never hides an alert. Zones start expanded. Collapsing is manual, with no auto-collapse threshold, and is remembered per device ([Floor view mock](mockups/key-hub-floor.html), Zone C). |
| No printer | Hub | Settle, Z-report and KOT complete without printing; no blocking error ([POS mock](mockups/key-hub-pos.html), no-printer state). |
| Waiting for Hub approval | Kiosk plan picker | "Waiting for the counter…" with the selected plan; returns to the picker with a typed reason on refusal. |
| Data age / pending command | Owner app | ([Owner Today](mockups/key-owner-today.html), [Price change](mockups/key-owner-price-change.html) mocks) Each figure shows data age. Remote changes show "Waiting for branch" with time queued. Expired after 7 days → "Expired" + Resend. |
| Media | Kiosk, Pi, marketing | Video → still cover (video missing or the PC can't play it; for games not in the global library, the launcher or exe icon) → solid `{colors.bg}`. Never a broken player. High-contrast skips straight to solid. |
| Cold load | Kiosk | The attract screen renders from cached branding and rates; media streams in after. |
| Cold load | Hub floor view | Tiles render from the local DB immediately. No skeletons (local-first). |
| Empty | Games/Apps grid | "No games here yet — ask at the counter." |
| Empty | Kiosk Shop | "Kitchen's closed right now." when the menu is empty or unavailable. |
| Empty | Kiosk / POS cart | "Cart's empty." |
| Empty | Kitchen display column | Column header only; no filler copy. |
| Empty | Owner Alerts | "All quiet." |
| Empty | Partner portal | "No referred cafés yet." + referral link. |
| Application under review | Partner portal | Before approval the portal shows only "Under review" with a three-step tracker (submitted → review → approved); referral link and cafés appear on approval ([Partner portal mock](mockups/key-web-partner-portal.html), state B). |
| KYC pending | Partner portal | After approval, payouts show "KYC needed before first payout" with a link to KYC (PAN, bank, optional GSTIN). |
| Below payout minimum | Partner portal | Payout row "Pending — below ₹500 minimum, rolls over". |
| No match | Hub `player-lookup` | "No player with that number." + Create profile / Guest. |
| Refusal | Kiosk plan picker | "Balance ₹40. Top up at counter."; stays on the picker. |
| Refusal | Extend | Past closing time / credit limit / insufficient balance: Voice copy; options stay open. |
| Credit block | Kiosk / Hub | "Credit limit reached, see counter." The Hub offers an `inline-pin` override, once per player per day. |
| Sign-up duplicate | Kiosk | Field-level error on username or phone, using the Voice copy. |
| Login lock | Kiosk | 60 s countdown on the panel after 3 failures; the Hub is notified with an `alert-badge`. |
| Blocked app | Kiosk | `hud-toast` "That app isn't allowed on this PC."; logged on the Hub. |
| Forced end | Kiosk | `lock-screen` with the forced-end line. |
| Low balance | Kiosk HUD | Persistent warning `hud-toast` with Extend / top-up guidance. |
| Remote action pending / failed | Hub | Result shown within 5 s, otherwise marked "Failed — retry" on the tile. |
| Printer offline | Hub | `alert-badge` on the Hub. The Kitchen display still receives the order. |
| Low stock / expiring | Hub | `alert-badge` in the header; the item is listed in Inventory. Expired items are excluded from sale. |
| Idle lock | Hub | After 5 min idle (default), PIN unlock in place; returns to the same screen. |
| Error (save/apply) | Any | Inline, next to the control: plain cause + retry. Never a blocking modal during a sale. |
| Tier-gated | Staff / owner | `tier-lock`. Player surfaces hide the feature. |
| Not permitted (role) | Hub | The action stays visible; tapping it opens `inline-pin` in place for manager elevation (PRD PIN elevation). |
| LAN loss | Kitchen display | Tickets stay visible; advances are allowed and the ticket simply moves to its new column, syncing on reconnect. There is **no per-ticket marker**: "Reconnecting" in the `kitchen-top-bar` LAN status is the only signal. New orders can't arrive until reconnect. |
| No network | Owner PWA | Cached data with data age. Commands queue on the device as "Queued on this device", then become "Waiting for branch" once sent. |
| Old main Hub returns (split-brain) | Hub | The returning PC locks itself: "This PC is no longer the main Hub" with **Make this the standby**. It never serves Kiosks. |
| Command conflict | Hub | The owner's command overwrites the local edit; toast "Price updated by owner"; both changes in the audit. |
| Ad interstitial due | Free Android Hub | Deferred while a cart or sheet is open or a new sale starts; shown after the next completed settle instead. |

## Interaction Primitives

- **Hub speed rules.** Every common action is one tap from floor view, and no action takes more than two taps. Start and settle each finish under 10 s from the first tap.
- **No modal blocks a sale.** Sheets are non-modal; the floor view stays live and tappable behind them.
- **Money actions: one tap + Undo.** End, Settle, Collect and the Kitchen display stage advance execute immediately and show `undo-toast` for 5–10 s. There is never a confirm modal.
- **Manager PIN is inline.** Entry never leaves the current screen.
- **Keyboard.**
  - The counter flow can be completed without a mouse.
  - Floor grid: arrow keys move between tiles and Enter opens the sheet; Esc closes it and returns focus to the tile.
  - Shortcuts never fire while a text field has focus.
  - **Hub / POS keys:** Ctrl+1 Floor · Ctrl+2 POS · Ctrl+3 Players · Ctrl+4 Shift · Ctrl+K search · Ctrl+Enter settle · Esc close sheet; arrows move on the floor grid and Enter opens the tile sheet. The payment method in `settle-bar` is chosen with Tab/arrows + Enter; there is no dedicated shortcut.
  - **Kiosk keys (minimal):** Tab / Shift+Tab move, Enter activates, Esc goes back, plus the **global Extend hotkey Ctrl+Shift+E**, which opens the expanded HUD with focus on the first option; HUD toasts show it.
  - **Kiosk admin exit:** hidden combo Ctrl+Alt+Shift+A, then manager `inline-pin`; or Hub remote unlock from the `tile-sheet`. Both are logged.
- **Sound + visual.**
  - The Hub plays a sound on alerts and on new orders. The Kitchen display chimes on each new order. HUD warnings use toast + sound.
  - **Every sound has a persistent `alert-badge` twin** until acknowledged.
  - Hub alerts: new Kiosk order, order ready, call staff, console time-up, Kiosk login lockout, printer offline, low stock, expiring stock.
  - Background videos are **always muted**.
- **Hover, pressed, disabled.** One rule set for every surface (`interaction-states`); disabled always says why.
- **Video lifecycle.** Background video plays on Kiosk login, the Kiosk Home hero and Pi idle TV. It stops entirely while a game runs. The Kiosk has a visible pause/play `video-control`.
- **HUD never steals focus** from the game on any event. Focus moves to it only when the player presses the hotkey.
- **Motion.** Short, subtle transitions only; cut to instant in high-contrast. OS reduced-motion is **not** honoured separately (user decision).
- **Banned:** blocking modals during a sale, confirm modals on money actions, stacked modals, live blur, autoplay audio, gamification (coins, quests, prize vault, social feed), ads on any Kiosk or Pi screen in Phase 1.

## Accessibility Floor

Visual contrast and token ratios live in `DESIGN.md`.

- **Targets.** ≥ `{spacing.touch-min}` (44px) on every surface, including the Hub on an Android phone (two tile columns via `hub-bottom-bar`). Airy spacing never shrinks a target.
- **Focus.** `focus-ring` on every focusable element, distinct from the featured-tile outline. In HC it is the thicker yellow ring.
- **Boundaries.** Inputs, PIN cells and focusable edges use `{colors.border-strong}` (≥3:1).
- **Links** are always underlined.
- **High-contrast theme** (`*-hc` tokens) is a **branch/device setting**: an owner or manager toggles it per device from Hub settings. There is no player toggle. HC removes art, video and translucency, adds 2px outlines, and cuts motion. It applies to Kiosk, Hub, Kitchen display, Pi TV and Owner surfaces.
- **Light theme** (`*-light` tokens) on the Owner app and web surfaces follows `prefers-color-scheme`, with `theme-toggle` to override. All its load-bearing pairs meet the same floors (`DESIGN.md` Colors, Light theme).
- **Text over media.**
  - Always sits on `{components.scrim}` inside its copy zone, verified against a white frame.
  - Never in accent.
  - Game-tile labels sit on `{components.tile-scrim}`.
- **Colour is never the only cue.** Floor tiles, badges, tickets, the time-chip warning and field errors always carry text or a glyph.
- **Sound is never the only cue.** Every alert sound has a persistent `alert-badge` (Hub, Kitchen display).
- **Moving media.**
  - Visible pause/play on Kiosk login and Home.
  - Only curated videos from the global control-plane library autoplay; each is flash-screened before publishing. There are no owner uploads.
  - Marketing autoplay is not restricted (user decision).
- **Timed warnings.**
  - Warning toasts persist until dismissed or time-up.
  - Extend is reachable by hotkey and from the account menu, not only from the toast.
  - Fullscreen-exclusive games: taskbar flash + sound at 5 and 1 min (see `hud-pill`).
  - Extend stays available during the 2-min grace, on the lock screen.
  - **Known limitation (accepted):** in true fullscreen-exclusive games the taskbar flash may be invisible, so the 5- and 1-min warnings can be effectively sound-only for deaf and hard-of-hearing players. The 10-min chip warning, the grace lock screen and Extend by hotkey still apply.
- **Authentication (WCAG 3.3.8).**
  - The Kiosk password login has a non-memory alternative: staff starts the session from the Hub.
  - Show-password is available, paste is allowed, and the error copy never reveals which field failed.
  - Sensitive Owner commands ask for fresh MFA; nothing else re-prompts.
- **Web screen-reader baseline** (Owner app, onboarding, partner portal, marketing, Kitchen display, Hub web UI):
  - Landmarks (`header`, `nav`, `main`); one `h1` per view.
  - Every control programmatically labelled.
  - Polite live regions for `status-badge` changes, `chip-offline`, `alert-badge` counts, `hud-toast` and new kitchen tickets.
  - Countdowns announced at milestones, not every tick.
  - Self-advancing wizard steps move focus to the new heading.
- **Numbers.** Timers use tabular numerals so counting text doesn't shift.

## Key Flows

Step-level "what must be true" lives in the journeys source. The flows below are the experience path. Each has numbered steps ending in a **Climax**, then its failures.

### Journey 1 — Owner onboarding (Vikram, owner of two Pune cafés, moving off ggLeap)

1. Vikram taps Get started on the marketing site ([Marketing home mock](mockups/key-web-marketing-home.html)): name, phone, email, café name, city. Phone OTP.
2. The wizard creates the tenant and branch, and he picks a tier (Free needs no card; paid prices show "+ 18% GST", Monthly or Yearly).
3. The wizard shows the `pairing-code`: 8 characters, large, with Copy code and a 15-min countdown; no QR ([Onboarding install-Hub mock](mockups/key-web-onboarding-install-hub.html)).
4. On the counter PC he installs the Hub and types the code; the installer asks for nothing else.
5. The Hub pairs, and the wizard advances by itself on the first heartbeat.
6. On each gaming PC the Kiosk finds the Hub and asks for the 6-digit code shown on the Hub. Each PC appears on the floor view as **Unassigned** (dashed tile) until he drags it into a group ([Floor view mock](mockups/key-hub-floor.html)).
7. He sets up groups and plans (template or CSV dry-run) and products, and invites Neha as manager with a PIN. **Products, staff invite, and printer & payments can be skipped and finished later from the Hub.**
8. Printer & payments: detect the ESC/POS printer and run a test print. Card and UPI references are recorded manually, so there are no UPI QR keys to enter in this phase.
9. Test session: a 2-minute session starts and the HUD counts down.
10. **Climax:** the Kiosk locks at zero exactly on cue, so Vikram knows the floor is live. The go-live checklist (opening hours, backup location, standby PC on paid tiers, ad consent on Free) unlocks the Owner app dashboard.

Failures:
- An expired or mistyped code: the installer retries in place (inline error, field cleared, "Get a new code from your browser"); the wizard offers "Generate a new code" (Onboarding mock, state B).
- A Kiosk that can't see the Hub shows a field to type the Hub's IP.

### Journey 2 — Walk-in postpaid session (Rohit, 19, counter staff; a walk-in guest)

1. A walk-in arrives. Rohit types the phone number into `player-lookup` or taps Guest.
2. Rohit taps a free tile ([Floor view mock](mockups/key-hub-floor.html)). The sheet shows the group rate and any active `chip-rule`, and one tap starts the session ([Tile sheet mock](mockups/key-hub-tile-sheet.html)).
3. The Kiosk unlocks within 2 s. The HUD shows elapsed time and the rate.
4. The player orders from the Kiosk Shop (Basic+); the order attaches to the session.
5. The player taps "I'm done" on the HUD, or Rohit ends from the tile (`undo-toast`). The PC locks with "Pay at counter".
6. The sheet shows the `bill-breakdown` (minutes × rate, orders, GST) ([Tile sheet mock](mockups/key-hub-tile-sheet.html)).
7. Rohit settles with any mix in `payment-split` ([POS mock](mockups/key-hub-pos.html) shows the same settle anatomy): cash, UPI (last 4 digits of the reference), card reference, or wallet. A remainder can go to credit only for a profiled credit player. He can convert the guest to a profile here.
8. **Climax:** the receipt prints (if a printer is available), the PC cleans up and returns to attract, and the tile turns `{colors.state-free}`. The bill matched the clock to the minute, so there's no argument.

Failures:
- Settle with a remainder: Settle stays disabled, with the reason shown.
- Credit at the limit: `inline-pin` override (once per player per day).
- A mis-tapped End or Settle: Undo, until a receipt has printed; after printing, Refund.

### Journey 3 — Prepaid player at the Kiosk (Aman, 17, prepaid regular)

1. The attract screen loops game video, rates and the active dynamic pricing rule. There are no ads in Phase 1.
2. Aman logs in with **username + password** (show-password available) ([Login mock](mockups/key-kiosk-login.html)). Staff-start from the Hub would also work.
3. The `plan-picker` shows only plans valid for this PC; his pack shows hours left and expiry (spine-only; no mock).
4. "Waiting for the counter…", then the Hub approves. The shell opens on Home with his time in the `time-chip` ([Home mock](mockups/key-kiosk-home.html)).
5. He launches a game from the hero, and the background video stops.
6. HUD warning toasts + sound arrive at 10, 5 and 1 min and stay until he dismisses them ([HUD & time-up mock](mockups/key-kiosk-hud-lock.html)); in a fullscreen-exclusive title the 5- and 1-min warnings arrive as a taskbar flash + sound. At 10 min the chip gains a 2px warning border and a ⚠ glyph replaces its dot.
7. He presses Ctrl+Shift+E (or taps Extend on the toast). `extend-options` are priced live by the Hub: "More time added. GG."
8. Later, time is up. The grace countdown runs over his still-visible ranked match, with Extend still on offer (HUD & time-up mock, lower frame).
9. **Climax:** the match finishes inside grace and the lock lands after it. In this branch's "clean up" time-up mode, the next player never sees Aman's Steam or Discord: their processes are ended, accounts are signed out, the clipboard is cleared, volume resets and the PC returns to attract.

Failures:
- Insufficient balance: "Balance ₹40. Top up at counter."; stays on the picker.
- 3 bad logins: 60 s lock.
- Extend past closing time: refused with the Extend past closing line from Voice.
- The branch uses the "staff handles" mode: the lock holds over the paused game until staff arrive.
- The Hub drops mid-session: play continues (see State Patterns, Hub unreachable; [Login mock](mockups/key-kiosk-login.html) state 3 shows the disabled treatment).

#### Journey 3a — Sign-up (Aman's first visit)

1. Aman taps Create account on the login panel.
2. He enters username, password, phone and name, and ticks the required consent box ("I agree to the Terms and Privacy Policy and to receive service messages on WhatsApp/SMS."). No OTP.
3. **Climax:** on success his profile is created on the Hub and he lands on the plan picker.

Failures:
- Duplicate username or phone: field-level error with the Sign-up duplicate copy from Voice.
- Consent unticked: Create account stays disabled with the reason inline.

### Journey 4 — Kiosk order to kitchen (Aman orders; Sunita, kitchen)

Basic+ only; on Free, orders go through the counter POS. Order fulfilment is a branch setting: delivered to seat or pick up at counter.

1. Aman opens Shop mid-session ([Shop & cart mock](mockups/key-kiosk-shop.html)). Out-of-stock items are greyed "Out of stock", and top sellers carry `popular-badge`.
2. He builds a `cart`. Stock is checked live, and a bundle refuses if a component is short.
3. He places the order. It attaches to his session, or he pays now by wallet. With no session, he pays at the counter.
4. The KOT prints within 3 s. The Hub plays the new-order sound with an `alert-badge`. Sunita's display chimes and the ticket appears at the top with the seat number and "NEW" ([Kitchen board mock](mockups/key-kitchen-board.html)).
5. Sunita taps New → Preparing → Ready (each with Undo). The ticket gains a green edge and "READY".
6. Aman's HUD toasts the Order ready line that matches the branch's fulfilment setting (also kept in `messages-bell`). Rohit sees a "READY" `tile-order-badge` ([Floor view mock](mockups/key-hub-floor.html), PC-06).
7. **Climax:** this branch delivers to seat, so Aman reads "Order's ready — on its way to you." and the food arrives at seat 7 with nobody shouting across the room; in a pick-up branch he reads "Order's ready — grab it at the counter." Either way, the line is already on Aman's bill.

Failures:
- Printer offline: `alert-badge` on the Hub; the Kitchen display still receives the order.
- Cancel after preparing (Hub/counter only): a reason is required, it lands in the waste report, and the ticket shows "Cancelled" then leaves the board.
- Kitchen display loses the LAN: tickets stay and advanced tickets just move (no per-ticket marker), syncing on reconnect; "Reconnecting" in the top bar is the only signal.

### Journey 5 — Shift close and variance (Rohit closes; Neha, branch manager)

1. Rohit opens the shift with a confirmed float.
2. Sales, deposits (slip reference) and expenses (receipt photo) record through the day.
3. Close shift ([Shift close mock](mockups/key-hub-shift-close.html)): `denomination-counter` or a single total. Expected cash stays hidden until counted cash is entered.
4. `variance-readout` shows the signed difference (counted − expected) with "short" or "over". The breakdown is one tap away.
5. Within the threshold (default ₹100), the shift closes with no PIN.
6. Over the threshold, Neha enters `inline-pin` + a reason in place.
7. Vikram is alerted (web push + WhatsApp) under "Needs attention" ([Owner Today mock](mockups/key-owner-today.html)). The Z-report prints, and closing cash becomes the next float unless a deposit was made.
8. **Climax:** Rohit sees a number, not an accusation, and the Z-report matches what Vikram's Owner app shows.

Failures:
- None defined.

### Journey 6 — Owner changes a price remotely (Vikram)

1. In the Owner app, Manage → Pricing (Plans & rates · Pricing rules · Coupons), Vikram edits a rate and chooses one branch or all. A preview lists the affected branches with each one's online/offline status ([Price change mock](mockups/key-owner-price-change.html), frames 1–2).
2. He sends it; price changes need no MFA prompt. Each branch shows a `command-row` with "Waiting for branch" and the time queued (or "Queued on this device" first if his phone is offline).
3. The Hub polls, applies once and reports.
4. **Climax:** the badge flips to Applied (Price change mock, frame 3), and the new rate is on floor view and the Kiosk attract screen within a minute. Vikram never touched the branch.

Failures:
- Rejected: "Device/Hub revoked", or "Couldn't apply at branch" + Retry.
- Branch offline for 7 days: Expired + Resend.
- Staff edited the same price at the Hub: the owner's command wins; the Hub shows "Price updated by owner" and the audit keeps both.

### Journey 7 — Credit player (Karan, 22, credit player)

1. Neha enables credit on Karan's profile with a limit and a reminder cycle.
2. Karan starts a session on credit, and the Hub shows remaining headroom.
3. A later order would exceed the limit, so the Kiosk shows "Credit limit reached, see counter."
4. Neha overrides with `inline-pin`, **once per player per day**, logged ([Tile sheet mock](mockups/key-hub-tile-sheet.html) shows the inline override on Karan's PC-06).
5. Reminders go by WhatsApp/SMS on cycle days, only with consent. They are off on Free, where the Hub shows an overdue list instead.
6. At 15+ days overdue, Vikram's Owner app Today alerts him, and credit auto-pauses.
7. Karan returns, and Rohit looks him up: outstanding is shown first.
8. **Climax:** a one-tap collect (`undo-toast`), partial allowed. Outstanding drops and credit reopens automatically; it blocks again only when the limit is reached.

Failures:
- None defined.

### Journey 8 — Hub fails, standby promoted (Rohit, Saturday rush)

Spine-only (no mock).

1. The main Hub PC dies mid-rush. The Kiosks show "Reconnecting…", and every session keeps running.
2. Rohit opens the Hub on the standby PC.
3. He taps "Promote to main", and Neha's (or any manager's) PIN is entered inline.
4. **Climax:** the Kiosks reconnect by themselves, sessions reconcile, and no player notices.

Failures:
- Promote fails: a warning says data since the last sync may be lost; a manager can **force promote** with `inline-pin`. The forced promote is audited and Vikram is alerted.
- The old main Hub comes back (split-brain): it locks itself with "This PC is no longer the main Hub" and **Make this the standby**, and never serves Kiosks.

### Journey 9 — Partner refers a café (Imran, partner / installer)

There is no co-branded Kiosk screen; branch customisation stays logo-only. Terms: Information Architecture, Pricing and partner terms.

1. Imran finds Partners on the marketing site and applies ([Pricing mock](mockups/key-web-pricing.html), Partners tab): name, phone (OTP), email, city, business name, type (reseller / installer), cafés served.
2. Until approval the portal shows only his application status ([Partner portal mock](mockups/key-web-partner-portal.html), state B).
3. Approval arrives **by WhatsApp only**, with the portal login link. He logs in (own login).
4. The portal shows his referral link and code, and asks for KYC (PAN, bank details, optional GSTIN) before his first payout.
5. Vikram signs up using Imran's link.
6. **Climax:** the partner portal lists Vikram's café as referred, with the monthly 10% accruing; the payout lands by the 10th of next month, TDS at 2% shown (Partner portal mock, state A).

Failures:
- Balance below ₹500: shown as pending and rolled over.
- KYC not done: payout held with "KYC needed before first payout".
- GST-registered partner without an uploaded GST invoice: payout held with "Upload GST invoice".

### Journey 10 — Console session (Rohit; PS5, TV 3)

The Pi/QR path is v1.1 ([Pi console mock](mockups/key-pi-console.html)). Without a Pi, Rohit starts from the tile (step 4) and the tile alone carries the warnings and time-up.

1. TV 3 is idle ([Pi console mock](mockups/key-pi-console.html), frame 1): `tv-idle` loops muted game video with the branch logo, "TV 3", console rates and a large "Scan to start" QR.
2. Two friends scan it. `qr-start-page` opens on a phone with **no login**; they pick a console plan and tap Request ([Pi console mock](mockups/key-pi-console.html), frame 2): "Waiting for the counter — pay at the counter to start."
3. The "PS5 · TV 3" `floor-tile-console` raises `alert-badge` "REQUEST" + sound. Rohit opens the sheet: the requested plan is pre-selected.
4. The friends pay at the counter (or Rohit looks them up, or picks Guest; postpaid, pack and pass rules apply). Rohit taps Start. The Pi switches the TV to the console.
5. The phone page becomes `live-timer` ([Pi console mock](mockups/key-pi-console.html), frame 3): "Time left on TV 3", updating over the LAN, with "Extend at the counter". The Hub runs the timer; with consent on a profile, a WhatsApp/SMS goes out at 5 min.
6. At 5 and 1 min the Pi switches the TV to `tv-warning` for 5 s ([Pi console mock](mockups/key-pi-console.html), frame 4) ("5 min left — ask at the counter to extend"), then back to the game. The tile shows the warning states.
7. At time-up the TV locks immediately, with no grace, showing `tv-lock` ("Time's up — see the counter."; staff handle it; CEC standby or smart plug per branch; [Pi console mock](mockups/key-pi-console.html), frame 5), and the tile raises `alert-badge` "TIME UP" + sound until acknowledged ([Floor view mock](mockups/key-hub-floor.html), PS5 · TV 3).
8. Rohit ends the session from the tile (Undo available until a receipt prints) and settles. The tile shows "CHECK CONTROLLERS"; he taps **Returned** ([Pi console mock](mockups/key-pi-console.html), frame 6).
9. **Climax:** TV 3 returns to `tv-idle`, the audit records who ended it and that both controllers came back, and nobody overran unnoticed.

Failures:
- Extend asked mid-session: done at the counter from the tile; the phone timer and TV pick up the new time.
- A controller is missing: Rohit taps **Missing**; it is logged against the session.
- Rohit refuses the request: the phone shows "Ask at the counter." and the tile badge clears.
- The phone leaves the café Wi-Fi: `live-timer` keeps its last value with "Reconnect to café Wi-Fi to update".

## Receipts, Prints & Messages

Structure only; register is formal. Print treatment (58mm and 80mm, ESC/POS font, text header): see `DESIGN.md` Layout & Spacing, Thermal print.

| Output | Contents (in order) |
|---|---|
| Receipt / GST invoice | Branch name (text header, no logo image), GSTIN · invoice no. (sequential per branch and financial year, never reused) · date/time · staff · player or "Guest" · session line (device, minutes × rate) · item lines (name, HSN, qty, rate, GST rate, amount) · coupon · discount · subtotal · GST breakdown · total · payments by method (with UPI last-4 / card refs) · remainder to credit · footer text. B2B adds the customer GSTIN. Delivery: print · WhatsApp (consent) · none |
| Credit note | As invoice, negative, referencing the original invoice no. |
| Credit collection receipt | Amount, method, references to the settled transactions, outstanding after |
| KOT | Seat/device no. · order id · time · items + modifiers. Reprint from the Hub |
| Z-report | Sales by type · payments by method · expected vs counted · variance · waste · expenses · deposits · refunds · drawer opens |
| WhatsApp / SMS | Credit reminder (amount, branch) · low balance · session ending · console 5-min alert · receipt. Service messages only, under the sign-up consent; no marketing |
| Subscription invoice (owner) | Tier, branches, add-ons, billing cycle, subtotal, GST 18% itemised, total |
| Partner (WhatsApp, email) | Application approved + portal link (WhatsApp) · payout statement: accrued, TDS, paid, invoice status |
| Owner alerts (web push + WhatsApp) | Branch offline · cash variance (branch, staff, amount, reason) · low stock · credit overdue · large refund/discount |

## Game Media

- Game media is **global only**: the Arena 360 team curates games and media (flash-screened) on the control plane. There are **no owner uploads** and no matching UI.
- Media appears on tiles, the hero and Pi idle when the global library has a match for an auto-detected game. Devices hold cached copies of global media only.
- A game not in the library still shows a cover from its launcher or exe icon, else the solid fallback.
- Fallback: video → still cover → solid `{colors.bg}`. Muted always, stopped while any game runs, skipped in high-contrast. Offline lookup failures fall straight through the chain with no player-facing error.

## Offline & Sync

- The branch is local-first. Hub, Kiosk, Kitchen display and POS work fully with the internet down (see State Patterns, Cloud offline).
- The Kiosk survives losing its Hub (see State Patterns, Hub unreachable).
- The Owner app reads the cloud mirror: it shows data age, and remote commands sit as "Waiting for branch". Offline, the PWA shows cached data and queues commands as "Queued on this device".
- The Kitchen display survives LAN loss: tickets stay, advances move with no per-ticket marker and sync on reconnect.
- Owner commands win on conflict with local Hub edits; both are audited.
- Only one main Hub serves Kiosks: a returning old main locks itself (split-brain rule, Journey 8).
- UPI and card payments are always recorded manually by reference, so settling never depends on the internet.

## Ads

- **Phase 1 has no Kiosk ads.** The attract screen exists without an ad slot, and the PRD's "every 15 min on Free" is deferred.
- The Free-tier Android Hub carries an `ad-banner` and an `ad-interstitial` (AdMob) after every 2nd completed settle, skippable after 5 s, never while a cart or tile sheet is open, deferred if another sale starts. There are no rewarded ads. Basic and Premium show none. Upgrading removes ads within one heartbeat. There is never an ad on a player screen (Kiosk or Pi).

## Tier Gating in UI

The tier matrix lives in the PRD. See `tier-lock` for how staff, owner and player surfaces treat gated features.

- Free:
  - Hub on Android only.
  - Kiosk Shop hidden; orders at the counter POS.
  - Kitchen display locked.
  - Owner app shows Subscription, pairing and support, with dashboards locked.
  - Credit reminders off (Hub overdue list instead).
  - 3 users and 20 devices; limits show `tier-lock` when reached.
- Live view is locked below Premium.

## Responsive & Platform

| Surface | Behaviour |
|---|---|
| Kiosk | Landscape on Windows 10–11 via Tauri/WebView2. Responsive breakpoints at 1366, 1920, 2560 and ultrawide (21:9): the layout reflows (columns, featured-row length, hero width) rather than scaling a 1080p canvas (`DESIGN.md` Layout & Spacing, Kiosk). |
| Hub, Windows desktop | Rail + header + reflowing tile grid with airy spacing (`{spacing.floor-grid-gap}`, `{spacing.hub-section-gap}` between zones). Fewer tiles fit per screen; the floor scrolls and zones collapse at high device counts. |
| Hub, Android tablet | Same layout, touch-first (Tauri 2 mobile). |
| Hub, Android phone | ([Hub phone mock](mockups/key-hub-phone.html)) Below **840px** wide (phones and small portrait tablets): same tile grid and screens; the rail becomes `hub-bottom-bar` (Floor · POS · Players · Shift · More), and tiles fit 2 columns (`{spacing.floor-grid-gap-phone}` gap) and scroll vertically. POS is a full-width grid with `phone-cart-bar`; the cart and settle bar open as a full-height sheet. 44px targets are kept. |
| Kitchen display | Tablet landscape browser. Airy columns mean fewer tickets show per column; each column scrolls independently, newest on top. |
| Owner app | ([Owner Today mock](mockups/key-owner-today.html), 390 and 1440) One responsive web app from phone to desktop, installable as a PWA, with web push alerts. Full parity across widths. `owner-nav` is a bottom tab bar on phone and a left sidebar on desktop. Light and dark themes (OS default, `theme-toggle`). |
| Web (marketing, onboarding, partner) | Responsive web ([Marketing home mock](mockups/key-web-marketing-home.html) at 390 and 1440). Light and dark themes. Below 840px, onboarding's `wizard-progress` becomes horizontal dots + the current step name. Marketing phone navigation: hamburger → full-screen nav, "Get started" always in the header. |
| Pi / QR (v1.1) | TV 16:9 (`tv-idle`, `tv-warning`, `tv-lock`); phone portrait (`qr-start-page`, `live-timer`). |

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
  - In-Kiosk "Add money" (top-up is at the counter).
  - GamePass / merch categories.
- **Explored and set aside:** other directions and palettes ([.working/directions-index.html](.working/directions-index.html), audit trail); Cinema was chosen (see `DESIGN.md` Visual references).

## Upstream Updates

Changes the PRD and journey docs still need are tracked in [upstream-changes.md](upstream-changes.md).

## Open Questions

None open. Consent, GST display, yearly billing, add-on prices, refunds, partner payouts and the partner application are decided (see Information Architecture, Pricing and partner terms; Voice and Tone). PRD rows these decisions change are tracked in [upstream-changes.md](upstream-changes.md).

### Undecided UX items

None. Every formerly undecided item is decided in place.

No assumptions remain.

Known limitations (accepted):
- Fullscreen-exclusive warnings may be effectively sound-only for deaf and hard-of-hearing players: [Accessibility Floor](#accessibility-floor).
