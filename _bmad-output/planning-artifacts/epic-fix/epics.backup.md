---
status: draft
stepsCompleted: [1, 2, 3]
inputDocuments:
  - _bmad-output/specs/spec-arena-360/SPEC.md
  - _bmad-output/specs/spec-arena-360/glossary.md
  - docs/Arena 360 - Product.md
  - _bmad-output/planning-artifacts/architecture/architecture-arena-360-final-2026-10-23/ARCHITECTURE-SPINE.md
  - docs/Arena360 — High-Level Design.md
  - docs/Arena360 — Low-Level Design.md
  - _bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md
  - _bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/EXPERIENCE.md
  - docs/Arena360 — User Journeys.md
  - _bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/mockups/ (visual reference)
---

# Arena 360 - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for Arena 360, decomposing the requirements from the PRD, UX Design if it exists, and Architecture requirements into implementable stories.

**Source precedence used in extraction:** SPEC.md (canonical contract, CAP-n) > ARCHITECTURE-SPINE.md (AD-n, binding over HLD/LLD) > DESIGN.md + EXPERIENCE.md (UX spines win over mocks) > PRD (`docs/Arena 360 - Product.md`, cited as "PRD §section / row") > HLD / LLD / User Journeys (detail and traceability). Phase tags: **[MVP]** = delivery wave 1, **[v1.1]** = wave 2; everything ships in one launch. PRD wave-3 (v2) items are SPEC non-goals and are excluded.

## Requirements Inventory

### Functional Requirements

#### Onboarding and setup

FR1: [MVP] An owner signs up from the marketing site "Get started" form (name, phone, email, café name, city) and must verify the phone by OTP before the account exists. (CAP-1; PRD §Marketing site "Get started"; §Decisions "OTP")

FR2: [MVP] An owner logging in on a new device must pass TOTP; backup codes are issued at enrolment and an SMS OTP fallback is optional. (CAP-1; PRD §Owner app Account "MFA")

FR3: [MVP] Fresh step-up MFA is required only for sensitive owner commands (staff users/roles, revoke device/Hub, backup restore); prices, products and plans send without an MFA prompt. (CAP-1; AD-10; EXPERIENCE `command-row`)

FR4: [MVP] The onboarding wizard runs 10 ordered steps (tenant → tier → install Hub → devices → plans → products → staff → printer & payments → test session → go live), is resumable at every step, resumes on the first incomplete step, lets done steps be re-edited, opens upcoming steps only in order, and marks Products, Staff and Printer & payments as skippable. (CAP-2; PRD §Onboarding wizard)

FR5: [MVP] Wizard step 1 (Tenant) captures café name, country, currency, timezone, GST registered yes/no and GSTIN, and creates the tenant and first branch. (CAP-2; PRD §Onboarding wizard step 1)

FR6: [MVP] Wizard step 2 (Tier) picks Free, Basic or Premium per branch; Free needs no card; paid tiers take payment; the step completes when the entitlement is issued. (CAP-2; PRD §Onboarding wizard step 2)

FR7: [MVP] Plans and products can be seeded from a template (Indian café, console lounge, VR arcade) or by CSV import of players (phone, name, balance, credit limit), plans and products, each with a dry-run showing conflicts before commit; plan CSV is validated against existing groups; 1,000 rows import in < 30 s. (CAP-2; PRD §Players "Import", §Plans "Bulk import", §Inventory "Products", §Cross-cutting "Migration")

FR8: [MVP] Wizard step 4 (Devices) shows kiosks arriving live, lets the owner create groups and drag devices in and add consoles manually; done when at least one device is in a group; step 5 (Plans) is done when every group has at least one rate. (CAP-2; PRD §Onboarding wizard steps 4–5)

FR9: [MVP] Wizard step 7 (Staff) invites staff by phone with a role and shows a one-time setup code; done when at least one manager exists. (CAP-2, CAP-24; PRD §Onboarding wizard step 7)

FR10: [MVP] Wizard step 8 (Printer & payments) detects an ESC/POS printer and runs a test print (or is skipped); no payment-provider keys are requested because UPI and card are recorded by reference. (CAP-2; PRD §Onboarding wizard step 8)

FR11: [MVP] Wizard step 9 starts a 2-minute test session on one device from the wizard and completes when the kiosk locks at zero; step 10 go-live checklist (opening hours, backup location, standby PC on paid, ad consent on Free) marks the branch live and unlocks the Owner app dashboard. (CAP-2; PRD §Onboarding wizard steps 9–10)

FR12: [MVP] Running the wizard for a second branch reuses the tenant's groups, plans and products as templates by default. (CAP-2; PRD §Onboarding "Single-key setup")

FR13: [MVP] Install-Hub step shows an 8-character, single-use pairing code (grouped 4+4, Copy, 15-min countdown, no QR) encoding tenant, branch and tier; an expired code offers "Generate a new code"; a bad/expired code typed into the Hub installer retries in place (inline error, field cleared); the page advances by itself on the first Hub heartbeat. (CAP-3; PRD §Onboarding wizard step 3)

FR14: [MVP] Pairing exchanges the code once for a Hub device token and signed licence; the onboarding profile (name, tz, currency, GST registration, hours, branch code) is applied at first pairing as `branch.bootstrap`; a Free branch may pair only an Android Hub and a Windows Hub on Free is refused. (CAP-3; AD-3, AD-10, AD-29)

FR15: [MVP] A Kiosk on first run discovers the Hub on the LAN (mDNS) with a manual Hub-IP fallback, pairs by the 6-digit code shown on the Hub or a manager PIN, and appears as "Unassigned" on the floor view within 60 s. (CAP-4; PRD §Kiosk "Install"; §Devices "Device registry")

FR16: [MVP] Staff register consoles (with group and TV number), VR rigs and simulators manually; each appears on the floor view like a PC. (CAP-4, CAP-34; PRD §Devices "Device registry"; §Console MVP "Device type")

FR17: [MVP] Device groups have a name, a colour from six fixed swatches and a sort order; every device belongs to exactly one group; console groups are never shared with PC groups. (CAP-4, CAP-14; PRD §Devices "Groups"; §Decisions "Console pricing")

FR18: [MVP] Device status is one of online, offline, in session, reserved or maintenance; setting maintenance removes the device from the pool without deleting it. (CAP-4; PRD §Devices "Status")

FR19: [MVP] Staff drag devices onto a floor grid organised into zones (e.g. PC hall, console room); the layout persists per branch. (CAP-4; PRD §Floor view "Layout")

FR20: [MVP] Pairing a device beyond the tier device cap (Free 20, Basic 60, Premium 200, +20 per add-on) is refused with a tier-lock. (CAP-4; AD-29; PRD §Tiers "Max devices")

FR21: [MVP] With the Tunnel entitlement (Premium, or the Tunnel add-on on a paid tier) a kiosk or Pi outside the café LAN pairs and connects as a remote device by device token, never anonymously. (CAP-4; AD-6)

FR22: [v1.1] An owner migrates from ggLeap or Smartlaunch by uploading their exports, with field mapping and a dry-run report before commit. (CAP-5; PRD §Onboarding "Migration")

#### Floor and sessions

FR23: [MVP] The Hub floor view shows every device tile with live state updated ≤ 2 s from source events; each state carries colour + text label (free, in session with time, paused, reserved, offline, maintenance, order pending, unassigned); 60 devices render without lag on a 2015 PC. (CAP-6; PRD §Floor view "Live state")

FR24: [MVP] Tapping a tile opens a non-modal tile sheet with start, extend, order, transfer, pause/resume, lock and end; no action is more than 2 taps from the floor view and sheets never block the floor. (CAP-6; PRD §Floor view "One-tap actions")

FR25: [MVP] The active dynamic pricing rule is shown in the Hub header; a rule change is reflected within 1 min. (CAP-6; PRD §Floor view "Current pricing")

FR26: [MVP] Floor zones can be collapsed (manually, remembered per device); a collapsed zone still shows its tile count, per-state counts and alert count. (CAP-6; EXPERIENCE State Patterns "High device count")

FR27: [MVP] Staff start an open postpaid session for a profiled player or a guest; the start shows the group rate and active rule; the kiosk unlocks ≤ 2 s; start completes < 10 s from the first tap. (CAP-7; PRD §Sessions "Postpaid walk-in")

FR28: [MVP] The postpaid bill = minutes × rate with the branch rounding unit (default 15 min) and minimum (default 30 min), paused time excluded, one line per rate segment (transfer or rule flip), plus orders and tax. (CAP-7; LLD §Hub engines "Postpaid bill")

FR29: [MVP] A player can end a postpaid session via "I'm done" on the HUD; the PC locks with "Pay at counter". (CAP-7; Journeys §2)

FR30: [MVP] Guest sessions appear in reports as "Guest"; a guest can be converted to a profile (including at settlement) and conversion keeps the session history. (CAP-7; PRD §Players "Guest")

FR31: [MVP] A player or staff starts a prepaid session on a pack, wallet balance, block or pass that counts down; only plans valid for the device group are offered; a refusal returns a typed reason (e.g. "Balance ₹40. Top up at counter."). (CAP-8; PRD §Sessions "Prepaid")

FR32: [MVP] Partial pack use deducts minutes; expired packs are greyed and unselectable; a pack restricted to a group cannot start a device in another group. (CAP-8; PRD §Plans "Time pack"; §Players "Packs owned")

FR33: [MVP] A block session ends at its fixed time regardless of start; a pass is usable only inside its window (which may wrap midnight); a player holds at most one active pass. (CAP-8, CAP-14; PRD §Plans "Block", "Pass")

FR34: [MVP] Staff pause a session with a reason (Player stepped away · Hardware issue · Power cut · Other); paused time is not billed; resume restores the timer; the tile shows the reason. (CAP-9; PRD §Sessions "Pause / resume")

FR35: [MVP] Staff or player extend a session charged to wallet, pack or credit; each extension writes a ledger row; extension past branch closing time is refused. (CAP-9; PRD §Sessions "Extend")

FR36: [MVP] Transferring a session locks the old device and unlocks the new one ≤ 5 s; across groups the rate changes from that moment. (CAP-9; PRD §Sessions "Transfer")

FR37: [MVP] A group session puts several players on several devices under one bill with one payer; the bill splits by device minutes and the payer can change before settlement. (CAP-9; PRD §Sessions "Group session")

FR38: [MVP] A forced end requires manager PIN + reason, is audited, and the kiosk shows "Your session was ended by staff. Please see the counter." (CAP-9; PRD §Sessions "Forced end")

FR39: [MVP] Staff send remote PC actions (lock, unlock, message, restart, shutdown); the result shows ≤ 5 s or the tile is marked "Failed — retry". (CAP-9; PRD §Devices "Remote actions")

FR40: [MVP] Staff broadcast or per-seat messages reach the kiosk as a toast ≤ 2 s and are kept in the messages bell for the session. (CAP-9; PRD §Kiosk "Messages")

FR41: [MVP] The kiosk "Call staff" button raises a "CALL" alert badge on that tile plus the alert sound and confirms "Staff notified." to the player. (CAP-9; EXPERIENCE `call-staff-button`)

FR42: [v1.1] Staff take bookings that reserve a device or group slot with a deposit and no-show rules; the tile shows Reserved 15 min before the slot. (CAP-10; PRD §Sessions "Bookings")

FR43: [v1.1] Staff run tournaments with a bracket, an entry fee posted to a tournament income account, and devices blocked for the event. (CAP-10; PRD §Sessions "Tournaments")

#### Players

FR44: [MVP] Staff create a player profile (username + password, phone, name, optional email, DOB, notes, tags) in < 15 s; username and phone are each unique per branch. (CAP-11; PRD §Players "Profile")

FR45: [MVP] Phone lookup finds a player in 1 tap (results as you type, Enter picks top); no match shows "No player with that number." with Create profile and Guest. (CAP-11; EXPERIENCE `player-lookup`)

FR46: [MVP] Staff reset a player's password from the Players profile (kiosk "Forgot password?" directs players to the counter). (CAP-11, CAP-28)

FR47: [MVP] Each player has a wallet in integer minor units that is never negative; every change is a ledger row with actor; top-ups at the counter by cash, UPI (last-4 required) or card. (CAP-11; PRD §Players "Wallet")

FR48: [MVP] Owned packs list hours remaining and expiry on the player profile and kiosk account page. (CAP-11; PRD §Players "Packs owned")

FR49: [MVP] A manager enables credit for a player with a limit and reminder cycle (default 3, 7, 14 days). (CAP-12; PRD §Players "Credit")

FR50: [MVP] Every credit spend checks outstanding + new ≤ limit inside the transaction; a block shows "Credit limit reached, see counter."; a manager PIN overrides once per player per business day and is logged. (CAP-12; PRD §Players "Credit")

FR51: [MVP] Outstanding credit is aged 0–7, 8–14, 15+ days and shown first on the player profile and in lookup results. (CAP-12; PRD §Money "Credit collection")

FR52: [MVP] Credit reminders are sent on cycle days only, only with stored consent and only on paid tiers; Free shows an overdue list on the Hub instead. (CAP-12; Journeys §7)

FR53: [MVP] Credit auto-pauses at 15+ days overdue (unless the branch disables this rule) and reopens automatically on payment. (CAP-12; Journeys §7)

FR54: [MVP] Staff collect credit (partial allowed) by any method; the collection receipt references the settled transactions and shows outstanding after. (CAP-12; PRD §Money "Credit collection"; EXPERIENCE Receipts)

FR55: [v1.1] A branch rewards loyalty with points per rupee redeemable as wallet credit and a referral bonus on a referee's first paid session, rates configurable per branch. (CAP-13; PRD §Players "Loyalty, referrals")

FR56: [v1.1] A branch sells a monthly membership (recurring pass) with a WhatsApp renewal reminder. (CAP-13; PRD §Plans "Membership")

#### Plans and pricing

FR57: [MVP] A manager/owner sets an hourly postpaid rate per device group; a rate change affects new sessions only; a session only ever uses its device group's prices. (CAP-14; PRD §Plans "Hourly rate"; §Devices "Group-based pricing")

FR58: [MVP] A time pack has hours, price, validity days and allowed groups (e.g. 5 h, ₹300, 7 days, Basic only). (CAP-14; PRD §Plans "Time pack")

FR59: [MVP] A block is a fixed slot with price and start window; a pass is unlimited play within a time window (e.g. night pass 10 pm–6 am, wrapping midnight). (CAP-14; PRD §Plans "Block", "Pass")

FR60: [MVP] Console groups and packs are never shared with PCs; a "combo" is sold as two packs. (CAP-14; PRD §Decisions "Console pricing")

FR61: [MVP] A manager defines time-window pricing rules (weekday + hour range, per group, rate override or percentage), only one rule applies at a time and never stacks. (CAP-15; PRD §Dynamic pricing "Time-window rules"; LLD rate resolution)

FR62: [MVP] Date rules (specific dates or ranges) outrank weekly rules; conflicting rules are flagged on save. (CAP-15; PRD §Dynamic pricing "Date rules")

FR63: [MVP] The active rule is shown on the floor view header, kiosk attract screen and plan picker; a rule that starts mid-session changes the rate from that minute (new segment). (CAP-15; LLD rate resolution)

FR64: [MVP] Coupons (code, discount type, validity, usage limit, allowed groups/plans) are applied at settlement and shown on the receipt. (CAP-15; PRD §Dynamic pricing "Coupons")

FR65: [MVP] A manual discount (percentage or amount) requires manager PIN + reason and appears in the audit log and shift report. (CAP-15; PRD §Dynamic pricing "Manual discount")

#### POS, money and tax

FR66: [MVP] The POS product grid shows categories, images and favourites first, with one combined "Search or scan barcode" field; a scanned item sells in 1 scan + 1 tap; a product sale completes < 10 s. (CAP-16; PRD §POS "Product grid")

FR67: [MVP] A cart attaches to a running session so it settles with the bill; the order shows "ordered" on the kiosk HUD. (CAP-16; PRD §POS "Cart to session")

FR68: [MVP] A bundle has its own price independent of components, decrements its components on sale, and a component shortage blocks the sale unless overridden by manager PIN. (CAP-16; PRD §Inventory "Bundles")

FR69: [MVP] Out-of-stock and expired items are disabled in the POS grid and kiosk shop. (CAP-16, CAP-20)

FR70: [MVP] Settlement accepts any mix of cash, UPI, card, wallet and credit with a live remainder; Settle is blocked until the remainder is zero or moved to credit (profiled, credit-enabled player only). (CAP-17; PRD §POS "Split payment")

FR71: [MVP] A UPI row cannot be saved without the last 4 digits of the transaction reference; card requires the terminal reference; both print on the receipt and Z-report; settling never needs the internet. (CAP-17; PRD §POS "UPI and card")

FR72: [MVP] Receipts go by thermal print (ESC/POS 58/80 mm over USB, LAN or Bluetooth), WhatsApp (only with consent) or none; a missing printer never blocks a sale. (CAP-17; PRD §POS "Receipts"; §Configuration "Receipts")

FR73: [MVP] A cash sale kicks the cash drawer via the printer and the drawer event is logged; drawer-open count appears in the shift report. (CAP-17; PRD §Money "Cash drawer")

FR74: [MVP] End, Settle and Collect commit in one tap and show Undo for 5–10 s; Undo restores the prior state exactly; no Undo once a receipt has printed (Refund instead). (CAP-17; AD-13; EXPERIENCE `undo-toast`)

FR75: [MVP] Refund/void requires a reason, manager PIN and the original invoice reference, optionally returns stock, and shows as a negative line in the shift report. (CAP-17; PRD §POS "Refund / void")

FR76: [MVP] A GST-registered Indian branch charges GST per item with HSN, inclusive or exclusive per branch, prints GSTIN on the receipt and issues B2B invoices with the customer GSTIN. (CAP-18; PRD §Cross-cutting "Tax")

FR77: [MVP] Invoice numbers are sequential per branch per financial year in an epoch-segmented series (`<branch code>/<FY>/E<counter><tag>/<seq>`) and are never reused, including across standby promotion and restore. (CAP-18; AD-26)

FR78: [MVP] Tax is summed per rate, computed once per rate and rounded once (half-up) per invoice; non-GST tenants use a simple tax rate with the same series shape. (CAP-18; AD-26)

FR79: [MVP] Refunds issue credit notes that reference the original invoice number in the same series. (CAP-18; PRD §Cross-cutting "Tax")

FR80: [MVP] Pack and pass purchases are sales carrying their own GST rate and are reported separately from product sales. (CAP-18; PRD §Money "Plan payments")

FR81: [MVP] The branch exports a GSTR-1-format file; a sample month reconciles. (CAP-18; PRD §Cross-cutting "Tax")

FR82: [MVP] Staff open a shift with a confirmed float bound to their staff login. (CAP-19; PRD §Money "Shift")

FR83: [MVP] Only recorded sales, deposits (with slip reference), expenses (category, amount, paid from drawer or owner, receipt photo) and refunds change expected drawer cash. (CAP-19; PRD §Money "Cash deposit", "Expenses")

FR84: [MVP] Shift close accepts a per-denomination count or a single total; expected cash is hidden until counted cash is entered; variance = counted − expected, signed (+ over, − short), with a one-tap breakdown (float, cash sales, refunds, deposits, expenses). (CAP-19; PRD §Money "Shift")

FR85: [MVP] A variance over the branch threshold (default ₹100) requires manager PIN + a reason (Counting error · Unrecorded expense · Change float mismatch · Other + free text), is audited, and alerts the owner; within threshold closes with no PIN. (CAP-19; PRD §Money "Shift")

FR86: [MVP] A one-tap Z-report prints at shift close (sales by type, payments by method, expected vs counted, variance, waste, expenses, deposits, refunds, drawer opens); closing cash becomes the next float unless deposited; shift close completes < 10 s. (CAP-19; PRD §Money "Z-report")

FR87: [MVP] A product has name, category, price, tax rate, HSN, track-stock flag, image, barcode and kitchen flag; products import by CSV with dry-run. (CAP-20; PRD §Inventory "Products")

FR88: [MVP] Stock-in records quantity, cost, free-text supplier and expiry date and updates the weighted average cost. (CAP-20; PRD §Inventory "Stock-in")

FR89: [MVP] Waste records quantity, reason (spoiled, broken, free) and staff; a waste report shows totals by reason and value. (CAP-20; PRD §Inventory "Waste")

FR90: [MVP] Expiry alerts fire N days before; expired stock is listed daily and excluded from sale until written off. (CAP-20; PRD §Inventory "Expiry")

FR91: [MVP] A per-product low-stock threshold alerts the Hub and owner; the alert clears when stock-in is recorded. (CAP-20; PRD §Inventory "Low stock")

FR92: [MVP] Stock decrements at served by default (configurable to at order); an order cancelled after preparing lands in waste. (CAP-20; Journeys §4)

FR93: [v1.1] A manager runs stock counts with a count sheet, variance per item, adjustment with reason and a variance-by-value report. (CAP-21; PRD §Inventory "Stock count")

FR94: [v1.1] A manager keeps supplier records and purchase orders; receiving against a PO creates stock-in rows. (CAP-21; PRD §Inventory "Suppliers and POs")

FR95: [v1.1] A branch keeps a petty-cash ledger. (CAP-21; PRD §Money "Refund approvals / petty cash")

FR96: [v1.1] Staff clock in/out tied to their shift; reports show hours per staff. (CAP-21; PRD §Users "Attendance")

#### Kitchen

FR97: [MVP] On Basic and above, kitchen-flagged items print a KOT ≤ 3 s of order with seat/device number, order id, time, items and modifiers; KOTs can be reprinted from the Hub; Kitchen is tier-locked on Free. (CAP-22; PRD §Kitchen "KOT")

FR98: [MVP] A kitchen display page runs on any LAN tablet browser (paired by code) with columns New → Preparing → Ready → Served; tap advances one stage with Undo; new tickets chime and carry a persistent "NEW" badge until advanced. (CAP-22; PRD §Kitchen "Kitchen display"; AD-42)

FR99: [MVP] The kitchen display survives LAN loss (tickets stay, advances allowed and synced on reconnect); only the counter/Hub can cancel (with reason). (CAP-22; EXPERIENCE `kitchen-ticket`)

FR100: [MVP] A printer offline shows a Hub alert badge while the kitchen display still receives orders. (CAP-22; Journeys §4)

FR101: [MVP] Order status reaches the kiosk HUD ≤ 5 s and the floor tile shows ORDER/READY per the branch fulfilment setting (deliver to seat or pick up at counter). (CAP-22; PRD §Kitchen "Order status to player")

FR102: [v1.1] Kitchen items route to prep stations (e.g. bar vs kitchen) with a separate ticket per station, and item-level timing is recorded. (CAP-23; PRD §Kitchen "Prep stations")

#### Staff, configuration and reports

FR103: [MVP] Roles: built-in Owner, Manager, Counter, Kitchen, Kiosk-device plus custom roles composed from a fixed ~40-permission catalogue grouped by module, each allow/deny; every intent maps to exactly one permission. (CAP-24; PRD §RBAC "Roles", "Permission catalogue")

FR104: [MVP] An owner-created staff user receives a single-use, expiring setup code; the staff member sets their own PIN and password at the Hub on first sign-in; the owner never sets or sees them; user changes reach the branch within one poll. (CAP-24; PRD §RBAC "Users"; AD-10)

FR105: [MVP] Manager PIN elevation is entered inline without leaving the current screen and the audit records both actors. (CAP-24; PRD §RBAC "PIN elevation"; AD-30)

FR106: [MVP] The Hub UI locks after 5 min idle (default, configurable); PIN unlock returns to the same screen. (CAP-24; PRD §RBAC "Session lock")

FR107: [MVP] Free tier is capped at 3 branch users (tier-lock beyond). (CAP-24; PRD §Tiers)

FR108: [MVP] The Hub produces offline reports: Shift/Z; daily sales by hour, group and product category; sessions (count, minutes, revenue by device and group, utilisation per hour); players (top spenders, new vs returning, credit aged); stock (value, movements, waste by reason, expiring); staff (sales, sessions, discounts, voids). (CAP-25; PRD §Reports)

FR109: [MVP] Every Hub report exports to CSV and PDF. (CAP-25; PRD §Reports "Export")

FR110: [MVP] The Hub audit log records every change with actor, time and before/after, filterable by actor, module and date. (CAP-25; PRD §Ops "Audit log")

FR111: [MVP] Branch settings: currency (ISO 4217), symbol position, number format, timezone; a tz or day-start change takes effect next business day. (CAP-26; PRD §Configuration "Currency and locale")

FR112: [MVP] Tax settings: GST on/off, GSTIN, default rates, inclusive/exclusive, invoice prefix (branch code locks once the first invoice exists). (CAP-26; PRD §Configuration "Tax"; AD-3)

FR113: [MVP] Billing rules: rounding unit, minimum billing, grace period and time-up mode ("clean up" or "staff handles") per branch. (CAP-26; PRD §Configuration "Billing rules")

FR114: [MVP] Receipt settings (printer, paper width 58/80 mm, logo, footer, WhatsApp template) and notification templates with no provider keys at the branch. (CAP-26; PRD §Configuration "Receipts", "Notifications")

FR115: [MVP] Opening hours per day (used by passes, pricing and offline alerts); order fulfilment (to seat / pick up); kiosk default volume; high-contrast theme per device; ads consent (Free). (CAP-26; EXPERIENCE IA Hub "Settings")

#### Kiosk

FR116: [MVP] One kiosk installer sets the Kiosk as the Windows shell for the player account (Explorer, Task Manager, Run, Settings blocked; Ctrl+Alt+Del and Win keys limited) so a player cannot reach the desktop in 30 min of trying. (CAP-27; PRD §Kiosk "Shell replacement")

FR117: [MVP] The kiosk enforces a launch allow-list; block-listed processes are killed on sight with "That app isn't allowed on this PC." and logged to the Hub; USB autorun is off. (CAP-27; PRD §Kiosk "Process control")

FR118: [MVP] Admin exit via hidden combo Ctrl+Alt+Shift+A + manager PIN (works with Hub down via cached verifier) or Hub remote unlock from the tile sheet drops to desktop for maintenance; both are logged. (CAP-27; AD-31)

FR119: [MVP] The game booster closes listed background apps, sets process priority and optionally switches power plan on launch; measured launch time is shown to staff. (CAP-27; PRD §Kiosk "Game booster")

FR120: [MVP] A player logs in at the kiosk with username + password in < 3 s on LAN, or staff start the session from the Hub; the error never says which field failed; 5 failures lock login for 60 s with a visible countdown and alert the Hub. (CAP-28; PRD §Kiosk "Login")

FR121: [MVP] Kiosk sign-up takes username, password, phone and name with no OTP; account creation is blocked until the single required, unticked consent box (Terms, Privacy, service messages) is ticked; duplicates are blocked by username and by phone with field-level errors; success creates the profile on the Hub and continues to the plan picker. (CAP-28; PRD §Kiosk "Sign-up")

FR122: [MVP] The kiosk attract screen shows branch logo, muted game video, current rates and active rule, with no ads, rendered from cache (media streams in after). (CAP-29; PRD §Kiosk "Attract screen")

FR123: [MVP] The kiosk shell offers Home · Games · Apps · Shop (Shop hidden on Free), an always-visible time chip, messages bell, call staff, volume control (resets to branch default at logout) and account menu (balance, Extend, Messages, Account, PC settings, Lock, Log out). (CAP-29; EXPERIENCE IA Kiosk)

FR124: [MVP] PC settings (mouse sensitivity, display resolution/refresh from the branch allowed list, audio output device, keyboard layout) save to the player profile and re-apply on any PC. (CAP-29; EXPERIENCE `account-menu`)

FR125: [MVP] Background video stops entirely while a game runs. (CAP-29; EXPERIENCE Interaction Primitives)

FR126: [MVP] The kiosk account page shows details, wallet & packs, order history (this branch) and change password. (CAP-29; EXPERIENCE `account-subnav`)

FR127: [MVP] The overlay HUD shows time left/elapsed and balance, never steals focus, hides in fullscreen-exclusive games (then flashes the taskbar and plays a sound at 5 and 1 min). (CAP-30; PRD §Kiosk "Overlay HUD")

FR128: [MVP] Warnings at 10, 5 and 1 min and on low balance (configurable per branch) appear as toast + sound and persist until dismissed or time-up; at ≤ 10 min the time chip gains a warning border and ⚠ glyph. (CAP-30; PRD §Kiosk "Warnings")

FR129: [MVP] Extend opens from Ctrl+Shift+E, the account menu, a warning toast or the grace lock; options are priced live by the Hub and charged to wallet, pack or credit; the change reaches the floor ≤ 2 s; refusals (closing time, credit limit, balance) keep the options open with Voice copy. (CAP-30; PRD §Kiosk "Extend")

FR130: [MVP] At zero, a grace countdown (default 2 min) runs over the still-visible game with Extend available and no data loss; then the branch time-up mode applies: clean up (end non-allow-listed processes, sign out launchers, clear temp/clipboard, reset volume, return to attract) or staff handles (lock held over paused game). (CAP-30; PRD §Kiosk "Time-up")

FR131: [MVP] On Basic and above a player orders food/drink from the seat: menu with images, live stock check, out-of-stock greyed, bundle short of a component refused, order attaches to the running session or is paid by wallet, no session → "Pay at counter", order reaches Hub POS and KOT ≤ 3 s, status toasts on the HUD. (CAP-31; PRD §Kiosk "Orders")

FR132: [MVP] The Hub holds the game catalogue (name, executable, launcher, category, age rating), edited once and applied to every kiosk. (CAP-32; PRD §Games "Library")

FR133: [MVP] The kiosk auto-scans Steam, Epic, Riot and common folders on first run (and daily), proposing entries the manager approves in bulk; the Hub matches executables against the cached global catalogue. (CAP-32; PRD §Games "Auto-scan"; AD-17)

FR134: [MVP] One-click launch from the kiosk game grid applies the booster and logs the launch with player and time. (CAP-32; PRD §Games "Launch")

FR135: [MVP] Game art/video comes only from the global curated catalogue via the Hub cache, else the launcher cover/exe icon, else a solid tile. (CAP-32; AD-17)

FR136: [MVP] At session end the kiosk signs out Steam, Epic, Riot, Discord and browser profiles and clears saved credentials; failures report to the Hub and badge the tile; the next player never sees previous accounts. (CAP-32; PRD §Games "Launcher logout")

FR137: [MVP] Hours per game per branch are recorded and feed owner dashboards and the kiosk top-10 "Most played here" carousel (last 7 days). (CAP-32; PRD §Games "Stats")

FR138: [v1.1] A branch lends café-owned launcher accounts from a vault: the kiosk injects login at launch, one account per concurrent seat, released at session end. (CAP-33; PRD §Games "Café-owned vault")

FR139: [v1.1] Players request games from the kiosk; requests appear on the Hub and request counts per game appear in reports. (CAP-33; PRD §Games "Game request")

#### Consoles

FR140: [MVP] Staff run consoles as timed devices from the Hub with the same postpaid, pack and pass rules; timer and warnings show on the console tile. (CAP-34; PRD §Console MVP "Session")

FR141: [MVP] Console time-up raises a tile "TIME UP" alert + sound until acknowledged; the audit records who ended the session; an optional 5-min WhatsApp/SMS goes only with consent. (CAP-34; PRD §Console MVP "Player alert", "End")

FR142: [v1.1] A Pi Zero 2 W or Pi 4 per TV (BOM < ₹4,000) pairs to the Hub by 6-digit code, shows its TV number, and is visible on the floor view ≤ 60 s. (CAP-35; PRD §Pi "Hardware", "Pairing")

FR143: [v1.1] The idle TV shows branch logo, TV number, muted game video, console rates and a "Scan to start" QR (encoding branch, TV and a short-lived token); no ads. (CAP-35; PRD §Pi "Idle screen")

FR144: [v1.1] Scanning the QR on café Wi-Fi opens a Hub LAN page with no login; requesting a plan raises a tile "REQUEST" badge + sound with the plan pre-selected; the player pays at the counter; staff start; the TV switches to the console ≤ 3 s; the phone then shows a live timer only (refused → "Ask at the counter."). (CAP-35; PRD §Pi "Start by QR")

FR145: [v1.1] At 5 and 1 min the TV switches to the Pi input for a 5-s warning then back; at time-up the TV locks at once ("Time's up — see the counter."), no grace, tile alert until acknowledged, CEC standby or smart plug per branch. (CAP-35; PRD §Pi "Time and warnings", "Time-up")

FR146: [MVP] After console time-up the tile shows "CHECK CONTROLLERS" until staff mark Returned or Missing, logged against the session. (CAP-35; PRD §Pi "Controller check")

FR147: [v1.1] Hub sends console power on/off, input switch and TV message with the result ≤ 5 s; the Pi keeps the resolved timings and enforces warnings/time-up while the Hub is unreachable. (CAP-35; PRD §Pi "Remote commands"; AD-12)

#### Owner app

FR148: [MVP] On Basic and above, Today shows per branch: open/closed, sessions live, revenue so far, cash in drawer, per-shift variance, staff on shift, last sync, loading < 2 s from the Hub-published summary, plus a "Needs attention" list. (CAP-36; PRD §Owner "Today")

FR149: [MVP] Every owner figure shows its data age within tier cadence (Basic ≤ 15 min, Premium ≤ 5 min) on 99% of opens; an offline branch shows "Branch offline" with growing ages and its queued commands, never a broken state. (CAP-36; AD-35)

FR150: [MVP] Owner reports — revenue (day/week/month/custom; by branch, type, payment method), cash reconciliation (shift-by-shift, variance highlighted), stock (levels, low-stock, waste, expiring; per branch and total) and credit (outstanding aged, tap to send reminder) — match Hub Z-report totals exactly and export as CSV/PDF. (CAP-36; PRD §Owner "Dashboards and reports")

FR151: [MVP] Live view (Premium, or Basic + Tunnel add-on) is a read-only second-by-second floor view through the Tunnel; locked with tier-lock otherwise. (CAP-36; AD-43)

FR152: [MVP] On Free the Owner app shows only Subscription, pairing and support; everything else shows tier-lock. (CAP-36; PRD §Owner app)

FR153: [MVP] Owners receive alerts by web push (iOS only when installed) and WhatsApp, with per-tenant thresholds, for branch offline (no heartbeat/upload 30 min in opening hours), cash variance over threshold (branch, staff, amount, reason), low stock, credit overdue, large refund or discount; alerts list under "Needs attention" / Alerts. (CAP-37; PRD §Owner "Alerts")

FR154: [MVP] Owners send commands for prices/plans/rules, products/bundles, staff users/roles (one branch or all), revoke device/Hub, send credit reminder, and backup now; only valid, entitled commands and targets can be sent. (CAP-38; PRD §Owner "Commands")

FR155: [MVP] Before sending, a preview lists the target branches with online/offline status; each target shows a command row: Queued on this device → Waiting for branch → Applied / Rejected (reason + Retry) / Expired after 7 days (Resend). (CAP-38; EXPERIENCE `command-row`)

FR156: [MVP] Each command applies exactly once; the owner's change wins over a prior local edit, the Hub toasts "<Entity> updated by owner" and the audit keeps both changes. (CAP-38; AD-5, AD-39)

FR157: [MVP] The floor view and kiosk attract show a new rate ≤ 1 min after apply; delivery p95 < 2 min when the branch is online. (CAP-38)

FR158: [MVP] Owners search a branch's change history in the cloud by branch, actor and module; the first search of a month shows "Preparing audit…" and returns first results < 10 s; failure shows Retry; player data appears as ids only. (CAP-39; AD-37)

FR159: [v1.1] Scheduled email/WhatsApp daily or weekly reports arrive by 8 am local. (CAP-40; PRD §Owner "Exports")

FR160: [v1.1] Deeper dashboards: player retention cohorts (weekly chart), device utilisation heatmap per group per hour, staff sorted by variance, branch comparison. (CAP-40; PRD §Owner "Players", "Devices", "Staff")

FR161: [v1.1] Additional owner logins with read-only or full roles (owner-login add-on). (CAP-40; PRD §Owner "Team")

FR162: [v1.1] Tenant-wide pricing rules pushed to all branches with branch override flagged in the owner app, and happy-hour automation. (CAP-40; PRD §Dynamic pricing "Tenant-wide push")

FR163: [v1.1] Refunds over a threshold wait for an owner approval command and show as pending alerts. (CAP-40; PRD §Money "Refund approvals")

FR164: [v1.1] Owner sends a message to all kiosks of a branch as a command. (CAP-40; PRD §Owner "Commands")

FR165: [v1.1] Device health (last seen, kiosk version, GPU/RAM) with a device-offline alert during opening hours. (CAP-40; PRD §Devices "Health")

#### Resilience and data

FR166: [MVP] Every Hub, Kiosk and Kitchen feature works with the WAN down; the Hub shows internet/cloud reachability (chip naming which) and nothing else changes; cloud features queue and resume. (CAP-41; PRD §Ops "Offline indicator")

FR167: [MVP] Paid Hubs upload change-log batches and the summary at tier cadence and daily rollups after close; gaps are detected by heartbeat and repaired by resend. (CAP-41; AD-4, AD-35)

FR168: [MVP] With the Hub unreachable, a kiosk continues the session on the last known time, enforces time-up locally, never locks the player out, disables login/orders/Extend with "Counter is reconnecting — try again shortly.", shows "Reconnecting…", and reconciles on reconnect. (CAP-41; PRD §Kiosk "Hub unreachable"; AD-12)

FR169: [MVP] A paid branch pairs a LAN standby as its own Hub; promotion by inline manager PIN completes < 10 s and kiosks reconnect by themselves. (CAP-42; PRD §Ops "Standby")

FR170: [MVP] If promotion fails, a manager can force promote with PIN after the warning "Data since the last sync may be lost. Promote anyway?"; force promote is audited and the owner alerted. (CAP-42; PRD §Ops "Standby")

FR171: [MVP] A returning old main Hub locks itself ("This PC is no longer the main Hub", offers "Make this the standby") and never serves kiosks; exactly one main Hub ever serves devices. (CAP-42; AD-11, AD-38)

FR172: [v1.1] A Premium branch keeps an off-site standby that follows and promotes over STUN/TURN with the same guarantees as the LAN standby. (CAP-43)

FR173: [MVP] One-tap full DB export to file or USB on every tier (1 GB < 10 s) and restore from file with confirmation (restore opens a new epoch). (CAP-44; PRD §Ops "Backup export")

FR174: [MVP] Cloud backup by tier: Free uploads nothing (upgrading uploads a full snapshot first); Basic 15-min batches, 1-year retention; Premium 5-min batches, 3-year retention; a yearly GST archive kept 8 years on any paid tier. (CAP-44; PRD §Tiers "Backup")

FR175: [MVP] The Hub header shows cloud backup status on paid tiers (last upload, last summary, pending batches); the owner can trigger "Backup now" by command. (CAP-44; PRD §Ops "Cloud backup status")

FR176: [v1.1] Hubs check for updates daily and install on restart with rollback; staged rollout by percentage from the control plane; kiosks and Pis update from the Hub's signed cache between sessions only, never during an open session. (CAP-45; PRD §Ops "Updates")

FR177: [MVP] Owners create and rename branches, set timezone, and issue pairing and standby codes. (CAP-46; PRD §Owner "Branches")

FR178: [MVP] Subscription per branch shows tiers Free ₹0, Basic ₹200, Premium ₹1,999 per branch per month GST-exclusive, yearly at 10× monthly, and add-ons (+20 devices ₹50, Tunnel ₹200, additional owner login ₹50), with billing-cycle switch, invoices, payment method and cancel. (CAP-46; PRD §Owner "Subscription"; §Tiers)

FR179: [MVP] One Razorpay subscription per branch with mandatory autopay (card or eNACH for renewals over ₹15,000 incl. GST); mid-cycle changes are prorated; invoices itemise GST. (CAP-46; AD-20)

FR180: [MVP] Full refund within 7 days of first payment; no partial refunds; cancel keeps the plan to period end, then the branch drops to Free. (CAP-46; PRD §Tiers "Refunds")

FR181: [MVP] The PPP price band (MY, SG, US) is set from the billing country and stored on the subscription so later price-table changes do not reprice. (CAP-46; PRD §Tiers "Regional pricing")

FR182: [MVP] An upgrade applies within one heartbeat; a lapse never ends live sessions or open orders and over-cap devices go read-only (newest pairing first). (CAP-46; AD-29)

FR183: [MVP] A Free Android Hub upgrading to paid migrates its database to a Windows Hub by snapshot export/import with no data loss. (CAP-46; LLD §Android Hub "Upgrade path")

FR184: [MVP] The Free Android Hub shows an always-visible AdMob banner that never covers the floor grid, cart or settle bar. (CAP-47; PRD §Ad placement rules)

FR185: [MVP] An interstitial shows only when staff return after the idle lock, is skippable after 5 s, and never after a settle or over an open cart or tile sheet. (CAP-47; PRD §Ad placement rules)

FR186: [MVP] Ads exclude adult, gambling and alcohol categories and the owner can block more; a consent view exists; upgrade removes ads within one heartbeat; a lapsed branch resumes ads only after entitlement expiry; the Windows build has no ad SDK. (CAP-47; PRD §Ad placement rules)

FR187: [MVP] Credit reminders, low balance, session ending, console 5-min alert and receipts are sent by the cloud from Arena 360's Gupshup account (SMS fallback) only with stored sign-up consent, exactly once, never for an undone action, queued while the branch is offline, with versioned templates; no player sends on Free. (CAP-48; AD-27)

FR188: [MVP] A reseller/installer applies (name, phone OTP, email, city, business, type, cafés served) and sees only "Under review" with a 3-step tracker until approved. (CAP-49; PRD §Partners)

FR189: [MVP] Approval is sent by WhatsApp with the partner-portal link (partners.arena360.in, separate login); KYC (PAN, bank, optional GSTIN) is required before the first payout. (CAP-49)

FR190: [MVP] A referral link/code attributes the referred tenant at sign-up; attribution is immutable after first payment. (CAP-49; AD-19)

FR191: [MVP] Partners earn 10% of paid subscription revenue net of refunds each month the referred tenant is active; payout by the 10th, ₹500 minimum rolls over, 2% TDS shown, GST-registered partners upload an invoice or are held; refunds after payout claw back. (CAP-49; AD-19)

FR192: [MVP] The partner portal shows referral link/code, referred cafés, accrued, pending, paid, TDS and invoice status, and no tenant operational data. (CAP-49; AD-18)

FR193: [MVP] Arena 360 staff use an admin app behind staff SSO to curate the global game catalogue; media is flash-screened before publishing. (CAP-50; AD-17, AD-21)

FR194: [MVP] The admin app handles partner approval, KYC review, payout marking after manual transfer, and 7-day refunds; every admin action is audited. (CAP-50; AD-19, AD-21)

FR195: [MVP] Support sees branch data only via the in-app diagnostics bundle (< 5 MB, no player PII, sent only on request) or audit search with a recorded reason. (CAP-50; AD-21)

FR196: [MVP] The public site has Home (install, pair, play story), Product (Hub, Kiosk, Console, Owner with 90-s demo videos), Pricing (Cafés · Partners tabs, Monthly · Yearly toggle, add-ons, offline/cancellation/refund FAQ), Get started, Downloads (installers, Pi image, requirements, checksums), Help centre (setup, troubleshooting, printer and CEC compatibility), FAQ and Legal (terms, privacy, data processing, ad policy). (CAP-51; PRD §Marketing site)

FR197: [v1.1] An in-browser sandbox demo Hub runs with sample data (same web UI, in-memory DB). (CAP-52; PRD §Marketing "Demo")

FR198: [v1.1] Compare pages vs ggLeap, Smartlaunch, iCafe (offline, price, hardware, migration) and status and changelog pages. (CAP-52; PRD §Marketing "Compare", "Status and changelog")

FR199: [MVP] The full branch DB and all reports are exportable any time on every tier, documented and tested. (CAP-53; PRD §Cross-cutting "Data ownership")

FR200: [MVP] Player erasure makes PII unrecoverable in Hub, standby, backups and snapshots and stays erased after restore or promote. (CAP-53; AD-24)

FR201: [MVP] Consent is stored per player with policy version and timestamp; logs carry no PII beyond player id. (CAP-53; AD-24)

FR202: [MVP] Every surface is usable by keyboard, at high contrast and without relying on colour or sound alone: ≥ 44 px targets (incl. Android phone Hub), HC theme per device on Kiosk/Hub/Kitchen/Pi/Owner, counter flow without a mouse, text/glyph on every state, persistent badge for every alert sound, web screen-reader baseline, light/dark/system on Owner and web, no hard-coded strings. (CAP-54; PRD §Cross-cutting "Accessibility")

### NonFunctional Requirements

NFR1: Offline reliability — every Hub, Kiosk and Kitchen feature works with the WAN down; an 8-hour WAN-outage soak with 20 kiosks (1,000 sessions, 500 orders) loses zero transactions and uploads/summary/rollups catch up within 10 min of WAN return. (PRD §Cross-cutting "Offline"; CAP-41; LLD §Test plan "Offline soak")

NFR2: Footprint — Rust Hub service idle < 50 MB RAM, Hub UI (WebView2) < 250 MB, Kiosk < 80 MB, no GPU use; checked by per-release benchmark on reference hardware (2015 i3 + HDD) in CI. (AD-33; PRD §Cross-cutting "Performance")

NFR3: LAN latency — UI actions < 100 ms server-side, < 2 s end to end including kiosk unlock; floor state ≤ 2 s; KOT ≤ 3 s; console input switch ≤ 3 s; remote device action result ≤ 5 s; staff message ≤ 2 s; device timeout 5 s shows "Hub slow". (Spine §Operational Envelope "Branch performance"; AD-25)

NFR4: Throughput — 60 devices per Hub, 5 intents/s sustained and 50/s burst on a 2015 i3 + HDD; floor view of 60 devices without lag. (Spine §Operational Envelope; PRD §Cross-cutting "Performance")

NFR5: Reporting compute — summary compute < 50 ms; daily rollup at close < 5 s on reference hardware and on a 3 GB Android device. (AD-33)

NFR6: Counter speed — session start, product sale and shift close each < 10 s from first tap; kiosk login < 3 s on LAN; profile create < 15 s; CSV import of 1,000 rows < 30 s; 1 GB DB export < 10 s. (SPEC Success signal; PRD §Success metrics)

NFR7: Owner experience — dashboard < 2 s from files (first render < 2 s on a 3-year-old phone over 4G); data age within tier cadence (Basic ≤ 15 min, Premium ≤ 5 min, or live) on 99% of opens; audit first query < 10 s with progress state; command delivery p95 < 2 min when branch online; Owner API 99.5% monthly availability. (Spine §Operational Envelope "Owner NFRs"; LLD §Owner app "Sizes")

NFR8: Failover — standby promotion < 10 s measured from promote confirmation to first accepted intent; kiosks reconnect automatically (mDNS re-resolve, jittered backoff 2–30 s). (Spine §Operational Envelope; AD-11)

NFR9: Durability — zero committed transactions lost on Hub crash (SQLite WAL + fsync on commit); at most one upload interval lost on disk failure without a standby. (Spine §Operational Envelope; HLD §NFR "Data loss")

NFR10: Consistency — wallet, stock and credit never negative; one active/paused/grace session per device; order totals balance; change log gap-free per epoch and replay reproduces the DB; duplicate idempotency keys apply once; Owner API sum-of-rollups equals Hub Z-report totals exactly over 30 synthetic days. (HLD §NFR "Consistency"; LLD §Test plan "Property")

NFR11: Security — Hub outbound-443 only, no inbound ports; every LAN client authenticated by class; device tokens revocable within one heartbeat (immediately on LAN); signed (EdDSA) entitlements; RBAC checked inside `apply()`; audit on every write; whole-DB SQLCipher opt-in deferred. (PRD §Cross-cutting "Security"; AD-6, AD-10, AD-42; HLD §Security)

NFR12: Kiosk hardening — a player cannot reach the desktop in 30 minutes of scripted and manual escape attempts on Windows 10 and 11; launcher logout leaves no residual login or credential; external pen test on kiosk lockdown passed before GA. (CAP-27, CAP-32; LLD §Test plan "Kiosk lockdown", "Launcher logout")

NFR13: Privacy (DPDP) — player PII encrypted per player and crypto-shredded on erasure; batches, snapshots and audit caches hold ciphertext only; logs carry no PII beyond player id; diagnostics bundle < 5 MB with no player PII, uploaded only on "send"; no card data anywhere. (AD-24; CAP-53)

NFR14: Data residency — control plane and `gz_control` hosted in India (Mumbai); branch files on R2 under a per-branch prefix. (PRD §Cross-cutting "Hosting and residency"; AD-9)

NFR15: Accessibility — contrast floors 4.5:1 text and 3:1 non-text/large text on every theme; ≥ 44 px targets on every surface; full keyboard operation of the counter flow; web screen-reader baseline (landmarks, labels, live regions); WCAG 3.3.8 auth alternatives; colour and sound never the only cue. (CAP-54; DESIGN §Colors; EXPERIENCE §Accessibility Floor)

NFR16: Localisation — English only at launch; all UI strings externalised in one message catalogue with a lint that fails release builds on hard-coded strings; numbers, dates and currency formatted by locale; currency per branch (ISO 4217); layouts tolerate +30% string length. (PRD §Cross-cutting "Languages"; DESIGN §Typography)

NFR17: Compatibility — Kiosk on Windows 10 1803 to 11, 64-bit, 4 GB RAM (WebView2 Evergreen); Windows Hub on Windows 10+; Android Hub on Android 9+, 3 GB RAM, 20 GB free (installer checks); any ESC/POS printer; Hub accepts Kiosks from the previous two releases; no Windows 7. (HLD §NFR "Compatibility"; PRD §Windows support; AD-8)

NFR18: Hardware support — ESC/POS printers (USB, LAN 9100, Bluetooth), cash drawer via printer kick, USB barcode scanners (keyboard wedge), Pi Zero 2 W and Pi 4, HDMI-CEC TVs, Tuya/Tapo local-API smart plugs; compatibility list and "tested device" badge in the help centre; CEC best-effort, plug as guarantee. (PRD §Cross-cutting "Hardware"; §Console)

NFR19: Upgrade safety — Hub/Kiosk/Pi updates never install during an open session; standby updates before primary; staged rollout; rollback is a roll-forward hotfix and is tested each release. (PRD §Cross-cutting "Updates"; AD-8, AD-32)

NFR20: Cost — marginal infra < ₹10 per paid branch per month on average (≈ ₹6 Basic, ≈ ₹12 Premium); Free tenant ≈ ₹0 cloud; upload cadence (Basic 900 s, Premium 300 s, floor 30 s) is the cost lever with a per-tenant R2 PUT cost guard. (PRD §Goals 5; Spine §Operational Envelope "Cost")

NFR21: Cloud resilience — branches unaffected by any cloud outage; `gz_control` backed up nightly off-provider; DR = rebuild K8s workloads from code + restore control DB, RTO 4 h. (Spine §Operational Envelope "DR")

NFR22: Licence offline tolerance — licence cached 7 days and evaluated against trusted time (cloud time + monotonic elapsed); wall-clock skew > 5 min alerts staff; unlimited offline duration on Free. (AD-10, AD-44; HLD §NFR "Offline duration")

NFR23: Abuse limits — Cloudflare per-IP limits on public/auth endpoints; per-tenant limits for Owner API, presign, audit builds and command posts; pairing 5 attempts per code and 20 per IP per hour; kiosk login lockout; phone page rate-limited per TV. (AD-23; HLD §Security "Abuse")

NFR24: Retention — Hub `changes` pruned 90 days after upload; batches/snapshots/rollups 1 year (Basic) or 3 years (Premium); GST archive and GST money records 8 years regardless of tier; audit cache 24 h. (AD-36)

NFR25: Observability — every write traceable to a typed actor; cloud OpenTelemetry traces with tenant/branch to Grafana Cloud, crashes to Sentry (PII scrubbed); metrics for summary age, rollup missing days, builder queue depth, command backlog, heartbeat gaps, webhook failures, R2 PUTs per tenant. (Spine §Operational Envelope "Observability"; LLD §Error codes, events and observability)

NFR26: Rendering performance — `backdrop-filter`/live blur banned on every platform; no autoplay audio; background video always muted and stopped while a game runs; Sora bundled with no runtime font/CDN fetch on branch surfaces. (DESIGN §Elevation; Spine §Consistency Conventions)

NFR27: Business success metrics instrumented — 80% of branches reach first paid session < 60 min after sign-up; 20% free-to-paid in 90 days; paid churn < 3%/month; median shift variance < ₹50 after 30 days; < 2% of branches raise a ticket per month; Free ad fill 40% in year one (requires onboarding telemetry, Hub timing logs, control-plane metrics). (SPEC Success signal; PRD §Success metrics)

### Additional Requirements

> **STARTER / SCAFFOLD — Epic 1 Story 1.** The architecture specifies no third-party starter template; it is a greenfield monorepo whose shape is fixed by the spine. Epic 1 Story 1 must scaffold it: **one monorepo, one package manager** containing (a) a **Rust workspace** with crates `arena-proto` (protocol types, error codes, permission + command catalogues, file schemas), `arena-store` (all SQL, migrations; rusqlite, libsql only for `gz_control`), `arena-core` (domain + `apply()`, injected `Clock`, rollup/summary engines), `arena-api` (LAN WS/HTTP/KDS/phone page), `arena-tasks` (background tasks), `arena-replay` (shared replayer), `arena-cloud` (one binary, roles control/command/builder/jobs), `arena-hub` (bin), `arena-kiosk-core` + kiosk Windows service, `arena-pi` (aarch64 agent); (b) **Tauri 2.12.5 apps** (CLI 2.12.4, updater plugin 2.14.0): Windows Hub, Android Hub (Tauri 2 mobile), Kiosk; (c) **SvelteKit 3.1 / Svelte 5.57 apps**: Hub UI, Kiosk UI, Kitchen display (`adapter-static`), Owner PWA, partner portal, admin app, onboarding (`adapter-cloudflare`, Workers static assets); (d) **Astro 7.4 + @astrojs/svelte 9.1** marketing site; (e) **one shared UI package** (design tokens from DESIGN.md, shared components, message catalogue, generated types); (f) **generated TypeScript types** from `arena-proto` via ts-rs (one chosen at build start) with a CI freshness gate. Pin Rust toolchain and Node 22.23.4+ at build start; Vite 8, TypeScript 6. (AD-15, AD-41; Spine §Design Paradigm layer table, §Stack; LLD §Hub module map)

**Core invariants (build substrate)**

- Single writer per branch: only the Hub primary holding the current lease epoch writes; lease renews every 3 s, expires after 15 s; every `apply()` re-reads the epoch and fails `NOT_PRIMARY`; standby is read-only; no CRDT/multi-master. (AD-1)
- One mutation path: every state change is an intent through `apply()` in one `BEGIN IMMEDIATE` (permission → epoch → TTL/dedupe → `base_rev` → validate → mutate → stamp → append `changes` → commit → emit), serialised by a single Writer actor; no I/O but SQLite inside the transaction; change log append-only and is the audit log; typed `actor{kind,id}`, `intent_id`, `elevated_by`, stamped `business_day`/`fy`. (AD-2)
- Data ownership: Hub writes branch rows; cloud writes only `gz_control` and R2 objects it owns; surfaces write only intents/commands; `branch_code` assigned by control plane, unique per tenant, locked after first invoice. (AD-3)
- Data conventions: ULIDs minted by writer; `branch_id` on every branch row; money in integer minor units with ISO 4217 code; rates in basis points; UTC ms timestamps; `rev` on mutable rows; FY Apr–Mar label `26-27`; `days_mask` bit 0 = Monday; typed `AppError{code,message,field?}` with stable codes, user text by code per locale never over the wire. (AD-7)
- Undo = compensating reverse intent `domain.verb.reverted` within the UX window, same user/device, carrying forward `rev` (`STALE` on mismatch), refused after any irreversible effect (receipt printed, drawer kicked, invoice issued, notification sent); every undoable intent defines its reverse. (AD-13)
- Idempotent offline-queued intents: client ULID `idempotency_key` at user action, TTL ≤ 7 days, `base_rev` on edits; dedupe records 30 days; `EXPIRED` past TTL; queues replay in creation order. (AD-14)
- Optimistic row versions: `rev` incremented by `apply()`; `STALE` on mismatch; owner commands always apply and list the overwritten change. (AD-39)
- Invariants enforced in `apply()`: one active session per device; wallet ≥ 0; credit outstanding ≤ limit unless override; stock ≥ 0 for tracked; totals balance with tax once per rate; UPI 4-digit reference. (LLD §Hub domain schema "Invariants")

**Infrastructure and deployment**

- Control plane: `arena-cloud` one Axum binary with roles control, command, builder, jobs as Kubernetes workloads on the existing Hostinger VPS in Mumbai (single node at launch; builder may move to a second node). (Spine §Operational Envelope "Infra / residency"; HLD §Deployment)
- Turso (libSQL) Mumbai holds only `gz_control` (tenants, branches, licences, `branch_fence`, `commands`, `builder_jobs`, billing, partners, catalogue, rate limits, send ledger, push subscriptions, admin audit, KEK escrow); no per-branch DBs; tenant/branch scope only from the authenticated session. (AD-9)
- Cloudflare R2 per-branch prefix `tenants/<t>/branches/<b>/` with `changes/`, `snapshots/`, `state/latest.json`, `rollups/`, `gst-archive/<fy>.zst`, `audit-cache/` (24 h lifecycle), `keys/player-keys.bin`; shared prefix for catalogue media (CDN) and signed installers. (AD-36)
- All web apps on Cloudflare Workers static assets (not Pages); admin behind Cloudflare Access; Cloudflare edge for TLS, per-IP limits, Tunnel + Access, TURN, CDN. (AD-15, AD-21; HLD §Deployment)
- Environments: dev (local cloud binary, SQLite for Turso, MinIO for R2, seeded tenant); staging (separate namespace/node, Turso org, R2 bucket, `api.staging.arena360.app`, internal 10-PC test café + two beta cafés); production `api.arena360.app` behind Cloudflare; Owner app at `app.arena360.app`; partner portal `partners.arena360.in`. (Spine §Operational Envelope "Environments")
- Network topology: Hub outbound-443 only; devices ↔ Hub over LAN WebSocket `:7400` and HTTPS/HTTP `:7401`; provider webhooks terminate in the cloud only. (AD-6)

**Sync, files and reporting**

- Change-log batches to R2 by presigned PUT (10-min URLs, `POST /presign` returns batch + summary URLs); key `changes/<epoch>/<first:012>-<last:012>.zst`; msgpack header (epoch, fork, sha256) + full-row maps, zstd, ≤ 5,000 rows; upload every `upload_interval_s` (floor 30 s) or 500 pending rows; fork declaration on first batch of a new epoch; heartbeat-based gap detection with `resend{from,to}`; no manifest/ingest; daily snapshot after close; standby bootstrap by LAN snapshot. (AD-4)
- Owner reporting only from Hub-computed `state/latest.json` (every `summary_interval_s`) and `rollups/<date>.json` (closing + 30 min or last shift close), written via system intents `summary.publish`/`rollup.publish`; rollup `schema` integer, cloud accepts N and N−1 (`ROLLUP_SCHEMA_OLD`), Hub regenerates old days (oldest first, max 10/min); Owner read API `GET /branches/:id/live` and `/summary?range=` (in-memory sum, 30 s cache, ETag); Free publishes nothing. **Rollup schema v1 field list must be signed off before the Owner dashboards epic.** (AD-35; Spine §Deferred)
- On-demand audit builder: `POST /branches/:id/audit/build {month}` → `builder_jobs`; replays nearest snapshot + month batches with `arena-replay` honouring fork points; fixed query catalogue in `arena-proto`; `AUDIT_BUILDING` / `AUDIT_BUILD_FAILED`; 50 GB LRU. (AD-37)
- Cloud API contract: all web surfaces reach data only via versioned `/v1` HTTPS JSON API; browser auth HttpOnly Secure SameSite cookie with rotating refresh; Hub/device auth bearer device token + licence JWT; DTOs in `arena-proto`. (AD-40)

**Commands, epochs and failover**

- Command mailbox: single `commands` table in `gz_control` with relay cursor `id` and unique `(branch_id, idempotency_key)`; Hub polls every 60 s (or on poke) `?after=<cursor>` with epoch, applies once (deduped by `applied_commands`), posts result with epoch; lifecycle `pending → fetched → applied | rejected | expired`, fetched reverts after 10 min, expiry 7 days; `schema_version` negotiation (`UNSUPPORTED`, `BRANCH_UPDATE_REQUIRED`); `branch: all` fans out under one `group_id`; owner wins with "updated by owner" event; billing never sends gate-changing commands. (AD-5)
- Manual promote/restore with epoch fencing: epoch `(counter, hub_instance_id)` issued by the cloud via CAS on `branch_fence` (provisional offline, confirmed on first heartbeat); every consumer rejects lower epochs; higher-epoch sighting self-locks the Hub; standby schema ≥ primary (`SCHEMA_BEHIND`); standby keeps its own reference cache; restore re-applies erasures. (AD-11, AD-38)
- Kiosk/Pi while Hub unreachable: Hub ticks carry `lock_after_ms` and `time_up_mode`; devices count down on monotonic clock and perform the local action; no device writes state. (AD-12)

**Identity, entitlement and security**

- Owner auth password + TOTP + backup codes, SMS OTP at sign-up/new device/fallback; staff password + PIN (Argon2id at Hub), staff credentials never travel in commands; pairing codes single-use short-expiry exchanged for 256-bit device token (argon2id hash) + first licence; revocation within one heartbeat. (AD-10)
- Licence: EdDSA-signed JWT, 7-day expiry, claims `{tenant_id, branch_id, tier, flags, features, upload_interval_s, summary_interval_s, rollup_schema, retention_days, limits, iat, exp}`; two verify keys on Hub, rotated with a release; tampering ⇒ Free limits (no lock). (AD-10)
- Entitlement enforcement in Hub `apply()` and pairing (device/user caps, kiosk orders, KOT/KDS, standby, owner-login add-on) and in the cloud API for owner-side gates; lapse path `exp` → `grace` → `lapsed`. (AD-29)
- Hub clock trust: heartbeat carries cloud time; `trusted_now` persisted; licence/grace/lapse use it; skew > 5 min alerts staff and is audited. (AD-44)
- RBAC: fixed permission catalogue in `arena-proto` + custom roles; PIN elevation verified at Hub and recorded in `elevated_by{kind: pin|mfa, id, mode, verifier_version}`; cached-verifier only for kiosk offline exit; staff surfaces lock after 5 min idle. (AD-30)
- LAN client classes: paired devices (kiosk, Pi, standby) by device token over TLS pinned to the Hub cert; kitchen display by scoped code-paired token over plain HTTP/WS; anonymous phone page may only call `console.plan_requested` and read its own timer, rate-limited per TV, plain HTTP; out-of-scope calls `FORBIDDEN`. (AD-42)
- LAN protocol envelope: every frame `{v, kind, rid}`; first frame `hello{credential, class, versions, capabilities}`; 5 s device timeout ("Hub slow"); unknown fields ignored; state pushed over WS, no polling. (AD-25)
- Kiosk privilege split: LocalSystem Windows service enforces lockdown/process control and holds the DPAPI token; unprivileged Tauri UI talks only to the service (named pipe) and Hub; watchdog restarts the app. (AD-31; LLD §Kiosk internals)
- Secrets: cloud secrets SOPS 3.13.3 + age 1.3.2 in repo, decrypted into K8s secrets at deploy; device tokens and branch KEK in OS keystore (DPAPI, Android Keystore); Hub never holds a tenant-wide secret. (AD-22)
- Personal data: per-player data keys in a Hub key table outside the change log, replicated to standby over LAN, backed up as `keys/player-keys.bin` wrapped by the branch KEK (escrowed in `gz_control`); erasure via `player.erased` tombstone; consent rows with policy version. (AD-24)
- Rate limits per AD-23 (Cloudflare per-IP; per-tenant in `arena-cloud` against `rate_limits`; pairing 5/code, 20/IP/hour). (AD-23)
- Premium live view: read-only AD-25 subscription over Cloudflare Tunnel + Access gated by `tunnel` flag; cloud-signed ≤ 5-min branch-scoped token verified by the Hub; no writes. (AD-43)

**Integrations**

- Razorpay Subscriptions (monthly + yearly), one subscription per branch, card/eNACH above ₹15,000, states `active → renewal_needs_approval → grace (7 days) → lapsed`, webhooks verified and idempotent by event id, refunds by admin. Razorpay coverage for MY/SG/US to be confirmed before first non-India tenant. (AD-20; Spine §Deferred)
- Gupshup (WhatsApp + SMS fallback, owner OTP) behind a swappable messaging adapter selected by config; notifications drained from a Hub outbox (`notify.requested`, `not_before = commit + undo window`, consent/erasure/revert re-check, send ledger unique on outbox ULID); owner alerts via VAPID Web Push. Submit WhatsApp templates in wave 1. (AD-27, AD-34; HLD §Risks)
- AdMob only via an in-house Tauri Kotlin plugin calling Google Mobile Ads SDK 25.5.x (off main thread); no ad code in Rust; `ad_impressions` rows carry no device/creative ids; Android targetSdk 36, minSdk 28, NDK 28+, Kotlin 2.1+. (AD-16)
- ESC/POS printing (USB, LAN 9100, Bluetooth SPP via Android plugin), drawer kick, CEC (libcec/cec-client on Pi) and local notifications run after commit from a local side-effect outbox recording result intents (`receipt.printed`, `drawer.kicked`); a missing printer never blocks. (AD-28)
- Every provider sits behind an adapter with a recorded fake and a manual mode; no integration on a player's critical path. (AD-34)
- Global game catalogue in `gz_control` edited only via admin app; media in R2 behind CDN; Hub reference cache (default 10 GB LRU, covers always, video best-effort) outside the change log; kiosks fetch media only from the Hub over LAN; `game.matched` intent records matches. (AD-17)
- Partner identity separate account type and API scope; monthly payout job in Asia/Kolkata business time; KYC encrypted, admin-read only; manual transfer. (AD-18, AD-19)
- Android Hub specifics: foreground service, mDNS via NsdManager plugin, WAKE_LOCK + battery-optimisation exemption, 20 GB free check, no Tunnel/standby on Android. (LLD §Android Hub)
- Pi agent: Rust aarch64 on Raspberry Pi OS Lite image (read-only root + overlay), systemd service, CEC capability probed at pairing; TV screens (idle, warning, lock) rendered by Chromium in kiosk mode showing a Svelte page served by the Pi's Rust agent, using the shared tokens/components. smart plugs deferred to the v1.1 Pi epic. (LLD §Pi agent; Spine §Deferred)

**CI/CD, signing, rollout**

- GitHub Actions pipeline: lint + unit, `proptest` property tests on `apply` invariants, rollup golden tests, generated-type freshness (fail on diff), Hub + fake Kiosk + cloud integration tests, builder round-trip checksum = Hub DB, Owner API sum-of-rollups = Hub Z-report over 30 synthetic days, signed Windows and Android builds, web builds, release candidate to staging on tag; nightly integration; failover, soak, load, lockdown, hardware and restore drills before release. (Spine §Operational Envelope "CI/CD"; LLD §Test plan)
- Signing and distribution: Windows Hub and Kiosk as Authenticode-signed installers updated by Tauri updater (ed25519 manifests, staged by control-plane flag); Android via Play with staged rollout; Pi agent ed25519-signed; Hub caches and verifies signed Kiosk/Pi installers before serving on LAN and installs only with no active session. (AD-32)
- Rollout order: cloud first (accepts N and N−1), standby before primary, Windows Hub 1% → 10% → 50% → 100% over a week, Android Play staged rollout, then Kiosk/Pi via Hub cache. Release train: cloud weekly, Hub/Kiosk biweekly, Pi monthly, web apps biweekly. (Spine §Operational Envelope "Rollout"; LLD §Release train)
- Performance budgets enforced by per-release benchmark on reference hardware in CI. (AD-33)

**Migrations and versioning**

- Schema migrations forward-only and additive, numbered SQL embedded in the binary; `SCHEMA_TOO_NEW` refusal; drop a column only two releases after nothing reads it; renames = add + backfill + drop; enums TEXT + CHECK from the Rust enum; booleans 0/1; catalogues soft-delete; ledgers never deleted. (AD-8; LLD §Migrations)
- Protocol versioning: frames carry `v`; unknown kinds `UNSUPPORTED`; Hub accepts Kiosks from previous two releases; heartbeat reports schema and command `schema_version` range; shared cross-unit formats (mDNS service name, export file, Android→Windows snapshot) defined once in `arena-proto` and versioned. (AD-8)

**Observability and operations**

- OpenTelemetry → Grafana Cloud free tier + Sentry; metrics: summary age per branch, rollup missing-day count, builder queue depth, command backlog, webhook failures, fleet heartbeat gaps; tenant alert job: no heartbeat 30 min in opening hours ⇒ owner "branch offline". (Spine §Operational Envelope "Observability", "Tenant alerts")
- Hub `tracing` JSON logs (intent id, actor, device; 50 MB ring buffer, 14 days, no PII); Kiosk ring buffer 7 days + Sentry; health endpoints Hub/cloud `GET /healthz`; diagnostics bundle (logs, settings, device list, licence claims, integrity check, kiosk logs) uploaded only on "send". (LLD §Error codes, events and observability)

**Build spikes and open technical decisions (to schedule)**

- First build spike: aws-sdk-s3 against R2 (`request_checksum_calculation = WhenRequired`, region `auto`, presigned PUT); single SQLite linkage with rusqlite + libsql in `arena-cloud`; ed25519-dalek 3 + jsonwebtoken EdDSA keys; Cloudflare Tunnel pricing per branch. Choose ts-rs vs specta and pin K8s distribution at build start. (Spine §Deferred, §Stack)

### UX Design Requirements

#### Design tokens and theming

UX-DR1: Implement the **Cinema Electric dark token set** in the shared UI package: `bg #09071A`, `surface #16122C`, `surface-2 #201A3D`, `line #362D5C` (decorative only), `border-strong #8178B5` (every operable boundary), `text #F5F2FF`, `text-muted #ACA5CC`, `accent #FF4FA8` / `on-accent #1A0010`, `ok #5FE6A0`, `warning #FFBE3D` / `on-warning #1E1300`, `danger #FF5C5C` / `on-danger #1A0505`, `info #7FB2FF`, `scrim-base #0A0816`; magenta used only for primary CTA, featured outline, focus inner ring and Undo — never state, alerts, decoration, text over media or link colour in body text. (DESIGN §Colors)

UX-DR2: Implement **Hub tones** `hub-bg #0B0920`, `hub-header #110E28`, `hub-tile #141030`, `hub-line #2A2350` (decorative only) and the **8 floor-tile state tokens** free `#5FE6A0`, in-session `#7FB2FF`, paused `#5C82BD`, reserved `#C8A2FF`, order-pending `#FFBE3D`, maintenance `#8A85A8`, offline `#FF5C5C`, unassigned `#ACA5CC` (dashed); state is always colour + text label. (DESIGN §Floor-tile states)

UX-DR3: Implement **chip and HUD toast tokens**: rule chip `#3D0F2A`/`#FFA3D1`, offline chip `#3D1418`/`#FF9A9A`, HUD toast solid `#1E1606` with `#FFBE3D` border/title and `#F5F2FF` body (never translucent). (DESIGN §Colors)

UX-DR4: Implement the **high-contrast (`-hc`) theme**: pure black surfaces, white text and hairlines, yellow accent `#FFE14D`, brightened semantics and state-hc tokens, inverted chips (rule yellow/black, offline red/black), no art/video/translucency/shadows, 2px white outlines on every interactive surface, 4px yellow focus ring; every component token resolves to its `-hc` twin; toggled per device by owner/manager in Hub settings (no player toggle); applies to Kiosk, Hub, Kitchen display, Pi TV and Owner surfaces. (DESIGN §High-contrast theme; EXPERIENCE §Accessibility Floor)

UX-DR5: Implement the **light theme "Cool paper" (`-light`)** for Owner app and all web surfaces only: `bg #F6F5FC`, `surface #FFFFFF`, `surface-2 #ECEAF7`, `border-strong #7A72A8`, `text #16123A`, `text-muted #555075`, `accent-light #D4127A` (fills only, white ink), `accent-text-light #AD0E63` (links/focus/magenta ink), `warning-text-light #8F5300` with 1px edge on every amber fill, `danger-light #C21F2E`, `info-light #1F56C2`, light chips and light state inks; hub-tone mapping (`hub-bg`/`hub-header` → `bg-light`, `hub-tile` → `surface-light`); full-bleed marketing media sections stay dark; never on Kiosk, Hub, Kitchen or Pi. (DESIGN §Light theme)

UX-DR6: Implement **theme mode rules**: Kiosk/Hub/Kitchen/Pi dark-only (+HC); Owner/web default follows `prefers-color-scheme` live, overridden by `theme-toggle` remembered per device; HC overrides both where it applies. (DESIGN §Light theme "Mode rules"; EXPERIENCE `theme-toggle`)

UX-DR7: Implement the **six device-group swatches** (teal `#3FD0D4`, orange `#FF8C42`, lime `#C4E35A`, sand `#E0B27A`, coral `#FF8FA3`, silver `#E6E3F5`) as an owner-pickable fixed set, shown only as a dot beside the group name (never a tile fill or state bar), with a 1px `border-strong-light` ring in the light-theme picker. (DESIGN §Device-group swatches)

UX-DR8: Implement **Sora** (OFL) as a bundled self-hosted web font (no runtime font/CDN fetch) with **tabular numerals mandatory** for every timer, countdown and currency amount. (DESIGN §Typography)

UX-DR9: Implement the **Kiosk type ramp** (also Pi TV): `kiosk-display` 144/200 lh 0.9 +0.02em, `kiosk-headline` 48/300, `kiosk-title` 28/600, `kiosk-body` 20/400, `kiosk-label` 18/600, `kiosk-cta` 24/800, `kiosk-eyebrow` 16/600 uppercase +0.14em, `kiosk-tile-name` 20/800, `hud-numeric` 22/600 tnum. (DESIGN §Kiosk ramp)

UX-DR10: Implement the **Hub type ramp** (also Kitchen): `hub-numeric` 26/800 tnum, `hub-title` 18/600, `hub-body` 15/400, `hub-label` 13/500, `hub-micro` 11/700 uppercase +0.06em; nothing below 11px. (DESIGN §Hub ramp)

UX-DR11: Implement the **Web type ramp**: `web-hero` 64/200 (marketing/onboarding hero ≥ 1024px, else `web-display`), `web-display` 40/300, `web-body` 16/400; owner figures use `hub-numeric`; layouts tolerate +30% string length. (DESIGN §Web)

UX-DR12: Implement the **radius scale** xs 6 (status tags, order badge), sm 10 (rail items, wizard rows, text fields, PIN cells), md 16 (floor tiles, kitchen tickets, undo toast), lg 20 (game tiles, shop cards, launcher cards), xl 24 (panels, menus, sheets, HUD toast, cart, cards), full (buttons, pills, chips, badges); media clips to container radius. (DESIGN §Shapes)

UX-DR13: Implement the **4-based spacing scale** 4·8·12·16·24·32·40·56·64·80·96·128 (no off-scale values) with `touch-min` 44px, and surface tokens: Kiosk margin 56; Hub gutter 32 (phone 16), section gap 40, panel padding 24, tile padding 16, header 72, rail 88, floor gap 24 (phone 12); Owner gutter 40 (phone 16), section gap 56, card padding 32 (phone 24), header 64, sidebar 256; Kitchen gutter 24, column gap 24, ticket padding 16, ticket gap 16, header 64; Web gutter 56 (phone 16), marketing section gap 128 (phone 64), app section gap 56, card padding 32 (phone 24), header 80 (phone 64). (DESIGN §Layout & Spacing)

UX-DR14: Implement **elevation rules**: player mood shadows (game tiles 0 10 30 @35%; account menu/cart 0 16 40 @35%; HUD toast/extend-options 0 20 60 @50%); staff mood no shadows (hierarchy `hub-bg` → `hub-header` → `hub-tile` → `surface`); HC no shadows; `backdrop-filter` banned on every platform. (DESIGN §Elevation & Depth)

UX-DR15: Implement **`scrim` and `tile-scrim`**: two-gradient hero scrim with copy confined to the left 45% × bottom 50% copy zone (verified against a white frame, text ≥ 10.4:1); tile-scrim α 0.85 over bottom 25% fading by 55%; omitted only with no media or in HC. (DESIGN §Scrim; EXPERIENCE `scrim`)

UX-DR16: Implement **`interaction-states`** for every button, tile, card and row: hover surface 6% lighter / accent 8% lighter (light theme 6% darker); pressed 6% darker + 1px inset; touch shows pressed only; disabled 40% opacity, no hover, not-allowed cursor and the reason as inline text at full ink; HC hover thickens outline 2→3px, pressed insets it. (DESIGN/EXPERIENCE `interaction-states`)

#### Shared components

UX-DR17: **`focus-ring`** — double ring (2px magenta inner, 2px white outer, offset 2px), HC 4px yellow, light 2px `#AD0E63`; shown on keyboard focus on every focusable element, never suppressed, distinct from the featured-tile outline; sticky bars and open tile sheet never cover the focused element (scroll padding). (DESIGN/EXPERIENCE `focus-ring`)

UX-DR18: **`text-field`** — `surface-2` fill, 1px `border-strong`, radius sm, ≥ 44px, label above and programmatically linked; focus 2px accent border + focus ring; error 2px danger border + ⚠ icon + message below; validation on submit and blur with focus moving to the first invalid field and the error announced; password show/hide; paste allowed everywhere (incl. kiosk password). (DESIGN/EXPERIENCE `text-field`)

UX-DR19: **`button-primary`** (magenta pill, `kiosk-cta`, ≥ 340px on kiosk hero, one per view, disabled shows why inline), **`button-secondary`** (`surface-2` pill with `border-strong`; Dismiss, Cancel, End, Lock) and **`button-warning`** (amber pill, only Extend in `hud-toast`/`extend-options`; light theme 1px edge). (DESIGN/EXPERIENCE buttons)

UX-DR20: **`theme-toggle`** — `tab-control` System · Light · Dark in Owner Account and web footer; System default tracks OS live; remembered per device; absent on Kiosk/Hub/Kitchen/Pi. (EXPERIENCE `theme-toggle`)

UX-DR21: **`undo-toast`** — `surface-2` bar with `border-strong` edge showing what happened (tabular amount), seconds remaining ("8 s") and an accent Undo; Hub bottom-centre above `settle-bar`/`hub-bottom-bar`, Kitchen bottom; used for End, Settle, Collect and kitchen stage advance; 5–10 s window; never blocks the floor; a new money action replaces it and commits the previous; not shown once a receipt printed; kiosk cleanup runs immediately and Undo restores only the session record. (DESIGN/EXPERIENCE `undo-toast`; AD-13)

UX-DR22: **`alert-badge`** — red pill with `on-danger` ink: count in Hub header, or tag ("NEW", "CALL", "TIME UP", "REQUEST") on a tile/ticket; persistent visual twin of every alert sound until acknowledged by opening the item; header badge opens the alert list. (DESIGN/EXPERIENCE `alert-badge`)

UX-DR23: **`status-badge`** — uppercase `hub-micro` tag, colour as text + 1px border on `hub-tile`; also owner command states (queued on this device = muted, pending = info, applied = ok, rejected = danger, expired = muted); changes announced in a polite live region on web. (DESIGN/EXPERIENCE `status-badge`)

UX-DR24: **`tier-lock`** — lock glyph in muted ink + "Available on Basic/Premium" + underlined "Upgrade" link to Owner Subscription; staff/owner surfaces only (Kitchen display on Free, Owner dashboards on Free, Live view below Premium, device limit, Free 3 users); player surfaces hide gated features entirely. (DESIGN/EXPERIENCE `tier-lock`)

UX-DR25: **`card`** generic `surface` container (radius xl, owner/web padding tokens) and **`tab-control`** segmented pill (surface track, 4px inset, items ≥ 44px, selected inverted to `text` fill/`bg` ink; ARIA tabs with arrow keys; selected tab reflected in URL; Monthly · Yearly swaps every displayed price; not for navigation). (DESIGN/EXPERIENCE `card`, `tab-control`)

UX-DR26: **`logo-slot`** — branch logo only (the only per-branch customisation) on Kiosk top-left and Pi screens, falling back to the Arena 360 mark. (DESIGN/EXPERIENCE `logo-slot`)

#### Player components (Kiosk, HUD)

UX-DR27: **`video-control`** — 44px round pause/play (solid `bg`, `border-strong` ring, icon shows next action) bottom-right of kiosk login and Home hero; always visible while video plays; keyboard-reachable; pause holds the frame for the rest of the view. (DESIGN/EXPERIENCE `video-control`; mock key-kiosk-home, key-kiosk-login)

UX-DR28: **`login-panel`** — solid `surface` panel radius xl centred over full-bleed muted video: username + password (show/hide), primary button, underlined "Forgot password?" and "Create account" links, footer PC name + version; Enter submits; 3 failures → 60 s lock countdown + Hub notified; submit disabled while Hub unreachable; no language picker, no social/QR login. States: rest, failed submit, Hub unreachable. (DESIGN/EXPERIENCE `login-panel`; mock key-kiosk-login)

UX-DR29: **`signup-form`** (spine-only) — same shell as login: username, password, phone, name, required unticked consent checkbox (≥ 44px hit area, underlined Terms/Privacy links, exact Voice copy), primary button disabled with inline reason until ticked; duplicate username/phone field-level errors; success → plan picker. (DESIGN/EXPERIENCE `signup-form`)

UX-DR30: **`pill-nav`** — Home · Games · Apps · Shop in a `surface` pill track; active item inverted; Shop absent on Free; exactly one active; arrow keys move; selection persists per session, resets on logout; source tab stays active on Account page. (DESIGN/EXPERIENCE `pill-nav`)

UX-DR31: **Kiosk top bar** — 32px from top, 64px tall: left `logo-slot`, centre `pill-nav`, right cluster in order `reconnecting-indicator` (when shown), `volume-control`, `messages-bell`, `call-staff-button`, `time-chip`, account chip. (DESIGN §Layout Kiosk; mock key-kiosk-home)

UX-DR32: **`time-chip`** — always-visible pill with status dot + time in `hud-numeric`, counting down (prepaid) or up (postpaid); at ≤ 10 min a 2px warning border and ⚠ glyph replace the dot (never colour alone); click opens `account-menu`. (DESIGN/EXPERIENCE `time-chip`)

UX-DR33: **`reconnecting-indicator`** — quiet `surface` pill "Reconnecting…" with static icon, shown only while the Hub is unreachable, removed after reconcile. (DESIGN/EXPERIENCE)

UX-DR34: **`volume-control`** — speaker icon opening a horizontal slider (`border-strong` track, `text` fill, 44px thumb hit area); sets Windows system volume for the session; resets to branch default at logout. (DESIGN/EXPERIENCE)

UX-DR35: **`messages-bell`** — bell button; unread adds an `info` dot + count carried in the accessible name; lists staff broadcast/per-seat messages and order-status updates; opening marks read; cleared at session end. (DESIGN/EXPERIENCE)

UX-DR36: **`call-staff-button`** — "?" icon + visible "Call staff" label in a `surface` pill with `border-strong`; tap pings Hub (tile "CALL" badge + sound) and shows "Staff notified". (DESIGN/EXPERIENCE)

UX-DR37: **`account-menu`** — `surface` panel: header with name, wallet, time left; rows Balance, Extend (with Ctrl+Shift+E key caps, opens `extend-options`), Messages, Account, PC settings (card panel: mouse sensitivity slider, resolution/refresh from allowed list, audio output, keyboard layout), Lock, Log out (confirms only if prepaid time remains). (DESIGN/EXPERIENCE `account-menu`; mock key-kiosk-account)

UX-DR38: **`account-subnav`** — vertical Details · Wallet & packs · Order history · Change password; active `text` with 3px left bar, others muted; single-select, arrow keys; content on right. (DESIGN/EXPERIENCE; mock key-kiosk-account)

UX-DR39: **`hero`** — muted video → cover → `bg` under scrim; eyebrow, `kiosk-display` title, meta line (incl. "You last played 2 days ago"), Play now (primary) and Game details (secondary → card panel with genre, age rating, launcher; Esc closes and returns focus); featured title = branch's most-played game this week; stops when a game launches; copy ≤ 864px (≤ 720 at 1366). (DESIGN/EXPERIENCE `hero`; mock key-kiosk-home)

UX-DR40: **`game-tile` / `game-tile-featured`** — portrait cover 140×196 (featured 190×260 with 3px magenta outline offset 3px), name bottom-left in `kiosk-tile-name` over tile-scrim, fallback `surface` with centred name; focus = focus-ring (featured ≠ focus); activate launches (Games) or opens (Apps); featured drives hero media. (DESIGN/EXPERIENCE; mock key-kiosk-home, key-kiosk-games)

UX-DR41: **`all-games-tile`** — last tile in the featured row: `surface` with `border-strong`, grid icon, "All games"; opens Games unfiltered. (DESIGN/EXPERIENCE)

UX-DR42: **`most-played-carousel`** — "Most played here": branch top 10 games by session hours over last 7 days, ranked "#1…" badges (`bg` fill, `text` ink) top-left; arrow keys scroll; not gamification. (DESIGN/EXPERIENCE; mock key-kiosk-games)

UX-DR43: **`launcher-card`** — `surface` card with Steam/Epic/Riot logo, name and "Use your own account"; opens the launcher for the player's own account; logout enforced at end. (DESIGN/EXPERIENCE; mock key-kiosk-games)

UX-DR44: **`filter-rail`** — left column of categories/tags (active `text`, others muted) with search field right-aligned in the content title row; single-select + search; filters apply instantly; empty result shows underlined "Clear filters". Used on Games, Apps, Shop. (DESIGN/EXPERIENCE)

UX-DR45: **`shop-item-card`** — photo, name, tabular price, Add; `popular-badge` top-left when applicable; stock checked live; out of stock dims photo but keeps name + "Out of stock" tag at full ink and disables Add; bundles list components and refuse if short. (DESIGN/EXPERIENCE; mock key-kiosk-shop)

UX-DR46: **`popular-badge`** — uppercase "POPULAR" pill (`bg` fill, `text` ink, `border-strong` edge) marking the branch's top 3 shop items by units over 7 days. (DESIGN/EXPERIENCE)

UX-DR47: **`cart` (Kiosk)** — right-side `surface` panel radius xl: lines with ±1 steppers (live stock check on each change), bundle components indented, total in `hud-numeric`, payment choice Add to session bill · Pay now from wallet, primary "Place order"; no session → "Pay at counter"; empty "Cart's empty.". (DESIGN/EXPERIENCE `cart`; mock key-kiosk-shop)

UX-DR48: **`plan-picker`** (spine-only) — plan options as cards; only plans valid for the device group; selected card 2px `text` border; packs show hours left + expiry; expired packs muted with "Expired" tag and unused time ("0h 45m unused"), not selectable; wallet option shows balance, hourly rate and active rule chip; selecting shows "Waiting for the counter…" until Hub approves or refuses with typed reason (stays on picker). (DESIGN/EXPERIENCE `plan-picker`)

UX-DR49: **`hud-pill`** — small always-on-top solid `bg` pill with time left/elapsed + balance (`hud-numeric`); Ctrl+Shift+E expands it; never takes focus on its own; hidden in fullscreen-exclusive games (taskbar flash + sound at 5 and 1 min instead). (DESIGN/EXPERIENCE; mock key-kiosk-hud-lock)

UX-DR50: **`hud-toast`** — solid amber-bordered panel (~640px) top-right with amber title, body, Extend (`button-warning`) + Dismiss (`button-secondary`) and Ctrl+Shift+E key caps; appears with sound, never steals focus; warning toasts (10/5/1 min, low balance) persist until dismissed or time-up; info toasts (order status, messages) auto-dismiss after 8 s and are kept in the bell. (DESIGN/EXPERIENCE; mock key-kiosk-hud-lock)

UX-DR51: **`extend-options`** — expanded-HUD panel in hud-toast palette listing time options with live Hub prices as secondary rows and the payment source (wallet/pack/credit); entry from toast, hotkey (focus on first option), account menu or grace lock; disabled while Hub unreachable; refusals show Voice copy and keep the panel open. (DESIGN/EXPERIENCE `extend-options`)

UX-DR52: **`lock-screen`** variants — grace countdown (`kiosk-display` digits over still-visible game behind scrim with Extend `button-warning` beside it; successful Extend dismisses); fullscreen-exclusive (no visual, taskbar flash + sound); Pay at counter / Time's up (solid `bg`); staff handles (solid lock over paused game); forced end (forced-end line). (DESIGN/EXPERIENCE `lock-screen`; mock key-kiosk-hud-lock)

#### Player components (Pi / QR, v1.1)

UX-DR53: **`tv-idle`** — 1920 layout: muted video → cover → `bg` under scrim, `logo-slot` top-left, "TV n" in `kiosk-headline` and tabular console rates bottom-left in the copy zone, QR ≥ 360px with white quiet zone and md frame bottom-right labelled "Scan to start"; no ads. (DESIGN/EXPERIENCE; mock key-pi-console frame 1)

UX-DR54: **`tv-warning`** — 5-s screen at 5 and 1 min: solid `bg`, 8px warning frame, minutes in `kiosk-display`, "ask at the counter to extend" in `kiosk-headline`; then switch back to console. (DESIGN/EXPERIENCE; mock key-pi-console frame 4)

UX-DR55: **`tv-lock`** — no grace: solid `bg`, logo, "Time's up — see the counter." only, held until staff end, then back to `tv-idle`. (DESIGN/EXPERIENCE; mock key-pi-console frame 5)

UX-DR56: **`qr-start-page`** — phone portrait, 16px gutters, no login: header logo + "TV n", console plans as plan-picker cards with tabular live prices, primary "Request"; then "Waiting for the counter — pay at the counter to start."; refusal "Ask at the counter.". (DESIGN/EXPERIENCE; mock key-pi-console frame 2)

UX-DR57: **`live-timer`** — single `surface` card: "Time left on TV n", time in 96px Sora 200 tabular, "Extend at the counter"; no actions; off-LAN keeps last value with "Reconnect to café Wi-Fi to update". (DESIGN/EXPERIENCE; mock key-pi-console frame 3)

#### Staff components (Hub, Kitchen)

UX-DR58: **`hub-rail`** — persistent left icon rail (88px) with `hub-micro` labels, active item filled `hub-line`: Floor view (home, default after unlock) · POS · Players · Sessions · Inventory · Shift · Reports · Settings · System; items always labelled. (DESIGN/EXPERIENCE `hub-rail`)

UX-DR59: **`hub-bottom-bar`** — below 840px replaces the rail: 64px bar with `border-strong` top edge, Floor · POS · Players · Shift · More; More opens a bottom sheet (Sessions first, Inventory, Reports, Settings, System); closing returns to current screen. (DESIGN/EXPERIENCE; mock key-hub-phone)

UX-DR60: **`hub-header`** — 72px: branch name, active `chip-rule`, `chip-offline` (naming internet or cloud), paid-tier backup status in muted `hub-label` (last upload, pending batches if non-zero; tap opens System), `alert-badge` count, clock, signed-in staff; on phone drops backup status and clock and moves chips to a row below. (DESIGN/EXPERIENCE `hub-header`; mock key-hub-floor)

UX-DR61: **`floor-tile`** — `hub-tile` body with 1px `border-strong`, radius md, ≥ 128px tall, 16px padding: top row device name + `status-badge`; middle time in `hub-numeric`; bottom player + `hub-label` secondary line; 3px state bar along bottom; paused = paused bar/badge + pause icon + frozen timer + reason line; unassigned = 2px dashed border, no bar, no group-chip; `group-chip` bottom-right; `tile-order-badge` + `alert-badge` top-right (not separate targets); tap/Enter opens sheet; live updates never move focus or reorder a focused tile. (DESIGN/EXPERIENCE `floor-tile`; mock key-hub-floor)

UX-DR62: **`floor-tile-console`** — floor-tile with controller glyph and "TV n" secondary line; Hub-only start/end and timer; "TIME UP" alert-badge + sound until acknowledged; "CHECK CONTROLLERS" warning status-badge until Returned/Missing; QR request raises "REQUEST" badge and opens sheet with plan pre-selected. (DESIGN/EXPERIENCE; mock key-pi-console frame 6)

UX-DR63: **`group-chip`** — neutral `surface-2` pill with `hub-line` edge, 8px swatch dot + group name in muted `hub-micro`; read-only; hidden on unassigned tiles. (DESIGN/EXPERIENCE `group-chip`)

UX-DR64: **`tile-order-badge`** — small amber tag (`hub-micro`, `hub-tile` ink) reading "ORDER" (kitchen working) or "READY" (per fulfilment setting); tapping the tile opens the sheet at the order section. (DESIGN/EXPERIENCE)

UX-DR65: **`tile-sheet`** — non-modal `surface` sheet docked right (floor reflows to 4 columns at 1920): start · extend · order · transfer · pause/resume · lock · end, each one tap; End and Lock as secondary ≥ 24px apart from Extend and Order; shows rate and `bill-breakdown`; pause reason picker; transfer by picking a target tile; remote unlock needs inline PIN; focus moves in on open, Esc closes and returns focus to the tile. (DESIGN/EXPERIENCE `tile-sheet`; mock key-hub-tile-sheet)

UX-DR66: **`inline-pin`** — 4–6 cell PIN field rendered in place in the current sheet/row/kiosk panel (`surface-2` cells, `border-strong` edge, filled cells show a `text` dot, radius sm); numeric keypad + keyboard; never navigates away; reason picker appears in the same place when required; used for forced end, manual discount, refund/void, variance over threshold, credit override, promote/force promote, kiosk admin exit, any action the role lacks. (DESIGN/EXPERIENCE `inline-pin`)

UX-DR67: **`player-lookup`** — one phone field with results directly below and a "Guest" secondary button beside; results as you type, Enter picks top; result row name + phone with outstanding credit first in warning text; no match → "No player with that number." + Create profile (≤ 15 s) + Guest. (DESIGN/EXPERIENCE `player-lookup`)

UX-DR68: **`pos-product-grid`** — category strip, favourites (star) first, one "Search or scan barcode" field above a grid of `hub-tile` product tiles (photo, name, tabular price, 16px gap); barcode wedge adds item; out-of-stock/expired disabled with readable tag; short bundle refuses with inline-pin override; full width on phone. (DESIGN/EXPERIENCE; mock key-hub-pos)

UX-DR69: **`cart` (Hub)** — same anatomy on `hub-tile` with `hub-numeric` total; header names attached player + seat with "Change" link; can attach to a running session; empty "Cart's empty.". (DESIGN/EXPERIENCE `cart`; mock key-hub-pos)

UX-DR70: **`phone-cart-bar`** — below 840px, a 64px bar on `hub-header` pinned above `hub-bottom-bar` once the cart has an item: "3 items · ₹310 · View" (tabular); tap opens cart + settle-bar as full-height sheet; Esc/Back closes keeping the cart. (DESIGN/EXPERIENCE)

UX-DR71: **`settle-bar`** — pinned bottom on `hub-header` with `border-strong` top edge: total in `hub-numeric`, payment methods cash · UPI · card · wallet · credit (no QR), primary Settle disabled with inline reason until remainder zero/credit; Ctrl+Enter settles; method chosen with Tab/arrows + Enter; coupon and manual discount (PIN + reason) apply here; receipt choice print · WhatsApp (consent only) · none; no printer never blocks; Settle → undo-toast. States: remainder open, UPI ref missing, no printer, settled. (DESIGN/EXPERIENCE `settle-bar`; mock key-hub-pos)

UX-DR72: **`payment-split`** — one row per method (label, amount field, reference field: UPI last-4 required, card terminal ref); running remainder in `hub-numeric`, warning with "Remaining" while non-zero, ok with "Paid in full" at zero; credit only for profiled credit-enabled player with inline-pin override at limit; guest-to-profile conversion available here. (DESIGN/EXPERIENCE `payment-split`)

UX-DR73: **`bill-breakdown`** — on `hub-tile`, 24px padding, 12px row gap: session (device, minutes × rate after rounding/min, paused excluded, one line per rate segment), orders, coupon, discount, subtotal, GST (incl/excl), ruled total in `hub-numeric`, then "Paid so far" and "Remaining"; group session splits by device minutes with changeable payer. (DESIGN/EXPERIENCE `bill-breakdown`; mock key-hub-tile-sheet)

UX-DR74: **`denomination-counter`** — one row per note/coin (denomination, count field, tabular subtotal) with running total at foot; "Single total" toggle swaps to one field; expected cash never shown until counted is entered (blind state). (DESIGN/EXPERIENCE; mock key-hub-shift-close)

UX-DR75: **`variance-readout`** — Counted, Expected, Difference as three `hub-numeric` figures 32px apart; Difference signed with "over"/"short"; over threshold = warning border + "Over threshold" label (never red) then inline-pin + reason list (Counting error · Unrecorded expense · Change float mismatch · Other + free text) in place; "Breakdown" underlined link lists float, cash sales, cash refunds, deposits, expenses; close prints Z-report with one tap. (DESIGN/EXPERIENCE; mock key-hub-shift-close)

UX-DR76: **`chip-rule`** (active dynamic pricing rule on Hub header, kiosk attract and plan picker; change reaches chip ≤ 1 min) and **`chip-offline`** (header pill naming internet or cloud; nothing else in the UI changes). (DESIGN/EXPERIENCE)

UX-DR77: **`kitchen-top-bar`** — 64px `hub-header` bar: branch name (`hub-title`), clock, "5 open · oldest 9 min" (tabular), LAN status-badge "CONNECTED" (ok) / "RECONNECTING" (warning), 44px mute-chime toggle (muted shows struck bell + "Chime off"; NEW badges stay). (DESIGN/EXPERIENCE; mock key-kitchen-board)

UX-DR78: **`kitchen-ticket`** — `hub-tile` card with `border-strong`, radius md: seat number in `hub-numeric`, items + modifiers, order id, tabular elapsed time; new = "NEW" alert-badge + danger edge + chime until advanced; Ready = `state-free` edge + "READY" tag; Served = muted ink; Cancelled (Hub only) = muted + "CANCELLED" badge then leaves; no cancel control; tap advances one stage → undo-toast; 4 columns New → Preparing → Ready → Served, newest-first, each column scrolls independently, empty column shows header only. (DESIGN/EXPERIENCE; mock key-kitchen-board)

UX-DR79: **`ad-banner`** — plain `surface` strip with "Ad" label at the bottom of the Free Android Hub; never overlaps floor grid, cart or settle bar and never shares an edge with `settle-bar` or `cart`; never on Kiosk. (DESIGN/EXPERIENCE)

UX-DR80: **`ad-interstitial`** — full-screen ad on solid `bg` with "Ad" label and secondary Skip top-right counting "Skip in 5 s" → "Skip ad"; only on PIN unlock after idle lock; never after settle; skipped if a cart or tile sheet is open. (DESIGN/EXPERIENCE)

#### Owner and web components

UX-DR81: **`metric-figure`** — muted `hub-label` label, `hub-numeric` value, `hub-micro` data age ("as of 2 min ago" / "Live"); Today branch card shows signed per-shift variance beside cash in drawer with warning edge + label over threshold; offline branch drops figures to muted ink with "Branch offline" status-badge, growing data age and its queued commands; Audit "Preparing audit…" as muted text on a surface panel (no spinner overlay). (DESIGN/EXPERIENCE `metric-figure`; mock key-owner-today)

UX-DR82: **`owner-nav`** — Today · Reports · Alerts · Manage · Account; desktop 256px left sidebar on `hub-bg` (icon + `hub-label`, active filled `hub-line`), phone 64px bottom tab bar (icon + `hub-micro`, `border-strong` top edge); Today is home; Live view in sidebar (desktop) or a button on each Today branch card (phone), tier-locked below Premium; follows light theme. (DESIGN/EXPERIENCE `owner-nav`; mock key-owner-today)

UX-DR83: **`command-row`** — `surface` row: what changed, scope (branch or "All branches"), status-badge, time queued, Retry/Resend; grouped Today / Earlier; states Queued on this device → Waiting for branch → Applied / Rejected ("Device/Hub revoked" or "Couldn't apply at branch" + Retry) / Expired (Resend); only valid targets offered; MFA prompt only for sensitive kinds. (DESIGN/EXPERIENCE `command-row`; mock key-owner-price-change)

UX-DR84: **`wizard-progress`** — ~256px left column: "Step n of 10" in `hub-micro`, one ≥ 44px row per step with round tabular number (done = `text` ink + ok check; current = `hub-line` fill + 2px ring, `aria-current="step"`; later = muted); "Can skip" tags on Products, Staff, Printer & payments; on self-advance focus moves to the new step heading; below 840px a row of 10 dots + "Step 3 of 10 · Install Hub". (DESIGN/EXPERIENCE; mock key-web-onboarding-install-hub)

UX-DR85: **`pairing-code`** — `surface` card with `border-strong`: 8-character code in `web-display` tabular grouped 4+4, "Copy code" secondary button, 15-min countdown announced at milestones (not every tick); expired state offers "Generate a new code"; no QR. (DESIGN/EXPERIENCE; mock key-web-onboarding-install-hub)

#### Layouts, IA and responsive behaviour

UX-DR86: **Kiosk layout & breakpoints** — landscape, reflows (never scales) at 1366 (4 featured + All games, 5-col grid, hero ≤ 720px), 1920 (6 + All games, 7 cols, ≤ 864), 2560 (8 + All games, 9 cols), ultrawide 21:9 (9 cols, content max 2560 centred, hero anchored left 45%); 56px margin; Home = featured row upper-left + hero copy bottom-left + video-control bottom-right; Games = launcher-card row + most-played carousel + filter-rail grid; Shop opens cart as right panel; Account = subnav left/content right. Kiosk flow Attract → Login → Plan picker → Waiting for Hub → Main shell; session end lock-screen → time-up mode → Attract. (DESIGN §Layout Kiosk; EXPERIENCE IA Kiosk)

UX-DR87: **Hub layout** — desktop/tablet: rail + header + main with 32px gutter; floor tiles grouped by zone (6 columns at 1920), zones 40px apart, zone header (name, tile count, collapse control, muted hardware/rate line e.g. "RTX 4070 · 240 Hz · ₹100/hr"), Unassigned section at the end; POS = grid left / cart right 40px apart / settle-bar bottom; shift close = denomination-counter and variance-readout as two panels 40px apart. Phone (< 840px): bottom bar, 2 tile columns, 16px gutter, 12px gap, chips in row under header, POS full-width grid + phone-cart-bar; 44px targets kept. Floor cold-load renders from local DB immediately with no skeletons. (DESIGN §Layout Hub; EXPERIENCE §Responsive)

UX-DR88: **Hub IA** — Players (lookup → profile with outstanding first, credit enable, collect, reset password); Shift (open with float, close, Z-report); Settings (layout editor with groups/zones/six swatches, time-up mode, order fulfilment, HC per device, receipts/printer 58/80 mm, kiosk default volume, opening hours, ads consent on Free); System (backup, standby promote, audit, diagnostics); Sessions, Inventory, Reports screens (spine-only, build from spines). (EXPERIENCE §IA Hub)

UX-DR89: **Kitchen display layout** — tablet landscape, kitchen-top-bar over four status columns 24px apart with ticket counts in headers, tickets newest-first 16px apart. (DESIGN §Layout Kitchen)

UX-DR90: **Owner app layout & IA** — one responsive web/PWA, full parity phone → desktop: desktop sidebar + content ≤ ~1200px with 40px gutter, two-column figure grid in branch cards; phone bottom tab bar, single column, 16px gutter; Today = one branch card per branch 56px apart + "Needs attention"; Reports = Revenue · Cash reconciliation · Stock · Credit · Audit · Exports; Manage = Pricing (Plans & rates · Pricing rules · Coupons) · Products · Devices & groups · Staff users & roles (setup code display) · command list; Account = MFA · Subscription (tier per branch, add-ons, Monthly · Yearly tab-control, invoices, payment method, Cancel with refund policy) · Branches · Theme; Free shows only Subscription, pairing, support; PWA caches last payload per screen with `as_of` (IndexedDB) and queues commands offline. (DESIGN §Owner app; EXPERIENCE §IA Other surfaces)

UX-DR91: **Web surfaces layout** — onboarding and partner portal staff mood ≤ ~1200px; onboarding wizard-progress left column (dots below 840px); marketing player mood with full-bleed media sections 128px apart (64 on phone); Home sections in order: hero (`web-hero` copy left in scrim zone, Hub floor-view product shot right), "Install. Pair. Play." three steps, "What you get" three cards, closing Get started band, footer with theme-toggle; Pricing: Cafés · Partners tab-control, Cafés panel with one primary "Get started" above, Monthly · Yearly tab-control, three equal plan cards (no emphasised plan), comparison table, add-ons, FAQ; Partners tab with terms, worked payout example, payout terms, Apply; phone nav hamburger → full-screen nav (Product · Pricing · Downloads · Help · FAQ) with "Get started" always in header. (DESIGN §Web surfaces; EXPERIENCE §IA Marketing)

UX-DR92: **Partner portal IA & states** — Referred cafés · Payouts · Referral link · KYC; home = referral link + code card, metric row (payout this month, active paying cafés, commission rate), referred-cafés table, payouts table (accrued, pending below minimum, paid, TDS, GST-invoice status); states: Under review three-step tracker (submitted → review → approved), "KYC needed before first payout", "Pending — below ₹500 minimum, rolls over", "Upload GST invoice", empty "No referred cafés yet." + link. (EXPERIENCE §IA Partner portal, §State Patterns; mock key-web-partner-portal)

UX-DR93: **Pricing and partner displayed values** — Free ₹0 · 1 branch · ads on Android Hub; Basic ₹200/branch/month + 18% GST, yearly ₹2,000; Premium ₹1,999 + 18% GST, yearly ₹19,990; yearly "2 months free" paid only; add-ons +20 devices ₹50, Tunnel ₹200, owner login ₹50; refund policy text; partner 10% commission, payout by 10th, ₹500 minimum, 2% TDS, GST invoice; displayed identically on Pricing, Owner Subscription and partner portal. (EXPERIENCE §Pricing and partner terms)

UX-DR94: **Pi / QR layout** — TV screens use the 1920 kiosk layout and ramp (16:9); phone screens single portrait column with 16px gutters. (DESIGN §Pi / QR screens)

UX-DR95: **Thermal print layouts** — ESC/POS Font A (never rasterised Sora); 80 mm 48 chars/line with name, qty, rate, amount columns; 58 mm 32 chars with name on its own line and qty × rate + amount below; text-only header (branch name bold double-width/height, address, GSTIN, phone centred); amounts right-aligned; `-` rules; KOT seat number double-width/height; contents per output: receipt/GST invoice, credit note, credit collection receipt, KOT, Z-report (field orders per EXPERIENCE). (DESIGN §Thermal print; EXPERIENCE §Receipts, Prints & Messages)

UX-DR96: **Message and print content** — WhatsApp/SMS templates (credit reminder, low balance, session ending, console 5-min alert, receipt; service only), subscription invoice (tier, branches, add-ons, cycle, subtotal, GST 18% itemised, total), partner messages (approval + link; payout statement), owner alerts (branch offline, cash variance with branch/staff/amount/reason, low stock, credit overdue, large refund/discount). (EXPERIENCE §Receipts, Prints & Messages)

#### State patterns

UX-DR97: **Floor tile state behaviours** — Free (label + group rate; tap → start postpaid/prepaid/guest), In session (time + player; extend/order/transfer/pause/lock/end), Paused (dimmed, icon, frozen timer; resume/end), Reserved (for whom/when; start for reservation/release), Order pending (badge over session state; order section/mark served), Maintenance (return to service), Offline (last seen; diagnostics/remote actions), Unassigned (dashed, PC name; assign to group/zone); consoles identical. (EXPERIENCE §State Patterns)

UX-DR98: **Connectivity states** — Hub cloud/internet offline: chip-offline only, nothing else changes; Kiosk Hub unreachable: sessions continue, reconnecting-indicator, login/Shop orders/Extend disabled with "Counter is reconnecting — try again shortly."; Kitchen LAN loss: tickets stay, advances move with no per-ticket marker, "RECONNECTING" only signal, no new orders until reconnect; Owner PWA no network: cached data with data age, commands "Queued on this device" then "Waiting for branch"; phone live-timer off-LAN keeps last value. (EXPERIENCE §State Patterns)

UX-DR99: **Media states** — fallback video → still cover (or launcher/exe icon for unmatched games) → solid `bg`, never a broken player; HC skips to solid; offline lookups fall through silently; kiosk attract cold-loads from cached branding and rates; only curated flash-screened global videos autoplay; always muted. (EXPERIENCE §State Patterns "Media", §Game Media)

UX-DR100: **Empty states** — Games/Apps "No games here yet — ask at the counter."; Kiosk Shop "Kitchen's closed right now."; Kiosk/POS cart "Cart's empty."; Kitchen column header only; Owner Alerts "All quiet."; Partner portal "No referred cafés yet." + referral link. (EXPERIENCE §State Patterns)

UX-DR101: **Refusal and error states** — plan picker refusal "Balance ₹40. Top up at counter." (stays); Extend refusals (closing, credit, balance) keep options open; credit block "Credit limit reached, see counter." + Hub inline-pin once/day; sign-up duplicate field errors; login lock 60 s countdown + Hub alert-badge; blocked app hud-toast + Hub log; forced end lock-screen; low balance persistent toast; remote action "Failed — retry" on tile after 5 s; printer offline header alert-badge; low stock/expiring header badge + Inventory list; save/apply errors inline with plain cause + retry, never a blocking modal during a sale; role lacks permission → action stays visible and opens inline-pin. (EXPERIENCE §State Patterns)

UX-DR102: **Ops states** — idle lock after 5 min with PIN unlock in place returning to the same screen; split-brain "This PC is no longer the main Hub" + "Make this the standby", never serves kiosks; command conflict toast "Price updated by owner" with both changes audited; audit "Preparing audit…" with progress then results (failure "Couldn't prepare the audit." + Retry); staff setup code display ("Setup code for Neha: K7Q4 9MZD. Give it to her; it works once.") and Hub first sign-in ("Enter your setup code, then choose your PIN and password."); ad interstitial due only on idle-lock return, skipped if cart/sheet open. (EXPERIENCE §State Patterns, §Voice)

#### Voice, copy and interaction primitives

UX-DR103: **Voice registers and fixed copy catalogue** — player casual ("10 min left. Extend?", "More time added. GG."), staff plain/no-blame ("Counted ₹4,850. Expected ₹4,900. Difference −₹50 short."; errors never name a person), money/legal formal; implement every fixed player line (login failure/lock, forgot password, sign-up duplicate, consent text, insufficient balance, credit limit, low balance, extend past closing, extended, order ready ×2, blocked app, forced end, call staff, reconnecting ×2, tier-gated, TV warning/time-up, QR waiting/live timer) and staff/owner line (updated by owner, command rejected, queued offline, old main returns, audit building/failed, setup code, staff first sign-in, forced promote warning, installer bad code, partner approval) in the message catalogue. (EXPERIENCE §Voice and Tone)

UX-DR104: **Hub speed and modality rules** — every common action one tap from floor view, none > 2 taps; start and settle < 10 s; sheets non-modal with floor live behind; money actions one tap + Undo, never a confirm modal; manager PIN always inline; modal depth max one, never stacked; banned: blocking modals during a sale, confirm modals on money actions, stacked modals, live blur, autoplay audio, gamification, ads on Kiosk/Pi. (EXPERIENCE §Interaction Primitives)

UX-DR105: **Hub/POS keymap** — Ctrl+1 Floor · Ctrl+2 POS · Ctrl+3 Players · Ctrl+4 Shift · Ctrl+K search · Ctrl+Enter settle · Esc close sheet (focus returns to tile); arrow keys move between floor tiles, Enter opens tile sheet; payment method via Tab/arrows + Enter (no dedicated shortcut); shortcuts never fire while a text field has focus; full counter flow without a mouse. (EXPERIENCE §Interaction Primitives "Keyboard")

UX-DR106: **Kiosk keymap** — Tab/Shift+Tab move, Enter activates, Esc goes back; global Ctrl+Shift+E opens expanded HUD with focus on first Extend option (shown on HUD toasts and account menu); hidden admin exit Ctrl+Alt+Shift+A → manager inline-pin; arrow keys in pill-nav, account-subnav and carousels. (EXPERIENCE §Interaction Primitives)

UX-DR107: **Sound + badge twins** — Hub sounds for new kiosk order, order ready, call staff, console time-up, kiosk login lockout, printer offline, low stock, expiring stock (and console REQUEST); Kitchen chime per new order; HUD warnings toast + sound; every sound has a persistent alert-badge until acknowledged; background videos always muted. (EXPERIENCE §Interaction Primitives "Sound + visual")

UX-DR108: **Motion and focus behaviour** — short subtle transitions only, cut to instant in HC (OS reduced-motion not separately honoured); HUD never steals focus from a game (only on hotkey); video plays only on kiosk login, Home hero and Pi idle and stops entirely while a game runs. (EXPERIENCE §Interaction Primitives)

#### Accessibility floor

UX-DR109: **Targets, focus, boundaries, links** — ≥ 44px targets on every surface incl. Android phone Hub (airy spacing never shrinks targets); focus-ring on every focusable element; inputs/PIN cells/focusable edges use `border-strong` (≥ 3:1); links always underlined (colour never the only marker). (EXPERIENCE §Accessibility Floor)

UX-DR110: **Non-colour / non-sound cues** — floor tiles, badges, tickets, time-chip warning and field errors always carry text or a glyph; every alert sound has a persistent badge on Hub and Kitchen display; text over media always on scrim inside copy zone, never in accent; game-tile labels on tile-scrim. (EXPERIENCE §Accessibility Floor)

UX-DR111: **Timed warnings and known limitation** — warning toasts persist until dismissed or time-up; Extend reachable by hotkey and account menu, not only the toast; Extend available during grace on the lock screen; fullscreen-exclusive taskbar flash + sound at 5 and 1 min; document the accepted limitation that those warnings may be sound-only for deaf/HoH players while the 10-min chip warning, grace lock and hotkey still apply. (EXPERIENCE §Accessibility Floor)

UX-DR112: **Authentication accessibility (WCAG 3.3.8)** — kiosk password login has the non-memory alternative of staff-start from the Hub; show-password; paste allowed; error copy never reveals which field failed; sensitive owner commands only re-prompt MFA. (EXPERIENCE §Accessibility Floor)

UX-DR113: **Web screen-reader baseline** (Owner app, onboarding, partner portal, marketing, Kitchen display, Hub web UI) — landmarks (`header`, `nav`, `main`), one `h1` per view, every control programmatically labelled, polite live regions for status-badge changes, chip-offline, alert-badge counts, hud-toast and new kitchen tickets, countdowns announced at milestones not every tick, self-advancing wizard steps move focus to the new heading. (EXPERIENCE §Accessibility Floor)

UX-DR114: **Contrast verification** — every load-bearing pair meets 4.5:1 text and 3:1 non-text in dark, HC and light themes (ratios documented in DESIGN frontmatter); text over media verified against a white frame; add an automated token-contrast check to the shared UI package CI. (DESIGN §Colors)

#### Visual references (mockups)

UX-DR115: Build from the 19 key-screen mocks in `_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/mockups/` (index `mocks-index.html`): Kiosk `key-kiosk-login`, `key-kiosk-home`, `key-kiosk-games`, `key-kiosk-shop`, `key-kiosk-account`, `key-kiosk-hud-lock`; Hub `key-hub-floor`, `key-hub-tile-sheet`, `key-hub-pos`, `key-hub-shift-close`, `key-hub-phone`; `key-owner-today`, `key-owner-price-change`, `key-kitchen-board`; `key-pi-console`; Web `key-web-marketing-home`, `key-web-pricing`, `key-web-onboarding-install-hub`, `key-web-partner-portal`; palettes `color-themes.html` (only Cinema Electric + High Contrast adopted) and `color-themes-light.html` (only "Cool paper"); `direction-cinema.html` layout cues only (its champagne accent, frosted glass and translucent HUD are superseded). Spines win on conflict. (DESIGN §Visual references; EXPERIENCE §Visual References)

UX-DR116: Apply the **stale-mock corrections** when building from these mocks: Floor view lacks `group-chip`; Hub phone lacks `phone-cart-bar`/full-height cart; Owner Today and Price change are dark-only, show Live view as a locked section and lack "Queued on this device", MFA and Manage → Pricing; Onboarding install-Hub still shows a pairing QR (remove), lacks phone dots, dark-only; Pricing lacks Monthly · Yearly toggle, GST-exclusive note, add-on prices and refund FAQ and still lists rewarded ads (remove); Marketing home hero not on `web-hero`, dark-only; HUD & time-up annotation describes a forced warning in fullscreen-exclusive (superseded by taskbar flash + sound). Spine-only screens (no mock): kiosk attract, sign-up, plan picker, Apps tab; Hub Players, Sessions, Inventory, Reports, Settings, System; Owner Reports, Alerts, Account, Manage beyond commands, Pricing, Subscription; onboarding steps other than Install Hub; marketing pages other than Home and Pricing; receipts/prints/messages; Hub ad-interstitial. (EXPERIENCE §Visual References)

### FR Coverage Map

FR1: Epic 2 - An owner signs up from the marketing site "Get started" form (name, phone, email, café name, city) and…
FR2: Epic 2 - An owner logging in on a new device must pass TOTP
FR3: Epic 8 - Fresh step-up MFA is required only for sensitive owner commands (staff users/roles, revoke device/Hub,…
FR4: Epic 2 - The onboarding wizard runs 10 ordered steps (tenant → tier → install Hub → devices → plans → products →…
FR5: Epic 2 - Wizard step 1 (Tenant) captures café name, country, currency, timezone, GST registered yes/no and GSTIN,…
FR6: Epic 2 - Wizard step 2 (Tier) picks Free, Basic or Premium per branch
FR7: Epic 5 - Plans and products can be seeded from a template (Indian café, console lounge, VR arcade) or by CSV import…
FR8: Epic 2 - Wizard step 4 (Devices) shows kiosks arriving live, lets the owner create groups and drag devices in and…
FR9: Epic 2 - Wizard step 7 (Staff) invites staff by phone with a role and shows a one-time setup code
FR10: Epic 2 - Wizard step 8 (Printer & payments) detects an ESC/POS printer and runs a test print (or is skipped)
FR11: Epic 2 - Wizard step 9 starts a 2-minute test session on one device from the wizard and completes when the kiosk…
FR12: Epic 5 - Running the wizard for a second branch reuses the tenant's groups, plans and products as templates by default
FR13: Epic 2 - Install-Hub step shows an 8-character, single-use pairing code (grouped 4+4, Copy, 15-min countdown, no…
FR14: Epic 2 - Pairing exchanges the code once for a Hub device token and signed licence
FR15: Epic 2 - A Kiosk on first run discovers the Hub on the LAN (mDNS) with a manual Hub-IP fallback, pairs by the…
FR16: Epic 2 - Staff register consoles (with group and TV number), VR rigs and simulators manually
FR17: Epic 2 - Device groups have a name, a colour from six fixed swatches and a sort order
FR18: Epic 2 - Device status is one of online, offline, in session, reserved or maintenance
FR19: Epic 2 - Staff drag devices onto a floor grid organised into zones (e.g. PC hall, console room)
FR20: Epic 2 - Pairing a device beyond the tier device cap (Free 20, Basic 60, Premium 200, +20 per add-on) is refused…
FR21: Epic 8 - With the Tunnel entitlement (Premium, or the Tunnel add-on on a paid tier) a kiosk or Pi outside the café…
FR22: Epic 13 - An owner migrates from ggLeap or Smartlaunch by uploading their exports, with field mapping and a dry-run…
FR23: Epic 3 - The Hub floor view shows every device tile with live state updated ≤ 2 s from source events
FR24: Epic 3 - Tapping a tile opens a non-modal tile sheet with start, extend, order, transfer, pause/resume, lock and end
FR25: Epic 3 - The active dynamic pricing rule is shown in the Hub header
FR26: Epic 3 - Floor zones can be collapsed (manually, remembered per device)
FR27: Epic 3 - Staff start an open postpaid session for a profiled player or a guest
FR28: Epic 3 - The postpaid bill = minutes × rate with the branch rounding unit (default 15 min) and minimum (default 30…
FR29: Epic 6 - A player can end a postpaid session via "I'm done" on the HUD
FR30: Epic 3 - Guest sessions appear in reports as "Guest"
FR31: Epic 3 - A player or staff starts a prepaid session on a pack, wallet balance, block or pass that counts down
FR32: Epic 3 - Partial pack use deducts minutes
FR33: Epic 3 - A block session ends at its fixed time regardless of start
FR34: Epic 3 - Staff pause a session with a reason (Player stepped away · Hardware issue · Power cut · Other)
FR35: Epic 3 - Staff or player extend a session charged to wallet, pack or credit
FR36: Epic 3 - Transferring a session locks the old device and unlocks the new one ≤ 5 s
FR37: Epic 3 - A group session puts several players on several devices under one bill with one payer
FR38: Epic 3 - A forced end requires manager PIN + reason, is audited, and the kiosk shows "Your session was ended by…
FR39: Epic 3 - Staff send remote PC actions (lock, unlock, message, restart, shutdown)
FR40: Epic 6 - Staff broadcast or per-seat messages reach the kiosk as a toast ≤ 2 s and are kept in the messages bell…
FR41: Epic 6 - The kiosk "Call staff" button raises a "CALL" alert badge on that tile plus the alert sound and confirms…
FR42: Epic 13 - Staff take bookings that reserve a device or group slot with a deposit and no-show rules
FR43: Epic 13 - Staff run tournaments with a bracket, an entry fee posted to a tournament income account, and devices…
FR44: Epic 3 - Staff create a player profile (username + password, phone, name, optional email, DOB, notes, tags) in < 15 s
FR45: Epic 3 - Phone lookup finds a player in 1 tap (results as you type, Enter picks top)
FR46: Epic 3 - Staff reset a player's password from the Players profile (kiosk "Forgot password?" directs players to the…
FR47: Epic 3 - Each player has a wallet in integer minor units that is never negative
FR48: Epic 3 - Owned packs list hours remaining and expiry on the player profile and kiosk account page
FR49: Epic 3 - A manager enables credit for a player with a limit and reminder cycle (default 3, 7, 14 days)
FR50: Epic 3 - Every credit spend checks outstanding + new ≤ limit inside the transaction
FR51: Epic 3 - Outstanding credit is aged 0–7, 8–14, 15+ days and shown first on the player profile and in lookup results
FR52: Epic 8 - Credit reminders are sent on cycle days only, only with stored consent and only on paid tiers
FR53: Epic 3 - Credit auto-pauses at 15+ days overdue (unless the branch disables this rule) and reopens automatically on…
FR54: Epic 3 - Staff collect credit (partial allowed) by any method
FR55: Epic 13 - A branch rewards loyalty with points per rupee redeemable as wallet credit and a referral bonus on a…
FR56: Epic 13 - A branch sells a monthly membership (recurring pass) with a WhatsApp renewal reminder
FR57: Epic 2 - A manager/owner sets an hourly postpaid rate per device group
FR58: Epic 3 - A time pack has hours, price, validity days and allowed groups (e.g. 5 h, ₹300, 7 days, Basic only)
FR59: Epic 3 - A block is a fixed slot with price and start window
FR60: Epic 3 - Console groups and packs are never shared with PCs
FR61: Epic 3 - A manager defines time-window pricing rules (weekday + hour range, per group, rate override or…
FR62: Epic 3 - Date rules (specific dates or ranges) outrank weekly rules
FR63: Epic 3 - The active rule is shown on the floor view header, kiosk attract screen and plan picker
FR64: Epic 5 - Coupons (code, discount type, validity, usage limit, allowed groups/plans) are applied at settlement and…
FR65: Epic 5 - A manual discount (percentage or amount) requires manager PIN + reason and appears in the audit log and…
FR66: Epic 5 - The POS product grid shows categories, images and favourites first, with one combined "Search or scan…
FR67: Epic 5 - A cart attaches to a running session so it settles with the bill
FR68: Epic 5 - A bundle has its own price independent of components, decrements its components on sale, and a component…
FR69: Epic 5 - Out-of-stock and expired items are disabled in the POS grid and kiosk shop
FR70: Epic 3 - Settlement accepts any mix of cash, UPI, card, wallet and credit with a live remainder
FR71: Epic 3 - A UPI row cannot be saved without the last 4 digits of the transaction reference
FR72: Epic 5 - Receipts go by thermal print (ESC/POS 58/80 mm over USB, LAN or Bluetooth), WhatsApp (only with consent)…
FR73: Epic 5 - A cash sale kicks the cash drawer via the printer and the drawer event is logged
FR74: Epic 3 - End, Settle and Collect commit in one tap and show Undo for 5–10 s
FR75: Epic 5 - Refund/void requires a reason, manager PIN and the original invoice reference, optionally returns stock,…
FR76: Epic 5 - A GST-registered Indian branch charges GST per item with HSN, inclusive or exclusive per branch, prints…
FR77: Epic 5 - Invoice numbers are sequential per branch per financial year in an epoch-segmented series (`<branch…
FR78: Epic 5 - Tax is summed per rate, computed once per rate and rounded once (half-up) per invoice
FR79: Epic 5 - Refunds issue credit notes that reference the original invoice number in the same series
FR80: Epic 5 - Pack and pass purchases are sales carrying their own GST rate and are reported separately from product sales
FR81: Epic 5 - The branch exports a GSTR-1-format file
FR82: Epic 5 - Staff open a shift with a confirmed float bound to their staff login
FR83: Epic 5 - Only recorded sales, deposits (with slip reference), expenses (category, amount, paid from drawer or…
FR84: Epic 5 - Shift close accepts a per-denomination count or a single total
FR85: Epic 5 - A variance over the branch threshold (default ₹100) requires manager PIN + a reason (Counting error ·…
FR86: Epic 5 - A one-tap Z-report prints at shift close (sales by type, payments by method, expected vs counted,…
FR87: Epic 5 - A product has name, category, price, tax rate, HSN, track-stock flag, image, barcode and kitchen flag
FR88: Epic 5 - Stock-in records quantity, cost, free-text supplier and expiry date and updates the weighted average cost
FR89: Epic 5 - Waste records quantity, reason (spoiled, broken, free) and staff
FR90: Epic 5 - Expiry alerts fire N days before
FR91: Epic 5 - A per-product low-stock threshold alerts the Hub and owner
FR92: Epic 5 - Stock decrements at served by default (configurable to at order)
FR93: Epic 14 - A manager runs stock counts with a count sheet, variance per item, adjustment with reason and a…
FR94: Epic 14 - A manager keeps supplier records and purchase orders
FR95: Epic 14 - A branch keeps a petty-cash ledger
FR96: Epic 14 - Staff clock in/out tied to their shift
FR97: Epic 7 - On Basic and above, kitchen-flagged items print a KOT ≤ 3 s of order with seat/device number, order id,…
FR98: Epic 7 - A kitchen display page runs on any LAN tablet browser (paired by code) with columns New → Preparing →…
FR99: Epic 7 - The kitchen display survives LAN loss (tickets stay, advances allowed and synced on reconnect)
FR100: Epic 7 - A printer offline shows a Hub alert badge while the kitchen display still receives orders
FR101: Epic 7 - Order status reaches the kiosk HUD ≤ 5 s and the floor tile shows ORDER/READY per the branch fulfilment…
FR102: Epic 14 - Kitchen items route to prep stations (e.g. bar vs kitchen) with a separate ticket per station, and…
FR103: Epic 1 - Roles
FR104: Epic 2 - An owner-created staff user receives a single-use, expiring setup code
FR105: Epic 1 - Manager PIN elevation is entered inline without leaving the current screen and the audit records both actors
FR106: Epic 1 - The Hub UI locks after 5 min idle (default, configurable)
FR107: Epic 4 - Free tier is capped at 3 branch users (tier-lock beyond)
FR108: Epic 5 - The Hub produces offline reports
FR109: Epic 5 - Every Hub report exports to CSV and PDF
FR110: Epic 1 - The Hub audit log records every change with actor, time and before/after, filterable by actor, module and date
FR111: Epic 1 - Branch settings
FR112: Epic 5 - Tax settings
FR113: Epic 3 - Billing rules
FR114: Epic 5 - Receipt settings (printer, paper width 58/80 mm, logo, footer, WhatsApp template) and notification…
FR115: Epic 2 - Opening hours per day (used by passes, pricing and offline alerts)
FR116: Epic 1 - One kiosk installer sets the Kiosk as the Windows shell for the player account (Explorer, Task Manager,…
FR117: Epic 1 - The kiosk enforces a launch allow-list
FR118: Epic 1 - Admin exit via hidden combo Ctrl+Alt+Shift+A + manager PIN (works with Hub down via cached verifier) or…
FR119: Epic 6 - The game booster closes listed background apps, sets process priority and optionally switches power plan…
FR120: Epic 6 - A player logs in at the kiosk with username + password in < 3 s on LAN, or staff start the session from…
FR121: Epic 6 - Kiosk sign-up takes username, password, phone and name with no OTP
FR122: Epic 6 - The kiosk attract screen shows branch logo, muted game video, current rates and active rule, with no ads,…
FR123: Epic 6 - The kiosk shell offers Home · Games · Apps · Shop (Shop hidden on Free), an always-visible time chip,…
FR124: Epic 6 - PC settings (mouse sensitivity, display resolution/refresh from the branch allowed list, audio output…
FR125: Epic 6 - Background video stops entirely while a game runs
FR126: Epic 6 - The kiosk account page shows details, wallet & packs, order history (this branch) and change password
FR127: Epic 6 - The overlay HUD shows time left/elapsed and balance, never steals focus, hides in fullscreen-exclusive…
FR128: Epic 6 - Warnings at 10, 5 and 1 min and on low balance (configurable per branch) appear as toast + sound and…
FR129: Epic 6 - Extend opens from Ctrl+Shift+E, the account menu, a warning toast or the grace lock
FR130: Epic 6 - At zero, a grace countdown (default 2 min) runs over the still-visible game with Extend available and no…
FR131: Epic 6 - On Basic and above a player orders food/drink from the seat
FR132: Epic 6 - The Hub holds the game catalogue (name, executable, launcher, category, age rating), edited once and…
FR133: Epic 6 - The kiosk auto-scans Steam, Epic, Riot and common folders on first run (and daily), proposing entries the…
FR134: Epic 6 - One-click launch from the kiosk game grid applies the booster and logs the launch with player and time
FR135: Epic 6 - Game art/video comes only from the global curated catalogue via the Hub cache, else the launcher cover/exe…
FR136: Epic 6 - At session end the kiosk signs out Steam, Epic, Riot, Discord and browser profiles and clears saved…
FR137: Epic 6 - Hours per game per branch are recorded and feed owner dashboards and the kiosk top-10 "Most played here"…
FR138: Epic 13 - A branch lends café-owned launcher accounts from a vault
FR139: Epic 13 - Players request games from the kiosk
FR140: Epic 3 - Staff run consoles as timed devices from the Hub with the same postpaid, pack and pass rules
FR141: Epic 3 - Console time-up raises a tile "TIME UP" alert + sound until acknowledged
FR142: Epic 12 - A Pi Zero 2 W or Pi 4 per TV (BOM < ₹4,000) pairs to the Hub by 6-digit code, shows its TV number, and is…
FR143: Epic 12 - The idle TV shows branch logo, TV number, muted game video, console rates and a "Scan to start" QR…
FR144: Epic 12 - Scanning the QR on café Wi-Fi opens a Hub LAN page with no login
FR145: Epic 12 - At 5 and 1 min the TV switches to the Pi input for a 5-s warning then back
FR146: Epic 3 - After console time-up the tile shows "CHECK CONTROLLERS" until staff mark Returned or Missing, logged…
FR147: Epic 12 - Hub sends console power on/off, input switch and TV message with the result ≤ 5 s
FR148: Epic 8 - On Basic and above, Today shows per branch
FR149: Epic 8 - Every owner figure shows its data age within tier cadence (Basic ≤ 15 min, Premium ≤ 5 min) on 99% of opens
FR150: Epic 8 - Owner reports — revenue (day/week/month/custom
FR151: Epic 8 - Live view (Premium, or Basic + Tunnel add-on) is a read-only second-by-second floor view through the Tunnel
FR152: Epic 8 - On Free the Owner app shows only Subscription, pairing and support
FR153: Epic 8 - Owners receive alerts by web push (iOS only when installed) and WhatsApp, with per-tenant thresholds, for…
FR154: Epic 8 - Owners send commands for prices/plans/rules, products/bundles, staff users/roles (one branch or all),…
FR155: Epic 8 - Before sending, a preview lists the target branches with online/offline status
FR156: Epic 8 - Each command applies exactly once
FR157: Epic 8 - The floor view and kiosk attract show a new rate ≤ 1 min after apply
FR158: Epic 8 - Owners search a branch's change history in the cloud by branch, actor and module
FR159: Epic 14 - Scheduled email/WhatsApp daily or weekly reports arrive by 8 am local
FR160: Epic 14 - Deeper dashboards
FR161: Epic 14 - Additional owner logins with read-only or full roles (owner-login add-on)
FR162: Epic 14 - Tenant-wide pricing rules pushed to all branches with branch override flagged in the owner app, and…
FR163: Epic 14 - Refunds over a threshold wait for an owner approval command and show as pending alerts
FR164: Epic 14 - Owner sends a message to all kiosks of a branch as a command
FR165: Epic 14 - Device health (last seen, kiosk version, GPU/RAM) with a device-offline alert during opening hours
FR166: Epic 1 - Every Hub, Kiosk and Kitchen feature works with the WAN down
FR167: Epic 8 - Paid Hubs upload change-log batches and the summary at tier cadence and daily rollups after close
FR168: Epic 6 - With the Hub unreachable, a kiosk continues the session on the last known time, enforces time-up locally,…
FR169: Epic 9 - A paid branch pairs a LAN standby as its own Hub
FR170: Epic 9 - If promotion fails, a manager can force promote with PIN after the warning "Data since the last sync may…
FR171: Epic 9 - A returning old main Hub locks itself ("This PC is no longer the main Hub", offers "Make this the…
FR172: Epic 14 - A Premium branch keeps an off-site standby that follows and promotes over STUN/TURN with the same…
FR173: Epic 9 - One-tap full DB export to file or USB on every tier (1 GB < 10 s) and restore from file with confirmation…
FR174: Epic 9 - Cloud backup by tier
FR175: Epic 9 - The Hub header shows cloud backup status on paid tiers (last upload, last summary, pending batches)
FR176: Epic 14 - Hubs check for updates daily and install on restart with rollback
FR177: Epic 2 - Owners create and rename branches, set timezone, and issue pairing and standby codes
FR178: Epic 10 - Subscription per branch shows tiers Free ₹0, Basic ₹200, Premium ₹1,999 per branch per month…
FR179: Epic 10 - One Razorpay subscription per branch with mandatory autopay (card or eNACH for renewals over ₹15,000 incl.…
FR180: Epic 10 - Full refund within 7 days of first payment
FR181: Epic 10 - The PPP price band (MY, SG, US) is set from the billing country and stored on the subscription so later…
FR182: Epic 10 - An upgrade applies within one heartbeat
FR183: Epic 10 - A Free Android Hub upgrading to paid migrates its database to a Windows Hub by snapshot export/import with…
FR184: Epic 4 - The Free Android Hub shows an always-visible AdMob banner that never covers the floor grid, cart or settle bar
FR185: Epic 4 - An interstitial shows only when staff return after the idle lock, is skippable after 5 s, and never after…
FR186: Epic 4 - Ads exclude adult, gambling and alcohol categories and the owner can block more
FR187: Epic 8 - Credit reminders, low balance, session ending, console 5-min alert and receipts are sent by the cloud from…
FR188: Epic 11 - A reseller/installer applies (name, phone OTP, email, city, business, type, cafés served) and sees only…
FR189: Epic 11 - Approval is sent by WhatsApp with the partner-portal link (partners.arena360.in, separate login)
FR190: Epic 11 - A referral link/code attributes the referred tenant at sign-up
FR191: Epic 11 - Partners earn 10% of paid subscription revenue net of refunds each month the referred tenant is active
FR192: Epic 11 - The partner portal shows referral link/code, referred cafés, accrued, pending, paid, TDS and invoice…
FR193: Epic 11 - Arena 360 staff use an admin app behind staff SSO to curate the global game catalogue
FR194: Epic 11 - The admin app handles partner approval, KYC review, payout marking after manual transfer, and 7-day refunds
FR195: Epic 11 - Support sees branch data only via the in-app diagnostics bundle (< 5 MB, no player PII, sent only on…
FR196: Epic 11 - The public site has Home (install, pair, play story), Product (Hub, Kiosk, Console, Owner with 90-s demo…
FR197: Epic 14 - An in-browser sandbox demo Hub runs with sample data (same web UI, in-memory DB)
FR198: Epic 14 - Compare pages vs ggLeap, Smartlaunch, iCafe (offline, price, hardware, migration) and status and changelog…
FR199: Epic 9 - The full branch DB and all reports are exportable any time on every tier, documented and tested
FR200: Epic 9 - Player erasure makes PII unrecoverable in Hub, standby, backups and snapshots and stays erased after…
FR201: Epic 3 - Consent is stored per player with policy version and timestamp
FR202: Epic 1 - Every surface is usable by keyboard, at high contrast and without relying on colour or sound alone

## Epic List

### Epic 1: Foundation — Hub and Kiosk install, start, and staff log in

On a seeded branch, the Hub and a locked-down Kiosk install and start, and a staff member logs in with roles, PIN elevation and idle lock, all with the WAN down.

**FRs covered:** FR103, FR105, FR106, FR110, FR111, FR116, FR117, FR118, FR166, FR202
**Phase:** MVP
**Depends on:** none
**Notes:** Story 1 = monorepo scaffold; Hub `apply()`/change log/epoch lease core, LAN protocol envelope and client classes, kiosk privilege split, shared token + component package (Cinema Electric, HC theme, a11y floor, message-catalogue lint), CI with proptest; NFR1, 2, 11, 12, 15, 16, 17, 25. FR118's Hub remote unlock goes live once pairing lands in Epic 2.

### Epic 2: Owner onboarding and pairing

An owner signs up with OTP/TOTP, completes the wizard, pairs the Hub and kiosks, invites staff and runs a 2-minute test session.

**FRs covered:** FR1, FR2, FR4, FR5, FR6, FR8, FR9, FR10, FR11, FR13, FR14, FR15, FR16, FR17, FR18, FR19, FR20, FR57, FR104, FR115, FR177
**Phase:** MVP
**Depends on:** Epic 1
**Notes:** Licence JWT and entitlement claims, pairing rate limits, device caps. FR6 delivers the Free path only (no card, entitlement issued); the paid-tier payment branch is deferred to Epic 10.

### Epic 3: Counter staff run the floor

Staff run postpaid and prepaid sessions on PCs and consoles, manage players, wallet, credit, plans and dynamic pricing, and settle bills with mixed payments and Undo.

**FRs covered:** FR23–FR28, FR30–FR39, FR44–FR51, FR53, FR54, FR58–FR63, FR70, FR71, FR74, FR113, FR140, FR141, FR146, FR201
**Phase:** MVP
**Depends on:** Epics 1–2
**Notes:** Floor tile and tile-sheet components; `apply()` invariants (wallet ≥ 0, credit ≤ limit, one session per device); undo intents; consent storage; NFR3, 4, 6, 10.

### Epic 4: Free cafés run on an ad-supported Android Hub

A Free café runs its branch on an Android Hub (Tauri 2 mobile) with a non-intrusive banner, an interstitial only on idle return, and Free limits enforced.

**FRs covered:** FR107, FR184, FR185, FR186
**Phase:** MVP
**Depends on:** Epics 1–3
**Notes:** In-house AdMob Kotlin plugin, no ad code in Rust; Android foreground service, NsdManager mDNS, wake lock, 20 GB check; NFR5 and NFR17 on a 3 GB device; ads never cover the grid, cart or settle bar.

### Epic 5: POS, inventory, tax-compliant receipts and shift close

Staff sell products and bundles, keep stock, issue GST invoices, refunds and receipts, and close a shift with a Z-report; reports and CSV/template imports are included.

**FRs covered:** FR7, FR12, FR64–FR69, FR72, FR73, FR75–FR92, FR108, FR109, FR112, FR114
**Phase:** MVP
**Depends on:** Epics 1–3
**Notes:** ESC/POS adapter, epoch-segmented invoice series, GSTR-1 export; the WhatsApp receipt is queued to the outbox, which Epic 8 drains; NFR6, 18.

### Epic 6: Player experience at the kiosk

Players log in or sign up, see a HUD with warnings, extend through grace, launch games from the library, order from the seat, and keep their session when the Hub drops.

**FRs covered:** FR29, FR40, FR41, FR119–FR137, FR168
**Phase:** MVP
**Depends on:** Epics 1–3, 5
**Notes:** Player components (HUD, time chip, toasts, grace lock); NFR26 rendering bans; launcher logout (NFR12); countdown on the monotonic clock while offline.

### Epic 7: Kitchen tickets and display

Kitchen staff get KOTs and a LAN display that survives LAN loss, and order status reaches the seat.

**FRs covered:** FR97–FR101
**Phase:** MVP
**Depends on:** Epics 5–6
**Notes:** `kitchen-ticket` component, code-paired client class, KOT ≤ 3 s, Basic+ gating.

### Epic 8: Owner sees and controls branches remotely

Owners get timely dashboards, reports, alerts, live view, audit search and exactly-once commands, and player and owner notifications go out from the cloud.

**FRs covered:** FR3, FR21, FR52, FR148–FR158, FR167, FR187
**Phase:** MVP
**Depends on:** Epics 1–7
**Notes:** R2 presigned batches, summary and rollups, command mailbox, audit builder, Tunnel + Access, step-up MFA, Gupshup adapter and outbox, owner PWA layouts; NFR7, 14, 20, 21, 23.

### Epic 9: Resilience, backups and data rights

A branch survives Hub failure through standby and promote, and keeps exports, cloud backups, the GST archive and an erasure that holds up.

**FRs covered:** FR169, FR170, FR171, FR173, FR174, FR175, FR199, FR200
**Phase:** MVP
**Depends on:** Epics 1–8
**Notes:** Epoch fencing via `branch_fence` CAS, crypto-shred keys; NFR8, 9, 13, 24.

### Epic 10: Subscriptions and tiers

Owners pay for, upgrade, cancel and lapse a subscription per branch without disruption, and a Free Android Hub can migrate to Windows when it goes paid.

**FRs covered:** FR178–FR183
**Phase:** MVP
**Depends on:** Epics 2, 4, 8, 9
**Notes:** Razorpay state machine, card/eNACH above ₹15,000, PPP band, 7-day refund; completes the paid-tier branch of the FR6 wizard step; NFR22 trusted time.

### Epic 11: Marketing site, partner programme and admin operations

Prospects find and sign up through the public site, partners earn and track commission, and Arena 360 staff curate the catalogue, approve partners and support branches.

**FRs covered:** FR188–FR196
**Phase:** MVP
**Depends on:** Epics 2, 8, 10
**Notes:** Separate partner identity, admin behind Cloudflare Access, diagnostics bundle with no PII, flash-screened media; NFR27 instrumentation.

### Epic 12: Console TVs with Pi and QR self-start

Players start and see console time on the TV through a Pi agent and a QR code.

**FRs covered:** FR142, FR143, FR144, FR145, FR147
**Phase:** v1.1
**Depends on:** Epics 1–3, 6, 7
**Notes:** Pi/QR UX-DRs, CEC adapter, read-only image.

### Epic 13: Bookings, tournaments, loyalty and game accounts

Cafés grow revenue with reservations, tournaments, loyalty and referrals, memberships, a launcher-account vault, game requests and migration from competitors.

**FRs covered:** FR22, FR42, FR43, FR55, FR56, FR138, FR139
**Phase:** v1.1
**Depends on:** Epics 3, 5, 6, 8
**Notes:** Builds on the session, POS, kiosk and owner-remote epics; no additional cross-cutting themes.

### Epic 14: Deeper back office, owner depth and platform ops

Managers run stock counts, purchase orders, petty cash, attendance and prep stations; owners get scheduled reports, cohorts, team logins and approvals; the platform gains off-site standby, auto-updates, a demo and compare pages.

**FRs covered:** FR93–FR96, FR102, FR159–FR165, FR172, FR176, FR197, FR198
**Phase:** v1.1
**Depends on:** Epics 5, 7–9, 11
**Notes:** NFR19 upgrade safety.

## Epic 1: Foundation — Hub and Kiosk install, start, and staff log in

On a seeded branch, the Hub and a locked-down Kiosk install and start, and a staff member logs in with roles, PIN elevation and idle lock, all with the WAN down.

### Story 1.1: Monorepo scaffold with pinned toolchains, generated types, message catalogue and CI

As a developer on Arena 360,
I want one monorepo with every Rust crate, Tauri app, Svelte surface, the shared UI package and CI gates in place with pinned versions,
So that every later story starts from the agreed structure and drift in types, strings or versions is caught automatically instead of in review.

**Covers:** AD-8, AD-15, AD-41, NFR16, NFR17, UX-DR103; Additional Requirements "STARTER / SCAFFOLD", "CI/CD"

**Acceptance Criteria:**

**Given** a fresh clone of the repository
**When** a developer runs the documented bootstrap command with the pinned toolchains
**Then** one package manager (pinned via `packageManager`) installs all JS workspaces and `cargo build --workspace` succeeds
**And** the Rust workspace contains the crates `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-tasks`, `arena-replay`, `arena-cloud`, `arena-hub` (bin), `arena-kiosk-core`, `arena-kiosk-svc` and `arena-pi`, each compiling with a placeholder module and one passing test

**Given** the layer table in the architecture spine
**When** CI runs the dependency-direction check
**Then** it fails if `arena-proto` depends on any workspace crate, if `arena-core` depends on `arena-api`/`arena-tasks`/`arena-cloud`, or if any Svelte app imports protocol types from anywhere other than the shared UI package's generated folder

**Given** the version pins
**When** a developer inspects the repo
**Then** `rust-toolchain.toml` pins a stable Rust, `.nvmrc`/`engines` pin Node 22.23.4+, and manifests pin Tauri 2.12.5, Tauri CLI 2.12.4, tauri-plugin-updater 2.14.0, Svelte 5.57.x, SvelteKit 3.1.x, Vite 8, TypeScript 6, adapter-static 4, adapter-cloudflare 8, Astro 7.4.x and @astrojs/svelte 9.1.x
**And** a CI step fails if any pinned version is changed without updating the pin table in the repo README

**Given** the Tauri apps
**When** `pnpm build` (or the chosen package manager equivalent) runs on the Windows CI runner
**Then** a Windows Hub Tauri app and a Kiosk Tauri app build with `adapter-static` SvelteKit front ends showing a placeholder screen
**And** SvelteKit skeletons exist and build for Kitchen display (`adapter-static`), Owner PWA, partner portal, admin app and onboarding (`adapter-cloudflare`, Workers static assets), and an Astro 7 marketing site skeleton renders one shared Svelte component

**Given** `arena-proto` defines `AppError{code, message, field?}`, an `ErrorCode` enum and an empty permission catalogue
**When** the type generator (ts-rs, chosen and recorded in the README) runs
**Then** TypeScript types are written into the shared UI package's `generated/` folder
**And** CI regenerates them and fails with a diff listing if the committed files are stale, and a lint forbids hand-written copies of protocol types

**Given** the message catalogue in the shared UI package (English only, keyed strings, seeded with the fixed Voice lines from EXPERIENCE.md)
**When** the string lint runs on a Svelte or TS file containing a user-visible hard-coded string literal
**Then** the lint fails release builds and names the file and line
**And** a catalogue key referenced in code but missing from the catalogue also fails the lint

### Story 1.2: Design tokens generated from DESIGN.md with a contrast gate

As a developer building any Arena 360 surface,
I want every colour, type, spacing, radius and elevation token generated from DESIGN.md into the shared UI package, with contrast checked in CI,
So that all surfaces look the same, the HC and light themes are complete, and no inaccessible colour pair can ship.

**Covers:** NFR15, NFR26 (Sora bundled), UX-DR1, UX-DR2, UX-DR3, UX-DR4, UX-DR5, UX-DR7, UX-DR8, UX-DR9, UX-DR10, UX-DR11, UX-DR12, UX-DR13, UX-DR14, UX-DR114

**Acceptance Criteria:**

**Given** the colour, typography, spacing, radius and elevation tokens in DESIGN.md frontmatter
**When** the token generator runs
**Then** the shared UI package exposes CSS custom properties and TS constants for the Cinema Electric dark set, Hub tones, floor-tile state tokens, chip/HUD toast tokens, six group swatches, the light "Cool paper" set, and an `-hc` twin for every component token, plus the Kiosk, Hub and Web type ramps, the 4-based spacing scale with `touch-min` 44px, and the radius scale
**And** editing DESIGN.md without regenerating fails CI as stale tokens

**Given** the generated tokens
**When** the token-contrast check runs in CI
**Then** every load-bearing pair listed in DESIGN.md meets 4.5:1 for text and 3:1 for non-text/large text in dark, HC and light themes, and the build fails naming any pair below its floor

**Given** the shared UI package
**When** any surface renders text
**Then** Sora is loaded from bundled font files with tabular numerals available (`font-variant-numeric: tabular-nums` utility), and a CI check fails if any branch-surface build (Hub, Kiosk, Kitchen) references a remote font or CDN URL

**Given** a token is renamed or removed in the generated package
**When** a Svelte app still references the old token
**Then** the build fails naming the missing token, and no surface may declare raw hex colours (a lint fails on hex literals outside the generated token files)

### Story 1.3: Shared base components meet the accessibility floor

As a staff member or player using any Arena 360 surface,
I want every shared control to be keyboard-operable, clearly focused, large enough to hit and never colour- or sound-only,
So that I can use the product at the counter without a mouse and with low vision.

**Covers:** FR202, NFR15, NFR16, UX-DR16, UX-DR17, UX-DR18, UX-DR19, UX-DR22, UX-DR23, UX-DR25, UX-DR108, UX-DR109, UX-DR110, UX-DR113

**Acceptance Criteria:**

**Given** the shared UI package
**When** a developer opens the dev-only component gallery route
**Then** it shows `focus-ring`, `text-field` (incl. password variant), `button-primary`, `button-secondary`, `button-warning`, `status-badge`, `alert-badge`, `card` and `tab-control` in dark, HC and light themes, each in rest, hover, pressed, focused, disabled and error states where applicable

**Given** any focusable shared component
**When** it receives keyboard focus
**Then** the `focus-ring` shows as 2px magenta inner + 2px white outer at 2px offset (HC: 4px yellow; light: 2px `#AD0E63`) and is never suppressed by `outline: none`
**And** a Playwright test tabs through the gallery and asserts a visible focus indicator on every stop

**Given** a disabled button
**When** it renders
**Then** it shows at 40% opacity with no hover, a not-allowed cursor, and its reason as inline text at full ink (the reason comes from a catalogue key, never empty)

**Given** a `text-field` with a validation error
**When** the form is submitted or the field blurs
**Then** the field shows a 2px danger border, a ⚠ glyph and the message below, focus moves to the first invalid field, and the error is announced through a live region
**And** the label is programmatically linked, paste is allowed, and the password variant has a show/hide control

**Given** every interactive shared component
**When** the automated target-size check runs
**Then** each hit area is at least 44 × 44 px in every theme, and the check fails the build otherwise

**Given** `status-badge` and `alert-badge`
**When** they render a state
**Then** the state is carried by text (and glyph where specified), never by colour alone, and `alert-badge` count changes are announced in a polite live region

**Given** the HC theme is active
**When** components transition between states
**Then** transitions cut to instant, no shadows or translucency render, and every interactive surface shows a 2px white outline (hover 3px)

**Given** an axe-core scan of the component gallery in all three themes
**When** CI runs it
**Then** there are zero serious or critical violations, and a pseudo-locale run with strings lengthened by 30% shows no clipped or overlapping text in any component

### Story 1.4: Hub service starts as the single writer of its branch database

As a café operator,
I want the Hub service to open its local database safely and know it is the only writer for the branch,
So that branch data can never be corrupted by two writers or lost on a crash.

**Covers:** AD-1, AD-8, AD-38 (offline provisional epoch), NFR9, NFR25 (Hub logs), NFR17

**Acceptance Criteria:**

**Given** a machine with no Hub database
**When** `arena-hub` starts
**Then** it creates the SQLite file in WAL mode with `synchronous=FULL` (fsync on commit), applies the numbered embedded migrations in one transaction, and records each in `schema_version`
**And** it mints and persists a `hub_instance_id` once at first start

**Given** a database whose `schema_version` is newer than the binary knows
**When** `arena-hub` starts
**Then** it refuses to start with `SCHEMA_TOO_NEW` and a log line naming both versions, leaving the file untouched

**Given** no cloud is reachable at first start
**When** the lease task starts with an empty `lease` table
**Then** the Hub mints a provisional epoch `(1, hub_instance_id)` marked `provisional = 1`, takes the lease, and renews it every 3 s with a 15 s expiry

**Given** a `lease` row held by a different instance with a higher epoch that has not expired
**When** a second `arena-hub` process starts on the same database
**Then** it starts in follower role, opens no writer channel, and reports `role: follower` on `/healthz`

**Given** a running primary
**When** its lease is superseded by a higher epoch
**Then** it closes its writer channel within one renewal tick, so any later write attempt fails `NOT_PRIMARY`

**Given** a running Hub
**When** `GET /healthz` is called on the loopback interface
**Then** it returns db open, role, lease state, current epoch and schema version as JSON

**Given** the Hub is running
**When** it logs
**Then** logs are `tracing` JSON lines in a 50 MB on-disk ring buffer retained 14 days, carrying no PII

**Given** the Hub process is killed with `SIGKILL`/`TerminateProcess` immediately after a committed write
**When** it restarts
**Then** the committed write is present and the database passes `PRAGMA integrity_check`

### Story 1.5: Every change goes through apply() into a gap-free change log, starting with a seeded branch

As a café owner,
I want every change at my branch recorded once, in order, with who made it,
So that the branch has a trustworthy audit trail and can later be replicated and replayed exactly.

**Covers:** AD-2, AD-3 (`branch.bootstrap`), AD-7, NFR11 (audit on every write), NFR25 (typed actor), FR110 (recording half)

**Acceptance Criteria:**

**Given** the Hub is primary
**When** any intent is submitted
**Then** a single Writer actor processes it in one `BEGIN IMMEDIATE` transaction in the order permission → epoch check → TTL/dedupe → `base_rev` → validate → mutate → stamp → append `changes` → commit → emit, and no I/O other than SQLite happens inside the transaction
**And** a failure at any step rolls back and returns exactly one typed `AppError`

**Given** a committed intent
**When** the `changes` table is inspected
**Then** each written row has one change row with `seq` (gap-free per epoch), `epoch`, `tbl`, `row_id`, `op`, the full row after write as a MessagePack map, `intent_id`, `intent_kind`, typed `actor_kind`/`actor_id` and `at`, and the table rejects UPDATE and DELETE (append-only)

**Given** the dev fixture `arena-hub dev seed` in a build with the `dev-fixtures` feature
**When** it runs against an empty database
**Then** it submits a `branch.bootstrap` intent as `actor{kind: system}` that creates the `branch` row (name, `branch_code`, ISO 4217 currency, tz, `day_start_min`, FY start) and the `settings` table defaults, visible as change rows
**And** running it a second time is rejected as already bootstrapped, and a release build has no `dev seed` command

**Given** an intent on a Hub in follower role or with a superseded epoch
**When** it reaches `apply()`
**Then** it fails `NOT_PRIMARY` and writes nothing

**Given** the change log of the seeded branch
**When** a test replays `changes` into an empty database
**Then** the resulting rows equal the source database's branch rows, and `seq` has no gaps for the epoch

### Story 1.6: Idempotent intents, optimistic row versions and property tests on apply()

As a café owner,
I want retried or late actions to apply exactly once and never overwrite a newer change,
So that flaky connections and double taps can never double-charge or silently undo someone's edit.

**Covers:** AD-14, AD-39, AD-2, NFR11, NFR25

**Acceptance Criteria:**

**Given** an intent whose `idempotency_key` has already been applied
**When** it is submitted again
**Then** it is not re-applied, the original outcome is returned, and no new change rows appear; dedupe records in `applied_intents` are retained 30 days

**Given** an intent older than its TTL (≤ 7 days)
**When** it is submitted
**Then** it is rejected `EXPIRED`

**Given** an edit intent carrying a `base_rev` that differs from the row's current `rev`
**When** it is applied
**Then** it is rejected `STALE` and nothing changes; a successful edit increments `rev` by exactly 1

**Given** an intent with no `idempotency_key` or a malformed one
**When** it is submitted
**Then** it is rejected with `BAD_FRAME` before any transaction opens

**Given** dedupe records older than 30 days
**When** the daily housekeeping runs
**Then** they are pruned, and no record younger than 30 days is removed

**Given** the `proptest` suite generating random sequences of the intents defined so far against an in-memory Hub
**When** it runs in CI on every push
**Then** after every step `changes.seq` is gap-free, duplicate idempotency keys apply once, stale `base_rev` is rejected, and replaying `changes` into an empty database reproduces the same rows

### Story 1.7: LAN protocol envelope, hello handshake and client classes

As a café operator,
I want every device on the café LAN to talk to the Hub through one authenticated protocol,
So that guests on the shared Wi-Fi cannot reach staff, money or device actions.

**Covers:** AD-6, AD-8 (frame `v`, two-release window), AD-25, AD-42, NFR11, NFR17

**Acceptance Criteria:**

**Given** the Hub is running
**When** it opens its LAN listeners
**Then** it serves WebSocket on `:7400` over TLS with a Hub certificate generated at first start (fingerprint stored) and HTTP(S) on `:7401`, and the Windows installer opens those ports only on the Private network profile; the Hub opens no other inbound port

**Given** a connection to `:7400`
**When** any frame is exchanged
**Then** every frame is `{v, kind, rid}` and replies correlate on `rid`; an unknown `kind` returns `UNSUPPORTED`, unknown fields are ignored, and a malformed frame returns `BAD_FRAME` without closing other sessions

**Given** a new connection
**When** the first frame is not `hello{credential, class, versions, capabilities}`
**Then** the Hub rejects it and closes the connection

**Given** a dev-fixture-seeded kiosk device token (stored as an argon2id hash in the `devices` table created by this story)
**When** a client sends `hello` with that token and `class: paired`
**Then** the session is accepted, the device's reported version is recorded, and the Hub pushes subsequent state as events (no client polling)
**And** a wrong or unknown token returns `TOKEN_INVALID` and closes the connection

**Given** a paired device whose `hello.versions` is older than the previous two releases
**When** it connects
**Then** the Hub rejects it with `UNSUPPORTED` naming the minimum version; versions within the window are accepted

**Given** a connected paired device
**When** its token is revoked on the Hub
**Then** its socket is closed immediately with `TOKEN_REVOKED` and reconnects with that token are refused

**Given** the class scope table in `arena-proto` (paired, kds, phone)
**When** a `kds` or `phone` class client calls an intent outside its scope
**Then** the Hub returns `FORBIDDEN` and logs the attempt; a CI test asserts no browser-class scope includes any staff, money or device intent

**Given** the shared Rust client in `arena-kiosk-core`
**When** a request gets no reply within 5 s
**Then** it surfaces a `HUB_SLOW` status to its caller without blocking other requests

### Story 1.8: Hub app shell with left rail, header and floor placeholder on the seeded branch

As counter staff,
I want the Hub app to open straight into a familiar frame with navigation, the branch header and the floor area,
So that I can find every area of the Hub by mouse, touch or keyboard from the first day.

**Covers:** NFR2, NFR17, UX-DR58, UX-DR59, UX-DR60 (shell), UX-DR87 (shell), UX-DR104, UX-DR105 (navigation keys), UX-DR113

**Acceptance Criteria:**

**Given** the Windows Hub installer built by CI
**When** it is run on Windows 10 or 11
**Then** it installs the Tauri Hub app, which starts the `arena-hub` service and opens the Hub UI; on an unsupported Windows version the installer stops with a message naming the minimum version

**Given** a seeded branch
**When** the Hub UI opens on a window ≥ 840px wide
**Then** it shows the 88px `hub-rail` with labelled items Floor view · POS · Players · Sessions · Inventory · Shift · Reports · Settings · System, the 72px `hub-header` with branch name and clock, and Floor view selected as home
**And** the floor area renders from the local database immediately with no skeleton, showing an empty-state zone message from the catalogue and an "Unassigned" section listing the dev-seeded kiosk by name

**Given** a rail item whose screen is not built yet
**When** staff select it
**Then** a placeholder view with a single `h1` from the catalogue opens; no rail item is hidden or broken

**Given** the Hub window is narrower than 840px
**When** the layout reflows
**Then** the rail is replaced by the 64px `hub-bottom-bar` (Floor · POS · Players · Shift · More), More opens a bottom sheet listing Sessions, Inventory, Reports, Settings, System, and closing it returns to the current screen; all targets stay ≥ 44px

**Given** keyboard focus is not in a text field
**When** staff press Ctrl+1, Ctrl+2, Ctrl+3 or Ctrl+4
**Then** Floor, POS, Players or Shift opens respectively
**And** the same keys typed while a text field has focus do nothing

**Given** the Hub UI
**When** an axe scan runs on the shell
**Then** it has `header`, `nav` and `main` landmarks, one `h1` per view and zero serious/critical violations

**Given** the per-release benchmark job on the reference hardware runner (2015 i3 + HDD)
**When** the Hub has been idle for 5 minutes on the seeded branch
**Then** the Rust Hub service uses < 50 MB RAM and the Hub UI (WebView2) < 250 MB with no GPU use, and the job fails the release otherwise

### Story 1.9: Staff sign in to the Hub with roles and permissions

As a café owner,
I want each staff member to sign in as themselves and only do what their role allows,
So that every action is attributable and counter staff cannot change things reserved for managers.

**Covers:** FR103, AD-10 (staff password + PIN, Argon2id), AD-30, NFR11 (RBAC inside `apply()`), NFR25, UX-DR18, UX-DR60 (signed-in staff)

**Acceptance Criteria:**

**Given** `arena-proto`
**When** the permission catalogue is generated
**Then** it holds the fixed ~40 permissions grouped by module, each intent kind maps to exactly one permission, and a CI test fails if any intent kind has zero or more than one permission

**Given** the migration added by this story (`staff`, `roles`, `role_perms`)
**When** the branch is seeded
**Then** built-in roles Owner, Manager, Counter, Kitchen and Kiosk-device exist with the LLD permission sets and are marked `builtin`
**And** the dev fixture seeds one staff member per staff role with Argon2id-hashed password and PIN (dev only; real setup-code onboarding is Epic 2)

**Given** a `role.upsert` intent for a custom role
**When** it lists permissions each marked allow or deny
**Then** a role is created only from catalogue permissions; an unknown permission is rejected with a field error, and any attempt to modify a built-in role is rejected

**Given** the Hub sign-in screen
**When** a staff member enters a correct login and password
**Then** they are signed in, their name and role show in the `hub-header`, and Floor view opens

**Given** a wrong login or password
**When** sign-in is submitted
**Then** the error does not say which field failed, nothing about the account is revealed, and after 5 consecutive failures sign-in for that login is paused for 60 s with a visible countdown

**Given** a signed-in Counter
**When** they submit an intent requiring a permission their role does not hold
**Then** `apply()` rejects it with `PERMISSION_DENIED` before any write, and the UI shows the plain refusal inline (no blocking modal)

**Given** a signed-in staff member
**When** they choose Sign out
**Then** the Hub returns to the sign-in screen and subsequent intents from that UI are rejected until someone signs in

**Given** any intent submitted from the Hub UI
**When** it is applied
**Then** its change rows carry `actor{kind: staff, id}` of the signed-in staff member, never a shared or anonymous actor

### Story 1.10: Branch locale settings — currency, number format and timezone

As a branch manager,
I want to set the branch currency, symbol position, number format and timezone,
So that every amount and time on the Hub reads the way my café expects, and day boundaries stay correct.

**Covers:** FR111, AD-7, NFR16 (locale formatting, currency per branch), UX-DR18, UX-DR88 (Settings)

**Acceptance Criteria:**

**Given** a Manager or Owner on Settings → Branch
**When** they open the screen
**Then** it shows currency (ISO 4217 code), symbol position (before/after), number format (e.g. Indian 1,23,456.78 or international 123,456.78), timezone (IANA) and day start, each with its current value

**Given** a valid change
**When** they save
**Then** a `settings.set` intent is applied with the setting key and value, the change appears in the change log, and a `settings.changed` event updates every open Hub view without a reload

**Given** an invalid currency code or an unknown timezone
**When** they save
**Then** the field shows an inline error from the catalogue and nothing is written

**Given** a currency is set
**When** any amount is displayed on the Hub
**Then** it is formatted from integer minor units using the code's exponent, the chosen symbol position and number format, with tabular numerals, via one shared formatter in the UI package

**Given** a timezone or day-start change saved during business day D
**When** `apply()` stamps `business_day` on later rows
**Then** rows in day D still use the old tz/day start and the new values take effect from business day D+1, and the settings screen shows "takes effect from <date>"

**Given** two Hub windows editing the same setting
**When** the second saves with an outdated `base_rev`
**Then** it is rejected `STALE` and the screen shows the current value with a plain message to retry

**Given** a Counter
**When** they open Settings → Branch
**Then** the fields are read-only and the save action states why it is unavailable

### Story 1.11: Inline manager PIN elevation

As counter staff,
I want a manager to approve an action by typing their PIN right where I am,
So that I can finish the job without signing out or leaving the screen, and the audit shows who approved it.

**Covers:** FR105, AD-30, UX-DR66, UX-DR101 (role lacks permission), UX-DR104

**Acceptance Criteria:**

**Given** a signed-in Counter on Settings → Branch
**When** they activate Save on a setting their role lacks permission for
**Then** the action stays visible and an `inline-pin` (4–6 cells, `surface-2` cells with `border-strong` edge, numeric keypad + keyboard) opens in place in that panel without navigating away

**Given** the inline PIN is open
**When** a Manager's correct PIN is entered
**Then** the Hub verifies it with Argon2id, applies the intent with permission checked against the manager, and the change row records `actor` = the Counter and `elevated_by{kind: pin, id: manager, mode: hub, verifier_version}`

**Given** an incorrect PIN, or the PIN of a staff member whose role also lacks the permission
**When** it is submitted
**Then** nothing is applied, the cells clear, and a plain inline message says the PIN can't approve this action without naming anyone

**Given** an intent that requires a reason in addition to elevation
**When** the PIN is accepted
**Then** the reason picker appears in the same place and the intent is applied only once a reason is chosen

**Given** a PIN was entered
**When** the intent completes or is cancelled with Esc
**Then** the PIN is cleared from memory and the UI, is never sent to any other screen or device, and focus returns to the control that opened it

**Given** a manager's PIN has been rotated
**When** an elevation carries the old `verifier_version`
**Then** it is rejected and the user is asked to enter the PIN again

**Given** the full flow
**When** it is driven by keyboard only
**Then** opening, entering the PIN, choosing a reason and submitting all work without a mouse

### Story 1.12: Hub idle lock with PIN unlock in place

As a café owner,
I want an unattended Hub to lock itself and unlock with a PIN back to the same screen,
So that nobody can use a signed-in counter while staff step away, and staff don't lose their place.

**Covers:** FR106, AD-30 (5-min idle lock), UX-DR102 (idle lock), UX-DR66

**Acceptance Criteria:**

**Given** a signed-in staff member and the default idle timeout of 5 minutes
**When** there is no keyboard, mouse or touch input for 5 minutes
**Then** the Hub UI shows a lock screen with the signed-in staff name and an `inline-pin`, and nothing behind it can be operated

**Given** a Manager changes the idle timeout in Settings (allowed range shown inline)
**When** the setting is saved
**Then** the new timeout applies to every Hub UI from the next idle period, and an out-of-range value is rejected with a field error

**Given** the lock screen
**When** the same staff member enters their correct PIN
**Then** the Hub returns to exactly the screen, scroll position and unsaved field contents shown before the lock

**Given** the lock screen
**When** a different staff member enters their own correct PIN
**Then** the Hub stays on the same screen, the header shows the new staff member, and all later intents carry the new actor

**Given** the lock screen
**When** a wrong PIN is entered 5 times in a row
**Then** the PIN entry pauses for 60 s with a visible countdown, and Sign out remains available

**Given** the Hub UI is locked
**When** LAN events arrive (e.g. device state)
**Then** the Hub service keeps processing them and the UI shows the current state after unlock; locking never stops the service

**Given** the lock and unlock events
**When** they occur
**Then** they are written to the Hub log with staff id and time (no PIN material), and unlock is done entirely on the keyboard without a mouse

### Story 1.13: Audit log viewer on the Hub

As a café owner or manager,
I want to browse every change at the branch with who made it, when, and what it was before and after,
So that I can answer "who changed this?" at the branch without the internet.

**Covers:** FR110, AD-2 (log = audit), AD-30 (both actors), NFR11, NFR25, UX-DR23, UX-DR88 (System → Audit)

**Acceptance Criteria:**

**Given** a signed-in Manager or Owner (permission `audit.view`)
**When** they open System → Audit
**Then** the newest changes are listed first with time (branch timezone), actor name and kind, module, intent and the affected record

**Given** a change row
**When** staff open it
**Then** the before and after values are shown side by side, where "before" is the row from the previous change to the same record (or "Created" if none), and only changed fields are highlighted with a text marker, not colour alone

**Given** an elevated change
**When** it is displayed
**Then** both the acting staff member and the approving manager (`elevated_by`) are shown, with the elevation mode

**Given** filters for actor, module and date range
**When** staff combine them
**Then** only matching changes are listed, filters can be cleared in one action, and no match shows the catalogue empty-state line

**Given** a branch with 100,000 change rows
**When** a filtered query runs on the reference hardware
**Then** the first page renders in under 1 s and further pages load on scroll or "Load more"

**Given** a Counter without `audit.view`
**When** they select System → Audit
**Then** the action stays visible and opens `inline-pin` for manager elevation (Story 1.11); without elevation no audit data is shown

**Given** the audit viewer
**When** it is used
**Then** it is read-only (no edit or delete controls exist), it works with no WAN, and personal data columns are shown only as ids

### Story 1.14: Kiosk privilege split — locked service and unprivileged UI

As a café operator,
I want the kiosk's enforcement to run as a protected Windows service separate from the player-facing app,
So that a player who crashes or kills the app cannot unlock the PC, and the device token stays protected.

**Covers:** AD-22 (DPAPI token), AD-31, AD-25 (kiosk client), NFR2 (Kiosk < 80 MB), NFR11, NFR17

**Acceptance Criteria:**

**Given** the Kiosk installer on Windows 10 1803+ or 11, 64-bit, with ≥ 4 GB RAM
**When** it runs
**Then** it installs `arena-kiosk-svc` as a LocalSystem Windows service and the `arena-kiosk` Tauri app, bundles the WebView2 Evergreen bootstrapper if the runtime is missing, and refuses to install on unsupported Windows versions, 32-bit or < 4 GB RAM with a message naming the requirement

**Given** the dev fixture kiosk token and a stored Hub IP (dev only; pairing and mDNS are Epic 2)
**When** the service starts
**Then** it stores the token encrypted with DPAPI, connects to the Hub over WebSocket TLS pinned to the Hub certificate fingerprint, sends `hello{class: paired}`, and the Hub shows the kiosk as online

**Given** the Hub certificate presented does not match the pinned fingerprint
**When** the service connects
**Then** it refuses the connection and logs the mismatch; it never falls back to an unpinned connection

**Given** the Hub becomes unreachable
**When** the service reconnects
**Then** it retries with jittered backoff between 2 and 30 s and the app shows the "Reconnecting…" catalogue line without blocking

**Given** the Tauri app runs as the standard `player` user
**When** it needs Hub data or enforcement actions
**Then** it talks only to the service over a named pipe whose ACL admits only the kiosk app's user, and the app has no direct Hub socket, admin rights or access to the token

**Given** the app is killed or hangs
**When** the service misses 3 consecutive 2 s pipe pings
**Then** the service restarts the app with its last state, and the kiosk screen stays covered (no desktop exposed) throughout

**Given** a kiosk idle with no session on reference hardware
**When** the per-release benchmark measures it
**Then** service + app together use < 80 MB RAM with no GPU use, and the job fails the release otherwise

**Given** kiosk-originated log events
**When** the Hub is unreachable
**Then** they are queued as intents with client idempotency keys and replayed in creation order on reconnect, applying once

### Story 1.15: Kiosk shell replacement and Windows lockdown

As a café operator,
I want the kiosk to replace the Windows shell for the player account and block escape routes,
So that a player cannot reach the desktop, Task Manager or settings on a café PC.

**Covers:** FR116, FR117 (USB autorun), NFR12, NFR17

**Acceptance Criteria:**

**Given** the Kiosk installer
**When** it completes
**Then** a standard `player` account exists whose per-user `Winlogon\Shell` is `arena-kiosk`, so Explorer never starts for that account
**And** policy keys `DisableTaskMgr`, `NoRun`, `NoClose`, `DisableLockWorkstation`, `DisableChangePassword` are set, removable drives are hidden and USB autorun is off for that account

**Given** the `player` account is signed in
**When** the player presses Win-key combinations or Alt+F4 on kiosk windows
**Then** the low-level keyboard hook swallows them and nothing outside the kiosk opens

**Given** the player presses Ctrl+Alt+Del
**When** the secure screen appears
**Then** only the options left by policy (no Task Manager, lock, change password or sign-out to desktop) are offered, and returning lands back on the kiosk

**Given** no session is active
**When** the kiosk is running
**Then** the main window is `WS_EX_TOPMOST` and covers every monitor, showing a locked placeholder screen from the catalogue

**Given** a USB drive with an autorun payload is inserted
**When** Windows detects it
**Then** nothing runs and the drive is not visible to the player account

**Given** an administrator account on the same PC
**When** it signs in
**Then** it gets the normal Windows shell; the lockdown applies only to the `player` account

**Given** the Kiosk is uninstalled by an administrator
**When** the uninstaller completes
**Then** the `player` account's shell and policy keys are restored to Windows defaults

### Story 1.16: Automated 30-minute kiosk escape suite on Windows 10 and 11

As a café operator,
I want every kiosk release proven against scripted escape attempts on both supported Windows versions,
So that a lockdown regression is caught before it reaches a café PC.

**Covers:** FR116, NFR12, NFR17

**Acceptance Criteria:**

**Given** Windows 10 (1803 or later) and Windows 11 VM images with the release-candidate Kiosk installed
**When** the escape suite runs in CI before release
**Then** it drives, for at least 30 minutes per OS, key combos (Win-key, Alt+Tab, Alt+F4, Ctrl+Shift+Esc, Ctrl+Alt+Del options), Task Manager, Run, Settings, USB insertion, process spawn via file dialogs, Sticky Keys and other accessibility shortcuts, and killing the kiosk app

**Given** the suite is running
**When** any attempt reaches the desktop, Explorer, a command shell, Task Manager or a non-allowed process window
**Then** the suite records the step, a screenshot and the OS, and fails the release

**Given** a passing run
**When** the suite finishes
**Then** a run report (OS build, kiosk version, attempts, duration, result) is stored as a release artefact

**Given** a new escape technique found in manual testing
**When** it is added as a scripted case
**Then** it runs on both OS images in every later run

**Given** the VM images
**When** the suite starts
**Then** each run starts from a clean snapshot, so one run's state cannot hide another's failure

### Story 1.17: Kiosk process allow/block list

As a café operator,
I want the kiosk to kill any program that isn't allowed, tell the player, and log it at the counter,
So that players can't run tools or apps that break the café's rules or the lockdown.

**Covers:** FR117, AD-31, AD-14, NFR12, UX-DR101 (blocked app), UX-DR103

**Acceptance Criteria:**

**Given** the Hub holds the branch allow-list and block-list (seeded defaults: kiosk, its service, Windows essentials; block-list includes cmd, PowerShell, regedit, Task Manager)
**When** the kiosk service connects
**Then** it receives the lists and caches them for offline use, and any later list change is pushed by `settings.changed` and applied without restart

**Given** a process starts in the `player` session
**When** it is on the block-list, or not on the allow-list
**Then** the service terminates it within 200 ms of the process-creation event, matching allowed programs by signed path

**Given** a process was killed
**When** the kill completes
**Then** the kiosk shows "That app isn't allowed on this PC." as a non-focus-stealing toast that persists until dismissed, and the service sends a `kiosk.process_blocked` intent (device actor, executable name and path) that appears in the Hub audit log

**Given** the Hub is unreachable
**When** a blocked process is killed
**Then** enforcement uses the cached lists, the event is queued with its idempotency key, and it reaches the Hub audit exactly once after reconnect

**Given** a renamed copy of a blocked executable or an allowed name at an unsigned path
**When** it is launched
**Then** it is killed, because matching uses signed path and not file name alone

**Given** the app or service is restarted
**When** enforcement resumes
**Then** processes already running that are not allowed are terminated within 2 s of service start

### Story 1.18: Kiosk admin exit with manager PIN, offline verifier and Hub remote unlock

As a branch manager,
I want to drop a kiosk to the desktop for maintenance with my PIN at the PC, even when the Hub is down, or remotely from the Hub,
So that I can fix a PC without reinstalling, and every exit is logged.

**Covers:** FR118, AD-30 (cached verifier), AD-31, UX-DR66, UX-DR106 (admin exit combo)

**Acceptance Criteria:**

**Given** a kiosk with the Hub reachable
**When** someone presses Ctrl+Alt+Shift+A
**Then** an `inline-pin` opens in a kiosk panel (no other visible hint of the combo exists), and a correct manager PIN verified by the Hub starts Explorer for 15 minutes with a countdown badge, after which the kiosk re-locks

**Given** the Hub is reachable
**When** each manager's PIN is set or rotated
**Then** the Hub pushes a versioned manager-PIN verifier to every connected kiosk, and the service stores it protected by DPAPI

**Given** the Hub is unreachable
**When** a manager enters a correct PIN after the combo
**Then** the service verifies it against the cached verifier, grants the same 15-minute exit, and queues an exit intent with `elevated_by.mode = cached_verifier` that is applied and flagged in the audit after reconnect

**Given** a cached verifier whose version is older than the Hub's current one
**When** the kiosk reconnects
**Then** the old verifier is replaced, and an offline exit made with a rotated-out PIN is accepted only if its verifier version was current at the time of exit, otherwise it is flagged in the audit for review

**Given** a wrong PIN, or a PIN of staff without the admin-exit permission
**When** it is entered
**Then** nothing exits, the cells clear, and the failed attempt is logged; after 5 consecutive failures the prompt pauses for 60 s

**Given** a Manager on the Hub floor placeholder (Story 1.8) selects the dev-seeded kiosk's entry
**When** they choose Remote unlock and pass the `inline-pin`
**Then** the Hub applies a `device.admin_unlock` intent and the kiosk drops to the desktop for 15 minutes within 5 s; if the kiosk does not confirm within 5 s the entry shows "Failed — retry"

**Given** any admin exit
**When** the audit log is viewed
**Then** it shows the kiosk, the approving manager, the mode (Hub, cached verifier or remote), and start and re-lock times

### Story 1.19: High-contrast theme as a per-device branch setting

As a branch manager,
I want to switch the high-contrast theme on for a specific Hub or kiosk,
So that staff or players with low vision can use that screen without changing anyone else's.

**Covers:** FR202 (HC per device), FR111 (settings path), NFR15, UX-DR4, UX-DR6, UX-DR108, UX-DR114

**Acceptance Criteria:**

**Given** a Manager or Owner on Settings → Devices & display
**When** they view the list
**Then** the Hub itself and each known kiosk appear with a high-contrast on/off control, labelled by device name

**Given** HC is switched on for a kiosk
**When** the setting is saved
**Then** a `settings.set` intent records it per device, and that kiosk switches to `-hc` tokens within 2 s without a restart, while other devices are unchanged

**Given** HC is active on a surface
**When** it renders
**Then** surfaces are pure black with white text and hairlines, the accent is `#FFE14D`, chips are inverted, every interactive surface has a 2px white outline, the focus ring is 4px yellow, art/video/translucency/shadows are removed and motion cuts to instant

**Given** a kiosk that last received HC = on
**When** it cold-starts with the Hub unreachable
**Then** it applies HC from its cached setting before the first paint

**Given** a player at a kiosk
**When** they look for a theme control
**Then** none exists on any player screen; Hub, Kiosk and Kitchen remain dark-only plus HC with no light theme or `theme-toggle`

**Given** a Counter without settings permission
**When** they change the HC control
**Then** the `inline-pin` opens in place for manager elevation, and the change is audited with both actors

**Given** the HC theme on the Hub shell and kiosk placeholder screens
**When** the contrast and axe checks run in CI
**Then** all load-bearing pairs meet 4.5:1 text and 3:1 non-text, with zero serious/critical axe violations

### Story 1.20: The branch runs with the WAN down and shows which link is offline

As counter staff,
I want the Hub and kiosks to keep working exactly the same when the internet drops, with a clear chip telling me what's down,
So that the café never stops trading because of a broken connection.

**Covers:** FR166, NFR1 (offline reliability, Epic 1 scope), NFR26 (no runtime font/CDN fetch), UX-DR76 (`chip-offline`), UX-DR98 (Hub offline state), UX-DR113 (live region)

**Acceptance Criteria:**

**Given** the Hub service is running
**When** its connectivity probe runs every 30 s
**Then** it distinguishes "internet unreachable" (no general egress) from "cloud unreachable" (egress works but the Arena 360 API does not), with neither probe blocking any intent

**Given** the internet or the cloud becomes unreachable
**When** the next probe fails
**Then** the `hub-header` shows `chip-offline` naming which one ("Internet offline" or "Cloud unreachable"), announced once through a polite live region, and nothing else in the UI changes

**Given** connectivity returns
**When** the next probe succeeds
**Then** the chip disappears within 30 s with no restart or user action

**Given** an integration test with all WAN egress blocked at the network layer from first boot
**When** it runs the Epic 1 flows — Hub install and start, dev seed, staff sign-in, branch settings, inline PIN elevation, idle lock and unlock, audit viewer, kiosk start and Hub connection, process blocking, admin exit (Hub and cached verifier), HC toggle
**Then** every flow succeeds with results identical to a WAN-up run, and the test fails if any process attempted an outbound connection other than the connectivity probe

**Given** the Hub, Kiosk and Kitchen front ends
**When** they load with the WAN down
**Then** all fonts, icons, tokens and scripts come from bundled assets, and no screen shows a broken or loading state caused by a remote resource

**Given** a WAN outage longer than 8 hours on the test LAN with the Hub and a kiosk running Epic 1 flows
**When** the WAN returns
**Then** no change rows are lost or duplicated (seq gap-free), and the Hub log shows no error caused by the outage

## Epic 2: Owner onboarding and pairing

An owner signs up with OTP/TOTP, completes the wizard, pairs the Hub and kiosks, invites staff and runs a 2-minute test session.

> Draft notes: this epic replaces Epic 1's dev fixtures on the real path — `branch.bootstrap` now arrives at first Hub pairing (Story 2.12), staff credentials come from one-time setup codes (Story 2.13), and kiosk tokens and the Hub address come from mDNS discovery and code pairing (Stories 2.16–2.17). The `dev-fixtures` feature stays for automated tests only. FR6 is delivered for the Free path only; the paid-tier payment branch is Epic 10. Because a Windows Hub is refused on Free (FR14) and the Android Hub ships in Epic 4, Story 2.7 adds an operator-issued **comp entitlement** so internal and beta cafés can pair a Windows Hub end to end before Epic 10. The command mailbox built here (Story 2.14) carries only the **setup command kinds** (`branch.bootstrap`, `group.upsert`, `device.assign`, `device.register`, `staff.upsert`, `settings.set` for branch name/tz, `setup.test_session`, `branch.go_live`), which run on every tier; the full owner command UI, fan-out, step-up MFA, sweeper and fencing are Epic 8. In wizard step 4 the owner creates groups and drags devices into them in the web wizard, delivered to the Hub as setup commands; devices arriving are shown from the heartbeat. Steps 5 and 8 (rates, printer) are done at the Hub and the web wizard mirrors their progress from the heartbeat. Each story lists what it covers on its **Covers** line.

### Story 2.1: Arena 360 cloud runs in India behind Cloudflare

As a café owner signing up,
I want the Arena 360 service I talk to hosted in India, reachable only over HTTPS and returning plain, consistent errors,
So that my account data stays in India and every onboarding screen behaves predictably.

**Covers:** NFR14, AD-9, AD-22, AD-40; Additional Requirements "Infrastructure and deployment" (Hostinger K8s, Turso Mumbai, environments), "Build spikes" (single SQLite linkage)

**Acceptance Criteria:**

**Given** the `arena-cloud` binary started with `--role control`
**When** it is deployed by CI to the Kubernetes cluster on the Hostinger VPS in Mumbai
**Then** it runs as a Deployment in a `staging` namespace on every merge to main and in `production` on a release tag, served at `api.staging.arena360.app` and `api.arena360.app` behind Cloudflare (TLS terminated at the edge, origin reachable only from Cloudflare IP ranges)
**And** `GET /healthz` returns role, version, `gz_control` reachability and migration version

**Given** the `gz_control` database
**When** the control role starts
**Then** it connects to Turso in the Mumbai region via libsql (local SQLite file in dev), applies numbered forward-only embedded migrations recorded in a migrations table, and refuses to start with `SCHEMA_TOO_NEW` if the database is newer than the binary
**And** this story creates no business tables; later stories add only the tables they need
**And** `arena-cloud` links one SQLite library shared by rusqlite and libsql (build spike), checked by a CI test that fails on duplicate symbol linkage

**Given** cloud secrets (Turso token, signing keys added later)
**When** CI deploys
**Then** they are decrypted from SOPS 3.13.3 + age 1.3.2 files in the repo into Kubernetes secrets, and a CI check fails if any plaintext secret or VPS env file is committed

**Given** any `/v1` request that fails
**When** the API responds
**Then** the body is the `AppError{code, message, field?}` envelope with a stable code from `arena-proto`, never a stack trace, and unknown paths return `NOT_FOUND`

**Given** the dev environment
**When** a developer runs the documented `arena-cloud --role all --dev` command
**Then** the cloud runs locally with SQLite for `gz_control` and MinIO for R2, with no Turso or Cloudflare account required

### Story 2.2: Control data is backed up nightly and the cloud is observable

As a café owner,
I want Arena 360's account and licence data backed up every night and the service watched for errors,
So that a cloud failure never loses my account and problems are found before they reach me.

**Covers:** NFR21, NFR25, NFR13 (no PII in telemetry); Spine §Operational Envelope "DR", "Observability"

**Acceptance Criteria:**

**Given** the production `gz_control`
**When** the `jobs` role CronJob runs nightly
**Then** it writes an encrypted dump to an off-provider bucket and keeps the latest 30 nightly copies, deleting older ones

**Given** a nightly dump
**When** the documented restore procedure is run into staging
**Then** the control role starts on the restored database with every table and row count matching the dump, and the drill is run once before GA to prove the 4 h RTO

**Given** a nightly backup fails
**When** the job exits non-zero
**Then** an alert fires to the on-call channel and the next run is not skipped

**Given** any `/v1` request to `arena-cloud`
**When** it is handled
**Then** an OpenTelemetry trace with role, route, status and tenant/branch ids (when known) is exported to Grafana Cloud, and unhandled errors go to Sentry

**Given** traces, logs and Sentry events
**When** they are recorded
**Then** request bodies, phone numbers, emails, OTPs, passwords, tokens and codes are scrubbed, verified by a test that sends a sign-up request and asserts none of its field values appear in exported telemetry

### Story 2.3: Owner signs up with phone OTP

As a café owner,
I want to sign up with my name, phone, email, café name and city and confirm my phone with a one-time code,
So that my account exists only once I've proved the phone is mine.

**Covers:** FR1, AD-10 (SMS OTP at sign-up), AD-23, AD-27/AD-34 (messaging adapter, OTP only), NFR11, NFR13, NFR15, NFR23, UX-DR5, UX-DR6, UX-DR18, UX-DR19, UX-DR20, UX-DR91, UX-DR113

**Acceptance Criteria:**

**Given** the onboarding web app (SvelteKit, `adapter-cloudflare`, Workers static assets) at its "Get started" route
**When** a visitor opens it
**Then** it shows text fields for name, phone (with country code, default +91), email, café name and city, one primary "Send code" button, and a footer `theme-toggle` (System · Light · Dark, System default, remembered per device), in light and dark themes

**Given** a submission with a missing or invalid field (phone not E.164-valid, email malformed)
**When** "Send code" is pressed
**Then** validation runs on submit and blur, focus moves to the first invalid field, and the error is announced; nothing is sent

**Given** a valid form
**When** "Send code" is pressed
**Then** the cloud creates an `otp_challenges` row (code hashed, 6 digits, 5-minute expiry) and sends the code by SMS through the messaging adapter (Gupshup at launch; a recorded fake in dev and CI), and the page shows a 6-cell code field with "Resend code" available after 30 s
**And** no `owners` row exists yet

**Given** the correct code within 5 minutes
**When** it is entered
**Then** the `owners` row is created (name, phone, email, café name, city, `mfa_state = pending`), the challenge is consumed, a browser session cookie (HttpOnly, Secure, SameSite=Lax, rotating refresh) is issued, and the owner lands on password + TOTP setup (Story 2.4)

**Given** a wrong code
**When** it is entered
**Then** a plain inline error shows and the cells clear; after 5 wrong attempts the challenge is void and a new code must be requested

**Given** a phone number that already belongs to an owner
**When** sign-up is submitted
**Then** the code is still sent (no account enumeration), and a correct code leads to sign-in for that existing account instead of creating a duplicate

**Given** repeated sends
**When** a phone requests more than 5 codes in an hour, or an IP exceeds the Cloudflare per-IP limit on the sign-up endpoints
**Then** further sends are refused with `RATE_LIMITED` and the page says when to try again; per-phone counts live in `gz_control.rate_limits`

**Given** the sign-up flow
**When** an axe scan runs in both themes
**Then** it has landmarks, one `h1`, labelled fields, all targets ≥ 44 px and zero serious/critical violations, and logs contain no phone number or email in clear

### Story 2.4: Owner sets a password and enrols TOTP with backup codes

As a café owner,
I want to set a password and link an authenticator app, and get backup codes,
So that my account can't be taken over with just my phone number.

**Covers:** FR2, AD-10, NFR11, NFR15 (WCAG 3.3.8), UX-DR18, UX-DR112, UX-DR113

**Acceptance Criteria:**

**Given** a newly verified owner with `mfa_state = pending`
**When** they reach account setup
**Then** they must set a password (at least 10 characters, rejected if it appears in a bundled common-password list, show/hide and paste allowed) before any wizard step opens

**Given** a password is set
**When** the TOTP step opens
**Then** the cloud generates a TOTP secret (RFC 6238, SHA-1, 6 digits, 30 s), stores it encrypted in `owner_mfa`, and shows it as a QR for the authenticator app plus the secret as copyable text for owners who cannot scan

**Given** the owner enters a valid current TOTP code (±1 step tolerance)
**When** they confirm
**Then** `mfa_state` becomes `enrolled`, 10 single-use backup codes are generated, shown once with Copy and Download (.txt), and stored only as Argon2id hashes in `owner_backup_codes`
**And** the owner must tick "I've saved my backup codes" before continuing to the wizard

**Given** an invalid TOTP code at enrolment
**When** it is submitted
**Then** a plain inline error shows and enrolment stays pending; leaving and returning resumes at the TOTP step with a new secret

**Given** an enrolled owner in Account → Security
**When** they choose "Regenerate backup codes" and confirm with a current TOTP code
**Then** all previous backup codes stop working and 10 new ones are shown once

**Given** an enrolled owner
**When** they turn on "SMS code as a fallback"
**Then** the setting is stored and SMS becomes an offered alternative at sign-in (Story 2.5); it is off by default

### Story 2.5: Owner signs in on a new device with TOTP, backup code or SMS fallback

As a café owner,
I want signing in on a new phone or laptop to ask for my authenticator code, with backups if I don't have it,
So that only I can open my account, and I'm never locked out by a lost phone.

**Covers:** FR2, AD-10, AD-23, AD-40, NFR11, NFR23, UX-DR18, UX-DR112

**Acceptance Criteria:**

**Given** an enrolled owner on a browser without a trusted-device cookie
**When** they sign in with phone (or email) and password
**Then** a TOTP challenge is required before any session is issued
**And** a correct code issues the session cookie and offers "Trust this device for 30 days", recorded in `owner_devices` with a device label and last-seen time

**Given** a browser with a valid trusted-device cookie
**When** the owner signs in with the correct password
**Then** no TOTP prompt is shown

**Given** the TOTP challenge
**When** the owner chooses "Use a backup code" and enters an unused one
**Then** sign-in succeeds, that code is consumed, and the remaining count is shown with a prompt to regenerate when 3 or fewer remain

**Given** SMS fallback is enabled for the owner
**When** they choose "Text me a code" at the challenge
**Then** an SMS OTP (6 digits, 5 minutes) is sent through the messaging adapter and accepted in place of TOTP; if fallback is off, the option is not shown

**Given** wrong password or wrong second factor
**When** submitted
**Then** the error never says which part failed, and after 5 consecutive failures the account's sign-in pauses for 15 minutes with the time shown; Cloudflare per-IP limits apply to the sign-in endpoints

**Given** a signed-in owner
**When** the access cookie expires
**Then** the refresh token rotates silently; reuse of an already-rotated refresh token revokes the whole session family and forces sign-in

**Given** Account → Security
**When** the owner views trusted devices
**Then** each device shows label and last seen, and "Sign out everywhere" revokes all sessions and trusted-device cookies

### Story 2.6: Resumable setup wizard and step 1 — café and first branch

As a café owner,
I want a 10-step setup that remembers where I stopped, starting with my café's details,
So that I can set up my branch over several sittings without redoing anything.

**Covers:** FR4, FR5, FR177 (create first branch, set timezone), AD-3 (`branch_code` assigned by control plane), AD-7, AD-9, UX-DR84, UX-DR91, UX-DR113, UX-DR116 (phone dots)

**Acceptance Criteria:**

**Given** an owner with `mfa_state = enrolled` and no tenant
**When** they open the wizard
**Then** `wizard-progress` shows "Step 1 of 10" and ten ≥ 44 px rows — Café details, Subscription tier, Install Hub, Devices, Rate plans, Products, Staff, Printer & payments, Test session, Go live — with "Can skip" tags on Products, Staff and Printer & payments; the current row has `aria-current="step"`; below 840 px it shows 10 dots plus "Step 1 of 10 · Café details"

**Given** step 1
**When** it opens
**Then** café name and city are prefilled from sign-up, and the owner sets country, currency (ISO 4217, defaulted from country), timezone (IANA, defaulted from country), and GST registered yes/no with GSTIN required and format-checked only when yes

**Given** valid step 1 values
**When** the owner continues
**Then** the cloud creates the `tenants` row and the first `branches` row (name, tz, currency, GST flag, GSTIN) in one transaction, with `tenant_id` taken from the session only, and assigns a `branch_code` (3–6 uppercase letters/digits derived from the branch name, unique per tenant) shown as editable until the Hub pairs
**And** step progress is stored in `onboarding_progress` (branch, step, state, completed_at)

**Given** an owner who leaves mid-wizard
**When** they return on any device
**Then** the wizard opens on the first incomplete step; done steps show an ok check and link back for edits; upcoming steps beyond the first incomplete one are not openable (direct URL redirects to the current step)

**Given** a done step re-opened for edit
**When** the owner saves a change
**Then** the change is stored, completion is kept, and the owner returns to the current step

**Given** a step completes by itself (later stories)
**When** the wizard advances
**Then** focus moves to the new step's heading and the change is announced once in a polite live region

**Given** a request that names a tenant or branch the session doesn't own
**When** it reaches the API
**Then** it returns `FORBIDDEN` and nothing is read or written

### Story 2.7: Step 2 — Free tier issues a signed licence

As a café owner,
I want to choose the Free tier without a card and have my branch's entitlement issued at once,
So that I can start installing straight away and the Hub knows exactly what my branch may do.

**Covers:** FR6 (Free path), AD-10 (licence JWT, claims, signing key), AD-20 (entitlement source, no gate commands), AD-29, NFR11, NFR20, UX-DR25, UX-DR93; Additional Requirements "Build spikes" (ed25519-dalek + jsonwebtoken EdDSA)

**Acceptance Criteria:**

**Given** step 2
**When** it opens
**Then** it shows a Monthly · Yearly `tab-control` and three equal cards — Free ₹0 (1 branch, ads on the Android Hub), Basic ₹200/branch/month + 18% GST (yearly ₹2,000), Premium ₹1,999 + 18% GST (yearly ₹19,990) — with values identical to the shared pricing table in the UI package

**Given** the owner picks Free
**When** they continue
**Then** no payment is asked, an `entitlements` row (branch, tier `free`, source `free`, add-ons none, issued_at) is written, and step 2 is marked done

**Given** the tenant already has a Free branch
**When** the owner picks Free for another branch
**Then** Free is disabled with the inline reason "One Free branch per account"

**Given** the owner picks Basic or Premium
**When** billing is not yet enabled (feature flag off until Epic 10)
**Then** Continue is disabled with an inline reason and no entitlement is written; nothing else in the wizard changes

**Given** an entitlement
**When** the licence module signs a licence for the branch
**Then** it is an EdDSA JWT with claims `{tenant_id, branch_id, tier, flags, features, upload_interval_s, summary_interval_s, rollup_schema, retention_days, limits, iat, exp = iat + 7 d}`, signed with the key from SOPS-managed secrets
**And** for Free: no paid flags, `upload_interval_s = summary_interval_s = 0` (no uploads), `limits = {devices: 20, users: 3, branches: 1}`; tier defaults live in one table in `arena-proto` shared by cloud and Hub

**Given** an Arena 360 operator running the audited `arena-cloud ops grant-comp --branch <id> --tier basic|premium --until <date>` command
**When** it completes
**Then** the branch's entitlement source becomes `comp` with that tier, the licence carries paid limits (Basic devices 60, Premium 200), the action is logged with operator and reason, and the wizard shows step 2 done for that branch

**Given** the signing and two-verify-key setup
**When** the licence unit tests run
**Then** a token signed with the current key verifies against the Hub's two-key verify set, a token signed by an unknown key fails, and a token with an edited claim fails

### Story 2.8: Step 3 — single-use 8-character Hub pairing code

As a café owner,
I want a short code shown large that I type into the Hub installer, with a copy button and a visible expiry,
So that pairing the Hub is one typed code and nothing else.

**Covers:** FR13 (code display, expiry, regenerate), AD-10 (single-use short-expiry code), UX-DR85, UX-DR84, UX-DR113, UX-DR116 (no QR)

**Acceptance Criteria:**

**Given** steps 1 and 2 are done
**When** step 3 opens
**Then** the cloud creates a `pairing_codes` row (code hash, branch_id, tier at issue, `expires_at = now + 15 min`, `used_at` null) and the page shows "1 · Download" (Hub for Windows 10 or 11, plus an "Installing on an Android tablet or phone instead?" link), "2 · Install", "3 · Type the code", and the `pairing-code` card

**Given** the `pairing-code` card
**When** it renders
**Then** the 8-character code is shown in `web-display` tabular figures grouped 4 + 4, from an alphabet without ambiguous characters (no 0/O, 1/I/L, U), with a secondary "Copy code" button, "Single use" and an "Expires in mm:ss" countdown; there is no QR anywhere on the page

**Given** the countdown
**When** it runs
**Then** it is announced to screen readers only at 10, 5 and 1 minutes remaining and at expiry, never every tick

**Given** "Copy code" is pressed
**When** the clipboard write succeeds
**Then** a polite status "Code copied" is announced; if it fails the code stays selectable for manual copy

**Given** the code expires
**When** the countdown reaches zero
**Then** the code is struck through with an "Expired" `status-badge`, the message "This code has expired. Generate a new code and type it into the Hub installer." shows, and "Generate a new code" becomes the one primary action

**Given** "Generate a new code" is pressed (or the page is reloaded while a code is live)
**When** a new code is issued
**Then** every earlier unused code for that branch is invalidated, so only one code is ever valid per branch

**Given** the codes stored in `gz_control`
**When** the table is inspected
**Then** only a hash of each code is stored; the code encodes nothing itself and maps to tenant, branch and tier only by lookup

### Story 2.9: Hub installer pairs with the code and retries in place

As a café owner at the counter PC,
I want the Hub installer to ask only for the code, and to let me retype it if it's wrong,
So that installing the Hub never needs a restart or anything else typed.

**Covers:** FR13 (installer retry in place), FR14 (code exchange, token, licence, Windows Hub on Free refused), AD-10, AD-22, AD-23, NFR11, NFR23, UX-DR18, UX-DR103 (installer bad-code line)

**Acceptance Criteria:**

**Given** a freshly installed Windows Hub with no pairing
**When** the Hub UI opens
**Then** it shows the always-dark "Pair this Hub" screen with one 8-character code field (case-insensitive, space ignored), a primary "Pair" button and "Quit setup"; no other Hub screen is reachable until paired

**Given** a valid, unused, unexpired code
**When** the Hub posts `POST /v1/pair {code, platform: windows|android, hub_instance_id, version}`
**Then** in one transaction the cloud marks the code used, mints a 256-bit Hub device token, stores its Argon2id hash in a new `hubs` row (branch_id, platform, instance id, paired_at), and returns the token once with tenant id, branch id, tier, `branch_code` and the first licence JWT
**And** the Hub stores the token with DPAPI (Android Keystore on Android), writes the licence to `licence_cache`, and records branch and tenant ids locally

**Given** a wrong, used or expired code
**When** "Pair" is pressed
**Then** the installer stays on the same screen, clears the field, keeps focus in it, and shows "That code didn't work. Get a new code from your browser."; the response code (`PAIR_CODE_INVALID` / `PAIR_CODE_EXPIRED`) is logged but the text does not say which

**Given** a Free branch
**When** a `platform: windows` Hub tries to pair
**Then** pairing is refused with `FEATURE_NOT_ENTITLED`, the code is not consumed, and the installer explains that Free runs on the Android Hub and links to the Android download; `platform: android` with the same code is accepted (verified by an API test)

**Given** repeated attempts
**When** one code receives 5 failed attempts, or one IP makes more than 20 pairing attempts in an hour
**Then** the code is invalidated or the IP is refused with `RATE_LIMITED`, enforced both by a Cloudflare per-IP rule on `/v1/pair` (committed as config in the repo) and in `arena-cloud` against `gz_control.rate_limits`

**Given** the cloud is unreachable from the counter PC
**When** "Pair" is pressed
**Then** the installer shows a plain "Can't reach Arena 360 — check the internet and try again" message and keeps the typed code

**Given** a Hub that is already paired
**When** it restarts
**Then** it skips the pairing screen and the code endpoint is never called again

### Story 2.10: Hub checks in by heartbeat and the wizard advances by itself

As a café owner,
I want the setup page to move on by itself the moment my Hub connects,
So that I know the Hub is paired without refreshing or guessing.

**Covers:** FR13 (advance on first heartbeat), AD-10 (licence refresh), AD-38 (provisional epoch confirmed on first heartbeat), AD-44 (cloud time), NFR11, UX-DR84, UX-DR113

**Acceptance Criteria:**

**Given** a paired Hub
**When** its heartbeat task runs (every 10 s until the branch is live, then every 60 s)
**Then** it posts `POST /v1/heartbeat` with bearer device token, epoch, version, schema version and `last_seq_uploaded` (0 on Free), and never blocks any intent while doing so

**Given** a heartbeat with a valid token
**When** the cloud handles it
**Then** it updates `hubs.last_seen_at`, re-signs the licence from the current entitlement, and returns `{licence, fence, cloud_time}`; the Hub replaces `licence_cache` with the new licence

**Given** the branch's first heartbeat and no `branch_fence` row
**When** the cloud handles it
**Then** it creates `branch_fence` with the Hub's provisional epoch by compare-and-set, and the Hub clears `lease.provisional`

**Given** the wizard is open on step 3
**When** the first heartbeat for that branch is recorded
**Then** step 3 is marked done and the page advances to step 4 within 5 s (server-sent events with a 5 s polling fallback), moving focus to the step 4 heading

**Given** a heartbeat with an unknown or revoked token
**When** the cloud handles it
**Then** it returns `TOKEN_INVALID` / `TOKEN_REVOKED`, and the Hub shows a staff-visible "This Hub is no longer paired" state without stopping local trading

**Given** the WAN is down
**When** heartbeats fail
**Then** the Hub retries with jittered backoff up to its normal interval, Epic 1's `chip-offline` behaviour applies, and nothing else changes

### Story 2.11: Hub verifies its licence against trusted time and enforces its limits

As a café owner,
I want the Hub to obey only a genuine, current licence and keep working offline,
So that tampering can't unlock paid features and a dropped internet never stops my café.

**Covers:** AD-10 (two verify keys, tampering ⇒ Free limits), AD-29 (entitlement checks in `apply()`), AD-44, NFR11, NFR22

**Acceptance Criteria:**

**Given** a licence in `licence_cache`
**When** the Hub loads it at start and after each heartbeat
**Then** it verifies the EdDSA signature against the two verify keys compiled into the Hub release and checks `branch_id`/`tenant_id` match the paired branch
**And** the verified claims are exposed to `apply()` through one entitlement API (`limit(name)`, `has_flag(name)`) used by every gate

**Given** a licence that fails verification or names another branch
**When** the Hub loads it
**Then** the Hub applies Free limits, keeps running (a Windows Hub does not lock), shows staff "Licence couldn't be verified" in the header alert list, and writes an audit row

**Given** each heartbeat response
**When** it carries `cloud_time`
**Then** the Hub stores `trusted_clock {cloud_ms, mono_ms, wall_delta_ms}`, persists it across restarts, and evaluates licence `exp` against `trusted_now` = last cloud time + monotonic elapsed

**Given** the wall clock differs from `trusted_now` by more than 5 minutes
**When** the check runs
**Then** staff see a "PC clock is wrong" alert and an audit row is written; business timestamps keep using the wall clock

**Given** a licence whose `exp` has passed on `trusted_now`
**When** the Hub evaluates it
**Then** a Free licence keeps working indefinitely offline; a paid (comp) licence falls back to Free limits for new pairings only and nothing running is ended

**Given** the user changes the wall clock back by a week
**When** licence expiry is evaluated
**Then** the expiry decision is unchanged because it uses `trusted_now`

### Story 2.12: branch.bootstrap applies the onboarding profile at first pairing, with branch code lock

As a café owner,
I want the Hub to set itself up with the details I entered online the moment it pairs,
So that I never type my café's name, timezone, currency or tax details twice.

**Covers:** FR14 (`branch.bootstrap`), FR5, AD-3, AD-7, AD-2, AD-14, NFR25, UX-DR102 (owner's setup code display), UX-DR103

**Acceptance Criteria:**

**Given** a successful `/v1/pair` for a branch's first Hub
**When** the response is built
**Then** it carries a `branch.bootstrap` command `{idempotency_key, profile: {name, branch_code, currency, tz, day_start_min, gst_registered, gstin, tax_mode}, owner_staff: {name, login = owner phone, role: Owner, setup_code_hash, setup_expires_at}}`; the setup code itself never travels

**Given** the Hub receives the bootstrap command
**When** it applies it through `apply()` with `actor{kind: owner, id}`
**Then** the `branch` row and settings defaults are created and the owner's staff row (role Owner, no password or PIN) is created, all as change rows under one intent; `tax_mode` is `gst_in` only for a GST-registered Indian branch, else `simple`

**Given** a branch already bootstrapped
**When** a second bootstrap arrives (retry or re-pair)
**Then** the idempotency key applies it once; a different key is rejected `ALREADY_BOOTSTRAPPED` and nothing changes

**Given** the Hub is bootstrapped
**When** the wizard advances past step 3
**Then** the owner sees once "Your Hub sign-in code: XXXX XXXX. Enter it at the Hub to choose your PIN and password." with Copy; the plaintext is held encrypted in `gz_control` only until shown or 24 h, then deleted

**Given** a release build of the Hub
**When** it starts unpaired
**Then** it has no dev seed path; the only way to a bootstrapped branch is pairing (the dev fixture remains in the `dev-fixtures` feature for tests)

**Given** a bootstrapped branch
**When** a Manager or Owner changes `branch_code` in Settings → Branch
**Then** `settings.set` accepts it (uppercase letters/digits, 3–6) while `branch.code_locked_at` is null, and rejects it with `BRANCH_CODE_LOCKED` once set
**And** a system function `lock_branch_code()` (to be called by the invoice engine) sets `code_locked_at` idempotently, covered by a unit test

### Story 2.13: Owner and staff set their own PIN and password at the Hub with a one-time setup code

As a staff member (or the owner) signing in to the Hub for the first time,
I want to enter my setup code and choose my own PIN and password,
So that nobody else ever knows my credentials.

**Covers:** FR104 (Hub half), AD-10, AD-30, NFR11, NFR25, UX-DR66, UX-DR102, UX-DR103

**Acceptance Criteria:**

**Given** the Hub sign-in screen
**When** a person chooses "First time here? Use a setup code"
**Then** the screen shows "Enter your setup code, then choose your PIN and password." with login (phone) and an 8-character code field

**Given** a staff row whose `setup_code_hash` matches and `setup_expires_at` is in the future
**When** the code is submitted
**Then** the person chooses a password (same policy as Hub staff passwords) and a 4–6 digit PIN entered twice, and `staff.set_credentials` stores Argon2id hashes, clears `setup_code_hash`, bumps `pin_verifier_version`, and signs them in

**Given** a wrong, used or expired code
**When** it is submitted
**Then** `SETUP_CODE_INVALID` is shown as one plain line that doesn't say which, and after 5 failures for that login entry pauses for 60 s with a countdown

**Given** a staff member whose credentials are already set
**When** anyone submits a setup code for them
**Then** it is rejected; codes work once

**Given** `staff.set_credentials`
**When** its change row is written
**Then** the row carries `actor{kind: staff, id: self}` and the stored hashes, never the PIN, password or code in clear, and the PIN never appears in any log

**Given** the owner completed this with their own code from Story 2.12
**When** they sign in normally later
**Then** they use their phone and password like any staff member, with the Owner role

**Given** the Hub release build
**When** it runs
**Then** no staff are seeded; the dev staff fixture exists only under `dev-fixtures`

### Story 2.14: Owner changes reach the Hub through the command mailbox within one poll

As a café owner,
I want changes I make online during setup to reach my Hub reliably and exactly once,
So that groups, device assignments, staff invites and setup actions land even if the Hub was briefly offline.

**Covers:** FR104 ("within one poll"), FR8 (delivery path), AD-5 (setup kinds only), AD-14, AD-39, AD-40, NFR11, NFR25

**Acceptance Criteria:**

**Given** the `command` role of `arena-cloud`, deployed as its own Kubernetes workload
**When** the onboarding API submits a setup command
**Then** a `commands` row is written (`branch_id`, `idempotency_key`, `kind`, `schema_version`, `payload`, `issued_by`, `status = pending`) with a relay-assigned monotonic `id`, and a duplicate `(branch_id, idempotency_key)` returns the existing row

**Given** the command catalogue in `arena-proto`
**When** a command kind is submitted
**Then** only setup kinds (`group.upsert`, `device.assign`, `device.register`, `staff.upsert`, `settings.set` for name/tz, `setup.test_session`, `branch.go_live`) are accepted, on every tier; any other kind returns `UNSUPPORTED`

**Given** a paired Hub
**When** its poller runs every 60 s, or immediately when a heartbeat response carries `pending_commands > 0`
**Then** it fetches `GET /v1/commands?after=<cursor>` with its epoch in `id` order, rows move to `fetched`, and each is applied once through `apply()` with `actor{kind: owner}`, deduped in `applied_commands`

**Given** a command applied or rejected at the Hub
**When** the Hub posts `POST /v1/commands/{id}/result` with epoch and `applied_seq` or an `AppError`
**Then** the row becomes `applied` or `rejected` with the error, and repeating the same result is a no-op

**Given** the Hub is offline when a command is queued
**When** it reconnects
**Then** it fetches and applies the backlog in `id` order, and no command is applied twice even if a result post was lost

**Given** an applied owner command that overwrote a local edit
**When** it applies
**Then** the Hub toasts "<Entity> updated by owner" and the audit keeps both changes

### Story 2.15: Step 7 — invite staff by phone with a role and a one-time setup code

As a café owner,
I want to add my manager by phone, pick their role and get a code to hand them,
So that they can sign in at the Hub with their own PIN and password without me ever knowing them.

**Covers:** FR9, FR104 (owner half), AD-10, AD-5, UX-DR84, UX-DR102, UX-DR103

**Acceptance Criteria:**

**Given** step 7 on a paired, bootstrapped branch
**When** it opens
**Then** it shows a form for name, phone and role (Manager, Counter, Kitchen; Manager preselected for the first invite), an "Invite" primary button, the list of invited staff with their status, and "Skip for now"

**Given** valid details
**When** "Invite" is pressed
**Then** the cloud generates an 8-character setup code, sends a `staff.upsert` command carrying only `setup_code_hash` and `setup_expires_at = now + 72 h`, and shows once "Setup code for <name>: XXXX XXXX. Give it to her; it works once." with Copy
**And** the staff member's Hub login is their phone number

**Given** the Hub applies `staff.upsert`
**When** the login already exists at the branch
**Then** it is rejected `LOGIN_TAKEN` and the invite row shows "Couldn't apply at branch" with the reason

**Given** an invite that has not been used
**When** the owner chooses "New code"
**Then** a fresh code and hash are issued by a new command, the old code stops working on apply, and the new code is shown once

**Given** at least one Manager invite applied at the Hub
**When** the result arrives
**Then** step 7 is marked done; "Skip for now" also leaves it marked skipped and finishable later from the Hub

**Given** the Free 3-user limit in the licence
**When** an invite would exceed it
**Then** the Hub rejects `USER_LIMIT` and the wizard shows `tier-lock` "Available on Basic" beside the invite

### Story 2.16: Kiosk finds the Hub on the LAN or by typed IP

As a café owner setting up gaming PCs,
I want each kiosk to find the Hub by itself, or let me type the Hub's address,
So that pairing works on any café network.

**Covers:** FR15 (discovery, manual IP fallback), AD-8 (mDNS service name in `arena-proto`), AD-42, NFR17, UX-DR18, UX-DR103

**Acceptance Criteria:**

**Given** a paired Hub
**When** it is running
**Then** it advertises the mDNS service named in `arena-proto` with TXT records branch name, Hub name, protocol version and Hub certificate fingerprint, and shows its LAN IP in Settings → Devices

**Given** an unpaired kiosk on first run
**When** the kiosk app starts
**Then** the locked kiosk screen shows "Looking for the Hub…" and lists discovered Hubs by branch and Hub name within 10 s

**Given** no Hub is discovered in 10 s
**When** the search ends
**Then** the screen shows a "Hub IP address" field with the hint that the IP is shown on the Hub under Settings → Devices, plus "Search again"

**Given** a typed IP
**When** the kiosk checks it
**Then** it connects to `:7401` and confirms an Arena 360 Hub answers with branch name and fingerprint; otherwise it shows "No Hub at that address" and keeps the field

**Given** the kiosk is unpaired
**When** the kiosk shows the discovery screen
**Then** the lockdown from Epic 1 stays in force and Ctrl+Alt+Shift+A admin exit still works

### Story 2.17: Kiosk pairs with a 6-digit code from the Hub (or a manager PIN) and shows as Unassigned

As a café owner setting up gaming PCs,
I want to type the 6-digit code shown on the Hub into each PC,
So that every PC joins my branch and appears on the floor within a minute.

**Covers:** FR15, AD-10, AD-22, AD-42, NFR11, UX-DR2 (unassigned), UX-DR61 (unassigned dashed), UX-DR66

**Acceptance Criteria:**

**Given** a Manager or Owner on the Hub
**When** they open Settings → Devices → "Pair a kiosk"
**Then** a 6-digit single-use code is shown with a 10-minute countdown; after a code is used, a fresh one appears at once so many PCs can be paired in a row

**Given** a kiosk that has selected a Hub
**When** the 6-digit code is entered
**Then** over TLS to `:7401` the kiosk sends `pair{code, pc_name, version}`; the Hub mints a 256-bit device token (Argon2id hash in `devices`), creates a `devices_meta` row (`kind = pc`, `label = pc_name`, `group_id` null) via `device.register`, and returns the token and Hub certificate fingerprint once
**And** the kiosk service stores the token with DPAPI and pins that fingerprint for every later connection

**Given** a just-paired kiosk
**When** it sends `hello{class: paired}`
**Then** it appears on the Hub floor's Unassigned section as a dashed entry with its PC name within 60 s of entering the code

**Given** the kiosk alternative "Use a manager PIN"
**When** a Manager's PIN is entered on the kiosk instead of the code
**Then** the Hub verifies it with Argon2id and pairs the kiosk the same way, recording `elevated_by` the manager; a wrong PIN clears the cells and 5 failures pause the option for 60 s

**Given** a wrong or expired code
**When** it is entered
**Then** the kiosk stays on the same screen, clears the field and shows a plain retry line

**Given** Epic 1's dev kiosk token and stored Hub IP
**When** a release-build kiosk is installed
**Then** neither exists; a kiosk can reach a Hub only by pairing

**Given** a paired kiosk listed on the floor
**When** a Manager uses Remote unlock from its entry (Story 1.18)
**Then** it works against the real paired kiosk

### Story 2.18: Device cap and pairing rate limits at the Hub

As a café owner,
I want the Hub to stop at my tier's device limit and to resist code guessing on the café Wi-Fi,
So that limits are clear and a stranger can't pair a device.

**Covers:** FR20, AD-23, AD-29, NFR23, UX-DR24

**Acceptance Criteria:**

**Given** the licence `limits.devices` (Free 20, Basic 60, Premium 200, +20 per add-on)
**When** a kiosk pairing or `device.register` would make the non-revoked device count exceed it
**Then** `apply()` rejects it with `DEVICE_LIMIT` in the permission step and nothing is written; the count includes kiosks, Pis and manually registered devices and excludes standby Hubs and kitchen displays

**Given** a refused pairing at the cap
**When** the Hub shows it
**Then** Settings → Devices shows `tier-lock` "Available on Basic" (or Premium) with an underlined "Upgrade" link, and the kiosk shows "This branch has reached its device limit — ask the owner."

**Given** one kiosk pairing code
**When** it receives 5 failed attempts
**Then** it is invalidated and the Hub panel shows a fresh code

**Given** one LAN IP
**When** it makes more than 20 pairing attempts (code or PIN) in an hour
**Then** further attempts from that IP get `RATE_LIMITED` until the hour rolls, and the Hub log records it

**Given** a device is revoked
**When** the count is recalculated
**Then** the slot is freed immediately for a new pairing

### Story 2.19: Device groups with six swatches, held at the Hub

As a café owner,
I want device groups with a name and colour that the Hub enforces, and that managers can edit at the Hub later,
So that every device is priced and shown by its group, whether groups were set up online or at the counter.

**Covers:** FR17, AD-2, AD-39, UX-DR7, UX-DR63, UX-DR88 (Settings → Layout)

**Acceptance Criteria:**

**Given** a `group.upsert` intent (from the Hub UI or a setup command)
**When** it applies
**Then** it stores name (unique per branch), kind (pc, console, vr, other), colour from exactly the six swatches teal, orange, lime, sand, coral, silver, and sort order, in a new `device_groups` table; any other colour is rejected with a field error

**Given** a `device.assign` intent for a device
**When** it applies
**Then** it sets the device's `group_id`; every device belongs to at most one group, and assigning removes it from Unassigned

**Given** a PC (kiosk) device and a console group, or a console device and a PC group
**When** `device.assign` is applied
**Then** it is rejected `GROUP_KIND_MISMATCH`; console groups are never shared with PC groups

**Given** a group that still contains devices
**When** someone deletes it
**Then** it is refused with an inline reason; an empty group is soft-deleted

**Given** a Manager or Owner on Settings → Layout at the Hub
**When** they create or edit a group, or drag a device (or use the keyboard "Move to group" control) into a group
**Then** the same intents apply, so groups can be changed at the Hub after the wizard

**Given** groups exist
**When** the floor placeholder renders
**Then** each assigned device shows a `group-chip` (swatch dot + group name in muted ink), and Unassigned devices show none

**Given** a stale edit (old `base_rev`) from a second Hub window
**When** it is saved
**Then** it is rejected `STALE` with the current values shown; an owner setup command with an old `base_rev` still applies and the Hub toasts "Group updated by owner"

**Given** a Counter
**When** they try to edit groups at the Hub
**Then** the action opens `inline-pin` for manager elevation

### Story 2.20: Register consoles, VR rigs and simulators manually

As a café owner,
I want to add my PS5s, VR rigs and simulators by hand with a group and TV number,
So that they appear on the floor and can be timed like PCs.

**Covers:** FR16, FR20, AD-29

**Acceptance Criteria:**

**Given** Settings → Devices → "Add device"
**When** a Manager picks Console, VR rig or Simulator and enters a label, a group of the matching kind and (for consoles) a TV number
**Then** `device.register` creates a `devices_meta` row with that kind and no device token, and it appears on the floor placeholder like a PC with its label and, for consoles, "TV n"

**Given** a `device.register` setup command from the owner (used by wizard step 4)
**When** the Hub applies it
**Then** the result is the same as registering at the Hub, with `actor{kind: owner}`

**Given** a console TV number already used at the branch
**When** it is saved
**Then** it is rejected with a field error

**Given** the device cap is reached
**When** a manual device is added
**Then** it is refused `DEVICE_LIMIT` with the same `tier-lock` as pairing

**Given** a manually registered device
**When** a Manager edits or removes it
**Then** edits go through `device.register` with `base_rev`, and removal soft-deletes it and frees its cap slot

### Story 2.21: Step 4 — owner groups devices in the web wizard as they arrive

As a café owner,
I want to see my PCs arrive in the setup page, create groups and drag each device into one there,
So that I can organise the whole floor from my browser without walking to the counter.

**Covers:** FR8 (step 4), FR17, FR16 (add consoles from the wizard), AD-5, AD-29, UX-DR7, UX-DR84, UX-DR105, UX-DR113

**Acceptance Criteria:**

**Given** a paired Hub before go-live
**When** it heartbeats
**Then** the heartbeat carries a `setup` block with devices (id, label, kind, group id, status; no player data) and groups (id, name, kind, colour, sort), stored as the latest row in a new `branch_setup_status` table

**Given** step 4 is open
**When** a kiosk pairs at the branch
**Then** it appears in the step's Unassigned list within 15 s with its PC name and status, without a page reload

**Given** step 4
**When** the owner creates a group (name, kind, one of the six swatches)
**Then** a `group.upsert` setup command is sent through the mailbox, the group shows at once marked "Waiting for branch", and the mark clears when the Hub reports it applied

**Given** an Unassigned device and a group
**When** the owner drags the device onto the group (or, by keyboard, focuses the device, chooses "Move to group" and picks one with arrow keys + Enter)
**Then** a `device.assign` setup command is sent, the device moves optimistically with "Waiting for branch", and on rejection (e.g. `GROUP_KIND_MISMATCH`) it returns to its previous place with the plain reason inline

**Given** step 4
**When** the owner chooses "Add console" (or VR rig / simulator) with label, matching group and TV number
**Then** a `device.register` setup command is sent; a `DEVICE_LIMIT` rejection shows `tier-lock` "Available on Basic"

**Given** at least one device in a group, as reported by the Hub
**When** the setup block arrives
**Then** step 4 completes by itself and focus moves to the step 5 heading

**Given** the Hub is offline
**When** the owner works in step 4
**Then** the last reported devices show with their age ("as of 3 min ago") and a "Hub offline" status badge; new group and assignment commands queue as "Waiting for branch" and apply in order when the Hub returns

**Given** groups were also edited at the Hub
**When** the next heartbeat arrives
**Then** the wizard shows the Hub's current groups and assignments

### Story 2.22: Floor zones and grid layout per branch

As a café owner,
I want to arrange device tiles into zones like "PC hall" and "Console room",
So that the floor view matches my café and staff find each seat at a glance.

**Covers:** FR19, UX-DR87 (zones), UX-DR88 (layout editor), UX-DR105

**Acceptance Criteria:**

**Given** Settings → Layout
**When** a Manager adds, renames, reorders or deletes a zone
**Then** a `floor_zones` table stores name and sort, and a zone with devices cannot be deleted until they move

**Given** the layout grid
**When** a device tile is dragged to a cell in a zone, or moved by keyboard (arrow keys move the tile, Enter drops)
**Then** `device.place` stores `zone_id`, `floor_x`, `floor_y` on `devices_meta`; two devices cannot occupy one cell

**Given** a saved layout
**When** the Hub restarts or another Hub window opens
**Then** the floor placeholder shows zones in sort order with devices in their cells, and the Unassigned section stays at the end

**Given** a phone-width Hub (< 840 px)
**When** the layout editor opens
**Then** it falls back to a list with "Zone" and "Position" fields per device, with 44 px targets

### Story 2.23: Device status and maintenance

As counter staff,
I want every device to show one clear status and to take a broken PC out of use without deleting it,
So that nobody starts a session on a device that can't be used.

**Covers:** FR18, UX-DR2, UX-DR23, UX-DR110

**Acceptance Criteria:**

**Given** `devices_meta.state`
**When** the schema is created
**Then** it allows exactly online, offline, in session, reserved and maintenance (in session and reserved are set by later features); every status shows as text plus its state token, never colour alone

**Given** a paired kiosk
**When** its socket connects or stays disconnected for 15 s
**Then** its status becomes online or offline with `last_seen_at`, pushed to every open Hub view within 2 s

**Given** a manually registered device
**When** it is not in maintenance
**Then** it shows online

**Given** a Manager sets a device to maintenance with an optional note
**When** `device.maintenance` applies
**Then** the device shows "Maintenance", stays in its group and position, and is flagged unavailable for starts; "Return to service" restores its previous status

### Story 2.24: Hourly postpaid rate per device group

As a café owner,
I want to set an hourly rate for each device group,
So that every device is priced by its group from the first session.

**Covers:** FR57, FR8 (step 5 completion source), AD-7, AD-39, UX-DR8

**Acceptance Criteria:**

**Given** Settings → Layout → a group
**When** a Manager or Owner sets an hourly rate
**Then** `rate.upsert` writes a `rates` row (group, `price_per_hour_minor` in the branch currency's minor units, active) and the amount displays with tabular numerals in the branch format

**Given** a rate change
**When** it is saved
**Then** the previous rate row is deactivated (kept for history) and the new one becomes the group's only active rate, so a later session start reads the active rate at that moment

**Given** zero, negative or non-numeric input
**When** it is saved
**Then** a field error shows and nothing is written

**Given** each group
**When** the Hub computes setup status
**Then** groups without an active rate are listed as "No rate yet"

**Given** a Counter
**When** they edit a rate
**Then** `inline-pin` opens for manager elevation and the change is audited with both actors

### Story 2.25: Branch operating settings — opening hours, fulfilment, kiosk volume, backup location, ad consent

As a café owner,
I want to set my opening hours and the few branch-wide settings the go-live checklist needs,
So that pricing, passes and alerts follow my real hours and the branch is ready to open.

**Covers:** FR115, FR11 (checklist inputs), AD-7, UX-DR88

**Acceptance Criteria:**

**Given** Settings → Branch → Hours
**When** a Manager sets open and close times per weekday (closed allowed; close past midnight allowed)
**Then** `settings.set` stores them as local minutes from midnight with wrap in `branch.opening_hours`, and invalid entries (open = close) show a field error

**Given** Settings → Branch
**When** a Manager sets order fulfilment (deliver to seat / pick up at counter) and kiosk default volume (0–100)
**Then** they are stored and pushed by `settings.changed` to connected kiosks

**Given** Settings → System → Backup location
**When** a Manager picks a local folder or removable drive
**Then** the path is stored and checked as writable, with an inline error if not

**Given** a Free branch
**When** Settings → Branch → Ads opens
**Then** it shows the ads consent view with the ad policy link and a consent tick stored with time and staff id; on paid tiers this section is absent

**Given** these settings
**When** the Hub computes setup status
**Then** it reports opening hours set yes/no, backup location set yes/no and ad consent yes/no

### Story 2.26: Printer detection and test print at the Hub

As a café owner,
I want the Hub to find my receipt printer and print a test,
So that I know receipts will work before I open.

**Covers:** FR10, AD-28, AD-34, NFR18, UX-DR95

**Acceptance Criteria:**

**Given** Settings → Printer
**When** a Manager presses "Find printer"
**Then** the Hub lists ESC/POS printers found on USB and on the LAN at port 9100 (plus a manual IP field), through the printer adapter that has a recorded fake for tests

**Given** a selected printer and paper width 58 or 80 mm
**When** "Test print" is pressed
**Then** a text-only ESC/POS Font A test page (branch name double width/height, date, "Printer test OK", width-specific ruler) prints and the result is recorded as a setup status of ok or failed with the error

**Given** no printer is found or the test fails
**When** the Manager chooses "Skip printer for now"
**Then** the setup status is skipped, and nothing else on the Hub depends on a printer

**Given** UPI and card
**When** the printer & payments screen shows
**Then** it states that UPI and card are recorded by reference at settle and asks for no payment-provider keys

### Story 2.27: Wizard steps 5, 6 and 8 follow the Hub's setup progress

As a café owner,
I want the online wizard to tick off rates and the printer as I finish them at the Hub,
So that I always know what's left without switching screens to check.

**Covers:** FR8 (step 5 completion), FR4 (skippable steps), FR10 (step completion), UX-DR84, UX-DR113

**Acceptance Criteria:**

**Given** a paired Hub before go-live
**When** it heartbeats
**Then** its `setup` block also carries groups without a rate, manager exists yes/no, printer status, test session status, and opening hours/backup/ad consent flags

**Given** step 5 (Rate plans)
**When** every group has an active rate
**Then** the step completes by itself; until then it lists groups without a rate

**Given** step 6 (Products)
**When** it opens
**Then** it explains products can be added later at the Hub and offers "Skip for now", which marks it skipped and opens step 7

**Given** step 8 (Printer & payments)
**When** the Hub reports printer ok or skipped, or the owner chooses "Skip for now"
**Then** the step completes and the next step opens

**Given** the Hub's own Settings → Setup panel
**When** staff open it before go-live
**Then** it shows the same checklist (pair kiosks, groups, rates, printer, test session) with a link to each screen; groups set in the web wizard show as done

**Given** the Hub is offline
**When** the owner views steps 5–8
**Then** the last reported state shows with its age ("as of 3 min ago") and a "Hub offline" status badge, never a broken page

### Story 2.28: Step 9 — 2-minute test session locks a kiosk at zero

As a café owner,
I want to start a 2-minute test on one PC and watch it lock exactly at zero,
So that I know the floor is really under control before I open.

**Covers:** FR11 (test session), AD-12, AD-5, NFR3, UX-DR113

**Acceptance Criteria:**

**Given** step 9 and at least one online kiosk in a group
**When** the owner picks it and presses "Start test session" (or staff press it in the Hub's Setup panel)
**Then** a `setup.test_session` command (or Hub intent) applies `device.test_session.start`, and the kiosk unlocks to a test screen showing "Test session" and a 2:00 countdown in tabular figures within 15 s of the click while the branch is online

**Given** a test session running
**When** the Hub sends the unlock
**Then** it carries `lock_after_ms = 120000`, and the kiosk service counts down on the monotonic clock so the PC locks at zero even if the Hub drops mid-test

**Given** the countdown reaches zero
**When** the kiosk locks
**Then** it returns to the locked screen and sends `device.test_session.completed`; step 9 completes by itself and the wizard says "The PC locked on cue."

**Given** the kiosk does not confirm the lock within 10 s of zero
**When** the Hub times out
**Then** the step shows "That PC didn't confirm it locked — check it and try again" with Retry

**Given** the chosen kiosk is offline or in maintenance
**When** the test is requested
**Then** it is refused with a plain reason and the device list stays open

**Given** a test session
**When** reports and bills are produced later
**Then** it creates no session, bill or money row; only the two audit intents exist

### Story 2.29: Owner manages branches — create, rename, timezone, new Hub pairing code

As a café owner,
I want one place to see my branches, rename them, set their timezone and get a pairing code,
So that I can manage branches after setup without restarting the wizard.

**Covers:** FR177, AD-3, AD-5, AD-29, UX-DR24, UX-DR90 (Account → Branches)

**Acceptance Criteria:**

**Given** the owner app route Account → Branches
**When** it opens
**Then** each branch shows name, branch code, tier, setup state (in setup / live) and Hub status (not paired / last seen time)

**Given** a branch whose Hub is not yet paired
**When** the owner renames it or changes its timezone
**Then** the change is saved in `gz_control` and will reach the Hub in `branch.bootstrap`

**Given** a paired branch
**When** the owner renames it or changes its timezone
**Then** a `settings.set` command is sent and the row shows "Waiting for branch" until applied; the timezone takes effect from the next business day at the Hub

**Given** a branch with no paired Hub
**When** the owner chooses "Get pairing code"
**Then** the same `pairing-code` card as step 3 is shown

**Given** a Free tenant with one branch
**When** the owner chooses "Add branch"
**Then** it shows `tier-lock` "Available on Basic"; with a paid or comp entitlement it creates the branch and opens its wizard at step 1

**Given** standby pairing codes
**When** the owner looks for them on this page
**Then** the action is shown as `tier-lock` on Free and is delivered with the standby feature (not in this story)

### Story 2.30: Step 10 — go-live checklist marks the branch live

As a café owner,
I want a final checklist that confirms the branch is ready and then marks it live,
So that I open with confidence and my owner app switches from setup to running mode.

**Covers:** FR11 (go live), FR4, NFR27 (onboarding telemetry), UX-DR84, UX-DR113

**Acceptance Criteria:**

**Given** step 10
**When** it opens
**Then** it lists opening hours set, backup location set, standby PC (shown "Paid tiers only" on Free and not blocking), and ad consent (Free only), each with its state from the latest setup status and a link to the Hub screen that fixes it

**Given** all required items are done, and steps 1–5 and 9 are done (6–8 done or skipped)
**When** the owner presses "Go live"
**Then** `branches.live_at` is set, a `branch.go_live` command reaches the Hub, the Hub hides its Setup panel and drops heartbeats to the normal interval, and the wizard ends on Account → Branches showing the branch as Live

**Given** a required item is missing
**When** the owner views step 10
**Then** "Go live" is disabled with the inline reason naming the first missing item

**Given** a live branch
**When** the owner opens the wizard URL again
**Then** they are taken to Account → Branches; done steps remain viewable for reference

**Given** the onboarding timeline
**When** each step completes
**Then** the cloud records step completion timestamps from sign-up to go-live in `onboarding_progress` (no PII), available for the "first paid session within 60 min" metric

## Epic 3: Counter staff run the floor

Staff run postpaid and prepaid sessions on PCs and consoles, manage players, wallet, credit, plans and dynamic pricing, and settle bills with mixed payments and Undo.

> Draft notes: this epic builds on the Epic 1 shell, `apply()`, inline PIN and audit, and on the Epic 2 devices, groups, zones, rates, opening hours and kiosk unlock/lock mechanism (Story 2.28). **Settle path:** Epic 3 settles session, top-up and plan-sale bills **without a receipt and without an invoice number**. A bill is an `orders` row with session/plan/top-up `order_lines` and `payments`; totals are tax-inclusive and no tax line is shown. Settle emits a `bill.settled` event, which is the hook Epic 5 uses to add receipts (print/WhatsApp/none), tax breakup, invoice numbering, cash drawer and shift binding. Undo checks one shared "irreversible effect recorded?" guard. In Epic 3 nothing records an effect, so Undo is always allowed inside the window; Epic 5 registers `receipt.printed`, `drawer.kicked` and "invoice number assigned" with that guard. Decided invoice timing (for Epic 5): with no printer the invoice number is assigned when the 8 s Undo window closes; with a printer it is assigned at print, so printed bills are never undoable. Cash is accepted with no open shift until Epic 5 adds `SHIFT_NOT_OPEN`. **Kiosk side:** on session start the kiosk unlocks to a minimal session screen (time + device name, shared tokens), and it locks with a reason line (Pay at counter, ended by staff, time's up, locked by staff). Epic 6 replaces that screen with the player shell, HUD, grace lock, launcher logout and kiosk self-start. **Out of this epic:** the tile sheet's Order action and the order-pending tile state (Epics 5–6), the reserved state's data (bookings, Epic 13), credit reminders and the console 5-min message (Epic 8), Pi/QR (Epic 12), and the kiosk halves of FR31, FR35, FR48 and FR63 (player self-start, kiosk Extend, kiosk account page, kiosk attract/plan-picker chip; Epic 6). Every story adds its new intents to the Story 1.6 `proptest` generator.

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

### Story 3.4: Collapsible zones that never hide an alert

As counter staff in a large café,
I want to collapse zones I'm not watching while still seeing their counts and alerts,
So that I can focus on one part of the floor without missing anything.

**Covers:** FR26, UX-DR87 (zone header collapse), UX-DR22

**Acceptance Criteria:**

**Given** a zone header
**When** staff activate its collapse control (click, tap, or Enter/Space with focus)
**Then** the zone shrinks to one row showing zone name, tile count, a count per state (e.g. "5 in session · 2 free · 1 offline") and the zone's `alert-badge` count

**Given** a collapsed zone
**When** a device in it changes state or raises an alert
**Then** the counts and the alert count update within 2 s without expanding the zone

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
I want to tap a tile to open a sheet beside the floor and lock, unlock, message, restart or shut down that PC from it,
So that I can handle a device in two taps while the floor stays live.

**Covers:** FR24 (sheet shell, lock), FR39, NFR3 (remote result ≤ 5 s), AD-25, UX-DR65, UX-DR97, UX-DR101 (Failed — retry), UX-DR104, UX-DR105 (Esc)

**Acceptance Criteria:**

**Given** Floor view
**When** staff tap a tile or press Enter on it
**Then** a non-modal `tile-sheet` docks right, the floor reflows to 4 columns at 1920px and stays live and operable behind it, focus moves into the sheet, and Esc closes it and returns focus to the tile

**Given** the sheet for a device
**When** it opens
**Then** it shows device name, group, zone, group rate and status, and only the actions valid for that state (UX-DR97): free/in-session PCs show Lock, Unlock, Message, Restart and Shut down; maintenance shows "Return to service"; unassigned shows "Assign to group" (Story 2.19 intents); offline shows last seen; remote unlock for admin exit (Story 1.18) moves into this sheet behind `inline-pin`

**Given** a Counter on an online PC
**When** they choose Lock, Unlock or Message (with text up to 140 characters)
**Then** a `device.screen` intent (permission held by Counter) sends `pc.lock`, `pc.unlock` or `pc.message` to that kiosk; the kiosk covers the screen with "Locked by staff", removes that cover, or shows the message as a banner until dismissed, and confirms to the Hub

**Given** Restart or Shut down
**When** chosen by a role without `device.remote`
**Then** `inline-pin` opens in the sheet; with elevation the kiosk service restarts or shuts down Windows, and the action is audited with both actors

**Given** Restart or Shut down on a PC with an active session
**When** it is requested
**Then** it is refused `PC_BUSY` with the inline reason "End or pause the session first"

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
**Then** `player.create` (permission `player.manage`) creates `players`, `wallets` (balance 0), `credit_balances` (0) and `consents` (purpose transactional, current policy version from the catalogue, granted 1 or 0, timestamp) in one intent
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
**Then** it decrypts phones and names into an in-memory index only (never written to disk in clear), updates it on every create/edit/erase, and returns results for a 3,000-player branch within 100 ms server-side

**Given** no player matches
**When** the field holds a complete number
**Then** it shows "No player with that number." with **Create profile** (opens Story 3.6 with the phone prefilled) and **Guest**

**Given** a picked player
**When** the Players screen opens their profile
**Then** it shows name, username, phone, tags, wallet balance and consent state

**Given** focus is not in a text field
**When** staff press Ctrl+K
**Then** the lookup field on the current screen (or the Players screen if none) gets focus; Ctrl+3 opens Players

**Given** an erased or deleted player
**When** staff search
**Then** they never appear in results

### Story 3.8: Edit a player profile and reset their password

As counter staff,
I want to update a player's details and set a new password when they forget it,
So that players never lose their account because they forgot a password.

**Covers:** FR46, FR44 (edit), FR201 (consent change), AD-24, AD-39, UX-DR18

**Acceptance Criteria:**

**Given** a player's profile
**When** staff edit phone, name, email, DOB, notes or tags and save
**Then** `player.update` re-encrypts changed personal fields with the player's key, carries `base_rev`, and is rejected `STALE` with current values shown if the profile changed meanwhile; uniqueness rules from Story 3.6 still apply

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
**Then** a `wallet_txns` row (new table: player, kind wallet, delta, reason topup, method, staff, ref to the order) is appended, `wallets.balance_minor` increases, the payments are recorded, and `wallet.changed` updates every open view; the toast offers Undo as in Story 3.14 (`wallet.topup.reverted`)

**Given** a bill for a profiled player with a wallet balance
**When** staff add a wallet row in `payment-split`
**Then** the row shows the balance and accepts at most min(balance, remainder); settling appends a negative `wallet_txns` row in the same transaction

**Given** a wallet payment larger than the balance (e.g. a concurrent spend)
**When** it reaches `apply()`
**Then** it is rejected `INSUFFICIENT_BALANCE` and nothing is written; the balance never goes below zero

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
**Then** wallet and credit options become available if the linked player has them

### Story 3.18: Enable credit for a player and settle the remainder on credit

As a branch manager,
I want to give trusted players a credit limit and let staff put a bill's remainder on it,
So that regulars can play now and pay later without going over the limit I set.

**Covers:** FR49, FR50, FR70 (credit), NFR10 (credit ≤ limit), AD-30, UX-DR66, UX-DR72, UX-DR101 (credit block)

**Acceptance Criteria:**

**Given** a Manager on a player's profile
**When** they enable credit with a limit and a reminder cycle (default 3, 7, 14 days; editable list of days)
**Then** `credit.set_limit` (permission `credit.manage`) stores them; a limit below current outstanding is rejected `CREDIT_BELOW_OUTSTANDING`

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
**Then** no credit option exists

**Given** concurrent credit spends from two Hub windows
**When** both reach `apply()`
**Then** the limit check runs inside each transaction and the second is rejected if it would exceed the limit

### Story 3.19: Credit aging shown first, and auto-pause at 15+ days overdue

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
**Then** an outstanding section comes first with the total and buckets 0–7, 8–14 and 15+ days, computed by allocating collections to the oldest unpaid spends first (FIFO), plus the oldest unpaid date

**Given** a player whose oldest unpaid credit is 15+ days old and the branch rule "Auto-pause credit at 15+ days" is on (default on, in Settings → Billing)
**When** the daily system check runs at business-day start, or the player is looked up
**Then** a `credit.auto_pause` system intent sets `credit_paused`, the profile shows "Credit paused — 15+ days overdue", and new credit rows are refused `CREDIT_PAUSED` (the once-a-day override does not lift a pause)

**Given** a paused player
**When** a collection brings their oldest unpaid credit under 15 days or clears it
**Then** credit reopens automatically in the same transaction (`credit_paused = 0`)

**Given** the branch rule is off
**When** the check runs
**Then** no player is paused, and players paused earlier are reopened

### Story 3.20: Collect credit with partial payments and Undo

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
**Then** Undo (`credit.collect.reverted`) within 8 s restores outstanding, the allocation and a reopened/paused state exactly, under the Story 3.14 rules

**Given** the collection summary
**When** Epic 5 adds receipts
**Then** the summary data (cleared spends, outstanding after) is exposed on the `bill.settled` event so it can be printed without recomputation

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

**Covers:** UX-DR22, UX-DR60 (alert count), UX-DR107, UX-DR110, NFR15, NFR26 (bundled sounds)

**Acceptance Criteria:**

**Given** the Hub alert service
**When** an alert is raised for a device (any feature can raise a kind; a test fixture kind is used here)
**Then** the tile shows the `alert-badge` tag top-right, the header `alert-badge` count increments (extending the Story 2.11 alert list), and a bundled sound plays once and repeats every 30 s until the alert is acknowledged

**Given** an alert on a tile
**When** staff open that tile's sheet, or the alert row in the header list
**Then** the alert is acknowledged, the sound stops, and the badge clears; acknowledgement is written to the audit with staff and time

**Given** the header badge
**When** staff open it
**Then** a list shows each open alert with device, kind and age, and choosing one opens that tile's sheet

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

**Given** the new end would pass branch closing time (opening hours, Story 2.25)
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

**Given** a console device (Story 2.20) in a console group
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

## Epic 4: Free cafés run on an ad-supported Android Hub

A Free café runs its branch on an Android Hub (Tauri 2 mobile) with a non-intrusive banner, an interstitial only on idle return, and Free limits enforced.

> Draft notes: the Android Hub is the same Svelte Hub UI and the same Rust core (`arena-core`, `arena-store`, `arena-api`, `arena-tasks`) as the Windows Hub, hosted by Tauri 2 mobile. Epic 4 adds only what Android needs: the app build and its spec checks, a foreground service, mDNS through `NsdManager`, the Android Keystore, Bluetooth printing and a folder picker, phone layout behaviour, and the Free tier's ads. Everything from Epics 1–3 must run unchanged on it. Until this epic, a Free branch could not pair any Hub (Story 2.9 refuses Windows on Free). Story 4.4 closes that gap, and Story 4.8 proves a Free owner can go from sign-up to go-live on Android with no comp entitlement. **Ads** are reached only through an in-house Tauri Kotlin plugin (`tauri-plugin-arena-ads`). No Rust crate contains ad code. The Windows build ships without the plugin. Ads show when the platform is Android **and** the effective entitlement (Story 2.11) is Free. That includes the Free fallback after a failed licence check and after a lapse. A paid Android Hub shows none. **Banner:** the layout reserves a permanent fixed-height bottom slot while ads are enabled; sheets, dialogs and the lock screen stop above it, so the banner is always visible and never covered, and the slot keeps its height when AdMob has no fill. **Ad exclusion surfaces:** the interstitial checks one shared "ad-exclusion" registry in the UI package. In this epic the registry holds the `tile-sheet`, `settle-bar`, `undo-toast`, `inline-pin`, the More sheet and any dialog. The Hub POS cart and `phone-cart-bar` from Epic 5 add themselves to it, and Epic 4 does not wait for them. **Interstitial:** a standard AdMob interstitial is used (the policy-safest format), so Google controls its close button and timing. The UX spec's custom `ad-interstitial` chrome (solid `bg`, own "Ad" label, "Skip in 5 s" → "Skip ad" countdown, muted start) is **superseded**; DESIGN.md/EXPERIENCE.md `ad-interstitial` and UX-DR80 need updating to match. Its trigger rules stay: only on PIN unlock after an idle lock, never over an open cart or sheet, never after a settle, at most once per 15 minutes. **Owner ad controls** are a per-branch content rating (G · PG · T, default PG, never MA) applied through AdMob's maximum ad content rating request configuration on the Hub device (one Hub per branch). Free uploads **no** batches, summaries or rollups: ad impression counts travel inside the heartbeat. Phone POS (`pos-product-grid` full width, `phone-cart-bar`, full-height cart sheet) is built with the POS in Epic 5 on top of the phone rules from Story 4.5. The Android→Windows upgrade migration (FR183) is Epic 10. The rollup half of NFR5 is measured by the device-farm job from Story 4.17 once the rollup engine exists. Each story lists what it covers on its **Covers** line.

### Story 4.1: Android Hub app builds from the shared Hub code and checks the device spec on first run

As a Free café owner with an Android tablet or phone at the counter,
I want an Android Hub app built from the same code as the Windows Hub that tells me at once if my device is too small,
So that I get the same Hub on cheaper hardware and never set up on a device that will fail later.

**Covers:** AD-15, AD-16 (Tauri 2 mobile, shared UI and core), AD-8, AD-41, NFR17, NFR16; Additional Requirements "STARTER / SCAFFOLD" (Android Hub app), "CI/CD" (signed Android builds), "Signing and distribution" (build half); Stack pins (targetSdk 36, minSdk 28, NDK 28+, Kotlin 2.1+)

**Acceptance Criteria:**

**Given** the monorepo from Story 1.1
**When** CI runs the Android job
**Then** a Tauri 2 mobile Android Hub app builds as a signed release AAB and a debug APK from the same Hub SvelteKit (`adapter-static`) front end and the same `arena-hub` Rust crates as the Windows Hub
**And** the Gradle and Tauri config pin `targetSdk 36`, `minSdk 28`, NDK 28+ and Kotlin 2.1+, and a CI step fails if any pin changes without updating the pin table in the repo README

**Given** the release AAB
**When** CI inspects it
**Then** every native library is aligned for 16 KB memory pages, ABIs `arm64-v8a` and `x86_64` are included, and the upload key comes from SOPS-managed secrets, never from a plain file in the repo

**Given** the Rust core on Android
**When** the app starts
**Then** `arena-hub` runs in-process on its own Tokio runtime thread and opens its SQLite file in app-private storage with the Story 1.4 settings (WAL, `synchronous=FULL`, migrations, `hub_instance_id` minted once)
**And** `/healthz` on loopback reports the same fields as on Windows plus `platform: android`

**Given** a device running Android 8.1 or earlier
**When** someone tries to install the app
**Then** it cannot be installed (minSdk 28)

**Given** a device with less than 3 GB RAM, or less than 20 GB of free storage on the app's data volume
**When** the app opens for the first time, before the pairing screen
**Then** it shows one plain screen naming the requirement that failed and the value found (for example "This device has 14 GB free. The Hub needs 20 GB free."), with "Check again", and does not create the branch database
**And** a device that meets every requirement goes straight to the "Pair this Hub" screen

**Given** the app manifest
**When** CI checks it
**Then** Android auto-backup and device-to-device transfer exclude the database, the licence cache and every key file, so branch data and tokens are never copied to a cloud account or another phone

**Given** the Android System WebView on the device is older than the minimum version recorded in the README
**When** the Hub UI would load
**Then** a plain screen asks the user to update "Android System WebView" from Play, and the Rust core keeps running

**Given** the message-catalogue lint and the axe scan from Epic 1
**When** they run on the Android build's UI bundle
**Then** they pass with the same rules as on Windows, and every new string in this story is in the catalogue

### Story 4.2: The Android Hub keeps serving while the screen is off, in Doze and after a reboot

As counter staff on an Android Hub,
I want the Hub to keep running whatever the tablet does,
So that kiosks never lose their Hub because Android put the app to sleep.

**Covers:** AD-16, AD-1, AD-12, NFR1, NFR9, NFR17; Additional Requirements "Android Hub specifics" (foreground service, WAKE_LOCK + battery-optimisation exemption)

**Acceptance Criteria:**

**Given** the Hub app is started
**When** the Rust core comes up
**Then** it is hosted by an Android foreground service of type `connectedDevice`, not by the activity, with a persistent notification showing branch name and "Hub running · n devices online", updated when the count changes

**Given** the activity is destroyed (rotation, Back to home, swiping the app from recents)
**When** the foreground service is running
**Then** the core keeps serving kiosks without restarting, and reopening the app reattaches the UI to the running core within 2 s

**Given** first-run setup on Android
**When** the owner reaches the setup screen after the spec check
**Then** the Hub asks for notification permission (Android 13+) and for exemption from battery optimisation through the system dialog, explaining in one line why
**And** if either is refused, the Hub keeps working, a `status-badge` "Battery optimisation on" stays in System, and an `alert-badge` in the header opens a screen with a button to the system setting

**Given** the service is running
**When** the screen turns off or the device enters Doze (forced by `adb shell dumpsys deviceidle force-idle` in the test)
**Then** a partial wake lock and a high-performance Wi-Fi lock keep the core and the LAN up, and a fake kiosk sees no missed frame and no reconnect for 30 minutes

**Given** the Hub UI is in the foreground
**When** no one touches the screen
**Then** the screen stays on (keep-screen-on flag), and the Hub idle lock from Story 1.12 still applies after its timeout

**Given** the device reboots, or the app is updated
**When** Android finishes booting or replacing the package
**Then** the foreground service starts by itself without anyone opening the app, and kiosks reconnect by themselves

**Given** Android kills the process (low-memory killer, simulated with `am kill`)
**When** the system restarts the sticky service
**Then** every committed change is present, the database passes `PRAGMA integrity_check`, and the timer heap is rebuilt as on Windows

**Given** staff choose "Stop Hub" in System
**When** a Manager confirms with `inline-pin`
**Then** the service stops, the notification goes, the stop is audited with both actors, and kiosks continue their sessions offline under AD-12; opening the app starts the Hub again

### Story 4.3: Kiosks find and pair with an Android Hub on the LAN

As a café owner with an Android Hub,
I want my gaming PCs to find the tablet Hub by themselves exactly as they would find a Windows Hub,
So that pairing kiosks needs nothing extra on Free.

**Covers:** FR15 (Android Hub side), AD-6, AD-8 (shared mDNS service name), AD-11 (re-resolve), AD-25, AD-42, NFR11, NFR17; Additional Requirements "Android Hub specifics" (mDNS via `NsdManager`, no Tunnel/standby on Android)

**Acceptance Criteria:**

**Given** a paired Android Hub on Wi-Fi or Ethernet
**When** the core opens its listeners
**Then** WebSocket TLS on `:7400` and HTTP(S) on `:7401` are reachable from the LAN, and an in-house Tauri Kotlin plugin registers the `arena-proto` mDNS service through `NsdManager` with the same TXT records as Windows (branch name, Hub name, protocol version, certificate fingerprint)

**Given** an unpaired kiosk on the same LAN
**When** it runs discovery (Story 2.16)
**Then** it lists the Android Hub within 10 s, and pairing by the 6-digit code or manager PIN (Story 2.17) works unchanged, with the kiosk shown as Unassigned within 60 s

**Given** Settings → Devices on the Android Hub
**When** it opens
**Then** it shows the Hub's current LAN IP and the network name, for typed-IP fallback

**Given** the Android Hub's IP changes (DHCP renewal, Wi-Fi reconnect)
**When** the network callback fires
**Then** the plugin re-registers the service with the new address within 5 s, and paired kiosks re-resolve the Hub by mDNS before retrying their stored IP, reconnecting within 60 s with jittered backoff 2–30 s

**Given** the device loses Wi-Fi entirely
**When** the LAN is gone
**Then** the Hub shows an `alert-badge` "No LAN connection" in the header, keeps trading locally, and re-advertises when the network returns

**Given** the Android Hub
**When** staff look for Tunnel remote devices or standby pairing
**Then** neither is offered on Android; where those sections exist they read "Needs the Windows Hub"

**Given** the LAN client-class tests from Story 1.7
**When** they run against the Android Hub in CI on an emulator
**Then** they pass unchanged, including `FORBIDDEN` for out-of-scope browser classes and `TOKEN_REVOKED` closing a socket at once

### Story 4.4: The Android Hub pairs with the wizard code and keeps its secrets in the Android Keystore

As a Free café owner,
I want the setup wizard to send me to the Android Hub and pair it with the same 8-character code,
So that my Free branch gets its Hub without a Windows PC, and its keys stay locked to that device.

**Covers:** FR13 (Free download path), FR14 (Free pairs only Android), AD-10, AD-22 (Android Keystore), AD-29, NFR11, NFR17, UX-DR84, UX-DR85, UX-DR103

**Acceptance Criteria:**

**Given** wizard step 3 for a Free branch
**When** it opens
**Then** "1 · Download" offers the Android Hub (Google Play link from configuration) with the minimum spec "Android 9 or later · 3 GB RAM · 20 GB free", and the Windows download is not offered; the `pairing-code` card is the same as in Story 2.8
**And** for a paid or comp branch the step stays as in Story 2.8, with Windows first and the Android link second

**Given** the Android Hub's "Pair this Hub" screen (the Story 2.9 screen)
**When** a valid code for a Free branch is entered
**Then** `POST /v1/pair` is sent with `platform: android` and accepted, the device token and the branch KEK are stored wrapped by a non-exportable Android Keystore key, and the licence is written to `licence_cache`

**Given** a wrong, used or expired code on Android
**When** "Pair" is pressed
**Then** the field clears, focus stays in it, and the Story 2.9 line "That code didn't work. Get a new code from your browser." shows; the on-screen keyboard stays open

**Given** the Keystore key is lost (app data cleared, or a Keystore reset after a lock-screen change)
**When** the Hub starts
**Then** it does not crash or open a blank branch; it shows "This Hub needs to be paired again" with the pairing screen, and the database file is left untouched for restore

**Given** the first heartbeat from the Android Hub
**When** the cloud records it
**Then** the wizard advances to step 4 within 5 s (Story 2.10), `hubs.platform = android` is shown on Account → Branches, and `branch.bootstrap` and the owner's setup code (Stories 2.12–2.13) work on the tablet with the on-screen keyboard

**Given** the stored token and KEK
**When** a test reads the app's private files
**Then** neither appears in clear, and the Keystore key cannot be exported from the device

### Story 4.5: The Hub works on an Android phone and tablet by touch

As counter staff using a phone or tablet as the Hub,
I want every Hub screen to fit, scroll and respond to touch and the Back button without hiding what I'm working on,
So that a small screen never slows me down at the counter.

**Covers:** FR202 (≥ 44 px incl. Android phone Hub), FR24 (sheet on phone), NFR15, UX-DR17 (scroll padding under sticky bars), UX-DR59, UX-DR60 (phone header), UX-DR71 (settle bar on phone), UX-DR87 (phone layout), UX-DR104, UX-DR109

**Acceptance Criteria:**

**Given** the Hub on a window below 840px wide
**When** the header renders
**Then** it is 64px with branch name, `alert-badge` count and signed-in staff; backup status and clock are dropped, and `chip-rule`/`chip-offline` move to a row under the header (per the phone mock)

**Given** a phone below 840px
**When** staff open a tile
**Then** the `tile-sheet` opens as a full-height sheet over the floor, its `settle-bar` pinned at the bottom of that sheet, and Android Back or Esc closes it and returns focus to the tile
**And** at 840px and wider the sheet docks right as in Story 3.5

**Given** the More sheet, a tile sheet or the lock screen is open
**When** the Android Back gesture or button is used
**Then** it closes the topmost sheet only, never exits the app or navigates away from an unsaved field; on the floor with nothing open, Back does nothing

**Given** Android 15 edge-to-edge enforcement
**When** the Hub renders on a device with a display cutout, gesture bar or 3-button bar
**Then** no control, chip or bar sits under the status bar, cutout or navigation bar (system insets applied), checked on a phone and a tablet in portrait and landscape

**Given** a text field or `inline-pin` gets focus
**When** the on-screen keyboard opens
**Then** the focused field scrolls above the keyboard, `inline-pin` opens the numeric keypad, and sticky bars never cover the focused element (scroll padding equals their height)

**Given** the device rotates, or a tablet is split-screened across 840px
**When** the layout switches between rail and `hub-bottom-bar`
**Then** the current screen, scroll position, open sheet and unsaved field contents are kept

**Given** the system font size is raised
**When** the Hub renders
**Then** text scales up to 130% with no clipped or overlapping text, and larger system settings are capped at 130%

**Given** an automated target-size and axe check on the phone (412 × 915) and tablet (1280 × 800) layouts of Floor, tile sheet with settle bar, Players, Shift placeholder and More sheet
**When** it runs in CI
**Then** every target is at least 44 × 44 px and there are zero serious or critical violations

### Story 4.6: The Android Hub prints over Bluetooth or LAN

As a Free café owner on an Android Hub,
I want to connect my receipt printer by Bluetooth or the network and print a test,
So that I can finish the printer step on Android just as on Windows.

**Covers:** FR10 (Android), AD-28 (Bluetooth printing plugin), AD-34, NFR18, UX-DR95

**Acceptance Criteria:**

**Given** Settings → Printer on the Android Hub
**When** a Manager presses "Find printer"
**Then** the Hub lists Bluetooth printers already paired in Android settings (through an in-house Tauri Kotlin Bluetooth SPP plugin behind the printer adapter) and LAN printers on port 9100 (from the Rust `printer` task), plus the manual IP field; USB is shown as "Not supported on Android"

**Given** Bluetooth permission (Android 12+) has not been granted
**When** "Find printer" is pressed
**Then** the Hub asks for it with a one-line reason; if refused, the Bluetooth list shows "Bluetooth permission is off" with a link to settings and the LAN search still runs

**Given** a selected Bluetooth or LAN printer and paper width 58 or 80 mm
**When** "Test print" is pressed
**Then** the Story 2.26 test page prints, the result is recorded as setup status ok or failed, and a printer that is off or out of range fails within 10 s with a plain error and never blocks the Hub

**Given** the Bluetooth printer drops mid-print
**When** the plugin reports the failure
**Then** the outbox result records it as failed, the header shows the printer-offline `alert-badge`, and a later print retries the connection

**Given** the Bluetooth plugin
**When** it is tested in CI
**Then** it runs against the printer adapter's recorded fake, and "Skip printer for now" (Story 2.26) works on Android exactly as on Windows

### Story 4.7: The Android Hub saves backups to a folder the owner chooses

As a Free café owner on an Android Hub,
I want to choose where backups go, including a USB drive,
So that I can finish the go-live checklist on Android and keep my data off the tablet.

**Covers:** FR115 (backup location on Android), FR11 (go-live input on Android), AD-22, NFR17

**Acceptance Criteria:**

**Given** Settings → System → Backup location on Android
**When** a Manager picks a location
**Then** the system folder picker opens (Downloads, internal storage or a USB OTG drive), the Hub keeps persistent permission to that folder, writes and deletes a probe file to check it, and shows an inline error if the folder is not writable

**Given** a writable folder is chosen
**When** the Hub computes setup status
**Then** backup location reports set, and the change is audited with the staff id

**Given** the chosen USB OTG drive is unplugged or the permission is revoked
**When** the Hub next checks the location (at start and hourly)
**Then** setup status reports backup location not set, and System shows "Backup folder not available" with "Choose again"

**Given** a Counter without settings permission
**When** they try to change the location
**Then** `inline-pin` opens for manager elevation and the change is audited with both actors

### Story 4.8: A Free café goes from sign-up to go-live on Android with no help

As a Free café owner,
I want the whole setup, from signing up to going live, to work with only an Android Hub,
So that I can open my café on Free without a Windows PC, a comp grant or a support call.

**Covers:** FR6 (Free path end to end), FR11 (go live on Android), FR14 (Windows refused, Android accepted), FR4, NFR27 (onboarding telemetry on Android), NFR17

**Acceptance Criteria:**

**Given** an end-to-end test on an Android emulator or device-farm device with a Windows kiosk VM on the same virtual LAN
**When** it runs sign-up → Free → pair the Android Hub → owner first sign-in → pair a kiosk → group it in the wizard → set a rate → skip the printer → choose a backup folder → tick ad consent → 2-minute test session → go live
**Then** every step completes with no comp entitlement, the kiosk locks at zero, and the branch shows Live

**Given** the same Free branch
**When** the test first tries to pair a Windows Hub with the step 3 code
**Then** it is refused `FEATURE_NOT_ENTITLED`, the code is not consumed, and the same code then pairs the Android Hub

**Given** the go-live checklist on that branch
**When** step 10 opens
**Then** opening hours, backup location and ad consent show as done from the Android Hub's setup status, and standby PC shows "Paid tiers only" without blocking

**Given** the completed run
**When** `onboarding_progress` is read
**Then** it holds every step time from sign-up to go-live with no PII, the same as on Windows

**Given** the end-to-end job
**When** CI schedules it
**Then** it runs nightly and on every release candidate, and a failing step fails the release with the step name and a screenshot

### Story 4.9: Free limits — 3 users, 20 devices and one branch — enforced and explained at the Hub

As a Free café owner,
I want the Hub to show how many users and devices I have left and stop clearly at the Free limits,
So that I understand what Free includes and where to upgrade.

**Covers:** FR107, FR20 (Free device cap on Android), AD-29, AD-10 (limits from the licence), NFR11, UX-DR24, UX-DR88

**Acceptance Criteria:**

**Given** a Free licence with `limits.users = 3`
**When** `apply()` processes a `staff.upsert` that would add a user
**Then** it counts staff rows that are active and not deleted, including the Owner and invited users who have not yet used their setup code, and excluding the Kiosk-device role; a fourth is rejected `USER_LIMIT` in the permission step and nothing is written

**Given** Settings → Staff on the Hub (Owner or Manager)
**When** it opens
**Then** it lists users with name, role and status (active / invited), shows "2 of 3 users" in `hub-label`, and at 3 of 3 shows `tier-lock` "Available on Basic" with an underlined "Upgrade" link

**Given** an Owner on Settings → Staff
**When** they deactivate a staff member, confirming with their PIN
**Then** a `staff.deactivate` intent ends that person's Hub sign-in at once, frees a slot, and is audited with both actors; the Owner cannot deactivate themselves or the last Owner

**Given** a Free branch at 20 devices
**When** a kiosk tries to pair with the Android Hub
**Then** the Story 2.18 `DEVICE_LIMIT` refusal and `tier-lock` apply unchanged, verified on Android

**Given** a Free tenant
**When** the owner tries to add a second branch (Story 2.29) or pick Free for another branch (Story 2.7)
**Then** both are refused as before, and an API test confirms the cloud rejects a second Free branch even when called directly

**Given** a licence whose `limits` claim is missing or fails verification
**When** the limits are read
**Then** the Free defaults from the `arena-proto` tier table apply (users 3, devices 20, branches 1)

### Story 4.10: In-house AdMob plugin and the ads gate, with no ad code in Rust or on Windows

As the Arena 360 team,
I want ads to reach the Hub only through one small Kotlin plugin that switches on solely for Free on Android,
So that ads can never leak into Rust, the Windows Hub, a kiosk or a paid branch.

**Covers:** FR186 (Windows build has no ad SDK), AD-16, AD-29 (hub-android ads), NFR11, NFR17; Stack pin (Google Mobile Ads SDK 25.5.x)

**Acceptance Criteria:**

**Given** the Android Hub project
**When** the plugin `tauri-plugin-arena-ads` is built
**Then** it is a Kotlin module in the repo depending on Google Mobile Ads SDK 25.5.x (pinned), exposing only `init`, `banner.place(rect)`, `banner.hide`, `banner.destroy`, `interstitial.preload`, `interstitial.show`, and events `ad.loaded`, `ad.failed`, `ad.shown{slot}`, `ad.clicked`, `ad.dismissed`
**And** SDK initialisation and ad loads run off the main thread, verified by StrictMode in debug builds reporting no main-thread network or disk access from the plugin

**Given** the UI package
**When** the ads gate is evaluated
**Then** ads are enabled only when the platform is Android **and** the Hub's effective entitlement (Story 2.11) is Free, which includes the Free fallback for a failed licence check; the Hub emits `entitlement.changed` to the UI whenever the effective tier changes, and the gate re-evaluates on it without a restart

**Given** the gate is off (paid Android Hub, or any Windows Hub)
**When** the app runs
**Then** the plugin is never initialised, no ad request is made, and no ad slot is rendered

**Given** ad unit IDs
**When** CI builds debug and release
**Then** debug builds use Google's test ad unit IDs and the AdMob app ID comes from build config; CI fails if a release build contains a test unit ID or a debug build contains a production unit ID

**Given** the Rust workspace
**When** the CI ad-code lint runs
**Then** it fails if any crate depends on an ads/AdMob crate or contains ad symbols or ad endpoints, and the plugin is the only module that imports `com.google.android.gms.ads`

**Given** the Windows Hub and Kiosk builds
**When** CI inspects their artefacts and Tauri plugin lists
**Then** neither includes the ads plugin or any ad SDK or ad endpoint string, and the shared ad components are no-ops when the platform is not Android

**Given** a dev-only gallery route on a debug Android build
**When** a developer opens it
**Then** a test banner and a test interstitial can be loaded and shown, and each plugin event is listed as it fires

### Story 4.11: An always-visible banner in a permanent slot that nothing covers

As counter staff on a Free Android Hub,
I want the ad banner to sit in its own permanent strip that never covers anything and is never covered,
So that ads never slow a sale or cause a mis-tap.

**Covers:** FR184, UX-DR79, UX-DR17 (scroll padding), UX-DR87, UX-DR104, UX-DR109

**Acceptance Criteria:**

**Given** ads are enabled
**When** any Hub screen renders below 840px
**Then** the layout reserves a permanent fixed-height `ad-banner` slot (plain `surface` strip, 1px `line` edge, "Ad" label in `hub-micro` muted) between the main content and `hub-bottom-bar`, as in the phone mock, and the plugin places an anchored adaptive banner inside exactly that rect

**Given** ads are enabled at 840px and wider
**When** a screen renders
**Then** the permanent slot spans the bottom of the window below the main content and the `tile-sheet` column, never under the rail, with at least 24px between the slot and a docked `settle-bar` so they never share an edge

**Given** a `tile-sheet` with `settle-bar`, the More sheet, a dialog, `inline-pin` or the idle lock screen opens
**When** it lays out
**Then** it stops above the banner slot (a full-height phone sheet ends at the slot's top edge, with at least 24px between its `settle-bar` and the slot); no app surface ever draws over the slot and the banner never draws over app content, so the banner stays visible on every screen

**Given** the banner slot
**When** content scrolls
**Then** the floor grid, lists, sheets and forms scroll above the slot and never under it, and scroll padding equals the slot height so no focused element is ever hidden behind it

**Given** no ad is available (no fill, WAN down, load error)
**When** the slot renders
**Then** the strip keeps its height with only the "Ad" label, so nothing shifts under a finger; the slot never collapses or grows while ads are enabled

**Given** the app goes to the background or the screen turns off
**When** the activity pauses
**Then** the banner is paused, makes no requests, and resumes when the activity resumes

**Given** a banner tap
**When** the ad opens in the browser
**Then** the Hub core keeps serving kiosks, and returning shows the same screen with its state kept

**Given** an automated layout test at 412 × 915, 800 × 1280 and 1280 × 800 on Floor, a tile with the settle bar open, the More sheet, a dialog, the idle lock screen and Settings
**When** it compares the banner rect with every tile, sheet, dialog, the `settle-bar` and the bottom bar
**Then** no intersection exists in any case and the banner rect is fully visible, and the test fails otherwise

### Story 4.12: Safe ad categories, a per-branch content rating and the ads consent view

As a Free café owner,
I want ads on my counter screen to stay away from adult, gambling and alcohol content and to let me make them stricter,
So that nothing embarrassing shows at the counter in front of my customers.

**Covers:** FR186 (excluded categories, owner blocks more, consent view), FR115 (ads consent), UX-DR24, UX-DR88 (Settings → Ads)

**Acceptance Criteria:**

**Given** the AdMob account for the Hub app
**When** the release checklist runs
**Then** the sensitive categories adult/sexual, gambling and betting, and alcohol are blocked at app level, recorded as a versioned config note in the repo and checked by a named person before each release

**Given** Settings → Branch → Ads on a Free branch (the Story 2.25 consent view)
**When** an Owner or Manager opens it
**Then** it shows what is always excluded (adult, gambling, alcohol), a per-branch content rating choice G · PG · T labelled in plain words (default PG; MA is never offered), the ad policy link, and the consent tick with time and staff id; there is no control to turn ads off, only "Ads keep Arena 360 free. Upgrade to remove them." with an underlined "Upgrade" link

**Given** the branch content rating
**When** the plugin initialises or the rating changes
**Then** it sets the AdMob request configuration's maximum ad content rating to that value on the Hub device (one Hub per branch, so the setting is per branch), and every banner and interstitial request uses it; a stored value of MA or an unknown value is treated as PG

**Given** a stricter content rating is saved
**When** `settings.set` applies
**Then** it is audited, and the next ad request uses it without a restart

**Given** the ads consent tick is not yet recorded
**When** ads are requested
**Then** they are requested as non-personalised; after consent, standard requests are used

**Given** a Counter without settings permission
**When** they change the content rating
**Then** `inline-pin` opens for manager elevation and the change is audited with both actors

### Story 4.13: A standard interstitial only when staff come back from the idle lock

As counter staff on a Free Android Hub,
I want any full-screen ad to appear only when I come back to an idle Hub, and never mid-task,
So that ads never interrupt a sale or a session I'm handling.

**Covers:** FR185, FR106 (idle-lock return), UX-DR80 (trigger rules; custom skip UI superseded), UX-DR102 (interstitial due), UX-DR104

**Acceptance Criteria:**

**Given** ads are enabled and the Hub UI locks after its idle timeout (Story 1.12)
**When** the lock screen shows
**Then** the plugin preloads one standard AdMob interstitial while locked, without blocking the lock screen or the core

**Given** staff unlock with a correct PIN
**When** an interstitial is loaded, at least 15 minutes have passed since the last one, and no ad-exclusion surface was open at lock time
**Then** the plugin shows the standard AdMob interstitial, whose close control and timing are Google's (no custom Skip countdown or overlay is drawn), and when it is dismissed the Hub shows the exact screen from before the lock

**Given** a `tile-sheet`, `settle-bar`, `undo-toast`, `inline-pin`, dialog or any other ad-exclusion surface (including the Hub cart once it registers) was open when the lock started
**When** staff unlock
**Then** no interstitial shows (it is skipped, not delayed), and unlock returns straight to that surface

**Given** a settle
**When** it commits, its undo window runs, or the bill closes
**Then** no interstitial is ever triggered; the only trigger is a PIN unlock after an idle lock

**Given** no interstitial is loaded (no fill, WAN down, load still running), or the last one showed less than 15 minutes ago
**When** staff unlock
**Then** the Hub returns at once; unlock never waits for an ad

**Given** the interstitial is showing
**When** a kiosk alert arrives (call staff, time up, login lockout)
**Then** the alert sound plays, its `alert-badge` is waiting when the ad closes, and the core keeps applying intents throughout

**Given** the interstitial closes, fails to show, or the user leaves through the ad
**When** the Hub UI returns to the foreground
**Then** focus returns to the element focused before the lock, and the idle timer restarts from zero

### Story 4.14: Ad impressions are recorded without ids and sent with the heartbeat only

As the Arena 360 team,
I want to count ad impressions per Free branch without collecting device or creative ids,
So that we can track ad revenue health while Free branches upload nothing else.

**Covers:** AD-16 (`ad.shown` → `ad_impressions`), AD-2, AD-36 (Free uploads only heartbeat, ad impressions, catalogue pull), AD-9, NFR13, NFR20, NFR25, NFR27 (ad metrics instrumentation); Inherited Constraint "ad rows carry no kiosk device/creative ids"

**Acceptance Criteria:**

**Given** the migration added by this story
**When** the Hub schema is inspected
**Then** `ad_impressions(id, slot CHECK IN ('banner','interstitial'), at)` exists with no device, creative, advertiser or advertising-ID column, plus a single-row `ad_upload_cursor(through_id)`

**Given** the plugin emits `ad.shown{slot}` on the SDK impression callback
**When** the UI forwards it
**Then** a system intent `ad.impression_recorded{slot}` with an idempotency key from the event is applied through `apply()`, writing one row; a duplicate event writes nothing

**Given** a heartbeat from a Free branch
**When** it is built
**Then** it carries `ad_impressions{through_id, counts: [{business_day, slot, n}]}` for rows after `ad_upload_cursor`, aggregated by day and slot, and no other branch data; `last_seq_uploaded` stays 0 and the Hub makes no presign, batch, summary or rollup call

**Given** the cloud handles that heartbeat
**When** `through_id` is beyond the branch's stored cursor
**Then** it adds the counts to `gz_control.ad_impression_daily(branch_id, day, slot, count)` and stores the new cursor in the same transaction, and the response returns the acknowledged `through_id`, which the Hub saves to `ad_upload_cursor`
**And** a repeated heartbeat with a `through_id` at or below the stored cursor adds nothing

**Given** the WAN is down for days
**When** it returns
**Then** the next heartbeat sends every pending day's counts once, and no impression is lost or double-counted (integration test with a 3-day outage)

**Given** the heartbeat payload and Hub logs
**When** a test inspects them
**Then** they contain no advertising ID, creative ID, ad unit response data, kiosk device id or player data

**Given** the cloud metrics
**When** impressions arrive
**Then** daily impressions per slot across Free branches are exported to Grafana Cloud with no tenant names, ready for the ad-fill business metric

### Story 4.15: Upgrading removes ads within one heartbeat, and a lapse brings them back only after expiry

As a café owner,
I want ads to disappear as soon as my branch is paid and to come back only if my paid entitlement actually ends,
So that I get what I pay for straight away and am never shown ads while I'm still covered.

**Covers:** FR186 (upgrade removes ads within one heartbeat; lapsed branch resumes ads only after entitlement expiry), AD-10, AD-29, AD-44, NFR22

**Acceptance Criteria:**

**Given** a Free Android Hub showing the banner
**When** an operator grants a comp Basic entitlement (Story 2.7) and the next heartbeat returns the new licence
**Then** within that heartbeat the gate turns off: the banner is destroyed, its slot is removed and the layout reflows without moving focus, any preloaded interstitial is discarded, and no further ad request is made

**Given** a paid Android Hub
**When** it starts with the WAN down and a valid cached paid licence
**Then** no ad is shown at any point

**Given** a paid (comp) entitlement that ends
**When** the cloud signs the next licence as Free after the entitlement end
**Then** ads come back after that heartbeat; while the paid licence is still current on `trusted_now`, ads stay off even if the wall clock is moved forward

**Given** a paid licence that passes its `exp` on `trusted_now` with the WAN down
**When** the Hub re-evaluates the entitlement
**Then** the effective tier becomes Free, the gate turns on, and ads appear once the network allows a load

**Given** a tampered or unverifiable licence on an Android Hub
**When** the Hub falls back to Free limits (Story 2.11)
**Then** ads are enabled as for Free

**Given** each gate change
**When** it happens
**Then** an audit row records the old and new effective tier and the time, with `actor{kind: system}`

### Story 4.16: The Android Hub ships through Google Play with a staged rollout that never interrupts a session

As the Arena 360 team,
I want the Android Hub released through Play to the right devices in stages, and installed without cutting a live session,
So that Free cafés get updates safely and a bad build can be stopped early.

**Covers:** AD-32 (Play with staged rollout), AD-8 (rollout order), NFR17, NFR19; Additional Requirements "Signing and distribution", "Rollout order" (Android Play staged rollout), release train (Hub Android every two weeks)

**Acceptance Criteria:**

**Given** a release tag
**When** the CI release job runs
**Then** it uploads the signed AAB to the Play internal testing track through the Play Developer API with Play App Signing, using credentials from SOPS-managed secrets, after cloud release N is live (cloud first)

**Given** the Play listing
**When** it is configured
**Then** it declares "Contains ads", a data safety form matching the ad SDK's data use, the foreground-service type declaration, and device catalogue rules excluding devices with less than 3 GB RAM; minSdk excludes Android 8.1 and earlier

**Given** a build promoted to production
**When** the release manager starts the rollout
**Then** it goes 1% → 10% → 50% → 100% over a week by Play staged rollout, and the runbook says to halt when the Play crash rate or the fleet heartbeat-gap metric for that version goes above the recorded threshold

**Given** an update is available on the Hub
**When** the Hub checks through the Play in-app updates API
**Then** it downloads in the background and offers "Update ready" in System, and installs only when no session is active, paused or in grace; an update that Play forces while sessions run restarts the Hub service by itself after install, and kiosks continue offline meanwhile (AD-12)

**Given** the Play pre-launch report
**When** a release candidate is tested
**Then** it has no crashes or accessibility errors on the tested devices before it can go past internal testing

### Story 4.17: Android Hub proven on a 3 GB device with 20 kiosks, Doze and no WAN

As a Free café owner on a budget tablet,
I want the Android Hub proven to run a full floor on the cheapest supported device,
So that I can trust it on a busy evening with ads on screen.

**Covers:** NFR1, NFR3, NFR5 (3 GB Android device; summary/rollup timing joins when those engines exist), NFR9, NFR17, AD-33; LLD Test plan "Android Hub"

**Acceptance Criteria:**

**Given** a device-farm device with Android 9 and 3 GB RAM, and a second with the latest Android version
**When** the per-release Android job runs with 20 fake kiosks over Wi-Fi
**Then** a 60-minute mixed load of start, pause, extend, transfer, end and settle intents at 5/s keeps server-side apply under 100 ms at p95 and floor updates within 2 s, with no intent lost or duplicated; the job fails the release otherwise

**Given** the same load
**When** the device is forced into Doze and the screen is off for 30 minutes
**Then** the foreground service keeps serving, every kiosk stays connected, and timers and time-up locks fire on time

**Given** the banner and interstitial are active during the load
**When** ads load, show and fail
**Then** apply latency stays within the same budget and no UI frame blocks for longer than 100 ms while an ad loads

**Given** all WAN egress blocked
**When** the Epic 1–3 flows run on the Android Hub (sign-in, idle lock, PIN elevation, audit, settings, profiles, every session mode, settle, undo, credit, plans, pricing rules, console time-up)
**Then** results match a WAN-up run, the banner shows its empty strip, and no outbound connection is attempted other than the connectivity probe and ad requests

**Given** the process killed during the load
**When** the service restarts
**Then** every committed change is present, the timer heap is rebuilt and kiosks re-sync to the Hub's record

**Given** the job's measurement hooks
**When** a later story adds a measured operation (for example the daily rollup)
**Then** it is timed on the 3 GB device against its NFR budget in the same job without changing the harness

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

## Epic 6: Player experience at the kiosk

Players log in or sign up, see a HUD with warnings, extend through grace, launch games from the library, order from the seat, and keep their session when the Hub drops.

> Draft notes: this epic builds on the Epic 1 kiosk service, lockdown, allow-list (Story 1.17), admin exit and per-device HC (Story 1.19); the Epic 2 pairing, groups, group rates (Story 2.24) and branch operating settings (fulfilment, kiosk default volume, opening hours; Story 2.25); and the Epic 3 session engine: `session.start` / `session.extend` / `session.end` (Stories 3.9, 3.24, 3.27, 3.28), the timer engine and its 10/5/1-min warnings (Story 3.23), grace and time-up mode (Story 3.26), the minimal session screen and lock reasons (Story 3.10), the alert service (Story 3.25), `chip-rule` (Story 3.30), forced end (Story 3.34), player profiles, wallet, packs and credit (Stories 3.6, 3.16, 3.18, 3.22). From the Epic 5 draft it uses, by title: "Products with categories, barcodes, images and the kitchen flag" (the `catalog.changed` availability feed), "Bundles priced on their own", "Sell at the counter — a walk-up sale settles and takes stock", "Attach an order to a running session so it settles with the bill", "Take stock at served, mark items served, and cancel unserved lines into waste" and "Invoice numbers in an epoch-segmented series that never repeat". **Player actor:** kiosk-originated intents carry `actor{kind: player, id}` (or `{kind: device}` for guests and system events) plus the device id, travel through the kiosk service over the paired-device connection, and are checked against a fixed self-service permission set in `arena-proto` (`session.self_start`, `session.self_extend`, `session.self_end`, `order.self_place`, `player.self_update`); a player can only act on their own session and profile. **Login lockout** uses 5 failures → 60 s (decision), matching staff sign-in (FR120 updated; UX-DR28's "3 failures" is superseded); the catalogue line "Too many tries. Try again in 0:60." is unchanged. **Hub unreachable:** every story that adds a Hub-dependent player action (login, sign-up, plan request, Extend, Place order, I'm done, Log out, Call staff, change password, PC-settings save) disables it with "Counter is reconnecting — try again shortly."; Story 6.36 adds the shell-wide behaviour and the proof. **Out of this epic:** catalogue curation and flash-screening UI (Epic 11 admin app — this epic adds the `gz_control` catalogue, its read API and a seed import tool), KOT, the kitchen display and the READY tile badge (Epic 7 — this epic adds a Hub "Mark ready" action that the kitchen display will reuse), owner dashboards reading play hours and the low-balance WhatsApp message (Epic 8), the launcher-account vault and game requests (Epic 13), Pi/QR (Epic 12), and kiosk ads (none in Phase 1). Every story adds its new intents to the Story 1.6 `proptest` generator.

### Story 6.1: The control plane publishes the global game catalogue to Hubs

As a café owner,
I want my Hub to receive Arena 360's curated list of games with their covers and videos,
So that my PCs show proper game art without me uploading anything.

**Covers:** FR135 (source), AD-17, AD-9, AD-36 (shared catalogue media prefix), AD-40, AD-23, NFR14

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** new tables `catalog_games` (id ULID, name, launcher, category, age rating, published flag, `rev`, `updated_at`, soft delete), `catalog_exe_signatures` (game id, executable file name, product name, publisher, optional SHA-256) and `catalog_media` (game id, kind cover|video, content hash, bytes, `flash_screened` flag) exist, and a `catalog_version` counter increments on every published change

**Given** a paired Hub with a valid device token and licence
**When** it calls `GET /v1/catalog/manifest?since=<version>`
**Then** it receives only published games changed since that version (including tombstones for unpublished or deleted ones), each with exe signatures and media refs, plus the current `catalog_version`, with an ETag; an unchanged version returns 304

**Given** a video ref
**When** the manifest is built
**Then** it is included only if `flash_screened = true`; unscreened videos never reach a Hub, while the game's cover still does

**Given** catalogue media
**When** it is stored
**Then** it lives under the shared R2 prefix `catalog/<sha256>.<ext>` behind the CDN, is immutable (content-addressed), and the manifest gives each item's CDN URL, hash and size

**Given** dev, staging and launch before the Epic 11 admin app exists
**When** an operator runs `arena-cloud catalog import <file>` with a JSON/CSV list and media files
**Then** games, signatures and media are upserted idempotently by name + launcher, media are uploaded once by hash, the version increments, and the import is recorded in the admin audit with `actor.kind = admin`

**Given** an unauthenticated caller, a revoked device token, or more requests than the per-tenant limit
**When** the manifest endpoint is called
**Then** it returns 401, 403 or `RATE_LIMITED` respectively and no catalogue data

### Story 6.2: The Hub caches the catalogue and media and serves them on the LAN

As a café owner,
I want the Hub to keep a local copy of game art and serve it to every PC,
So that kiosks never reach the internet for media and art still shows when the WAN is down.

**Covers:** FR135 (Hub cache), AD-17, AD-11 (standby keeps its own cache), AD-42, NFR1, NFR11, UX-DR99

**Acceptance Criteria:**

**Given** the Hub service with the WAN up
**When** it starts, and every 6 hours after
**Then** it pulls the manifest from its last `catalog_version` and stores games, signatures and media refs in a separate reference database outside branch state and the change log, never through `apply()`

**Given** manifest entries
**When** the cache fills
**Then** covers for every catalogue game are downloaded first and always kept; videos are downloaded best-effort afterwards; each file is verified against its SHA-256 and a mismatch is discarded and retried on the next pull

**Given** the media cap (Settings → System → "Game media cache", default 10 GB, minimum 1 GB)
**When** stored media would exceed it
**Then** videos are evicted least-recently-served first; covers are never evicted; the screen shows used and cap sizes

**Given** a paired kiosk
**When** it requests `GET /media/<sha256>` on `:7401` with its device token
**Then** the Hub serves the file with immutable cache headers; an unknown or not-yet-downloaded hash returns 404 quickly; an unauthenticated request is refused

**Given** the WAN is down or the manifest call fails
**When** kiosks request media
**Then** everything already cached is served as normal, no player-visible error appears, and the next successful pull resumes where it stopped

**Given** a standby Hub, or a Hub promoted or restored
**When** it runs
**Then** it keeps or refetches its own reference cache from the manifest; nothing in the cache is replicated through the change log or included in branch batches and snapshots

### Story 6.3: The branch game and app library at the Hub, applied to every kiosk

As a branch manager,
I want one list of the games and apps on our PCs that I edit once at the Hub,
So that every PC shows the same library and can launch it.

**Covers:** FR132, AD-17 (`game.matched` branch rows), AD-39, FR117 (allow-list follows the library), UX-DR18, UX-DR66, UX-DR88

**Acceptance Criteria:**

**Given** Settings → Games on the Hub
**When** a Manager adds an entry
**Then** `library.upsert` (permission `games.manage`) stores kind (game or app), name, launcher (Steam · Epic · Riot · Battle.net · None), executable path, optional launch URI or arguments, category, tags, age rating and enabled flag in new `library_entries` table, with optional link to a global catalogue id

**Given** an entry linked to a global catalogue game
**When** it is shown or edited
**Then** name, launcher and age rating come from the catalogue and are read-only; category and tags stay editable for the branch

**Given** a saved, enabled entry
**When** it commits
**Then** a `library.changed` event reaches every connected kiosk within 2 s, and the entry's executable (and its launcher's executables) are added to the branch allow-list from Story 1.17 by signed path, so a library launch is never killed as unknown

**Given** an entry disabled or soft-deleted
**When** it commits
**Then** it disappears from every kiosk within 2 s and its executable leaves the allow-list unless another entry still needs it

**Given** a duplicate (same kind, launcher and executable path), an empty name, or a missing executable for a non-URI entry
**When** saved
**Then** a field error shows and nothing is written; edits carry `base_rev` and conflict as `STALE`

**Given** a Counter
**When** they edit the library
**Then** `inline-pin` opens for manager elevation and the change is audited with both actors

**Given** a kiosk that cold-starts with the Hub unreachable
**When** the shell needs the library
**Then** it uses the last library it received

### Story 6.4: Kiosks scan for installed games and launchers and report them to the Hub

As a branch manager,
I want each PC to find its installed games and launchers by itself,
So that I don't have to type install paths for every game on every PC.

**Covers:** FR133 (kiosk auto-scan), AD-17, AD-14, AD-31

**Acceptance Criteria:**

**Given** a paired kiosk
**When** the kiosk service first connects after pairing, and daily at 05:00 branch time when no session is running (or at the next moment with no session)
**Then** it scans Steam library folders and app manifests, Epic manifests, the Riot client install list and common folders (Program Files, Program Files (x86), `C:\Games` and other fixed drives' `Games` folders)

**Given** a completed scan
**When** the service reports it
**Then** it sends `kiosk.scan_reported` with each found executable's path, file name, product name, publisher and launcher, plus which of Steam, Epic and Riot are installed and their paths

**Given** a scan report
**When** the Hub receives it
**Then** it stores it as that device's latest scan in a new `kiosk_scans` table (device, scanned at, entries, installed launchers), replacing the previous one, and Settings → Games shows each PC's last scan time

**Given** a scan running
**When** a session starts on the PC
**Then** the scan pauses and resumes after the session, so it never slows a player's game

**Given** the Hub is unreachable when a scan completes
**When** the report is sent
**Then** it is queued with its idempotency key and applied once on reconnect

**Given** a folder the service cannot read
**When** the scan reaches it
**Then** it is skipped, logged and the rest of the scan continues

### Story 6.5: The Hub matches scanned games to the catalogue and the manager approves them in bulk

As a branch manager,
I want found games matched to Arena 360's catalogue and approved in one go,
So that the library fills with proper names and art in minutes.

**Covers:** FR133 (matching, bulk approval), AD-17 (`game.matched`), FR132, UX-DR88

**Acceptance Criteria:**

**Given** a stored scan report
**When** the Hub processes it
**Then** it matches each executable against the cached catalogue signatures (file name + product name/publisher, or SHA-256 when given), records each match with a `game.matched` intent (global id, device, path), and keeps unmatched executables as proposals named by product name or file name

**Given** Settings → Games → "Found on PCs"
**When** a Manager opens it
**Then** it lists proposals not yet in the library with matched name and cover (or the file name for unmatched ones), launcher, category from the catalogue, and the number of PCs where each was found; already-approved or dismissed items are not listed again

**Given** several proposals selected
**When** the Manager presses Approve
**Then** one `library.upsert` per proposal creates library entries linked to their catalogue ids, and per-device install paths are stored in a new `library_installs` table (entry, device, path), so the same game at different paths on different PCs works

**Given** a library entry
**When** a kiosk shows its library
**Then** the entry appears only on PCs where an install path is known for it, or where its configured executable path exists

**Given** a later scan on a PC that finds an already-approved game at a new path
**When** it is processed
**Then** the PC's install path is updated without a new proposal

**Given** a Manager presses Dismiss on proposals
**When** later scans report the same executables
**Then** they are not proposed again unless the Manager clears the dismissed list

**Given** a Counter
**When** they approve or dismiss proposals
**Then** `inline-pin` opens for manager elevation

### Story 6.6: Game media on the kiosk falls back from video to cover to solid

As a player,
I want game art and trailers to load cleanly and never show a broken player,
So that the screens look polished and nothing flashes or blares at me.

**Covers:** FR135 (fallback chain), NFR26, UX-DR15, UX-DR27, UX-DR99, UX-DR108, UX-DR4 (HC skips media)

**Acceptance Criteria:**

**Given** the shared UI package
**When** a `game-media` component renders for a library entry
**Then** it plays the entry's curated, flash-screened video from the Hub if one exists, else shows the cached cover, else (for unmatched entries) the launcher or executable icon extracted locally by the kiosk, else solid `bg`; any load or decode error falls straight to the next step with no visible error

**Given** a video plays
**When** it renders
**Then** it is muted with no audio track ever played, loops, uses only `:7401` media URLs (never an internet URL), and only one video plays on the screen at a time

**Given** media behind copy
**When** it renders
**Then** `scrim` (or `tile-scrim` on tiles) is always drawn with copy inside its copy zone, and no `backdrop-filter` or live blur is used (a lint fails the build on `backdrop-filter` in any branch surface)

**Given** a playing video
**When** the `video-control` (44px round button, `bg` fill, `border-strong` ring, icon showing the next action) is activated by click, Enter or Space
**Then** the video pauses on its current frame for the rest of that view and the button switches to Play; it is always visible while video plays and reachable by Tab

**Given** the component receives the shell's "game running" signal
**When** the signal is on
**Then** the video is unloaded entirely (not just paused) and only the cover shows until the signal clears

**Given** the HC theme on the device
**When** media would render
**Then** it skips straight to solid `bg` with no scrim, video or `video-control`

**Given** media files previously fetched
**When** they are requested again
**Then** they come from the WebView cache by content hash, so the attract and Home screens repaint without refetching

### Story 6.7: The attract screen shows branding, rates and the active rule, with no ads

As a player walking up to a free PC,
I want to see what the café charges and what's on right now,
So that I know the price before I log in.

**Covers:** FR122, FR63 (kiosk attract), UX-DR26, UX-DR76 (`chip-rule`), UX-DR86 (flow), UX-DR99 (cold load), UX-DR104 (no kiosk ads)

**Acceptance Criteria:**

**Given** Settings → Branch → Logo on the Hub
**When** a Manager uploads a PNG or SVG
**Then** it is resized to at most 512 px and 200 KB, stored by `settings.set`, and pushed to kiosks; with no logo set, `logo-slot` shows the Arena 360 mark

**Given** a kiosk with no session
**When** it is idle
**Then** it shows the attract screen in place of the Epic 1 locked placeholder: `logo-slot` top-left, branch name, the hourly rate of this PC's group, the active `chip-rule` when a rule applies to the group, and full-bleed `game-media` cycling the curated videos of the branch library's games (each up to 30 s), with no ad slot anywhere

**Given** a rule starts, ends or changes, or the group rate changes
**When** the Hub pushes the change
**Then** the attract rates and chip update within 1 min without a reload

**Given** the kiosk cold-starts
**When** the attract screen first paints
**Then** branding, rates and rule come from the kiosk's last cached copy before any network call, and media streams in afterwards

**Given** the attract screen
**When** the player presses any key, clicks or taps
**Then** the login panel opens over the same media; Esc on the login panel, or 60 s without input, returns to attract

**Given** a PC in maintenance, unassigned or with no group rate
**When** attract renders
**Then** it shows "Ask at the counter" in place of rates and the login panel is not offered

### Story 6.8: A player logs in at the kiosk with username and password

As a player,
I want to log in with my username and password in a couple of seconds,
So that I can start playing without queueing at the counter.

**Covers:** FR120, NFR6 (login < 3 s), NFR23 (kiosk login lockout), UX-DR18, UX-DR28, UX-DR101 (login lock), UX-DR107 (lockout alert), UX-DR112, UX-DR103

**Acceptance Criteria:**

**Given** the login panel (solid `surface` panel, radius xl, centred over muted media under the scrim)
**When** it renders
**Then** it shows username and password fields (show/hide, paste allowed), a primary "Log in", underlined "Forgot password?" and "Create account" links, and a footer with PC name, zone and kiosk version; there is no language picker and no social or QR login

**Given** valid credentials
**When** the player presses Enter or Log in
**Then** the kiosk sends `player.authenticate` through the service, the Hub verifies the Argon2id hash, records a `kiosk.player_login` audit row with player and device, and returns the player context in under 3 s on LAN, after which the kiosk shows the plan step for that player

**Given** a wrong username or password
**When** submitted
**Then** both fields show the error treatment with "Username or password didn't match." (never naming which), focus returns to the username field, and the password field clears

**Given** 5 consecutive failures on this kiosk
**When** the fifth fails
**Then** the panel disables both fields and shows "Too many tries. Try again in 0:60." counting down in tabular figures, the Hub raises a login-lockout alert on the tile with sound and badge (Story 3.25), and the counter resets after the 60 s or a successful login

**Given** "Forgot password?"
**When** activated
**Then** the panel shows "Ask staff at the counter to reset it." in place; no reset flow exists on the kiosk

**Given** a player who already has an active, paused or grace session on another PC
**When** they log in
**Then** login is refused with "You're already playing on <PC name>. Ask at the counter."

**Given** the Hub is unreachable
**When** the login panel shows
**Then** Log in is disabled with "Counter is reconnecting — try again shortly." inline, and it re-enables by itself on reconnect

**Given** the plan step is open with no input for 60 s
**When** the timeout passes
**Then** the kiosk forgets the player context and returns to attract

### Story 6.9: The plan picker starts a prepaid session from the kiosk

As a prepaid player,
I want to pick my pack, pass or wallet after logging in and start playing,
So that I don't need staff to start my time.

**Covers:** FR31 (player self-start), FR63 (plan picker), FR32, FR33, NFR10, UX-DR48, UX-DR101 (refusal), UX-DR76

**Acceptance Criteria:**

**Given** a logged-in player on the plan step
**When** the `plan-picker` renders
**Then** it shows only options valid for this PC's group as cards: each owned unexpired pack (hours left, expiry), each owned pass (window), each active block plan (price, end time) payable from the wallet, and Wallet (balance, hourly rate and the active `chip-rule`); postpaid is not offered on the kiosk

**Given** an expired pack, or a pass outside its window
**When** it renders
**Then** it is muted with an "Expired" tag and unused time ("0h 45m unused"), or the window, and cannot be selected

**Given** the player selects an option
**When** they confirm with the primary button
**Then** the selected card gets a 2px `text` border, the kiosk sends `session.start` with the matching mode and `actor{kind: player}` (permission `session.self_start`), and shows "Waiting for the counter…" with the selected plan

**Given** `apply()` accepts the start (rules of Stories 3.24 and 3.27)
**When** `session.started` arrives
**Then** the kiosk unlocks to the Story 3.10 session screen within 2 s and the floor tile shows the player in session

**Given** `apply()` refuses (`INSUFFICIENT_BALANCE`, `PACK_EXPIRED`, `PLAN_NOT_FOR_GROUP`, `PASS_OUTSIDE_WINDOW`, `PC_BUSY`)
**When** the refusal returns
**Then** the picker stays open with the typed reason (e.g. "Balance ₹40. Top up at counter.") and the player can choose another option

**Given** a block chosen
**When** the start applies
**Then** the block price is paid from the wallet in the same intent (wallet never below zero) and the session ends at the block window's end

**Given** the Hub is unreachable
**When** the player confirms
**Then** the request is not sent and the picker shows "Counter is reconnecting — try again shortly."

### Story 6.10: A new player signs up at the kiosk with one consent box

As a first-time player,
I want to create my account at the PC with just a few fields,
So that I can play without waiting for staff to set me up.

**Covers:** FR121, FR201 (consent), AD-24, NFR13, UX-DR18, UX-DR29, UX-DR101 (duplicate), UX-DR103

**Acceptance Criteria:**

**Given** "Create account" on the login panel
**When** the `signup-form` opens in the same shell
**Then** it shows username, password (show/hide, paste allowed), phone (default +91) and name, plus one consent checkbox, unticked, with ≥ 44px hit area and the text "I agree to the Terms and Privacy Policy and to receive service messages on WhatsApp/SMS." with underlined Terms and Privacy links that open read-only pages in the kiosk; there is no OTP and no marketing consent

**Given** the box is unticked
**When** the form renders
**Then** the primary "Create account" is disabled with the reason inline ("Tick the box to continue")

**Given** valid input and the box ticked
**When** submitted
**Then** `player.create` with `source: kiosk` and `actor{kind: device}` creates the player, wallet, credit balance and a granted consent row with the current policy version, encrypting personal data exactly as Story 3.6, and the player continues to the plan picker logged in

**Given** a username or phone already used at the branch
**When** submitted
**Then** the field shows "That username's taken." or "That phone number already has an account — ask staff if it's yours.", focus moves to it, and nothing is written

**Given** a password under 6 characters or an invalid phone
**When** submitted or the field blurs
**Then** the field shows its inline error and nothing is written

**Given** the Hub is unreachable
**When** the form is submitted
**Then** nothing is sent and the form shows "Counter is reconnecting — try again shortly."

**Given** the Hub audit and log
**When** a kiosk sign-up applies
**Then** the audit shows the player id, the kiosk device and the consent version, and the log carries no name or phone

### Story 6.11: Session-end cleanup ends the player's programs and clears the clipboard, temp files and volume

As the next player at a PC,
I want the PC to be clean when I sit down,
So that nothing the last player left open or copied is still there.

**Covers:** FR130 (clean-up actions), FR136 (failures report to the Hub and badge the tile), NFR12, AD-31, AD-14, UX-DR21 (kiosk cleanup on End), UX-DR34 (volume reset)

**Acceptance Criteria:**

**Given** a session ends by any path that releases the PC (staff End, time-up in clean-up mode, forced end, a later player end)
**When** the kiosk service receives the end
**Then** it immediately runs a cleanup pipeline that ends every process in the player session that is not on the protected list (kiosk, its service, Windows essentials), clears the clipboard and clipboard history, deletes the player account's temp files, sets system volume to the branch default, and returns to attract; the pipeline has named steps so later steps can be added to it

**Given** the session ended into a "Pay at counter" lock
**When** cleanup runs
**Then** it runs before the lock shows, so nothing of the player's remains while the bill is open

**Given** "Staff handles" time-up mode
**When** grace ends
**Then** cleanup does not run until staff End the session

**Given** any cleanup step fails (a process will not exit, a file is locked)
**When** one retry also fails
**Then** the service sends `kiosk.cleanup_failed` naming the step, the Hub raises a "CHECK PC" alert on the tile (Story 3.25), and the kiosk holds a locked "Getting this PC ready — see the counter." screen instead of attract until staff choose "Retry cleanup" or "Checked" in the tile sheet

**Given** the Undo of an End (Story 3.14)
**When** it applies after cleanup ran
**Then** the session record is restored as before, and the player must log in again

**Given** the Hub is unreachable when the kiosk locks locally
**When** cleanup runs
**Then** it runs the same steps and queues its result for the Hub with an idempotency key

**Given** an automated test on Windows 10 and 11 images
**When** a session that left apps open, text on the clipboard, temp files and a changed volume ends
**Then** the next session starts with no player processes, an empty clipboard and history, no leftover temp files and the branch default volume

### Story 6.12: Session-end cleanup signs out launchers, Discord and browsers

As the next player at a PC,
I want every launcher and browser signed out when I sit down,
So that I never see someone else's Steam, Discord or browser account.

**Covers:** FR136, NFR12 (launcher logout), AD-31

**Acceptance Criteria:**

**Given** the cleanup pipeline from Story 6.11
**When** a session ends
**Then** it also signs out Steam, Epic, Riot and Discord by removing their saved login tokens and remembered-account files, resets Chrome, Edge and Firefox profiles to the clean template, and removes the player account's Windows Credential Manager entries, before the PC returns to attract

**Given** a launcher still running at cleanup
**When** its sign-out step starts
**Then** the launcher is shut down first, and its files are removed only after it has exited

**Given** a sign-out step fails after one retry
**When** it is reported
**Then** it uses the Story 6.11 failure path, naming the launcher or browser in `kiosk.cleanup_failed` and on the tile's "CHECK PC" alert

**Given** a launcher or browser that is not installed on the PC
**When** cleanup runs
**Then** its step is skipped without error

**Given** an automated test on Windows 10 and 11 images with real Steam, Epic, Riot, Discord, Chrome and Edge logins made during a session
**When** the session ends and a new player session starts
**Then** every launcher and browser opens signed out and no saved credential is offered

### Story 6.13: The player shell with top bar, pill nav and time chip

As a player,
I want a clear home screen with my time always visible,
So that I can find things fast and always know how long I have.

**Covers:** FR123 (shell, pill nav, time chip, account chip), FR27 (staff start lands in the shell), UX-DR26, UX-DR30, UX-DR31, UX-DR32, UX-DR52 (solid lock variants), UX-DR86 (flow), UX-DR100, UX-DR106 (arrow keys in pill nav), UX-DR115

**Acceptance Criteria:**

**Given** a session starts on the PC, from the kiosk plan picker or by staff from the Hub
**When** `session.started` arrives
**Then** the kiosk opens the player shell (replacing the Story 3.10 minimal screen) within 2 s, for the profiled player or "Guest <n>"

**Given** the shell
**When** it renders
**Then** the top bar sits 32px from the top, 64px tall, with `logo-slot` left, `pill-nav` centred and, at the right, the `time-chip` and the account chip (initial and name), matching the Home mock layout

**Given** the `time-chip`
**When** a session runs
**Then** it shows a status dot and the time in `hud-numeric` tabular figures, counting down for prepaid and up for postpaid, following the Hub ticks; at ≤ 10 min left a 2px warning border and a ⚠ glyph replace the dot

**Given** `pill-nav` with Home · Games · Apps
**When** the player uses it
**Then** exactly one item is active (inverted), arrow keys move between items and Enter selects, Home is active at session start, the selection persists through the session and resets at session end

**Given** a tab with no content yet
**When** it renders
**Then** it shows "No games here yet — ask at the counter."

**Given** the Hub sends a lock reason (paused, locked by staff, Pay at counter, forced end)
**When** the kiosk shows it
**Then** a solid `bg` `lock-screen` with `logo-slot` and the reason's catalogue line covers the shell (forced end: "Your session was ended by staff. Please see the counter."), and it lifts when the Hub clears it

**Given** a Playwright run on reference hardware
**When** staff start a session from a free tile
**Then** the shell is interactive within 2 s of the tap

### Story 6.14: The overlay HUD shows time and balance without stealing focus

As a player in a game,
I want a small always-on-top pill with my time and balance,
So that I can check my time without leaving the game.

**Covers:** FR127, UX-DR49, UX-DR50 (toast component), UX-DR106 (Ctrl+Shift+E), UX-DR108, FR117 (blocked-app toast as `hud-toast`), UX-DR115 (key-kiosk-hud-lock)

**Acceptance Criteria:**

**Given** a running session and a game or app in the foreground
**When** the HUD shows
**Then** a separate always-on-top, non-activating window shows `hud-pill` (solid `bg`, time left or elapsed and the wallet balance in `hud-numeric`; guests show time only), and it never takes keyboard focus or input from the game

**Given** the player presses Ctrl+Shift+E anywhere in the session
**When** the hotkey fires
**Then** the HUD expands into a panel in the `hud-toast` palette with time, balance and the session's plan, focus moves into the panel, and Esc collapses it and returns focus to the previous window

**Given** a fullscreen-exclusive game (detected via the Windows user-notification state)
**When** it is in the foreground
**Then** the pill is hidden and nothing is drawn over the game

**Given** the `hud-toast` component in the HUD window
**When** an info toast is raised
**Then** it shows top-right (~640px, solid `#1E1606`, amber border and title, `text` body, Dismiss) with its sound, never takes focus, and auto-dismisses after 8 s; a warning toast persists until dismissed or time-up

**Given** a blocked app is killed (Story 1.17)
**When** the kiosk tells the player
**Then** "That app isn't allowed on this PC." shows as a persistent warning `hud-toast`

**Given** the HUD on reference hardware
**When** a game runs at full load
**Then** the HUD adds no measurable frame-time spikes above 1 ms per frame in the benchmark and uses no blur or translucency

### Story 6.15: Volume control and calling staff from the top bar

As a player,
I want to set my volume and call staff with one tap,
So that I can get help without shouting across the room.

**Covers:** FR41, FR123 (volume, call staff), UX-DR34, UX-DR36, UX-DR31 (cluster order), UX-DR22, UX-DR107

**Acceptance Criteria:**

**Given** the top bar right cluster
**When** it renders
**Then** it shows, in order, `volume-control`, `call-staff-button` ("?" icon + "Call staff" label), `time-chip` and the account chip, leaving a slot for the messages bell left of Call staff

**Given** the speaker icon
**When** the player opens it
**Then** a horizontal slider (`border-strong` track, `text` fill, 44px thumb hit area, arrow keys step 5%) sets Windows system volume through the service immediately

**Given** a new session starts
**When** the shell opens
**Then** system volume is the branch default from Story 2.25, whatever the previous player set

**Given** the player presses Call staff
**When** `device.call_staff` applies
**Then** the tile shows a "CALL" `alert-badge` with the alert sound (Story 3.25) within 2 s, and the kiosk shows the info toast "Staff notified."

**Given** a call already open for this PC
**When** the player presses Call staff again
**Then** no second alert is raised and the kiosk shows "Staff notified." again

**Given** staff open the tile sheet
**When** the alert is acknowledged
**Then** the CALL badge clears

**Given** the Hub is unreachable
**When** the player presses Call staff
**Then** nothing is sent and the toast reads "Counter is reconnecting — try again shortly."

### Story 6.16: Staff messages reach the seat and stay in the messages bell

As counter staff,
I want to message one PC or every PC from the Hub,
So that I can tell players about closing time or a ready order without walking over.

**Covers:** FR40, FR39 (message moves to the bell), NFR3 (staff message ≤ 2 s), UX-DR35, UX-DR50 (info toast)

**Acceptance Criteria:**

**Given** the tile sheet's Message action (Story 3.5) on an in-session PC
**When** staff send text up to 140 characters
**Then** `device.message{scope: seat}` applies and the kiosk shows it as an info `hud-toast` within 2 s, replacing the Story 3.5 banner

**Given** Floor view
**When** staff choose "Message all PCs" and send text
**Then** `device.message{scope: all}` reaches every PC with an active session within 2 s, and the Hub shows how many PCs received it

**Given** the top bar
**When** it renders
**Then** `messages-bell` sits left of Call staff; unread messages add an `info` dot and count, and the count is part of the button's accessible name

**Given** the bell
**When** the player opens it
**Then** it lists this session's messages newest first with time, and opening marks them read

**Given** the session ends
**When** cleanup runs
**Then** the bell list is cleared, and the next player never sees earlier messages

**Given** a message sent to a PC whose kiosk does not confirm within 5 s
**When** the timeout passes
**Then** the sheet shows "Failed — retry" and Retry resends with the same idempotency key

### Story 6.17: The account menu with Lock and Log out

As a player,
I want one menu with my balance, messages, a lock and log out,
So that I can step away safely or finish when I'm done.

**Covers:** FR123 (account menu), UX-DR37, UX-DR32 (chip opens menu), UX-DR14 (menu shadow), UX-DR115 (key-kiosk-account)

**Acceptance Criteria:**

**Given** the account chip or the `time-chip`
**When** the player activates it
**Then** the `account-menu` opens as a `surface` panel with a header (name, wallet balance, time left) and rows Balance, Messages, Lock and Log out; Esc closes it and returns focus to the chip

**Given** Balance
**When** chosen
**Then** it shows wallet balance and owned pack hours, with the line "Top-ups are accepted at the counter."

**Given** Messages
**When** chosen
**Then** the messages bell list opens

**Given** Lock on a profiled player's session
**When** chosen
**Then** a solid lock screen covers the shell and any game, time keeps running, and it unlocks only with the player's own password (paste allowed; 5 failures lock the field for 60 s) or by staff Unlock from the tile sheet; Lock is not offered to guests

**Given** Log out with prepaid time remaining
**When** chosen
**Then** the menu asks in place "You still have 1h 42m. Log out anyway?" (showing the actual time); on confirm `session.end{actor: player}` (permission `session.self_end`) applies with the Story 3.24 deductions, cleanup runs and the PC returns to attract

**Given** Log out on a postpaid session
**When** chosen
**Then** `session.end{actor: player}` applies, the bill is computed to the minute as in Story 3.11, cleanup runs and the PC locks to "Pay at counter"

**Given** the Hub is unreachable
**When** the player chooses Log out
**Then** nothing is sent and the row shows "Counter is reconnecting — try again shortly."

### Story 6.18: "I'm done" ends a postpaid session from the HUD

As a postpaid player,
I want to end my session myself when I'm finished,
So that I stop paying the moment I stop playing.

**Covers:** FR29, AD-13, UX-DR52 (Pay at counter)

**Acceptance Criteria:**

**Given** a postpaid session
**When** the expanded HUD or the account menu shows
**Then** it offers "I'm done" as a secondary button; prepaid sessions do not show it

**Given** the player chooses "I'm done"
**When** the in-place prompt "End your session? You'll pay at the counter." is confirmed
**Then** `session.end{actor: player}` applies, the bill is computed to the minute as in Story 3.11, cleanup runs, the kiosk locks to "Pay at counter", and the tile shows "UNPAID <amount>" within 2 s

**Given** the confirm prompt
**When** the player presses Esc or Cancel
**Then** nothing changes and the session continues

**Given** an "I'm done" end
**When** the Hub shows it
**Then** the audit records the player as actor, and staff Undo within 8 s works as for a staff End (Story 3.14)

**Given** the Hub is unreachable
**When** the player chooses "I'm done"
**Then** nothing is sent and the panel shows "Counter is reconnecting — try again shortly."

### Story 6.19: Time warnings as HUD toasts, a chip warning, and a taskbar flash in fullscreen games

As a player,
I want clear warnings before my time runs out, even in fullscreen games,
So that I'm never cut off without a chance to extend.

**Covers:** FR128 (time warnings), FR127 (fullscreen-exclusive flash + sound), UX-DR50, UX-DR111 (incl. documented limitation), UX-DR32, UX-DR107, UX-DR103

**Acceptance Criteria:**

**Given** a Story 3.23 warning arrives at 10, 5 or 1 min
**When** the kiosk shows it
**Then** a warning `hud-toast` appears top-right with its sound, the title "10 min left. Extend?" (5 and 1 min likewise), the end time and grace line ("Time's up at 7:23 pm. You get a 2-min grace after that."), Dismiss, and the Ctrl+Shift+E key caps; it never takes focus

**Given** a warning toast
**When** it is not dismissed
**Then** it stays until dismissed or time-up; a newer warning replaces the older one

**Given** a postpaid session approaching branch closing time
**When** the marks pass
**Then** the same warnings show, counting to closing time

**Given** a fullscreen-exclusive game in the foreground at the 5- and 1-min marks
**When** the warning fires
**Then** no overlay is drawn; the kiosk flashes its taskbar button and plays the warning sound

**Given** the shell (not a game) is in the foreground
**When** time left is ≤ 10 min
**Then** the `time-chip` shows its warning border and ⚠ glyph, and warnings also show as toasts over the shell

**Given** the kiosk accessibility notes in the repo docs and help-centre source
**When** this story is done
**Then** they record the accepted limitation: in true fullscreen-exclusive games the 5- and 1-min warnings may be sound-only for deaf and hard-of-hearing players, while the 10-min chip warning, the grace lock and the Ctrl+Shift+E hotkey still apply

**Given** the Hub is unreachable
**When** a mark passes on the kiosk's monotonic countdown
**Then** the kiosk raises the same warning locally

### Story 6.20: Per-branch warning marks and the low-balance warning

As a café owner,
I want to choose when players are warned and when they hear their wallet is low,
So that warnings suit how my café plays.

**Covers:** FR128 (configurable per branch, low balance), UX-DR50, UX-DR101 (low balance), UX-DR103

**Acceptance Criteria:**

**Given** Settings → Branch → Billing (Story 3.1)
**When** a Manager opens it
**Then** it also shows "Warn players at" (up to three marks, each 1–30 min, default 10, 5, 1) and "Low balance warning at" (an amount, default ₹50, or off)

**Given** valid values
**When** saved
**Then** `settings.set` stores them, the Story 3.23 engine uses the new marks for sessions from the next tick, and kiosks receive them via `settings.changed`; duplicate or out-of-range marks show a field error

**Given** a profiled player's session paid from the wallet
**When** the remaining wallet balance (after what the session has used) first falls to or below the threshold
**Then** the kiosk shows a persistent warning `hud-toast` "Balance running low — ₹40 left." (actual amount) with "Top up at the counter" guidance and the Extend key caps, once per crossing

**Given** the balance rises above the threshold (a top-up at the counter)
**When** it later falls again
**Then** the warning can fire again

**Given** the `time-chip` warning
**When** the marks are changed
**Then** the chip still turns to its warning state at ≤ 10 min, independent of the configured marks

### Story 6.21: Extend from the kiosk with live prices

As a prepaid player,
I want to add time from the HUD, the menu or a warning without leaving my game,
So that I can keep playing without walking to the counter.

**Covers:** FR129, FR35 (player extend), FR50 (credit limit), NFR3 (floor ≤ 2 s), UX-DR51, UX-DR19 (`button-warning`), UX-DR101 (Extend refusals), UX-DR103

**Acceptance Criteria:**

**Given** a prepaid session
**When** the player presses Ctrl+Shift+E, Extend on a warning toast, or Extend in the account menu (shown with key caps)
**Then** `extend-options` opens in the expanded HUD with focus on the first option, listing +30 min, +1 h and +2 h with prices quoted live by the Hub at the resolved rate, and the payment sources the player has: wallet (balance), each owned pack valid for the group (hours left), and credit (headroom) if credit is enabled

**Given** an option and source chosen
**When** the player confirms
**Then** `session.extend{actor: player}` (permission `session.self_extend`) applies with the Story 3.28 rules, the kiosk shows "More time added. GG.", the chip and HUD update, any warning toast closes, and the floor tile shows the new time within 2 s

**Given** a refusal (`PAST_CLOSING`, `CREDIT_LIMIT`, `INSUFFICIENT_BALANCE`)
**When** it returns
**Then** the panel stays open with "Can't extend past closing time (11:00 pm).", "Credit limit reached, see counter." or "Balance ₹40. Top up at counter.", and the other options stay usable; there is no manager override on the kiosk

**Given** warning toasts from Story 6.19
**When** they render
**Then** they now show Extend as `button-warning` beside Dismiss

**Given** a postpaid session or a guest
**When** the player looks for Extend
**Then** it is not offered

**Given** the Hub is unreachable
**When** `extend-options` opens
**Then** every option is disabled with "Counter is reconnecting — try again shortly."

### Story 6.22: The grace lock with Extend, then the branch time-up mode at the kiosk

As a prepaid player,
I want a short grace period to finish my round with the chance to extend,
So that the PC never cuts me off mid-fight.

**Covers:** FR130, FR168 (local time-up), AD-12, UX-DR52, UX-DR111, UX-DR113 (milestone announcements)

**Acceptance Criteria:**

**Given** a session enters grace (Story 3.26)
**When** the kiosk shows it
**Then** a topmost overlay shows "Time's up", the grace countdown in `kiosk-display` tabular digits and "Wrap up your round. This PC locks when the timer hits 0:00." over the still-visible game behind the scrim (no blur), with Extend (`button-warning`) and the Ctrl+Shift+E key caps

**Given** the grace overlay
**When** the player keeps playing
**Then** the overlay does not take focus and passes input through to the game everywhere except the Extend button; Ctrl+Shift+E moves focus to `extend-options`

**Given** a successful Extend during grace
**When** it applies
**Then** the overlay disappears and the session continues

**Given** a fullscreen-exclusive game at grace start
**When** grace begins
**Then** no overlay is drawn; the taskbar flashes and the warning sound plays

**Given** grace ends in "Clean up" mode
**When** the session ends
**Then** Story 6.11 cleanup runs and the PC returns to attract

**Given** grace ends in "Staff handles" mode
**When** the session expires
**Then** a solid lock "Time's up — see the counter." holds over the game, the game's processes are suspended until staff act, and staff End runs cleanup while a staff Extend from the Hub resumes them and lifts the lock

**Given** the Hub is unreachable
**When** `lock_after_ms` reaches zero on the monotonic clock
**Then** the kiosk performs the mode's local action (clean up and attract, or the staff-handles lock), writes no state, and re-syncs to the Hub record on reconnect

### Story 6.23: The Games tab shows the library and launches a game in one click

As a player,
I want to see every game on this PC and start one with a click,
So that I'm in the game seconds after sitting down.

**Covers:** FR134 (launch, launch log), FR125, UX-DR40, UX-DR100, UX-DR108, UX-DR115 (key-kiosk-games)

**Acceptance Criteria:**

**Given** the Games tab
**When** it renders
**Then** it shows a grid of `game-tile`s (portrait cover 140×196, radius lg, name bottom-left on `tile-scrim`, `surface` fallback with centred name) for every enabled library game installed on this PC, sorted by name

**Given** a focused tile
**When** the player presses Enter or clicks it
**Then** the kiosk service starts the game from its install path or launch URI, checks it against the library and allow-list first, and the Hub records `game.launched` with player, device, game and time

**Given** a game is running
**When** the shell is visible
**Then** the "game running" signal is on and all background video is stopped entirely until the last launched process exits

**Given** the game is already running
**When** the player activates its tile again
**Then** the existing game window is brought to the front and no second copy starts

**Given** the executable is missing or fails to start
**When** the launch fails
**Then** the player sees "Couldn't start <game> — ask at the counter." as an info toast and the Hub log records the error

**Given** a library with no games on this PC
**When** the tab renders
**Then** it shows "No games here yet — ask at the counter."

### Story 6.24: Filter and search the Games tab

As a player,
I want to filter games by type and search by name,
So that I can find my game in a big library in seconds.

**Covers:** UX-DR44, UX-DR106, UX-DR100

**Acceptance Criteria:**

**Given** the Games tab
**When** it renders
**Then** a `filter-rail` on the left lists "All" plus every category and tag in the library (active item in `text`, others muted), and a search field sits right-aligned in the content title row

**Given** a category chosen
**When** it is selected
**Then** the grid filters instantly with no Apply button; only one category is active at a time

**Given** text typed in search
**When** characters are entered
**Then** the grid filters by name, case-insensitive, combined with the active category

**Given** no game matches
**When** the grid is empty
**Then** it shows an underlined "Clear filters" link that resets to All with empty search

**Given** keyboard use
**When** the player uses Tab and arrow keys
**Then** they move between the rail, search and grid in reading order, with the focus ring on every stop

### Story 6.25: The game booster speeds up launches and shows launch time to staff

As a café owner,
I want PCs to free up resources when a game starts and to see how long launches take,
So that games run smoothly and I can spot a slow PC.

**Covers:** FR119, FR134 (booster at launch), AD-31

**Acceptance Criteria:**

**Given** Settings → Games → Booster on the Hub
**When** a Manager opens it
**Then** it shows a list of background programs to close at launch (empty by default; executable names), game process priority (Normal · Above normal · High, default Above normal) and "Switch to High performance power plan during play" (off by default), stored by `settings.set`

**Given** a game launch (Story 6.23)
**When** the service starts it
**Then** it first closes the listed programs running in the player session, starts the game, sets its priority, and switches the power plan if enabled

**Given** the session ends
**When** cleanup runs
**Then** the original power plan is restored

**Given** a launch
**When** the game's first main window appears
**Then** the service measures time from the click to that window and sends it with the launch record; the tile sheet shows "Last launch: <game> · 14.2 s" and Settings → Games shows each game's median launch time over 7 days per PC

**Given** a game that shows no window within 3 minutes
**When** the timeout passes
**Then** the launch is recorded with no launch time and no error is shown to the player

**Given** a listed program that refuses to close
**When** the booster runs
**Then** the game still launches and the failure is logged

### Story 6.26: Launcher cards open the player's own Steam, Epic or Riot account

As a player,
I want to open Steam, Epic or Riot with my own account,
So that I can play games I own, knowing I'll be signed out when I leave.

**Covers:** UX-DR43, FR136 (logout enforced), FR117 (launchers allowed)

**Acceptance Criteria:**

**Given** the Games tab on a PC where Steam, Epic or Riot is installed (found by Story 6.4 or configured)
**When** it renders
**Then** a row of `launcher-card`s ("Play on your own account") shows each installed launcher's logo, name and "Use your own account"; launchers not installed are not shown

**Given** a launcher card
**When** the player activates it
**Then** the service opens that launcher signed out, on its login screen, and records `launcher.opened` with player and time

**Given** the player signs in to the launcher
**When** the session ends
**Then** Story 6.12 signs it out and removes its saved login

**Given** the launcher is already open
**When** its card is activated
**Then** its window is brought to the front

### Story 6.27: Play hours per game and the "Most played here" carousel

As a player,
I want to see what's popular at this café,
So that I can jump into what everyone else is playing.

**Covers:** FR137, UX-DR42, UX-DR106 (arrow keys)

**Acceptance Criteria:**

**Given** a game launched in a session
**When** its last process exits, or the session ends
**Then** a `game_plays` row (new table: player or guest, device, library entry, started, ended, minutes) is written by a `game.play_recorded` intent, so hours per game per branch are in the change log for later owner reporting

**Given** the Hub
**When** a kiosk asks for "most played"
**Then** it returns the branch's top 10 games by played hours over the last 7 × 24 hours, recomputed at most every 15 minutes, and each player's last-played time per game

**Given** the Games tab with plays in the window
**When** it renders
**Then** a `most-played-carousel` titled "Most played here" with "Top 10 by play hours · last 7 days · <branch>" sits above the grid, each tile ranked "#1"…"#10" (`bg` fill, `text` ink) top-left; activating a tile launches the game; arrow keys scroll

**Given** fewer than one game played in the window
**When** the tab renders
**Then** the carousel is hidden

**Given** a game in the top 10 that is not installed on this PC
**When** the carousel renders
**Then** it is skipped and the next one fills the place

**Given** the Hub is unreachable
**When** the session's plays end
**Then** the records are queued and applied once on reconnect

### Story 6.28: The Apps tab

As a player,
I want the café's apps (Discord, a browser, music) in one place,
So that I can open them as easily as games.

**Covers:** FR123 (Apps), UX-DR40, UX-DR44, UX-DR100

**Acceptance Criteria:**

**Given** library entries of kind app (Story 6.3)
**When** the Apps tab renders
**Then** it shows the same structure as Games — `filter-rail` with the apps' categories, search, and a tile grid — without launcher cards or a carousel

**Given** an app tile
**When** activated
**Then** the app opens through the service with the same allow-list check and is recorded as an app launch (not counted in game hours)

**Given** no apps on this PC
**When** the tab renders
**Then** it shows "No games here yet — ask at the counter."

**Given** an app that keeps saved logins (Discord, a browser)
**When** the session ends
**Then** Story 6.12 cleanup covers it

### Story 6.29: Home shows the featured game with a hero video and a featured row

As a player,
I want the home screen to show what's hot here with one button to play,
So that I can start the café's favourite game straight away.

**Covers:** UX-DR39, UX-DR40 (featured), UX-DR41, UX-DR27, UX-DR15, FR125, UX-DR115 (key-kiosk-home)

**Acceptance Criteria:**

**Given** Home
**When** it renders
**Then** the `hero` shows the branch's most-played game over the last 7 days (Story 6.27), falling back to the first library game with media, with `game-media` under the scrim, eyebrow "Most played at <branch> this week", the title in `kiosk-display`, a meta line (genre · launcher · "You last played 2 days ago", omitted if never played), primary Play now and secondary Game details, with copy inside the left-45% × bottom-50% zone and ≤ 864px wide (≤ 720px at 1366)

**Given** Play now
**When** activated
**Then** the game launches as in Story 6.23 and the hero video stops entirely

**Given** Game details
**When** activated
**Then** a card panel shows genre, age rating and launcher; Esc closes it and returns focus to Game details

**Given** the featured row
**When** it renders
**Then** it shows the featured game as `game-tile-featured` (190×260, 3px magenta outline offset 3px, distinct from the focus ring), then the next most-played installed games, ending with the `all-games-tile` ("All games", grid icon) that opens Games with no filter

**Given** `video-control` bottom-right of the hero
**When** the player pauses
**Then** the hero holds its frame for the rest of the session's Home view

**Given** a library with no games
**When** Home renders
**Then** the hero shows solid `bg` with "No games here yet — ask at the counter." and no Play now

### Story 6.30: The kiosk Shop shows the menu with live stock and popular items

As a player on a Basic or Premium café,
I want to browse food and drinks from my seat,
So that I can see what's available without getting up.

**Covers:** FR131 (menu, live stock, out of stock, bundles), FR69 (kiosk shop), AD-29, UX-DR24 (player surfaces hide gated features), UX-DR30 (Shop on Basic+), UX-DR44, UX-DR45, UX-DR46, UX-DR100, UX-DR115 (key-kiosk-shop)

**Acceptance Criteria:**

**Given** a branch whose licence has the kiosk-orders feature
**When** the shell renders
**Then** `pill-nav` shows Shop after Apps; on a Free branch Shop is absent and the route cannot be reached

**Given** the Shop tab
**When** it renders
**Then** it shows a `filter-rail` of product categories with "All", a "Search food & drinks" field, and `shop-item-card`s (photo from the Hub, name, tabular price, Add) for active products and bundles

**Given** a product the Hub publishes as unavailable on `catalog.changed` (out of stock, expired, inactive), or a bundle with a short component
**When** the card renders
**Then** the photo dims, the name and an "Out of stock" tag stay at full ink, and Add is disabled; the card updates within 2 s of the Hub change

**Given** a bundle
**When** its card renders
**Then** it lists its components

**Given** the Hub
**When** the kiosk asks for popular items
**Then** the top 3 products by units sold over the last 7 days carry a `popular-badge` ("POPULAR")

**Given** no available products
**When** the Shop renders
**Then** it shows "Kitchen's closed right now."

**Given** an order-capable intent from a kiosk on a branch without the feature
**When** it reaches `apply()`
**Then** it is rejected `FEATURE_NOT_IN_TIER`

### Story 6.31: Build a cart and add the order to the session bill from the seat

As a player,
I want to order food and drink onto my session bill,
So that I eat without leaving my game and pay once at the end.

**Covers:** FR131 (cart, attach to session, reaches Hub ≤ 3 s), FR67 (kiosk), FR68 (bundle refusal), NFR3, UX-DR47, UX-DR64 (ORDER badge), UX-DR107 (new kiosk order sound)

**Acceptance Criteria:**

**Given** Add on a card
**When** the player adds an item
**Then** the `cart` opens as a right-side panel ("Your order · To <PC> · <zone>") with ±1 steppers, bundle components indented, and the total in `hud-numeric` with "Inclusive of applicable taxes"; an empty cart shows "Cart's empty."

**Given** a stepper change
**When** the player changes a quantity
**Then** stock is checked live with the Hub and a step past availability is refused with the reason inline

**Given** a running session
**When** the cart shows its payment choice
**Then** it offers "Add to session bill" with the line "Payable at the counter when your session ends", and a primary Place order

**Given** Place order
**When** `order.place{actor: player}` (permission `order.self_place`) applies through the Epic 5 "Attach an order to a running session so it settles with the bill" path
**Then** the lines join the session's open bill, stock follows the branch's take-stock mode, and the cart clears with a confirmation

**Given** any kiosk order
**When** it commits
**Then** the Hub plays the new-order sound and raises an alert badge, and the tile shows the "ORDER" `tile-order-badge` within 3 s, until every line is served

**Given** a stock shortage at placement (`OUT_OF_STOCK`, `BUNDLE_SHORT`)
**When** it is refused
**Then** the cart stays, the short line is marked with the reason, and there is no override on the kiosk

**Given** the Hub is unreachable
**When** the cart renders
**Then** Place order is disabled with "Counter is reconnecting — try again shortly."

### Story 6.32: Pay now from the wallet, Pay at counter, and unpaid order lines at the end of a prepaid session

As a player,
I want to pay for my order from my wallet straight away, or at the counter if my session has ended,
So that every order is paid for one way or another without fuss.

**Covers:** FR131 (wallet, Pay at counter), NFR10, UX-DR47, UX-DR52 (Pay at counter)

**Acceptance Criteria:**

**Given** a profiled player's cart in a running session
**When** the payment choices show
**Then** "Pay now from wallet" with the wallet balance is offered beside "Add to session bill", disabled with the reason inline when the balance is below the total; guests see only "Add to session bill"

**Given** "Pay now from wallet" and Place order
**When** the order applies through the Epic 5 "Sell at the counter — a walk-up sale settles and takes stock" path with a single wallet payment
**Then** the order is settled in one transaction (wallet never below zero), its invoice number is issued as for any unprinted bill, and the order is tied to this PC for delivery

**Given** a concurrent wallet spend that leaves too little balance
**When** the wallet order reaches `apply()`
**Then** it is rejected `INSUFFICIENT_BALANCE`, the cart stays and shows "Balance ₹40. Top up at counter." with the actual balance

**Given** the session ended while the cart was open
**When** the cart shows payment choices
**Then** the only option is "Pay at counter", which places an unpaid order for this PC and player for staff to settle at the POS

**Given** a prepaid session whose bill holds unpaid order lines
**When** the session ends
**Then** the kiosk locks to "Pay at counter" and the tile shows "UNPAID <amount>" for the order lines, while the time is settled from the pack or wallet as before

### Story 6.33: Order status reaches the HUD with delivery or pick-up copy

As a player who ordered,
I want to know when my order is ready and whether it's coming to me,
So that I don't wait at the counter for nothing.

**Covers:** FR131 (status toasts), FR67 ("ordered" on the HUD), FR115 (fulfilment setting), UX-DR50 (info toasts), UX-DR35 (kept in bell), UX-DR103

**Acceptance Criteria:**

**Given** an order placed from the kiosk or added by staff to this session
**When** it commits
**Then** the kiosk shows the info toast "Ordered: <n> items" and the `hud-pill` shows an "Ordered" tag while any line is unserved

**Given** the tile sheet's order section on the Hub
**When** staff press "Mark ready" on an order
**Then** `order.ready` applies and the kiosk shows, within 5 s, "Order's ready — on its way to you." when fulfilment is deliver-to-seat or "Order's ready — grab it at the counter." when it is pick-up

**Given** all lines are served (Mark served, Epic 5)
**When** the last line is served
**Then** the "Ordered" tag clears from the HUD

**Given** an unserved line cancelled at the Hub
**When** the cancellation commits
**Then** the kiosk shows "<item> was cancelled — see the counter."

**Given** any order-status toast
**When** it shows
**Then** it auto-dismisses after 8 s, never takes focus, and is kept in `messages-bell`

**Given** the kiosk reconnects after the Hub was unreachable
**When** it re-syncs
**Then** it shows the latest status of each open order once, without replaying every missed toast

### Story 6.34: The kiosk Account page — details, wallet and packs, order history, change password

As a player,
I want to see my balance, packs and orders and change my password at the PC,
So that I know where I stand without asking staff.

**Covers:** FR126, FR48 (kiosk), FR123 (Account row), UX-DR38, UX-DR30 (source tab stays active), UX-DR18, UX-DR115 (key-kiosk-account)

**Acceptance Criteria:**

**Given** the account menu
**When** the player chooses Account
**Then** the Account page opens with `account-subnav` (Details · Wallet & packs · Order history · Change password) on the left and content on the right, the active item in `text` with a 3px left bar, arrow keys moving between items, and the `pill-nav` tab the player came from still active; guests do not see Account

**Given** Details
**When** shown
**Then** it shows name, username and the phone with all but the last 4 digits masked

**Given** Wallet & packs
**When** shown
**Then** it shows the wallet balance with "Top-ups are accepted at the counter.", each owned pack with valid groups, expiry and hours left, passes with their window, and expired packs muted with "Expired" and unused time ("0h 45m unused")

**Given** Order history
**When** shown
**Then** it lists this player's orders at this branch newest first with date, items, total, status and invoice number

**Given** Change password
**When** the player enters the current password and a new one (min 6 characters)
**Then** `player.password_change{actor: player}` (permission `player.self_update`) stores a new Argon2id hash and shows "Password changed."; a wrong current password shows a field error, and 5 wrong attempts lock the form for 60 s

**Given** the Hub is unreachable
**When** the Account page opens
**Then** it shows the last data received this session, and Change password is disabled with "Counter is reconnecting — try again shortly."

### Story 6.35: PC settings saved to the player's profile

As a regular player,
I want my mouse, display, audio and keyboard settings to follow me to any PC,
So that every seat feels like mine.

**Covers:** FR124, FR123 (PC settings row), UX-DR37 (PC settings panel)

**Acceptance Criteria:**

**Given** Settings → Branch → Kiosk display on the Hub
**When** a Manager edits it
**Then** they set the allowed display modes (resolution + refresh pairs, default: each PC's current mode only)

**Given** PC settings in the account menu
**When** the panel opens
**Then** it shows a mouse sensitivity slider (Windows 1–20), display mode from the allowed list that this PC's monitor supports, audio output device from this PC's devices, and keyboard layout from the layouts installed on this PC

**Given** a change
**When** the player makes it
**Then** the service applies it at once, a display-mode change reverts after 15 s unless the player confirms "Keep this", and for profiled players `player.pc_settings_set` saves it to a new `player_pc_settings` row

**Given** a profiled player with saved settings
**When** their session starts on any PC in the branch
**Then** the saved settings are applied; a setting this PC can't honour (mode not allowed or not supported, device missing) is skipped and the branch default is kept

**Given** guests and players without saved settings
**When** their session starts
**Then** the branch defaults apply, and a guest's changes last only for that session

**Given** the session ends
**When** cleanup runs
**Then** mouse, display, audio output and keyboard layout return to the branch defaults

### Story 6.36: The kiosk keeps the session running while the Hub is unreachable

As a player,
I want my game to keep going if the counter PC drops,
So that a problem at the counter never kicks me out of a match.

**Covers:** FR168, AD-12, AD-14, NFR1, NFR8 (kiosk reconnect), UX-DR33, UX-DR98, UX-DR103

**Acceptance Criteria:**

**Given** the kiosk loses the Hub (no reply within 5 s or socket closed)
**When** the shell or HUD is visible
**Then** the `reconnecting-indicator` ("Reconnecting…" quiet `surface` pill, static icon) shows left of the volume control and on the expanded HUD; nothing else locks or interrupts the game

**Given** a running session
**When** the Hub stays unreachable
**Then** the time chip and HUD keep counting from the last tick on the monotonic clock (`lock_after_ms`), a wall-clock change on the PC has no effect, warnings fire at the marks, and at zero the kiosk performs the branch's time-up action locally (Story 6.22)

**Given** the Hub is unreachable
**When** the player tries login, sign-up, a plan request, Extend, Place order, I'm done, Log out, Call staff, change password or saving PC settings
**Then** each is disabled with "Counter is reconnecting — try again shortly.", while launching games and apps, volume and Lock keep working

**Given** events made offline (launches, play records, cleanup results, scan reports)
**When** the Hub returns
**Then** they replay in creation order and apply once by idempotency key

**Given** the Hub returns (or a promoted standby takes over)
**When** the kiosk reconnects with jittered backoff (2–30 s) and re-syncs
**Then** the Hub's record wins: the chip and HUD snap to the Hub's time, a session the Hub ended meanwhile locks with its reason, and the indicator disappears only after this reconcile

**Given** an integration test with the Hub process stopped for 30 minutes during a prepaid session
**When** it runs
**Then** the player is never locked out before `lock_after_ms`, the kiosk locks within 1 s of zero, writes no session state, and after restart the Hub and kiosk agree on the session

### Story 6.37: Kiosk screens reflow at every supported width, by keyboard and in high contrast

As a player on any café monitor,
I want the kiosk to fit my screen and work with just a keyboard,
So that it looks right on every seat and I never need a mouse to get around.

**Covers:** UX-DR86, UX-DR106, UX-DR4, UX-DR17, UX-DR109, UX-DR110, NFR15, NFR16 (+30% strings), NFR26

**Acceptance Criteria:**

**Given** the shell at 1366 px wide
**When** Home and Games render
**Then** the featured row shows 4 tiles + All games, the grid has 5 columns and the hero copy is ≤ 720px; at 1920: 6 + All games, 7 columns, ≤ 864px; at 2560: 8 + All games, 9 columns; at 21:9 ultrawide: 9 columns with content capped at 2560px centred and the hero anchored to the left 45%; the layout reflows and is never scaled, with 56px margins

**Given** a Playwright visual suite at the four widths
**When** it runs on Home, Games, Apps, Shop with cart, Account, login, sign-up, plan picker, HUD, warning toast and grace lock
**Then** nothing clips, overlaps or scrolls horizontally, including with a +30% pseudo-locale

**Given** keyboard only
**When** a tester goes from login through plan picker, Home, Games, launch, Shop order, Account and Log out
**Then** every step works with Tab/Shift+Tab, Enter, Esc and arrow keys in pill nav, sub-nav and carousels, plus Ctrl+Shift+E; no other kiosk shortcut exists, and the focus ring is visible at every stop

**Given** HC is on for the kiosk (Story 1.19)
**When** every player screen renders
**Then** media, scrims, shadows and translucency are gone, every interactive surface has a 2px white outline, and the axe and contrast checks pass with zero serious or critical violations

**Given** any kiosk or HUD stylesheet
**When** CI lints it
**Then** `backdrop-filter`, `filter: blur` and autoplaying unmuted media fail the build

### Story 6.38: The player journey proven on reference hardware

As a café owner,
I want the whole player experience measured on a cheap PC before release,
So that I can trust it on my real floor.

**Covers:** NFR2 (Kiosk < 80 MB), NFR3, NFR6 (kiosk login < 3 s), NFR12 (launcher logout), FR168, FR131 (≤ 3 s), FR129 (≤ 2 s)

**Acceptance Criteria:**

**Given** the reference kiosk PC and a Hub on the LAN in CI's hardware lab
**When** the scripted Journey 3 runs (attract → login → plan picker → Home → launch → warning → Extend → grace → clean-up → attract)
**Then** login completes in < 3 s, the shell opens < 2 s after approval, Extend reaches the floor ≤ 2 s and the PC returns to attract after cleanup, with each timing recorded in the release report

**Given** the same rig
**When** an order is placed from the kiosk
**Then** it appears on the Hub tile ≤ 3 s

**Given** the kiosk service and app idle on attract and idle in the shell
**When** the per-release benchmark measures them
**Then** they use < 80 MB RAM together, excluding the running game, and the job fails the release otherwise

**Given** the launcher-logout suite on Windows 10 and 11 with real accounts
**When** 20 consecutive sessions sign in to Steam, Epic, Riot, Discord, Chrome and Edge and end by each end path
**Then** no later session sees a previous login or saved credential, and any residual fails the release

**Given** the Hub is stopped for 30 minutes mid-session on the rig
**When** the test runs
**Then** play is not interrupted, the lock lands on time, and everything reconciles on restart

## Epic 7: Kitchen tickets and display

Kitchen staff get KOTs and a LAN display that survives LAN loss, and order status reaches the seat.

> Draft notes: this epic builds on the Epic 1 LAN envelope and class scope table (Story 1.7: `paired`, `kds`, `phone`), idempotent intents and `base_rev` (Story 1.6), the Kitchen display SvelteKit `adapter-static` skeleton (Story 1.1), the entitlement API `has_flag()` (Story 2.11), per-device HC (Story 1.19), the printer adapter and paper width (Story 2.26), the fulfilment setting (Story 2.25), the pairing rate limiter (Story 2.18) and the alert service (Story 3.25). From Epic 5 it uses, by title: "Products with categories, barcodes, images and the kitchen flag", "Sell at the counter — a walk-up sale settles and takes stock", "Attach an order to a running session so it settles with the bill", "Take stock at served, mark items served, and cancel unserved lines into waste" (`order.line_served`, cancel with "Was it already made?") and "Choose print or none at settle, and print through the side-effect outbox" (the `side_effect_outbox`, Reprint, "Printer offline" alert). From Epic 6 it uses, by title: "Place an order from the seat to the session bill or the wallet" (kiosk `order.place`, new-order sound, ORDER badge) and "Order status reaches the HUD with delivery or pick-up copy" (the Hub "Mark ready" action applying `order.ready`, the delivered vs pick-up HUD copy, the cancelled-line toast). **Ticket model (decided here):** each placement that contains kitchen-flagged lines opens one kitchen ticket holding only those lines; the ticket's stage (New → Preparing → Ready → Served) is the single source for the Hub sheet, the KOT, the display and the READY badge; reaching Ready applies the Epic 6 `order.ready` effects for that ticket, and reaching Served applies `order.line_served` to its lines. **Kitchen display transport (decided):** the Hub's `:7401` listener serves both TLS (paired Tauri apps) and plain HTTP by path — `/kitchen` for the board (and `/console` for the phone page in Epic 12) — each plain-HTTP path accepting only its own scoped token; the spine amendment to AD-42 / Story 1.7 is logged separately. The board uses a scoped code-paired token (AD-42 class `kds`) whose scope is the board feed, `kitchen.advance` and `kitchen.advance.reverted` only — it can never cancel. **Seat only:** tickets on the board and on the KOT identify the order by seat label (PC/TV number or "Counter") and ticket number; no player name appears on either. **No cancel slip:** a counter cancellation prints nothing; the board greys the ticket out only. **Known limit:** plain HTTP rules out a service worker, so a tablet reloaded during LAN loss cannot load the board until the LAN returns; its IndexedDB queue survives and replays when the LAN is back (Story 7.10). **Out of this epic:** prep stations and item-level timing (FR102, Epic 14), owner-side kitchen reports (Epic 8), Bluetooth KOT on the Android Hub beyond the existing adapter interface (Epic 4 plugin already plugs in). Every story adds its new intents and their `.reverted` twins to the Story 1.6 `proptest` generator.

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
**Then** the Story 2.26 "Find printer" flow runs for a second ESC/POS printer with its own paper width (58 or 80 mm) and "Test print", stored as the branch KOT printer; the receipt printer is unchanged

**Given** a Free branch (no `kitchen` flag)
**When** Settings → Kitchen opens
**Then** it shows `tier-lock` — lock glyph, "Available on Basic" and an underlined "Upgrade" link to Owner app Subscription — and no editable fields
**And** every kitchen intent added in this epic is rejected by `apply()` in the permission step with `TIER_LOCKED`

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

### Story 7.5: A KOT prints within 3 s and can be reprinted from the Hub

As kitchen staff,
I want a printed ticket for every kitchen order within seconds,
So that I can start cooking even when I'm not looking at a screen.

**Covers:** FR97 (KOT ≤ 3 s, reprint), FR100 (printer offline alert, display still receives), NFR3, NFR18, AD-28, AD-34, UX-DR95, UX-DR101, UX-DR107

**Acceptance Criteria:**

**Given** the `kot` renderer (pure function from a kitchen ticket and settings to ESC/POS commands)
**When** it renders
**Then** it prints in Font A: "KOT" title, seat label double-width/height, ticket number, branch-local time, then each line as "qty × name" with each note indented below, a `-` rule, no prices and no player name; at 80 mm within 48 characters and at 58 mm within 32 with long names wrapped

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
**Then** the same KOT prints marked "REPRINT" with the reprint time, and the audit records who reprinted it

**Given** a ticket cancelled at the counter
**When** the cancellation commits
**Then** no KOT or cancel slip is printed

**Given** golden tests
**When** fixture tickets render at both widths (one line, many lines, long names, several notes, "Counter", "TV 3")
**Then** the byte output matches the stored golden files and no line exceeds the width

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
**Then** the field clears with "That code didn't work. Get a new one from the counter."; 5 failures invalidate the code and more than 20 attempts per IP per hour return `RATE_LIMITED` (Story 2.18 limiter)

**Given** 4 non-revoked displays
**When** a fifth pairing is attempted
**Then** it is refused with "This branch already has 4 kitchen displays — remove one at the counter."; kitchen displays never count toward the device cap

**Given** a paired display in the list
**When** a Manager presses Revoke
**Then** its socket closes with `TOKEN_REVOKED`, the page clears its token and returns to the pairing screen

**Given** a Free branch
**When** the address is opened
**Then** the page shows "Kitchen display is available on Basic. Ask the owner to upgrade." and pairing returns `TIER_LOCKED`

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

**Given** the display set to high contrast from its Hub list entry (per device, Story 1.19)
**When** the board renders
**Then** it uses the `-hc` tokens with 2px outlines and the yellow focus ring

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

## Epic 8: Owner sees and controls branches remotely

Owners get timely dashboards, reports, alerts, live view, audit search and exactly-once commands, and player and owner notifications go out from the cloud.

> Draft notes: this epic builds on the Epic 1 `apply()`, change log, typed actors and audit; the Epic 2 cloud (`arena-cloud` roles, `gz_control`, owner sign-in with TOTP, backup codes and SMS fallback in Stories 2.3–2.5), heartbeat (Story 2.10), licence and entitlement API (Story 2.11), `branch_fence` row (Story 2.10) and the **minimal setup-command mailbox** of Story 2.14. Epic 8 extends that mailbox. It adds the owner command kinds, `branch: all` fan-out, step-up MFA, the sweeper, epoch fencing of fetch and results, and the owner-side command UI. **No continuous cloud mirror:** the cloud never holds a queryable copy of branch rows. Owner figures come only from the Hub-computed `state/latest.json` (every `summary_interval_s`) and `rollups/<date>.json` (daily at close), and audit comes only from the on-demand builder. **Cadence:** `upload_interval_s` = `summary_interval_s`: Basic 900 s, Premium 300 s, floor 30 s. Free uploads no batches, summaries or rollups (Story 4.14), and its Owner app shows only Subscription, pairing and support. **Paid tiers before Epic 10:** Basic, Premium and the Tunnel add-on come from the Story 2.7 operator-issued comp entitlement (tier + `tunnel` flag), so every story here can be tested end to end before billing exists. **Snapshots:** this epic produces the daily snapshot because the audit builder needs one. Tier retention, the upgrade-first snapshot, the yearly GST archive, the player-key backup and the Hub header backup status are Epic 9 (FR174, FR175). **Outbox:** Story 5.19 already writes `notify_outbox` rows for WhatsApp receipts. Epic 8 adds the drain, the cloud send path and the other message kinds. **Rejection copy:** the Owner app shows only two rejection reasons: "Device/Hub revoked", and "Couldn't apply at branch" + Retry for every other `AppError`. The exact code stays in the command detail for support. **Rollup schema v1** (Story 8.5) is signed off as drafted; later fields arrive by a schema bump (the cloud accepts N and N−1, Story 8.7). **Summary `catalogue`** (Story 8.4) is confirmed: configuration only, with a per-entity `rev` for `base_rev`, and no transactions or player data. Each story lists what it covers on its **Covers** line.

### Story 8.1: Presign issues batch and summary upload URLs under the branch's own R2 prefix

As a café owner on a paid tier,
I want my branch's data to go only to my branch's storage area, through short-lived upload links,
So that nobody else's Hub can write over my data and nothing in the cloud holds a standing key to it.

**Covers:** FR167 (upload path), AD-4 (presign), AD-9, AD-22, AD-23, AD-36 (prefix layout), NFR14, NFR20 (per-tenant PUT cost guard), NFR23, NFR25; Additional Requirements "Build spikes" (aws-sdk-s3 against R2)

**Acceptance Criteria:**

**Given** the `arena-cloud` control role with R2 credentials from SOPS-managed secrets (MinIO in dev)
**When** the build spike test runs
**Then** aws-sdk-s3 issues a presigned PUT against R2 with `request_checksum_calculation = WhenRequired` and region `auto`, and a PUT to that URL succeeds; the test runs in CI against MinIO and on staging against R2

**Given** a paid Hub with a valid device token
**When** it calls `POST /v1/presign {epoch, batch: {first, last}, summary: true}`
**Then** the cloud returns `{batch_url, summary_url}` in one response, for keys `tenants/<t>/branches/<b>/changes/<epoch>/<first:012>-<last:012>.zst` and `tenants/<t>/branches/<b>/state/latest.json`, each valid for 10 minutes
**And** `tenant_id` and `branch_id` come only from the device token, never from the request body

**Given** a presign request with `kind: rollup` (with `date`) or `kind: snapshot` (with `date`)
**When** the cloud handles it
**Then** it returns a single URL for `rollups/<yyyy-mm-dd>.json` or `snapshots/<yyyy-mm-dd>.db.zst` under the same prefix; any other kind returns `UNSUPPORTED`

**Given** a requested key that would fall outside the caller's branch prefix (path traversal, another branch id)
**When** presign is called
**Then** it is refused `FORBIDDEN` and an audit event is recorded with the device id

**Given** an epoch below `branch_fence`
**When** presign is called
**Then** summary, rollup and snapshot URLs are refused `FENCED`; batch URLs are still issued so the old epoch's rows can be stored for reconciliation

**Given** a Free-tier Hub
**When** it calls presign
**Then** it is refused `FEATURE_NOT_ENTITLED` and nothing is issued

**Given** a tenant whose presign issuance exceeds its per-tenant limit in `rate_limits` (default 4× its tier cadence per branch per hour)
**When** another presign arrives
**Then** it is refused `RATE_LIMITED` with a retry-after, and the per-tenant R2 PUT count metric records the refusal for the cost guard

**Given** the control role's metrics
**When** presigns are issued
**Then** R2 PUTs per tenant are exported to Grafana Cloud with tenant and branch labels and no other identifiers

### Story 8.2: The Hub uploads gap-free change-log batches at its tier cadence

As a café owner on a paid tier,
I want every change at my branch shipped to the cloud in order, without gaps, while the café keeps running,
So that my reports and audit can always be rebuilt from what the Hub recorded.

**Covers:** FR167 (batches), AD-4 (batch format, cadence, fork declaration), AD-10 (cadence from licence), AD-36, NFR1, NFR9, NFR20

**Acceptance Criteria:**

**Given** a paid Hub whose licence carries `upload_interval_s` (Basic 900, Premium 300)
**When** the uploader task runs
**Then** it wakes every `upload_interval_s` (never more often than 30 s) or as soon as 500 rows are pending, whichever is first, and never runs inside or blocks an `apply()` transaction

**Given** pending `changes` rows above `upload_cursor.last_seq_uploaded`
**When** a cycle runs
**Then** it reads up to 5,000 rows in `seq` order, builds a msgpack header `{epoch, fork?, first, last, sha256, schema}` plus full-row maps keyed by column name, compresses it with zstd, presigns (Story 8.1), PUTs it to `batch_url`, and only then advances `last_seq_uploaded` to `last` in a new `upload_cursor` table

**Given** more than 5,000 pending rows (for example after an 8-hour outage)
**When** the uploader catches up
**Then** it ships consecutive batches back to back until the backlog is empty, with no gap or overlap between `first` and `last` of successive keys

**Given** the first batch after the Hub's epoch changed
**When** it is built
**Then** the header declares `fork = {parent_epoch, parent_seq}` and `first = parent_seq + 1`

**Given** a PUT that fails or times out
**When** the cycle ends
**Then** the cursor is not advanced, the same key is retried next cycle with jittered backoff, and a repeated PUT of the same key is harmless

**Given** the WAN is down
**When** cycles fail
**Then** rows accumulate locally, the Hub keeps trading, `chip-offline` is the only staff-visible change, and Hub `GET /healthz` reports last upload time and pending row count

**Given** the licence tier is Free, or changes to Free
**When** the uploader would run
**Then** it does nothing and makes no presign call (Story 4.14 behaviour is unchanged)

**Given** a batch read back from R2 in an integration test
**When** its sha256 and row count are checked
**Then** they match the header, and replaying it with `arena-replay` into an empty database reproduces those rows

### Story 8.3: The heartbeat detects upload gaps and the Hub resends them

As a café owner on a paid tier,
I want the cloud to notice when a batch went missing and ask my Hub for it again,
So that a lost upload never leaves a hole in my history.

**Covers:** FR167 (gap detection and resend), AD-4 (heartbeat gap check, no manifest), NFR1, NFR25 (heartbeat gaps metric)

**Acceptance Criteria:**

**Given** a paid Hub's heartbeat
**When** it is sent
**Then** the body adds `last_seq_uploaded`, `epoch`, `last_summary_at` and `last_rollup_date` to the Story 2.10 fields

**Given** the cloud's cached view per branch (`hubs.last_seq_seen`, `last_epoch_seen`)
**When** a heartbeat's `last_seq_uploaded` and epoch agree with it
**Then** no R2 listing is made

**Given** a heartbeat that disagrees with the cached view
**When** the cloud handles it
**Then** it lists the branch's `changes/<epoch>/` prefix, finds the first missing seq range after the fork point, and adds `resend{from, to}` to the response; with no gap it updates the cached view and adds nothing

**Given** a heartbeat response carrying `resend{from, to}`
**When** the uploader next runs
**Then** it re-ships that range first as batches with keys for exactly those seqs, then continues from its cursor

**Given** Hub `changes` rows in the resend range that were already pruned
**When** the resend is attempted
**Then** the Hub reports `resend_unavailable{from, to}` in the next heartbeat, the cloud records it on the branch, and the gap is counted in the "heartbeat gaps" metric instead of being requested again

**Given** the fleet
**When** gaps are found
**Then** the "heartbeat gaps" metric per branch is exported, and no `/uploads/manifest` endpoint, `upload_manifest` table or ingest cursor exists anywhere in the codebase (checked by a CI grep)

### Story 8.4: The Hub publishes a branch summary every cadence

As a café owner on a paid tier,
I want my Hub to publish a short summary of right now (open, sessions, revenue, cash, staff, what needs attention),
So that my phone can show today's picture in under two seconds without reaching into my branch.

**Covers:** FR148 (summary data), FR167 (summary), AD-35 (`summary.publish`, `rollup_log`), AD-33, NFR5 (summary < 50 ms), NFR7

**Acceptance Criteria:**

**Given** a paid Hub with `summary_interval_s` from its licence
**When** the summary engine runs each interval (in the same cycle as the uploader, sharing its presign)
**Then** it builds `state/latest.json` from in-memory floor state plus 6 queries over stamped columns and stored invoice totals, with `{schema, branch_id, as_of, business_day, open, sessions_live, revenue_so_far_minor, currency, cash_in_drawer_minor, shifts: [{shift_id, staff_name, opened_at, variance_minor?}], staff_on_shift: [names], attention: [{kind, ref, text_params}], catalogue}` and PUTs it to `summary_url`

**Given** the `catalogue` section
**When** it is built
**Then** it carries the owner-editable state with each row's `rev`: groups, rates, plans, pricing rules, coupons, products and bundles, staff (name, role, active; no credentials), roles and devices (name, group, kind, revoked), so the Owner app can edit with `base_rev`; it holds configuration only, never transactions or player data

**Given** a published summary
**When** the PUT succeeds
**Then** a system intent `summary.publish` is applied that writes only the `rollup_log` row with `date = 'latest'` (a new `rollup_log(date PK, schema, built_at, key)` table) and appears in the audit; no business row changes

**Given** the attention list
**When** it is built
**Then** it includes open shift variance over threshold, low-stock products (Story 5.8), players at 15+ days overdue (Story 3.19) and the Hub's open header alerts, each by id and display parameters, never a player name, phone or email

**Given** the summary engine
**When** it runs on the reference PC with 60 devices and a full business day of rows
**Then** computing the file takes under 50 ms (CI benchmark), and a concurrent burst of intents shows no added `apply()` latency

**Given** the WAN is down
**When** an interval passes
**Then** no summary is published or queued; the next successful cycle publishes a fresh one (summaries are not backfilled)

**Given** a Free Hub
**When** the interval would pass
**Then** no summary is built

### Story 8.5: Rollup schema v1 and the daily rollup at close

As a café owner on a paid tier,
I want my Hub to compute each business day's totals into one fixed, versioned file shape, tested against its own Z-reports,
So that my reports are the Hub's own numbers, not a cloud recalculation.

**Covers:** FR167 (rollups), FR150 (data source), AD-35, AD-7, AD-26, NFR5 (rollup < 5 s), NFR10 (totals match Z-report); Additional Requirements "Rollup schema v1 field list" (signed off)

**Acceptance Criteria:**

**Given** `arena-proto`
**When** rollup schema v1 is defined
**Then** it is a versioned Rust type (generated to TypeScript) with `{schema: 1, branch_id, business_day, currency, revenue: {total, by_type: {play, products, plans, other}, by_method: {cash, upi, card, wallet, credit}}, refunds, discounts, voids, sessions: {count, minutes}, shifts: [{shift_id, staff_name, opened_at, closed_at, float, expected, counted, variance, reason?, approver_name?}], stock: {levels: [{product_id, name, qty, low_qty?}], waste: [{product_id, qty, value, reason}], expiring: [{product_id, lot, qty, expires_on}]}, credit: {outstanding, aged: {d0_7, d8_14, d15p}, players: [{player_id, username, outstanding, oldest_unpaid_on, consent}]}, play_hours: [{game_id, minutes}]}` in integer minor units

**Given** the rollup engine in `arena-core`
**When** it is called for a `business_day`
**Then** it runs about 20 queries over stamped columns and stored invoice totals and returns the v1 document, without writing any row or making any network call

**Given** golden tests with a fixed SQLite fixture (sessions, POS, refunds, undone sales, shifts with variance, waste, credit spends and collections)
**When** the rollup is built
**Then** the JSON equals the expected file exactly, and revenue, payments and cash totals equal the fixture's Z-reports (Story 5.27) for the same day

**Given** the reference PC and a busy day (1,000 sessions, 500 orders)
**When** the rollup runs
**Then** it completes in under 5 s; the Story 4.17 device-farm job adds the same check on a 3 GB Android device

### Story 8.6: The daily rollup publishes at close and catches up missed days

As a café owner on a paid tier,
I want my Hub to publish each business day's rollup soon after close, and fill in days it missed,
So that my reports have every day without me doing anything.

**Covers:** FR167 (rollups after close), AD-35 (`rollup.publish`, `rollup_log`), AD-4 (presign `kind: rollup`), NFR1

**Acceptance Criteria:**

**Given** the branch's closing time from opening hours (Story 2.25)
**When** closing time + 30 minutes passes, or the last open shift of the business day closes, whichever is first
**Then** the rollup engine runs its queries for that `business_day`, writes `rollups/<yyyy-mm-dd>.json` via presign `kind: rollup`, and applies `rollup.publish`, which upserts `rollup_log(date, schema, built_at, key)` and appears in the audit

**Given** a 24-hour branch with no closing time
**When** the business day rolls at `day_start_min`
**Then** the previous day's rollup runs 30 minutes after the roll

**Given** the Hub was off or offline at the trigger time
**When** it next runs with WAN available
**Then** every past business day with no `rollup_log` row (since the branch went paid) is generated and uploaded, oldest first

**Given** a late intent stamped to an already-rolled business day (for example an Undo inside its window just after close)
**When** it commits
**Then** that day's rollup is regenerated and re-uploaded, overwriting the file

### Story 8.7: Rollups regenerate after a schema bump, and the cloud accepts only the current and previous schema

As a café owner on a paid tier,
I want old days to be rebuilt in the new format after an update,
So that new report fields show for past days too, without a cloud release.

**Covers:** AD-35 (N and N−1, regeneration oldest first, max 10/min), AD-8, AD-10 (`rollup_schema` pinned per tier), NFR25 (rollup missing-day count)

**Acceptance Criteria:**

**Given** a Hub release whose current rollup schema is N and a licence whose `rollup_schema` is N
**When** the Hub starts
**Then** it finds `rollup_log` days with `schema < N` and regenerates them oldest first, at most 10 per minute, without blocking `apply()` or the normal upload cycle

**Given** a licence pinning `rollup_schema` below the Hub's current schema
**When** a rollup is built
**Then** the Hub emits the pinned schema (it keeps writers for N and N−1), so a tier's file shape changes only when the cloud raises the pin

**Given** the cloud reading a rollup file
**When** its `schema` is N or N−1
**Then** it is accepted; any other value makes the day count as missing with `ROLLUP_SCHEMA_OLD` recorded for that branch and day

**Given** a rollup day that is missing or `ROLLUP_SCHEMA_OLD`
**When** the jobs role runs its daily metrics pass
**Then** the "rollup missing-day count" metric per branch is exported

**Given** a regeneration interrupted by a restart
**When** the Hub starts again
**Then** it resumes from the oldest day still below N, and days already rebuilt are not rebuilt again

### Story 8.8: The Hub uploads a daily snapshot after close

As a café owner on a paid tier,
I want a full copy of my branch database stored each night,
So that my audit history can be rebuilt quickly from a recent starting point.

**Covers:** AD-4 (daily snapshot), AD-24 (PII ciphertext only), AD-36, NFR13

**Acceptance Criteria:**

**Given** a paid Hub after the day's rollup has published
**When** the snapshot task runs
**Then** it runs `VACUUM INTO` a temp file, compresses it with zstd, uploads it to `snapshots/<yyyy-mm-dd>.db.zst` via presign `kind: snapshot`, deletes the temp file, and records the date and last included seq in a new `snapshot_log` table

**Given** the snapshot file
**When** it is inspected in a test
**Then** player PII columns hold only ciphertext, and the player key table is not in it

**Given** the WAN is down at close
**When** the snapshot would run
**Then** it is retried on the next cycle with WAN, and only the latest missed day is uploaded (older missed days are skipped)

**Given** the snapshot is running
**When** staff keep trading
**Then** `apply()` latency stays within NFR3 on the reference PC

**Given** low free disk space (below twice the database size)
**When** the snapshot would run
**Then** it is skipped, a "Not enough disk space for backup" alert appears in the Hub header list, and an audit row is written

### Story 8.9: The owner read API serves the live summary and rollup ranges from files

As a café owner on a paid tier,
I want my app to get today's numbers and any date range quickly, only for my own branches,
So that the dashboard is fast and no one can see another café's figures.

**Covers:** FR149 (as_of), FR150 (ranges), AD-35 (owner read API, 30 s cache, ETag), AD-9, AD-23, AD-40, NFR7, NFR14, NFR23

**Acceptance Criteria:**

**Given** a signed-in owner and one of their paid branches
**When** the app calls `GET /v1/branches/{id}/live`
**Then** the cloud reads `state/latest.json`, returns it with `as_of`, `branch_online` (heartbeat within the last 3 minutes) and `last_heartbeat_at`, and serves repeated calls within 30 s from an in-memory cache checked against the R2 ETag

**Given** `GET /v1/branches/{id}/summary?from=&to=&group_by=day|week|month`
**When** it is called
**Then** the cloud loads the `rollups/` files for that range (per branch, month cache with ETag), sums them in memory, and returns totals plus per-period rows, with `missing_days` listing any day without an accepted rollup (`ROLLUP_SCHEMA_OLD` or absent)

**Given** `branch=all` on either endpoint
**When** it is called
**Then** the response has one entry per permitted branch plus a total, and branches in different currencies are never summed together (one total per currency)

**Given** a branch id not in the session's permitted branches
**When** any owner read endpoint is called
**Then** it returns `NOT_FOUND`, and the R2 key is always built from the session's tenant and branch, never from request input

**Given** a Free branch
**When** `live` or `summary` is called
**Then** it returns `FEATURE_NOT_ENTITLED`

**Given** a tenant above its per-tenant Owner API limit
**When** more calls arrive
**Then** they return `RATE_LIMITED` with a retry-after

**Given** a branch with no summary file yet
**When** `live` is called
**Then** it returns `{as_of: null}` and the app shows "Waiting for the first update from this branch."

### Story 8.10: The Owner app shell — navigation, themes, install and Free limits

As a café owner,
I want one app on my phone and laptop with the same five places to go, in light or dark,
So that I always know where things are, and on Free I see only what my plan includes.

**Covers:** AD-15, AD-40, NFR15, NFR16, UX-DR5, UX-DR6, UX-DR11, UX-DR13, UX-DR17, UX-DR20, UX-DR24, UX-DR82, UX-DR90, UX-DR98 (Owner PWA no network), UX-DR113, UX-DR114, UX-DR115 (key-owner-today), UX-DR116

**Acceptance Criteria:**

**Given** the Owner PWA (SvelteKit, `adapter-cloudflare`, Workers static assets) at `app.arena360.app`
**When** a signed-in owner opens it on a phone (< 768 px)
**Then** `owner-nav` is a 64 px bottom tab bar Today · Reports · Alerts · Manage · Account (icon + `hub-micro`, `border-strong` top edge), Today is home, and the layout is one column with a 16 px gutter

**Given** a desktop width (≥ 1024 px)
**When** the app opens
**Then** `owner-nav` is a 256 px left sidebar (icon + `hub-label`, active item filled) with content up to about 1200 px and a 40 px gutter, with full feature parity with the phone

**Given** the theme
**When** the app opens
**Then** it follows `prefers-color-scheme` live, Account → Theme offers `theme-toggle` System · Light · Dark remembered per device, light uses the "Cool paper" tokens, and both themes pass the token contrast check and an axe scan with zero serious/critical violations

**Given** Account
**When** it opens
**Then** it holds MFA (the Story 2.4–2.5 security screens), Subscription, Branches (Story 2.29) and Theme

**Given** a supported browser
**When** the owner installs the app
**Then** a web manifest and service worker make it installable to the home screen, and the service worker caches the app shell for offline open

**Given** any screen that loads data
**When** a response arrives
**Then** the payload is stored in IndexedDB with its `as_of`, the next open renders the cached payload at once with its age, and logout clears the cache

**Given** no network
**When** the owner opens the app
**Then** it shows cached screens with their data age and a "You're offline" `status-badge`, never a blank or broken page

**Given** a screen reader
**When** any route opens
**Then** it has `header`, `nav` and `main` landmarks, one `h1`, labelled controls, the active nav item exposed as current, and all strings come from the message catalogue

### Story 8.11: On Free, the Owner app shows only Subscription, pairing and support

As a café owner on Free,
I want the Owner app to show only what my plan includes, and say plainly what upgrading unlocks,
So that I'm never shown empty dashboards or broken screens.

**Covers:** FR152, AD-29 (owner-side gates in the cloud API), UX-DR24, UX-DR90 (Free shows only Subscription, pairing, support)

**Acceptance Criteria:**

**Given** a tenant whose branches are all Free
**When** the app opens
**Then** only Account → Subscription, pairing (Branches) and Support are usable; Today, Reports, Alerts and Manage show `tier-lock` "Available on Basic" with Upgrade leading to Subscription, and no owner read API is called

**Given** a mixed tenant (one Free branch, one paid)
**When** Today, Reports or Manage open
**Then** the Free branch appears with `tier-lock` in place of its figures and controls, and the paid branch works normally

**Given** a Free branch
**When** any owner read or command endpoint other than Subscription, pairing and support is called for it
**Then** the cloud returns `FEATURE_NOT_ENTITLED`, so the gate holds even if the UI is bypassed

**Given** a Free branch that upgrades
**When** its next heartbeat carries the paid licence
**Then** the locked areas unlock on the next app open without signing out

### Story 8.12: Today shows every branch with its figures, cash variance and data age

As a café owner,
I want to open my phone and see each branch's state, money and staff right now, with how old each number is,
So that I know at a glance whether everything is fine, even when a branch has gone offline.

**Covers:** FR148, FR149, NFR7, UX-DR81, UX-DR90 (Today), UX-DR98, UX-DR100 (Needs attention empty), UX-DR103, UX-DR115 (key-owner-today), UX-DR116

**Acceptance Criteria:**

**Given** Today for a tenant with paid branches
**When** it opens
**Then** each branch is a `card` (56 px apart) with `metric-figure`s: open/closed, sessions live, revenue so far, cash in drawer with the signed per-shift variance beside it (counted − expected, "+ over" / "− short"), staff on shift and last sync, each with a `hub-micro` data age ("as of 2 min ago"); on desktop the figures sit in a two-column grid

**Given** a shift variance beyond the branch threshold
**When** the card renders
**Then** the variance shows a warning edge and the label "Over threshold" (text, not colour alone)

**Given** the app is visible
**When** the tier cadence passes (Basic 15 min, Premium 5 min)
**Then** Today re-fetches `live` for each branch without a full reload and without moving focus, and pull-to-refresh (phone) or a Refresh button fetches at once

**Given** a figure's data age beyond 30 minutes during the branch's opening hours
**When** it renders
**Then** the age turns to warning ink

**Given** a branch whose heartbeat is older than 3 minutes
**When** Today renders
**Then** its card drops figures to muted ink, shows the "Branch offline" `status-badge`, keeps the last values with a growing data age, and is never shown as an error

**Given** "Needs attention"
**When** Today renders
**Then** it lists the `attention` items of every branch's summary, newest first, each naming the branch, and shows "All quiet." when there are none

**Given** a 3-year-old mid-range phone on a throttled 4G profile in a Lighthouse CI run
**When** Today opens with two branches and a warm service worker
**Then** first render of figures is under 2 s

**Given** the "data age within cadence" target
**When** Today opens
**Then** the app reports each open's data age to the cloud (branch id, age in seconds, tier; no other data), and the "summary age per branch" metric and a 99th-percentile dashboard exist in Grafana

### Story 8.13: Reports — revenue and cash reconciliation

As a café owner,
I want revenue by day, week, month or custom range, per branch, type and payment method, and every shift's cash count,
So that I can see what each branch earns and spot cash problems shift by shift.

**Covers:** FR150 (revenue, cash reconciliation), NFR7, NFR10, UX-DR25, UX-DR81, UX-DR90 (Reports)

**Acceptance Criteria:**

**Given** Reports → Revenue
**When** it opens
**Then** it shows a range picker (Today · This week · This month · Custom), a branch picker (one or All branches) and totals by type (play, products, plans, other) and by payment method (cash, UPI, card, wallet, credit), with refunds as negative lines and discounts listed, all from `summary?range=`

**Given** a range that includes today
**When** it renders
**Then** today's row comes from the live summary and is labelled "So far today" with its data age; past days come from rollups

**Given** days in the range without an accepted rollup
**When** it renders
**Then** a muted line says "No data yet for 2 days in this range" with the dates, and totals are labelled as partial

**Given** Reports → Cash reconciliation
**When** it opens
**Then** it lists shifts in the range with staff, opened/closed time, float, expected, counted and signed variance, reason and approver; variances over the branch threshold are flagged with a warning edge and "Over threshold"

**Given** a multi-currency tenant
**When** All branches is chosen
**Then** totals are shown per currency, never mixed

### Story 8.14: Reports — stock

As a café owner,
I want stock levels, low stock, waste and expiring items per branch and in total,
So that I can reorder and see where stock is lost without visiting.

**Covers:** FR150 (stock), UX-DR81, UX-DR90 (Reports)

**Acceptance Criteria:**

**Given** Reports → Stock
**When** it opens for one branch
**Then** it shows current levels from the latest rollup (with its date) and the live summary's low-stock attention items, a Low stock list (qty ≤ threshold), Waste in the range by product and reason with value, and Expiring lots in the next 7 days

**Given** All branches
**When** chosen
**Then** levels and waste are listed per branch and as a total per product matched by product name and category, with branches that lack the product shown as "—"

**Given** a branch with stock tracking unused (no tracked products)
**When** the report opens
**Then** it shows "No tracked products at this branch."

### Story 8.15: Owner reports export to CSV and PDF and match the Hub's Z-reports exactly

As a café owner,
I want to download any report and trust that it equals the branch's own Z-reports,
So that I can hand it to my accountant without reconciling two sets of numbers.

**Covers:** FR150 (exact match, CSV/PDF), NFR10 (sum of rollups = Z-report over 30 synthetic days), UX-DR90 (Exports)

**Acceptance Criteria:**

**Given** Revenue, Cash reconciliation or Stock with a range and branch chosen
**When** the owner chooses Export → CSV
**Then** the cloud streams a CSV built from the same rollups (one row per day per branch, amounts in major units with the currency code), with a header row naming the range and branches

**Given** Export → PDF
**When** chosen
**Then** a print-styled view of the report opens and the browser's print-to-PDF produces it with branch, range, generated time and data age in the header

**Given** Reports → Exports
**When** it opens
**Then** it lists the exports available for the current range in one place

**Given** a CI job that generates 30 synthetic business days at a Hub (sessions, POS, refunds, undos, shifts) and uploads its rollups to MinIO
**When** the owner read API sums the 30 days
**Then** revenue by type, payments by method, refunds and cash expected/counted/variance equal the sum of the Hub's 30 days of Z-reports to the minor unit; any difference fails the build

### Story 8.16: The cloud accepts owner command kinds, checks them, and fans "all branches" out

As a café owner,
I want only valid changes I'm entitled to send to be accepted, for one branch or all of them at once,
So that I can't send something a branch can't apply.

**Covers:** FR154, FR155 (only valid targets), FR156 (one key per change), AD-5 (catalogue, `group_id` fan-out, `schema_version`), AD-8 (`BRANCH_UPDATE_REQUIRED`), AD-14, AD-23, AD-29, NFR23

**Acceptance Criteria:**

**Given** the command catalogue in `arena-proto`
**When** this story ships
**Then** it adds the owner kinds `rate.upsert`, `plan.upsert`, `rule.upsert`, `coupon.upsert`, `product.upsert` (products and bundles), `staff.upsert`, `staff.deactivate`, `role.upsert`, `device.revoke`, `credit.remind`, `backup.now` and `settings.set` (owner alert thresholds), each with a payload schema, `schema_version`, required tier/flag and a `sensitive` flag (true for `staff.upsert`, `staff.deactivate`, `role.upsert`, `device.revoke` and the reserved `hub.revoke` and `backup.restore`)
**And** the Story 2.14 setup kinds keep working unchanged

**Given** `POST /v1/commands {branch_id | "all", idempotency_key, kind, payload, base_rev?}` from a signed-in owner
**When** the cloud handles it
**Then** it validates the payload, checks the branch belongs to the session's tenant and is entitled for the kind (`FEATURE_NOT_ENTITLED` otherwise), and inserts a `commands` row with the relay-assigned `id`, `issued_by` and `status = pending`

**Given** `branch: "all"`
**When** it is accepted
**Then** one row per permitted, entitled branch is inserted, each with `group_id` = the user's idempotency key and a per-branch key derived from it; the response lists every created row; branches not entitled are listed as skipped with their reason

**Given** the same `(branch_id, idempotency_key)` posted twice
**When** the second arrives
**Then** the existing row is returned and nothing new is inserted

**Given** a branch whose last heartbeat reported a supported command `schema_version` range below the kind's version
**When** the command is posted
**Then** it is refused `BRANCH_UPDATE_REQUIRED` for that branch only

**Given** a tenant above its per-tenant command-post limit
**When** more posts arrive
**Then** they are refused `RATE_LIMITED`

**Given** `GET /v1/commands?status=&branch=&group=`
**When** the owner app calls it
**Then** it returns the tenant's commands with state, error code, time queued and `done_at`, newest first, paged

### Story 8.17: The Hub applies owner commands exactly once, and the owner's change wins

As a café owner,
I want my change to land once at the branch and take precedence over an edit made there a moment before,
So that the branch ends up exactly as I set it, and staff can see what changed.

**Covers:** FR156, FR157 (≤ 1 min on floor and attract), AD-5 (owner wins), AD-39 (`base_rev`, overwritten change listed), AD-2, AD-30, NFR3, UX-DR102 (command conflict toast), UX-DR103

**Acceptance Criteria:**

**Given** a fetched owner command of any kind from Story 8.16
**When** the poller applies it through `apply(command.apply{c})`
**Then** the inner intent runs with `actor{kind: owner, id}`, the key, `applied_seq` and epoch are recorded in `applied_commands` in the same transaction, and the result is posted with epoch and `applied_seq`

**Given** a command whose `base_rev` is lower than the row's current `rev` because staff edited it at the Hub
**When** it applies
**Then** it applies anyway, the `updated_by_owner` event names the overwritten change (its intent id, actor and time), the audit keeps both changes, and the Hub shows a toast naming the entity: "Price updated by owner", "Plan updated by owner", "Pricing rule updated by owner", "Coupon updated by owner", "Product updated by owner", "Staff member updated by owner", "Role updated by owner"

**Given** a command whose inner intent fails validation (for example a plan for a deleted group)
**When** it is applied
**Then** the Hub posts `rejected` with the `AppError` code, nothing changes, and the command is not retried by the Hub

**Given** an applied `rate.upsert`, `rule.upsert` or `plan.upsert`
**When** it commits
**Then** `catalog.changed` / `settings.changed` reach floor tiles, the Hub header `chip-rule` and kiosk attract screens, and an integration test shows the new rate on both within 1 minute of apply

**Given** `backup.now`
**When** it applies
**Then** the uploader and summary engine run a cycle at once (Stories 8.2, 8.4), and the command is reported applied when that cycle's uploads succeed or `rejected` with the transport error

**Given** the `proptest` generator
**When** this story ships
**Then** it includes owner commands interleaved with staff edits of the same rows, and the invariant holds that the final row equals the last applied owner command where one followed the staff edit

### Story 8.18: Commands are swept, expire after 7 days, and are fenced by epoch

As a café owner,
I want a command that got stuck to be retried, one that waited a week to expire visibly, and a replaced Hub never to apply my changes,
So that every change ends in a clear state and only the real main Hub applies it.

**Covers:** FR155 (Expired after 7 days), FR156, AD-5 (sweeper, `FENCED` re-queue, fence move), AD-11, AD-38, NFR25 (command backlog)

**Acceptance Criteria:**

**Given** the jobs role sweeper, running every minute
**When** it finds `fetched` commands older than 10 minutes with no result
**Then** it returns them to `pending` so the next fetch delivers them again; the Hub's `applied_commands` dedupe makes a second delivery report `applied` without re-applying

**Given** `pending` commands older than 7 days
**When** the sweeper runs
**Then** they become `expired` with `COMMAND_EXPIRED`, and a later fetch never returns them

**Given** a fetch `GET /v1/commands?after=&epoch=` with an epoch below `branch_fence`
**When** the command role handles it
**Then** it returns `FENCED` with the current fence and no commands, and the Hub self-locks per Story 2.10 behaviour

**Given** a result post whose epoch is below `branch_fence`
**When** it arrives
**Then** it is rejected `FENCED` and the command is re-queued to `pending`

**Given** `branch_fence` moves to a new epoch with `fork_seq`
**When** the control role commits the move
**Then** commands applied at the old epoch with `applied_seq > fork_seq` return to `pending`

**Given** re-queued commands whose `id` is below the Hub's cursor
**When** the next fetch runs
**Then** the relay returns every `pending` row for the branch in `id` order, regardless of the cursor, so re-queued rows are never skipped

**Given** a test that moves the fence with a fixture CAS during a burst of 100 commands
**When** the new-epoch Hub fetches
**Then** every command ends applied exactly once at the new epoch, and none is lost or doubled

**Given** the command role metrics
**When** commands wait
**Then** "command backlog" (pending + fetched per branch, oldest age) is exported

### Story 8.19: Preview the target branches and follow each command row to Applied

As a café owner,
I want to see which branches a change will reach and whether they're online before I send it, then watch each one apply,
So that I know exactly where my change has landed.

**Covers:** FR155, FR156, UX-DR23 (owner command states), UX-DR83, UX-DR101 (apply errors), UX-DR103 (command rejected copy), UX-DR113 (polite live region), UX-DR115 (key-owner-price-change), UX-DR116

**Acceptance Criteria:**

**Given** any Manage edit about to be sent
**When** the owner chooses Send
**Then** a preview lists each target branch (one, or "All branches" expanded) with an online/offline `status-badge` from its heartbeat, marks branches that can't take the change as not offered with the reason inline (not entitled, needs a Hub update), and offers Send and Cancel

**Given** a sent change
**When** the cloud accepts it
**Then** Manage shows one `command-row` per target branch: what changed, scope (branch or "All branches"), `status-badge` and time queued, grouped under Today / Earlier

**Given** a row's server state
**When** it renders
**Then** `pending` and `fetched` show "Waiting for branch · 2 min" (info), `applied` shows "Applied · 14:32" (ok), `rejected` shows "Device/Hub revoked" when the code is `TOKEN_REVOKED` and otherwise "Couldn't apply at branch" with Retry (danger), and `expired` shows "Expired" with Resend (muted)

**Given** Retry on a rejected row or Resend on an expired one
**When** pressed
**Then** a new command with a new idempotency key and the current `base_rev` is sent for that branch only, and the old row stays in history

**Given** the command list is open
**When** a row changes state
**Then** it updates within 10 s (polling while visible) and the change is announced once in a polite live region

**Given** an offline branch's card on Today
**When** it has pending commands
**Then** they are listed on the card as "Waiting for branch"

### Story 8.20: Commands queue on the owner's device while it is offline

As a café owner on a bad connection,
I want changes I make with no signal to wait on my phone and send themselves when I'm back online,
So that I don't lose a change or send it twice.

**Covers:** FR155 (Queued on this device), AD-14 (client ULID key, TTL ≤ 7 days, creation order), UX-DR83, UX-DR98, UX-DR103 (queued offline)

**Acceptance Criteria:**

**Given** no network
**When** the owner sends a change
**Then** the command (with a client ULID `idempotency_key`, `base_rev` and `created_at`) is stored in IndexedDB and its rows show "Queued on this device" (muted), and the preview shows target branch statuses as last known with their age

**Given** queued commands
**When** the network returns (or the app is reopened online)
**Then** they are posted in creation order, each row moves to "Waiting for branch" once accepted, and a retry after a lost response returns the same server row (no duplicate)

**Given** a queued command older than 7 days
**When** the app next runs
**Then** it is not sent and shows "Expired" with Resend

**Given** a command refused by the cloud on send (for example `FEATURE_NOT_ENTITLED` or `BRANCH_UPDATE_REQUIRED`)
**When** the queue drains
**Then** that row shows "Couldn't apply at branch" with Retry and the rest of the queue continues

**Given** logout
**When** it completes
**Then** queued commands are kept only if the owner chooses "Keep queued changes"; otherwise they are deleted with the cache

### Story 8.21: Manage → Pricing — plans and rates, pricing rules and coupons

As a café owner,
I want to change rates, plans, pricing rules and coupons for one branch or all of them from my phone,
So that I can run a price change without going to the café.

**Covers:** FR154 (prices, plans, rules), FR157, UX-DR25 (`tab-control`), UX-DR76 (rule rows), UX-DR83, UX-DR90 (Manage → Pricing), UX-DR115 (key-owner-price-change), UX-DR116

**Acceptance Criteria:**

**Given** Manage → Pricing
**When** it opens
**Then** a `tab-control` offers Plans & rates · Pricing rules · Coupons (selected tab in the URL), filled from each branch's summary `catalogue` with its data age

**Given** Plans & rates
**When** the owner edits a group's hourly rate or a plan (name, minutes, price, validity, groups)
**Then** the form validates like the Hub (same field rules and error copy), and Send opens the Story 8.19 preview with `rate.upsert` or `plan.upsert` and the row's `base_rev`; no MFA prompt is shown

**Given** Pricing rules
**When** the list renders
**Then** each rule row shows name, schedule (weekdays and hours, or dates), branches and status (active now, scheduled, ended), and create/edit sends `rule.upsert`

**Given** Coupons
**When** the owner creates or edits one
**Then** `coupon.upsert` is sent with the same fields as the Hub (Story 5.21)

**Given** "All branches" for an edit of an item that exists by name in some branches only
**When** the preview opens
**Then** branches where the item doesn't exist are offered "Create here" or excluded, and the owner chooses before sending

**Given** an applied rate change at an online branch
**When** Journey 6 is run end to end in staging
**Then** the row reads Applied and the new rate shows on the floor view and kiosk attract within 1 minute of apply, with command delivery under 2 minutes

### Story 8.22: Manage → Products and bundles

As a café owner,
I want to add or change products and bundles for my branches remotely,
So that the menu and prices are right everywhere without calling the counter.

**Covers:** FR154 (products/bundles), UX-DR83, UX-DR90 (Manage → Products)

**Acceptance Criteria:**

**Given** Manage → Products
**When** it opens
**Then** it lists products and bundles per branch from the summary `catalogue` with category, price, tax rate, active flag and kitchen flag, searchable by name or barcode

**Given** a product edit (name, category, price, tax rate, HSN, barcode, active, kitchen flag, low-stock threshold)
**When** sent
**Then** `product.upsert` goes through the preview with `base_rev`, with no MFA prompt, and stock quantities are never editable from the Owner app

**Given** a bundle edit
**When** the owner sets components and the bundle's own price
**Then** components must exist at each target branch, otherwise that branch is not offered with the reason inline

**Given** a product image
**When** the owner attaches one
**Then** it is resized to at most 512 px and 200 KB in the browser before it is sent in the payload

### Story 8.23: Step-up MFA for sensitive commands

As a café owner,
I want a fresh authenticator check only when I change staff, roles or revoke a device,
So that a stolen session can't lock out my staff, and I'm not nagged for everyday price changes.

**Covers:** FR3, AD-10 (step-up), AD-30 (`elevated_by.kind = mfa`), NFR11, NFR15 (WCAG 3.3.8), UX-DR83 (MFA only for sensitive kinds), UX-DR112

**Acceptance Criteria:**

**Given** a command whose kind is flagged `sensitive`
**When** it is posted without a step-up proof younger than 5 minutes
**Then** the cloud refuses it `MFA_REQUIRED`, and the app asks for a current TOTP code (or a backup code, or an SMS code when the owner enabled fallback) in place, then re-sends the same command with the same key

**Given** a correct code
**When** the step-up succeeds
**Then** the session holds a step-up proof for 5 minutes, and the command row records `elevated_by{kind: mfa, id: owner_id}`, which the Hub copies into the change row when it applies

**Given** a non-sensitive kind (prices, plans, rules, coupons, products, credit reminder, backup now, alert thresholds)
**When** sent
**Then** no MFA prompt appears, even with no recent step-up

**Given** a sensitive command queued on the device while offline (Story 8.20)
**When** the queue drains
**Then** it is held as "Queued on this device" until the owner passes step-up MFA in the app; it is never sent without it

**Given** 5 wrong step-up codes in a row
**When** the next is entered
**Then** step-up pauses for 15 minutes with the time shown, and the sign-in session itself is not ended

**Given** the step-up dialog
**When** used with a keyboard and screen reader
**Then** focus moves into it, paste is allowed, the error never says which factor failed, and Esc cancels without sending

### Story 8.24: Manage → Staff users and roles by command

As a café owner,
I want to add, change and deactivate staff and edit roles for one branch or all,
So that I can manage my team remotely without ever knowing their PIN or password.

**Covers:** FR154 (staff users/roles, one branch or all), FR3, AD-10 (credentials never travel), AD-30, UX-DR90 (Staff users & roles), UX-DR102 (setup code display), UX-DR103

**Acceptance Criteria:**

**Given** Manage → Staff users & roles
**When** it opens
**Then** it lists staff per branch from the summary `catalogue` (name, phone, role, active) and the tenant's roles with their permissions from the fixed catalogue

**Given** a new staff member (name, phone, role) for one branch or All branches
**When** the owner sends it and passes step-up MFA
**Then** the cloud generates one 8-character setup code per target branch, sends `staff.upsert` carrying only `setup_code_hash` and `setup_expires_at = now + 72 h` (as in Story 2.15), and shows once "Setup code for Neha: K7Q4 9MZD. Give it to her; it works once." with Copy, one per branch

**Given** a role change or `staff.deactivate`
**When** sent with step-up MFA
**Then** the Hub applies it, a deactivated person's Hub sign-in ends at once, and the Owner cannot deactivate the last Owner (rejected, shown as "Couldn't apply at branch")

**Given** a custom role edit
**When** sent
**Then** `role.upsert` carries permissions only from the `arena-proto` catalogue; unknown permissions are refused by the cloud before sending

**Given** a Free branch at its 3-user limit
**When** the owner adds a user there
**Then** that branch is not offered in the preview and shows `tier-lock` "Available on Basic"

### Story 8.25: Manage → Devices and groups, and revoking a device

As a café owner,
I want to see my branches' devices and groups and revoke a lost or replaced device from my phone,
So that a stolen PC or kiosk can never connect again.

**Covers:** FR154 (revoke device), FR3, AD-10 (revocation within one heartbeat, immediately on LAN), AD-29, NFR11, UX-DR7, UX-DR83, UX-DR90 (Devices & groups)

**Acceptance Criteria:**

**Given** Manage → Devices & groups
**When** it opens
**Then** it lists each branch's groups (name and swatch dot) and devices (name, kind, group, revoked) from the summary `catalogue`, and group edits send `group.upsert` / `device.assign` (non-sensitive)

**Given** "Revoke device" on a kiosk or Pi
**When** the owner confirms and passes step-up MFA
**Then** `device.revoke` is sent; at the Hub it marks the token revoked, closes the device's socket at once with `TOKEN_REVOKED`, frees the device slot, and is audited with `elevated_by{kind: mfa}`

**Given** a command for a branch whose Hub token has been revoked in the cloud
**When** it is pending
**Then** the cloud marks it `rejected` with `TOKEN_REVOKED`, shown as "Device/Hub revoked"

**Given** a revoke for a device that is already revoked
**When** applied
**Then** it reports `applied` and changes nothing

> Draft notes: revoking the Hub itself (`hub.revoke`) is reserved in the catalogue with its sensitive flag but delivered with standby and Hub replacement in Epic 9.

### Story 8.26: The cloud sends player and owner messages through a swappable messaging adapter, exactly once

As a café owner,
I want messages to my players sent from Arena 360's own WhatsApp account, once, with SMS if WhatsApp fails,
So that I need no provider account or keys, and no player gets the same message twice.

**Covers:** FR187 (cloud send, exactly once, versioned templates, no Free sends), AD-27 (send ledger, recipient hash only), AD-34 (adapter, recorded fake, manual mode), NFR13, NFR23, UX-DR96

**Acceptance Criteria:**

**Given** `POST /v1/notify {outbox_id, kind, purpose, consent_policy_version, recipient, params}` from a paid Hub's device token
**When** the cloud handles it
**Then** it inserts a `send_ledger` row unique on `outbox_id` with `recipient_hash` (salted SHA-256), template version, channel and state, picks the current approved template version for `kind`, and sends through the messaging adapter

**Given** the same `outbox_id` posted again
**When** the cloud handles it
**Then** no second send happens and the original result is returned

**Given** the adapter selected by config (Gupshup at launch)
**When** a WhatsApp send fails with a non-retryable delivery error
**Then** the same message is sent by SMS, and the ledger records both attempts and the final channel

**Given** a Free branch
**When** it posts to `/notify`
**Then** it is refused `FEATURE_NOT_ENTITLED` and nothing is sent

**Given** templates for credit reminder, low balance, session ending, console 5-minute alert and receipt
**When** they are stored
**Then** each has a version, the approved WhatsApp template id and an SMS text, and changing a template creates a new version without changing the ledger rows of earlier sends

**Given** dev and CI
**When** the adapter is the recorded fake
**Then** sends are captured for assertions and no provider is called; a "manual" mode logs the rendered message for operators instead of sending

**Given** the cloud logs and traces
**When** a send happens
**Then** they carry the outbox id and branch id only, never the phone number or message text

### Story 8.27: The Hub drains its notification outbox after the undo window

As a player,
I want my receipt and other messages sent only once the action can no longer be undone, and only if I still agree to messages,
So that I never get a message for something that was taken back.

**Covers:** FR187 (never for an undone action, consent re-check, queued while offline), FR72 (WhatsApp receipt sent), AD-27, AD-28, AD-13 (notification sent is irreversible), AD-24, NFR13

**Acceptance Criteria:**

**Given** `notify_outbox` rows in state `queued` whose `not_before` has passed
**When** the drain task runs (every 10 s on a paid Hub)
**Then** for each row it re-checks the player's current consent and erasure, skips (state `skipped`) rows whose forward intent was reverted, decrypts the recipient, and posts to `/v1/notify` keyed by the outbox row ULID

**Given** a cloud reply
**When** it arrives
**Then** the Hub applies `notify.sent` (with channel) or `notify.failed` (with code), and "notification sent" is registered with the irreversible-effect guard so the forward action can no longer be undone

**Given** a receipt row (Story 5.19) whose order has no invoice number yet
**When** the drain looks at it
**Then** it waits until the number exists

**Given** the WAN is down
**When** rows are due
**Then** they stay `queued` and are sent in `not_before` order once the cloud is reachable; rows older than 7 days are marked `expired` instead of sent

**Given** a player erased or whose consent was withdrawn after the row was queued
**When** the drain reaches it
**Then** it is marked `skipped` and nothing is posted

**Given** a lost reply after the cloud sent the message
**When** the drain retries
**Then** the send ledger returns the original result and no second message is sent

### Story 8.28: Low-balance, session-ending and console 5-minute messages

As a player who agreed to service messages,
I want a WhatsApp when my wallet runs low, when my session is about to end, and 5 minutes before my console time is up,
So that I can top up or extend before I'm cut off.

**Covers:** FR187 (low balance, session ending, console 5-min alert), FR114 (per-message on/off), AD-27, UX-DR96

**Acceptance Criteria:**

**Given** a paid branch with "Low balance" on in Settings → Notifications (Story 5.15) and a profiled player with current consent
**When** the Story 6.20 low-balance crossing fires
**Then** a `notify.requested` (kind low_balance, amount) is written once per crossing

**Given** "Session ending" on and a PC session with an end time for a consenting profiled player
**When** remaining time reaches 5 minutes
**Then** one `notify.requested` (kind session_ending, minutes, device name) is written for that session; it is not repeated after an Extend

**Given** a console session for a consenting profiled player
**When** remaining time reaches 5 minutes
**Then** one `notify.requested` (kind console_5min, TV name) is written for that session

**Given** a guest, a player without consent, a Free branch or the message turned off
**When** the trigger fires
**Then** no outbox row is written

**Given** the session is ended or extended within the undo window of the triggering moment
**When** the drain runs
**Then** a session_ending or console_5min row for a session no longer near its end is skipped

### Story 8.29: Credit reminders go out on cycle days, and Free shows an overdue list

As a café owner,
I want players who owe credit reminded automatically on the days I set, only if they agreed,
So that credit gets collected without my staff chasing anyone.

**Covers:** FR52, FR49 (cycle days used), FR187 (credit reminder), AD-27, UX-DR88 (Players), UX-DR96

**Acceptance Criteria:**

**Given** a paid branch with "Credit reminder" on and a consenting player with outstanding credit and a reminder cycle (default 3, 7, 14 days)
**When** the daily credit check runs at 11:00 branch time
**Then** for each cycle day equal to the number of days since the player's oldest unpaid spend (FIFO, Story 3.19), one `notify.requested` (kind credit_reminder, outstanding amount, branch name) is written

**Given** a player not on a cycle day today
**When** the check runs
**Then** nothing is written for them

**Given** a player without consent, a guest balance, or "Credit reminder" off
**When** the check runs
**Then** no reminder is written, and the player stays on the Hub's overdue list

**Given** the Hub was off at 11:00
**When** it starts later the same business day
**Then** the check runs once for that day; missed earlier days are not sent

**Given** a Free branch
**When** Players → Overdue opens on the Hub
**Then** it lists players with outstanding credit by oldest unpaid date with their aging buckets and a "Reminders are available on Basic" `tier-lock` line, and no reminder is ever written

### Story 8.30: Reports — credit, with a one-tap reminder

As a café owner,
I want to see who owes what at each branch, by age, and send a reminder with one tap,
So that I can follow up on overdue credit myself.

**Covers:** FR150 (credit, tap to send reminder), FR154 (send credit reminder), FR52, UX-DR81, UX-DR83, UX-DR90 (Reports → Credit)

**Acceptance Criteria:**

**Given** Reports → Credit
**When** it opens
**Then** it shows outstanding per branch aged 0–7, 8–14 and 15+ days from the latest rollup (with its date), and a list of players by player id and username with outstanding and oldest unpaid date, never phone, name or email

**Given** a player row with consent recorded
**When** the owner taps "Send reminder"
**Then** a `credit.remind{player_id}` command is sent (no MFA) and the row shows its `command-row` state

**Given** the Hub applies `credit.remind`
**When** the player has consent and outstanding credit
**Then** it writes a `notify.requested` (kind credit_reminder) and reports applied; otherwise it reports rejected, shown as "Couldn't apply at branch"

**Given** a player without consent
**When** the row renders
**Then** "Send reminder" is not offered and the row says "No consent for messages"

**Given** a reminder already sent to that player today (automatic or manual)
**When** the owner taps again
**Then** the app warns "Reminder already sent today" and asks to confirm before sending

### Story 8.31: Owner alerts by web push and WhatsApp, and the Alerts tab

As a café owner,
I want alerts on my phone the moment something needs me, with WhatsApp as the dependable channel,
So that I don't miss a problem even when the app is closed.

**Covers:** FR153 (channels, Alerts list), AD-27 (VAPID web push, WhatsApp primary), UX-DR96 (owner alerts), UX-DR100 ("All quiet."), UX-DR113

**Acceptance Criteria:**

**Given** Alerts → Notification settings
**When** the owner turns on push on a supported browser
**Then** the app requests permission, subscribes with the cloud's VAPID public key, and stores the subscription in `push_subscriptions`; on iOS not installed to the Home Screen it shows "Add Arena 360 to your Home Screen to get alerts here." and does not ask

**Given** an owner alert raised for a branch (a test fixture source in this story)
**When** the cloud handles it
**Then** it writes an `owner_alerts` row (new table: id, tenant, branch, kind, display parameters, created_at; kept 90 days), sends web push to each subscription, and sends WhatsApp to the owner's phone through the messaging adapter, each exactly once per alert id

**Given** a push subscription the push service reports gone (404/410)
**When** a send fails that way
**Then** the subscription is deleted

**Given** a tapped push notification
**When** the app opens
**Then** it deep-links to the branch card or report the alert names

**Given** the Alerts tab
**When** it opens
**Then** it lists `owner_alerts` newest first with branch, kind, plain text in the staff register and time, and shows "All quiet." when empty; a new alert is announced once in a polite live region

**Given** Alerts → Notification settings
**When** the owner turns WhatsApp alerts off
**Then** only push and the Alerts list are used (on by default)

### Story 8.32: The Hub raises owner alerts for cash variance, low stock, overdue credit and large refunds or discounts

As a café owner,
I want to be told when a shift's cash is off, stock runs low, credit goes 15+ days overdue, or a big refund or discount is given,
So that I can act the same day.

**Covers:** FR153 (Hub-sourced alerts, per-tenant thresholds), AD-27, AD-5 (`settings.set` thresholds), UX-DR96

**Acceptance Criteria:**

**Given** Alerts → Thresholds in the Owner app
**When** the owner sets cash variance (default: the branch threshold), large refund (default ₹500) and large discount (default ₹200 or 20%)
**Then** a non-sensitive `settings.set` command is sent to all branches and each Hub stores them as `owner_alert_thresholds`

**Given** a `shift.closed` with |variance| over the threshold
**When** it commits
**Then** a `notify.requested` (purpose owner_alert, kind cash_variance, branch, staff name, amount, reason) is written with `not_before` = commit

**Given** a `stock.low` crossing (Story 5.8), a player reaching 15+ days overdue (Story 3.19), or a refund or discount over its threshold
**When** it commits
**Then** a `notify.requested` owner_alert of the matching kind is written once per crossing, with `not_before` = commit + the action's undo window

**Given** a refund or discount undone inside its window
**When** the drain runs
**Then** its owner alert is skipped

**Given** the drain posts an owner_alert row
**When** the cloud receives it
**Then** it creates the alert of Story 8.31 for the tenant's owners, deduped by outbox id, with no player personal data (player ids only)

### Story 8.33: The cloud alerts the owner when a branch goes offline during opening hours

As a café owner,
I want to know when a branch has been unreachable for half an hour while it should be open,
So that I can call the branch before customers notice.

**Covers:** FR153 (branch offline), FR149 (offline state), AD-27, NFR25; Spine §Operational Envelope "Tenant alerts"

**Acceptance Criteria:**

**Given** the heartbeat
**When** a paid Hub sends it
**Then** it carries the branch's opening hours and timezone (from Story 2.25 settings) and the cloud caches them on the branch

**Given** the jobs role, every 5 minutes
**When** a paid branch has had no heartbeat for 30 minutes and the current time is inside its opening hours
**Then** one "branch offline" owner alert (Story 8.31) is raised for that outage

**Given** a branch that was alerted offline
**When** its heartbeat returns
**Then** a "branch back online" entry is added to the Alerts list (push only, no WhatsApp) and the next outage can alert again

**Given** a branch offline outside opening hours
**When** opening time arrives and it is still offline
**Then** the alert is raised 30 minutes after opening time

**Given** many branches missing heartbeats at once (for example a cloud-side fault)
**When** more than 20% of paid branches would alert in one run
**Then** owner alerts are held and the on-call is paged instead

### Story 8.34: The audit builder rebuilds a branch month on demand

As a café owner or support agent,
I want a month of a branch's history rebuilt in the cloud only when I ask for it,
So that I can search what happened without the cloud keeping a standing copy of my data.

**Covers:** FR158 (build), AD-37, AD-4 (replay honouring fork points), AD-21, AD-23, AD-24, AD-36 (audit-cache 24 h), NFR13, NFR23, NFR24, NFR25 (builder queue depth)

**Acceptance Criteria:**

**Given** `POST /v1/branches/{id}/audit/build {month}` from an owner of that branch
**When** no ready cache exists for that month
**Then** a `builder_jobs(branch_id, month, state = queued)` row is created or the existing queued/building job is reused, and the response is `AUDIT_BUILDING` with progress 0

**Given** the `builder` role
**When** it picks a queued job
**Then** it downloads the nearest snapshot on or before the month start plus every batch up to the month end, replays them with `arena-replay` into a temp SQLite honouring fork points (orphaned rows kept in a separate table), uploads `audit-cache/<yyyy-mm>.db.zst` (the R2 prefix has a 24 h lifecycle rule), sets the job `ready` with `built_at` and `key`, and updates `progress` as it goes

**Given** no snapshot before the month
**When** the job runs
**Then** it replays from the branch's first batch

**Given** a missing batch range or a checksum mismatch
**When** the replay hits it
**Then** the job becomes `failed` and the API returns `AUDIT_BUILD_FAILED`; a new build request starts a new attempt

**Given** the round-trip test
**When** a fixture Hub's snapshot and batches are replayed by the builder
**Then** the checksum of the rebuilt database equals the Hub database's checksum for the same seq

**Given** a tenant above its per-tenant audit-build limit
**When** another build is requested
**Then** it returns `RATE_LIMITED`, and "builder queue depth" is exported as a metric

### Story 8.35: The builder serves audit queries from a local cache

As a café owner or support agent,
I want searches over a built month to answer quickly from a fixed set of queries,
So that audit is fast after the first build and can never run arbitrary queries over my data.

**Covers:** FR158 (search), AD-37 (fixed query catalogue, 50 GB LRU), AD-21 (support reads with a recorded reason), AD-24 (player ids only), NFR7, NFR24 (audit cache 24 h)

**Acceptance Criteria:**

**Given** a `ready` job for the month
**When** `GET /v1/branches/{id}/audit?q=` is called
**Then** the builder serves it from its local copy of `audit-cache/<yyyy-mm>.db.zst` (downloading it on a miss) using only queries from the `arena-proto` query catalogue (by actor, module, intent, entity, time range; paged 50 rows), and any other query shape returns `UNSUPPORTED`

**Given** the local cache
**When** it exceeds 50 GB
**Then** least-recently-used months are evicted, and an evicted month is re-downloaded from R2 on the next query while its cache object exists

**Given** a cache whose `built_at` is older than 24 h, or whose R2 object has expired
**When** a query arrives
**Then** the API returns `AUDIT_BUILDING` and a new build job is queued

**Given** a query while the month is `queued` or `building`
**When** it is called
**Then** it returns `AUDIT_BUILDING` with the job's progress; a `failed` job returns `AUDIT_BUILD_FAILED`

**Given** query results
**When** they are returned
**Then** player PII columns are omitted (ciphertext never leaves the builder) and players appear by id only; orphaned rows are returned in a separate list

**Given** a support agent using the admin app
**When** they query a branch's audit
**Then** the request requires a recorded reason and is written to `admin_audit` with `actor.kind = admin`

### Story 8.36: Audit search in Reports with "Preparing audit…"

As a café owner,
I want to search a branch's change history by who did it and what part of the app,
So that I can find out what happened to a price, a bill or a shift.

**Covers:** FR158, NFR7 (first results < 10 s), UX-DR81 (Preparing audit…), UX-DR90 (Reports → Audit), UX-DR102 (audit building/failed), UX-DR103

**Acceptance Criteria:**

**Given** Reports → Audit
**When** it opens
**Then** it offers branch, month, actor (staff, owner, system, device, player id) and module (sessions, money, POS, stock, staff, settings, commands) filters and a free-text intent search from the query catalogue

**Given** the first search of a month with no ready cache
**When** it runs
**Then** the results area shows "Preparing audit…" as muted text on a surface panel with progress (no spinner overlay), polls the API, and shows the first page of results when ready; first results arrive in under 10 s for a busy branch month in the staging benchmark

**Given** a later search of the same month within 24 h
**When** it runs
**Then** results return without the preparing state

**Given** a failed build
**When** the API returns `AUDIT_BUILD_FAILED`
**Then** the panel shows "Couldn't prepare the audit." with Retry

**Given** results
**When** they render
**Then** each row shows time, actor (staff name or "Owner" or "System"), intent, entity and changed fields, with players shown as ids only, and `elevated_by` noted ("PIN by Priya", "Owner MFA")

**Given** orphaned rows from a fork in that month
**When** results render
**Then** they are listed in a separate "Not applied (orphaned)" section

### Story 8.37: A Cloudflare Tunnel for entitled branches, run by the Windows Hub

As a café owner with the Tunnel add-on or Premium,
I want my Hub reachable for live view and remote devices without opening any port at the café,
So that remote features work through ordinary internet with nothing exposed.

**Covers:** FR151 (Tunnel prerequisite), FR21 (Tunnel prerequisite), AD-6, AD-29 (`tunnel` flag), AD-22, NFR11; Additional Requirements "Build spikes" (Cloudflare Tunnel pricing per branch)

**Acceptance Criteria:**

**Given** a branch whose licence carries the `tunnel` flag (Premium, or the Tunnel add-on on Basic)
**When** the heartbeat sees the flag for the first time
**Then** the control role creates a named Cloudflare Tunnel and an Access application for `<branch_id>.tunnel.arena360.app` through the Cloudflare API and returns the tunnel credential once in the heartbeat response

**Given** the credential
**When** the Windows Hub receives it
**Then** it stores it in DPAPI and runs the bundled `cloudflared` connector as a child process, outbound only, forwarding to the Hub's live-view and remote-device listeners only

**Given** the `tunnel` flag removed (downgrade or lapse after `exp`)
**When** the next heartbeat arrives
**Then** the Hub stops the connector and deletes the credential, and the cloud deletes the tunnel and Access app

**Given** an Android Hub
**When** the flag is present
**Then** no tunnel is created (no Tunnel on Android) and the owner app shows the add-on as "Windows Hub only"

**Given** the connector crashes or the WAN drops
**When** it happens
**Then** the Hub restarts it with backoff and trading is never affected

**Given** the build spike
**When** it is run before this story starts
**Then** the per-branch cost of a tunnel plus Access app is recorded against the ₹200 add-on price

### Story 8.38: Live view — a read-only floor through the Tunnel

As a café owner on Premium or with the Tunnel add-on,
I want to watch my branch's floor second by second from anywhere,
So that I can see a busy night as it happens without anyone being able to change anything.

**Covers:** FR151, AD-43, AD-42 (live-view client class), AD-25, AD-29, UX-DR24, UX-DR61, UX-DR82 (Live view placement)

**Acceptance Criteria:**

**Given** an entitled branch
**When** the owner opens Live view (sidebar on desktop, the button on each Today branch card on phone)
**Then** the app calls `GET /v1/live/{branch}`, gets a branch-scoped token signed by the cloud valid for at most 5 minutes, and opens a WebSocket to the branch's tunnel hostname with it

**Given** the token
**When** the Hub receives the `hello`
**Then** it verifies the signature with its licence verify keys, checks `branch_id` and expiry, and admits the connection as the live-view client class, which may only subscribe

**Given** a live-view connection
**When** it sends any intent or command frame
**Then** the Hub answers `FORBIDDEN` and closes nothing else

**Given** an open live view
**When** sessions start, tick and end
**Then** the owner sees read-only floor tiles (device, state, time, player id or Guest) grouped by zone, updated from `session.tick` and `device.state` within 2 s, with a "Live" data age

**Given** the token nears expiry
**When** 4 minutes have passed
**Then** the app fetches a new token and re-authenticates without dropping the view; an expired token is refused `TOKEN_INVALID`

**Given** a branch without the `tunnel` flag
**When** the owner looks for Live view
**Then** it shows `tier-lock` "Available on Premium" with Upgrade, and `GET /v1/live/{branch}` returns `FEATURE_NOT_ENTITLED`

**Given** the Hub unreachable through the tunnel
**When** the owner opens Live view
**Then** the app shows "Branch offline" and falls back to the Today card

### Story 8.39: Remote kiosks connect through the Tunnel with their device token

As a café owner with the Tunnel entitlement,
I want a kiosk outside the café network to pair and connect securely,
So that I can run a remote seat without opening the café to the internet.

**Covers:** FR21, AD-6, AD-42 (paired-device class, device token, pinned TLS), AD-29, NFR11

**Acceptance Criteria:**

**Given** an entitled branch
**When** staff generate a kiosk pairing code and choose "Remote device"
**Then** the code screen also shows the tunnel hostname, and a kiosk outside the LAN pairs by typing it, exchanging the code for a device token as on the LAN

**Given** a remote kiosk with its device token
**When** it connects through the tunnel
**Then** it is admitted only as the paired-device class with TLS pinned to the Hub certificate, appears on the floor view with a "Remote" label, and counts toward the device cap

**Given** a connection through the tunnel without a valid device token
**When** it sends `hello`
**Then** it is refused `TOKEN_INVALID` and closed; no anonymous or browser class is accepted through the tunnel

**Given** the `tunnel` flag is removed
**When** the connector stops
**Then** remote kiosks lose connection and follow Story 6.36 Hub-unreachable behaviour; running sessions are not ended by the Hub

**Given** a revoked remote device
**When** it reconnects
**Then** it is refused `TOKEN_REVOKED`

> Draft notes: the Pi agent (Epic 12) uses this same remote-device path when it ships.

### Story 8.40: The owner's remote experience proven against its targets

As a café owner,
I want the dashboard speed, data age, command delivery, audit time and offline catch-up measured on every release,
So that what I'm promised is what I get.

**Covers:** NFR1 (catch-up within 10 min), NFR7, NFR20, NFR21, NFR25

**Acceptance Criteria:**

**Given** the nightly staging run with the internal test café on Basic and Premium comp entitlements
**When** it measures Today
**Then** first render is under 2 s on the reference phone profile, and data age is within cadence (Basic ≤ 15 min, Premium ≤ 5 min) on at least 99% of 1,000 scripted opens

**Given** 200 owner commands to an online branch
**When** delivery is timed from post to `applied`
**Then** p95 is under 2 minutes

**Given** an 8-hour WAN outage soak with 20 kiosks (1,000 sessions, 500 orders)
**When** the WAN returns
**Then** all batches, the current summary and any missed rollups and outbox sends complete within 10 minutes, with no gap reported by the heartbeat and no duplicate send

**Given** the cloud stopped entirely for 1 hour during the soak
**When** the branch trades
**Then** nothing at the branch changes beyond `chip-offline`, and everything catches up after the cloud returns

**Given** a month of staging traffic
**When** R2 PUTs per tenant are summed
**Then** the per-branch cost estimate is under ₹10 on average (≈ ₹6 Basic, ≈ ₹12 Premium) and is published to the metrics dashboard

**Given** a busy branch month in staging
**When** an audit is requested for the first time
**Then** first results arrive in under 10 s

## Epic 9: Resilience, backups and data rights

A branch survives Hub failure through standby and promote, and keeps exports, cloud backups, the GST archive and an erasure that holds up.

> Draft notes: this epic builds on the Epic 1 store and lease (Story 1.4: WAL + fsync, `hub_instance_id`, provisional epoch, follower role, `NOT_PRIMARY`), `apply()` and the change log (Stories 1.5–1.6), the LAN envelope and client classes (Story 1.7), inline PIN (Story 1.11), the audit viewer (Story 1.13) and `chip-offline` (Story 1.20); on the Epic 2 heartbeat (Story 2.10: `{licence, fence, cloud_time}`, first-heartbeat fence creation), entitlement (Story 2.11), Hub pairing and the installer (Stories 2.8–2.9), kiosk discovery and pinned-fingerprint pairing (Stories 2.16–2.17), the device cap that already excludes standby Hubs (Story 2.18), the backup location setting (Story 2.25), Account → Branches with the standby-code placeholder (Story 2.29) and the go-live "standby PC" item (Story 2.30); on Epic 3 player profiles with per-player keys in `player_keys` wrapped by the branch KEK (Story 3.6) and lookup that already hides erased players (Story 3.7); on the Android folder picker (Story 4.7); on Hub report exports (Story 5.32) and the epoch-segmented invoice series (Story 5.14); on the reference cache that a standby or promoted Hub keeps or refetches (Story 6.2) and kiosk reconnect-and-reconcile (Story 6.36); and on the Kitchen display reconnect (Story 7.10). From Epic 8 it uses, by title: "Presign issues batch and summary upload URLs under the branch's own R2 prefix" (extended here with `kind: keys` and `kind: gst_archive`), "The Hub uploads gap-free change-log batches at its tier cadence" (which already writes the `fork` header from the `lease` fork fields), "The heartbeat detects upload gaps and the Hub resends them", "The Hub uploads a daily snapshot after close" (`snapshot_log`), "The Hub applies owner commands exactly once, and the owner's change wins" (`backup.now`), "Commands are swept, expire after 7 days, and are fenced by epoch" (`FENCED` results and re-queue when the fence moves), "Step-up MFA for sensitive commands" (reserved sensitive kinds `hub.revoke` and `backup.restore`, delivered here), "The Hub drains its notification outbox after the undo window" (skips erased players), "Owner alerts by web push and WhatsApp, and the Alerts tab" and "The audit builder rebuilds a branch month on demand" (`arena-replay` honouring fork points). **Epoch (decided by the spine):** `(counter, hub_instance_id)`; the counter comes from the cloud by compare-and-set on the single `gz_control.branch_fence` row, written only by the control role; offline, the Hub mints `max seen + 1` provisionally and the first heartbeat confirms it or tells it to self-lock. Promote and restore are the only things that open a new epoch, and both re-apply every `player.erased` tombstone before the writer opens. **Promotion is manual only** — there is no auto-promote anywhere. **Roles (decided):** `ops.promote` is granted to the built-in Manager role (Journey 8: "any manager's PIN"); `ops.restore` stays Owner-only. This amends the LLD role table ("Manager: all except `ops.*`"). **Standby** is Windows-only and needs the `standby` licence flag (paid tiers); MVP allows one LAN standby per branch. Off-site standby (FR172) is Epic 14. **Snapshots:** the daily snapshot is Epic 8's; this epic adds the upgrade-first snapshot, retention, the GST archive, the key backup, restore and the backup status. **Data rights:** erasure is crypto-shredding — batches, snapshots, exports and audit caches only ever carry ciphertext, so deleting the key is the erasure. The branch KEK is escrowed in `gz_control` on every tier, Free included (decided): this is the one key-only exception to "Free uploads nothing" — it is a control row, never an object under the branch prefix — so a Free branch can restore its own export on new hardware. This amends the LLD role table ("Manager: all except `ops.*`") as stated above. **Known limits (accepted):** (1) on Free, a local export taken before an erasure, restored on a new PC with no access to the old database, cannot learn of that erasure; paid branches close the gap with the cloud key backup; (2) after a cloud restore onto new hardware, kiosks must pair again, because they pinned the old PC's certificate; (3) a standby needs two codes — the owner's standby code from the cloud, then the 6-digit "Link standby" code shown at the main Hub. **Out of this epic:** off-site standby (Epic 14), the Android → Windows migration (Epic 10, which reuses the export format from Story 9.4), owner-side report exports from rollups (Epic 8). Every story adds its new intents to the Story 1.6 `proptest` generator.

### Story 9.1: Erase a player's personal data by crypto-shredding

As a café manager,
I want to erase a player's personal data when they ask,
So that the branch meets DPDP and that player's details can never be read again.

**Covers:** FR200 (Hub erasure), AD-24, AD-2, AD-30, NFR13, UX-DR66, UX-DR103

**Acceptance Criteria:**

**Given** a player profile on the Hub
**When** a Manager chooses "Erase personal data"
**Then** an in-place panel shows what is erased (name, phone, email, DOB, notes, username) and what is kept (player id, money and session history, consent record), with `inline-pin` for the manager PIN

**Given** the PIN is confirmed
**When** `player.erased` (permission `player.manage` + manager PIN) is applied
**Then** in one transaction it clears the PII columns, sets `erased_at`, deletes the player's `player_keys` row and removes the `phone_lookup` hash; the consent row is kept with the tombstone; the change rows carry only the player id and `erased_at`

**Given** the player has an active, paused or grace session, or credit outstanding above zero
**When** erasure is attempted
**Then** it fails `ERASE_BLOCKED` with the reason ("End the session first" / "Collect or write off credit first") and nothing changes

**Given** the player has a wallet balance above zero
**When** erasure is confirmed
**Then** the panel first requires the balance to be refunded at the counter or forfeited with a manager reason, recorded as a normal wallet intent before the tombstone

**Given** an erased player
**When** any screen, report, receipt reprint, audit row or export shows them
**Then** they appear as "Erased player" with their id and no personal data, and the in-memory lookup index drops them at once

**Given** queued notifications for an erased player
**When** the drain from "The Hub drains its notification outbox after the undo window" re-checks the row at send time
**Then** the row is marked `skipped` with reason `erased` and nothing is sent

**Given** a ciphertext column of an erased player in any older change row
**When** anything tries to decrypt it
**Then** decryption fails with `KEY_ERASED` and the caller shows "Erased player"

### Story 9.2: Player keys are backed up and the branch key is escrowed

As a café owner,
I want the keys that protect my players' data kept safely off the Hub,
So that I can restore my branch on a new PC without ever being able to bring an erased player back.

**Covers:** FR200 (key backup), AD-24, AD-22, AD-36, AD-9, NFR13, NFR14

**Acceptance Criteria:**

**Given** a Hub pairs (any tier), or an already paired Hub has no escrow record
**When** pairing completes or the next heartbeat runs
**Then** the Hub sends its branch KEK wrapped with the cloud escrow public key to `POST /v1/kek/escrow`, the control role stores it in `gz_control.kek_escrow(branch_id, hub_instance_id, wrapped_kek, created_at)`, and the escrow private key exists only as a SOPS-managed cloud secret

**Given** a paid-tier Hub and any change to `player_keys` (create, erase)
**When** the change commits
**Then** within one upload interval the key backup task wraps the whole table with the branch KEK and PUTs it to `keys/player-keys.bin` via presign `kind: keys` (new presign kind, same prefix check and `FENCED` rule as snapshots; overwritten, unversioned), and an erased player's key is absent from that object

**Given** a Free-tier Hub
**When** `player_keys` changes
**Then** nothing is uploaded under the branch prefix; the escrowed KEK in `gz_control` is the only key material held by the cloud, the single key-only exception to "Free uploads nothing"

**Given** the key backup PUT fails or the WAN is down
**When** the task retries with backoff
**Then** only the latest table state is uploaded once connectivity returns, and the time of the last successful key backup and the last error are recorded in `backup_state` (new Hub table, outside the change log)

**Given** the escrow endpoint
**When** a caller reads or writes it
**Then** only the control role can read the wrapped KEK, no endpoint in this story returns it, and every access is written to the admin audit

**Given** a privacy test scanning a batch, a key backup object and the Hub log after creating and erasing a player
**When** it runs on every push
**Then** no plaintext name, phone or email appears in any of them

### Story 9.3: The cloud issues epoch counters by compare-and-set on the branch fence

As a café owner,
I want the cloud to decide which PC is the one true main Hub whenever my branch changes Hubs,
So that two PCs can never both take orders and money for my branch.

**Covers:** AD-38, AD-11, AD-1, NFR10

**Acceptance Criteria:**

**Given** a Hub with a valid device token
**When** it calls `POST /v1/fence/claim {expected: <current fence>, instance_id, reason: promote|restore|confirm, fork: {parent_epoch, parent_seq}}`
**Then** the control role compare-and-sets `branch_fence{epoch_counter + 1, instance_id, fork_seq, updated_at}` and returns the new epoch; if `expected` no longer matches it returns `FENCED` with the current fence and writes nothing

**Given** two claims for the same branch at the same moment
**When** both reach the control role
**Then** exactly one succeeds and the other gets `FENCED` (integration test with concurrent claims)

**Given** a Hub holding a provisional epoch minted offline
**When** its first heartbeat after reconnect arrives
**Then** if no higher fence exists the cloud confirms it by compare-and-set and the Hub clears `lease.provisional`; if a higher fence exists the response carries it and the Hub closes its writer channel (Story 1.4 demotion) and stops accepting LAN intents with `NOT_PRIMARY`

**Given** two Hubs that each minted the same provisional counter offline
**When** both confirm on reconnect
**Then** the first compare-and-set wins and the second receives `FENCED` with the winner's epoch and demotes, whichever has the higher instance id

**Given** a successful claim
**When** the control role commits it
**Then** it stores `fork_seq` from the claim, so the re-queue rule in "Commands are swept, expire after 7 days, and are fenced by epoch" uses the real fork point instead of a fixture

**Given** any role other than control (command, builder, jobs)
**When** it needs the current epoch
**Then** it reads `branch_fence` and never writes it (code review gate: one write path in `arena-cloud`)

**Given** the Hub
**When** it sees an epoch on the LAN, in a heartbeat or in a claim reply
**Then** it records the highest seen counter in `lease`, so an offline claim mints `max seen + 1`

### Story 9.4: One-tap full database export to a folder or USB drive

As a café owner on any tier,
I want to save my whole branch database to a folder or USB drive with one tap,
So that I always hold my own copy of my data.

**Covers:** FR173 (export), FR199 (DB export), AD-8 (export format in `arena-proto`), AD-11, AD-24, NFR6 (1 GB < 10 s), NFR13, UX-DR88

**Acceptance Criteria:**

**Given** Hub → System → Backup
**When** a Manager taps "Export now"
**Then** `backup.export` (permission `backup.export`) writes `<branch_code>-<yyyy-mm-dd-hhmm>.a360` to the backup location from Story 2.25 (or Story 4.7 on Android), and the panel shows file name, size and time on success

**Given** the export runs
**When** the file is written
**Then** it holds a versioned header defined in `arena-proto` (format version, branch id, `branch_code`, epoch, last `seq`, schema version, exported_at, sha256) followed by a zstd-compressed `VACUUM INTO` copy of the branch database, with `player_keys` still wrapped by the branch KEK
**And** it excludes the reference cache (catalogue, media), the licence cache, device tokens and every keystore secret

**Given** trading continues during an export
**When** intents arrive
**Then** they are never blocked; the export is a consistent point-in-time copy and its header `seq` names that point

**Given** a 1 GB branch database on the reference PC (2015 i3 + HDD)
**When** the export benchmark runs in CI
**Then** it completes in under 10 s from tap to file closed

**Given** no backup location is set, the location is not writable, or the USB drive is removed mid-write
**When** export is attempted
**Then** the panel shows the plain cause inline with "Choose location" or "Try again", any partial file is deleted, and nothing else on the Hub is affected

**Given** a Counter without `backup.export`
**When** they tap "Export now"
**Then** `inline-pin` opens for manager elevation, and the export is audited with both actors

### Story 9.5: The Hub saves a daily export to the backup location

As a café owner,
I want a copy saved automatically every day,
So that I have a recent backup even if nobody remembers to tap Export.

**Covers:** FR173, NFR9, UX-DR88

**Acceptance Criteria:**

**Given** a branch with a writable backup location
**When** the business day closes (closing time + 30 min, or the last shift close)
**Then** the Hub runs the Story 9.4 export as `actor{kind: system}` and keeps the newest 7 daily files, deleting older auto-export files only (never files a person exported)

**Given** the backup location is unavailable at that time
**When** the daily export fails
**Then** the Hub raises an `alert-badge` "Backup folder not available" in the header until the next successful export, and setup status reports backup location not set (Story 2.25 / 4.7)

**Given** System → Backup
**When** it opens
**Then** it lists the last 7 exports with time, size and "auto" or the staff name

### Story 9.6: The export format is documented and every report exports alongside it

As a café owner,
I want my data in a documented format I can open without Arena 360,
So that I can leave or audit at any time and my data is truly mine.

**Covers:** FR199, AD-8, AD-41, NFR16

**Acceptance Criteria:**

**Given** the export file format in `arena-proto`
**When** the docs build runs
**Then** it generates a public reference (header fields, decompression step, every table and column with type and meaning, money in minor units, ciphertext columns marked) for the help centre, and CI fails if the reference is stale against the migrations

**Given** System → Backup → "Export everything"
**When** a Manager picks a date range
**Then** the Hub writes one folder holding the Story 9.4 database file plus every Hub report from Story 5.32 for that range as CSV, with a `README.txt` linking the format reference

**Given** an export file
**When** a CI test decompresses it with the stock `zstd` tool and opens it with stock `sqlite3`
**Then** every table listed in the reference is present and its row counts match the source database

**Given** a Free branch
**When** export or "Export everything" is used
**Then** both work exactly as on paid tiers, with no `tier-lock`

### Story 9.7: Restore a branch from an export file as a new epoch

As a café owner,
I want to restore my branch from an export file on this PC,
So that a corrupted or wrongly changed database can be put back to a known good point.

**Covers:** FR173 (restore), FR200 (stays erased after restore), AD-11, AD-38, AD-24, AD-8, AD-30, UX-DR66, UX-DR103

**Acceptance Criteria:**

**Given** Hub → System → Backup → "Restore from file"
**When** the Owner picks a `.a360` file
**Then** the Hub checks the sha256 and branch id and shows branch name, exported time, epoch and last `seq`, and the warning "Everything after <exported time> on this PC will be set aside. Kiosks reconnect by themselves."

**Given** the file's branch id differs, its sha256 fails, or its schema is newer than the binary
**When** it is checked
**Then** restore is refused with `RESTORE_WRONG_BRANCH`, `RESTORE_CORRUPT` or `SCHEMA_TOO_NEW` and the current database is untouched; an older schema is migrated forward after restore

**Given** the Owner confirms with their PIN (`ops.restore`) and types the branch code
**When** restore runs
**Then** the Hub closes its writer, moves the current database to `orphaned/<epoch>-<timestamp>.db` (kept 90 days), loads the file, claims a new epoch (Story 9.3) with `fork = {file epoch, file seq}` (provisional if the cloud is unreachable within 3 s), re-applies every `player.erased` tombstone, opens the writer and announces `epoch.changed`

**Given** tombstones in the replaced database, or (paid tier, cloud reachable) players absent from the latest `keys/player-keys.bin`
**When** erasures are re-applied
**Then** each such player is erased in the restored database before the writer opens, so a player erased after the export stays erased

**Given** a Free branch restoring, on a new PC with no replaced database, an export taken before a player was erased
**When** restore runs
**Then** that player cannot be known to be erased and comes back with their data (accepted known limit); the confirmation panel says "Players erased after <exported time> may reappear." on Free

**Given** a Manager or Counter
**When** they open "Restore from file"
**Then** it asks for the Owner PIN in place and refuses any other role

**Given** a restore
**When** it completes
**Then** the audit records it as `ops.restore` with the file header, the old and new epoch, and the number of rows set aside

### Story 9.8: Owner replaces a failed Hub and the new PC receives the branch key

As a café owner whose Hub PC has died,
I want to pair a replacement PC to the same branch and let it unlock my backups,
So that I can get back to trading on new hardware.

**Covers:** FR173 (restore on new hardware), FR177, AD-10, AD-24, AD-5 (sensitive kinds `hub.revoke`, `backup.restore`), AD-22, UX-DR83

**Acceptance Criteria:**

**Given** Account → Branches in the owner app for a branch with a paired Hub
**When** the owner chooses "Replace Hub" and passes step-up MFA ("Step-up MFA for sensitive commands")
**Then** the reserved sensitive kind `hub.revoke` is delivered: the cloud revokes the old Hub's token at once (its next heartbeat is refused `TOKEN_REVOKED`), and a new single-use 8-character Hub pairing code is shown with the Story 2.8 card

**Given** a fresh Hub install paired with that code to a branch that already has history
**When** pairing completes
**Then** no `branch.bootstrap` is queued, and the Hub opens in a "Restore this branch" state offering "Restore from a file" and (paid tiers) "Restore from cloud backup", serving no devices until a restore completes

**Given** the new Hub in that state
**When** it requests the branch key
**Then** the owner app shows "Release branch key to <PC name>"; approving sends the reserved sensitive kind `backup.restore` (step-up MFA), shown as a `command-row`, and the control role then unwraps the escrowed KEK and returns it re-wrapped to a one-time public key the new Hub generated at pairing; the Hub stores it in the OS keystore

**Given** the KEK has been released
**When** the Owner restores from a file
**Then** the Story 9.7 flow runs on this PC (no replaced database), and erasures are re-applied from the cloud key backup when the tier allows

**Given** no approval within 7 days, or a denial
**When** the Hub asks again
**Then** it shows "Waiting for the owner to approve on their phone" and never receives the key; every release, denial and expiry is in the admin audit

### Story 9.9: Upgrading from Free uploads a full snapshot before the first batch

As a café owner moving from Free to a paid tier,
I want my existing history uploaded first,
So that my cloud backup is complete from day one, not from the upgrade.

**Covers:** FR174 (upgrade path), AD-36, AD-4, AD-29

**Acceptance Criteria:**

**Given** a Free branch whose next heartbeat delivers a paid licence
**When** the Hub applies it
**Then** before any change-log batch it uploads a full snapshot (the "The Hub uploads a daily snapshot after close" mechanism, key `snapshots/<today>.db.zst`, recorded in `snapshot_log`) and `keys/player-keys.bin`, then starts normal batching from the snapshot's `seq + 1`

**Given** the snapshot upload fails
**When** the uploader runs
**Then** no batch is uploaded until the snapshot succeeds, and `backup_state` records "first snapshot pending" with the last error

**Given** the cloud's view after upgrade
**When** heartbeat gap detection runs
**Then** it treats the snapshot `seq` (reported in the heartbeat as `base_seq`) as the starting point, so no `resend` is requested for history before it

**Given** a branch that lapses back to Free
**When** the lapse applies
**Then** uploads stop at once, already uploaded objects stay in R2, and re-upgrading starts again with a fresh full snapshot

### Story 9.10: Fetch a paid branch's cloud backup onto a replacement Hub

As a café owner on a paid tier,
I want my replacement PC to download my branch's cloud backup safely,
So that it has everything it needs to rebuild the branch.

**Covers:** FR173 (restore), FR174, AD-36, AD-4, AD-40, NFR11

**Acceptance Criteria:**

**Given** a Hub in the "Restore this branch" state with the KEK released (Story 9.8) on a paid tier
**When** the Owner chooses "Restore from cloud backup" and confirms with their PIN
**Then** the Hub calls `POST /v1/restore/plan`, which returns read URLs (10 min) for the newest snapshot, every batch after its `seq` across epochs with their fork headers, and `keys/player-keys.bin`, all under the caller's branch prefix only

**Given** the plan
**When** the Hub downloads the objects
**Then** it verifies every batch's sha256 and header, stores them in a staging folder, refreshes expired URLs by calling the plan again, and resumes after a dropped connection without re-downloading finished objects

**Given** a missing or corrupt batch inside the range
**When** the download check runs
**Then** it stops with `RESTORE_GAP` naming the seq range and offers "Restore up to <last good seq>" with the count of changes that would be lost; nothing is replayed until the Owner chooses

**Given** progress
**When** downloads run
**Then** the Hub shows "Downloading 3 of 41" with bytes and percent, and "Cancel" leaves the PC in the "Restore this branch" state with the staging folder removed

**Given** a Free branch, a caller without the released KEK, or a token for another branch
**When** `POST /v1/restore/plan` is called
**Then** it is refused `FEATURE_NOT_ENTITLED`, `KEY_NOT_RELEASED` or `FORBIDDEN` and returns no URLs

### Story 9.11: Rebuild the branch from the fetched backup as a new epoch

As a café owner on a paid tier,
I want the replacement PC to rebuild my branch exactly and open it as the new main Hub,
So that losing the Hub PC and its disk does not lose my business records.

**Covers:** FR173 (restore), FR200 (stays erased after restore), AD-11, AD-38, AD-4, AD-24, NFR9

**Acceptance Criteria:**

**Given** a complete staging folder from Story 9.10
**When** the Hub replays it with `arena-replay`
**Then** it starts from the snapshot, applies batches in order honouring fork points (parent-epoch rows above `fork.parent_seq` are skipped and counted as orphaned), unwraps the key backup with the KEK, and drops any key not in the key backup

**Given** the replay finishes
**When** the Hub opens the branch
**Then** it re-applies every `player.erased` tombstone, claims a new epoch (Story 9.3) with `fork = {last epoch, last seq replayed}`, opens the writer and announces `epoch.changed`

**Given** kiosks and Pis paired to the old Hub PC
**When** they find the restored Hub
**Then** they refuse its certificate (pinned to the old PC), and the restored Hub lists them under Settings → Devices as "Pair again" so staff re-pair each one with the 6-digit code (accepted known limit); their device records, groups and history are kept

**Given** replay
**When** it runs
**Then** the Hub shows "Replaying" and "Checking" with percent, can be cancelled before the writer opens (leaving the "Restore this branch" state), and the audit records the restore with snapshot date, batch range, orphaned count and the new epoch

**Given** the restore drill in CI (Hub with snapshot + 3 days of batches + a fork + an erased player)
**When** a fresh Hub restores it
**Then** the restored database's checksum over branch rows equals the source at the last uploaded `seq`, the erased player stays erased, and orphaned rows are absent

### Story 9.12: The Hub emits a yearly GST archive kept for 8 years

As a GST-registered café owner,
I want each financial year's invoices and tax summaries archived for 8 years,
So that I can answer a tax notice even after my plan's backup retention has passed.

**Covers:** FR174 (GST archive), AD-36, AD-26, AD-7, NFR24

**Acceptance Criteria:**

**Given** a paid-tier Hub and the first business-day close after the FY ends (31 March for India)
**When** the publisher runs
**Then** it writes `gst-archive/<fy>.zst` holding every invoice, credit note and per-rate tax summary stamped with that `fy`, plus a list of invoice numbers orphaned by a fork, and PUTs it via presign `kind: gst_archive` (new presign kind, same prefix and `FENCED` rules as snapshots)

**Given** the archive is uploaded
**When** the publisher records it
**Then** a system intent records `fy`, object key, sha256 and counts in the audit, and System → Backup shows "GST archive 26-27 saved"

**Given** an invoice or credit note created later with a past `fy` (late refund)
**When** the next close runs
**Then** the archive for that `fy` is regenerated and overwritten

**Given** a branch with `tax_mode` other than `gst_in`
**When** the FY closes
**Then** the same archive is produced with its generic tax lines, under the same key and retention

**Given** a Free branch
**When** the FY closes
**Then** nothing is uploaded, and the GST money rows stay in the Hub database for 8 years and are included in every export

### Story 9.13: Backups are kept for the plan's retention and GST records for 8 years

As a café owner,
I want backups kept for as long as my plan promises and no longer,
So that I can rely on old records while storage costs stay low.

**Covers:** FR174 (retention), NFR24, NFR20, AD-36

**Acceptance Criteria:**

**Given** the jobs role's daily retention sweep
**When** it runs for a branch
**Then** it deletes `changes/`, `snapshots/` and `rollups/` objects older than the licence `retention_days` (Basic 365, Premium 1,095) and never touches `gst-archive/`, `keys/` or `state/`

**Given** `gst-archive/<fy>.zst` objects
**When** the sweep runs
**Then** each is deleted only after 8 years from the end of its FY, regardless of tier or lapse

**Given** a branch that moves from Premium to Basic, or lapses to Free
**When** the sweep runs
**Then** objects are kept under the previous tier's retention for 30 days, then the new retention applies (lapsed branches keep the last paid tier's retention)

**Given** the Hub
**When** daily housekeeping runs
**Then** it prunes `changes` rows uploaded more than 90 days ago, never prunes rows not yet uploaded, and never prunes invoices, credit notes, payments or tax rows younger than 8 years; on Free (nothing uploaded) `changes` is not pruned

**Given** a sweep that would delete the newest snapshot needed to restore a retained batch range
**When** it plans deletions
**Then** it keeps that snapshot, so every retained batch has a base to restore from

### Story 9.14: Cloud backup status in the Hub header and System screen

As counter staff and managers on a paid tier,
I want to see at a glance when the branch last backed up,
So that we notice early if backups stop.

**Covers:** FR175 (Hub status), UX-DR60, UX-DR88, UX-DR23, UX-DR113

**Acceptance Criteria:**

**Given** a paid-tier Hub
**When** the header renders
**Then** it shows backup status in muted `hub-label` ("Backed up 4 min ago", plus "· 3 pending" when batches are waiting), and tapping it opens System → Backup

**Given** the last successful upload is older than twice `upload_interval_s`
**When** the header updates
**Then** the status changes to a warning `status-badge` "Backup behind" with text, and the change is announced in the polite live region; it returns to normal after the next upload

**Given** System → Backup
**When** it opens
**Then** it shows last batch upload, last summary, last snapshot, last key backup, last GST archive, pending batch count, "First backup pending" after an upgrade, and the last error (if any) in plain words, read from `upload_cursor`, `snapshot_log` and `backup_state`

**Given** a Free Hub
**When** the header renders
**Then** no backup status is shown, and System → Backup shows local exports only, with cloud backup as `tier-lock` "Available on Basic"

**Given** the phone layout (below 840px)
**When** the header renders
**Then** backup status is dropped from the header and stays reachable in System → Backup

### Story 9.15: Owner sees backup status and sends "Backup now"

As a café owner,
I want to see each branch's backup state and force a backup from my phone,
So that I can secure the day's data before a risky change or a power cut.

**Covers:** FR175 (owner "Backup now"), AD-5, UX-DR83, UX-DR81, UX-DR24

**Acceptance Criteria:**

**Given** the owner app Account → Branches on a paid tier
**When** it opens
**Then** each branch shows last upload, last snapshot and pending batches from the latest heartbeat with their data age

**Given** a paid branch
**When** the owner taps "Backup now"
**Then** the Epic 8 `backup.now` command is sent with a `command-row` (Queued on this device → Waiting for branch → Applied), and the Hub runs an upload and summary cycle at once, plus a key backup if one is pending

**Given** a "Backup now" applied less than 15 minutes ago
**When** another is sent
**Then** it is rejected `RATE_LIMITED` with "A backup ran at <time>. Try again after <time>."

**Given** the upload triggered by the command fails
**When** the Hub reports the result
**Then** the command is `rejected` with the transport error (as Epic 8 defines for `backup.now`), its row shows "Couldn't apply at branch" + Retry, and the branch's backup line shows "Backup failed" with the time

**Given** a Free branch
**When** the owner looks for "Backup now"
**Then** it shows `tier-lock` "Available on Basic"

### Story 9.16: Owner issues a standby code and a second PC pairs as standby

As a café owner on a paid tier,
I want to add a second PC as a standby Hub,
So that my branch can keep going if the main Hub PC fails.

**Covers:** FR169 (pairing), FR177 (standby codes), AD-10, AD-29, AD-42, AD-11, UX-DR24, UX-DR66

**Acceptance Criteria:**

**Given** Account → Branches on a branch whose licence has the `standby` flag
**When** the owner chooses "Add standby PC"
**Then** a single-use 8-character standby code is shown (same card and expiry as the Hub pairing code), replacing the Story 2.29 `tier-lock` placeholder; on Free it stays `tier-lock`, and a second standby is refused `STANDBY_EXISTS`

**Given** the Windows Hub installer on a second PC
**When** the standby code is entered
**Then** `POST /v1/pair` registers it as a Hub device with `role: standby`, its own `hub_instance_id`, device token and licence, and it never runs `branch.bootstrap`

**Given** the new standby on the LAN
**When** it finds the main Hub by mDNS and a Manager enters the 6-digit "Link standby" code shown at the main Hub (Settings → Standby, 10-minute expiry)
**Then** the main Hub mints a paired-device token with `kind = standby` over pinned TLS and stores the standby's certificate fingerprint

**Given** a standby has linked
**When** the main Hub next talks to each paired kiosk and Pi
**Then** it sends the standby's certificate fingerprint, and each device adds it to its pinned set, so it will accept the standby after promotion

**Given** an Android Hub, or a branch without the `standby` flag
**When** standby is attempted
**Then** pairing is refused `TIER_LOCKED` (or `PLATFORM_UNSUPPORTED` on Android) and the Hub shows `tier-lock`

**Given** the go-live checklist (Story 2.30) on a paid branch
**When** a standby is linked
**Then** "Standby PC" shows done

### Story 9.17: The standby bootstraps from the main Hub and follows it live

As a café owner,
I want the standby PC to hold an up-to-the-second copy of the branch,
So that promoting it loses nothing.

**Covers:** FR169 (follow), AD-4 (standby bootstrap), AD-8 (`SCHEMA_BEHIND`), AD-24 (keys replicated), AD-1, NFR9

**Acceptance Criteria:**

**Given** a linked standby with an empty database
**When** it starts following
**Then** it pulls a LAN snapshot from the main Hub (`GET /v1/replicate/snapshot`, consistent at a stated `seq`), loads it, then connects to `wss://<main>:7400/v1/replicate?from_seq=N` with its device token over pinned TLS

**Given** the replication stream
**When** the main Hub commits rows
**Then** the standby receives closed batches first, then live rows as they commit, applies them with `arena-replay` in one transaction per batch, and writes them verbatim into its own `changes` (same `seq`, same epoch), so after promotion it continues the sequence

**Given** `player_keys` changes on the main Hub (create, erase)
**When** they commit
**Then** they reach the standby on the same channel and an erased key is deleted on the standby within one replication message

**Given** the standby's schema version is below the main Hub's
**When** it connects
**Then** the main Hub refuses with `SCHEMA_BEHIND`, and the standby shows "Update this PC to follow the main Hub"

**Given** the stream drops
**When** the standby reconnects with jittered backoff (2–30 s)
**Then** it resumes from its last applied `seq` with no gap and no duplicate

**Given** the standby at any time
**When** any intent reaches it from a device or its own UI
**Then** it is rejected `NOT_PRIMARY` and nothing is written (read-only follower)

### Story 9.18: The standby keeps its own catalogue, media and licence and shows its status

As a café manager,
I want the standby PC to show clearly that it is the standby and how far behind it is,
So that I know it is ready before I ever need it.

**Covers:** FR169, AD-11 (reference cache), AD-17, AD-10, UX-DR88, UX-DR23

**Acceptance Criteria:**

**Given** the standby Hub app
**When** a staff member signs in (staff are replicated)
**Then** it shows a standby screen: "Standby — following <main Hub name>", lag in rows and seconds, last row received, schema version, licence state and "Promote to main"; the floor is visible read-only and every action is disabled

**Given** the standby
**When** it runs
**Then** it pulls the catalogue manifest itself and prefetches media from the main Hub over the LAN (cloud as fallback), and refreshes its own licence by heartbeat, so nothing in the reference cache comes through replication

**Given** lag above 0 rows for more than 30 s, or no stream for more than 60 s
**When** the standby screen updates
**Then** it shows a warning `status-badge` "Standby behind" with the cause, and the main Hub's System → Standby shows the same

**Given** the main Hub's System → Standby
**When** it opens
**Then** it shows the linked standby's name, lag, last contact and version, plus "Unlink standby" (manager PIN)

**Given** `GET /healthz` on either Hub
**When** it is called
**Then** it includes role, epoch and standby lag

### Story 9.19: Promote the standby to main with a manager PIN

As a counter manager during a busy night,
I want to make the standby the main Hub with one tap and my PIN,
So that the kiosks reconnect and nobody's game stops.

**Covers:** FR169 (promote < 10 s, kiosks reconnect), AD-11, AD-38, AD-1, AD-24, AD-30, NFR8, UX-DR66, UX-DR102

**Acceptance Criteria:**

**Given** the standby screen and a main Hub whose lease has expired (no renewal for 15 s)
**When** a Manager taps "Promote to main" and enters their PIN in place (`ops.promote`)
**Then** `lease.promote` checks schema ≥ the last seen main schema (`SCHEMA_BEHIND` otherwise), claims a new epoch from the cloud with `fork = {old epoch, last replicated seq}` (provisional if no reply within 3 s), re-applies every `player.erased` tombstone, opens the writer, and announces `epoch.changed` on the LAN and in its mDNS TXT record

**Given** promotion
**When** it is measured from PIN confirmation to the first accepted intent
**Then** it takes under 10 s on the reference PC

**Given** kiosks and Pis that lost the old Hub
**When** they see `epoch.changed` or re-resolve by mDNS with jittered backoff (2–30 s)
**Then** they connect to the new main using the pinned standby fingerprint (Story 9.16), present the highest epoch they have seen, reconcile running sessions by the Story 6.36 rules, and from then on refuse any Hub with a lower epoch

**Given** a Kitchen display
**When** it loses the old Hub
**Then** it tries the Hub address list it was given in its feed (main and standby), reconnects to the new main with its existing scoped token, and resumes by the Story 7.10 rules

**Given** promotion completes
**When** the new main runs
**Then** the audit records `lease.promote` with the manager, old and new epoch, and lag at promotion; the old main's tile on the System screen shows "Former main — offline"

**Given** the main Hub's lease is still fresh
**When** "Promote to main" is tapped
**Then** it fails `LEASE_FRESH` with "The main Hub is still running. Check it before promoting." and nothing changes

### Story 9.20: Force promote after a data-loss warning

As a counter manager whose main Hub is frozen but not dead,
I want to force the standby to take over after a clear warning,
So that the floor can keep running, with the owner told what happened.

**Covers:** FR170, AD-11, AD-30, AD-27 (owner alert), NFR8, UX-DR66, UX-DR103

**Acceptance Criteria:**

**Given** promotion fails with `LEASE_FRESH`, or the cloud claim or schema check cannot complete
**When** the panel shows the failure
**Then** it shows "Data since the last sync may be lost. Promote anyway?" with the lag ("12 changes not yet copied") and a "Force promote" action that opens `inline-pin`

**Given** a manager PIN and the data-loss acknowledgement
**When** force promote runs
**Then** `lease.promote{force: true}` takes the lease regardless of freshness and runs the Story 9.19 steps; on `SCHEMA_BEHIND` force is not offered

**Given** a force promote
**When** it completes
**Then** the audit records `force: true`, the manager, the lag and the reason, and the owner receives an alert "Standby forced to main at <branch> by <manager>" through "Owner alerts by web push and WhatsApp, and the Alerts tab" (raised at the Hub and sent when the WAN is next up)

**Given** the old main is still running and on the LAN
**When** the new main announces its epoch
**Then** the old main stops writing within one lease tick (it sees a higher epoch), so the two never accept intents at the same time for more than one renewal tick

### Story 9.21: The new main Hub lists what was set aside after a promote or restore

As a café owner,
I want every promote or restore to leave a clear record of which old changes were set aside,
So that my books and audit stay correct and nothing is silently lost.

**Covers:** AD-4 (fork declaration end to end), AD-11, AD-26 (orphaned invoices), AD-37, NFR10

**Acceptance Criteria:**

**Given** a Hub that opened a new epoch by promote or restore, with `fork_parent_epoch` and `fork_parent_seq` in `lease`
**When** an integration test lets it upload
**Then** its first batch (from "The Hub uploads gap-free change-log batches at its tier cadence") declares that fork with first `seq = parent_seq + 1`, and `branch_fence.fork_seq` equals `parent_seq`

**Given** the old main uploads late batches under its old epoch after the fence moved
**When** standby replay, cloud restore (Story 9.11) and "The audit builder rebuilds a branch month on demand" replay the branch
**Then** old-epoch rows above the fork are never applied to state, and the builder lists them as orphaned with orphaned invoice numbers shown separately

**Given** the new main Hub
**When** a Manager opens System → Backup → "Set aside after promote or restore"
**Then** it lists each fork (time, old and new epoch, manager, forced or not) and, from any `orphaned/` database on this PC, the count of set-aside changes and every orphaned invoice number with amount and time, for the owner's accountant
**And** "Export list" writes it as CSV to the backup location

**Given** an `orphaned/` database older than 90 days
**When** daily housekeeping runs
**Then** it is deleted after its list has been written once into the audit as a system intent (invoice numbers and amounts only, no PII)

**Given** orphaned invoice numbers
**When** later invoices are issued
**Then** their numbers are never reused, because the new epoch writes a new invoice segment (Story 5.14)

### Story 9.22: A returning old main Hub locks itself

As a café owner,
I want the old main Hub to lock itself if it comes back after a promote,
So that exactly one Hub ever serves my kiosks.

**Covers:** FR171 (self-lock, never serves kiosks), AD-11, AD-38, AD-1, UX-DR102, UX-DR103

**Acceptance Criteria:**

**Given** an old main Hub that restarts or reconnects
**When** it sees a higher epoch in mDNS TXT records, in a device `hello`, from the new main, or in a heartbeat response fence
**Then** it closes its writer, stops advertising itself as the Hub, rejects every device `hello` with `FENCED`, and shows a full-screen lock "This PC is no longer the main Hub" with "Make this the standby"

**Given** the LAN is partitioned so the old main cannot see the new one
**When** its next heartbeat returns a higher fence
**Then** it self-locks the same way

**Given** neither LAN nor cloud reveals a higher epoch (fully isolated)
**When** a device that has seen the higher epoch connects
**Then** the device refuses the lower-epoch Hub, and the Hub self-locks on that device's `hello`

**Given** the locked screen
**When** a staff member interacts
**Then** only "Make this the standby", the audit viewer and diagnostics are available; nothing else is reachable and the lock survives a restart

**Given** the old main had committed rows after the fork point
**When** it locks
**Then** it keeps them untouched for reconciliation and its late batches upload under the old epoch (Story 9.21)

### Story 9.23: Make the old main Hub the standby

As a café manager,
I want to turn the old main PC into the new standby with one action,
So that the branch is protected again without reinstalling anything.

**Covers:** FR171 ("Make this the standby"), AD-11, AD-4, AD-8, UX-DR66

**Acceptance Criteria:**

**Given** the self-locked old main Hub
**When** a Manager taps "Make this the standby" and enters their PIN in place
**Then** the Hub moves its database to `orphaned/<epoch>-<timestamp>.db` (kept 90 days), asks the cloud to re-role its Hub device as `standby`, and shows the Story 9.16 "Link standby" step for the new main's 6-digit code

**Given** the branch already has a standby linked
**When** "Make this the standby" is tried
**Then** it is refused `STANDBY_EXISTS` with "Unlink the current standby on the main Hub first."

**Given** the link succeeds
**When** the standby starts
**Then** it bootstraps by LAN snapshot and follows (Story 9.17), and paired devices receive its fingerprint again

**Given** the WAN is down
**When** "Make this the standby" is tried
**Then** the re-role waits and the screen shows "Waiting for internet to confirm"; the PC stays locked and serves nothing meanwhile

### Story 9.24: Failover and split-brain proven on the test LAN

As a café owner,
I want promote, force promote and the old-Hub lock tested before every release,
So that a Hub failure on a busy night is a non-event.

**Covers:** NFR8, NFR9, NFR10, FR169, FR170, FR171, AD-5 (fenced results re-queued), AD-11, AD-38

**Acceptance Criteria:**

**Given** the failover harness (cloud binary + MinIO + main Hub + standby + 5 fake kiosks + a kitchen display) with sessions running
**When** the main Hub process is killed and the standby is promoted
**Then** the first intent is accepted under 10 s after PIN confirmation, every kiosk reconnects and its timer resumes from the Hub's record, and no committed row replicated before the kill is missing

**Given** the main Hub is frozen with its lease fresh
**When** force promote runs
**Then** it succeeds, the owner alert is queued, and the audit row has `force: true`

**Given** the old main comes back on the LAN, and in a second run only via WAN with the LAN partitioned
**When** it sees the higher epoch
**Then** it self-locks, serves no device, and its late batches are stored under the old epoch and never replayed into state

**Given** an owner command fetched and applied by the old main at a `seq` above the fork
**When** its result is posted with the old epoch
**Then** the relay rejects it `FENCED`, the command returns to `pending`, and the new main applies it exactly once

**Given** a promote with the cloud unreachable
**When** the WAN returns
**Then** the provisional epoch is confirmed by the first heartbeat, or (when a higher fence exists) the Hub self-locks

**Given** the release pipeline
**When** a release candidate is built
**Then** this suite runs before release and blocks it on any failure

### Story 9.25: Restore drill and privacy proof

As a café owner,
I want backups and erasure proven end to end before every release,
So that a restore works on the day I need it and an erased player stays erased everywhere.

**Covers:** FR200, FR173, FR174, FR199, NFR9, NFR13, AD-24, AD-36

**Acceptance Criteria:**

**Given** a seeded branch with 30 days of activity, one fork from a promote and two erased players
**When** the drill restores it (a) from a local export on a new PC with the released KEK and (b) from cloud backup
**Then** both restores reach a new epoch, branch-row checksums equal the source at the restore point, and both erased players have no key and no readable PII

**Given** a player erased after the export used in (a) on a paid branch
**When** restore (a) runs with the cloud reachable
**Then** that player is erased in the restored database before the writer opens

**Given** a promote after an erasure on the main Hub
**When** the standby becomes main
**Then** the erased player has no key on the new main

**Given** every batch, snapshot, key backup, GST archive, audit cache and export file produced by the drill
**When** the privacy scanner searches them for the seeded names, phones and emails
**Then** none appears in plaintext, and the Hub and cloud logs carry only player ids

**Given** the 1 GB reference database
**When** the export benchmark runs
**Then** export completes in under 10 s and the result is recorded in the release report

**Given** the release pipeline
**When** a release candidate is built
**Then** the drill runs before release and blocks it on any failure

## Epic 10: Subscriptions and tiers

Owners pay for, upgrade, cancel and lapse a subscription per branch without disruption, and a Free Android Hub can migrate to Windows when it goes paid.

> Draft notes: this epic builds on the Epic 1 `apply()`, change log, typed actors, inline PIN (Story 1.11) and audit; on the Epic 2 cloud (`arena-cloud` roles, `gz_control`, owner sign-in), the resumable wizard (Story 2.6), step 2 and its `entitlements` table, licence signing, the `arena-proto` tier table, the shared pricing table and the audited `ops grant-comp` command (Story 2.7), Hub pairing that refuses a Windows Hub on Free (Story 2.9), the heartbeat that re-signs the licence from the current entitlement and returns `cloud_time` (Story 2.10), licence verification, the entitlement API and `trusted_now` (Story 2.11), the device cap (Story 2.18) and Account → Branches (Story 2.29); on Epic 3 sessions and their invariants; on the Epic 4 Free limits (Story 4.9) and "Upgrading removes ads within one heartbeat, and a lapse brings them back only after expiry" (Story 4.15), which this epic now drives from real billing instead of comp grants; and on Epic 5 GST settings and money formatting. From Epic 8 it uses, by title: "The Owner app shell — navigation, themes, install and Free limits" (Account → Subscription route), "On Free, the Owner app shows only Subscription, pairing and support" (owner-side gates read `gz_control` entitlements), "Step-up MFA for sensitive commands", "Owner alerts by web push and WhatsApp, and the Alerts tab", "The cloud sends player and owner messages through a swappable messaging adapter, exactly once" and "A Cloudflare Tunnel for entitled branches, run by the Windows Hub" (reacts to the `tunnel` flag). From Epic 9 it uses, by title: "Upgrading from Free uploads a full snapshot before the first batch" (fires on the first paid licence; not rebuilt here), "One-tap full database export to a folder or USB drive" (the `.a360` format reused for the Android → Windows move), "Restore a branch from an export file as a new epoch", "Owner replaces a failed Hub and the new PC receives the branch key" (KEK release reused for the move) and "The cloud issues epoch counters by compare-and-set on the branch fence". **Decided (spine AD-10, AD-20, AD-29, AD-44):** Razorpay only; one Razorpay subscription per branch; monthly and yearly (yearly = 10× monthly, paid tiers only); renewals over ₹15,000 incl. GST use card or eNACH, never UPI Autopay; states `active → renewal_needs_approval → grace (7 days) → lapsed`; webhooks terminate in the cloud only and are applied idempotently by event id; paid features reach the Hub **only** through the licence token, and billing never sends gate-changing commands; a lapse or downgrade never ends running sessions or open orders and over-cap devices go read-only newest pairing first; 7-day refunds are issued by Arena 360 staff via Razorpay. **Admin UI is Epic 11:** this epic delivers the cloud refund capability and an audited `arena-cloud ops` path; Epic 11's admin app calls the same service. **Partner referral code:** not captured here — no FR in FR178–183 needs it; Epic 11 owns it (AD-19: immutable after first payment, which Epic 11 enforces against `subscriptions.first_paid_at` from this epic). **Owner-login add-on:** FR178 lists it, but the feature it pays for (FR161) is v1.1 (Epic 14); it is shown with its price and "Available soon" and cannot be bought in MVP. **Deferred (spine):** Razorpay Subscriptions coverage for Malaysia, Singapore and the US must be confirmed before the first non-India tenant; the MY/SG/US bands are built and tested against the Razorpay fake and test mode, and are switched on per band by a control-plane flag. Every Hub story adds its new intents to the Story 1.6 `proptest` generator.

### Story 10.1: The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans

As an Arena 360 operator,
I want every price kept once in a versioned price book and mirrored to Razorpay plans,
So that the wizard, the Owner app and every invoice charge the same amount, and a later price change never touches existing subscribers.

**Covers:** FR178 (prices), FR181 (bands), AD-20, AD-34 (adapter with recorded fake and manual mode), AD-7 (minor units, ISO 4217), AD-9, UX-DR93

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `price_books` (id, version, effective_from, status `draft|live|retired`) and `price_book_items` (price_book_id, band `IN|MY|SG|US`, currency, item `basic|premium|addon_devices_20|addon_tunnel|addon_owner_login`, cycle `monthly|yearly`, amount_minor, gst_applicable, razorpay_plan_id)
**And** the seed live book is:

| Band | Currency | Basic / month | Premium / month | +20 devices | Tunnel | Owner login |
| --- | --- | --- | --- | --- | --- | --- |
| IN | INR | ₹200 | ₹1,999 | ₹50 | ₹200 | ₹50 |
| MY | MYR | RM 23 | RM 229 | RM 6 | RM 23 | RM 6 |
| SG | SGD | S$8 | S$79 | S$2 | S$8 | S$2 |
| US | USD | $6 | $59 | $1.50 | $6 | $1.50 |

with every yearly amount exactly 10× monthly, Free absent (it is never billed), and `gst_applicable = 1` only for band IN

**Given** a book in `draft`
**When** an operator runs the audited `arena-cloud ops price-book publish --id <id> --reason <text>`
**Then** the Razorpay adapter creates one Razorpay plan per (band, item, cycle) with the book's amount and currency, stores each `razorpay_plan_id`, marks the book `live` and the previous book `retired`; a Razorpay error leaves the draft unpublished and nothing half-written

**Given** a published book
**When** anyone tries to change an amount in it
**Then** the API and ops command refuse with `PRICE_BOOK_IMMUTABLE`; prices change only by publishing a new version

**Given** the shared pricing table in the UI package (Story 2.7)
**When** the web apps build
**Then** it is generated from `GET /v1/pricing?band=` against the live book (with a committed snapshot for offline builds), and a CI check fails if the committed snapshot differs from the seed

**Given** the Razorpay adapter
**When** tests run
**Then** they use the recorded fake (plans, subscriptions, payments, refunds, webhooks); `billing.provider = manual` makes every Razorpay call a no-op that writes an ops task, so a Razorpay outage never blocks the rest of the cloud

**Given** a band whose `band_enabled` flag is off (MY, SG, US until Razorpay coverage is confirmed)
**When** pricing is requested for it
**Then** the API returns `BAND_NOT_AVAILABLE` and the wizard shows "Paid plans aren't available in your country yet — Free is" with Free still selectable

### Story 10.2: A branch subscription is created at a PPP band that is locked to it

As a café owner,
I want my branch's price fixed by my billing country when I subscribe,
So that I pay the local price I signed up for and a later price-table change never reprices me.

**Covers:** FR179 (one Razorpay subscription per branch, autopay, card/eNACH over ₹15,000), FR181, AD-20, AD-9, AD-40, NFR14, NFR23

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `subscriptions` (id, tenant_id, branch_id unique among non-ended rows, band, currency, price_book_id, tier, cycle, add_ons JSON with quantities, state, razorpay_subscription_id, mandate_method `upi_autopay|card|emandate`, current_period_start/end, first_paid_at, cancel_at_period_end, created_at, rev)

**Given** an owner calling `POST /v1/branches/:id/subscription {tier, cycle, add_ons, billing_country}` for a branch they own
**When** the branch has no live subscription
**Then** the cloud derives the band from `billing_country` (IN → IN, MY → MY, SG → SG, US → US; any other country refused `BAND_NOT_AVAILABLE`), copies the live `price_book_id` onto the row, creates the Razorpay subscription for the matching plans with add-ons, stores it in state `pending`, and returns the checkout parameters (key id, subscription id, allowed methods, amount, tax, total)

**Given** the renewal debit (tier + add-ons for the cycle, plus 18% GST on band IN) exceeds ₹15,000 (or the band's converted equivalent)
**When** the subscription is created
**Then** the allowed methods are card and eNACH only, and the response says "Renewals over ₹15,000 need a card or bank mandate"; at or below it UPI Autopay, card and eNACH are all offered (verified with Premium yearly ₹19,990 + GST = ₹23,588.20 → no UPI, Basic yearly → UPI allowed)

**Given** a subscription created on price book v1
**When** book v2 with higher prices is published later
**Then** renewals, prorations and invoices for that subscription keep using v1's amounts until the owner changes tier, cycle or add-ons, which re-prices only the changed items at the then-live book (and is shown before confirm)

**Given** the first successful payment
**When** Razorpay reports the payment method's country (card issuer or bank)
**Then** if it does not match the band, the subscription moves to `pending_review`, no entitlement is issued, ops are alerted and the owner sees "We're checking your payment — this usually takes a working day"

**Given** a branch that already has a live subscription, a branch the session doesn't own, or a tier `free`
**When** the endpoint is called
**Then** it returns `SUBSCRIPTION_EXISTS`, `FORBIDDEN` or `VALIDATION` and Razorpay is not called

**Given** a `pending` subscription with no payment
**When** 24 h pass
**Then** a jobs-role sweep cancels it at Razorpay and marks it `abandoned`, freeing the branch to start again

### Story 10.3: Razorpay webhooks are verified, applied once, and issue the entitlement

As an Arena 360 operator,
I want every Razorpay event checked and applied exactly once, with a sweep for anything missed,
So that a paid branch is entitled the moment its payment lands and never twice or never.

**Covers:** FR179 (activation, charges), FR6 (paid-tier entitlement issue), AD-20, AD-6 (webhooks terminate in the cloud), AD-10, AD-29, NFR11, NFR25

**Acceptance Criteria:**

**Given** `POST /v1/webhooks/razorpay`
**When** a request arrives
**Then** the `X-Razorpay-Signature` HMAC is verified against the webhook secret from SOPS before anything is parsed; a bad signature returns 400, is counted in the `webhook_failures` metric and writes nothing

**Given** a verified event
**When** it is handled
**Then** it is inserted into `billing_events` (razorpay event id unique, type, payload hash, received_at, applied_at) and applied in the same transaction; a duplicate event id returns 200 and applies nothing

**Given** `subscription.authenticated` followed by `subscription.charged` (or `subscription.activated`) for a `pending` subscription
**When** they are applied in any order
**Then** the subscription becomes `active` with the period from Razorpay, `first_paid_at` is set on the first charge, and the branch's `entitlements` row becomes source `subscription` with the tier and add-ons (replacing `free` or `comp`), written in the same transaction

**Given** a later `subscription.charged` for a renewal
**When** it is applied
**Then** the period moves forward and the state returns to `active` from `renewal_needs_approval` or `grace`; an out-of-order older event never moves the period back

**Given** a webhook that never arrives
**When** the jobs role runs its 10-minute reconcile
**Then** it fetches every subscription in `pending`, `renewal_needs_approval` or `grace` from Razorpay and applies the same transitions through the same code path, recording `source: reconcile`

**Given** the next heartbeat after activation
**When** the cloud re-signs the licence (Story 2.10)
**Then** the licence carries the paid claims, and the cloud never posts a command to change gates

**Given** every transition
**When** it is applied
**Then** a `subscription_history` row records old state, new state, cause (event id or job) and cloud time, and webhook handling time and failures appear in the observability metrics

### Story 10.4: Wizard step 2 and Upgrade take payment through Razorpay Checkout

As a café owner,
I want to pick Basic or Premium, monthly or yearly, pay once and have the step finish by itself,
So that setting up or upgrading a paid branch takes one payment and no waiting around.

**Covers:** FR6 (paid path completes when the entitlement is issued), FR178 (prices shown GST-exclusive, yearly 10×, add-ons), FR179 (autopay mandate), AD-20, NFR15, UX-DR24, UX-DR25, UX-DR84, UX-DR93, UX-DR103, UX-DR113

**Acceptance Criteria:**

**Given** wizard step 2 with billing enabled (the Story 2.7 flag turned on)
**When** the owner picks Basic or Premium
**Then** Continue is enabled, the card shows the price for the selected Monthly · Yearly tab in the branch band's currency with "+ 18% GST" on band IN only, yearly shows "2 months free", and an "Add-ons" panel lists +20 devices (quantity 0–10), Tunnel (Basic only; "Included" on Premium) and Owner login ("Available soon", not selectable)

**Given** the owner confirms
**When** the summary opens
**Then** it shows tier, cycle, add-ons, subtotal, GST 18% (band IN) and total, the renewal date, the refund line "Full refund within 7 days of your first payment. After that, cancel anytime; no partial refunds.", and a "Pay and continue" primary button

**Given** "Pay and continue"
**When** Razorpay Checkout opens with the Story 10.2 parameters
**Then** only the allowed methods are offered, the mandate is set up as part of payment, and closing Checkout returns to the summary with nothing changed

**Given** a successful payment
**When** Checkout returns
**Then** the page shows "Confirming your payment…" and, as soon as the Story 10.3 entitlement is written (server-sent events with a 5 s polling fallback), step 2 is marked done and the wizard moves to step 3 with focus on its heading, announced once

**Given** no confirmation within 2 minutes
**When** the wait ends
**Then** the page shows "Your payment is still being confirmed. We'll move you on as soon as it lands." and the wizard can be closed and resumed; it advances whenever the entitlement is issued

**Given** a failed or declined payment
**When** Checkout reports it
**Then** the summary stays with "Payment didn't go through — nothing was charged. Try again or use another method." and the same subscription can be retried

**Given** a `tier-lock` "Upgrade" link anywhere (Hub via owner link, Owner app) or Account → Branches "Add branch" on a paid tenant
**When** the owner follows it for a branch
**Then** the same tier picker, add-ons panel, summary and Checkout open in the Owner app for that branch, and success returns them to where they came from

**Given** step 3 for a paid branch
**When** it opens
**Then** the Windows Hub download is the primary choice and the Android link remains available

### Story 10.5: A paid licence reaches the Hub within one heartbeat and staff see the plan

As a café owner,
I want the Hub to switch to my paid plan as soon as I've paid, without a restart,
So that what I bought works straight away at the counter.

**Covers:** FR182 (upgrade within one heartbeat), FR14 (Windows Hub pairs on paid), AD-10, AD-29, AD-44, NFR11, NFR22, UX-DR24

**Acceptance Criteria:**

**Given** the tier table in `arena-proto`
**When** the cloud signs a licence from an entitlement with source `subscription`
**Then** claims follow the tier — Basic `limits {devices: 60 + 20×n, users: unlimited, branches: unlimited}`, `upload_interval_s = summary_interval_s = 900`, `retention_days = 365`, flags `owner_app`, `kiosk_orders`, `kitchen`, `standby`; Premium devices `200 + 20×n`, intervals 300, `retention_days = 1095`, plus `tunnel`, `live_view` — and the Tunnel add-on on Basic adds `tunnel` and `live_view`; `n` is the +20 devices quantity

**Given** a branch whose entitlement just became paid
**When** the Hub's next heartbeat returns the new licence
**Then** the Hub verifies and swaps it without a restart, every gate (`limit()`, `has_flag()`) reads the new claims for the next intent, `tier-lock`s on the Hub lift, and an audit row records the old and new tier with `actor{kind: system}`

**Given** the Hub header and System screen
**When** the tier changes
**Then** System shows "Plan: Basic · Monthly" (or the current tier and cycle) and the licence `exp` in branch time, and a one-time `hub-label` notice "Plan updated to Basic" appears in the alert list; nothing on the floor moves or loses focus

**Given** a branch with a paid entitlement
**When** a `platform: windows` Hub pairs (Story 2.9)
**Then** pairing is accepted; the `FEATURE_NOT_ENTITLED` refusal applies only to Free

**Given** the Hub's heartbeat interval (60 s when live)
**When** a test measures activation-to-gate
**Then** the paid licence is in force on the Hub within 70 s of the entitlement being written, verified on Windows and Android

**Given** a paid licence cached when the WAN drops
**When** the Hub runs offline
**Then** paid claims hold until `exp` on `trusted_now`; moving the wall clock forward or back does not change that

### Story 10.6: Every subscription charge gets a GST tax invoice from Arena 360

As a café owner,
I want a proper tax invoice for every payment, with GST shown separately,
So that I can claim input tax credit and keep my books straight.

**Covers:** FR179 (invoices itemise GST), FR178 (invoices), AD-7, AD-9, UX-DR96, NFR16

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `billing_invoices` (id, number, fy, subscription_id, branch_id, kind `invoice|credit_note`, currency, lines JSON, subtotal_minor, cgst_minor, sgst_minor, igst_minor, total_minor, buyer GSTIN, place of supply, issued_at, pdf_key) with `number` unique

**Given** a `subscription.charged` applied by Story 10.3
**When** the charge is for band IN
**Then** an invoice numbered `A360/<FY>/<seq:06>` (FY Apr–Mar in Asia/Kolkata, gap-free per FY) is issued with one line per item (tier, add-ons with quantity, cycle and period), subtotal, and GST computed once on the subtotal at 18% and rounded half-up to paise: CGST 9% + SGST 9% when the branch's state matches Arena 360's registered state, else IGST 18%

**Given** the branch has a GSTIN (Story 2.6 / 5.1)
**When** the invoice is issued
**Then** it shows the buyer's legal name, GSTIN and state code, Arena 360's GSTIN, address and SAC 998314, and the place of supply

**Given** a charge on band MY, SG or US
**When** the invoice is issued
**Then** it is in the band's currency with no GST lines and the note "Export of services — zero-rated under LUT"

**Given** an issued invoice
**When** the jobs role renders it
**Then** a PDF is written to the cloud's billing prefix in R2 (never under a branch prefix), the owner gets an "Invoice ready" message through the messaging adapter, and the invoice is immutable thereafter

**Given** the same charge event delivered twice
**When** it is applied
**Then** exactly one invoice exists for it

### Story 10.7: Account → Subscription shows each branch's plan, invoices and payment method

As a café owner,
I want one screen that shows what each branch is on, when it renews and every invoice,
So that I know exactly what I'm paying for and can find any invoice.

**Covers:** FR178 (Subscription per branch, invoices, payment method), FR181 (band shown), AD-40, NFR15, UX-DR24, UX-DR25, UX-DR90 (Account → Subscription), UX-DR93, UX-DR113, UX-DR116 (Subscription is spine-only)

**Acceptance Criteria:**

**Given** Account → Subscription in the Owner app
**When** it opens
**Then** each branch shows a `card` with branch name, tier, cycle, state as a `status-badge` (Active · Needs approval · Grace · Cancels on <date> · Lapsed · Free), add-ons with quantities, next renewal date and amount (+ GST on band IN), and the payment method (type and last 4 or bank)

**Given** a Free branch
**When** its card renders
**Then** it shows "Free · 1 branch · ads on the Android Hub" and an "Upgrade" primary button that opens the Story 10.4 checkout for that branch

**Given** "Invoices"
**When** the owner opens it
**Then** it lists every invoice and credit note across branches, newest first, filterable by branch and FY, each with number, date, branch, total and a "Download PDF" link served by a short-lived signed URL; a credit note is shown as a negative amount with its original invoice number

**Given** the Monthly · Yearly `tab-control`
**When** the owner views prices on this screen
**Then** it swaps every displayed price in place and is reflected in the URL, using the band's locked price book (Story 10.2), not the live one

**Given** a phone (< 768 px) and a desktop
**When** the screen renders
**Then** the cards stack in one column on phone and two on desktop with full parity, and all money uses the band's currency formatting

**Given** a screen reader
**When** the screen loads or a state changes
**Then** the screen has one `h1`, each card is a labelled region, and state changes are announced once in a polite live region

**Given** a request for another tenant's subscription or invoice
**When** it reaches the API
**Then** it returns `FORBIDDEN` and nothing is read

### Story 10.8: Change tier and add-ons mid-cycle with proration

As a café owner,
I want to move a branch up or down a tier or add and remove add-ons whenever I need,
So that I only pay for what I use, and an upgrade starts working today.

**Covers:** FR179 (mid-cycle changes prorated), FR178 (add-ons), FR182 (upgrade within one heartbeat), AD-20, AD-29, UX-DR93, UX-DR103

**Acceptance Criteria:**

**Given** an `active` subscription
**When** the owner chooses "Change plan" and picks a higher tier, or adds an add-on or raises the device add-on quantity
**Then** the summary shows the prorated charge for the rest of the period (new daily rate − old daily rate × days left, in minor units, + GST on band IN) and the new renewal amount, and confirming updates the Razorpay subscription immediately

**Given** the prorated charge succeeds
**When** Razorpay confirms it
**Then** the entitlement and licence change at once (applied within one heartbeat by Story 10.5), a Story 10.6 invoice is issued for the prorated amount, and the cloud logs any difference between its computed and Razorpay's charged amount to ops

**Given** a lower tier, an add-on removed or the device quantity lowered
**When** the owner confirms
**Then** the change is scheduled for the period end with the text "Changes on <date>. No partial refunds.", the card shows the pending change with "Keep current plan" to withdraw it, and the entitlement is unchanged until then

**Given** Tunnel on a Premium branch, or on a branch whose Hub is Android
**When** the add-ons panel renders
**Then** Tunnel shows "Included" on Premium, and on Android shows "Windows Hub only" and cannot be added

**Given** a change that takes the renewal debit over ₹15,000 incl. GST on a UPI Autopay mandate
**When** the owner confirms
**Then** the change is held and Razorpay Checkout opens with card and eNACH only to authorise a new mandate; nothing is charged and the plan is unchanged until that mandate exists, and cancelling Checkout leaves everything as it was

**Given** a subscription in `renewal_needs_approval`, `grace` or `pending_review`
**When** the owner opens "Change plan"
**Then** it is disabled with the inline reason "Settle the pending renewal first"

**Given** two changes submitted at once from two devices
**When** they reach the API
**Then** the second fails `STALE` on the subscription `rev` and the owner sees the current plan

### Story 10.9: Switch billing cycle and payment method, with card or eNACH for renewals over ₹15,000

As a café owner,
I want to switch a branch between monthly and yearly and change how I pay,
So that I can save with yearly billing and keep renewals going when my card or bank changes.

**Covers:** FR178 (billing-cycle switch, payment method), FR179 (card/eNACH over ₹15,000), AD-20, UX-DR25, UX-DR93

**Acceptance Criteria:**

**Given** an `active` monthly subscription
**When** the owner switches to Yearly
**Then** the summary shows the yearly amount (10× monthly + GST on band IN), a credit for the unused days of the current month, the net charged today, and the new renewal date one year out; on confirm the cloud replaces the Razorpay subscription with the yearly plan, keeps the same `subscriptions` row and band, and the entitlement never lapses during the swap

**Given** an `active` yearly subscription
**When** the owner switches to Monthly
**Then** the switch is scheduled for the end of the paid year with "Switches to monthly on <date>. No partial refunds."

**Given** a cycle switch whose new renewal debit exceeds ₹15,000 incl. GST
**When** the current mandate is UPI Autopay
**Then** Checkout opens with card and eNACH only to authorise the new mandate before the switch applies, and cancelling Checkout leaves the old cycle unchanged

**Given** "Change payment method"
**When** the owner opens it
**Then** Razorpay's mandate update flow opens with the methods allowed for the current renewal amount, and on success the card shows the new method; the old mandate is cancelled only after the new one is authorised

**Given** a mandate authorisation that fails or is abandoned
**When** the owner returns
**Then** the previous method stays in force and the card says "Payment method unchanged"

**Given** a webhook for the replaced Razorpay subscription arriving after a cycle switch
**When** it is applied
**Then** it is recorded in `billing_events` but cannot change the state of the current subscription

### Story 10.10: Renewals that need approval go to grace, then lapse, on cloud time

As a café owner,
I want clear warnings and a week's grace when a renewal can't go through,
So that a payment hiccup never surprises me or switches off my branch overnight.

**Covers:** FR179 (renewal states), FR182 (lapse path), AD-20 (`active → renewal_needs_approval → grace → lapsed`), AD-29, AD-44, NFR22, NFR25, UX-DR96, UX-DR103

**Acceptance Criteria:**

**Given** a renewal debit that needs customer authentication (a card or eNACH debit over ₹15,000, or a pre-debit notice the bank requires approved)
**When** Razorpay reports it
**Then** the subscription moves to `renewal_needs_approval`, the entitlement is unchanged, and the owner gets a web push and WhatsApp alert "Your <branch> renewal needs approval" with a link to approve in the Owner app

**Given** a renewal charge that fails (`subscription.pending`), or `renewal_needs_approval` not approved by the period end
**When** the period ends
**Then** the subscription moves to `grace` with `grace_until = period end + 7 days` on cloud time, the cloud keeps signing the paid licence, and the owner is alerted on day 1, day 5 and day 7 of grace

**Given** a branch in `grace`
**When** the owner pays (approve, retry or new method) and Razorpay confirms the charge
**Then** the subscription returns to `active` with the next period counted from the original renewal date, and no lapse ever happened

**Given** `grace_until` passes on cloud time, or Razorpay reports `subscription.halted`
**When** the jobs role runs (every 5 minutes)
**Then** the subscription moves to `lapsed`, the entitlement becomes source `free` with Free claims, and the next heartbeat delivers a Free licence

**Given** a Hub offline with a paid licence
**When** its branch lapses in the cloud
**Then** the Hub keeps the paid claims until the cached licence's `exp` on `trusted_now`, then applies Free limits; winding the PC clock back does not extend it

**Given** an owner opening the Owner app while a branch is in `renewal_needs_approval` or `grace`
**When** any screen loads
**Then** a persistent warning-coloured banner names the branch, the state and the date, with "Approve" or "Update payment method"; the Hub shows nothing to staff until the lapse applies

**Given** each state change
**When** it is applied
**Then** it is in `subscription_history`, and the observability metrics count subscriptions per state

### Story 10.11: A lapse or downgrade never ends a running session, and over-cap devices go read-only

As a counter staff member,
I want a plan drop to leave every running session alone and to tell me plainly which devices are over the limit,
So that players keep playing and I know which PCs I can still start.

**Covers:** FR182 (lapse never ends live sessions; over-cap devices read-only, newest pairing first), AD-29, AD-2, AD-44, NFR10, NFR22, UX-DR24, UX-DR97, UX-DR103

**Acceptance Criteria:**

**Given** a Hub that receives a lower licence (lapse to Free, Premium → Basic, or add-ons removed)
**When** it applies the licence
**Then** a system intent `entitlement.applied` recomputes device standing in one transaction: non-revoked devices counted by Story 2.18's rules are ordered by `paired_at`, the oldest up to `limits.devices` stay active, and the rest are marked `over_cap` (newest pairing first); an audit row lists each device marked

**Given** a device with an active, paused or grace session when it becomes `over_cap`
**When** the session continues
**Then** extend, pause, transfer away and end all work as before; only when the session ends does the device lock to "This PC is over the plan's device limit — ask staff."

**Given** an `over_cap` device
**When** staff try to start a session on it, or transfer a session onto it
**Then** `apply()` rejects with `DEVICE_OVER_CAP` and the tile shows `tier-lock` "Over plan limit" with Upgrade; the tile stays visible on the floor

**Given** a device is revoked or the plan rises again
**When** the next licence or revocation is applied
**Then** standing is recomputed and the oldest `over_cap` devices become active first, without staff action

**Given** a paid licence held offline past its `exp`
**When** `trusted_now` passes `exp`
**Then** the same recompute runs against Free limits; winding the wall clock back never undoes or delays it

**Given** the `proptest` generator
**When** licence changes are interleaved with session intents
**Then** no session is ever ended or changed by `entitlement.applied`, and the count of active devices never exceeds `limits.devices` for new starts

### Story 10.12: A lapse or downgrade keeps open orders, signed-in staff and the main Hub working

As a café manager,
I want open orders, signed-in staff and the Hub itself to carry on when the plan drops,
So that a lapse changes what we can start next, not what is already happening.

**Covers:** FR182 (lapse never ends open orders), AD-29, AD-2, AD-11, NFR10, UX-DR24, UX-DR103

**Acceptance Criteria:**

**Given** open kiosk orders or kitchen tickets when `kiosk_orders` or `kitchen` drops
**When** the lower licence applies
**Then** open orders and tickets still move to served or are cancelled as usual and the Kitchen display keeps showing them until the last open ticket is served; new kiosk orders are refused `FEATURE_NOT_ENTITLED`, the kiosk Shop tab hides ordering, and new KOTs are not created

**Given** more active staff than `limits.users` after the change
**When** the licence applies
**Then** anyone signed in stays signed in; new sign-ins are allowed for the Owner and the most recently active users up to the cap, others see "This branch's plan allows <n> users — ask the owner."

**Given** a Windows Hub whose branch lapses to Free
**When** the Free licence applies
**Then** it keeps running at Free limits with no ads and no lock

**Given** a standby Hub when the `standby` flag drops
**When** the standby receives the licence
**Then** it stops following and shows "Standby needs a paid plan"; the main Hub is unaffected and promote is refused `FEATURE_NOT_ENTITLED`

**Given** the plan rises again
**When** the next licence applies
**Then** kiosk ordering, KOTs, the user cap and standby following return without restarting anything

**Given** the `proptest` generator
**When** licence changes are interleaved with order and staff intents
**Then** no open order is ever ended or changed by `entitlement.applied`

### Story 10.13: Cancel keeps the plan to the end of the period, then the branch drops to Free

As a café owner,
I want to cancel a branch's plan and keep what I've paid for until the period ends,
So that cancelling is simple and fair, and I know exactly when it stops.

**Covers:** FR180 (cancel keeps the plan to period end, then Free; no partial refunds), FR178 (cancel), AD-20, AD-29, UX-DR93, UX-DR103, UX-DR112

**Acceptance Criteria:**

**Given** an `active` subscription more than 7 days after its first payment
**When** the owner chooses "Cancel plan"
**Then** a panel shows the policy text "Your plan stays active until <period end>. After that this branch moves to Free. No partial refunds." and what changes on Free (users 3, devices 20, no owner dashboards, no cloud backup, ads on the Android Hub), and asks for step-up MFA before "Cancel plan" completes

**Given** the cancellation is confirmed
**When** the cloud applies it
**Then** Razorpay is told to cancel at cycle end, `cancel_at_period_end = 1`, the card shows "Cancels on <date>" with "Keep my plan", and the entitlement and licence are unchanged

**Given** "Keep my plan" before the period end
**When** the owner confirms
**Then** the cancellation is withdrawn at Razorpay and the subscription stays `active` with the same renewal date

**Given** the period end on cloud time for a cancelled subscription
**When** the jobs role runs
**Then** the subscription becomes `ended` (no grace), the entitlement becomes source `free`, and the next heartbeat delivers a Free licence handled by Stories 10.11–10.12

**Given** the tenant has another branch already on Free
**When** a second branch's cancellation ends
**Then** that branch still runs at Free limits, shown as "Lapsed — Free limits" with "Resubscribe", and the one-Free-branch rule for new branches is unchanged

**Given** a cancelled branch
**When** the owner resubscribes later
**Then** a new subscription is created at the then-live price book for the tenant's band (Story 10.2)

### Story 10.14: Full refund within 7 days of the first payment, issued through the ops path

As a café owner who changed my mind,
I want my first payment refunded in full if I cancel within 7 days,
So that I can try a paid plan without risk.

**Covers:** FR180 (full refund within 7 days of first payment; no partial refunds), AD-20 (refunds by admin via Razorpay), AD-21 (admin actions audited), AD-19 (refund visible to commission), UX-DR93, UX-DR96

**Acceptance Criteria:**

**Given** an `active` subscription within 7 days (on cloud time) of `first_paid_at`
**When** the owner chooses "Cancel plan"
**Then** the panel offers "Cancel and get a full refund of <amount>", stating the branch moves to Free at once, and after step-up MFA a `refund_requests` row (subscription, payment id, amount, requested_at, status `requested`) is written and the owner sees "Refund requested — usually processed within 2 working days"

**Given** a request in `requested`
**When** an Arena 360 operator runs the audited `arena-cloud ops refund approve --request <id> --reason <text>` (the service Epic 11's admin app will call)
**Then** the Razorpay adapter issues a full refund of every payment on that subscription, cancels it immediately, and the request moves to `issued`; a Razorpay error leaves it `requested` with the error shown to the operator

**Given** `refund.processed` from Razorpay
**When** it is applied
**Then** the subscription becomes `refunded`, the entitlement becomes source `free` (the next heartbeat delivers a Free licence), a credit note `A360/<FY>/CN<seq:06>` reversing each invoice is issued, and the owner is told "Refund of <amount> processed"

**Given** a refund request after the 7-day window, a second request, or any partial amount
**When** it reaches the API or ops command
**Then** it is refused `REFUND_NOT_ALLOWED`; after 7 days the cancel panel shows only the Story 10.13 flow

**Given** an operator rejecting a request with `ops refund reject --reason`
**When** it completes
**Then** the owner sees the reason and the subscription stays `active`

**Given** any refund action
**When** it is recorded
**Then** the admin audit holds operator, reason and amounts, and the refund is stored against the subscription so partner commission can be computed net of refunds

### Story 10.15: Owner starts moving a Free Android Hub to a Windows Hub after upgrading

As a café owner who has just upgraded,
I want to move my branch from the Android tablet to a Windows PC with a code,
So that I get standby and Tunnel without retyping anything.

**Covers:** FR183 (start of the move), AD-10, AD-22, AD-23, UX-DR85, UX-DR103

**Acceptance Criteria:**

**Given** a paid branch whose Hub is Android
**When** the owner opens its Subscription card or Account → Branches
**Then** a "Move to a Windows Hub" action explains the steps (install on the PC, type the code, confirm on the tablet) and that sessions keep running; on Free the action is `tier-lock` "Available on Basic"

**Given** the owner starts the move
**When** the cloud creates it
**Then** a `hub_migrations` row (branch, from_hub, state `awaiting_windows`, created_at) and a `pairing_codes` row with purpose `migrate` are written, and the Story 2.8 `pairing-code` card shows the 8-character code with the Windows download

**Given** a fresh Windows Hub install
**When** it pairs with a `migrate` code
**Then** pairing returns the token and licence as usual, no `branch.bootstrap` is queued, the Android Hub's token stays valid, and the Windows Hub opens a "Waiting for your Android Hub" screen serving no devices; the migration moves to `awaiting_handover`

**Given** `awaiting_handover`
**When** the Android Hub's next heartbeat arrives
**Then** the response carries `migration {id, target_hub_name}` and the Android Hub shows staff a persistent "Move this branch to <PC name>" item in its alert list

**Given** a code that is wrong, expired or used, or a `migrate` code typed into an Android install
**When** pairing is attempted
**Then** the Story 2.9 errors apply unchanged, and an Android install is refused `FEATURE_NOT_ENTITLED` for a `migrate` code

**Given** the owner cancels the move before handover
**When** the cloud applies it
**Then** the Windows Hub's token is revoked, the migration is `cancelled`, and the Android Hub clears the alert at its next heartbeat

### Story 10.16: The Android Hub hands over a final snapshot and steps down

As a café manager at the counter,
I want to confirm the move on the tablet and see it hand everything to the new PC,
So that nothing written on the tablet is left behind.

**Covers:** FR183 (snapshot export with no data loss), AD-1, AD-8 (Android→Windows snapshot format in `arena-proto`), AD-11, AD-42, AD-30, NFR9, NFR10, UX-DR66

**Acceptance Criteria:**

**Given** the "Move this branch to <PC name>" item on the Android Hub
**When** an Owner or Manager opens it and confirms with `inline-pin`
**Then** the Android Hub shows a 6-digit handover code and the Windows Hub's "Waiting" screen asks for it; both are on the same LAN (found by mDNS or typed IP)

**Given** the Windows Hub enters the correct code
**When** the handover starts
**Then** the Android Hub stops accepting new intents (`HUB_HANDING_OVER`, kiosks show their Epic 6 reconnecting state and count down on `lock_after_ms`), waits for the writer queue to drain, and writes a final snapshot in the Story 9.4 `.a360` format with `purpose: migrate`, last `seq` and epoch in the header

**Given** the snapshot
**When** it is transferred
**Then** the Windows Hub pulls it over the LAN on the device-token TLS channel and checks sha256, branch id and last `seq`; on success the Android Hub enters "This branch has moved to <PC name>" (read-only, serving nothing) and reports `migration.handed_over` with the last `seq` in its next heartbeat

**Given** the LAN transfer fails, or the tablet and PC cannot see each other
**When** the manager chooses "Use a file instead"
**Then** the Android Hub writes the same snapshot to its backup folder (Story 4.7) for copying to the PC, and stays stopped until the Windows Hub confirms import or the manager chooses "Cancel move"

**Given** "Cancel move" before the Windows Hub confirms import
**When** the manager confirms with PIN
**Then** the Android Hub resumes as main with the same epoch and nothing set aside

**Given** a wrong handover code 5 times
**When** the sixth attempt arrives
**Then** the code is invalidated and a new one shown, as for kiosk pairing

### Story 10.17: The Windows Hub imports the snapshot as the next epoch

As a café owner,
I want the new Windows Hub to carry on exactly where the tablet stopped,
So that the move loses no data and no session.

**Covers:** FR183 (import with no data loss), AD-11, AD-38 (epoch by fence CAS), AD-24 (KEK release), AD-29, NFR8, NFR9, NFR10, UX-DR103

**Acceptance Criteria:**

**Given** a verified `purpose: migrate` snapshot on the Windows Hub (pulled over the LAN or picked from a file)
**When** it imports
**Then** it loads the database, migrates the schema forward if older, claims the next epoch through "The cloud issues epoch counters by compare-and-set on the branch fence" with `fork = {snapshot epoch, last seq}`, and nothing is set aside because the snapshot is the final state

**Given** the branch KEK
**When** the Windows Hub needs it to read `player_keys`
**Then** the owner approves "Release branch key to <PC name>" with step-up MFA as in "Owner replaces a failed Hub and the new PC receives the branch key", and the Hub stores it in DPAPI

**Given** the Windows Hub opens its writer
**When** it reports `migration.imported` in its heartbeat
**Then** the cloud revokes the Android Hub's token, marks the migration `done`, records the Windows Hub as the branch's Hub, and the Android Hub at its next heartbeat (or at once on the LAN) shows "This branch has moved" permanently

**Given** kiosks paired to the Android Hub
**When** the Windows Hub is serving
**Then** each kiosk shows "Pair again" and pairs with a Hub code (Story 2.17); its running session is found by device and resumes

**Given** sessions that were running during the move
**When** the Windows Hub serves the kiosks
**Then** every session continues with the same start time, balance and remaining time, and the time the Hub was unreachable is counted as normal session time

**Given** row counts and checksums of the Android database at the final `seq` and of the Windows database after import
**When** they are compared
**Then** every branch row matches, and the first batch on a paid branch follows "Upgrading from Free uploads a full snapshot before the first batch" if it has not yet run

### Story 10.18: Kiosks follow the new Windows Hub without pairing again

As a café manager,
I want the kiosks to move to the new Windows Hub by themselves,
So that I don't walk to every PC and pair it again after the move.

**Covers:** FR183 (move without disruption), AD-42 (paired-device class, pinned Hub certificate), AD-25, AD-12, NFR8, NFR11, UX-DR98, UX-DR103

**Acceptance Criteria:**

**Given** a handover that has started (Story 10.16), with the Windows Hub sending its LAN address and certificate fingerprint to the Android Hub along with the handover code
**When** the Android Hub stops accepting intents
**Then** it sends each connected kiosk `hub.successor {address, cert fingerprint, branch_id, expires_at = now + 24 h}` signed with its own pinned key, and each kiosk stores it beside its current pin

**Given** a kiosk holding a valid `hub.successor`
**When** its current Hub reports "This branch has moved" or stays unreachable
**Then** the kiosk connects to the successor address, accepts only the named fingerprint, presents its existing device token, and on success replaces its pin and drops the old one

**Given** the Windows Hub (after Story 10.17 import)
**When** a kiosk presents a device token from the imported database
**Then** it is accepted under the paired-device class, and the kiosk's running session resumes with its Epic 6 reconnect-and-reconcile

**Given** a `hub.successor` with a bad signature, a different branch id, an expired `expires_at`, or a fingerprint mismatch at connect
**When** the kiosk checks it
**Then** it is ignored, the kiosk keeps its old pin, and an audit row is written at the Hub it next reaches

**Given** a kiosk that was offline during the handover
**When** it comes back
**Then** it falls back to "Pair again" (Story 10.17)

**Given** a move with 20 kiosks on the test LAN
**When** the import completes
**Then** every online kiosk is connected to the Windows Hub within 30 s without re-pairing

### Story 10.19: Billing, tier changes and the Hub move proven end to end

As an Arena 360 operator,
I want the full subscription life cycle and the Android → Windows move tested before every release,
So that billing never stalls a café and no plan change breaks a running branch.

**Covers:** FR178–FR183, AD-20, AD-29, AD-44, NFR1, NFR10, NFR22

**Acceptance Criteria:**

**Given** the Razorpay fake with a controllable cloud clock
**When** the billing suite runs
**Then** it walks each band through create → activate → upgrade with proration → cycle switch → renewal needs approval → grace → recovery, grace → lapse, cancel → period end → Free, and 7-day refund → credit note, asserting states, entitlements, licences and invoice totals at each step, including duplicate and out-of-order webhooks

**Given** Razorpay test mode on staging
**When** the nightly run executes
**Then** a Basic monthly UPI Autopay, a Premium yearly card and a Premium yearly eNACH subscription are created, charged and refunded, and UPI is never offered for the Premium yearly plan

**Given** the internal 10-PC test café with 10 live sessions and 2 open kitchen tickets
**When** the branch is downgraded from Premium to Basic and then lapsed to Free
**Then** no session or ticket ends, the newest-paired devices beyond 20 become over cap only after their sessions end, and ads appear on the Android Hub only after the paid licence's `exp` on `trusted_now`

**Given** a Free Android Hub with 30 days of seeded history and 5 live sessions
**When** it upgrades and moves to a Windows Hub over the LAN, and again by file
**Then** both moves finish with checksums equal at the final `seq`, every session continues, and the kiosks reconnect without re-pairing

**Given** the release pipeline
**When** a release candidate is built
**Then** these suites run and block the release on any failure

## Epic 11: Marketing site, partner programme and admin operations

Prospects find and sign up through the public site, partners earn and track commission, and Arena 360 staff curate the catalogue, approve partners and support branches.

> Draft notes: this epic builds on the Epic 1 scaffold (Story 1.1: the Astro 7 marketing skeleton, the partner-portal and admin SvelteKit skeletons on Workers static assets, the shared UI package and message catalogue), tokens with the contrast gate (Story 1.2) and shared base components (Story 1.3); on the Epic 2 cloud and sign-up (Stories 2.1–2.3: `arena-cloud` roles, `gz_control`, per-IP and per-phone rate limits, the messaging adapter used for OTP, telemetry scrubbing), the `arena-proto` tier table, licence signing and the audited `arena-cloud ops grant-comp` command (Story 2.7), heartbeats (Story 2.10), the Hub pairing that refuses a Windows Hub on Free (Story 2.9) and `onboarding_progress` (Stories 2.6, 2.30); on the Hub `tracing` ring buffer and `PRAGMA integrity_check` (Story 1.4) and Settings → System (Story 2.25); on the Epic 4 ad metrics (`ad_impressions`); on the Epic 6 catalogue tables, manifest API and `arena-cloud catalog import` (Story 6.1 — this epic adds only the curation UI and the screening gate; the manifest's `flash_screened` filter already exists); on Epic 8 by title: "The cloud sends player and owner messages through a swappable messaging adapter, exactly once" (partner WhatsApp and email go through it), "The cloud accepts owner command kinds, checks them, and fans "all branches" out" and "The Hub applies owner commands exactly once, and the owner's change wins" (the `diagnostics.request` command kind is added to that catalogue), "Presign issues batch and summary upload URLs under the branch's own R2 prefix" (diagnostics upload) and "The builder serves audit queries from a local cache" (support audit search with a recorded reason already enforced at the API); and on Epic 10 by title: "The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans" (generated pricing table used by the Pricing page), "Razorpay webhooks are verified, applied once, and issue the entitlement" (`subscriptions.first_paid_at`), "Every subscription charge gets a GST tax invoice from Arena 360" (`billing_invoices`, GST-exclusive `subtotal_minor`, credit notes) and "Full refund within 7 days of the first payment, issued through the ops path" (the refund service and `arena-cloud ops refund approve|reject`, which the admin app calls). **Decided (spine AD-15, AD-17, AD-18, AD-19, AD-21, AD-40):** marketing is Astro rendering shared Svelte components, deployed to Cloudflare Workers static assets (not Pages); partner portal at `partners.arena360.cloud` is a separate identity with its own login and API scope and never reads tenant operational data; the payout job runs on the 1st in Asia/Kolkata business time, pays 10% of referred tenants' paid GST-exclusive subscription revenue net of refunds, rolls balances under ₹500 over, deducts 2% TDS, claws refunds back in the next payout, and staff transfer manually and mark paid; KYC (PAN, bank, optional GSTIN) is encrypted and admin-read only; the admin app sits behind Cloudflare Access (staff SSO) and every admin action is written to `admin_audit` in `gz_control` with `actor.kind = admin`; support reads branch data only through the diagnostics bundle and the audit builder with a recorded reason. **One audit store:** the `admin_audit` table already written by the `arena-cloud ops` commands, the catalogue import (Story 6.1) and the audit builder (Story 8.35) is reused; admin-app actions and `ops` commands call the same services so CLI and UI never diverge. **Out of this epic:** the in-browser demo Hub, Compare, Status and changelog pages (Epic 14, v1.1), automated payout transfers (spine Deferred), and partner co-branding (logo-only branch customisation stays as is). English only (NFR16).

### Story 11.1: Marketing site shell — navigation, footer and light and dark themes

As a prospective café owner,
I want a fast public site that reads well on my phone and laptop, in light or dark,
So that I can find what Arena 360 does and how to start without fighting the page.

**Covers:** FR196 (site shell), AD-15, NFR15, NFR16, NFR26, UX-DR5, UX-DR6, UX-DR11, UX-DR13, UX-DR17, UX-DR19, UX-DR20, UX-DR91 (header, phone nav, footer), UX-DR113, UX-DR114

**Acceptance Criteria:**

**Given** the Astro 7.4 + `@astrojs/svelte` 9.1 project from Story 1.1
**When** it builds
**Then** every page is prerendered to static HTML and deployed to Cloudflare Workers static assets at `arena360.cloud` (staging at `staging.arena360.cloud`), with interactive parts (theme toggle, phone nav, tab controls) as Svelte islands from the shared UI package, and Sora served from the site itself with no third-party font or CDN fetch

**Given** a desktop width (≥ 1024 px)
**When** any page opens
**Then** an 80 px header shows the wordmark (link to Home), Product · Pricing · Downloads · Help · FAQ and one `button-primary` "Get started", with the content on the web gutter (56 px) and section gap (128 px) tokens

**Given** a phone width (< 768 px)
**When** any page opens
**Then** the header is 64 px with a hamburger and "Get started" always visible; the hamburger opens a full-screen nav (Product · Pricing · Downloads · Help · FAQ) that traps focus, closes on Esc or a link, and returns focus to the hamburger; the header stays sticky while scrolling

**Given** any page
**When** it renders
**Then** the footer holds links to Legal (Terms, Privacy, Data processing, Ad policy), Help, FAQ, Downloads and `theme-toggle` System · Light · Dark; System follows `prefers-color-scheme` live, a chosen mode is remembered per device, and an inline head script applies it before first paint so the page never flashes the wrong theme

**Given** light mode
**When** a page renders
**Then** it uses the "Cool paper" tokens, links in body text are underlined in `accent-text-light`, and full-bleed media sections keep the dark tokens and scrim

**Given** any page
**When** an axe scan runs in both themes at 390 px and 1440 px
**Then** it has `header`, `nav`, `main` and `footer` landmarks, one `h1`, labelled controls, ≥ 44 px targets, a skip-to-content link, zero serious or critical violations, and no horizontal scroll at 390 px with strings lengthened by 30%

**Given** the site build
**When** CI runs
**Then** every user-facing string comes from the message catalogue (the hard-coded-string lint passes), each page has a title, meta description, canonical URL and Open Graph tags, `sitemap.xml` and `robots.txt` are generated, and an unknown path returns a 404 page in the same shell with links to Home and Help

### Story 11.2: Home tells the install, pair, play story

As a prospective café owner,
I want the first page to tell me plainly what I get and how fast I can start,
So that I believe it runs my café offline and costs less than what I use now.

**Covers:** FR196 (Home), NFR26, UX-DR11, UX-DR91 (Home sections), UX-DR103 (player register), UX-DR115 (key-web-marketing-home), UX-DR116 (hero on `web-hero`, light theme)

**Acceptance Criteria:**

**Given** Home at ≥ 1024 px
**When** it opens
**Then** the hero shows the promise "Run your café offline. See it from anywhere. Pay less than you do now." in `web-hero` 64/200 on the left in the scrim zone, a short supporting line and the "Get started" primary, and a dark Hub floor-view product shot on the right; below 1024 px the title uses `web-display` and the shot stacks under the copy

**Given** the rest of Home
**When** it scrolls
**Then** sections follow in order: "Install. Pair. Play." three steps (install the Hub, pair your PCs with a code, start a session), "What you get" three cards (works with the internet down, owner app from anywhere, per-branch price with Free to start), a closing "Get started" band, then the footer, with sections 128 px apart (64 px on phone)

**Given** a hero background video
**When** the page loads
**Then** it is muted with no audio track, loops only while visible, stops when `prefers-reduced-motion` is set, and falls back to its still cover and then solid `bg` if it cannot play; no element uses `backdrop-filter` or live blur

**Given** the light theme
**When** Home renders
**Then** the hero and media bands stay dark with the scrim while the step and card sections use "Cool paper"

**Given** the page on a mid-range phone over a throttled 4G profile
**When** a Lighthouse CI run executes
**Then** Largest Contentful Paint is under 2.5 s and the performance and accessibility scores are each at least 90, and the build fails otherwise

### Story 11.3: Pricing — Cafés tab with Monthly · Yearly, equal plan cards, add-ons and FAQ

As a prospective café owner,
I want every price shown plainly with GST stated and the refund rule up front,
So that I know exactly what I'd pay before I sign up.

**Covers:** FR196 (Pricing, Cafés), FR178 (displayed prices), FR180 (refund FAQ), UX-DR25, UX-DR91 (Pricing), UX-DR93, UX-DR103 (money/legal register), UX-DR115 (key-web-pricing), UX-DR116 (toggle, GST note, add-ons, refund FAQ, no rewarded ads)

**Acceptance Criteria:**

**Given** Pricing
**When** it opens
**Then** a `tab-control` Cafés · Partners sits under the title with Cafés selected, the selected tab is in the URL (`/pricing?tab=cafes`), arrow keys move between tabs, and Partners shows "Partner programme details coming soon" until Story 11.11

**Given** the Cafés tab
**When** it renders
**Then** one primary "Get started" sits above a Monthly · Yearly `tab-control` (also reflected in the URL), followed by three equal tier `card`s with no emphasised plan: Free "₹0 · 1 branch · ads on the Android Hub", Basic "₹200/branch/month + 18% GST", Premium "₹1,999/branch/month + 18% GST", each card's secondary action "Choose <tier>" leading to Get started with the tier pre-selected

**Given** Yearly is selected
**When** prices swap
**Then** Basic shows "₹2,000/branch/year + 18% GST" and Premium "₹19,990/branch/year + 18% GST", each with "2 months free", Free is unchanged, and the change is announced to screen readers

**Given** the prices
**When** the site builds
**Then** every amount comes from the Epic 10 generated pricing table for band IN (never typed into page copy), and a CI check fails if a displayed amount differs from the price book snapshot

**Given** the comparison table and add-ons
**When** they render
**Then** the table lists each tier's limits from the `arena-proto` tier table (branches, devices, users, owner dashboards, cloud backup and retention, standby, Tunnel, kitchen display, ads), and add-ons list "+20 devices ₹50", "Tunnel for remote devices ₹200" and "Additional owner login ₹50 — available soon", each per branch per month + 18% GST; no rewarded ads appear anywhere

**Given** the Pricing FAQ
**When** it renders
**Then** it answers what happens offline, how cancellation works and the refund rule in the exact text "Full refund within 7 days of your first payment. After that, cancel anytime: your plan stays active to the end of the paid period, then the branch drops to Free. No partial refunds.", as an accessible disclosure list

**Given** a visitor at 390 px
**When** the page renders
**Then** the cards stack in one column in the same order and the comparison table scrolls inside its own container with the tier column pinned, so the page itself never scrolls sideways

### Story 11.4: Product pages for Hub, Kiosk, Console and Owner with demo videos

As a prospective café owner,
I want to see each part of Arena 360 working before I install anything,
So that I can judge whether it fits my counter, my PCs and my consoles.

**Covers:** FR196 (Product), NFR26, UX-DR5 (media stays dark), UX-DR91, UX-DR113, UX-DR116 (spine-only marketing page)

**Acceptance Criteria:**

**Given** Product
**When** it opens
**Then** it has an overview and one section each for Hub, Kiosk, Console and Owner, reachable as anchors (`/product#hub` etc.) from an in-page link list, each with an illustrated walkthrough of three to five steps and a 90-second demo video

**Given** a demo video
**When** the page loads
**Then** it shows its poster still and does not play until pressed; it plays with sound only after the press, has WebVTT English captions on by default and a text transcript link, and is served from the CDN under the shared `marketing/` R2 prefix

**Given** a video that fails to load or a browser that cannot play it
**When** the section renders
**Then** the poster still remains with "Video unavailable — read the walkthrough below", never a broken player

**Given** the Console section
**When** it renders
**Then** it says plainly that console time is run from the Hub tile at launch and that the TV and QR self-start arrive with Pi support, without promising a date

**Given** keyboard and screen-reader users
**When** they operate a video
**Then** play, pause, captions and volume are native or labelled controls reachable by Tab, and pressing play moves no focus

### Story 11.5: Downloads with installers, system requirements and checksums

As a café owner setting up,
I want to download the right installer and check it is genuine,
So that I install the real Arena 360 on hardware that will run it.

**Covers:** FR196 (Downloads), AD-32 (signed installers), NFR17, NFR18

**Acceptance Criteria:**

**Given** the signed-installer prefix in R2 (AD-32)
**When** the release pipeline publishes a release
**Then** it also writes `releases/manifest.json` (product, channel `stable`, version, file name, size, SHA-256, signature URL, published_at), and the Downloads page reads it at build time and again in the browser so a new release appears without a site redeploy

**Given** Downloads
**When** it opens
**Then** it lists Windows Hub, Windows Kiosk (installer for each gaming PC), Android Hub (a Google Play link, not an APK) and, when present in the manifest, the Raspberry Pi image, each with version, size, release date, SHA-256 in a copyable monospace field and a "How to verify" disclosure showing the PowerShell `Get-FileHash` command

**Given** a product missing from the manifest (for example the Pi image before console TV support ships)
**When** the page renders
**Then** its row reads "Coming soon" with no download link

**Given** the system requirements section
**When** it renders
**Then** it states Kiosk: Windows 10 1803 to 11, 64-bit, 4 GB RAM, WebView2 Evergreen; Windows Hub: Windows 10 or later; Android Hub: Android 9+, 3 GB RAM, 20 GB free; "Windows 7 is not supported"; any ESC/POS printer; and links to the Help centre compatibility lists

**Given** the manifest fetch fails in the browser
**When** the page is open
**Then** the build-time list stays shown with "Showing the list from <date>", and no link points to a file whose checksum is not shown

### Story 11.6: Help centre with setup guides, troubleshooting and compatibility lists

As a café owner or installer,
I want setup steps, fixes for common problems and a list of hardware that is known to work,
So that I can get unstuck without waiting for support.

**Covers:** FR196 (Help centre), NFR18 (compatibility list, "tested device" badge), UX-DR113, UX-DR116 (spine-only marketing page)

**Acceptance Criteria:**

**Given** the Help centre
**When** it opens
**Then** it shows three categories — Setup, Troubleshooting, Hardware compatibility — and a search box, with articles authored as Markdown content collections in the repo, each with a title, summary, last-reviewed date and stable slug

**Given** the launch content set
**When** the site builds
**Then** Setup holds at least: install the Hub (Windows, Android), pair a kiosk, set up a printer and cash drawer, add a standby PC, invite staff; Troubleshooting holds at least: kiosk can't find the Hub, printer offline, "Hub slow", branch shows offline in the owner app, restoring from an export; and the build fails if an internal link points to a missing slug

**Given** Hardware compatibility
**When** it renders
**Then** printers and HDMI-CEC TVs are listed from a versioned data file (brand, model, connection USB/LAN/Bluetooth or CEC, result) with a "Tested device" badge on rows verified by the Arena 360 test rig, a note that CEC is best-effort and a smart plug is the fallback, and the table is filterable by brand and connection

**Given** a search term
**When** the visitor types
**Then** a static search index built at deploy returns matching article titles and summaries within 200 ms with no server call, and "No articles match — try Troubleshooting or contact support" when nothing matches

**Given** any article
**When** it ends
**Then** it shows the support channel for the visitor to use (community and help centre on Free, email within 24 h on Basic, chat within 4 h on Premium) without asking for any personal data on the page

### Story 11.7: FAQ and Legal pages

As a prospective café owner,
I want straight answers on billing, data, ads and refunds, and the terms I'm agreeing to,
So that nothing in the contract surprises me later.

**Covers:** FR196 (FAQ, Legal), FR180 (refund text), FR201 (policy version), UX-DR103 (money/legal register), UX-DR113

**Acceptance Criteria:**

**Given** FAQ
**When** it opens
**Then** it groups answers under Billing, Data ownership, Offline, Ads and Refunds as an accessible disclosure list, with the refund answer using the same text as the Pricing FAQ (one catalogue key), and the page emits `FAQPage` structured data

**Given** the Data ownership answers
**When** they render
**Then** they state that the full branch database and every report can be exported at any time on every tier, that branch data lives on the café's own Hub, and that cloud data is hosted in India

**Given** Legal
**When** it opens
**Then** it lists Terms of service, Privacy policy, Data processing terms and Ad policy, each on its own page with a version number, an effective date and a change history list

**Given** the Privacy policy
**When** it builds
**Then** its version is read from the same `arena-proto` policy-version constant that kiosk sign-up consent rows record, and CI fails if the page version and the constant differ

**Given** the Ad policy
**When** it renders
**Then** it states that ads appear only on the Free Android Hub at the counter, never on player screens, exclude adult, gambling and alcohol categories, and are removed on upgrade

### Story 11.8: Get started hands off to sign-up, and funnel events are counted without personal data

As the Arena 360 team,
I want to know how many visitors go from the site to sign-up and on to a live branch, without tracking anyone,
So that we can measure the launch funnel and fix where people drop off.

**Covers:** FR196 (Get started), NFR27 (funnel instrumentation), NFR13, NFR23, AD-40

**Acceptance Criteria:**

**Given** "Get started" anywhere on the site
**When** it is pressed
**Then** the visitor lands on the onboarding sign-up route from Story 2.3, carrying any `utm_source`, `utm_medium`, `utm_campaign` and selected tier as query parameters, and the marketing `/get-started` URL itself redirects there so it can be linked

**Given** `gz_control`
**When** the migration runs
**Then** it adds `product_events` (id ULID, name, at, anonymous session id, tenant_id nullable, branch_id nullable, page path, utm source/medium/campaign, referrer domain) with no IP, user agent, phone, email or name column, and a retention job deletes rows older than 13 months

**Given** the public `POST /v1/events` endpoint
**When** the site or onboarding app sends an event
**Then** only allow-listed names are stored (`site_page_view`, `get_started_click`, `signup_started`, `otp_verified`, `signup_completed`, `pricing_tab_changed`, `partner_apply_started`), unknown names and extra fields are dropped, and the per-IP Cloudflare limit applies

**Given** the anonymous session id
**When** it is created
**Then** it is a random id kept in `sessionStorage` (no cookie, no cross-site identifier), discarded when the tab closes, and linked to a `tenant_id` only by the `signup_completed` event sent from the authenticated onboarding session

**Given** a browser sending Global Privacy Control or Do Not Track
**When** any page loads
**Then** no event is sent

**Given** the event endpoint is down or slow
**When** a visitor navigates
**Then** events are sent with `navigator.sendBeacon` and failures are dropped silently; navigation and sign-up never wait on them

**Given** telemetry export
**When** the CI privacy test sends a sign-up with known values
**Then** none of those values appear in `product_events`, OpenTelemetry traces or Sentry

### Story 11.9: Admin app behind Cloudflare Access, with every action audited

As an Arena 360 staff member,
I want one internal app that only staff can reach, where everything I do is recorded,
So that we can run the business safely and answer for every change we make.

**Covers:** FR193 (staff SSO), FR194 (every admin action audited), AD-21, AD-40, AD-9, NFR11, NFR14, NFR25

**Acceptance Criteria:**

**Given** the admin SvelteKit app from Story 1.1
**When** it deploys
**Then** it is served from Workers static assets at `admin.arena360.cloud` behind a Cloudflare Access application that allows only the staff identity provider group, with staging behind a separate Access application

**Given** any `/v1/admin/*` request at the cloud
**When** it arrives
**Then** the cloud verifies the `Cf-Access-Jwt-Assertion` against the Access team certificates and the admin audience, and returns 401 if it is missing, expired or for another audience; a request that bypasses Cloudflare reaches no admin handler

**Given** `gz_control`
**When** the migration runs
**Then** it adds `admin_staff` (email, display name, roles subset of `catalogue|partners|finance|support|superadmin`, active, added_by, added_at), and a valid Access identity with no active `admin_staff` row gets 403 with "Your account isn't set up for admin. Ask a superadmin." and the denial is audited

**Given** the existing `admin_audit` table
**When** the migration runs
**Then** it gains, additively where missing, `staff_email`, `role_used`, `action`, `target_kind`, `target_id`, `reason`, `summary` (no secrets, no KYC values, no player data) and `request_id`, and an update or delete on `admin_audit` is refused by a trigger

**Given** any admin endpoint that changes state or reveals sensitive data
**When** it succeeds or fails
**Then** one `admin_audit` row with `actor{kind: admin, id: <staff email>}` is written in the same transaction as the change, and an endpoint without an audit call fails a CI check that lists every admin route against its audit action

**Given** the admin app
**When** a staff member opens it
**Then** it shows a navigation of Catalogue · Partners · Payouts · Billing · Support · Metrics · Audit · Staff with sections they lack a role for hidden, follows the light and dark themes, and meets the web screen-reader baseline

**Given** Audit
**When** it opens
**Then** it lists `admin_audit` rows newest first, filterable by staff, action, target and date, paged 50 at a time and exportable to CSV, and only `superadmin` can add or deactivate staff from Staff (itself audited)

### Story 11.10: A reseller or installer applies with phone OTP and sees "Under review"

As a reseller or installer,
I want to apply in a couple of minutes and see where my application stands,
So that I know I'm in the queue and don't have to chase anyone.

**Covers:** FR188, AD-18, AD-23, AD-40, NFR13, NFR23, UX-DR18, UX-DR92 (Under review tracker), UX-DR103, UX-DR113, UX-DR115 (key-web-partner-portal, state C)

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `partners` (id ULID, name, phone E.164 unique, email, city, business name, type `reseller|installer`, cafés served, status `applied|approved|rejected|suspended`, applied_at, decided_at, decided_by, rejection reason) and `partner_sessions`, entirely separate from `owners` and owner sessions

**Given** the partner portal (SvelteKit, Workers static assets) at `partners.arena360.cloud/apply`
**When** an applicant opens it
**Then** it shows text fields for name, phone (default +91), email, city, business name, type (Reseller / Installer) and cafés served (number), validated on submit and blur, with one primary "Send code"

**Given** a valid form
**When** "Send code" is pressed
**Then** a 6-digit code is sent by SMS through the messaging adapter with the same expiry, attempt and per-phone limits as owner sign-up, and a correct code creates the `partners` row in `applied`, issues a partner session cookie scoped to `partners.arena360.cloud`, and shows the status page

**Given** an `applied` partner
**When** they open the portal
**Then** they see only "Under review" with a three-step tracker (Submitted ✓ → Review (current) → Approved) and the text "We'll message you on WhatsApp when you're approved.", and no referral, café or payout data

**Given** a phone already used by a partner
**When** the form is submitted
**Then** the code is still sent (no enumeration) and a correct code signs in to the existing partner instead of creating a second one

**Given** a partner session cookie
**When** it calls any owner, tenant, branch or admin endpoint
**Then** the cloud returns 403 `FORBIDDEN`, and an owner session calling `/v1/partner/*` gets the same; the two scopes share no cookie, token or table

**Given** a returning partner
**When** they sign in
**Then** they sign in with phone OTP at `partners.arena360.cloud/sign-in`, and logs and telemetry carry the partner id only, never phone or email

### Story 11.11: Pricing — Partners tab with terms, a worked payout example and Apply

As a reseller or installer,
I want to see exactly what I'd earn and when I'd be paid before applying,
So that I can decide whether the programme is worth my time.

**Covers:** FR196 (Pricing, Partners), FR191 (terms displayed), UX-DR25, UX-DR91 (Partners tab), UX-DR93, UX-DR103 (money/legal register), UX-DR115 (key-web-pricing frames 3–4)

**Acceptance Criteria:**

**Given** the Pricing page
**When** the Partners tab is selected (or `/pricing?tab=partners` is opened)
**Then** it replaces the Story 11.3 placeholder with the programme terms: "10% of subscription revenue for every month the café you refer stays active and paying", leads not exclusive by territory, and who can apply (resellers and installers)

**Given** the worked payout example
**When** it renders
**Then** it shows, from the same catalogue values as the portal: three referred cafés on Basic monthly (₹600 GST-exclusive revenue) → ₹60 commission, "Pending — below ₹500 minimum, rolls over"; and ten cafés on Premium monthly (₹19,990) → ₹1,999 commission, TDS 2% ₹39.98, payout ₹1,959.02 by the 10th of the next month

**Given** payout terms
**When** they render
**Then** they state: monthly bank transfer by the 10th for the previous month; ₹500 minimum, a lower balance rolls over; TDS deducted at 2% (subject to tax advice), shown on each payout; GST-registered partners upload a GST invoice; KYC (PAN, bank details, optional GSTIN) after approval and before the first payout; refunds after a payout are recovered from the next one

**Given** the Partners tab
**When** "Apply" is pressed
**Then** it goes to `partners.arena360.cloud/apply` (Story 11.10) and a `partner_apply_started` event is sent under Story 11.8's rules

### Story 11.12: Staff approve or reject partners, and approval goes out on WhatsApp

As an Arena 360 partner manager,
I want to review applications and approve or reject them with a reason,
So that only real resellers and installers get a referral code.

**Covers:** FR189 (approval by WhatsApp with portal link), FR194 (partner approval, audited), AD-18, AD-21, AD-27/AD-34 (messaging adapter), UX-DR92, UX-DR96 (partner approval message), UX-DR115 (key-web-partner-portal, state B)

**Acceptance Criteria:**

**Given** a staff member with the `partners` role
**When** they open Partners → Applications
**Then** they see `applied` partners oldest first with name, business, type, city, cafés served and applied date, and can open one to Approve or Reject

**Given** Approve
**When** it is confirmed
**Then** the partner becomes `approved`, a unique referral code (8 characters, uppercase letters and digits without 0/O/1/I, e.g. `IMRN4K7Q`) and link `arena360.cloud/r/<code>` are generated, `decided_by` and `decided_at` are stored, and one `admin_audit` row is written

**Given** an approval
**When** it is committed
**Then** a WhatsApp message "Your Arena 360 partner application is approved. Log in: https://partners.arena360.cloud/sign-in" is sent once through the messaging adapter (SMS fallback), keyed on the partner id so a retry never sends twice, and a send failure shows on the application as "Message not sent — Resend" without undoing the approval

**Given** Reject
**When** it is confirmed
**Then** a reason is required, the partner becomes `rejected`, the portal shows "Your application wasn't approved." with the reason, and no message is sent beyond that portal status

**Given** an approved partner signing in
**When** the portal opens
**Then** the tracker is replaced by a referral card showing the code and link with "Copy link" and "Copy code", and a banner "KYC needed before first payout" linking to KYC

**Given** a staff member with the `partners` role
**When** they suspend an approved partner with a reason
**Then** the partner becomes `suspended`, the referral link stops attributing new sign-ups, existing attributions and accrued balances stay, and the action is audited

### Story 11.13: An approved partner submits KYC, stored encrypted and readable only by admin

As an approved partner,
I want to give my PAN and bank details once, securely,
So that I can be paid without sending documents over chat.

**Covers:** FR189 (KYC before first payout), FR194 (KYC review, audited), AD-19 (KYC encrypted, admin-read only), AD-22, NFR13, UX-DR18, UX-DR92 (KYC)

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `partner_kyc` (partner_id, status `submitted|verified|rejected`, ciphertext, key version, PAN last 4, account last 4, IFSC, gst_registered flag, submitted_at, reviewed_by, reviewed_at, rejection reason), where PAN, legal name, account number and GSTIN live only inside the AES-256-GCM ciphertext under a KYC data key held in SOPS-managed secrets, never in logs or telemetry

**Given** an approved partner on KYC
**When** they submit legal name, PAN, bank account number (entered twice), IFSC and optional GSTIN
**Then** PAN must match `AAAAA9999A`, IFSC `AAAA0XXXXXX`, a GSTIN must be 15 characters with a valid check digit and contain the given PAN, the two account numbers must match, and errors show per field without clearing the others

**Given** submitted KYC
**When** the partner views it later
**Then** the portal shows only "PAN ••••<last 4>", "Account ••••<last 4> · <IFSC>", "GST registered: yes/no" and the status; no endpoint returns the full values to a partner session

**Given** a staff member with the `partners` or `finance` role
**When** they press "Show KYC" on a partner
**Then** a reason is required, the decrypted values are shown for that view only, and the reveal is written to `admin_audit`; they can then mark the KYC Verified or Rejected (reason required), and a rejected partner sees the reason and can resubmit

**Given** a partner who changes their bank details after verification
**When** they resubmit
**Then** the KYC returns to `submitted`, payouts are held until it is verified again, and the previous ciphertext is kept for the audit history

### Story 11.14: A referral link or code is captured at owner sign-up and locked after the first payment

As a partner,
I want every café that signs up through my link or code to be credited to me and to stay credited,
So that I'm paid for the cafés I bring in.

**Covers:** FR190, AD-19 (URL/cookie → onboarding → tenant, immutable after first payment), AD-18, NFR13

**Acceptance Criteria:**

**Given** a visitor opening `arena360.cloud/r/<code>` or any marketing URL with `?ref=<code>`
**When** the code belongs to an `approved` partner
**Then** a first-party cookie `a360_ref` (code only, 30 days, Secure, SameSite=Lax) is set on `arena360.cloud` and its subdomains and the visitor lands on Home; an unknown, rejected or suspended code sets nothing and the page still loads

**Given** the onboarding sign-up from Story 2.3
**When** it opens
**Then** it shows an optional "Partner code" field prefilled from `a360_ref` (editable), and an entered code is checked on submit with "We don't recognise that code" for an invalid one, which never blocks sign-up

**Given** sign-up completes with a valid code
**When** the tenant is created
**Then** `tenants.referral_partner_id` and `referral_captured_at` are set, and a `referral_attributions` history row is written

**Given** a tenant with no first payment yet (no `subscriptions.first_paid_at` on any of its branches)
**When** the owner adds or changes a partner code under Account → Subscription, or staff change it in the admin app with a recorded reason
**Then** the attribution is updated and the history row records who changed it and when

**Given** a tenant where any branch has `first_paid_at`
**When** anyone tries to set, change or clear the referral
**Then** the API returns `REFERRAL_LOCKED` for owners and staff alike, and the admin app shows the field read-only with "Locked after first payment on <date>"

**Given** a code whose partner phone equals the signing-up owner's phone
**When** sign-up completes
**Then** no referral is recorded (self-referral) and the owner sees no error

### Story 11.15: Partner portal home — referral card, metrics and referred cafés

As a partner,
I want to see my link, how many of my cafés are paying and who they are,
So that I can follow up with the cafés I brought in.

**Covers:** FR192 (referral link/code, referred cafés, no tenant operational data), AD-18, UX-DR81, UX-DR92 (home, empty state), UX-DR100 (partner empty state), UX-DR113, UX-DR115 (key-web-partner-portal, state A)

**Acceptance Criteria:**

**Given** an approved partner on the portal home
**When** it opens
**Then** it shows the referral card (link and code with Copy buttons), a metric row with active paying cafés and commission rate "10%", a referred-cafés table, and navigation Referred cafés · Payouts · Referral link · KYC, in staff mood up to about 1200 px wide, in light and dark

**Given** the referred-cafés table
**When** it renders
**Then** each row shows café name, city, number of branches, plan status per café (Free, Paying, Grace, Lapsed, Refunded) and referred since, from a partner-scoped API that reads only `tenants`, `branches` and `subscriptions` state in `gz_control`

**Given** the partner API
**When** it builds a response
**Then** it never includes owner name, phone, email, GSTIN, revenue figures per café, branch data, R2 files or any Owner API data, and a contract test fails if the DTO gains a field outside the allow-list

**Given** no referred cafés
**When** the home opens
**Then** it shows "No referred cafés yet." with the referral link and Copy button

**Given** a partner who is `suspended`
**When** they sign in
**Then** they see the suspension notice and their payouts only, with the referral card hidden

### Story 11.16: Commission accrues on paid revenue net of refunds

As a partner,
I want each payment from a café I referred to add 10% to my balance, and refunds to take it back,
So that my balance always matches what Arena 360 actually earned from my cafés.

**Covers:** FR191 (10% of paid subscription revenue net of refunds), AD-19 (ledgered, net of refunds, clawback), AD-7 (minor units), AD-9

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `partner_ledger` (id ULID, partner_id, kind `accrual|clawback|adjustment|payout|tds`, amount_minor (INR paise, signed), source invoice or credit-note id unique per kind, tenant_id, branch_id, business month in Asia/Kolkata, statement_id nullable, created_at), append-only with no update or delete

**Given** an Epic 10 invoice issued and paid in band IN for a branch whose tenant has `referral_partner_id`
**When** the invoice is recorded
**Then** one `accrual` line of 10% of `subtotal_minor` (GST-exclusive), rounded half-up to the paisa, is written for that partner, keyed on the invoice id so a replayed event never doubles it

**Given** an Epic 10 credit note reversing a referred invoice
**When** it is issued
**Then** one `clawback` line of the negative of that invoice's accrual is written, whether or not the accrual has already been paid out

**Given** an invoice for a tenant with no referral, a suspended partner's tenant attributed before suspension, or a non-IN band
**When** it is recorded
**Then** no line is written for the first and third; for the suspended partner the accrual is still written (attribution stays)

**Given** staff with the `finance` role
**When** they add an `adjustment` line
**Then** a reason is required, the amount is signed and the action is audited; adjustments are the only manual ledger write

### Story 11.17: The monthly payout job on the 1st writes each partner's statement with rollover, TDS and clawback

As an Arena 360 finance staff member,
I want each partner's statement worked out automatically on the 1st,
So that we pay the right amount by the 10th without spreadsheets.

**Covers:** FR191 (payout by the 10th, ₹500 rollover, 2% TDS, clawback after payout), AD-19 (monthly job, Asia/Kolkata, FY Apr–Mar), AD-7, NFR25

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `partner_statements` (id, partner_id, month, opening carry, accruals, clawbacks, adjustments, gross, tds, net, status `pending_minimum|ready|paid|carried_negative`, paid_at, bank reference, unique on (partner_id, month))

**Given** the jobs role
**When** 00:30 Asia/Kolkata on the 1st arrives
**Then** for every partner with ledger lines or a non-zero carry it computes gross = previous statement's carry + the previous month's accruals + clawbacks + adjustments, writes one statement per partner and month, and links the ledger lines to it

**Given** the job has already run for a month
**When** it runs again for that month (retry or manual rerun)
**Then** no statement or ledger line is added or changed

**Given** gross below ₹500.00 and above zero
**When** the statement is written
**Then** it is `pending_minimum` with no TDS, and its gross is carried into next month's opening carry

**Given** gross ≤ 0 (clawbacks exceed accruals, for example a refund after a paid month)
**When** the statement is written
**Then** it is `carried_negative`, nothing is payable, and the negative amount is the next month's opening carry

**Given** gross of ₹500.00 or more
**When** the statement is written
**Then** TDS is 2% of gross rounded to the paisa, net = gross − TDS, a `tds` ledger line is written, and the status is `ready`

**Given** the job
**When** it finishes or fails
**Then** it records counts and totals to metrics and logs, a failure alerts the ops channel, and a success posts "Statements for <month>: ready <n>, pending minimum <n>, carried negative <n>" to the ops channel

### Story 11.18: Payouts are held until KYC is verified and, for GST-registered partners, a GST invoice is attached

As an Arena 360 finance staff member,
I want statements held automatically when a partner's KYC or GST invoice is missing,
So that we never transfer money to unverified bank details or without the invoice the law needs.

**Covers:** FR189 (KYC before the first payout), FR191 (GST-registered partners upload an invoice or are held), AD-19

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it additively extends the `partner_statements` status with `held_kyc|held_invoice` and adds `invoice_key` (nullable) and `hold_reason`

**Given** a statement of ₹500.00 or more written by the payout job
**When** its status is decided
**Then** it is `held_kyc` if the partner's KYC is not `verified`, else `held_invoice` if the partner is GST-registered and `invoice_key` is empty, else `ready`

**Given** a `held_kyc` statement
**When** the partner's KYC becomes verified
**Then** the statement re-evaluates at once (to `held_invoice` or `ready`) without waiting for the next run

**Given** a `held_invoice` statement
**When** its `invoice_key` is set
**Then** it moves to `ready` at once

**Given** a `ready` statement not yet paid
**When** the partner resubmits KYC (Story 11.13 returns it to `submitted`)
**Then** the statement returns to `held_kyc` until the new KYC is verified

**Given** held statements
**When** the next month's job runs
**Then** they stay as they are (their amounts are not re-added to the next opening carry), the new month gets its own statement, and the ops summary adds "held KYC <n>, held invoice <n>"

### Story 11.19: Partner portal Payouts and GST invoice upload

As a partner,
I want to see every month's accrued, held and paid amounts with TDS, and upload my GST invoice,
So that I know exactly what's coming and nothing is held without me knowing why.

**Covers:** FR191 (GST-registered partners upload an invoice or are held), FR192 (accrued, pending, paid, TDS, invoice status), AD-19, UX-DR81, UX-DR92 (payouts table and states), UX-DR93, UX-DR103

**Acceptance Criteria:**

**Given** the portal Payouts view and the home metric row
**When** they open
**Then** the metric row adds "Payout this month" (the latest statement's net, or "₹0") and the payouts table lists each month with accrued, clawbacks, gross, TDS, net, status and GST-invoice status, plus a current-month row "Accruing" with this month's accruals so far

**Given** status labels
**When** they render
**Then** they read "Pending — below ₹500 minimum, rolls over", "KYC needed before first payout" (linking to KYC), "Upload GST invoice", "Ready — paying by the 10th", "Paid <date> · Ref <bank reference>" and "Recovered from next payout" for a negative carry, always as text with a `status-badge`, never colour alone

**Given** a GST-registered partner with a `held_invoice` or `ready` statement
**When** they upload a PDF (≤ 5 MB, `application/pdf`)
**Then** it is stored under a private partner prefix in R2 (never a branch prefix) via a 10-minute presigned PUT, linked to the statement, the hold is released, and finance staff can open it from the admin app; any other type or size is refused with the reason

**Given** a partner who is not GST-registered
**When** Payouts renders
**Then** no invoice column or upload control is shown

**Given** a statement already `paid`
**When** the partner tries to replace its invoice
**Then** the upload is refused with "This payout is already paid. Contact support to change its invoice."

### Story 11.20: Staff mark payouts paid after a manual bank transfer

As an Arena 360 finance staff member,
I want a list of payouts to make, the bank details to pay, and a way to record each transfer,
So that every partner is paid by the 10th and the books show it.

**Covers:** FR191 (payout by the 10th), FR194 (payout marking after manual transfer, audited), AD-19, AD-21, UX-DR96 (payout statement message)

**Acceptance Criteria:**

**Given** a staff member with the `finance` role
**When** they open Payouts
**Then** they see `ready` statements for the month with partner, gross, TDS, net, KYC status and invoice link, held statements in a separate list with their reason, and a CSV export of the ready list without account numbers

**Given** a ready statement
**When** staff press "Show bank details" with a reason
**Then** the decrypted account number, IFSC and legal name are shown for that view and the reveal is audited

**Given** a completed transfer
**When** staff press "Mark paid" and enter the bank reference (UTR) and transfer date
**Then** the statement becomes `paid` with `paid_at` and the reference, a `payout` ledger line of −net is written, the action is audited, and the partner gets one payout-statement message (WhatsApp, with email copy) showing month, accrued, TDS, paid and invoice status

**Given** a statement that is not `ready`
**When** anyone tries to mark it paid
**Then** the API returns `PAYOUT_NOT_READY` with the hold reason

**Given** a statement marked paid by mistake
**When** a `superadmin` reverses it with a reason
**Then** it returns to `ready`, a compensating ledger line is written, and both actions remain in the audit

**Given** the 10th of the month in Asia/Kolkata
**When** any statement for the previous month is still `ready`
**Then** Payouts shows "<n> payouts are late" and the ops channel is alerted once

### Story 11.21: Staff curate game records in the admin app

As an Arena 360 catalogue editor,
I want to add and correct games, their executables and age ratings in a browser,
So that every café's kiosks match games to proper names without anyone editing files.

**Covers:** FR193 (curate the global game catalogue), FR194 (audited), AD-17, AD-21

**Acceptance Criteria:**

**Given** a staff member with the `catalogue` role
**When** they open Catalogue
**Then** they see the Story 6.1 `catalog_games` list with name, launcher, category, age rating, published, media status (cover yes/no, video screened/unscreened/none), searchable by name or executable file name, paged 50 at a time

**Given** a game
**When** they create or edit it
**Then** they can set name, launcher, category and age rating and add or remove executable signatures (file name, product name, publisher, optional SHA-256), with `base_rev` checked so a concurrent edit returns `STALE` and shows the other editor's change

**Given** a new game with the same name and launcher as an existing one
**When** it is saved
**Then** it is refused `DUPLICATE` with a link to the existing record

**Given** Publish or Unpublish
**When** it is confirmed
**Then** `catalog_version` increments so Hubs pick it up through the Story 6.1 manifest on their next pull, and an unpublished or deleted game is delivered as a tombstone

**Given** any catalogue change
**When** it is saved
**Then** it is written to `admin_audit` with the before and after field values, and the Story 6.1 `arena-cloud catalog import` continues to work against the same tables and audit

### Story 11.22: Every catalogue video is flash-analysed before anyone can approve it

As an Arena 360 catalogue editor,
I want every game video checked automatically for dangerous flashing,
So that no café's screens ever play a video that could trigger a seizure.

**Covers:** FR193 (media flash-screened before publishing), AD-17, AD-21, NFR26

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** it adds `catalog_media_screening` (media_id, analyser version, result `queued|pass|fail|error`, failing timestamps, analysed_at, error text), and every existing video `catalog_media` row with `flash_screened = false` is queued

**Given** a video `catalog_media` row with `flash_screened = false` and no screening result (including videos added by the Story 6.1 `catalog import`)
**When** the jobs role picks it up
**Then** it decodes the file from R2 and runs a photosensitivity analysis (general flash and red flash: no more than 3 flashes in any 1-second period and no saturated-red transitions, per ITU-R BT.1702 thresholds), off the request path, and records pass or fail with the failing timestamps

**Given** a failing video
**When** the result is recorded
**Then** the media is marked "Failed flash check at <timestamps>" and can never become `flash_screened`, whatever path tries to set it

**Given** a passing video
**When** the result is recorded
**Then** `flash_screened` stays false and the media waits for a person's approval; passing the analysis alone never publishes it

**Given** a file that cannot be decoded
**When** analysis runs
**Then** it is retried once, then recorded `error` with the reason, and treated as not screened

**Given** any result
**When** it is recorded
**Then** an `admin_audit` row with `actor{kind: system}` records the media id, analyser version and result, and the Story 6.1 manifest still excludes every video whose `flash_screened` is false

### Story 11.23: Staff upload game media and a second person approves screened videos

As an Arena 360 catalogue editor,
I want to upload covers and videos in the admin app and have a colleague sign off each video,
So that game art reaches cafés quickly and no video publishes on one person's say-so.

**Covers:** FR193 (curate media; flash-screened before publishing), FR194 (audited), AD-17, AD-36 (shared catalogue prefix), AD-21

**Acceptance Criteria:**

**Given** a game in the admin app
**When** an editor uploads a cover (JPEG/PNG/WebP, ≤ 2 MB, at least 600 × 900) or a video (MP4 H.264, no audio track, ≤ 30 s, ≤ 20 MB)
**Then** the browser hashes it, uploads it by presigned PUT to `catalog/<sha256>.<ext>` (skipped if the hash exists), a `catalog_media` row is created (video with `flash_screened = false`, entering the Story 11.22 queue), and the upload is audited; files outside the limits or with an audio track are refused with the reason

**Given** a game's media panel
**When** it renders
**Then** each video shows its state as text: "Analysing", "Failed flash check at <timestamps>", "Couldn't analyse — upload again", "Waiting for approval" or "Screened · approved by <name>", and each cover shows a preview

**Given** a video "Waiting for approval"
**When** a staff member with the `catalogue` role who did not upload it watches it and presses "Approve"
**Then** `flash_screened = true` is set, `catalog_version` increments so Hubs receive it on their next manifest pull, and the approver is written to `admin_audit`

**Given** the uploader of a video
**When** they press "Approve" on it
**Then** it is refused `SAME_PERSON` with "Someone else must approve this video"

**Given** a video that failed or errored
**When** anyone tries to approve it
**Then** it is refused `NOT_SCREENABLE`, and the editor's only option is to upload a new file

**Given** a cover or video replaced or removed
**When** it is saved
**Then** the game points to the new media, `catalog_version` increments, the old object stays in R2 (content-addressed, possibly shared), and the change is audited

### Story 11.24: Staff approve or reject 7-day refunds in the admin app

As an Arena 360 finance staff member,
I want refund requests in a queue with the window and amounts shown,
So that owners get eligible refunds quickly and nothing outside the policy is paid.

**Covers:** FR194 (7-day refunds, audited), FR180, AD-20, AD-21

**Acceptance Criteria:**

**Given** a staff member with the `finance` role
**When** they open Billing → Refund requests
**Then** they see `requested` refund requests oldest first with tenant, branch, plan, amount, `first_paid_at`, requested at and time left in the 7-day window, from the Epic 10 `refund_requests` table

**Given** a request
**When** staff press Approve with a reason
**Then** the admin API calls the same refund service as `arena-cloud ops refund approve` (Epic 10), the request moves to `issued`, and a Razorpay error leaves it `requested` with the error shown inline

**Given** a request
**When** staff press Reject with a reason
**Then** the same service as `ops refund reject` runs and the owner sees the reason, as defined in Epic 10

**Given** a request the service refuses (`REFUND_NOT_ALLOWED`)
**When** staff act on it
**Then** the error is shown with the policy reason and nothing changes

**Given** any approve or reject
**When** it completes
**Then** exactly one `admin_audit` row records staff, reason, amounts and request id, whether the action came from the admin app or the CLI

### Story 11.25: Staff grant and revoke comp licences in the admin app

As an Arena 360 staff member,
I want to give a branch a free paid tier for a set time, and take it back,
So that pilots, partners' demo cafés and support goodwill don't need a fake payment.

**Covers:** FR194 (audited admin actions), AD-10, AD-20 (entitlements, no gate commands), AD-21, AD-29

**Acceptance Criteria:**

**Given** a staff member with the `finance` or `superadmin` role
**When** they open a branch in Billing and choose "Grant comp"
**Then** they pick Basic or Premium, an end date no more than 12 months ahead and a reason, and the admin API calls the same service as `arena-cloud ops grant-comp` (Story 2.7), so the entitlement source becomes `comp` and the next heartbeat delivers the licence

**Given** a branch with an active paid subscription
**When** comp is granted
**Then** it is refused `COMP_CONFLICTS_WITH_SUBSCRIPTION` with the subscription shown

**Given** an active comp
**When** staff choose "End comp" with a reason
**Then** the entitlement returns to `free` and the next heartbeat delivers the Free licence; running sessions and open orders are untouched (the Epic 10 lapse rules apply)

**Given** a comp reaching its end date
**When** the cloud clock passes it
**Then** it ends the same way, and Billing lists comps ending in the next 14 days

**Given** comp revenue
**When** the partner ledger runs
**Then** comps generate no invoice and therefore no commission

### Story 11.26: Support sees a tenant and branch summary, and searches audit only with a reason

As an Arena 360 support agent,
I want one page with a branch's plan, versions and connection health, and audit search when I need it,
So that I can help an owner quickly without ever reaching into their Hub or player data.

**Covers:** FR195 (audit search with a recorded reason), FR194 (audited), AD-21, AD-37, NFR13

**Acceptance Criteria:**

**Given** a staff member with the `support` role
**When** they search Support by tenant name, owner phone (matched by hash, never listed), branch code or tenant id
**Then** they get matching tenants and branches, and the search itself is audited

**Given** a branch support view
**When** it opens
**Then** it shows only control-plane facts: tenant and branch names, tier and entitlement source, subscription state, licence claims and expiry, last heartbeat time, Hub and schema versions, device count against cap, pending and failed commands by kind, last batch and summary upload times, and open support cases; no player, sales, staff or session data is shown

**Given** the view
**When** support opens a case
**Then** a `support_cases` row (id, tenant_id, branch_id nullable, opened_by, opened_at, category, closed_at) is created and the case id is shown for reference in later actions

**Given** audit search for a branch month
**When** support enters a reason and a query from the fixed catalogue (actor, module, intent, entity, time range)
**Then** the admin app calls the Story 8.35 audit API with the reason, shows "Preparing audit…" while it builds, and returns rows with players by id only; a missing reason blocks the request

**Given** a support agent
**When** they try any action that would write to a branch
**Then** no such action exists in the support view; owner-side changes are made by the owner

### Story 11.27: The Hub builds a diagnostics bundle with no personal data and sends it only when staff press Send

As a café manager,
I want to send Arena 360 support a technical bundle from my Hub with one button,
So that they can fix my problem without seeing my players' details or logging into my PC.

**Covers:** FR195 (diagnostics bundle < 5 MB, no player PII, sent only on request), AD-21, AD-24, NFR13, UX-DR103

**Acceptance Criteria:**

**Given** a manager in Hub Settings → System → Diagnostics
**When** they press "Create diagnostics bundle"
**Then** the Hub builds a zip with Hub `tracing` logs (most recent first), branch settings without secrets, the device list (name, class, version, last seen, status), licence claims, `PRAGMA integrity_check` result, schema and app versions, upload and command backlog counts, and a `manifest.json` listing every section and what was trimmed

**Given** the bundle
**When** it is built
**Then** it never contains device tokens, licence signatures, the branch KEK or player keys, PIN hashes or verifiers, passwords, player names, phones or emails, and a redaction pass removes anything matching phone, email or PAN patterns from log lines

**Given** the bundle would exceed 5 MB compressed
**When** it is built
**Then** the oldest log lines are dropped until it is under 4.5 MB, and the manifest records what was trimmed

**Given** a built bundle
**When** it is shown to the manager
**Then** they see its size and a list of what it contains, and two buttons, "Send to Arena 360 support" and "Save to file"; nothing leaves the Hub until one is pressed

**Given** "Send to Arena 360 support" with the WAN up
**When** it is pressed
**Then** the bundle is uploaded by presigned PUT to `tenants/<t>/branches/<b>/diagnostics/<ulid>.zip` (30-day lifecycle), a `diagnostics.sent` intent is audited with the manager, and the Hub shows a reference "DIAG-<6 chars>" to quote to support

**Given** the WAN is down
**When** the manager presses Send
**Then** it says "No internet — save the file and share it with support another way" and offers "Save to file" (folder or USB); nothing is queued to send later

**Given** a seeded branch with known player names, phones and emails
**When** the CI privacy test builds a bundle
**Then** none of those values appear anywhere in it

### Story 11.28: Kiosk logs are collected over the LAN into the diagnostics bundle

As an Arena 360 support agent,
I want the bundle to include recent logs from each gaming PC,
So that I can see kiosk-side problems without asking the café to copy files from every PC.

**Covers:** FR195 (diagnostics bundle < 5 MB, no player PII), AD-31 (kiosk service), AD-42 (paired-device class), AD-25, NFR13

**Acceptance Criteria:**

**Given** a diagnostics bundle being built at the Hub
**When** kiosks are online
**Then** the Hub sends each paired kiosk a `diag.logs_request{since: now − 24 h, max_bytes: 200 KB}` frame over its authenticated paired-device connection, and only the paired-device class may receive or answer it

**Given** a kiosk receiving the request
**When** its service answers
**Then** it returns the newest 24 h of its ring-buffer log, at most 200 KB compressed, after the same phone, email and PAN redaction pass as the Hub, read from disk by the service with no UI shown and no effect on a running session

**Given** the answers
**When** the bundle is assembled
**Then** each kiosk's log is added under `kiosks/<device name>.log`, kiosk logs together are capped at 2 MB, and the whole bundle still stays under 4.5 MB by the Story 11.27 trimming rule

**Given** a kiosk that is offline, does not answer within 10 s, or runs a release without `diag.logs_request` (`UNSUPPORTED`)
**When** the bundle is assembled
**Then** the manifest lists it as "not collected — offline", "not collected — timed out" or "not collected — update needed", and the bundle is still built without waiting further

**Given** a kiosk with seeded log lines containing a player's phone and email
**When** the CI privacy test builds a bundle
**Then** neither value appears in it

### Story 11.29: Support requests and downloads diagnostics bundles from the admin app

As an Arena 360 support agent,
I want to ask a branch for a diagnostics bundle and open what they send,
So that I can investigate without asking the owner to dig through folders.

**Covers:** FR195 (sent only on request), AD-5 (command mailbox), AD-21, AD-36, NFR13

**Acceptance Criteria:**

**Given** an open support case for a branch
**When** support presses "Request diagnostics" with a reason
**Then** a `diagnostics.request` command (case id, requested by, reason) is queued through the command mailbox, the action is audited, and the case shows "Requested — waiting for the branch"

**Given** the Hub applies `diagnostics.request`
**When** staff are signed in
**Then** the Hub shows a persistent banner "Arena 360 support asked for a diagnostics bundle (case <id>). Create and send / Decline" for managers; the bundle is never built or sent without a manager pressing Send, and Decline is reported back as the command result

**Given** a bundle sent from the Hub (on request or on its own)
**When** support opens Support → Diagnostics for the branch
**Then** they see each bundle with reference, size, sent at and sent by role, and can download it only after entering a reason; each download is a 5-minute presigned GET and is audited

**Given** a bundle older than 30 days
**When** it is listed
**Then** it shows "Expired" and cannot be downloaded

**Given** a Hub on a release that doesn't know `diagnostics.request`
**When** the command is fetched
**Then** it is rejected `UNSUPPORTED` and the case shows "Branch needs an update to send diagnostics on request — ask the manager to use Settings → System → Diagnostics"

### Story 11.30: Launch metrics are computed nightly and shown in the admin app

As the Arena 360 team,
I want the launch success metrics computed every night from data we already hold,
So that we know whether onboarding, conversion, churn, shift accuracy, support load and ad fill are on target.

**Covers:** NFR27, AD-9, AD-35, NFR13, NFR25

**Acceptance Criteria:**

**Given** the Hub heartbeat
**When** the first session at a branch is settled with a payment
**Then** the heartbeat carries `first_settled_session_at` (a timestamp only, sent until the cloud acknowledges it) and the cloud stores it on the branch once

**Given** the jobs role at 02:00 Asia/Kolkata
**When** the metrics job runs
**Then** it writes `launch_metrics` (date, metric, value, numerator, denominator) for: share of branches with first settled session < 60 min after owner sign-up (target 80%); free-to-paid within 90 days of sign-up (20%); paid churn this month (< 3%); median per-shift cash variance after a branch's first 30 days, from paid branches' rollups (< ₹50); share of branches with a support case this month (< 2%); Free ad fill rate from `ad_impressions` (40%); plus the Story 11.8 funnel counts (site visits → Get started → sign-up → OTP verified → live)

**Given** a staff member with any admin role
**When** they open Metrics
**Then** each metric shows its latest value against its target with ok or warning text, a 12-week trend table and its numerator and denominator, and missing inputs show "Not enough data yet" rather than zero

**Given** the metrics
**When** they are computed
**Then** they use only ids, timestamps, counts and amounts already in `gz_control` or rollups; no phone, email, name or player id enters `launch_metrics`

**Given** a rerun for a past date
**When** it executes
**Then** it overwrites that date's rows only and the result matches the first run for unchanged inputs

### Story 11.31: Partner money and admin access proven end to end

As the Arena 360 team,
I want the partner money flow and the access boundaries tested before every release,
So that partners are paid correctly and nobody reaches data they shouldn't.

**Covers:** FR188–FR195, AD-18, AD-19, AD-21, NFR10, NFR11, NFR13

**Acceptance Criteria:**

**Given** the Razorpay fake and a controllable cloud clock
**When** the partner suite runs
**Then** it walks: apply → approve (one WhatsApp) → KYC verify → owner signs up through the referral link → first payment (attribution locks) → monthly accruals → a month under ₹500 that rolls over → a month over ₹500 with 2% TDS → GST-registered hold → invoice upload → mark paid → a refund after payout that claws back into the next statement, asserting every ledger line and statement total to the paisa

**Given** the same statement month
**When** the payout job and webhook replays run twice
**Then** no ledger line or statement is duplicated

**Given** the access suite
**When** it runs
**Then** a partner session gets 403 on every owner, tenant, branch and admin route; an owner session gets 403 on every partner and admin route; an admin route without a valid Access JWT gets 401; a valid Access identity without a staff row gets 403; and every admin route in the router appears in the audit-action map

**Given** the privacy checks
**When** they run
**Then** partner API responses contain only allow-listed fields, KYC values appear nowhere outside the ciphertext column, the diagnostics bundle from a seeded branch contains no seeded PII, and `product_events` holds no seeded sign-up values

## Epic 12: Console TVs with Pi and QR self-start

Players start and see console time on the TV through a Pi agent and a QR code.

> Draft notes: this epic builds on the Epic 1 scaffold (Story 1.1: the `arena-pi` crate already exists as a placeholder, shared UI package, tokens and message catalogue), the LAN envelope and class scope table (Story 1.7: `paired`, `kds`, `phone`), `apply()` and idempotent intents (Stories 1.5–1.6), inline PIN (Story 1.11), the audit viewer (Story 1.13) and per-device HC (Story 1.19); on Epic 2 mDNS discovery and typed-IP fallback (Story 2.16), 6-digit device pairing with pinned fingerprint (Story 2.17), the device cap that already counts Pis and the pairing limiter (Story 2.18), console registration with a TV number (Story 2.20), group rates (Story 2.24) and opening hours (Story 2.25); on Epic 3 the tile sheet and "Failed — retry" pattern (Story 3.5), session start in every mode (Stories 3.9, 3.24, 3.27), session ticks with `lock_after_ms` and `time_up_mode` (Story 3.10), pause (Story 3.15), the timer engine and warning marks (Story 3.23), the alert service with sound and badge (Story 3.25), Hub Extend (Story 3.28), console sessions on the tile (Story 3.35) and console time-up plus controller check (Story 3.36); on Epic 6 the Hub media cache served on `:7401` to paired devices (Story 6.2), the `game-media` fallback component (Story 6.6), the branch logo setting (Story 6.7) and the kiosk Hub-unreachable rules (Story 6.36); and on the Epic 7 pattern of a plain-HTTP browser client served by path on `:7401` (Story 7.6). **Decided (architecture):** the Pi agent is Rust on aarch64, shipped as a Raspberry Pi OS Lite image with a read-only root and overlay, run by systemd; it pairs like a kiosk (paired-device class, device token, TLS pinned to the Hub certificate); TV screens are a Svelte page served by the agent on localhost and shown by Chromium in kiosk mode, using the shared tokens and components; the phone page is served by the Hub on `:7401` over plain HTTP at `/console`, as the anonymous `phone` class limited to `console.plan_requested` and reading its own timer, rate-limited per TV; staff confirm and start at the Hub with their own actor and the player pays at the counter; there is no player login and no console grace; CEC runs after commit from the side-effect outbox and records a result intent (AD-28); while the Hub is unreachable the Pi counts down on its monotonic clock, runs warnings and the lock, and writes nothing (AD-12); the phone page is unavailable then. **QR link (decided):** the QR uses the friendly mDNS name `http://arena360.local:7401/console/<tv>?t=<token>`, advertised by the primary Hub. **Known risk (accepted):** some Android phones cannot resolve `.local` names, so the phone page shows nothing for them; the fallback is staff starting the session from the tile, which works with or without the phone page. **Smart plugs:** vendors are deferred; this epic defines the power-switch adapter interface and a test fake only, and the branch option stays disabled until an adapter ships. **Out of this epic:** Pi agent auto-updates from the Hub's signed cache (FR176, Epic 14; this epic ships the signed image only), the console 5-minute WhatsApp/SMS (Story 8.28), the controller check itself (Story 3.36), and remote Pis through the Tunnel (Story 8.39 path, unchanged). Every story adds its new intents to the Story 1.6 `proptest` generator.

### Story 12.1: A signed, read-only Pi image is built in CI

As a café owner adding a TV,
I want one ready-made image I can flash onto a Pi,
So that setting up a console TV needs no Linux knowledge and the Pi can't be broken by being unplugged.

**Covers:** FR142 (hardware), AD-32 (signed agent), NFR17, NFR18

**Acceptance Criteria:**

**Given** the CI image job
**When** it runs on a release tag
**Then** it builds one Raspberry Pi OS Lite (64-bit) image for Pi Zero 2 W and Pi 4 containing the `arena-pi` aarch64 binary, Chromium, and a systemd unit that starts the agent and then Chromium in kiosk mode pointed at the agent's localhost page

**Given** the agent binary in the image
**When** it is built
**Then** it is ed25519-signed, and the agent's launcher verifies the signature before start and refuses to run an unsigned or tampered binary, logging why

**Given** the built image
**When** it boots
**Then** the root filesystem is mounted read-only with a tmpfs overlay, and a small writable `/data` partition holds only the device token, the pinned fingerprint, the last cached branding and rates, and size-capped logs

**Given** the image
**When** it is inspected in CI
**Then** SSH, the desktop environment, auto-login to a shell and any open inbound port other than the agent's localhost page are absent, and the job fails if any is present

**Given** a release
**When** the image is published
**Then** it is published with its SHA-256 and signature next to the help-centre flashing steps

### Story 12.2: The Pi boots into a TV screen, finds the Hub and recovers on its own

As a café owner adding a TV,
I want the Pi to show a clean screen on the TV and look for my Hub by itself,
So that the TV is ready to pair within a minute of plugging it in and keeps working after crashes and power cuts.

**Covers:** FR142 (discovery), AD-8 (mDNS name from `arena-proto`), AD-33, NFR17, NFR18, UX-DR6, UX-DR9, UX-DR94

**Acceptance Criteria:**

**Given** the Pi boots the Story 12.1 image
**When** the agent starts
**Then** within 60 s of power-on the TV shows the agent's Svelte page in the Kiosk 1920 layout and type ramp, dark-only, with `logo-slot` showing the Arena 360 mark and "Looking for the Hub…"; no desktop, cursor, console text or Chromium UI is ever visible

**Given** a Hub advertising the `arena-proto` mDNS service on the LAN
**When** the unpaired Pi searches
**Then** it lists discovered Hubs by branch and Hub name within 10 s; with none found it shows a "Hub IP address" field and "Search again", as on the kiosk (Story 2.16)

**Given** setup input
**When** a USB keyboard is plugged in, or the TV remote sends digit and arrow keys over CEC
**Then** the page can be operated with it (choose Hub, type IP or code); with neither, the page stays on the discovery screen and nothing else is reachable

**Given** the agent or Chromium crashes
**When** systemd or the agent's watchdog notices
**Then** it is restarted within 5 s and the TV returns to the agent page, and the restart is logged to `/data`

**Given** an automated power-pull test on both boards
**When** power is cut 100 times at random points during boot and while idle
**Then** every boot reaches the agent page and `/data` mounts clean each time

### Story 12.3: Pair a Pi with a 6-digit code and see it on the floor within a minute

As a café owner setting up a console TV,
I want to type the Hub's 6-digit code on the Pi once,
So that the TV joins my branch securely and I can see it at the counter.

**Covers:** FR142 (pairing, floor ≤ 60 s), AD-10, AD-42 (paired-device class), AD-29, NFR11, NFR23, UX-DR2, UX-DR61

**Acceptance Criteria:**

**Given** Settings → Devices on the Hub
**When** a Manager or Owner presses "Pair a TV (Pi)"
**Then** the same 6-digit single-use code panel as kiosk pairing (Story 2.17) is shown with a 10-minute countdown and a fresh code after each use

**Given** a Pi that has selected a Hub
**When** the code is entered
**Then** over TLS to `:7401` it sends `pair{code, name, version, kind: pi}`; the Hub mints a 256-bit device token (Argon2id hash in `devices`), creates a `devices_meta` row with `kind = pi` via `device.register`, and returns the token and the Hub certificate fingerprint once; the agent stores both on `/data` (mode 0600) and pins the fingerprint for every later connection

**Given** the agent connects with `hello{class: paired}`
**When** it reports `capabilities`
**Then** they include the board model and a CEC probe result (`cec: true|false`, adapter, the Pi's physical address), and the Hub stores them on the device row

**Given** a just-paired Pi
**When** 60 s have passed since the code was entered
**Then** it appears in the floor's Unassigned section as a dashed entry with its name and a TV glyph, and the TV shows "Not linked yet — link this TV at the counter."

**Given** a wrong or expired code, 5 failures on one code, or more than 20 attempts per IP per hour
**When** they happen
**Then** the Pi clears the field with a plain retry line, the code is invalidated, or `RATE_LIMITED` is returned (Story 2.18 limiter)

**Given** the device cap is reached
**When** a Pi pairs
**Then** it is refused `DEVICE_LIMIT`, the Hub shows the Story 2.18 `tier-lock`, and the TV shows "This branch has reached its device limit — ask the owner."

**Given** a Pi is revoked at the Hub or by owner command
**When** the revocation applies
**Then** its socket closes with `TOKEN_REVOKED`, the agent deletes its token from `/data` and returns to the discovery screen, and its slot is freed

### Story 12.4: Link a Pi to a console so the TV shows its TV number

As counter staff,
I want to link each Pi to the console it sits next to,
So that the TV shows the right number and the console tile knows it has a TV it can control.

**Covers:** FR142 (TV number, floor), UX-DR62, UX-DR97 (Unassigned, Offline), UX-DR110

**Acceptance Criteria:**

**Given** an Unassigned Pi's sheet
**When** a Manager chooses "Link to console" and picks a registered console (Story 2.20) that has no Pi
**Then** `pi.link{pi_id, console_id}` applies, the Pi leaves Unassigned, and the TV shows the console's "TV n" in `kiosk-headline` within 2 s

**Given** a console that already has a Pi, or a Pi already linked
**When** another link is attempted
**Then** it is refused `PI_ALREADY_LINKED` with the inline reason naming the other device; "Unlink" on the console sheet frees both (Manager, audited)

**Given** a linked console tile
**When** it renders
**Then** the sheet shows a "TV" line with the Pi state as text and glyph: "TV connected", "TV offline (last seen 8:42 pm)" after 30 s without the Pi, or "No CEC — switch inputs by hand" when the probe found no CEC

**Given** the console's TV number is edited (Story 2.20)
**When** it is saved
**Then** the linked TV shows the new number within 2 s

**Given** the Pi's link
**When** the console's input is set at linking
**Then** the agent scans the CEC bus and offers the devices it found with their HDMI port; staff pick the console's port (or type HDMI 1–4 if the scan finds nothing), and it is stored on the link as the console input

**Given** HC is switched on for a Pi in the Story 1.19 list (Pis now listed by name)
**When** it is saved
**Then** the TV switches to `-hc` tokens within 2 s and applies it from cache before first paint on its next cold start

### Story 12.5: TV control adapters — HDMI-CEC, and a power-switch interface for smart plugs

As a developer on Arena 360,
I want the Pi to drive the TV through one adapter interface with a CEC implementation and a smart-plug slot,
So that every TV feature uses the same commands and plug vendors can be added later without touching them.

**Covers:** FR145 (CEC standby or smart plug), FR147 (power, input), AD-28, NFR18 (CEC best-effort, plug as guarantee)

**Acceptance Criteria:**

**Given** `arena-pi`
**When** the `TvControl` trait is defined
**Then** it exposes `power_on`, `standby`, `show_pi_input`, `show_console_input(port)` and `status`, each returning a typed result (`Ok`, `Unsupported`, `NoAck`, `Timeout`) within 3 s; the CEC implementation uses libcec, and a recorded fake implementation is used in tests

**Given** the `PowerSwitch` trait (`on`, `off`, `state`)
**When** the agent starts
**Then** it loads registered adapters by name from a registry that is empty at launch apart from a test fake; Settings → Consoles at the Hub shows "Smart plug" as a disabled option with "No supported smart plug yet"

**Given** a TV command arrives from the Hub
**When** the agent runs it
**Then** it replies with the result kind and the time taken, and the Hub records it through the side-effect outbox as `tv.command.result` (never inside `apply()`); a CEC `NoAck` is retried once before it is reported

**Given** a TV whose CEC probe was negative
**When** any CEC command is requested
**Then** it returns `Unsupported` at once without touching the bus

**Given** the fake adapters
**When** the unit suite runs
**Then** every command, result kind, timeout and retry path is covered

### Story 12.6: The idle TV shows branding, console rates and a "Scan to start" QR

As a player walking past a free console,
I want the TV to show what it costs and a code I can scan,
So that I know the price and can ask for a session from my phone.

**Covers:** FR143, AD-17, AD-42 (QR token), UX-DR53, UX-DR26, UX-DR15, UX-DR94, UX-DR99, UX-DR104 (no ads on Pi), UX-DR108

**Acceptance Criteria:**

**Given** a linked Pi whose console tile is Free
**When** the TV is on the Pi input
**Then** it shows `tv-idle` at 1920×1080: full-bleed `game-media` (Story 6.6) cycling curated videos of console games in the branch library from the Hub's `/media` with the device token, falling back to cover then solid `bg`, under `scrim`; `logo-slot` top-left; "TV n" in `kiosk-headline` and the console group's plans and rates in tabular figures bottom-left in the copy zone; and a QR of at least 360 px with a white quiet zone and `rounded.md` frame bottom-right labelled "Scan to start"; there is no ad slot anywhere

**Given** the Hub
**When** it issues the QR for a console
**Then** the QR encodes `http://arena360.local:7401/console/<tv>?t=<token>` where the token is a random per-console value that rotates every 5 minutes, the previous token stays valid for one more period, and the TV redraws the QR on each rotation

**Given** the primary Hub
**When** it is running
**Then** it advertises the host name `arena360.local` by mDNS pointing at its LAN IP (a standby does not advertise it until promoted), and Settings → Consoles shows the QR link it encodes

**Given** the group rate, a console plan, a pricing rule or the branch logo changes
**When** the Hub pushes the change
**Then** the TV's rates and logo update within 1 min without a reload

**Given** a cold start
**When** the page first paints
**Then** branding, TV number and rates come from the `/data` cache before any network call, and media streams in afterwards

**Given** media cannot play (decode error, or the Pi drops frames past the agent's threshold)
**When** it renders
**Then** it falls to the cover for the rest of that view with no visible error; HC skips straight to solid `bg` with no video

**Given** a console tile that is in maintenance, has "CHECK CONTROLLERS" open, or has no plan for its group
**When** `tv-idle` renders
**Then** the QR is replaced by "Ask at the counter" and no token is issued

### Story 12.7: The phone start page requests a console plan with no login

As a player with my phone,
I want to scan the TV's code, pick a plan and tap Request,
So that the counter knows which TV I want and what I want to pay for.

**Covers:** FR144 (QR page, request), AD-42 (anonymous `phone` class, plain HTTP), AD-23, NFR23, UX-DR56, UX-DR48, UX-DR94, UX-DR103, UX-DR109

**Acceptance Criteria:**

**Given** a phone on the café Wi-Fi scanning the QR
**When** it opens the URL
**Then** the Hub's `:7401` listener serves the phone page on the `/console` path over plain HTTP (TLS clients on the same port unaffected), and it renders `qr-start-page` in one portrait column with 16 px gutters: `logo-slot` and "TV n", the console plans valid for that console's group as `plan-picker` cards with live tabular prices, and one primary "Request" button; there is no login, sign-up or account field

**Given** a valid token
**When** the player picks a plan and taps Request
**Then** the page calls `console.plan_requested{tv, plan_id, token}` as the `phone` class; the Hub writes a `console_requests` row (new table: id, device_id, plan_id, status, request-token hash, created_at) and returns a 128-bit request token that the page keeps in browser storage; the page shows "Waiting for the counter — pay at the counter to start."

**Given** an expired or unknown token
**When** the page loads or requests
**Then** it shows "This code has expired — scan the TV again." and nothing is written

**Given** the console is in session, in maintenance, or already has an open request
**When** a request is made
**Then** it is refused, the page shows "Ask at the counter.", and the open request is not changed

**Given** a phone that cannot resolve `arena360.local` (some Android phones do not resolve `.local` names)
**When** it scans the QR
**Then** the phone page does not load and nothing reaches the Hub; the players ask at the counter and staff start the session from the console tile as in Story 3.35, with the TV following the session as normal

**Given** rate limits
**When** one TV receives more than 6 requests in 10 minutes, or one IP more than 20 in an hour
**Then** further requests return `RATE_LIMITED` and the page shows "Ask at the counter."

**Given** a `phone`-class call
**When** it targets anything other than `console.plan_requested` or its own timer, or presents a paired or `kds` token over plain HTTP
**Then** the Hub returns `FORBIDDEN` and logs it; a CI test asserts the `phone` scope contains no staff, money, player or device intent and that no player name or phone number is ever in a `/console` response

### Story 12.8: Staff confirm or refuse a QR request from the console tile

As counter staff,
I want a console request to alert me with the plan already chosen,
So that I can take payment and start the TV in a couple of taps, or turn it down.

**Covers:** FR144 (REQUEST badge, staff start, refusal), UX-DR62, UX-DR22, UX-DR107, UX-DR101, UX-DR104

**Acceptance Criteria:**

**Given** a new `console_requests` row
**When** it is written
**Then** the console tile raises `alert-badge` "REQUEST" with sound through the Story 3.25 alert service, and the header alert list shows "Request · TV n · <plan>"

**Given** staff open that tile's sheet
**When** it renders
**Then** the requested plan is pre-selected in the start area, staff pick a player by lookup or Guest, and any start mode valid for the group (postpaid, pack, wallet, pass, block) can still be chosen instead

**Given** staff tap Start
**When** `session.start{request_id}` applies
**Then** the request becomes `started` and is linked to the session, the badge clears, and the audit records the staff actor (the phone is never the actor of a start)

**Given** staff tap "Refuse"
**When** `console.request.refused` applies
**Then** the badge clears and the phone shows "Ask at the counter." within 2 s

**Given** a request unanswered for 10 minutes
**When** the timer engine reaches it
**Then** it becomes `expired`, the badge clears, and the phone shows "Ask at the counter."

**Given** the tile still shows "CHECK CONTROLLERS"
**When** a request arrives
**Then** it is accepted and badged, but Start stays disabled with "Mark controllers first" (Story 3.36)

### Story 12.9: The TV follows the console session — console input at start, idle at end

As a player,
I want the TV to switch to the console as soon as the counter starts my time,
So that I can pick up the controller and play straight away.

**Covers:** FR144 (TV switches ≤ 3 s), AD-12 (ticks to Pi), AD-28, NFR3, UX-DR53, UX-DR103

**Acceptance Criteria:**

**Given** a linked console
**When** a session starts on it
**Then** the Hub sends the Pi `session.started` and then `session.tick` frames with remaining or elapsed time, `lock_after_ms` (postpaid: until closing time) and `time_up_mode: lock`, and the Pi calls `show_console_input(port)`; the TV shows the console within 3 s of the Start tap

**Given** the input switch fails or the TV has no CEC
**When** the Pi reports the result
**Then** the tile's TV line shows "Failed — retry" (or "No CEC — switch inputs by hand"), Retry resends with the same idempotency key, and the session keeps running

**Given** staff pause the session (Story 3.15)
**When** `session.pause` applies
**Then** the Pi switches the TV to its input with "Session paused — see the counter", and on resume switches back to the console within 3 s

**Given** a session ends early by staff, or by forced end
**When** `session.end` applies
**Then** the Pi switches to its input and shows `tv-idle` within 3 s

**Given** Extend from the Hub (Story 3.28)
**When** it applies
**Then** the Pi receives the new `lock_after_ms` within 2 s

**Given** the Pi reconnects after a restart during a session
**When** it re-syncs
**Then** the Hub's record wins and the TV shows the console, the lock or idle to match it

### Story 12.10: The phone shows a live timer for its TV

As a player,
I want my phone to show how long is left on our TV,
So that we know when to wrap up without asking the counter.

**Covers:** FR144 (live timer only), AD-42 (own timer), AD-12 (phone page unavailable offline), UX-DR57, UX-DR98, UX-DR103

**Acceptance Criteria:**

**Given** a phone holding the request token of a started request
**When** the session starts
**Then** the page replaces the waiting line with `live-timer`: one `surface` card with "Time left on TV n", the time in tabular 96 px Sora 200 (elapsed for postpaid), and "Extend at the counter"; there are no buttons or links

**Given** the page is open
**When** the Hub pushes ticks on the timer read (scoped to that request token's session only)
**Then** the time updates every second and reflects an Extend within 2 s

**Given** the phone leaves the café Wi-Fi, or the Hub is unreachable
**When** updates stop for 5 s
**Then** the card keeps the last value and adds "Reconnect to café Wi-Fi to update"; it resumes on reconnect

**Given** the session ends
**When** the phone next updates
**Then** it shows "Time's up — see the counter." and the request token stops working 10 minutes later

**Given** a request token from another TV or an ended session past its window
**When** it is used to read a timer
**Then** the Hub returns `FORBIDDEN` and the page falls back to "This code has expired — scan the TV again."

### Story 12.11: TV warnings at 5 and 1 minute on the Pi input

As a player on a console,
I want the TV to tell me when 5 and 1 minutes are left,
So that I can finish the match or ask for more time.

**Covers:** FR145 (warnings), AD-12, UX-DR54, UX-DR103, UX-DR110

**Acceptance Criteria:**

**Given** a running console session with a linked Pi
**When** 5 min and then 1 min remain (to `ends_at`, or to `closing_at` for postpaid)
**Then** the Pi switches the TV to its input and shows `tv-warning` for 5 s — solid `bg`, 8 px `warning` frame, "5 min left" (or "1 min left") in `kiosk-display` and "ask at the counter to extend" in `kiosk-headline` — then switches back to the console input

**Given** the warning marks
**When** the Pi computes them
**Then** it uses the marks from the Hub's ticks against its own monotonic clock, so a dropped tick never skips a warning, and each mark fires at most once per session

**Given** Extend moves the end past a mark that has not fired
**When** the new tick arrives
**Then** the mark is rescheduled; a mark that already fired does not fire again for the same remaining time

**Given** the switch back to the console fails
**When** the Pi gets `NoAck` twice
**Then** the tile's TV line shows "Failed — retry" and the TV stays on the warning screen until Retry or the next session event

**Given** a TV with no CEC
**When** a mark is reached
**Then** the Pi draws `tv-warning` on its own input only, and the tile still shows the Story 3.35 warning states

### Story 12.12: Time-up locks the TV with no grace, then the branch power action

As a café owner,
I want a console TV to stop the moment time runs out,
So that nobody plays on unpaid while staff are busy.

**Covers:** FR145 (time-up lock, CEC standby or plug per branch), AD-12, UX-DR55, UX-DR103

**Acceptance Criteria:**

**Given** a console session reaching zero
**When** the Pi's countdown or the Hub's time-up arrives (whichever is first)
**Then** the Pi immediately switches the TV to its input and shows `tv-lock` — solid `bg`, `logo-slot`, and "Time's up — see the counter." in `kiosk-headline`, nothing else — with no grace; the tile's "TIME UP" alert is Story 3.36's

**Given** Settings → Consoles → "At time-up" with Lock screen only · Lock, then TV standby · Lock, then smart plug off (disabled until a plug adapter exists)
**When** "Lock, then TV standby" is set and 30 s pass after the lock
**Then** the Pi sends CEC standby; the result is recorded as `tv.command.result`

**Given** the TV is locked
**When** the CEC bus shows another source becoming active (a player switched input with the TV remote) or the TV is powered back on
**Then** the Pi switches back to its input and re-shows `tv-lock` within 3 s

**Given** staff end the session
**When** `session.end` applies
**Then** the Pi powers the TV on if it was put in standby, shows `tv-idle`, and the tile carries on to "CHECK CONTROLLERS"

**Given** a postpaid console session reaching branch closing time (`lock_after_ms` from opening hours)
**When** it reaches zero
**Then** the TV locks the same way with the same copy and power action

### Story 12.13: Remote TV commands from the tile sheet — power, input and message

As counter staff,
I want to turn a TV on or off, switch its input or put a message on it from the counter,
So that I can sort out a TV without walking over.

**Covers:** FR147 (remote commands ≤ 5 s), NFR3, AD-25, UX-DR65, UX-DR101 (Failed — retry), UX-DR104

**Acceptance Criteria:**

**Given** a linked console's sheet with the Pi connected
**When** it renders
**Then** a "TV" section offers Power on, Power off, Show console, Show TV screen and Message, each one tap from the open sheet; actions the Pi reported `Unsupported` are shown disabled with "No CEC on this TV"

**Given** a Counter chooses Show console, Show TV screen or Message (text up to 140 characters)
**When** `tv.command` applies (permission `device.screen`)
**Then** the Pi runs it; a message switches the TV to the Pi input, shows the text in `kiosk-headline` on solid `bg` for 10 s, then returns to the previous input

**Given** Power on or Power off
**When** chosen by a role without `device.remote`
**Then** `inline-pin` opens in the sheet, and the command is audited with both actors

**Given** Power off on a console with an active, paused or expired-unended session
**When** requested
**Then** it is refused `CONSOLE_BUSY` with "End or pause the session first"

**Given** any TV command
**When** the Pi's result arrives within 5 s
**Then** the sheet shows it inline; otherwise the tile's TV line shows "Failed — retry", and Retry resends with the same idempotency key

**Given** the Pi is offline
**When** the sheet renders
**Then** the TV actions are disabled with "TV offline (last seen …)"

### Story 12.14: The Pi enforces the session while the Hub is unreachable

As a café owner,
I want a console TV to keep warning and locking on time even if the counter PC drops,
So that a Hub problem never means free play.

**Covers:** FR147 (offline enforcement), AD-12, AD-14, NFR1, NFR8, UX-DR98

**Acceptance Criteria:**

**Given** the Pi loses the Hub (no reply within 5 s or socket closed) during a session
**When** it stays unreachable
**Then** the agent counts `lock_after_ms` down on its monotonic clock (a wall-clock change has no effect), shows the 5- and 1-minute warnings and the time-up lock at the right moments, and runs the branch time-up power action

**Given** the Pi is unreachable from the Hub
**When** anything happens on the Pi
**Then** it writes no session or request state and queues nothing that changes branch data; command results it could not send are dropped and the Hub shows them as "Failed — retry"

**Given** the TV is idle while the Hub is unreachable
**When** `tv-idle` renders
**Then** the QR is replaced by "Ask at the counter" (the phone page cannot work) until the Hub is back

**Given** the Pi reconnects with jittered backoff (2–30 s), or a promoted standby takes over (standby fingerprint pinned per Story 9.16, epoch rules per Story 9.19)
**When** it re-syncs
**Then** the Hub's record wins: a session the Hub ended shows `tv-idle`, an extended one returns to the console, and the TV line on the tile clears "TV offline"

**Given** an integration test with the Hub stopped for 30 minutes during a prepaid console session on the fake TV adapter
**When** it runs
**Then** warnings fire within 1 s of 5 and 1 min, the lock within 1 s of zero, the Pi writes nothing, and after restart Hub and Pi agree

### Story 12.15: A hardware lab rig for console TVs runs the TV suite in CI

As a developer on Arena 360,
I want a permanent rig of real Pis and TVs that CI can drive,
So that every release is checked on the hardware cafés actually buy.

**Covers:** NFR18, FR142 (BOM < ₹4,000)

**Acceptance Criteria:**

**Given** the lab
**When** the rig is set up
**Then** it has a Pi Zero 2 W and a Pi 4 on the Story 12.1 image, at least two TV brands with CEC and one without, a console or CEC test source on each TV, a Hub on reference hardware, and a phone-browser runner on the same LAN, all reachable from a self-hosted CI runner

**Given** the rig
**When** a test needs to know what the TV shows
**Then** an HDMI capture device on each TV path reports the active input and a frame, so input switches and screens are asserted on the real signal, not on the agent's own report

**Given** the release pipeline
**When** a release candidate for the Pi agent or Hub is built
**Then** the rig flashes the new image, pairs each Pi with a fresh code and links it to its console, and fails the job with a clear message if any step does

**Given** the help-centre hardware list
**When** it is published with the release
**Then** it names the Pi Zero 2 W kit (board, case, power supply, microSD, mini-HDMI cable) with a total under ₹4,000, the Pi 4 option, and each TV model tested on the rig with its CEC result

### Story 12.16: Console TVs proven on the rig against their targets

As a café owner,
I want the TV path measured on the boards and TVs I'm told to buy,
So that the cheap kit I set up works the way the counter expects.

**Covers:** FR144 (≤ 3 s), FR147 (≤ 5 s), NFR3, NFR15, NFR18, UX-DR53–UX-DR57

**Acceptance Criteria:**

**Given** the Story 12.15 rig
**When** the release suite runs
**Then** console input switch at start is ≤ 3 s at p95 over 50 starts, every remote command result is ≤ 5 s at p95, and both 5-s warnings return to the console every time on the CEC TVs

**Given** the Pi Zero 2 W
**When** `tv-idle` runs for 8 hours
**Then** the agent stays under 60 MB RAM, Chromium does not restart from memory pressure, and media falls back to cover rather than stuttering

**Given** Playwright against the TV page and the phone page in normal and HC themes
**When** contrast and axe checks run
**Then** load-bearing pairs meet 4.5:1 text and 3:1 non-text with zero serious or critical violations, and the phone page targets are ≥ 44 px

**Given** a full Journey 10 run (scan, request, refuse, request, start, warnings, time-up, end, controller check)
**When** it runs on the rig with the WAN blocked
**Then** every step completes with the Voice lines from the catalogue and no outbound connection is attempted

**Given** any target missed
**When** the suite finishes
**Then** the release is blocked and the report names the board, TV and step

## Epic 13: Bookings, tournaments, loyalty and game accounts

Cafés grow revenue with reservations, tournaments, loyalty and referrals, memberships, a launcher-account vault, game requests and migration from competitors.

> Draft notes: this epic builds on the Epic 1 shell, `apply()`, idempotent intents and the `proptest` generator (Stories 1.5–1.6), inline PIN (Story 1.11) and the audit viewer (Story 1.13); on Epic 2 devices, groups, zones and opening hours (Stories 2.19, 2.22, 2.25) and the wizard's Hub-progress steps (Story 2.27); on the Epic 3 session engine: tile sheet (Story 3.5), player profiles and lookup (Stories 3.6–3.7), postpaid start (Story 3.9), settle and `settle-bar` (Stories 3.12–3.13), Undo (Story 3.14), wallet (Story 3.16), plans and plan sales (Stories 3.21–3.22), the timer engine (Story 3.23), alerts with sound and badge (Story 3.25), time-up (Story 3.26), pass sessions (Story 3.27), transfer (Story 3.32) and forced end (Story 3.34); on Epic 5 tax per rate (Story 5.2), invoice numbering (Story 5.14), notification settings (Story 5.15), print layouts and the side-effect outbox (Stories 5.16–5.17), refunds with credit notes (Story 5.24), the Z-report (Story 5.27), Hub reports and exports (Stories 5.29, 5.32) and the CSV import engine (Stories 5.33–5.34); on Epic 6 the Hub catalogue cache (Story 6.2), the branch library (Story 6.3), the plan picker (Story 6.9), kiosk sign-up (Story 6.10), session-end cleanup and launcher sign-out (Stories 6.11–6.12), the Games tab (Story 6.23), launcher cards (Story 6.26), the kiosk Account page (Story 6.34) and the Hub-unreachable rules (Story 6.36); on Epic 8 rollups and schema bumps (Stories 8.5–8.7), owner reports (Story 8.13), the command catalogue and Hub apply (Stories 8.16–8.17), the command preview (Story 8.19), Manage → Pricing (Story 8.21) and the messaging adapter and outbox drain (Stories 8.26–8.27); and on Epic 9 erasure, key backup, export and standby replication (Stories 9.1, 9.2, 9.4, 9.17). **Where screens live (decided for this draft):** the spine-only Sessions screen gains tabs Sessions · Bookings · Tournaments, so `hub-rail` keeps its nine items; café accounts and game requests are tabs of Settings → Games; importers sit in Settings → Import beside the Epic 5 CSV importers. **Money model (decided for this draft):** a booking deposit is an advance shown under "Payments received, not sales" with a payment receipt and no invoice; at seating it moves to the player's wallet; a forfeited deposit becomes a "No-show fee" sale with a tax invoice; tournament entry fees are a separate revenue type "Tournaments" — the "tournament income account" is that revenue type, not a general ledger; loyalty redemptions and referral bonuses are promotional wallet credit, reported apart from payments and sales. **Tiers:** bookings, tournaments, loyalty, referrals, memberships, the vault, game requests and importers run on every tier; WhatsApp reminders and owner commands stay paid-only as in Epic 8. **PII:** bookings, entrants, referrals and loyalty rows reference `player_id` only; screens show the username or "Erased player". **Out of this epic:** public online booking and assisted migration (SPEC non-goals), prize payouts (recorded as expenses with Story 5.25), team tournaments and formats other than single elimination, loyalty point expiry, and any coins, quests or points on the kiosk top bar (gamification stays banned). **Gates and sign-offs:** the launcher-vault stories (13.16–13.20) are gated — legal sign-off on Steam, Epic and Riot terms for shared café accounts must be recorded before build starts, and each carries that precondition AC; the tax treatment of booking deposits, no-show fees and promotional wallet credit needs CA sign-off before GA; the ggLeap and Smartlaunch source profiles (Stories 13.24–13.25) need real export samples from beta cafés before they are built. Every story adds its new intents to the Story 1.6 `proptest` generator.

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

**Given** a start in the past, a length outside 30 min–12 h or not in 15-min steps, a start more than 30 days ahead, or a slot outside opening hours (Story 2.25)
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

**Given** Reports → Bookings (added to the Story 5.29 list)
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
**When** the drain (Story 8.27) re-checks the row
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
**Then** the Story 9.2 key backup includes the wrapped vault data key and ciphertext, and Story 9.17 replication carries them to the standby over LAN

**Given** the command catalogue, change-log batches, snapshots, the export file (Story 9.4), the Owner API and the Hub log
**When** a CI privacy test creates an account and scans all of them
**Then** no plaintext login name, password or secret appears, and no command kind can carry vault fields

**Given** a library entry owned by at least one café account
**When** the library list renders
**Then** it shows "Café accounts: n"

### Story 13.17: The Hub lends one vault account per seat and takes it back at session end

As a branch manager,
I want each café account used by only one PC at a time and returned when the session ends,
So that two players never fight over one login and nobody keeps our account.

**Covers:** FR138 (one account per concurrent seat, released at session end), AD-2, AD-12, NFR10, UX-DR22, UX-DR65, UX-DR107

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Steam, Epic and Riot shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** a player in an active session asks to play a library entry with café accounts
**When** `vault.checkout{session_id, entry_id}` (player self-service permission `vault.self_checkout`) applies
**Then** it picks the least recently used account owning that entry that is neither leased nor quarantined and writes a `vault_leases` row (new table: account, session, device, state `active`, at), enforced by a unique partial index of one active lease per account and one per session per launcher

**Given** the session already holds an active lease for that launcher on an account that owns the entry
**When** another checkout arrives
**Then** the existing lease is returned and no second lease is created

**Given** every account owning the entry is leased or quarantined
**When** a checkout arrives
**Then** it is refused `VAULT_BUSY`

**Given** a session with an active lease ends by any path (player, staff, timer, forced end, transfer, orphaned)
**When** the end applies
**Then** in the same transaction its leases move to `releasing`, and a kiosk report `vault.signed_out` moves them to `released`

**Given** a lease in `releasing` that gets a sign-out failure report, or no report within 5 minutes
**When** the timer engine checks it
**Then** the lease becomes `quarantined`, the account is not lent again, and the tile raises `alert-badge` "CHECK PC" with "Café account <label> may still be signed in"

**Given** Settings → Games → Café accounts
**When** it renders
**Then** each account shows "Free", "In use · <device> · since <time>" or "Needs check", and a Manager can "Mark signed out" a quarantined account with `inline-pin`, audited

**Given** two checkouts for the last free account arriving together
**When** both reach `apply()`
**Then** exactly one gets the account and the other gets `VAULT_BUSY`

### Story 13.18: A secure sign-in helper hands vault logins to launchers

As a branch manager,
I want the kiosk to fetch a café login only for the seat that leased it and keep it out of the player's reach,
So that our passwords never touch disk, logs or the screen.

**Covers:** FR138 (kiosk injects login at launch), AD-31, AD-34, AD-42, NFR11, NFR12, NFR13, UX-DR40, UX-DR101, UX-DR103

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Steam, Epic and Riot shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** the Games tab and a library entry with café accounts on a PC where its launcher is not signed in to a player's own account
**When** it renders
**Then** the `game-tile` shows a "Café account" text tag

**Given** the player activates that tile
**When** the kiosk handles it
**Then** the UI asks the kiosk service to launch; the service gets a lease (Story 13.17), then fetches the secret with a one-use `vault.secret{lease_id}` call over the pinned paired-device connection, which the Hub answers only for the leased device's token within 60 s of checkout; any other caller gets `FORBIDDEN`

**Given** the secret in the service
**When** it passes it to a launcher adapter
**Then** it goes through a short-lived helper started in the player's desktop session behind a `LauncherSignIn` interface with a recorded fake; the secret is held in memory only, zeroed after use, and never sent to the UI process, written to disk or logged

**Given** the adapter reports failure, or `VAULT_BUSY` returns
**When** the kiosk handles it
**Then** the lease is released and the account quarantined with the reason, and the player sees "Couldn't sign in — ask at the counter." or "All café accounts for this game are in use — try again soon."

**Given** session end
**When** Story 6.12 cleanup signs the launcher out
**Then** the service reports `vault.signed_out` or the failure, as Story 13.17 expects

**Given** the Hub is unreachable
**When** a café-account tile is activated
**Then** nothing is sent and the player sees "Counter is reconnecting — try again shortly."

**Given** an automated test with the recorded fake adapter
**When** a launch, play and session end run
**Then** a scan of kiosk logs, temp folders, crash dumps and the UI process memory finds no secret

### Story 13.19: The Steam adapter signs a café account into Steam

As a player,
I want to play a café-owned Steam game with one click,
So that I can play games I don't own without typing any login.

**Covers:** FR138 (Steam), FR134, AD-34, NFR12, UX-DR101

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Steam shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** a time-boxed spike (up to 3 days) before the adapter is built
**When** it ends
**Then** a written result names the sign-in method that works on current Steam on Windows 10 and 11 (login window automation, command line or other), how Steam Guard codes are produced from the authenticator secret, and what breaks it; the adapter is built only on a method the spike proved

**Given** the Story 13.18 helper and a Steam lease
**When** the Steam adapter runs
**Then** it starts Steam signed out, fills the login and any Steam Guard code, turns off "Remember me", then starts the game by its Steam id

**Given** a wrong password, extra verification or Steam offline
**When** sign-in is attempted
**Then** the adapter gives up within 30 s and reports the failure reason to the Story 13.18 failure path

**Given** an automated Windows 10 and 11 test with a real test Steam account
**When** a player launches, plays, and the session ends
**Then** the game starts signed in, the next session opens Steam signed out, and no password is found by the Story 13.18 scan

### Story 13.20: Epic Games and Riot Client sign-in from the vault

As a player,
I want café-owned Epic and Riot games to work the same way as Steam,
So that every café game is one click whatever launcher it uses.

**Covers:** FR138 (Epic, Riot), AD-31, AD-34, NFR12, UX-DR40, UX-DR101

**Acceptance Criteria:**

**Given** the launcher-terms gate
**When** build of this story starts
**Then** legal sign-off on Epic and Riot shared café accounts is recorded in the decision log; without it the story stays blocked

**Given** the Story 13.18 `LauncherSignIn` interface
**When** this story ships
**Then** it adds Epic Games and Riot Client adapters, each with a recorded fake, that sign in with the leased login and an authenticator code where the account has one, start the entry by its launcher URI, and never persist the login

**Given** an Epic or Riot account whose launcher asks for an emailed code or a captcha
**When** sign-in is attempted
**Then** the adapter gives up within 30 s, the account is quarantined with "Needs a code we can't enter — switch the account to an authenticator app", and the player sees the Story 13.18 failure line

**Given** session end
**When** Story 6.12 cleanup runs
**Then** Epic and Riot sign-out is confirmed to the Hub with `vault.signed_out`

**Given** the same automated test as Story 13.19 with Epic and Riot test accounts
**When** it runs on Windows 10 and 11
**Then** the game starts signed in and the next session opens each launcher signed out with no residual credential

### Story 13.21: Players request a game from the kiosk and the Hub collects the requests

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

### Story 13.22: Game request counts in Hub reports

As a café owner,
I want to see how often each game was requested over a period,
So that I buy or install the games my players actually want.

**Covers:** FR139 (request counts per game in reports), FR108, UX-DR88

**Acceptance Criteria:**

**Given** Reports → Game requests (added to the Story 5.29 list, permission `report.view`)
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

### Story 13.23: Import framework — source detection, field mapping and a dry-run report

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

**Given** onboarding steps 5 and 6 (Story 2.27)
**When** they render before go-live
**Then** they show "Moving from another system? Import at the Hub: Settings → Import."

### Story 13.24: Import a ggLeap players export

As a café owner moving from ggLeap,
I want the Hub to recognise my ggLeap export and map it for me,
So that switching takes minutes, not an afternoon of matching columns.

**Covers:** FR22 (ggLeap), FR7, AD-8 (versioned profiles), NFR6

**Acceptance Criteria:**

**Given** real ggLeap user exports collected from beta cafés
**When** the ggLeap source profile is built
**Then** it is added to `arena-proto` with its header signature, column mapping and a fixture-based test per export variant

**Given** a ggLeap user export
**When** it is picked in the Story 13.23 importer
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
### Story 13.25: Import a Smartlaunch export through the same mapping and dry-run

As a café owner moving from Smartlaunch,
I want to bring my Smartlaunch members across the same way,
So that switching doesn't depend on which system I used before.

**Covers:** FR22 (Smartlaunch), FR7, AD-24, NFR6, NFR13

**Acceptance Criteria:**

**Given** the Story 13.23 importer
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
**Then** they behave exactly as in Story 13.23

### Story 13.26: Migrated time balances and products from ggLeap and Smartlaunch

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

### Story 13.27: Owner commands for loyalty, referral and booking rules and memberships

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
**Then** Plans & rates also lists memberships, and a new "Loyalty & bookings" tab shows each branch's current values with data age; Send opens the Story 8.19 preview with no MFA prompt

**Given** a command applied at the Hub (Story 8.17)
**When** a local manager changed the same setting earlier
**Then** the owner's value wins and the Hub shows "Updated by owner" naming the overwritten change

**Given** a Free branch
**When** the owner opens these tabs
**Then** they follow Story 8.11 and show only the upgrade path

### Story 13.28: Rollup schema v2 carries tournament income, deposits, loyalty and bookings

As a café owner on a paid tier,
I want my remote reports to show tournament income, deposits, rewards and bookings,
So that what I see on my phone matches the Hub's own reports line for line.

**Covers:** FR43 (tournament income in reports), FR42, FR55, FR139, AD-35, AD-8, NFR10

**Acceptance Criteria:**

**Given** `arena-proto`
**When** rollup schema v2 is defined
**Then** it is v1 plus `revenue.by_type.tournaments` and `revenue.by_type.no_show_fees`, `deposits {taken, applied, refunded, kept}`, `promo_credit {loyalty, referral}`, `loyalty {points_earned, points_redeemed}`, `bookings {made, seated, no_show, cancelled}`, `memberships {sold, renewed}` and `game_requests [{game_key, name, count}]`, in integer minor units

**Given** the cloud
**When** this story ships
**Then** it accepts schema 2 and 1 and refuses older with `ROLLUP_SCHEMA_OLD`, and the Hub regenerates earlier days oldest first through Story 8.7

**Given** Owner Reports → revenue (Story 8.13)
**When** a range includes v2 days
**Then** Tournaments and No-show fees show as their own rows, deposits and promotional credit show outside revenue, and v1 days show those rows as "—"

**Given** golden tests over a fixture with bookings, deposits, no-shows, tournaments with refunds, loyalty, referrals and memberships
**When** the v2 rollup is built
**Then** it equals the expected file exactly, and revenue, payments and cash equal the fixture's Z-reports for the same day

### Story 13.29: Bookings, tournaments, loyalty and the vault proven against their invariants

As a café owner,
I want these features tested under load and failure like the rest of the counter,
So that a busy tournament night or a WAN outage never double-books a PC, loses a deposit or leaks a password.

**Covers:** NFR1, NFR3, NFR4, NFR10, NFR12, NFR13, AD-2, AD-14

**Acceptance Criteria:**

**Given** the Story 1.6 `proptest` generator with every intent from this epic
**When** it runs on every push
**Then** no device ever has overlapping confirmed bookings, group bookings never exceed capacity, points and wallets never go negative, an account never has two active leases, deposits taken always equal applied + refunded + kept + held, and replaying the change log reproduces the database

**Given** a 60-device branch on reference hardware with 200 bookings and a 32-entrant tournament in one day
**When** windows open, no-shows fire and results are entered
**Then** every Reserved and free transition shows on the floor within 2 s, and `apply()` stays under 100 ms at the 5 intents/s sustained load

**Given** the WAN blocked for the whole day
**When** the same run executes
**Then** every step completes with no outbound connection, and queued renewal reminders send once after the WAN returns

**Given** a failover to the standby (Story 9.19 path) during an active café-account lease
**When** the new main Hub takes over
**Then** the lease and vault ciphertext are present, the account is not lent twice, and session end still releases it

**Given** a privacy scan after the full run
**When** it reads batches, snapshots, exports, key backups, the Owner API and all logs
**Then** no vault secret or player name, phone or email appears in plaintext

**Given** any target missed
**When** the suite finishes
**Then** the release is blocked and the report names the invariant or step

## Epic 14: Deeper back office, owner depth and platform ops

Managers run stock counts, purchase orders, petty cash, attendance and prep stations; owners get scheduled reports, cohorts, team logins and approvals; the platform gains off-site standby, auto-updates, a demo and compare pages.

> Draft notes: this epic builds on the Epic 1 `apply()`, change log, idempotent intents and `base_rev` (Stories 1.5–1.6), the LAN envelope and class scope table (Story 1.7), roles and permissions (Story 1.9), locale and money formatting (Story 1.10), inline PIN (Story 1.11), the audit viewer (Story 1.13) and per-device HC (Story 1.19); on the Epic 2 cloud, owner sign-in with TOTP (Stories 2.3–2.5), the heartbeat with `cloud_time` and licence re-sign (Story 2.10), the entitlement API and `trusted_now` (Story 2.11), the setup-command mailbox (Story 2.14), the Hub installer (Story 2.9), device pairing and the limiter (Stories 2.17–2.18), device groups and maintenance (Stories 2.19, 2.23), opening hours (Story 2.25), printer detection (Story 2.26) and Account → Branches (Story 2.29); on Epic 3 sessions and their "open" states (active, paused, grace), the tile sheet (Story 3.5), the alert service (Story 3.25) and pricing rules with the resolver (Stories 3.29–3.31); on the Epic 4 Android Hub and its Play rollout (Story 4.16, unchanged here); on Epic 5 stock-in with lots (Story 5.5), waste (Story 5.6), the shift (Stories 5.10, 5.25–5.27), refunds with credit notes (Story 5.24), Hub reports and exports (Stories 5.29–5.32); on Epic 6 staff messages to every PC (Story 6.16) and the kiosk Hub-unreachable rules (Story 6.36); on the Epic 7 kitchen ticket model, KOT, board and pairing (Stories 7.1, 7.3, 7.5–7.7). From Epic 8 it uses, by title: "Rollup schema v1 and the daily rollup at close", "Rollups regenerate after a schema bump, and the cloud accepts only the current and previous schema", "The owner read API serves the live summary and rollup ranges from files", "The Hub publishes a branch summary every cadence" (the summary `catalogue` and `attention` items), "The Owner app shell — navigation, themes, install and Free limits", "Reports — revenue and cash reconciliation", "Owner reports export to CSV and PDF and match the Hub's Z-reports exactly", "The cloud accepts owner command kinds, checks them, and fans "all branches" out", "The Hub applies owner commands exactly once, and the owner's change wins", "Commands are swept, expire after 7 days, and are fenced by epoch", "Preview the target branches and follow each command row to Applied", "Step-up MFA for sensitive commands", "Manage → Pricing — plans and rates, pricing rules and coupons", "Manage → Devices and groups, and revoking a device", "The cloud sends player and owner messages through a swappable messaging adapter, exactly once", "The Hub drains its notification outbox after the undo window", "Owner alerts by web push and WhatsApp, and the Alerts tab", "The Hub raises owner alerts for cash variance, low stock, overdue credit and large refunds or discounts", "The cloud alerts the owner when a branch goes offline during opening hours", "A Cloudflare Tunnel for entitled branches, run by the Windows Hub" and "Remote kiosks connect through the Tunnel with their device token". From Epic 9 it uses, by title: "The cloud issues epoch counters by compare-and-set on the branch fence", "Owner issues a standby code and a second PC pairs as standby", "The standby bootstraps from the main Hub and follows it live", "The standby keeps its own catalogue, media and licence and shows its status", "Promote the standby to main with a manager PIN", "Force promote after a data-loss warning", "A returning old main Hub locks itself" and "One-tap full database export to a folder or USB drive". From Epic 10 it uses, by title: "The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans" (`addon_owner_login` already priced), "Wizard step 2 and Upgrade take payment through Razorpay Checkout" and "Change tier and add-ons mid-cycle with proration" (both show Owner login as "Available soon" until Story 14.23). From Epic 11 it uses, by title: "Marketing site shell — navigation, footer and light and dark themes", "Pricing — Cafés tab with Monthly · Yearly, equal plan cards, add-ons and FAQ", "Downloads with installers, system requirements and checksums", "Get started hands off to sign-up, and funnel events are counted without personal data" (funnel events reused by the Demo page) and "Admin app behind Cloudflare Access, with every action audited" (status incidents are posted there). From Epic 12 it uses the signed read-only Pi image ("A signed, read-only Pi image is built in CI"). **Invariants kept:** every Hub business change goes through `apply()` — the one allowed exception (decided) is device health telemetry (`device_health`, `device_outages`, Story 14.13), which is not business state and is never synced as batches, snapshotted, exported or audited; every owner write is a command through the mailbox and applies once; promotion and restore open a new epoch through the fence; paid features are gated only by licence flags (or, for owner-side features, by `gz_control` entitlements); the cloud never runs SQL over branch rows — every owner figure in this epic is a rollup or summary field. **Tier gating (decided):** stock counts, suppliers and POs, petty cash, attendance and device health at the Hub are on every tier; prep stations need the `kitchen` flag (Basic+); scheduled reports, deeper dashboards, team logins, refund approvals, message to all kiosks, tenant-wide pricing and the device-offline owner alert need a paid branch (Owner app); off-site standby needs the new `offsite_standby` flag (Premium only, Windows Hub only). **Ticket model amended:** Epic 7 opened one kitchen ticket per placement; from Story 14.10 a placement opens one ticket per prep station that has kitchen lines in it; a branch with no stations defined behaves exactly as Epic 7. **Scheduled reports (decided):** WhatsApp only, through the existing messaging adapter; there is no email channel or email provider. **Rollup schema v2:** Stories 14.14–14.15 add the fields the deeper dashboards need; the cloud accepts v2 and v1 (N and N−1) (Story 14.16) and the licence pin is raised per tier by the control plane, so v1-only Hubs keep reporting. **Updates (decided by the spine):** Windows Hub and Kiosk use the Tauri updater with ed25519-signed manifests staged by control-plane percentage; the Pi agent is ed25519-signed; kiosks and Pis update only from the Hub's verified cache; nothing installs while any session at the branch (Hub) or on the device (kiosk, Pi) is active, paused or in grace; the standby updates before the primary; rollback is a roll-forward hotfix once a migration has committed. The Android Hub keeps Play staged rollout (Story 4.16) and is out of this epic except as a kiosk-update cache. **Out of this epic:** central purchasing across branches, payroll hints and accounting exports (v2, SPEC non-goals), smart-plug vendors, and the Epic 13 bookings, tournaments, loyalty and vault. Every Hub story adds its new intents and their `.reverted` twins to the Story 1.6 `proptest` generator.

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

**Given** the Z-report (Story 5.27)
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
**When** the Manager sets its KOT printer (Receipt printer, the branch KOT printer, Another printer via the Story 2.26 flow, or None)
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

### Story 14.13: Kiosks report device health and the Hub shows it

As a branch manager,
I want to see each PC's last contact, kiosk version and hardware at a glance,
So that I can spot a failing or outdated PC before a player complains.

**Covers:** FR165 (last seen, kiosk version, GPU/RAM), AD-25, AD-8, NFR2, UX-DR23, UX-DR88 (Settings → Devices), UX-DR97

**Acceptance Criteria:**

**Given** the kiosk service
**When** it sends `hello` and then every 5 minutes while connected
**Then** it reports `health{kiosk_version, os_build, cpu_model, ram_total_mb, ram_used_pct, gpu_model, gpu_driver, disk_free_gb, uptime_s}` collected by the service (WMI) without measurable GPU use; the Pi agent reports `health{agent_version, board, ram_total_mb, ram_used_pct, cec}` the same way

**Given** a health frame
**When** the Hub receives it
**Then** it upserts a `device_health` row (new table, one per device, not part of the change log: device_id, last_seen_at, fields above, updated_at) outside `apply()` and the change log: device health is an allowed exception to the single-writer rule because it is telemetry, not business state — it is never synced to the cloud as batches, never in snapshots or exports, and never audited

**Given** Settings → Devices
**When** it opens
**Then** each device row shows name, group, kind, last seen ("Online" or "Last seen 8:42 pm"), version, RAM and GPU summary, with a warning `status-badge` "Old version" when the kiosk is more than one release behind the Hub and "Low disk" under 5 GB free

**Given** a device sheet on the floor (Story 3.5)
**When** opened
**Then** a "Health" line shows version, last seen, RAM used and GPU, and a device in maintenance (Story 2.23) is marked as such

**Given** the branch summary
**When** it is published (Story 8.4)
**Then** it gains a `devices` block per device (id, name, group, kind, online, last_seen_at, version, ram_total_mb, gpu_model, maintenance) with no player data

**Given** a kiosk from the previous two releases that does not send `health`
**When** it connects
**Then** last seen still updates from the connection and the other fields show "—"

**Given** a paired kiosk or Pi
**When** its connection drops and later returns
**Then** the Hub records the interval in a `device_outages` telemetry table (device_id, from, to null while open; kept 400 days), from which offline time can be reported

### Story 14.14: Rollup schema v2 adds device utilisation, staff, kitchen and device fields

As a café owner on a paid tier,
I want my Hub to put the numbers behind the deeper dashboards into its daily rollup,
So that heatmaps and staff rankings come from my branch's own data and never from a cloud copy of it.

**Covers:** FR160 (utilisation, staff data), FR96 (hours), AD-35 (versioned rollups, writers for N and N−1), AD-7, AD-24, AD-8, NFR5 (rollup < 5 s), NFR10, NFR13

**Acceptance Criteria:**

**Given** `arena-proto`
**When** rollup schema v2 is defined
**Then** it is v1 plus `utilisation: [{group_id, group_name, hour (0–23 local), device_minutes, capacity_minutes}]`, `staff: [{staff_id, name, sales, sessions_started, discounts: {count, value}, voids, refunds, shifts, variance_total, variance_abs_max, hours}]`, `kitchen: [{station_id?, station_name, items, median_ready_s, p90_ready_s}]` and `devices: {offline_minutes_open_hours: [{device_id, minutes}]}` (from `device_outages` clipped to opening hours), in integer minor units, with no phone numbers or names of players (player ids appear nowhere in the new fields)

**Given** the utilisation block
**When** it is built
**Then** `device_minutes` sums session minutes per device group per local hour, split at hour edges, and `capacity_minutes` is 60 × devices in the group not in maintenance during that hour within opening hours

**Given** golden tests with the Story 8.5 fixture extended with a 24-hour day, attendance and kitchen timings
**When** the v2 rollup is built
**Then** the JSON equals the expected file exactly, and every v1 field is byte-identical to the v1 build of the same day

**Given** a licence pinning `rollup_schema = 1`
**When** a rollup is built
**Then** the Hub emits v1 (it keeps writers for v1 and v2), and when the pin rises to 2 the Story 8.7 regeneration rebuilds past days oldest first at most 10 per minute; until a licence pins 2, v2 is built only in tests

**Given** the reference PC and the Story 8.5 busy day
**When** the v2 rollup runs
**Then** it completes in under 5 s, and the Story 4.17 device-farm job checks the same on a 3 GB Android device

### Story 14.15: Rollup schema v2 gains the player cohort block

As a café owner on a paid tier,
I want my Hub to compute new, returning and weekly retained players into the rollup,
So that cohort charts come from my branch's own data, with no player names leaving the café.

**Covers:** FR160 (player retention cohorts — data), AD-35, AD-7, AD-24, NFR5, NFR13

**Acceptance Criteria:**

**Given** rollup schema v2 from Story 14.14
**When** this story ships
**Then** v2 gains `players: {new, returning, active, cohorts: [{cohort_week, size, retained: [w1..w11]}]}` with counts only — no player id, name or phone

**Given** the cohort block
**When** it is built for a business day
**Then** it covers the 12 ISO weeks ending in that day's week, where a player's cohort week is the week of their first settled session or sale and `retained[k]` counts the cohort's players with a settled session or sale in cohort week + k

**Given** guests, erased players and players whose only activity was undone
**When** cohorts are built
**Then** they are excluded, and an erasure later lowers the cohort sizes of regenerated days only

**Given** golden tests with the Story 14.14 fixture extended with 12 weeks of players (new, returning, lapsed, erased)
**When** the block is built
**Then** it equals the expected file exactly, and new + returning = active for the day

**Given** the reference PC with 12 weeks of history and 5,000 players
**When** the full v2 rollup runs
**Then** it still completes in under 5 s, and the 3 GB Android device-farm check passes

### Story 14.16: The cloud accepts rollup v2 and the owner API serves the new fields

As a café owner on a paid tier,
I want the Owner app to read the new rollup fields as soon as my branch sends them,
So that new reports appear after one Hub update, while branches that haven't updated yet keep working.

**Covers:** FR160 (data path), AD-35 (cloud accepts N and N−1, pin per tier), AD-8, AD-10 (`rollup_schema` claim), NFR7, NFR25

**Acceptance Criteria:**

**Given** the cloud release that knows v2
**When** it is deployed (cloud first)
**Then** the accepted set becomes {2, 1}; a rollup with any other schema counts as missing with `ROLLUP_SCHEMA_OLD`

**Given** the control plane's per-tier `rollup_schema` pin
**When** an operator raises it to 2 for a tier with the audited `arena-cloud ops rollup-pin --tier <t> --schema 2 --reason <text>`
**Then** the next heartbeat of each branch on that tier signs a licence with `rollup_schema = 2`; a Hub whose heartbeat reports it cannot write v2 keeps `rollup_schema = 1`

**Given** `GET /v1/branches/{id}/summary?from=&to=&fields=players,utilisation,staff,kitchen,devices`
**When** it is called
**Then** the cloud sums `utilisation`, `staff`, `kitchen` counts and `devices` across the range in memory, takes `players.cohorts` from the latest v2 day in the range, and lists in `v1_days` any day whose file is v1 (so the app can say the new fields are missing for those days)

**Given** `branch=all`
**When** the new fields are requested
**Then** each branch is returned separately plus a total for counts and minutes; cohorts are never merged across branches

**Given** a Free branch or a branch not in the session's permitted branches
**When** the new fields are requested
**Then** it returns `FEATURE_NOT_ENTITLED` or `NOT_FOUND` as in Story 8.9

**Given** the jobs role's daily metrics pass
**When** it runs
**Then** the "rollup missing-day count" metric is also broken down by schema so a stuck v1 fleet is visible

### Story 14.17: Reports → Players with new, returning and weekly retention cohorts

As a café owner,
I want to see how many new players come back week after week,
So that I can tell whether my promotions build regulars or one-time visits.

**Covers:** FR160 (player retention cohorts, weekly chart), AD-35, NFR7, NFR15, UX-DR25, UX-DR81, UX-DR90 (Reports), UX-DR100, UX-DR110, UX-DR113

**Acceptance Criteria:**

**Given** Reports in the Owner app
**When** it opens on a paid branch
**Then** a new Players tab sits after Credit, with the range and branch pickers of Story 8.13

**Given** Players for one branch
**When** it renders
**Then** `metric-figure`s show new, returning and active players for the range, and a cohort chart shows one row per cohort week (week label, cohort size) and one cell per following week with the retained percentage, shaded by a single sequential scale and always printed as a number, so colour is never the only cue

**Given** the cohort chart
**When** used with a keyboard or screen reader
**Then** it is a real table with row and column headers, each cell's accessible name reads "Week of 6 Oct, 42 players, week 3: 31%", and a "Show as table" toggle is not needed because the table is the chart

**Given** All branches
**When** chosen
**Then** each branch's cohort chart is shown one after another (never merged), each with its own data age

**Given** days in the range that are v1 only
**When** the tab renders
**Then** a muted line says "Player cohorts start from 14 Nov, when this branch updated." and figures cover only v2 days

**Given** a branch with fewer than 5 players in every cohort
**When** it renders
**Then** it shows "Not enough players yet to show cohorts." instead of the chart

**Given** Export
**When** chosen
**Then** CSV and PDF exports of the tab are produced as in Story 8.15

### Story 14.18: Reports → Devices with a utilisation heatmap and device health

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

**Given** v1-only days in the range
**When** it renders
**Then** the muted "starts from" line of Story 14.17 is shown and the heatmap covers v2 days only

### Story 14.19: Reports → Staff sorted by variance, and branch comparison

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
**Then** CSV and PDF are produced as in Story 8.15

### Story 14.20: The owner is alerted when a PC goes offline during opening hours

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
**Then** device alerts are not raised (the Hub cannot see them) and the Story 8.33 branch-offline alert covers it

### Story 14.21: Owners choose daily or weekly scheduled reports on WhatsApp

As a café owner,
I want to choose which branches' reports I get on WhatsApp each morning or each week,
So that the numbers come to me without opening the app.

**Covers:** FR159 (schedule settings), AD-27, AD-34 (adapter), UX-DR18, UX-DR25, UX-DR90 (Reports → Exports), UX-DR113

**Acceptance Criteria:**

**Given** Reports → Exports → Scheduled reports on a tenant with a paid branch
**When** the owner adds a schedule
**Then** they pick frequency (Daily · Weekly, `tab-control`), branches (one, several or All), contents (Revenue, Cash reconciliation, Stock, Players, Staff; Revenue and Cash on by default) and it is sent to the owner's verified phone on WhatsApp only, stored in a new `report_schedules` table in `gz_control` (id, tenant, owner_id, frequency, branch ids or all, sections, timezone from the first branch, active)

**Given** the owner has turned WhatsApp off in Alerts → Notification settings (Story 8.31)
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

### Story 14.22: Scheduled reports are built from rollups and arrive by 8 am local

As a café owner,
I want yesterday's or last week's figures on WhatsApp before 8 in the morning,
So that I can check the café with my morning tea.

**Covers:** FR159 (arrive by 8 am local), AD-35 (rollups only), AD-27, AD-34, NFR10, NFR20, NFR25, UX-DR96

**Acceptance Criteria:**

**Given** the jobs role
**When** it is 07:00 in a schedule's timezone (daily), or 07:00 on Monday (weekly, covering the previous Monday–Sunday)
**Then** it builds the report from the same owner read API sums as Story 8.15 (rollups only, never branch rows) and queues one WhatsApp send with an idempotency key of (schedule, period); there is no email channel and no email provider

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

### Story 14.23: The owner-login add-on can be bought for a branch

As a café owner,
I want to buy extra owner logins for a branch,
So that my partner or manager can see that branch in the Owner app with their own account.

**Covers:** FR161 (owner-login add-on), FR178 (add-on ₹50 per branch per month), FR179 (prorated mid-cycle), AD-20, AD-29, UX-DR93, UX-DR24

**Acceptance Criteria:**

**Given** the control-plane flag `addon_owner_login_enabled`
**When** it is switched on
**Then** the add-ons panel in wizard step 2, Upgrade and "Change plan" (Stories 10.4, 10.8) shows "Owner login" with its band price and a quantity of 0–10 instead of "Available soon"

**Given** a paid branch and a quantity above 0
**When** the owner pays or confirms the change
**Then** the subscription carries `addon_owner_login × n` with the Story 10.8 proration, and the branch's `entitlements` row records `owner_logins = n` when the webhook is applied

**Given** a Free branch
**When** the add-ons panel renders
**Then** Owner login is not offered, since the Owner app on Free shows only Subscription, pairing and support

**Given** the subscription invoice (Story 10.6)
**When** it is issued
**Then** the add-on appears as its own line with quantity

**Given** a lapse or downgrade to Free
**When** the entitlement drops
**Then** `owner_logins` becomes 0 for that branch and no command or Hub state changes

### Story 14.24: Invite a team member with a read-only or full role

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
**Then** the change takes effect on the member's next request, their sessions are revoked on removal, the seat is freed, and the change is in the admin-visible tenant audit

**Given** the tenant's primary owner
**When** any member tries to change Team, Subscription or the primary owner
**Then** it is refused `FORBIDDEN`; only the primary owner can, and the primary owner cannot be removed

### Story 14.25: An invited team member signs up with OTP, password and TOTP

As someone invited to help run a café,
I want to accept the invite from my phone and secure my own login,
So that I get into the Owner app with my own account and never use the owner's.

**Covers:** FR161 (additional owner logins), FR2 (password and TOTP for every login), AD-10, AD-23, NFR11, NFR15, UX-DR18, UX-DR112, UX-DR113

**Acceptance Criteria:**

**Given** the invitee opens the link
**When** they confirm their phone by OTP
**Then** they set a password and enrol TOTP with backup codes exactly as in Stories 2.3–2.4 (an existing `owners` account signs in instead), the member becomes `active`, and the primary owner sees "Neha joined"

**Given** an expired or cancelled invite link
**When** it is opened
**Then** it shows "This invite has expired. Ask the owner for a new one." and nothing is created

**Given** wrong OTP codes
**When** 5 are entered
**Then** the challenge is void as in Story 2.3, and per-phone send limits apply

**Given** a member who joined
**When** they sign in on a new device later
**Then** the Story 2.5 TOTP, backup-code and SMS-fallback rules apply to them exactly as to the primary owner

**Given** the sign-up pages
**When** an axe scan runs in both themes
**Then** there are zero serious or critical violations, paste is allowed in every code field, and logs hold no phone number in clear

### Story 14.26: Team roles are enforced on every owner read and command

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

**Given** a removed member or a branch that lost its seat mid-session
**When** the next request arrives
**Then** it is refused `UNAUTHORIZED` or `NOT_FOUND`, cached screens on their device are cleared on that response, and queued offline commands are dropped with "Your access changed — this change wasn't sent"

**Given** an integration test with three members (Full on A, Read-only on A and B, removed)
**When** it calls every owner endpoint and command kind
**Then** each response matches the role and branch matrix, and the test fails on any leak

### Story 14.27: Refunds above the owner's threshold wait for approval at the Hub

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
**Then** `refund.create` is rejected `APPROVAL_REQUIRED` and, in its place, `refund.request` writes a `refund_requests` row (new table: id, order_id, lines and quantities, method, return-to-stock flags, amount_minor, reason, requested_by, state `pending`, `rev`) with no payment, credit note, stock or cash change

**Given** a pending request
**When** it commits
**Then** a `notify.requested` owner_alert of kind `refund_pending` (branch, invoice number, amount, reason, staff name; no player data) is written with `not_before` = commit + 8 s, and the refund screen and the order show "Waiting for owner approval"

**Given** a pending request
**When** the manager cancels it
**Then** `refund.request_cancel` moves it to `cancelled`, the owner alert is followed by a "cancelled at branch" entry, and nothing else changes

**Given** a second refund request for the same invoice while one is pending
**When** submitted
**Then** it is refused `REFUND_PENDING`

**Given** the branch offline or on Free
**When** a refund over the threshold is needed
**Then** on paid it still waits (the request reaches the owner when the WAN returns); on Free the threshold cannot be set and Story 5.24 is unchanged

**Given** a request pending for 7 days
**When** the business day rolls
**Then** it becomes `expired` and the Hub shows "Approval expired — ask again"

### Story 14.28: The owner approves or declines a pending refund from the Owner app

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
**Then** a `refund.approve` or `refund.decline` command (new non-sensitive kinds with `base_rev` = the request's `rev`) goes through the Story 8.19 preview to one branch and follows its `command-row` to Applied

**Given** the command at the Hub
**When** it applies
**Then** the request moves to `approved` or `declined` with the owner as actor; a request no longer `pending` (cancelled, expired or already decided) is rejected `REQUEST_CLOSED`, shown as "Couldn't apply at branch"

**Given** the Hub
**When** a decision applies
**Then** the alert service shows "Refund approved by owner — complete it" or "Refund declined by owner: <reason>" with sound and badge, on the refund screen and the order

**Given** a Read-only member
**When** they open Pending approvals
**Then** they see the list without Approve or Decline

### Story 14.29: Staff complete an approved refund at the counter

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

**Given** the owner alerts for large refunds (Story 8.32)
**When** an approved refund completes
**Then** no second "large refund" alert is sent for it, since the owner already approved it

### Story 14.30: The owner sends a message to every kiosk of a branch

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

### Story 14.31: Tenant-wide pricing rules with branch overrides flagged

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
**Then** a `rule.upsert` with the tenant values goes to those branches, the owner's change wins as in Story 8.17, `overridden` returns to 0, and the Hub toasts "Pricing rule updated by owner"

**Given** a tenant rule edited in the Owner app later
**When** pushed
**Then** branches with `overridden = 1` are excluded by default and the preview offers "Also overwrite 2 overridden branches"

### Story 14.32: Happy-hour automation across branches

As a café owner,
I want to set one happy hour that starts and ends by itself at every branch in its own local time and tells players when it begins,
So that quiet hours fill up without staff remembering to do anything.

**Covers:** FR162 (happy-hour automation), FR61, FR157, AD-5, AD-7 (branch local time), UX-DR50, UX-DR76, UX-DR90 (Manage → Pricing)

**Acceptance Criteria:**

**Given** Manage → Pricing → "New happy hour"
**When** the owner sets name, weekdays, start and end time, discount percentage, device groups (by name) and branches (one, several or All)
**Then** it is sent as a tenant rule (Story 14.31) with `kind = happy_hour` and `announce = 1`, resolved at each branch in that branch's timezone

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

### Story 14.33: The cloud relays WebRTC signalling and issues TURN credentials for Premium branches

As a café owner on Premium,
I want my main Hub and an off-site PC to find each other through the cloud without opening any port,
So that an off-site standby can connect from anywhere.

**Covers:** FR172 (STUN/TURN path), AD-29 (`offsite_standby` flag), AD-22, AD-23, NFR11, NFR14, NFR20; Spine §Deferred "TURN credential lifetime", "Off-site standby (WebRTC) detail"

**Acceptance Criteria:**

**Given** the tier table in `arena-proto`
**When** this story ships
**Then** Premium licences gain the `offsite_standby` flag (no add-on sells it), and the heartbeat signs it from the Story 10.5 claims

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

### Story 14.34: Pair an off-site PC as the branch's off-site standby

As a café owner on Premium,
I want to add a PC at my home or another site as a standby for my branch,
So that a fire, theft or power failure at the café can't take my data and my Hub at once.

**Covers:** FR172 (pairing), FR177 (standby codes), AD-10, AD-11, AD-42, UX-DR24, UX-DR66, UX-DR102

**Acceptance Criteria:**

**Given** Account → Branches on a branch with the `offsite_standby` flag
**When** the owner chooses "Add off-site standby"
**Then** a single-use 8-character code is shown as in Story 9.16, and at most one LAN standby and one off-site standby can exist per branch (`STANDBY_EXISTS` otherwise)

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
**Then** each receives its fingerprint (as in Story 9.16) and the branch tunnel hostname, so they can reach it after a promotion

**Given** an Android Hub or a branch without the flag
**When** an off-site standby is attempted
**Then** it is refused `PLATFORM_UNSUPPORTED` or `TIER_LOCKED`, and Account → Branches shows `tier-lock` "Available on Premium"

### Story 14.35: Two Hubs open an authenticated WebRTC data channel and replicate over it

As a café owner on Premium,
I want the off-site PC to receive the branch's changes live over the internet,
So that it stays seconds behind the café without any open port.

**Covers:** FR172 (follows over STUN/TURN), AD-4 (follow), AD-8 (`SCHEMA_BEHIND`), AD-24 (keys), AD-1, NFR9, NFR11

**Acceptance Criteria:**

**Given** an approved off-site standby
**When** it connects
**Then** both Hubs fetch ICE servers (Story 14.33), negotiate one reliable, ordered WebRTC data channel through the signalling relay, verify the DTLS fingerprint against the pinned Hub certificate (a mismatch closes the channel and alerts both Hubs), and run the Story 9.17 replication protocol unchanged inside it (device token in the first frame, `from_seq`)

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

### Story 14.36: An empty off-site standby bootstraps over the data channel in resumable chunks

As a café owner on Premium,
I want a brand-new off-site PC to copy the whole branch over the internet even on a shaky line,
So that it becomes a ready standby without anyone carrying a USB drive.

**Covers:** FR172 (bootstrap), AD-4 (standby bootstrap), AD-24, NFR9, UX-DR23

**Acceptance Criteria:**

**Given** an approved off-site standby with an empty database
**When** it first connects over the Story 14.35 channel
**Then** it requests a snapshot consistent at a stated `seq`, received in 1 MB chunks each with a SHA-256, then follows from that `seq` by the Story 14.35 rules

**Given** the channel drops mid-bootstrap
**When** it reconnects
**Then** it resumes from the last verified chunk of the same snapshot; if the main Hub no longer holds that snapshot (more than 24 hours old) it starts a fresh one

**Given** a chunk whose hash does not match
**When** it is received
**Then** it is requested again, and three failures in a row abandon the snapshot with "Copy failed — retrying"

**Given** the snapshot
**When** it is built and sent
**Then** player PII is ciphertext only and `player_keys` travel only inside the authenticated channel, never through the cloud

**Given** both standby screens (Story 9.18)
**When** they render
**Then** the off-site one is labelled "Off-site standby", shows "Copying branch — 42%" during bootstrap and then lag in rows and seconds, the path (Direct or Relayed) and last contact; the main Hub's System → Standby lists both standbys

**Given** the main Hub serving a bootstrap
**When** staff keep trading
**Then** `apply()` latency stays within NFR3 and the upload of chunks is throttled to leave the café's uplink usable (at most 50% of measured upstream)

### Story 14.37: Promote the off-site standby to main

As a café owner whose café Hub PC has died,
I want to make the off-site PC the main Hub with a PIN,
So that the branch has a working main Hub again within seconds.

**Covers:** FR172 (promotes with the same guarantees), FR169, FR170, FR171, AD-11, AD-38, AD-24, NFR8, UX-DR66, UX-DR102

**Acceptance Criteria:**

**Given** the off-site standby screen and no replication frame from the main Hub for 15 s
**When** a Manager or the Owner taps "Promote to main" and enters their PIN
**Then** the Story 9.19 steps run (schema check, epoch claim through the fence with `fork = {old epoch, last replicated seq}`, erasures re-applied, writer opened); because the off-site PC cannot see the café LAN, a provisional epoch is never used: promotion waits for the cloud claim and fails `CLOUD_REQUIRED` without it

**Given** the main Hub is still sending replication frames
**When** "Promote to main" is tapped
**Then** it fails `LEASE_FRESH`, and force promote (Story 9.20) is offered only after its data-loss warning

**Given** promotion
**When** it is measured from PIN confirmation to the first accepted intent at the off-site Hub (from its own UI)
**Then** it takes under 10 s on the reference PC

**Given** the old café Hub comes back
**When** it sees the higher epoch by heartbeat
**Then** it locks itself (Story 9.22) and offers "Make this the standby", which re-links it as the LAN standby of the off-site main; moving the main back to the café is a normal promotion of that standby later

**Given** every promotion of an off-site standby
**When** it completes
**Then** the audit records it with path and lag, and the owner receives an alert "Off-site standby is now the main Hub for <branch>"

### Story 14.38: The café's devices reach a promoted off-site main through the Tunnel

As counter staff at a café whose main Hub is now off-site,
I want the PCs, TVs and kitchen tablet to find the new main by themselves,
So that players keep playing and we keep selling without re-pairing anything.

**Covers:** FR172 (same guarantees for devices), FR169 (kiosks reconnect by themselves), AD-6 (Tunnel), AD-12, AD-42, NFR8, UX-DR98

**Acceptance Criteria:**

**Given** an off-site Hub that has just been promoted (Story 14.37)
**When** it opens the writer
**Then** it starts the branch's Cloudflare Tunnel connector with the branch credential fetched from the cloud, so the branch tunnel hostname now reaches it

**Given** café kiosks and Pis that lost the old main
**When** mDNS finds no Hub for 30 s
**Then** they try the branch tunnel hostname learned in Story 14.34, connect with their device token over TLS pinned to the off-site fingerprint, present the highest epoch they have seen, and reconcile sessions by the Story 6.36 rules

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
**Then** it is only the paired-device class (Story 8.39 rules), and a device revoked at any time is refused `TOKEN_REVOKED`

### Story 14.39: Off-site standby proven across NAT, relay and failover

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
**Then** exactly one Hub accepts intents, the old one's later rows are orphaned (Story 9.21), and the builder lists them

**Given** the results
**When** the suite finishes
**Then** a run report (NAT pair, path, lag percentiles, bootstrap time, failover time) is stored as a release artefact, and any target missed fails the release

### Story 14.40: A release registry with signed manifests and staged rollout by percentage

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

### Story 14.41: The Windows Hub checks daily and installs on restart, never during an open session

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

### Story 14.42: The standby updates before the primary

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

### Story 14.43: The Hub caches signed kiosk updates and kiosks install them between sessions

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

### Story 14.44: Pi agents update from the Hub's signed cache between console sessions

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

### Story 14.45: Upgrade safety is proven on every release

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
**Then** the release cannot be registered as `rolling` (Story 14.40)

### Story 14.46: Spike — the Hub core compiles to WebAssembly with an in-memory SQLite

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

### Story 14.47: The Hub UI runs in a browser tab on the wasm core

As a prospect evaluating Arena 360,
I want to try the real Hub screens without installing anything,
So that I can judge the product before I sign up.

**Covers:** FR197 (same web UI, in-memory DB), AD-15, AD-41, NFR26

**Acceptance Criteria:**

**Given** the Hub UI SvelteKit app
**When** built in demo mode
**Then** it loads the Story 14.46 wasm core in a Web Worker and replaces its LAN transport with a worker transport carrying the same intent and event frames; no screen code is forked, and a CI check fails if a component imports a demo-only module

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

### Story 14.48: The demo opens on a sample café with players coming and going

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

### Story 14.49: The marketing Demo page with a guided tour

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

### Story 14.50: Compare pages against ggLeap, Smartlaunch and iCafe

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
**Then** it describes what can be imported today (Story 5.33 CSV imports of products, players and plans) and links Get started; it promises nothing that is not shipped

**Given** the compare pages
**When** scanned with axe in both themes and at 320 px width
**Then** tables reflow to stacked rows on phone with headers kept, and there are zero serious or critical violations

### Story 14.51: A public status page for the cloud

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

**Given** an incident
**When** an Arena 360 operator posts it from the admin app (title, affected components, updates, resolved)
**Then** it appears on `/status` within 5 minutes with its updates newest first, and resolved incidents stay listed for 90 days

**Given** `status.json` older than 15 minutes
**When** the page renders
**Then** it shows "Status checks are delayed" rather than claiming everything is fine

### Story 14.52: A public changelog built from release notes

As a café owner,
I want to read what changed in each Hub, Kiosk, Pi and app release,
So that I know what's new and what my staff will notice.

**Covers:** FR198 (changelog), AD-32, NFR16, UX-DR91, UX-DR113

**Acceptance Criteria:**

**Given** the release registry (Story 14.40)
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
**When** an update is ready (Story 14.41)
**Then** its "What's new" link opens the matching changelog entry
