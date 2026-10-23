---
id: SPEC-arena-360
companions:
  - glossary.md
  - ../../planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md
  - ../../planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/EXPERIENCE.md
  - ../../planning-artifacts/architecture/architecture-arena-360-final-2026-10-23/ARCHITECTURE-SPINE.md
  - ../../../docs/Arena 360 - Product.md
  - ../../../docs/Arena360 — High-Level Design.md
  - ../../../docs/Arena360 — Low-Level Design.md
sources:
  - ../../../docs/Arena360 — User Journeys.md
---

> **Canonical contract.** This SPEC and the files in `companions:` are the complete, preservation-validated contract for what to build, test, and validate. Source documents listed in frontmatter are for traceability — consult them only if you need narrative rationale or prose color this contract intentionally omits.

# Arena 360 — local-first gaming café management

## Why

**Pain and opportunity.** Owners of 1–5 gaming cafés in India (and Malaysia, Singapore, the US at launch) run on notebooks, WhatsApp photos and incumbent tools (ggLeap, Smartlaunch, iCafe) that cost more, need the internet, and leak cash and stock. Owners phone staff for numbers; managers reconcile cash by hand and argue with players about minutes; high-turnover counter staff fight software that needs training and crashes mid-shift; players wait for staff and lose games to surprise time-ups. Arena 360 claims that gap with one promise: **run your café offline, see it from anywhere, pay less than you do now.** A Hub at the counter runs the branch with the WAN down, a Kiosk on every PC and a Pi per console TV enforce time, an Owner app shows every branch from a phone, and a Free tier (ad-funded Android Hub) undercuts every incumbent while paid tiers keep marginal cloud cost under ₹10 per branch per month. Everything below ships in one launch, built in wave order MVP then v1.1.

## Capabilities

Phase tag in each intent. Tier gating per PRD Tiers table; UX behaviour per EXPERIENCE.md; terms in glossary.md.

### Onboarding and setup

- **CAP-1**
  - **intent:** [MVP] An owner can sign up and sign in to their tenant securely from any device.
  - **success:** Sign-up requires phone OTP; a new-device login requires TOTP (backup codes, optional SMS fallback); sensitive commands (staff users/roles, revoke device/Hub, backup restore) require fresh MFA, others do not.
- **CAP-2**
  - **intent:** [MVP] An owner can take a new branch from sign-up to live through a resumable 10-step web wizard, importing existing data.
  - **success:** Steps tenant → tier → install Hub → devices → plans → products → staff → printer & payments → 2-min test session (screen locks at zero) → go-live checklist; resume lands on first incomplete step; products, staff, printer skippable; templates (Indian café, console lounge, VR arcade) or CSV import of players, plans and products with dry-run conflicts before commit, 1,000 rows < 30 s; a second branch reuses groups, plans, products by default; 80% of branches reach a first paid session < 60 min after sign-up without support.
- **CAP-3**
  - **intent:** [MVP] An owner can pair a branch's Hub by typing one code, with tier entitlement applied.
  - **success:** 8-character single-use code, 15-min expiry, shown with Copy and countdown, no QR, encodes tenant, branch and tier; bad/expired code retries in place in the installer and the web offers "Generate a new code"; page advances on first Hub heartbeat; Hub holds a device token and signed licence; Free offers Android only and a Windows Hub on Free is refused; onboarding profile (name, tz, currency, GST, hours, branch code) lands at first pairing.
- **CAP-4**
  - **intent:** [MVP] Staff can pair gaming PCs and register consoles, VR and simulators, organising them into groups and zones.
  - **success:** Kiosk discovers the Hub on the LAN (manual Hub IP fallback), pairs by 6-digit Hub code or manager PIN, appears "Unassigned" on the floor view within 60 s; each device in exactly one group (name, colour from six swatches, sort order); statuses online, offline, in session, reserved, maintenance (maintenance removes from pool without deleting); layout persists per branch; pairing beyond the tier cap (Free 20, Basic 60, Premium 200, +20 add-on) is refused; with the Tunnel entitlement (Premium, or the Tunnel add-on on a paid tier) a kiosk or Pi outside the café LAN pairs and connects as a remote device by device token, never anonymously.
