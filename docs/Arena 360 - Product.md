# Gamezone SaaS — Product Requirements

Oct 22, 2026 · @Harish

## Summary and goals

Gamezone is a local-first management system for gaming cafes and game zones: a Hub app that runs the branch from a counter PC, a Kiosk app on every gaming PC, a Pi controller for consoles, an Owner app for multi-branch oversight, and a marketing site with a self-serve onboarding flow. The branch keeps working with the internet down; the cloud mirrors data and relays owner commands. Architecture is specified separately in "Gamezone SaaS — Low-Level Architecture".

**Goals**

1. A cafe owner installs the Hub, pairs 20 PCs and runs the first paid session within 60 minutes, without support.
2. Counter staff complete the three daily jobs (start a session, sell a snack, close the shift) in under 10 seconds each, offline.
3. Owners see cash, sessions and stock across every branch from their phone, and change prices from there.
4. Free tier is usable for a single-branch cafe forever, funded by ads; paid tiers are priced under every incumbent.
5. Marginal cost per tenant stays under ₹20 a month at any scale.

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
| Free-tier Hub | Free plan runs the Hub on Android (tablet or phone) only; the Windows Hub build is for paid tiers | AdMob serves rewarded and interstitial ads on Android; Windows build carries no ad SDK |
| Ads | Rewarded ads via AdMob on the Android Hub; attract-screen ads on kiosks served from the Hub's creative pack | No direct ad sales team needed at launch |
| WhatsApp | Gupshup as the WhatsApp Business provider | SMS fallback also via Gupshup |
| OTP | Mandatory on Owner app sign-up and new-device login; player sign-up at kiosk needs no OTP | SMS cost only on owner side |
| Kiosk OS | Windows 7 SP1 and later | Kiosk built on .NET Framework 4.8 (WPF), not Tauri or WebView2, which no longer support Windows 7 |
| Console pricing | Consoles always have their own groups and packs; never shared with PC packs | Simplifies pack rules; a "combo" pack is two packs sold together |
| Partner program | Reseller and installer program at launch | Pricing page carries a partner tab; control plane tracks referral codes |
| Markets | All countries at launch with purchasing-power-parity pricing | See Regional pricing under Tiers |

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
| Money | Split payment, UPI QR, cash drawer, GST invoices, shifts with variance, expenses, deposits | Refund approvals, petty cash ledger | Accounting export (Tally, Zoho) |
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
| Profile | Phone (unique per branch), name, PIN, optional email, DOB, notes, tags | Create in under 15 s at counter; phone lookup finds a player in 1 tap | MVP |
| Guest | Start a session without a profile; convert to profile later | Guest sessions appear in reports as "Guest"; conversion keeps history | MVP |
| Wallet | Prepaid balance in paise; top-up by cash, UPI, card; full ledger | Balance never negative; every change has a ledger row with actor | MVP |
| Credit | Postpaid limit per player, overdue days, reminder cycle (3, 7, 14 days) via WhatsApp/SMS | Session start blocked when limit exceeded; manager PIN can override once | MVP |
| Packs owned | Time packs with hours remaining and expiry | Expired packs greyed; partial use deducts minutes | MVP |
| Import | CSV of players with phone, name, balance, credit limit | Dry-run shows conflicts before commit | MVP |
| Loyalty, referrals | Points per rupee, redeem as wallet credit; referral bonus on first paid session | Configurable rates per branch | v1.1 |

**Sessions**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Postpaid walk-in | Open session on a device; billed at end at the device group's rate | Rate shown on start; bill computed to the minute, rounded by config (default 15 min, min 30 min) | MVP |
| Prepaid | Start on a pack or wallet; time counts down | Kiosk shows remaining time; auto-lock at zero with configurable grace (default 2 min) | MVP |
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
| UPI QR | Dynamic QR with exact amount via Razorpay/PhonePe; auto-confirm on webhook when online, manual confirm offline | Offline confirmations flagged for reconciliation | MVP |
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
| Shift | Open with float, close with counted cash; system expected vs counted = variance | Variance over threshold requires manager PIN and reason | MVP |
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
| Users | Name, login, password, PIN, role, active flag; owner can create from Owner app via command | User changes sync to branch within one poll | MVP |
| Roles | Default roles: Owner, Manager, Counter, Kitchen, Kiosk-device; custom roles from the permission catalogue | Permissions grouped by module; each is allow/deny | MVP |
| Permission catalogue | \~40 permissions: `session.start`, `session.force_end`, `wallet.topup`, `discount.manual`, `refund.issue`, `shift.close`, `shift.override_variance`, `product.edit`, `stock.adjust`, `price.edit`, `user.manage`, `report.view`, `settings.edit`, `backup.export`, `device.remote` and so on | Every `apply` intent maps to exactly one permission | MVP |
| PIN elevation | A counter user can perform a manager action by entering a manager's PIN; audit records both | PIN entry never leaves the current screen | MVP |
| Session lock | Hub UI locks after N minutes idle; unlock with PIN | Default 5 min | MVP |
| Attendance | Clock-in/out tied to shift | Hours per staff in reports | v1.1 |

