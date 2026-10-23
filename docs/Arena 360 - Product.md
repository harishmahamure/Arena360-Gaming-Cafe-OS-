# Arena 360 — Product Requirements

Oct 22, 2026 · @Harish

## Summary and goals

Arena 360 is a local-first management system for gaming cafes and game zones: a Hub app that runs the branch from a counter PC, a Kiosk app on every gaming PC, a Pi controller for consoles, an Owner app for multi-branch oversight, and a marketing site with a self-serve onboarding flow. The branch keeps working with the internet down; the cloud keeps the Hub's uploaded batches, snapshots and published reports (no continuous mirror) and relays owner commands (revised 23 Oct 2026, architecture). Architecture is specified separately in "Gamezone SaaS — Low-Level Architecture".

**Goals**

1. A cafe owner installs the Hub, pairs 20 PCs and runs the first paid session within 60 minutes, without support.
2. Counter staff complete the three daily jobs (start a session, sell a snack, close the shift) in under 10 seconds each, offline.
3. Owners see cash, sessions and stock across every branch from their phone, and change prices from there.
4. Free tier is usable for a single-branch cafe forever, funded by ads; paid tiers are priced under every incumbent.
5. Marginal infra cost stays under ₹10 per paid branch per month on average at any scale (about ₹6 Basic, ₹12 Premium) (revised 23 Oct 2026, architecture).

**Decisions taken on the open questions**

| Question | Decision | Reason |
| --- | --- | --- |
| Default session mode | Postpaid walk-in (open session, bill at end); prepaid packs, wallet and credit on top | Matches how Indian cafes operate today |
| GST invoicing | In MVP: rates per item, HSN, sequential invoice numbers, GSTIN on receipt | Retrofitting invoice numbering is painful and it blocks sales to registered cafes |
| Bookings and reservations | v1.1 | Not needed to run a cafe day one; needed to beat incumbents |
| Console control | MVP: manual start/stop from Hub with timer. v1.1: Pi with HDMI-CEC and a QR start screen | Ship consoles on day one without hardware dependency |
| Launcher accounts (Steam, Epic, Riot) | MVP: player-owned accounts with forced launcher logout at session end. v1.1: cafe-owned account vault | Vaulting credentials safely is its own project |
| Migration from competitors | MVP: CSV import for players, plans, products. v1.1: direct importers for ggLeap and Smartlaunch exports | CSV covers everyone; importers are a sales tool |
| Kitchen module | Included in Basic: KOT printing and a kitchen display page. Not in Free | Low build cost, high perceived value |

**Decisions from review, 22 Oct 2026**

| Topic | Decision | Consequence |
| --- | --- | --- |
| Scope | Everything in this PRD ships at launch; the phase labels below are delivery waves inside one release, not separate releases | Launch date moves out; see sequencing note under Scope |
| Free-tier Hub | Free plan runs the Hub on Android (tablet or phone) only, built on Tauri 2 mobile from the shared web UI; the Windows Hub build is for paid tiers | AdMob serves a banner (always) and an interstitial (only when staff return after the idle lock, skippable after 5 s) on Android only; Windows build carries no ad SDK (revised 23 Oct 2026, UX) (revised 23 Oct 2026, architecture) |
| Ads | AdMob banner and interstitial on the Free Android Hub only; no Kiosk attract-screen or Pi idle-screen ads in Phase 1 | No direct ad sales team needed at launch; no ad ever on a player screen (revised 23 Oct 2026, UX) |
| WhatsApp | Gupshup as the WhatsApp Business provider | SMS fallback also via Gupshup; messages are sent by the cloud from Arena 360's Gupshup account, so branches hold no provider keys (revised 23 Oct 2026, architecture) |
| OTP | Mandatory on Owner app sign-up and new-device login; player sign-up at kiosk needs no OTP | SMS cost only on owner side |
| Kiosk OS | Windows 10 and later (Windows 7 dropped) | Kiosk and Windows Hub built on Tauri (WebView2); one shared web UI and design-token codebase. Revised 22 Oct 2026 in UX planning, superseding .NET Framework 4.8 (WPF) on Windows 7 SP1 |
| Console pricing | Consoles always have their own groups and packs; never shared with PC packs | Simplifies pack rules; a "combo" pack is two packs sold together |
| Partner program | Reseller and installer program at launch | Pricing page carries a partner tab; control plane tracks referral codes |
| Markets | India plus Razorpay-supported countries at launch: Malaysia, Singapore and the US, with purchasing-power-parity pricing (revised 23 Oct 2026, architecture) | See Regional pricing under Tiers |

## Personas

Five people touch the product daily; the Hub serves three of them from one screen.

| Persona | Who | Uses | Cares about | Pain today |
| --- | --- | --- | --- | --- |
| Owner | Runs 1–5 branches, often not on site | Owner app, onboarding, Hub occasionally | Cash leakage, stock theft, knowing the branch is open and busy | Calls staff for numbers; WhatsApp photos of notebooks |
| Branch manager | On site most days, closes the day | Hub (all modules), Owner app read-only | Shift close, staff mistakes, credit collection | Manual cash reconciliation, disputes with players about time |
| Counter staff | Shift worker, often young, high turnover | Hub POS and sessions, kiosk pairing | Speed, not getting blamed for variance | Software that needs training; crashes mid-shift |
| Kitchen staff | Makes snacks and drinks | Kitchen display page, KOT printer | Clear tickets, order sequence | Verbal orders, forgotten items |
| Player | 14–30, visits weekly, pays cash or UPI | Kiosk, QR for consoles | Starting fast, seeing time left, not losing a game at time-up | Waiting for staff, surprise end of session |

The Hub is designed for counter staff first: large touch targets, every common action one tap from the floor view, and no modal that blocks a sale. Manager-only actions sit behind a PIN, not a separate screen.

## Scope by phase

Everything in this document is launch scope. The MVP, v1.1 and v2 columns are delivery waves within the one launch: wave 1 (MVP column) is what a single-branch cafe needs to replace a notebook, wave 2 (v1.1 column) is what wins against incumbents, wave 3 (v2 column) is the multi-branch and marketplace layer. Build in that order; do not launch until all three are done.