- **CAP-5**
  - **intent:** [v1.1] An owner can migrate from ggLeap or Smartlaunch by uploading their exports.
  - **success:** Field mapping plus a dry-run report before commit.

### Floor and sessions

- **CAP-6**
  - **intent:** [MVP] Counter staff can see every device's live state and act on it from one floor view.
  - **success:** State updates ≤ 2 s; every state carries colour + label (free, in session with time, paused, reserved, offline, maintenance, order pending, unassigned); no action > 2 taps; active dynamic rule in header, change reflected ≤ 1 min; 60 devices without lag on a 2015 PC; collapsed zones still show alert counts; sheets never block the floor.
- **CAP-7**
  - **intent:** [MVP] Staff can start an open postpaid session for a profiled player or guest and bill it from the clock at the end.
  - **success:** Start shows group rate and active rule; kiosk unlocks ≤ 2 s; bill = minutes × rate with rounding unit (default 15 min) and minimum (default 30 min), paused time excluded, one line per rate segment, plus orders and tax; player can end via "I'm done" on HUD (PC locks, "Pay at counter"); start and settle each < 10 s from first tap; guest sessions report as "Guest", conversion keeps history.
- **CAP-8**
  - **intent:** [MVP] A player or staff can start a prepaid session on a pack, wallet balance, block or pass that counts down.
  - **success:** Only plans valid for the device group offered; partial pack use deducts minutes; expired packs greyed; a pack restricted to a group cannot start another group; block ends at its fixed time; one active pass per player; refusal returns a typed reason ("Balance ₹40. Top up at counter.").
- **CAP-9**
  - **intent:** [MVP] Staff can pause, extend, transfer, group-bill, lock and force-end sessions and control devices remotely.
  - **success:** Pause needs a reason, paused time unbilled; extend charged to wallet, pack or credit writes a ledger row, never past closing time; transfer locks old and unlocks new device ≤ 5 s, rate changes from that moment across groups; group session splits by device minutes, payer changeable before settlement; forced end needs manager PIN + reason, is audited, kiosk shows forced-end message; PC lock/unlock/message/restart/shutdown result ≤ 5 s or "Failed — retry"; staff messages reach kiosk ≤ 2 s; kiosk "Call staff" raises a tile badge + sound.
- **CAP-10**
  - **intent:** [v1.1] Staff can take bookings and run tournaments.
  - **success:** Booking reserves a device or group slot with deposit and no-show rules, tile shows Reserved 15 min before; tournament has bracket, entry fee posted to a tournament income account, and blocks devices for the event.

### Players

- **CAP-11**
  - **intent:** [MVP] Staff can create and find player profiles and manage wallet balances and owned packs.
  - **success:** Profile (username + password, phone, name, optional email, DOB, notes, tags) created < 15 s; username and phone unique per branch; phone lookup finds a player in 1 tap, no match offers Create profile or Guest; staff reset passwords; wallet in minor units never negative, every change a ledger row with actor; top-up at counter by cash, UPI or card; packs show hours left and expiry.
- **CAP-12**
  - **intent:** [MVP] Regulars can play and buy on credit within a limit, with reminders and collection.
  - **success:** Manager enables limit and cycle (default 3, 7, 14 days); every credit spend checks outstanding + new ≤ limit inside the transaction; block shows "Credit limit reached, see counter.", manager PIN overrides once per player per day, logged; outstanding aged 0–7, 8–14, 15+ days and shown first on profile; reminders on cycle days only with consent and only on paid tiers (Free shows an overdue list); credit auto-pauses at 15+ days overdue unless the branch disables it and reopens on payment; partial collection by any method with a receipt referencing settled transactions.
- **CAP-13**
  - **intent:** [v1.1] A branch can reward loyalty and referrals and sell monthly memberships.
  - **success:** Points per rupee redeemable as wallet credit, referral bonus on first paid session, rates per branch; membership is a monthly recurring pass with WhatsApp renewal reminder.

### Plans and pricing