**Configuration**

| Setting | Options | Phase |
| --- | --- | --- |
| Currency and locale | Currency code, symbol position, number format, timezone | MVP |
| Tax | GST on/off, GSTIN, default rates, inclusive/exclusive pricing, invoice prefix and sequence | MVP |
| Billing rules | Rounding unit, minimum billing, grace period, auto-lock behaviour | MVP |
| Receipts | Printer (ESC/POS over USB, LAN or Bluetooth), paper width, logo, footer text, WhatsApp template | MVP |
| Opening hours | Per day; used by passes, dynamic pricing and "branch offline" alerts | MVP |
| Notifications | WhatsApp provider keys, SMS provider, templates for credit reminder, low balance, session ending | MVP |
| Ads (Free tier) | Consent view; ad slots cannot be disabled on Free | MVP |
| Language | English, Hindi, Tamil, Telugu, Kannada, Bengali, Indonesian, Vietnamese at launch | MVP (English, Hindi), v1.1 (rest) |

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
| Cloud backup status | Last upload time, last successful ingest, pending batches | Visible in Hub header | MVP (paid) |
| Standby | Pair a second PC as standby; promote button with confirmation | Promotion completes under 10 s; kiosks reconnect automatically | MVP (paid) |
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
| Attract screen | Idle loop: branch branding, current rates, promotions; ad slot on Free tier | Ad every 15 min minimum per entitlement; no ads on Basic/Premium | MVP |
| Login | Phone + PIN, or QR from Hub, or staff starts from Hub (walk-in) | Login under 3 s on LAN | MVP |
| Sign-up | Phone, name, PIN; OTP optional per branch; consent text | Creates profile on Hub; duplicates blocked by phone | MVP |
| Session start | Choose plan or pack when prepaid; postpaid shows rate and starts | Shows device group rate and any active dynamic rule | MVP |
| Overlay HUD | Small always-on-top widget: time left or elapsed, balance, hotkey to expand | Does not steal focus from games; hides in fullscreen exclusive mode with a toast on warnings | MVP |
| Warnings | At 10 and 5 minutes and 60 seconds; low balance | Toast plus sound; configurable | MVP |
| Extend | From overlay, charged to wallet, pack or credit; blocked if credit limit hit | Extension reflected on Hub floor view within 2 s | MVP |
| Time-up | Lock screen with grace countdown; game left running for grace period; then kill allow-listed processes if configured | Player has a visible countdown; no data loss within grace | MVP |
| Orders | Menu with images, cart, order to seat; status updates from kitchen | Order reaches Hub POS and KOT within 3 s | MVP |
| Messages | Staff broadcast or per-seat message appears as toast | Delivered within 2 s | MVP |

**Games and launchers**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Library | Hub holds the catalogue: name, executable, launcher (Steam, Epic, Riot, standalone), art, category, age rating | Edit once on Hub, every kiosk updates | MVP |
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
| Player alert | Optional WhatsApp or SMS at 5 minutes | Sent only if player profile has consent |
| End | Staff end session; time-up shows alert on Hub, staff walk over | Audit records who ended it |

**v1.1 — Pi controller**