| Area | MVP | v1.1 | v2 |
| --- | --- | --- | --- |
| Sessions | Postpaid walk-in, prepaid packs, pause, extend, transfer, group bill | Bookings and reservations, tournaments | Online booking from a public page |
| Players | Profiles, wallet, credit with limits, CSV import | Loyalty points, referrals, WhatsApp receipts | Player mobile app |
| Devices | PCs and consoles, groups, group-based pricing, manual console timer | Pi controller with CEC and QR start, device health | Peripheral tracking |
| Pricing | Per group, time-window and date-based dynamic pricing, coupons | Happy-hour automation across branches | Demand-based suggestions |
| POS and inventory | Products, bundles with components, stock-in, waste, expiry, low-stock alerts | Supplier records, purchase orders, stock count with variance | Central purchasing across branches |
| Kitchen | KOT printing, kitchen display page, order status | Prep stations, item-level timing | — |
| Money | Split payment, UPI and card recorded by reference (revised 23 Oct 2026, UX), cash drawer, GST invoices, shifts with variance, expenses, deposits | Refund approvals, petty cash ledger | Accounting export (Tally, Zoho) |
| Users | Branch users, roles, fine-grained permissions, PIN for manager actions | Staff attendance | Payroll hints |
| Kiosk | Login, signup, overlay HUD, orders, game library, lockdown, launcher logout | Cafe-owned launcher vault, game requests | Streaming/cloud gaming hooks |
| Owner app | Dashboards, exports, alerts, commands, MFA | Scheduled reports, branch comparison | Franchise roll-ups |
| Marketing | Site, pricing, demo, FAQs, get started | Comparison pages, partner program | Marketplace listing |
| Onboarding | Wizard, pairing, templates, CSV import, go-live checklist | ggLeap and Smartlaunch importers | Assisted migration service |
| Ops | Backup export, standby promote, audit log | Auto-updates with staged rollout | — |

Sequencing note: wave 1 alone is roughly 60% of the build; waves 2 and 3 add bookings, the Pi controller, the launcher vault, importers, scheduled reports, player analytics and the partner program. A beta with wave 1 to 20 friendly cafes while waves 2 and 3 are built is the recommended way to keep the launch date honest without splitting the release.

## Hub app — players, sessions, plans, devices, pricing

The Hub runs the floor. Everything below works with the internet down and is one or two taps from the floor view.

**Players**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Profile | Username and password (username unique per branch), phone (unique per branch), name, optional email, DOB, notes, tags | Create in under 15 s at counter; phone lookup finds a player in 1 tap | MVP |
| Guest | Start a session without a profile; convert to profile later | Guest sessions appear in reports as "Guest"; conversion keeps history | MVP |
| Wallet | Prepaid balance in paise; top-up by cash, UPI, card; full ledger | Balance never negative; every change has a ledger row with actor | MVP |
| Credit | Postpaid limit per player, overdue days, reminder cycle (3, 7, 14 days) via WhatsApp/SMS | Session start blocked when limit exceeded; manager PIN can override once per player per day (revised 23 Oct 2026, UX) | MVP |
| Packs owned | Time packs with hours remaining and expiry | Expired packs greyed; partial use deducts minutes | MVP |
| Import | CSV of players with phone, name, balance, credit limit | Dry-run shows conflicts before commit | MVP |
| Loyalty, referrals | Points per rupee, redeem as wallet credit; referral bonus on first paid session | Configurable rates per branch | v1.1 |

**Sessions**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Postpaid walk-in | Open session on a device; billed at end at the device group's rate | Rate shown on start; bill computed to the minute, rounded by config (default 15 min, min 30 min) | MVP |
| Prepaid | Start on a pack or wallet; time counts down | Kiosk shows remaining time; auto-lock at zero with configurable grace (default 2 min); Extend stays available during grace (revised 23 Oct 2026, UX) | MVP |
| Pause / resume | Staff pause with reason (power cut, hardware) | Paused time is not billed; resume restores timer | MVP |
| Extend | From Hub or kiosk, charged to wallet, pack or credit | Extension writes a ledger row; cannot extend past branch closing time | MVP |
| Transfer | Move a session to another device in the same group, or across groups with rate change from that moment | Old device locks, new device unlocks within 5 s | MVP |
| Group session | Several players on several devices under one bill, one payer | Bill splits by device minutes; payer can be changed before settlement | MVP |
| Forced end | Manager PIN, reason required | Shown in audit; player sees a message on kiosk | MVP |
| Bookings | Reserve a device or group for a time slot with deposit; no-show rules | Booked devices show as reserved on floor view 15 min before | v1.1 |
| Tournaments | Bracket, entry fee, block devices for the event | Entry fees post to a tournament income account | v1.1 |

**Plans**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Hourly rate | Per device group; the default postpaid rate | Changing a rate affects new sessions only | MVP |
| Time pack | Hours, price, validity days, allowed groups (e.g. 5 h, ₹300, 7 days, Basic group only) | Pack bought on a Basic device cannot start a Premium device unless pack allows | MVP |
| Block | Fixed slot (2 h for ₹150) with a start window | Block ends at fixed time regardless of start | MVP |
| Pass | Unlimited within a window (night pass 10 pm–6 am) | One active pass per player | MVP |
| Membership | Monthly recurring pass with auto-reminder | Renewal reminder via WhatsApp | v1.1 |
| Bulk import | CSV of plans | Validated against existing groups | MVP |

**Devices and device groups**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Device registry | PCs (auto-discovered on LAN from kiosk `hello`), consoles, VR, simulators (manual) | New kiosk appears as "unassigned" until mapped | MVP |
| Groups | Named groups (Basic, Pro, Premium, PS5, VR) with colour and sort order | A device belongs to exactly one group | MVP |
| Group-based pricing | Every rate, pack and dynamic rule is scoped to groups | A session on a device only ever uses its group's prices | MVP |
| Status | Online, offline, in session, reserved, maintenance | Maintenance removes a device from the pool without deleting it | MVP |
| Remote actions | Lock, unlock, message, restart, shutdown (PCs); power on/off (consoles via Pi) | Action result shown within 5 s or marked failed | MVP (PC), v1.1 (console) |
| Health | Last seen, kiosk version, GPU/RAM summary | Owner alert when a device is offline during open hours | v1.1 |