- **CAP-14**
  - **intent:** [MVP] A manager or owner can define hourly rates, time packs, blocks and passes scoped to device groups.
  - **success:** Rate change affects new sessions only; a session only ever uses its device group's prices; pack has hours, price, validity days, allowed groups; pass windows may wrap midnight; console groups and packs are never shared with PCs (a combo is two packs).
- **CAP-15**
  - **intent:** [MVP] A manager can apply time-window and date pricing rules, coupons and manual discounts.
  - **success:** Weekday + hour rules per group (rate override or %); date rules outrank weekly rules; conflicts flagged on save; active rule shown on floor view, kiosk attract and plan picker; coupon (code, type, validity, usage limit, allowed groups/plans) applied at settlement and printed; manual discount needs manager PIN + reason and appears in audit and shift report.

### POS, money and tax

- **CAP-16**
  - **intent:** [MVP] Counter staff can sell products quickly and attach orders to running sessions.
  - **success:** Categories, favourites, combined search/barcode field; scanned item sold in 1 scan + 1 tap; cart attaches to a session and shows "ordered" on kiosk; bundle has its own price, decrements components, component shortage blocks with PIN override; out-of-stock and expired items disabled; product sale < 10 s.
- **CAP-17**
  - **intent:** [MVP] Staff can settle bills with any mix of payment methods, issue receipts, and refund or void.
  - **success:** Cash, UPI, card, wallet, credit in any mix; settle blocked until remainder is zero or moved to credit (profiled, credit-enabled player only); UPI row cannot save without last 4 digits of the reference, card needs the terminal reference, both print on receipt and Z-report; settling never needs internet; receipt by print (ESC/POS 58/80 mm over USB, LAN, Bluetooth), WhatsApp (consent) or none; missing printer never blocks; cash sale kicks drawer and logs it; End/Settle/Collect commit in one tap with Undo for 5–10 s, none once a receipt has printed; refund/void needs reason, manager PIN and original invoice, optionally returns stock, shows as negative line in shift report.
- **CAP-18**
  - **intent:** [MVP] A GST-registered branch can issue compliant tax invoices and credit notes; other branches use a simple tax rate.
  - **success:** GST per item, HSN, inclusive/exclusive per branch, GSTIN on receipt, B2B invoice with customer GSTIN; invoice numbers sequential per branch per financial year, never reused (including across standby promotion); tax computed per rate and rounded once; credit notes reference the original invoice; pack/pass sales carry own GST rate and report separately; a sample month reconciles to GSTR-1 export format.
- **CAP-19**
  - **intent:** [MVP] Staff can open and close shifts with honest cash variance and a one-tap Z-report.
  - **success:** Open with float bound to staff login; only recorded sales, deposits (with slip reference), expenses (with receipt photo) and refunds change expected cash; close by denomination or single total, expected cash hidden until counted is entered; variance = counted − expected, signed (+ over, − short) with breakdown; over threshold (default ₹100) needs manager PIN + reason (list + free text), audited, owner alerted; one-tap Z-report; closing cash becomes next float unless deposited; shift close < 10 s.
- **CAP-20**
  - **intent:** [MVP] Staff can keep one stock level per product per branch with stock-in, waste, expiry and low-stock control.
  - **success:** Product has name, category, price, tax rate, HSN, track-stock, image, barcode, kitchen flag; stock-in (qty, cost, free-text supplier, expiry) updates weighted average cost; waste by reason (spoiled, broken, free) and staff with report by value; expiry alert N days before, expired stock excluded from sale until written off; low-stock threshold alerts Hub and owner, clears on stock-in; stock decrements at served by default (configurable to at order); order cancelled after preparing lands in waste.
- **CAP-21**
  - **intent:** [v1.1] A manager can run stock counts, suppliers and purchase orders, a petty-cash ledger and staff attendance.
  - **success:** Count sheet with variance per item and value report; PO receipt creates stock-in rows; petty cash ledger; clock-in/out tied to shift with hours per staff in reports.

### Kitchen