| Feature | Requirement | Acceptance criteria |
| --- | --- | --- |
| Hardware | Raspberry Pi Zero 2 W or Pi 4 with HDMI to TV; console on another HDMI input; optional smart plug for consoles without CEC | Reference BOM under ₹4,000 per TV |
| Pairing | Pi pairs to Hub with a 6-digit code like a kiosk; shows its TV number on screen | Visible on floor view within 60 s |
| Idle screen | Pi output shown on TV when no session: branch branding, rates, QR code to start; ad slot on Free | QR encodes branch and TV id |
| Start by QR | Player scans with phone, lands on a Hub page via LAN (or Tunnel on Premium), logs in, picks plan; Hub tells Pi to switch TV input to console (CEC) | Input switch within 3 s of start |
| Start by staff | Same from Hub | Same |
| Time and warnings | Pi overlays nothing on the console input; warnings go to player's phone and to the TV by switching briefly to the Pi input at 5 and 1 minutes (configurable) | Switch-back is automatic |
| Time-up | Pi switches TV to its own input with a lock screen; optional CEC standby to console, or smart plug off after grace | Console not powered off mid-save during grace |
| Controller check | Pi can prompt staff to confirm controllers returned at end | Logged against session |
| Remote commands | Power on/off, input switch, message to TV from Hub | Result within 5 s |

Consoles without CEC support (older TVs) fall back to the smart-plug path; the PRD treats CEC as best effort and the plug as the guarantee.

## Owner app

The Owner app is a phone app and a web app on the same account, reading the cloud mirror and writing commands. It never needs the branch to be open. On Free tier it shows subscription, pairing and support only; dashboards start at Basic.

**Dashboards and reports**

| Feature | Requirement | Acceptance criteria | Phase |
| --- | --- | --- | --- |
| Today | Per branch: open/closed, sessions live, revenue so far, cash in drawer, staff on shift, last sync time | Loads in under 2 s from mirror; shows data age | MVP |
| Revenue | Day, week, month, custom; by branch, by type (sessions, packs, products), by payment method | Matches Hub Z-report totals exactly | MVP |
| Cash reconciliation | Shift-by-shift expected vs counted, variance, who closed, deposits, expenses | Variance over threshold highlighted | MVP |
| Stock | Levels, low-stock, waste by reason and value, expiring | Per branch and total | MVP |
| Credit | Outstanding by player, aged; reminders sent | Tap to send reminder now (command) | MVP |
| Players | New, returning, top spenders, retention cohorts | Weekly cohort chart | v1.1 |
| Devices | Utilisation per group per hour; offline devices | Heatmap view | v1.1 |
| Staff | Sales, discounts, voids, shift variance per staff | Sort by variance | v1.1 |
| Audit | Searchable change log from the mirrored `changes` table | Filter by branch, actor, module | MVP |
| Exports | CSV and PDF for every report; scheduled email or WhatsApp daily/weekly | Scheduled reports arrive by 8 am local | MVP (export), v1.1 (scheduled) |

**Alerts** (push and WhatsApp, thresholds per tenant)

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
| Subscription | Tier per branch, add-ons, invoices, payment method, upgrade/downgrade mid-cycle prorated | MVP |
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
| Pricing | Per-branch pricing: Free (1 branch, ads), Basic ₹200, Premium ₹1,999; add-ons; yearly discount; FAQ on what happens offline and on cancellation | MVP |
| Demo | Interactive sandbox Hub in the browser with sample data (same web UI, in-memory DB) | v1.1 |
| Compare | Versus ggLeap, Smartlaunch, iCafe: offline, price, hardware needs, migration | v1.1 |
| Get started | Sign-up form: name, phone, email, cafe name, city; OTP; lands in onboarding | MVP |
| Downloads | Hub and Kiosk installers, Pi image, system requirements, checksums | MVP |
| Help centre | Setup guides, troubleshooting, printer and CEC compatibility lists | MVP |
| FAQ | Billing, data ownership, offline, ads, refunds | MVP |
| Status and changelog | Cloud status, release notes | v1.1 |
| Partners | Reseller and installer program: margin, lead sharing, co-branded kiosk screen | v1.1 |
| Legal | Terms, privacy, data processing, ad policy | MVP |

**Onboarding wizard** (web, after sign-up; resumable)

