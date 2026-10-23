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

FR17: [MVP] Device groups have a name, a colour from six fixed swatches and a sort order; every device belongs to exactly one group or is Unassigned (a pre-assignment state; an Unassigned device cannot start sessions); console groups are never shared with PC groups. (revised 23 Oct 2026, epics) (CAP-4, CAP-14; PRD §Devices "Groups"; §Decisions "Console pricing")

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

FR114: [MVP] Receipt settings (printer, paper width 58/80 mm, text-only header lines with no logo, footer, WhatsApp template) and notification templates with no provider keys at the branch; WhatsApp/notification templates are Arena 360-approved and the branch only toggles them. (revised 23 Oct 2026, epics) (CAP-26; PRD §Configuration "Receipts", "Notifications")

FR115: [MVP] Opening hours per day (used by passes, pricing and offline alerts); order fulfilment (to seat / pick up); kiosk default volume; high-contrast theme per device; ads consent (Free). (CAP-26; EXPERIENCE IA Hub "Settings")

#### Kiosk

FR116: [MVP] One kiosk installer sets the Kiosk as the Windows shell for the player account (Explorer, Task Manager, Run, Settings blocked; Ctrl+Alt+Del and Win keys limited) so a player cannot reach the desktop in 30 min of trying. (CAP-27; PRD §Kiosk "Shell replacement")

FR117: [MVP] The kiosk enforces a launch allow-list; block-listed processes are killed on sight with "That app isn't allowed on this PC." and logged to the Hub; USB autorun is off. (CAP-27; PRD §Kiosk "Process control")

FR118: [MVP] Admin exit via hidden combo Ctrl+Alt+Shift+A + manager PIN (works with Hub down via cached verifier) or Hub remote unlock from the tile sheet drops to desktop for maintenance; both are logged. (CAP-27; AD-31)

FR119: [MVP] The game booster closes listed background apps, sets process priority and optionally switches power plan on launch; measured launch time is shown to staff. (CAP-27; PRD §Kiosk "Game booster")

FR120: [MVP] A player logs in at the kiosk with username + password in < 3 s on LAN, or staff start the session from the Hub; the error never says which field failed; 5 failures lock login for 60 s with a visible countdown and alert the Hub (already 5 failures; confirmed). (revised 23 Oct 2026, epics) (CAP-28; PRD §Kiosk "Login")

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

FR145: [v1.1] At 5 and 1 min the TV switches to the Pi input for a 5-s warning then back; at time-up the TV locks at once ("Time's up — see the counter."), no grace, tile alert until acknowledged, CEC standby per branch; the smart-plug option is deferred (vendor TBD; CEC only for now). (revised 23 Oct 2026, epics) (CAP-35; PRD §Pi "Time and warnings", "Time-up")

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

FR159: [v1.1] Scheduled WhatsApp daily or weekly reports arrive by 8 am local; there is no email channel. (revised 23 Oct 2026, epics) (CAP-40; PRD §Owner "Exports")

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

FR178: [MVP] Subscription per branch shows tiers Free ₹0, Basic ₹200, Premium ₹1,999 per branch per month GST-exclusive, yearly at 10× monthly, and add-ons (+20 devices ₹50, Tunnel ₹200, additional owner login ₹50 [v1.1]), with billing-cycle switch, invoices, payment method and cancel. (revised 23 Oct 2026, epics: the additional-owner-login add-on is sold from v1.1, Epic 14) (CAP-46; PRD §Owner "Subscription"; §Tiers)

FR179: [MVP] One Razorpay subscription per branch with mandatory autopay (card or eNACH for renewals over ₹15,000 incl. GST); mid-cycle changes are prorated; invoices itemise GST. (CAP-46; AD-20)

FR180: [MVP] Full refund within 7 days of first payment; no partial refunds; cancel keeps the plan to period end, then the branch drops to Free. (CAP-46; PRD §Tiers "Refunds")

FR181: [MVP] The PPP price band (MY, SG, US) is set from the billing country and stored on the subscription so later price-table changes do not reprice. (CAP-46; PRD §Tiers "Regional pricing")