- **CAP-22**
  - **intent:** [MVP] Kitchen staff on Basic and above receive orders as printed tickets and on a LAN display and advance them to served.
  - **success:** Kitchen-flagged items print a KOT ≤ 3 s (seat/device no., order id, time, items, modifiers), reprint from Hub; display works on any LAN tablet with New → Preparing → Ready → Served, chime + persistent NEW badge, Undo on advance; display survives LAN loss (advances sync on reconnect); only the counter cancels; printer offline shows a Hub badge while the display still receives; status reaches kiosk ≤ 5 s and the tile shows ORDER/READY per fulfilment setting (to seat or pick-up).
- **CAP-23**
  - **intent:** [v1.1] A branch can route kitchen items to prep stations and time them.
  - **success:** Separate ticket per station (bar vs kitchen); item-level timing recorded.

### Staff, configuration and reports

- **CAP-24**
  - **intent:** [MVP] Owners compose staff roles from a fixed permission catalogue and managers authorise restricted actions inline by PIN.
  - **success:** Default roles Owner, Manager, Counter, Kitchen, Kiosk-device plus custom roles; ~40 permissions grouped by module, allow/deny, one per action; owner-created user gets a single-use expiring setup code and the staff member sets own PIN and password at the Hub (owner never sees them); user changes reach branch within one poll; manager PIN entry never leaves the current screen and audit records both actors; Hub locks after 5 min idle (default), PIN unlock returns to same screen; Free capped at 3 users.
- **CAP-25**
  - **intent:** [MVP] Managers can read branch reports and the audit log on the Hub offline and export them.
  - **success:** Shift/Z, daily sales (by hour, group, category), sessions (count, minutes, revenue, utilisation per hour), players (top spenders, new vs returning, credit aged), stock (value, movements, waste, expiring), staff (sales, sessions, discounts, voids); CSV and PDF for every report; audit log records every change with actor, time, before/after, filterable by actor, module, date.
- **CAP-26**
  - **intent:** [MVP] An owner or manager can configure branch currency, tax, billing rules, receipts, hours and device preferences.
  - **success:** Currency (ISO 4217), symbol, number format, timezone; GST on/off, GSTIN, default rates, inclusive/exclusive, invoice prefix (branch code locks once first invoice exists); rounding, minimum, grace, time-up mode; printer, paper width, logo, footer, WhatsApp template; opening hours (feed passes, pricing, offline alerts); notification templates without provider keys; order fulfilment; kiosk default volume; HC per device; ads consent (Free); tz or day-start change takes effect next business day.

### Kiosk

- **CAP-27**
  - **intent:** [MVP] The Kiosk owns each gaming PC between sessions and blocks players from leaving the player experience.
  - **success:** One installer; replaces the Windows shell for the player account (Explorer, Task Manager, Run, Settings blocked; Ctrl+Alt+Del and Win keys limited); a player cannot reach the desktop in 30 min of trying; launch allow-list, block-list killed on sight with message logged to Hub, USB autorun off; admin exit by hidden combo + manager PIN (works with Hub down) or Hub remote unlock, logged; game booster closes listed apps and sets priority with launch time shown to staff; pen test passed before GA.
- **CAP-28**
  - **intent:** [MVP] A player can log in or create an account at the kiosk and pick a plan.
  - **success:** Username + password login < 3 s on LAN, or staff start from Hub; error never says which field failed; 3 failures lock login 60 s and alert Hub; sign-up (username, password, phone, name) needs no OTP and is blocked until the single required, unticked consent box (Terms, Privacy, service messages) is ticked; duplicates blocked by username and by phone; forgotten password reset by staff.
- **CAP-29**
  - **intent:** [MVP] A player gets a branded attract screen and a console-style shell for games, apps, shop and account.
  - **success:** Attract shows branch logo, muted game video, rates and active rule, no ads, rendered from cache; shell Home/Games/Apps/Shop (Shop hidden on Free); always-visible time chip; messages bell; call staff; volume resets to branch default at logout; PC settings saved to player profile and re-applied on any PC; video stops while a game runs; account page shows wallet, packs, order history, change password.