| Step | What happens | Done when |
| --- | --- | --- |
| 1. Tenant | Cafe name, country, currency, timezone, GST registered yes/no and GSTIN | Tenant and first branch created |
| 2. Tier | Pick Free, Basic or Premium per branch; payment if paid; Free needs no card | Entitlement issued |
| 3. Install Hub | Download link; pairing code shown large with QR; page waits for the Hub to pair | Hub heartbeat received |
| 4. Devices | Hub auto-discovers kiosks that have the Kiosk app; wizard shows them arriving; create groups and drag devices in; add consoles manually | At least one device in a group |
| 5. Plans | Choose a template (Indian cafe, console lounge, VR arcade) or import CSV; edit rates per group | At least one rate per group |
| 6. Products | Template menu or CSV import; mark kitchen items | Optional; can skip |
| 7. Staff | Invite by phone; pick role; set PIN | At least one manager |
| 8. Printer and payments | Detect ESC/POS printer; add UPI provider keys (optional) | Test print succeeded or skipped |
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
| Payments | UPI dynamic QR via Razorpay and PhonePe; card via external terminal with reference entry; cash | Webhook confirm within 10 s online; manual confirm offline flagged |
| Notifications | WhatsApp Business API (via provider), SMS fallback, push for owner app | Templates versioned; opt-in stored per player |
| Ads (Free) | Attract-screen and Hub-banner slots served from a cached creative pack; impressions batched to the change log; no ads during a session on a player screen | Impression counts within 2% of cloud tally |
| Languages | UI strings externalised; English and Hindi at MVP; number and date formats by locale | No hard-coded strings in release build |
| Accessibility | Touch targets 44 px minimum on Hub; high-contrast theme; keyboard shortcuts for POS | Counter flow possible without mouse |
| Performance | Hub idle under 50 MB RAM, UI actions under 100 ms on LAN, floor view 60 devices without lag on a 2015 PC | Benchmark on reference hardware per release |
| Security | Device tokens, PIN elevation, audit on every write, no inbound ports on the Hub, SQLCipher opt-in | Pen test on Kiosk lockdown before GA |
| Data ownership | Owner can export the full branch database and all reports at any time, including on Free | Export documented and tested |
| Migration | CSV import for players, plans, products with dry-run; competitor importers in v1.1 | 1,000-row import under 30 s |
| Updates | Hub and Kiosk auto-update with staged rollout and rollback; never during an open session | Rollback tested each release |
| Support | In-app "send diagnostics" bundles logs and config (no player PII) | Bundle under 5 MB |

**Windows 7 support.** The Kiosk targets Windows 7 SP1 through Windows 11 on .NET Framework 4.8 with WPF. TLS 1.2 must be enabled explicitly on Windows 7 (registry and `ServicePointManager`), WebSocket uses a managed client rather than the OS stack, and the overlay avoids DirectComposition. The Windows Hub build targets Windows 10 and later; cafes on Windows 7 counter PCs use the Android Hub or upgrade the counter PC.

## Tiers and ads

Pricing is per branch per month. Free is a complete single-branch product funded by ads; Basic removes ads and adds the cloud; Premium adds resilience and remote access.

| Capability | Free (₹0, 1 branch) | Basic (₹200) | Premium (₹1,999) |
| --- | --- | --- | --- |
| Hub: sessions, players, plans, devices, groups, dynamic pricing | ✓ | ✓ | ✓ |
| POS, inventory, waste, expiry, shifts, expenses, GST invoices | ✓ | ✓ | ✓ |
| Kiosk app, lockdown, overlay, orders, game library | ✓ | ✓ | ✓ |
| Console as timed device | ✓ | ✓ | ✓ |
| Pi controller (v1.1) | ✓ | ✓ | ✓ |
| Branch users and RBAC | 3 users | Unlimited | Unlimited |
| Max devices | 20 | 60 | 200 |
| Kitchen KOT and display | — | ✓ | ✓ |
| Local reports and CSV export | ✓ | ✓ | ✓ |
| Backup | Manual export | Cloud, 5-min batches, 1-year retention | Cloud, 1-min batches, 3-year retention |
| Owner app dashboards, alerts, exports | — | ✓ | ✓ |
| Owner commands (remote price and staff changes) | — | ✓ | ✓ |
| Multi-branch | — | ✓ | ✓ |
| Standby Hub on LAN | — | ✓ | ✓ |
| Remote devices via Tunnel | — | add-on | ✓ |
| Off-site standby (STUN/TURN) | — | — | ✓ |
| Live owner view | — | — | ✓ |
| Scheduled reports | — | ✓ | ✓ |
| Ads on attract screen | Every 15 min | — | — |
| Ads on Hub banner | ✓ | — | — |
| Rewarded ads (player watches 30 s for bonus minutes, cafe sets the reward) | Optional | — | — |
| Support | Community and help centre | Email, 24 h | Chat, 4 h |