FR182: [MVP] An upgrade applies within one heartbeat; a lapse never ends live sessions or open orders and over-cap devices go read-only (newest pairing first). (CAP-46; AD-29)

FR183: [MVP] A Free Android Hub upgrading to paid migrates its database to a Windows Hub by snapshot export/import with no data loss. (CAP-46; LLD §Android Hub "Upgrade path")

FR184: [MVP] The Free Android Hub shows an always-visible AdMob banner that never covers the floor grid, cart or settle bar. (CAP-47; PRD §Ad placement rules)

FR185: [MVP] The interstitial is the standard AdMob interstitial, shown only when staff return after the idle lock, at most once per 15 min, with close timing controlled by Google (no custom "skippable after 5 s" countdown), and never after a settle or over an open cart or tile sheet. (revised 23 Oct 2026, epics) (CAP-47; PRD §Ad placement rules)

FR186: [MVP] Ads exclude adult, gambling and alcohol categories; the owner sets a per-branch ad content rating G/PG/T (default PG, MA never) instead of blocking categories; a consent view exists; upgrade removes ads within one heartbeat; a lapsed branch resumes ads only after entitlement expiry; the Windows build has no ad SDK. (revised 23 Oct 2026, epics) (CAP-47; PRD §Ad placement rules)

FR187: [MVP] Credit reminders, low balance, session ending, console 5-min alert and receipts are sent by the cloud from Arena 360's Gupshup account (SMS fallback) only with stored sign-up consent, exactly once, never for an undone action, queued while the branch is offline, with versioned templates; no player sends on Free. (CAP-48; AD-27)

FR188: [MVP] A reseller/installer applies (name, phone OTP, email, city, business, type, cafés served) and sees only "Under review" with a 3-step tracker until approved. (CAP-49; PRD §Partners)

FR189: [MVP] Approval is sent by WhatsApp with the partner-portal link (partners.arena360.cloud, separate login); KYC (PAN, bank, optional GSTIN) is required before the first payout. (revised 23 Oct 2026, epics) (CAP-49)

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

FR202: [MVP] Every surface is usable by keyboard, at high contrast and without relying on colour or sound alone: ≥ 44 px targets (incl. Android phone Hub), HC theme per device on Kiosk/Hub/Kitchen/Pi/Owner, counter flow without a mouse, text/glyph on every state, persistent badge for every alert sound, web screen-reader baseline, light/dark/system on Owner and web, no hard-coded strings; the per-device HC theme applies to the Owner app and web surfaces too (overriding light/dark). (revised 23 Oct 2026, epics) (CAP-54; PRD §Cross-cutting "Accessibility")

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