- **CAP-30**
  - **intent:** [MVP] A player always knows time left, gets warned, can extend, and never loses a game at time-up within grace.
  - **success:** HUD shows time and balance, never steals focus, hides in fullscreen-exclusive (taskbar flash + sound at 5 and 1 min); warnings at 10, 5, 1 min and low balance (configurable) with toast + sound, persisting until dismissed; Extend via Ctrl+Shift+E, account menu, toast or grace lock, priced live by Hub, reflected on floor ≤ 2 s, refusals (closing time, credit, balance) keep options open; at zero a grace countdown (default 2 min) runs over the still-visible game with Extend, no data loss; then branch mode applies: clean up or staff handles.
- **CAP-31**
  - **intent:** [MVP] A player on Basic and above can order food and drink from the seat.
  - **success:** Menu with images; stock checked live; out-of-stock greyed; bundle short of a component refused; order attaches to the running session or is paid by wallet; no session → "Pay at counter"; order reaches Hub POS and KOT ≤ 3 s; status toasts on HUD.
- **CAP-32**
  - **intent:** [MVP] A player can launch games from a curated library and the next player never sees their launcher accounts.
  - **success:** Hub holds the catalogue (name, executable, launcher, category, age rating), edited once for every kiosk; kiosk auto-scan of Steam, Epic, Riot and common folders proposes entries for bulk approval; one-click launch with booster, logged with player and time; art/video only from the global curated catalogue via Hub cache, else launcher cover/exe icon, else solid; at session end Steam, Epic, Riot, Discord and browser profiles are signed out and credentials cleared; hours per game per branch feed dashboards and a top-10 most-played carousel.
- **CAP-33**
  - **intent:** [v1.1] A branch can lend café-owned launcher accounts and collect players' game requests.
  - **success:** Vault injects login at launch, one account per concurrent seat, released at session end; requests appear on Hub with count per game in reports.

### Consoles

- **CAP-34**
  - **intent:** [MVP] Staff can run consoles as timed devices from the Hub.
  - **success:** Console registered with group and TV number, shown like a PC; same postpaid, pack and pass rules; timer and warnings on the tile; time-up raises a tile alert + sound until acknowledged; audit records who ended; optional 5-min WhatsApp/SMS only with consent.
- **CAP-35**
  - **intent:** [v1.1] A Pi per TV enforces console time and lets players request a console from their phone.
  - **success:** Pi Zero 2 W or Pi 4, BOM < ₹4,000 per TV; pairs by 6-digit code, visible ≤ 60 s; idle TV shows logo, TV number, rates and QR (no ads); QR on café Wi-Fi opens a Hub LAN page with no login, request raises tile REQUEST + sound with plan pre-selected, player pays at counter, staff start, TV switches to console ≤ 3 s, phone shows live timer only; at 5 and 1 min the TV shows a 5-s warning then switches back; time-up locks TV at once ("Time's up — see the counter."), no grace, tile alert until acknowledged, CEC standby or smart plug per branch; staff mark controllers Returned or Missing; power/input/message commands ≤ 5 s; Pi keeps resolved timings while Hub is unreachable.

### Owner app

- **CAP-36**
  - **intent:** [MVP] An owner on Basic and above can see cash, sessions and stock for every branch from a phone, export reports, and on Premium or with the Tunnel add-on watch a live floor.
  - **success:** Today per branch (open/closed, sessions live, revenue, cash in drawer, shift variance, staff on shift, last sync) loads < 2 s from the Hub-published summary; every figure shows data age within tier cadence (Basic ≤ 15 min, Premium ≤ 5 min) on 99% of opens; revenue, cash reconciliation, stock and credit reports match Hub Z-report totals exactly; offline branch shows "Branch offline" and ages, never broken; CSV/PDF export for every report; live view (Premium, or Basic + Tunnel add-on) is read-only and second-by-second, locked otherwise; Free shows subscription, pairing and support only.
- **CAP-37**
  - **intent:** [MVP] An owner is alerted to problems at any branch.
  - **success:** Web push (iOS only when installed) + WhatsApp, thresholds per tenant, for branch offline (no heartbeat/upload 30 min in opening hours), cash variance over threshold (branch, staff, amount, reason), low stock, credit overdue, large refund or discount; alerts listed under "Needs attention".