**Dynamic pricing**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Time-window rules | Day-of-week plus hour range, per group, rate override or percentage | Rule shown on floor view while active; the kiosk shows the current rate | MVP |
| Date rules | Specific dates or ranges (holidays, events) with priority over weekly rules | Higher priority wins; conflicts flagged on save | MVP |
| Coupons | Code, discount type, validity, usage limit, allowed groups and plans | Applied at settlement; shown on receipt | MVP |
| Manual discount | Percentage or amount with manager PIN and reason | In audit and shift report | MVP |
| Tenant-wide push | Owner app pushes rules to all branches as commands; branch override allowed | Branch override flagged in owner app | v1.1 |

## Hub app — POS, inventory, kitchen, money

The POS is a single screen: product grid on the left, cart on the right, settle at the bottom. Inventory is deliberately simple: one stock level per product per branch, no bins or warehouses.

**POS**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Product grid | Categories, images, favourites, barcode scan | Sale of a scanned item in 1 scan + 1 tap | MVP |
| Cart to session | Attach an order to a running session so it settles with the bill | Order appears on the kiosk overlay as "ordered" | MVP |
| Split payment | Any mix of cash, UPI, card, wallet, credit; remainder tracked | Settlement blocked until remainder is zero or moved to credit | MVP |
| UPI and card | Recorded manually by reference: UPI needs the last 4 digits of the reference (required); card needs the terminal reference. No dynamic UPI QR or payment webhook this phase | Settling never depends on the internet; references print on the receipt and appear in the Z-report (revised 23 Oct 2026, UX) | MVP |
| Receipts | Thermal print, WhatsApp, or none; GST fields when enabled | Sequential invoice number per branch, never reused | MVP |
| Refund / void | Reason, manager PIN, original invoice reference; stock returned if flagged | Refund appears as negative line in shift report | MVP |

**Inventory**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Products | Name, category, price, GST rate, HSN, track-stock flag, image, barcode | Bulk CSV import with dry-run | MVP |
| Bundles | A product made of components (combo: burger + cola); selling decrements components | Bundle price independent of components; component shortage blocks sale with override | MVP |
| Stock-in | Quantity, cost, supplier name (free text), expiry date | Cost updates weighted average | MVP |
| Waste | Quantity, reason (spoiled, broken, free), staff | Waste report by reason and value | MVP |
| Expiry | Alert N days before; expired stock listed daily | Expired items excluded from sale until written off | MVP |
| Low stock | Threshold per product; alert on Hub and owner app | Alert clears when stock-in recorded | MVP |
| Stock count | Count sheet, variance per item, adjustment with reason | Variance report by value | v1.1 |
| Suppliers and POs | Supplier records, purchase orders, receive against PO | PO receipt creates stock-in rows | v1.1 |

**Kitchen** (Basic and above)

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| KOT | Items flagged "kitchen" print a ticket on order; ticket shows device number | Ticket within 3 s of order | MVP |
| Kitchen display | Browser page: new, preparing, ready, served; tap to advance | Works on any tablet on LAN | MVP |
| Order status to player | Kiosk overlay shows status changes | Status within 5 s | MVP |
| Prep stations | Route items to bar vs kitchen | Separate ticket per station | v1.1 |

**Money, shifts, cash**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Shift | Open with float, close with counted cash; variance = counted − expected (+ over, − short); expected cash hidden until counted cash is entered | Variance over threshold requires manager PIN and reason (revised 23 Oct 2026, UX) | MVP |
| Cash drawer | Kick on cash sale; drawer open count in shift report | Drawer event logged | MVP |
| Cash deposit | Move cash from drawer to safe or bank with slip reference | Expected drawer cash reduces accordingly | MVP |
| Expenses | Category, amount, paid from drawer or owner, receipt photo | Petty cash reduces drawer expectation | MVP |
| Plan payments | Pack and pass purchases are sales with their own GST rate | Appear separately from product sales in reports | MVP |
| Credit collection | Record payment against outstanding credit; partial allowed | Outstanding aged 0–7, 8–14, 15+ days | MVP |
| Z-report | Printable end-of-day: sales by type, payments by method, variance, waste, expenses | One tap at shift close | MVP |
| Refund approvals | Refund above threshold needs owner approval via command | Pending refunds visible in owner app | v1.1 |

**Floor view**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Layout | Drag devices onto a grid; groups by colour; zones (PC hall, console room) | Layout persists per branch | MVP |
| Live state | Free, in session with time left, reserved, offline, maintenance, order pending | State updates within 2 s | MVP |
| One-tap actions | Tap device: start, extend, order, transfer, lock, end | No action more than 2 taps from floor view | MVP |
| Current pricing | Active dynamic rule shown in header | Rule change reflected within 1 min | MVP |

## Hub app — users, configuration, reports, ops

Access is role-based with a fixed permission catalogue; owners compose roles, they do not invent permissions. Manager-level actions prompt for a PIN inline rather than forcing a logout.

**Branch users and RBAC**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Users | Name, login, password, PIN, role, active flag; owner can create from Owner app via command, which issues a one-time setup code; the staff member sets their own PIN and password at the Hub on first login with that code; the owner never sets or sees a PIN or password (revised 23 Oct 2026, architecture) | User changes sync to branch within one poll; setup code is single-use and expires | MVP |
| Roles | Default roles: Owner, Manager, Counter, Kitchen, Kiosk-device; custom roles from the permission catalogue | Permissions grouped by module; each is allow/deny | MVP |
| Permission catalogue | \~40 permissions: `session.start`, `session.force_end`, `wallet.topup`, `discount.manual`, `refund.issue`, `shift.close`, `shift.override_variance`, `product.edit`, `stock.adjust`, `price.edit`, `user.manage`, `report.view`, `settings.edit`, `backup.export`, `device.remote` and so on | Every `apply` intent maps to exactly one permission | MVP |
| PIN elevation | A counter user can perform a manager action by entering a manager's PIN; audit records both | PIN entry never leaves the current screen | MVP |
| Session lock | Hub UI locks after N minutes idle; unlock with PIN | Default 5 min | MVP |
| Attendance | Clock-in/out tied to shift | Hours per staff in reports | v1.1 |

**Configuration**