NFR18: Hardware support — ESC/POS printers (USB, LAN 9100, Bluetooth), cash drawer via printer kick, USB barcode scanners (keyboard wedge), Pi Zero 2 W and Pi 4, HDMI-CEC TVs; Tuya/Tapo local-API smart plugs are deferred (vendor TBD; CEC only for now); compatibility list and "tested device" badge in the help centre; CEC best-effort; the smart-plug power-off guarantee is deferred with the plugs. (revised 23 Oct 2026, epics) (PRD §Cross-cutting "Hardware"; §Console)

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
- Environments: dev (local cloud binary, SQLite for Turso, MinIO for R2, seeded tenant); staging (separate namespace/node, Turso org, R2 bucket, `api.staging.arena360.cloud`, internal 10-PC test café + two beta cafés); production `api.arena360.cloud` behind Cloudflare; Owner app at `app.arena360.cloud`; partner portal `partners.arena360.cloud`. (Spine §Operational Envelope "Environments")
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
FR3: Epic 8, Epic 9 - Fresh step-up MFA is required only for sensitive owner commands (staff users/roles, revoke device/Hub,…
FR4: Epic 2 - The onboarding wizard runs 10 ordered steps (tenant → tier → install Hub → devices → plans → products →…
FR5: Epic 2 - Wizard step 1 (Tenant) captures café name, country, currency, timezone, GST registered yes/no and GSTIN,…
FR6: Epic 2 - Wizard step 2 (Tier) picks Free, Basic or Premium per branch
FR7: Epic 5 - Plans and products can be seeded from a template (Indian café, console lounge, VR arcade) or by CSV import…
FR8: Epic 2 - Wizard step 4 (Devices) shows kiosks arriving live, lets the owner create groups and drag devices in and…
FR9: Epic 2 - Wizard step 7 (Staff) invites staff by phone with a role and shows a one-time setup code
FR10: Epic 2 - Wizard step 8 (Printer & payments) detects an ESC/POS printer and runs a test print (or is skipped)
FR11: Epic 2, Epic 8 - Wizard step 9 starts a 2-minute test session on one device from the wizard and completes when the kiosk…
FR12: Epic 5 - Running the wizard for a second branch reuses the tenant's groups, plans and products as templates by default
FR13: Epic 2 - Install-Hub step shows an 8-character, single-use pairing code (grouped 4+4, Copy, 15-min countdown, no…
FR14: Epic 2 - Pairing exchanges the code once for a Hub device token and signed licence
FR15: Epic 2 - A Kiosk on first run discovers the Hub on the LAN (mDNS) with a manual Hub-IP fallback, pairs by the…
FR16: Epic 2 - Staff register consoles (with group and TV number), VR rigs and simulators manually
FR17: Epic 2 - Device groups have a name, a colour from six fixed swatches and a sort order; a device is in exactly one group or Unassigned
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
FR28: Epic 3, Epic 5 - The postpaid bill = minutes × rate with the branch rounding unit (default 15 min) and minimum (default 30…
FR29: Epic 6 - A player can end a postpaid session via "I'm done" on the HUD
FR30: Epic 3, Epic 5 - Guest sessions appear in reports as "Guest"
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
FR85: Epic 5, Epic 8 - A variance over the branch threshold (default ₹100) requires manager PIN + a reason (Counting error ·…
FR86: Epic 5 - A one-tap Z-report prints at shift close (sales by type, payments by method, expected vs counted,…
FR87: Epic 5 - A product has name, category, price, tax rate, HSN, track-stock flag, image, barcode and kitchen flag
FR88: Epic 5 - Stock-in records quantity, cost, free-text supplier and expiry date and updates the weighted average cost
FR89: Epic 5 - Waste records quantity, reason (spoiled, broken, free) and staff
FR90: Epic 5 - Expiry alerts fire N days before
FR91: Epic 5, Epic 8 - A per-product low-stock threshold alerts the Hub and owner
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
FR114: Epic 5 - Receipt settings (printer, paper width 58/80 mm, text-only header with no logo, footer, WhatsApp template)…
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
FR137: Epic 6, Epic 14 - Hours per game per branch are recorded and feed owner dashboards and the kiosk top-10 "Most played here"…
FR138: Epic 13 - A branch lends café-owned launcher accounts from a vault
FR139: Epic 13 - Players request games from the kiosk
FR140: Epic 3 - Staff run consoles as timed devices from the Hub with the same postpaid, pack and pass rules
FR141: Epic 3, Epic 8 - Console time-up raises a tile "TIME UP" alert + sound until acknowledged
FR142: Epic 12 - A Pi Zero 2 W or Pi 4 per TV (BOM < ₹4,000) pairs to the Hub by 6-digit code, shows its TV number, and is…
FR143: Epic 12 - The idle TV shows branch logo, TV number, muted game video, console rates and a "Scan to start" QR…
FR144: Epic 12 - Scanning the QR on café Wi-Fi opens a Hub LAN page with no login
FR145: Epic 12 - At 5 and 1 min the TV switches to the Pi input for a 5-s warning then back (smart plug deferred)
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
FR159: Epic 14 - Scheduled WhatsApp daily or weekly reports arrive by 8 am local
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
FR185: Epic 4 - The standard AdMob interstitial shows only on idle return, at most once per 15 min, close timing set by Google…
FR186: Epic 4 - Ads exclude adult, gambling and alcohol categories; owner sets per-branch content rating G/PG/T (default PG)
FR187: Epic 8 - Credit reminders, low balance, session ending, console 5-min alert and receipts are sent by the cloud from…
FR188: Epic 11 - A reseller/installer applies (name, phone OTP, email, city, business, type, cafés served) and sees only…
FR189: Epic 11 - Approval is sent by WhatsApp with the partner-portal link (partners.arena360.cloud, separate login)
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
FR202: Epic 1, Epic 8, Epic 11 - Every surface is usable by keyboard, at high contrast and without relying on colour or sound alone

NFR12: Epic 1, Epic 6, Epic 11 - Kiosk hardening and launcher logout (Epics 1 and 6); external kiosk-lockdown pen test as the GA gate in Epic 11 (Story 11.33 GA gate)

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

**FRs covered:** FR7, FR12, FR64–FR69, FR72, FR73, FR75–FR92, FR108, FR109, FR112, FR114; also parts of FR28, FR30
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

**FRs covered:** FR3, FR21, FR52, FR148–FR158, FR167, FR187; also parts of FR11, FR85, FR91, FR141, FR202
**Phase:** MVP
**Depends on:** Epics 1–7
**Notes:** R2 presigned batches, summary and rollups, command mailbox, audit builder, Tunnel + Access, step-up MFA, Gupshup adapter and outbox, owner PWA layouts; NFR7, 14, 20, 21, 23.

### Epic 9: Resilience, backups and data rights

A branch survives Hub failure through standby and promote, and keeps exports, cloud backups, the GST archive and an erasure that holds up.

**FRs covered:** FR169, FR170, FR171, FR173, FR174, FR175, FR199, FR200; also parts of FR3
**Phase:** MVP
**Depends on:** Epics 1–8
**Notes:** Epoch fencing via `branch_fence` CAS, crypto-shred keys; NFR8, 9, 13, 24.

### Epic 10: Subscriptions and tiers

Owners pay for, upgrade, cancel and lapse a subscription per branch without disruption, and a Free Android Hub can migrate to Windows when it goes paid.

**FRs covered:** FR178–FR183
**Phase:** MVP
**Depends on:** Epics 1–5, 8–9
**Notes:** Razorpay state machine, card/eNACH above ₹15,000, PPP band, 7-day refund; completes the paid-tier branch of the FR6 wizard step; NFR22 trusted time.

### Epic 11: Marketing site, partner programme and admin operations

Prospects find and sign up through the public site, partners earn and track commission, and Arena 360 staff curate the catalogue, approve partners and support branches.

**FRs covered:** FR188–FR196; also parts of FR202
**Phase:** MVP
**Depends on:** Epics 1–2, 4, 6, 8, 10
**Notes:** Separate partner identity, admin behind Cloudflare Access, diagnostics bundle with no PII, flash-screened media; NFR27 instrumentation.

### Epic 12: Console TVs with Pi and QR self-start

Players start and see console time on the TV through a Pi agent and a QR code.

**FRs covered:** FR142, FR143, FR144, FR145, FR147
**Phase:** v1.1
**Depends on:** Epics 1–3, 6–9, 11
**Notes:** Pi/QR UX-DRs, CEC adapter, read-only image.

### Epic 13: Bookings, tournaments, loyalty and game accounts

Cafés grow revenue with reservations, tournaments, loyalty and referrals, memberships, a launcher-account vault, game requests and migration from competitors.

**FRs covered:** FR22, FR42, FR43, FR55, FR56, FR138, FR139
**Phase:** v1.1
**Depends on:** Epics 1–3, 5–6, 8–9
**Notes:** Builds on the session, POS, kiosk and owner-remote epics; no additional cross-cutting themes.

### Epic 14: Deeper back office, owner depth and platform ops

Managers run stock counts, purchase orders, petty cash, attendance and prep stations; owners get scheduled reports, cohorts, team logins and approvals; the platform gains off-site standby, auto-updates, a demo and compare pages.

**FRs covered:** FR93–FR96, FR102, FR159–FR165, FR172, FR176, FR197, FR198; also parts of FR137
**Phase:** v1.1
**Depends on:** Epics 1–13
**Notes:** NFR19 upgrade safety; rollup schema v3 (Stories 14.15–14.17) is built on top of Epic 13's rollup schema v2 (Story 13.29), so Epic 13 ships first.