- **CAP-38**
  - **intent:** [MVP] An owner can change prices, plans, rules, products, staff and devices at one or all branches remotely and watch each change land.
  - **success:** Commands for prices/plans/rules, products/bundles, staff users/roles (one or all), revoke device/Hub, send credit reminder, backup now; preview shows target branches; status Queued on this device → Waiting for branch → Applied / Rejected (reason + Retry) / Expired after 7 days (Resend); applied exactly once; owner's change wins over a local edit, Hub toasts "updated by owner" and audits both; floor and attract show a new rate ≤ 1 min after apply; delivery p95 < 2 min when branch online; only valid, entitled commands can be sent.
- **CAP-39**
  - **intent:** [MVP] An owner can search a branch's change history from the cloud.
  - **success:** Filter by branch, actor, module; first search of a month shows "Preparing audit…" and returns first results < 10 s; failure shows Retry; player data appears as ids only.
- **CAP-40**
  - **intent:** [v1.1] An owner gets scheduled reports, deeper dashboards, team logins, tenant-wide pricing, refund approvals and device health.
  - **success:** Scheduled email/WhatsApp daily/weekly reports arrive by 8 am local; player cohorts (weekly chart), device utilisation heatmap, staff sorted by variance, branch comparison; additional owner logins read-only or full; rules pushed to all branches with branch override flagged and happy-hour automation; refunds over threshold wait for owner approval command and alert as pending; message to all kiosks; device health (last seen, kiosk version, GPU/RAM) with device-offline alert in opening hours.

### Resilience and data

- **CAP-41**
  - **intent:** [MVP] A branch keeps running with the internet down and catches the cloud up afterwards.
  - **success:** 8-hour WAN-outage soak with 20 kiosks loses zero transactions; Hub shows internet/cloud reachability and nothing else changes; cloud features queue and resume; paid Hubs upload change batches and summary at tier cadence with gaps repaired; kiosk with Hub unreachable continues sessions on last known time, enforces time-up locally, never locks the player out, disables login/orders/Extend with "Counter is reconnecting — try again shortly.", reconciles on reconnect.
- **CAP-42**
  - **intent:** [MVP] A paid branch can survive a Hub PC failure by promoting a LAN standby.
  - **success:** Standby paired as its own Hub; promotion by manager PIN inline completes < 10 s and kiosks reconnect by themselves; if it fails, force promote with PIN after "Data since the last sync may be lost" warning, audited, owner alerted; a returning old main locks itself ("This PC is no longer the main Hub", offers to become standby) and never serves kiosks; exactly one main Hub ever serves devices.
- **CAP-43**
  - **intent:** [v1.1] A Premium branch can keep its standby off-site.
  - **success:** Off-site standby follows and promotes over STUN/TURN with the same guarantees as CAP-42.
- **CAP-44**
  - **intent:** [MVP] An owner can back up and restore a branch, with tax records retained for 8 years.
  - **success:** One-tap full DB export to file/USB on every tier, 1 GB < 10 s; restore from file with confirmation; Free uploads nothing (upgrading uploads a full snapshot first); Basic 15-min batches with 1-year retention, Premium 5-min batches with 3-year retention; yearly GST archive kept 8 years on any paid tier; Hub header shows last upload, last summary, pending batches; owner "Backup now" command.
- **CAP-45**
  - **intent:** [v1.1] Hubs, kiosks and Pis update themselves safely.
  - **success:** Hub checks daily, installs on restart with rollback; staged rollout by percentage from control plane; kiosks and Pis update from the Hub's cache between sessions only; never during an open session; rollback tested each release.
- **CAP-46**
  - **intent:** [MVP] An owner can manage branches and per-branch subscriptions, and paid features follow payment automatically.
  - **success:** Branches created, renamed, tz set, pairing and standby codes issued; tiers Free ₹0, Basic ₹200, Premium ₹1,999 per branch per month shown GST-exclusive, yearly 10× monthly; add-ons +20 devices ₹50, Tunnel ₹200 (any paid tier, included in Premium; enables remote devices and live view), owner login ₹50; one Razorpay subscription per branch, autopay mandatory, card/eNACH for renewals over ₹15,000; mid-cycle changes prorated; invoices itemise GST; full refund within 7 days of first payment, no partial refunds, cancel keeps plan to period end then Free; PPP band (MY, SG, US) stored on the subscription; upgrade applies within one heartbeat; a lapse never ends live sessions or open orders and over-cap devices go read-only.