| Setting | Options | Phase |
| --- | --- | --- |
| Currency and locale | Currency per branch (ISO 4217), symbol position, number format, timezone (revised 23 Oct 2026, architecture) | MVP |
| Tax | GST on/off, GSTIN, default rates, inclusive/exclusive pricing, invoice prefix and sequence; GST invoicing applies only to GST-registered Indian tenants, other countries use a simple tax rate (revised 23 Oct 2026, architecture) | MVP |
| Billing rules | Rounding unit, minimum billing, grace period, time-up mode per branch: "clean up" (end non-allow-listed processes, sign out launchers, clear temp files and clipboard, reset volume, return to attract) or "staff handles" (lock screen held over the paused game until staff act) (revised 23 Oct 2026, UX) | MVP |
| Receipts | Printer (ESC/POS over USB, LAN or Bluetooth), paper width, logo, footer text, WhatsApp template | MVP |
| Opening hours | Per day; used by passes, dynamic pricing and "branch offline" alerts | MVP |
| Notifications | Templates for credit reminder, low balance, session ending; messages are sent by the cloud via Arena 360's Gupshup account, so there are no WhatsApp or SMS provider keys in branch settings (revised 23 Oct 2026, architecture) | MVP |
| Ads (Free tier) | Consent view; Android Hub ad slots (banner, interstitial) cannot be disabled on Free (revised 23 Oct 2026, UX) | MVP |
| Language | English only (revised 22 Oct 2026; other languages deferred, strings stay externalised) | MVP (English) |

**Reports (on the Hub, offline)**

| Report | Content | Phase |
| --- | --- | --- |
| Shift / Z-report | Sales by type, payments by method, expected vs counted cash, variance, waste, expenses, refunds | MVP |
| Daily sales | By hour, by group, by product category | MVP |
| Sessions | Count, minutes, revenue by device and group; utilisation per hour | MVP |
| Players | Top spenders, new vs returning, credit outstanding aged | MVP |
| Stock | Current stock with value, movements, waste by reason, expiring soon | MVP |
| Staff | Sales and sessions per staff, discounts given, voids | MVP |
| Export | CSV and PDF for every report | MVP |

**Ops**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Backup export | One-tap export of the branch database to file or USB; restore from file with confirmation | Export under 10 s for a 1 GB DB | MVP |
| Cloud backup status | Last upload time, last summary published, pending batches (revised 23 Oct 2026, architecture) | Visible in Hub header | MVP (paid) |
| Standby | Pair a second PC as standby; promote with a manager PIN inline; if promotion fails, a manager can force promote with PIN after a warning that data since the last sync may be lost (audited, owner alerted); a returning old main Hub locks itself ("This PC is no longer the main Hub", offer to become standby) and never serves kiosks | Promotion completes under 10 s; kiosks reconnect automatically (revised 23 Oct 2026, UX) | MVP (paid) |
| Audit log | Every change with actor, time, before/after | Filter by actor, module, date | MVP |
| Offline indicator | Internet and cloud reachability shown; nothing else changes | No feature degrades on LAN when offline | MVP |
| Updates | Hub checks for updates daily; installs on restart with rollback | Staged rollout by percentage from control plane | v1.1 |

## Kiosk app

The Kiosk is a Windows app that owns the PC between sessions and stays out of the way during one. It holds no data; every action is a request to the Hub.

**Setup and lockdown**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Install | One installer; first run asks for the Hub pairing code (or auto-discovers the Hub and asks for manager PIN) | Paired and visible on Hub floor view within 60 s | MVP |
| Shell replacement | Runs as the Windows shell for the player account; Explorer, Task Manager, Run, Settings blocked; Ctrl+Alt+Del and Win keys limited | A player cannot reach the desktop in 30 minutes of trying (test plan) | MVP |
| Process control | Allow-list of launchable executables; block-list killed on sight; USB autorun off | Blocked launch shows a message, logs to Hub | MVP |
| Admin exit | Manager PIN or Hub remote unlock drops to desktop for maintenance | Logged | MVP |
| Auto-update | Pulls kiosk updates from Hub (Hub caches from cloud) | Updates apply between sessions only | v1.1 |
| Game booster | On launch: close background apps from a list, set process priority, optional power plan switch | Measured launch time shown to staff | MVP |

**Player screens**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Attract screen | Idle loop: branch branding, game video, current rates, active dynamic rule, promotions; no ad slot in Phase 1 | No ads on any tier (revised 23 Oct 2026, UX) | MVP |
| Login | Username + password, or staff starts from Hub (walk-in) | Login under 3 s on LAN (revised 23 Oct 2026, UX) | MVP |
| Sign-up | Username, password, phone, name; no OTP; one required consent checkbox, unticked by default: "I agree to the Terms and Privacy Policy and to receive service messages on WhatsApp/SMS." No marketing consent | Creates profile on Hub; account creation blocked until consent is ticked; duplicates blocked by username and by phone (revised 23 Oct 2026, UX) | MVP |
| Session start | Choose plan or pack when prepaid; postpaid shows rate and starts | Shows device group rate and any active dynamic rule | MVP |
| Overlay HUD | Small always-on-top widget: time left or elapsed, balance, hotkey to expand | Does not steal focus from games; hides in fullscreen exclusive mode with a toast on warnings | MVP |
| Warnings | At 10 and 5 minutes and 60 seconds; low balance | Toast plus sound; configurable | MVP |
| Extend | From overlay or from the time-up lock screen during grace, charged to wallet, pack or credit; blocked if credit limit hit | Extension reflected on Hub floor view within 2 s (revised 23 Oct 2026, UX) | MVP |
| Time-up | Lock screen with grace countdown over the still-visible game, with Extend available; then the branch's time-up mode applies: clean up or staff handles (see Configuration) | Player has a visible countdown; no data loss within grace (revised 23 Oct 2026, UX) | MVP |
| Orders | Menu with images, cart, order to seat; status updates from kitchen. Basic and above; on Free, orders go through the counter POS | Order reaches Hub POS and KOT within 3 s (revised 23 Oct 2026, UX) | MVP |
| Hub unreachable | Running sessions continue on the last known time; "Reconnecting…" shows; new login, orders and Extend are disabled with "Counter is reconnecting — try again shortly." | Player is never locked out; kiosk reconciles with the Hub on reconnect (revised 23 Oct 2026, UX) | MVP |
| Messages | Staff broadcast or per-seat message appears as toast | Delivered within 2 s | MVP |