**Free-tier Hub on Android.** The Free plan runs the Hub as an Android app on a tablet or phone at the counter; it serves kiosks on the LAN exactly as the Windows Hub does, with the same SQLite and change log. Ads are delivered by AdMob inside the Android app: a banner on the Hub screen and rewarded video that the cafe can offer players for bonus minutes. Upgrading to Basic unlocks the Windows Hub build and removes ads on both. Standby and Tunnel features require the Windows build.

**Ad placement rules**

1. Ads never appear on a player screen during a paid session. They appear on the attract screen between sessions and on the Hub banner.
2. Rewarded ads are opt-in by the cafe; the cafe sets the reward (minutes or wallet credit) and the daily cap per player.
3. Creative packs are downloaded daily and served from the Hub, so ads work offline; impressions are recorded in the change log and uploaded with the daily heartbeat.
4. No adult, gambling or alcohol categories; owner can block categories.
5. A Free branch that upgrades stops showing ads within one heartbeat; a lapsed paid branch resumes ads only after the entitlement expires.

**Add-ons** (any tier): extra devices in blocks of 20, Tunnel for remote devices, additional owner logins.

**Regional pricing (purchasing-power parity).** India is the anchor at ₹200 and ₹1,999; other regions are set so the price feels like the Indian one locally, roughly 2.5× the raw exchange rate for high-income markets.

| Region | Basic / month | Premium / month | Currency |
| --- | --- | --- | --- |
| India | ₹200 | ₹1,999 | INR |
| Indonesia, Vietnam, Philippines, Bangladesh, Pakistan | \~$3 | \~$29 | local, via Stripe or Xendit |
| Brazil, Mexico, Colombia, Turkey, Egypt | \~$4 | \~$39 | local |
| Eastern Europe, Middle East, South Africa | \~$5 | \~$49 | local or USD |
| USA, Canada, Western Europe, UK, Australia, Japan, Korea | $6 | $59 | USD, EUR, GBP |

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

**Success metrics**

| Metric | Target | Measured by |
| --- | --- | --- |
| Time from sign-up to first paid session | Under 60 min for 80% of branches | Onboarding telemetry |
| Counter actions | Session start, product sale, shift close each under 10 s | Hub timing logs |
| Offline reliability | Zero lost transactions over an 8-hour WAN outage | Soak test and field reports |
| Free to paid conversion | 20% within 90 days | Control plane |
| Monthly churn (paid) | Under 3% | Control plane |
| Cash variance | Median shift variance under ₹50 after 30 days of use | Mirror data |
| Support load | Under 2% of branches raise a ticket per month | Help desk |
| Marginal infra cost | Under ₹20 per paid branch per month | Cloud bills |
| Ad fill (Free) | 40% in year one | Ad server |

**Open questions — resolved 22 Oct 2026**

| Question | Decision |
| --- | --- |
| WhatsApp Business provider | Gupshup |
| Rewarded-ad inventory | AdMob on the Android Hub (Free tier only); no Windows ad SDK |
| OTP on sign-up | Mandatory for the Owner app; none for player sign-up at kiosk |
| Minimum Windows version for Kiosk | Windows 7 SP1 |
| Console packs shared with PCs | Never; consoles always have separate groups and packs |
| Partner program timing | At launch, with reseller margin and lead sharing defined before pricing page goes live |
| Second-wave countries | All countries at launch with PPP pricing bands (see Tiers) |

**Resolved in second pass, 22 Oct 2026**

| Question | Decision | Consequence |
| --- | --- | --- |
| Reseller margin and territory | 10% of subscription revenue for every month the referred tenant stays active and paying; leads are not exclusive by territory | Control plane tracks `referral_code` on the tenant and computes payouts monthly; partner dashboard shows active referred tenants |
| Android Hub minimum spec | 3 GB RAM, 20 GB free storage, Android 9 or later | Play Store listing enforces it; installer checks free space before first run |
| Payment providers | Razorpay only. India in INR; other countries in local currency where Razorpay supports it, otherwise USD. Autopay (recurring mandate) is mandatory on every paid plan | One billing integration; no Stripe or regional PSPs; a country Razorpay cannot bill in is not sold to until it can |

No open questions remain for launch scope.