- **CAP-47**
  - **intent:** [MVP] The Free tier is funded by ads on the Android Hub only.
  - **success:** AdMob banner always visible but never over floor grid, cart or settle bar; interstitial only when staff return after idle lock, skippable after 5 s, never after settle or over an open cart/sheet; no adult, gambling or alcohol categories, owner can block more; consent view; upgrade removes ads within one heartbeat; lapsed branch resumes ads only after entitlement expires; no ad SDK in the Windows build.
- **CAP-48**
  - **intent:** [MVP] Players and owners receive service messages on WhatsApp/SMS without the branch holding provider keys.
  - **success:** Credit reminders, low balance, session ending, console 5-min alert and receipts sent by the cloud from Arena 360's Gupshup account (SMS fallback) only with stored sign-up consent; exactly-once; nothing sent for an undone action; queued while branch offline; templates versioned; no player sends on Free.
- **CAP-49**
  - **intent:** [MVP] A reseller or installer can apply, refer cafés and receive monthly commission.
  - **success:** Application (name, phone OTP, email, city, business, type, cafés served) shows only "Under review"; approval by WhatsApp with portal link (partners.arena360.in, separate login); KYC (PAN, bank, optional GSTIN) before first payout; referral link/code attributes the tenant, immutable after first payment; 10% of paid subscription revenue net of refunds each month active; payout by the 10th, ₹500 minimum rolls over, 2% TDS shown, GST-registered partners upload invoice or are held; portal shows referred cafés, accrued, pending, paid, TDS, invoice status and no tenant operational data.
- **CAP-50**
  - **intent:** [MVP] Arena 360 staff can curate the global game catalogue, run partners and refunds, and support branches without direct Hub access.
  - **success:** Admin app behind staff SSO; catalogue media flash-screened before publishing; partner approval, KYC review, payout marked paid after manual transfer; 7-day refunds issued; every admin action audited; support sees branch data only via the in-app diagnostics bundle (< 5 MB, no player PII, sent only on request) or audit search with a recorded reason.
- **CAP-51**
  - **intent:** [MVP] A prospective owner can learn, price and start Arena 360 from a public site.
  - **success:** Home (install, pair, play story), Product (Hub, Kiosk, Console, Owner with 90-s demo videos), Pricing (Cafés · Partners tabs, Monthly · Yearly toggle, add-ons, offline/cancellation/refund FAQ), Get started (name, phone, email, café, city, OTP → onboarding), Downloads (installers, Pi image, requirements, checksums), Help centre (setup, troubleshooting, printer and CEC compatibility), FAQ, Legal (terms, privacy, data processing, ad policy).
- **CAP-52**
  - **intent:** [v1.1] A prospective owner can try a demo Hub and compare against incumbents.
  - **success:** In-browser sandbox Hub with sample data; compare pages vs ggLeap, Smartlaunch, iCafe (offline, price, hardware, migration); status and changelog pages.
- **CAP-53**
  - **intent:** [MVP] An owner owns their data and players' personal data can be erased.
  - **success:** Full branch DB and all reports exportable any time on every tier, documented and tested; player erasure makes PII unrecoverable in Hub, standby, backups and snapshots and stays erased after restore or promote; consent stored per player with policy version and timestamp; logs carry no PII beyond player id.
- **CAP-54**
  - **intent:** [MVP] Every surface is usable by keyboard, at high contrast and without relying on colour or sound alone.
  - **success:** Targets ≥ 44 px everywhere incl. Android phone Hub; HC theme set per device on Kiosk, Hub, Kitchen, Pi, Owner; counter flow completes without a mouse (POS shortcuts); every state has text or glyph and every alert sound a persistent badge; web surfaces meet the screen-reader baseline (landmarks, labels, live regions); Owner and web offer light/dark/system; no hard-coded strings in release builds.