**Games and launchers**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Library | Hub holds the catalogue: name, executable, launcher (Steam, Epic, Riot, standalone), category, age rating. Game media (art, video) is global only: curated and flash-screened by the Arena 360 team; no owner media upload or matching UI; devices cache global media, else launcher cover or exe icon | Edit once on Hub, every kiosk updates (revised 23 Oct 2026, UX) | MVP |
| Auto-scan | Kiosk scans Steam, Epic, Riot and common folders on first run; proposes entries to Hub | Manager approves in bulk | MVP |
| Launch | One click from the kiosk game grid; booster applied | Launch logged with player and time | MVP |
| Launcher logout | At session end, kiosk signs out Steam, Epic, Riot, Discord and browser profiles; clears saved credentials | Next player never sees previous accounts (test plan) | MVP |
| Player-owned accounts | Player logs into their own launcher during session | Logout enforced at end | MVP |
| Cafe-owned vault | Hub holds shared launcher accounts; kiosk injects login at launch; one account per concurrent seat | Account released at session end | v1.1 |
| Game request | Player requests a game; appears on Hub for manager | Request count per game in reports | v1.1 |
| Stats | Hours per game per branch | Feeds owner app dashboards | MVP |

## Console management

Consoles (PS5, Xbox, Switch) cannot run the Kiosk app, so a Raspberry Pi per TV stands in for it. MVP runs consoles as timed devices controlled from the Hub; v1.1 adds the Pi so the TV itself enforces time.

**MVP — manual timed devices**

| Feature | Requirement | Acceptance criteria |
| --- | --- | --- |
| Device type | Console registered manually with group (PS5, Xbox) and TV number | Appears on floor view like a PC |
| Session | Staff start from Hub; same postpaid, pack and pass rules | Timer and warnings on Hub floor view |
| Player alert | Optional WhatsApp or SMS at 5 minutes | Sent only if the player profile has the sign-up service-message consent (revised 23 Oct 2026, UX) |
| End | Staff end session; time-up shows alert on Hub, staff walk over | Audit records who ended it |

**v1.1 — Pi controller**

| Feature | Requirement | Acceptance criteria |
| --- | --- | --- |
| Hardware | Raspberry Pi Zero 2 W or Pi 4 with HDMI to TV; console on another HDMI input; optional smart plug for consoles without CEC | Reference BOM under ₹4,000 per TV |
| Pairing | Pi pairs to Hub with a 6-digit code like a kiosk; shows its TV number on screen | Visible on floor view within 60 s |
| Idle screen | Pi output shown on TV when no session: branch branding, TV number, game video, console rates, QR code to start; no ad slot | QR encodes branch and TV id (revised 23 Oct 2026, UX) |
| Start by QR | Player scans with phone on the cafe Wi-Fi, lands on a Hub page via LAN with no login, picks a console plan and requests; pays at the counter; staff confirm and start from the Hub, which tells Pi to switch TV input to console (CEC); the phone then shows a live timer only | Request appears on the Hub tile with sound; input switch within 3 s of start (revised 23 Oct 2026, UX) |
| Start by staff | Same from Hub | Same |
| Time and warnings | Pi overlays nothing on the console input; warnings go to the live timer on the player's phone and to the TV by switching to the Pi input for 5 s at 5 and 1 minutes | Switch-back is automatic (revised 23 Oct 2026, UX) |
| Time-up | No grace on consoles: Pi switches TV to its own input with a lock screen ("Time's up — see the counter.") at once; staff handle the overrun; CEC standby or smart plug per branch | Hub tile raises a time-up alert until acknowledged (revised 23 Oct 2026, UX) |
| Controller check | After time-up the Hub tile prompts staff to mark controllers Returned or Missing | Logged against session (revised 23 Oct 2026, UX) |
| Remote commands | Power on/off, input switch, message to TV from Hub | Result within 5 s |

Consoles without CEC support (older TVs) fall back to the smart-plug path; the PRD treats CEC as best effort and the plug as the guarantee.

## Owner app

The Owner app is one responsive web app, phone to desktop, installable as a PWA with standard web push alerts (on iOS only when installed to the Home Screen) (revised 23 Oct 2026, UX), reading the summary and daily rollups each Hub publishes to the cloud and writing commands; there is no continuous cloud copy of branch data (revised 23 Oct 2026, architecture). It never needs the branch to be open. On Free tier it shows subscription, pairing and support only; dashboards start at Basic.

**Dashboards and reports**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Today | Per branch: open/closed, sessions live, revenue so far, cash in drawer, staff on shift, last sync time | Loads in under 2 s from the Hub-published summary; shows data age, which stays within the tier cadence (Basic ≤ 15 min, Premium ≤ 5 min, or live via Tunnel) (revised 23 Oct 2026, architecture) | MVP |
| Revenue | Day, week, month, custom; by branch, by type (sessions, packs, products), by payment method | Matches Hub Z-report totals exactly | MVP |
| Cash reconciliation | Shift-by-shift expected vs counted, variance, who closed, deposits, expenses | Variance over threshold highlighted | MVP |
| Stock | Levels, low-stock, waste by reason and value, expiring | Per branch and total | MVP |
| Credit | Outstanding by player, aged; reminders sent | Tap to send reminder now (command) | MVP |
| Players | New, returning, top spenders, retention cohorts | Weekly cohort chart | v1.1 |
| Devices | Utilisation per group per hour; offline devices | Heatmap view | v1.1 |
| Staff | Sales, discounts, voids, shift variance per staff | Sort by variance | v1.1 |
| Audit | Searchable change log, built on demand in the cloud from the branch's uploaded batches; shows "Preparing audit…" while it builds (revised 23 Oct 2026, architecture) | Filter by branch, actor, module; first results under 10 s | MVP |
| Exports | CSV and PDF for every report; scheduled email or WhatsApp daily/weekly | Scheduled reports arrive by 8 am local | MVP (export), v1.1 (scheduled) |

**Alerts** (web push and WhatsApp, thresholds per tenant; (revised 23 Oct 2026, UX))

| Alert | Trigger | Phase |
| --- | --- | --- |
| Branch offline | No heartbeat or upload during opening hours for 30 min | MVP |
| Cash variance | Shift variance over threshold | MVP |
| Low stock | Any product under threshold | MVP |
| Credit overdue | Any player past reminder cycle | MVP |
| Large refund or discount | Over threshold | MVP |
| Device offline | PC offline during opening hours | v1.1 |
| Pending approvals | Refund awaiting owner | v1.1 |

**Commands** (every write from the Owner app is a command; shown as "waiting for branch" until applied)

| Command | Scope | Phase |
| --- | --- | --- |
| Prices, plans, dynamic rules | One branch or all | MVP |
| Products and bundles | One or all | MVP |
| Staff users and roles | One or all | MVP |
| Revoke device or Hub | One | MVP |
| Send credit reminder | One player | MVP |
| Backup now | One branch | MVP |
| Approve refund | One | v1.1 |
| Message to all kiosks | One branch | v1.1 |

**Account and billing**

| Feature | Requirement | Phase |
| --- | --- | --- |
| MFA | TOTP required on new device; backup codes; SMS fallback optional | MVP |
| Subscription | Tier per branch, add-ons, billing cycle switch (Monthly or Yearly), invoices with GST itemised, payment method, cancel; prices shown GST-exclusive; upgrade/downgrade mid-cycle prorated; cancellation follows the refund policy (no partial refunds) (revised 23 Oct 2026, UX) | MVP |
| Branches | Create, rename, timezone, pairing codes, standby pairing | MVP |
| Live view | Premium: connect to a Hub through the Tunnel for second-by-second floor view | Premium |
| Team | Additional owner-level logins with read-only or full roles | v1.1 |

## Marketing site and onboarding

The site sells one promise: run your cafe offline, see it from anywhere, pay less than you do now. Onboarding is a single wizard that ends with a test session, not a settings page.

**Marketing site** (static, Cloudflare Pages)

| Page | Content | Phase |
| --- | --- | --- |
| Home | Hero with floor view illustration, three-step story (install, pair, play), logos of cafes, CTA to get started | MVP |
| Product | Per app: Hub, Kiosk, Console, Owner, with illustrated walkthroughs and a 90-second demo video each | MVP |
| Pricing | Per-branch pricing shown GST-exclusive ("₹200/branch/month + 18% GST"): Free (1 branch, ads on the Android Hub), Basic ₹200, Premium ₹1,999; Monthly/Yearly toggle (yearly = 10× monthly, paid tiers only); add-ons with prices; Partners tab; FAQ on what happens offline and on cancellation and refunds (see Tiers) (revised 23 Oct 2026, UX) | MVP |
| Demo | Interactive sandbox Hub in the browser with sample data (same web UI, in-memory DB) | v1.1 |
| Compare | Versus ggLeap, Smartlaunch, iCafe: offline, price, hardware needs, migration | v1.1 |
| Get started | Sign-up form: name, phone, email, cafe name, city; OTP; lands in onboarding | MVP |
| Downloads | Hub and Kiosk installers, Pi image, system requirements, checksums | MVP |
| Help centre | Setup guides, troubleshooting, printer and CEC compatibility lists | MVP |
| FAQ | Billing, data ownership, offline, ads, refunds (full refund within 7 days of first payment; no partial refunds after) (revised 23 Oct 2026, UX) | MVP |
| Status and changelog | Cloud status, release notes | v1.1 |
| Partners | Reseller and installer program: margin, lead sharing, payout terms (see Resolved in second pass). Light application (name, phone OTP, email, city, business name, type reseller/installer, cafes served); approval by WhatsApp only, with a login link to the partner portal at partners.arena360.in (separate login); KYC after approval (PAN, bank details, optional GSTIN) before first payout. Branch customisation stays logo-only (revised 23 Oct 2026, UX) | v1.1 |
| Legal | Terms, privacy, data processing, ad policy | MVP |

**Onboarding wizard** (web, after sign-up; resumable)

| Step | What happens | Done when |
| --- | --- | --- |
| 1. Tenant | Cafe name, country, currency, timezone, GST registered yes/no and GSTIN | Tenant and first branch created |
| 2. Tier | Pick Free, Basic or Premium per branch; payment if paid; Free needs no card | Entitlement issued |
| 3. Install Hub | Download link; 8-character pairing code shown large with Copy and a 15-min countdown, no QR; owner types it into the Hub installer, and a bad code retries in place; page waits for the Hub to pair | Hub heartbeat received (revised 23 Oct 2026, UX) |
| 4. Devices | Hub auto-discovers kiosks that have the Kiosk app; wizard shows them arriving; create groups and drag devices in; add consoles manually | At least one device in a group |
| 5. Plans | Choose a template (Indian cafe, console lounge, VR arcade) or import CSV; edit rates per group | At least one rate per group |
| 6. Products | Template menu or CSV import; mark kitchen items | Optional; can skip |
| 7. Staff | Invite by phone; pick role; a one-time setup code is shown, and the staff member sets their own PIN and password at the Hub on first login (revised 23 Oct 2026, architecture) | At least one manager |
| 8. Printer and payments | Detect ESC/POS printer; UPI and card are recorded by reference, so no payment provider keys | Test print succeeded or skipped (revised 23 Oct 2026, UX) |
| 9. Test session | Start a 2-minute session on one device from the wizard; watch it lock | Session completed |
| 10. Go live | Checklist: opening hours, backup location, standby PC (paid), ad consent (Free) | Branch marked live; owner app dashboard unlocks |

**Single-key setup.** The pairing code in step 3 encodes tenant, branch and tier, so the Hub installer needs nothing else typed; kiosks and Pis pair with a 6-digit code from the Hub screen. Re-running the wizard for a second branch reuses groups, plans and products as templates by default.

**Migration.** Step 5 and 6 accept CSV; v1.1 adds direct upload of ggLeap and Smartlaunch exports with field mapping and a dry-run report.

## Cross-cutting requirements

These apply to every app and are tested as a group before each release.