## Constraints

- Local-first: every Hub, Kiosk and Kitchen feature works with the WAN down; only the Hub primary writes branch data (no multi-master, no CRDT).
- The cloud keeps files, a command mailbox, the licence and an on-demand audit builder; no continuous mirror; owner figures come only from Hub-computed summary and rollup files.
- Owner and admin writes reach a branch only as commands the Hub pulls and applies.
- Kiosk and Windows Hub: Tauri 2 on WebView2, Windows 10+. Android Hub: Tauri 2 mobile, Android 9+, 3 GB RAM, 20 GB free. One shared web UI and design-token codebase.
- Free tier runs the Hub on Android only; Windows Hub, standby and Tunnel require a paid tier.
- English only at launch; strings externalised; numbers, dates and currency formatted by locale.
- Control-plane data stays in India (Mumbai).
- Razorpay is the only billing provider; sell only where Razorpay can bill (India, Malaysia, Singapore, US).
- No branch payment provider: UPI and card are recorded by reference; settling never depends on the internet.
- Hub is outbound-443 only; no inbound ports; branches hold no provider keys.
- Branch customisation is logo-only.
- No ad on any player screen (Kiosk, HUD, Pi TV, phone page); ads only on the Free Android Hub.
- Player messages are service-only, sent by the cloud, and require stored sign-up consent.
- Paid features come only from the signed licence; GST invoicing only for GST-registered Indian tenants; money in integer minor units.
- Hub promotion is manual only; exactly one main Hub serves devices.
- Kiosk holds no data: the kiosk asks, the Hub decides.
- No modal blocks a sale; money actions are one tap + Undo; manager PIN is entered inline.
- Kiosk, Hub, Kitchen display and Pi screens are dark-only plus high-contrast.
- Reference hardware 2015 i3 + HDD: Hub service idle < 50 MB, Hub UI < 250 MB, Kiosk < 80 MB, LAN action < 100 ms, daily rollup < 5 s (also on 3 GB Android).
- Consoles always have their own groups and packs, never shared with PCs.

## Non-goals

- Multi-warehouse or per-store inventory beyond one stock level per product per branch.
- Accounting ledgers or P&L; Tally/Zoho export.
- A player mobile app; players use the kiosk and QR pages.
- Game licensing or café-licensed game distribution; streaming, cloud gaming or remote play.
- Multi-master or CRDT sync inside a branch; automatic standby promotion.
- Staff payroll or HR.
- Arbitrary SQL reporting in the cloud; a continuous cloud mirror or per-branch cloud databases.
- Owner upload of game media or any media-matching UI.
- Ads on Kiosk attract or Pi idle screens (incl. the old "every 15 min on Free" slot); rewarded ads.
- Dynamic UPI QR or payment webhooks at the branch this phase.
- Windows 7 support.
- Light theme on Kiosk, Hub, Kitchen display or Pi.
- Gamification (coins, quests, prize vault, social feed); in-kiosk wallet top-up; kiosk language picker, social login or QR login.
- Player login on consoles; OTP on player sign-up; marketing consent or marketing messages to players.
- Partner branding on kiosks; live blur.
- Automated partner payout transfers; direct support access to a Hub.
- PRD wave-3 (v2) items: public online booking, peripheral tracking, demand-based pricing suggestions, central purchasing, payroll hints, streaming hooks, franchise roll-ups, marketplace listing, assisted migration.

## Success signal

- A new owner goes from sign-up to a first paid session in under 60 minutes without support (80% of branches), and counter staff start a session, sell a snack and close a shift in under 10 s each, offline.
- Over an 8-hour WAN outage with 20 kiosks the branch loses zero transactions, and owner dashboard figures stay within tier data age (Basic ≤ 15 min, Premium ≤ 5 min) on 99% of opens and match the Hub Z-report exactly.
- Business: 20% free-to-paid within 90 days; paid churn < 3%/month; median shift variance < ₹50 after 30 days; < 2% of branches raise a ticket per month; marginal infra < ₹10 per paid branch per month; Free ad fill 40% in year one.