| Area | Requirement | Acceptance criteria |
| --- | --- | --- |
| Offline | Every Hub and Kiosk feature works with the WAN down; cloud features queue and resume | 8-hour offline soak test with 20 kiosks, zero data loss |
| Tax | GST inclusive or exclusive per branch; rates per item; HSN; sequential invoice numbers per branch and financial year; credit notes for refunds; B2B invoice with customer GSTIN | Sample month reconciles to GSTR-1 format export |
| Hardware | ESC/POS printers (USB, LAN, Bluetooth), cash drawer via printer kick, USB barcode scanners (keyboard wedge), Pi Zero 2 W and Pi 4, HDMI-CEC TVs, Tuya or Tapo smart plugs | Compatibility list on the help centre; tested devices badge |
| Payments | UPI recorded manually by reference (last 4 digits, required); card via external terminal with reference entry; cash | Settling never depends on the internet; references on receipt and Z-report (revised 23 Oct 2026, UX) |
| Notifications | WhatsApp Business API and SMS fallback sent by the cloud via Arena 360's Gupshup account (delayed while the branch is offline), standard web push for the Owner app PWA (iOS: Home-Screen installs only); player messages are service messages only (revised 23 Oct 2026, architecture) | Templates versioned; sign-up consent stored per player (revised 23 Oct 2026, UX) |
| Ads (Free) | AdMob banner and interstitial on the Android Hub only; no ad on any player screen (Kiosk or Pi) | Banner always; interstitial only when staff return after the idle lock, never over an open cart or tile sheet; upgrade removes ads within one heartbeat (revised 23 Oct 2026, UX) (revised 23 Oct 2026, architecture) |
| Languages | UI strings externalised; English only at MVP (revised 22 Oct 2026); number and date formats by locale | No hard-coded strings in release build |
| Accessibility | Touch targets 44 px minimum on Hub; high-contrast theme; keyboard shortcuts for POS | Counter flow possible without mouse |
| Performance | Rust Hub service idle under 50 MB RAM (Hub UI budgeted separately, under 250 MB), UI actions under 100 ms on LAN, floor view 60 devices without lag on a 2015 PC; Hub daily rollup at close under 5 s and owner summary under 50 ms; owner dashboard under 2 s (revised 23 Oct 2026, architecture) | Benchmark on reference hardware per release; Android rollup under 5 s on a 3 GB device (revised 23 Oct 2026, architecture) |
| Security | Device tokens, PIN elevation, audit on every write, no inbound ports on the Hub, SQLCipher opt-in | Pen test on Kiosk lockdown before GA |
| Data ownership | Owner can export the full branch database and all reports at any time, including on Free | Export documented and tested |
| Hosting and residency | Control plane and its database hosted in India (Mumbai); branch files on object storage | Control-plane data stays in India (revised 23 Oct 2026, architecture) |
| Migration | CSV import for players, plans, products with dry-run; competitor importers in v1.1 | 1,000-row import under 30 s |
| Updates | Hub and Kiosk auto-update with staged rollout and rollback; never during an open session | Rollback tested each release |
| Support | In-app "send diagnostics" bundles logs and config (no player PII) | Bundle under 5 MB |

**Windows support.** The Kiosk and the Windows Hub are Tauri apps rendering in WebView2 and target Windows 10 and later. Windows 7 is not supported: cafes with Windows 7 gaming PCs must upgrade them, and cafes on Windows 7 counter PCs use the Android Hub or upgrade the counter PC. (Revised 22 Oct 2026; previously .NET Framework 4.8 with WPF on Windows 7 SP1.)

## Tiers and ads

Pricing is per branch per month, shown GST-exclusive ("₹200/branch/month + 18% GST") with GST itemised on the invoice (revised 23 Oct 2026, UX). Free is a complete single-branch product funded by ads; Basic removes ads and adds the cloud; Premium adds resilience and remote access.

| Capability | Free (₹0, 1 branch) | Basic (₹200) | Premium (₹1,999) |
| --- | --- | --- | --- |
| Hub: sessions, players, plans, devices, groups, dynamic pricing | ✓ | ✓ | ✓ |
| POS, inventory, waste, expiry, shifts, expenses, GST invoices | ✓ | ✓ | ✓ |
| Kiosk app, lockdown, overlay, game library | ✓ | ✓ | ✓ |
| Kiosk (table) orders (revised 23 Oct 2026, UX) | — (counter POS) | ✓ | ✓ |
| Console as timed device | ✓ | ✓ | ✓ |
| Pi controller (v1.1) | ✓ | ✓ | ✓ |
| Branch users and RBAC | 3 users | Unlimited | Unlimited |
| Max devices | 20 | 60 | 200 |
| Kitchen KOT and display | — | ✓ | ✓ |
| Local reports and CSV export | ✓ | ✓ | ✓ |
| Backup | Manual export; nothing uploaded to the cloud (upgrading uploads a full snapshot first) (revised 23 Oct 2026, architecture) | Cloud, 15-min batches, 1-year retention; yearly GST archive kept 8 years (revised 23 Oct 2026, architecture) | Cloud, 5-min batches, 3-year retention; yearly GST archive kept 8 years (revised 23 Oct 2026, architecture) |
| Owner app dashboards, alerts, exports | — | ✓ | ✓ |
| Owner commands (remote price and staff changes) | — | ✓ | ✓ |
| Multi-branch | — | ✓ | ✓ |
| Standby Hub on LAN | — | ✓ | ✓ |
| Remote devices via Tunnel | — | add-on | ✓ |
| Off-site standby (STUN/TURN) | — | — | ✓ |
| Live owner view | — | — | ✓ |
| Scheduled reports | — | ✓ | ✓ |
| Ads on Hub banner | ✓ | — | — |
| Ads on Hub interstitial (only on return after the idle lock, skippable after 5 s) (revised 23 Oct 2026, UX) (revised 23 Oct 2026, architecture) | ✓ | — | — |
| Support | Community and help centre | Email, 24 h | Chat, 4 h |

**Free-tier Hub on Android.** The Free plan runs the Hub as an Android app on a tablet or phone at the counter; it serves kiosks on the LAN exactly as the Windows Hub does, with the same SQLite and change log. The Android Hub is built on Tauri 2 mobile from the same web UI as the Windows Hub. Ads are delivered by AdMob inside the Android app: a banner on the Hub screen at all times and an interstitial only when staff return after the idle lock, skippable after 5 s, never while a cart or tile sheet is open (revised 23 Oct 2026, UX) (revised 23 Oct 2026, architecture). Upgrading to Basic unlocks the Windows Hub build and removes ads on both. Standby and Tunnel features require the Windows build.

**Ad placement rules**

1. Ads never appear on a player screen (Kiosk or Pi), in or between sessions. They appear only on the Free Android Hub: the banner and the interstitial (revised 23 Oct 2026, UX).
2. The banner shows at all times; the interstitial shows only when staff return after the idle lock, never after a settle, is skippable after 5 s, and never covers an open cart or tile sheet (revised 23 Oct 2026, UX) (revised 23 Oct 2026, architecture).
3. AdMob serves and counts the ads inside the Android Hub; no creative packs are sent to kiosks (revised 23 Oct 2026, UX).
4. No adult, gambling or alcohol categories; owner can block categories.
5. A Free branch that upgrades stops showing ads within one heartbeat; a lapsed paid branch resumes ads only after the entitlement expires.

**Add-ons** (any tier, per branch per month, + GST): +20 devices ₹50, Tunnel for remote devices ₹200, additional owner login ₹50; other regions follow the PPP bands below (revised 23 Oct 2026, UX). Razorpay bills one subscription per branch; a renewal over ₹15,000 (including GST) uses card or eNACH rather than UPI Autopay (revised 23 Oct 2026, architecture).

**Yearly billing.** Paid tiers can be billed yearly at 10× monthly (2 months free): Basic ₹2,000 and Premium ₹19,990 per branch per year + GST. Monthly/Yearly toggle on the Pricing page; cycle switch in Owner app Subscription (revised 23 Oct 2026, UX).

**Refunds.** Full refund within 7 days of the first payment. After that, cancel anytime: the plan stays active to the end of the paid period (monthly or yearly), then the branch drops to Free. No partial refunds (revised 23 Oct 2026, UX).

**Regional pricing (purchasing-power parity).** India is the anchor at ₹200 and ₹1,999; other regions are set so the price feels like the Indian one locally, roughly 2.5× the raw exchange rate for high-income markets.

| Region | Basic / month | Premium / month | Currency |
| --- | --- | --- | --- |
| India | ₹200 | ₹1,999 | INR |
| Malaysia | \~$5 | \~$49 | MYR |
| Singapore | $6 | $59 | SGD |
| USA | $6 | $59 | USD |

Launch markets are India plus the Razorpay-supported countries above; currency is set per branch, and a country Razorpay cannot bill in is not sold to until it can (revised 23 Oct 2026, architecture).

Region is set from the tenant's billing country at sign-up and verified by payment method; the control plane stores the price band on the subscription, so a later change to the table does not reprice existing tenants.

## Non-goals, success metrics, open questions

**Non-goals for MVP and v1.1**

- Multi-warehouse or per-store inventory beyond one stock level per branch.
- Accounting (ledgers, P&L); an export to Tally and Zoho is v2.
- A player mobile app; players use the kiosk and QR pages.
- Game licensing or cafe-licensed game distribution.
- Streaming, cloud gaming or remote play.
- Multi-master or CRDT sync inside a branch (see architecture doc).
- Staff payroll or HR.
- Arbitrary SQL reporting in the cloud over branch data; owner reports are the defined fields in the Hub's daily rollup (revised 23 Oct 2026, architecture).

**Success metrics**

| Metric | Target | Measured by |
| --- | --- | --- |
| Time from sign-up to first paid session | Under 60 min for 80% of branches | Onboarding telemetry |
| Counter actions | Session start, product sale, shift close each under 10 s | Hub timing logs |
| Offline reliability | Zero lost transactions over an 8-hour WAN outage | Soak test and field reports |
| Free to paid conversion | 20% within 90 days | Control plane |
| Monthly churn (paid) | Under 3% | Control plane |
| Cash variance | Median shift variance under ₹50 after 30 days of use | Daily rollups (revised 23 Oct 2026, architecture) |
| Support load | Under 2% of branches raise a ticket per month | Help desk |
| Owner dashboard data age | Within the tier target (Basic ≤ 15 min, Premium ≤ 5 min) on 99% of opens (revised 23 Oct 2026, architecture) | Owner app telemetry |
| Marginal infra cost | Under ₹10 per paid branch per month on average (about ₹6 Basic, ₹12 Premium) (revised 23 Oct 2026, architecture) | Cloud bills |
| Ad fill (Free) | 40% in year one | Ad server |

**Open questions — resolved 22 Oct 2026**

| Question | Decision |
| --- | --- |
| WhatsApp Business provider | Gupshup |
| Ad inventory | AdMob banner and interstitial on the Android Hub (Free tier only), banner and interstitial only; no Windows ad SDK (revised 23 Oct 2026, UX) |
| OTP on sign-up | Mandatory for the Owner app; none for player sign-up at kiosk |
| Minimum Windows version for Kiosk | Windows 10 (revised 22 Oct 2026; was Windows 7 SP1) |
| Console packs shared with PCs | Never; consoles always have separate groups and packs |
| Partner program timing | At launch, with reseller margin and lead sharing defined before pricing page goes live |
| Second-wave countries | India plus Malaysia, Singapore and the US at launch with PPP pricing (see Tiers); further countries once Razorpay can bill there (revised 23 Oct 2026, architecture) |

**Resolved in second pass, 22 Oct 2026**

| Question | Decision | Consequence |
| --- | --- | --- |
| Reseller margin and territory | 10% of subscription revenue for every month the referred tenant stays active and paying; leads are not exclusive by territory | Control plane tracks `referral_code` on the tenant and computes payouts monthly; payout by bank transfer by the 10th for the previous month, ₹500 minimum (lower balance rolls over), TDS deducted at 2% (subject to tax advice), GST-registered partners upload a GST invoice; partner portal (partners.arena360.in) shows referred tenants, accrued, pending, paid, TDS and invoice status (revised 23 Oct 2026, UX) |
| Android Hub minimum spec | 3 GB RAM, 20 GB free storage, Android 9 or later | Play Store listing enforces it; installer checks free space before first run |
| Payment providers | Razorpay only. India in INR; other countries in local currency where Razorpay supports it, otherwise USD. Autopay (recurring mandate) is mandatory on every paid plan; one subscription per branch, with card or eNACH for renewals over ₹15,000 (revised 23 Oct 2026, architecture) | One billing integration; no other payment providers (revised 23 Oct 2026, architecture); a country Razorpay cannot bill in is not sold to until it can |

No open questions remain for launch scope.
