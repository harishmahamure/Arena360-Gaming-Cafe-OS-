# Reconcile — sources vs spines

Date: 2026-10-22 · Sources: `docs/Arena 360 - Product.md` (PRD), `docs/Arena360 — User Journeys.md` (UJ) · Spines: `DESIGN.md`, `EXPERIENCE.md` (EXP) · Canon: `.memlog.md`

Line numbers are source line numbers. "→" is the suggested spine home. Spines were not edited.

## 1. UX-relevant requirements with no home in the spines (46)

### Kiosk (player)
| # | Requirement | Source | → Spine section |
|---|---|---|---|
| 1 | Sign-up screen: username, password, phone, name; OTP optional per branch; consent text; duplicates blocked by username **and** phone. No component, state or error copy exists | PRD 272, 87; UJ 163 | EXP Component Patterns (`signup-form`), State Patterns (duplicate / validation), Key Flows J3 |
| 2 | Alternate login paths: QR from Hub, and staff starts from Hub (walk-in). IA names only username + password | PRD 271 | EXP IA Kiosk, Component Patterns `login-panel` |
| 3 | Login completes under 3 s on LAN (needs a pending state) | PRD 271 | EXP State Patterns |
| 4 | Consent view: WhatsApp/SMS opt-in stored per player; gates WhatsApp receipt and credit reminders | PRD 427; UJ 62, 163 | EXP Component Patterns (sign-up consent), Receipts & messages |
| 5 | Process control: a blocked launch shows a message to the player and logs to the Hub | PRD 261 | EXP State Patterns (Kiosk "blocked app"), Voice copy |
| 6 | Admin exit: manager PIN or Hub remote unlock drops to the desktop; logged | PRD 262 | EXP Component Patterns (`inline-pin` on kiosk), IA Kiosk |
| 7 | Kiosk first run: pairing code **or** auto-discover the Hub + manager PIN; paired within 60 s | PRD 259 | EXP Key Flows J1 step 6, IA Kiosk (setup screen) |
| 8 | Game booster: measured launch time shown to staff | PRD 264 | EXP Hub floor tile / device detail |
| 9 | Low-balance warning; warning timings are configurable | PRD 275 | EXP `hud-toast` |
| 10 | Extend refused past branch closing time; refused when credit limit hit (HUD copy) | PRD 102, 276; UJ 83 | EXP State Patterns (Refusal), `hud-toast` |
| 11 | Time-up: kill allow-listed processes only if configured, otherwise game left paused for staff | PRD 277; UJ 85 | EXP `lock-screen` (see error 3.1) |
| 12 | Forced end: the player sees a message on the kiosk | PRD 105 | EXP State Patterns (Kiosk), Voice |
| 13 | Order status on the HUD within 5 s; cart attached to a session shows "ordered" | PRD 150, 175 | EXP `hud-toast` / `hud-pill` expanded view |
| 14 | Pay an order immediately from the HUD by wallet | UJ 106 | EXP Key Flows J4, HUD expanded |
| 15 | Staff broadcast or per-seat message delivered as a toast within 2 s; the Hub compose UI is absent | PRD 279 | EXP `hud-toast`; Hub `tile-sheet` / remote actions |
| 16 | Expired packs greyed in the plan picker; wallet option shows balance and hourly rate | PRD 91; UJ 79 | EXP `card` |
| 17 | Game library fields (age rating, launcher); auto-scan proposals approved in bulk on the Hub | PRD 285-286 | EXP IA Hub Settings / Games catalogue |
| 18 | Game request (v1.1) | PRD 291 | EXP IA Kiosk Games |

### Hub (staff)
| # | Requirement | Source | → Spine section |
|---|---|---|---|
| 19 | Pause / resume with a reason; paused time not billed. Missing from `tile-sheet` actions and tile states | PRD 101 | EXP `tile-sheet`, Floor tile states |
| 20 | Group session: one bill, several devices, payer changeable before settlement | PRD 104 | EXP `tile-sheet`, Key Flows |
| 21 | Transfer: old device locks and new device unlocks within 5 s; rate changes across groups | PRD 103 | EXP `tile-sheet` |
| 22 | Forced end and manual discount: manager PIN + reason | PRD 105, 138 | EXP `inline-pin` usage list |
| 23 | Bookings (v1.1): device shows Reserved on the floor view 15 min before the slot; deposit; no-show rules. No booking screen | PRD 106 | EXP Floor tile states (Reserved timing), IA Hub |
| 24 | Tournaments (v1.1): block devices, bracket, entry fee | PRD 107 | EXP IA Hub |
| 25 | Guest → profile conversion at settlement; profile created in under 15 s | PRD 87-88; UJ 54 | EXP Key Flows J2 |
| 26 | Remote device actions (lock, unlock, message, restart, shutdown): result within 5 s or marked failed | PRD 128 | EXP State Patterns (pending/failed action) |
| 27 | Floor layout editor: drag devices, zones, group colours; unassigned kiosks | PRD 124-125, 195 | EXP IA Hub (Settings → Layout) |
| 28 | Date-rule conflicts flagged on save; coupons applied at settle and shown on the receipt | PRD 136-137 | EXP State Patterns (save conflict), Receipts |
| 29 | POS: favourites, barcode (1 scan + 1 tap), UPI dynamic QR screen with manual confirm offline flagged, refund/void with PIN + invoice reference | PRD 149-154 | EXP Component Patterns (POS, `upi-qr`), Key Flows |
| 30 | Inventory screens: stock-in, waste with reason, expiry alerts, low-stock alert on the Hub | PRD 160-165 | EXP IA Hub Inventory, State Patterns (alerts) |
| 31 | KOT ticket content: seat, items, modifiers, order id; reprint from the Hub | UJ 101 | EXP Receipts & messages |
| 32 | Shift sheet details: denomination counter, threshold default ₹100, reason list + free text, deposits/expenses with slip or photo, cash-drawer count, next float | PRD 182-185; UJ 118-127 | EXP Key Flows J5, Component Patterns `shift-sheet` |
| 33 | Z-report: one tap at shift close; printable layout (sales by type, method, variance, waste, expenses, refunds) | PRD 188, 232; UJ 126 | EXP Receipts & messages (print layout) |
| 34 | Receipt / GST invoice layout: sequential invoice no., GSTIN, HSN, coupon line, credit note for refunds, B2B customer GSTIN; print / WhatsApp / none | PRD 153, 22, 424 | EXP Receipts & messages; DESIGN (thermal layout) |
| 35 | Users / roles / permission catalogue screens | PRD 208-211 | EXP IA Hub Settings |
| 36 | Configuration screens incl. **Ads consent view** (slots cannot be disabled on Free), billing rules, printer, opening hours, notification templates | PRD 219-226 | EXP IA Hub Settings, Ads |
| 37 | Hub reports with CSV/PDF export | PRD 230-238 | EXP IA Hub Reports |
| 38 | Ops: backup export/restore with confirmation; **cloud backup status in the Hub header**; standby pairing + promote confirmation; audit log filters; "send diagnostics" | PRD 244-248, 436 | EXP `hub-header`, IA Hub System, Key Flows J8 |
| 39 | Tier limits reached (3 users on Free; 20/60/200 devices) need a state | PRD 451-452 | EXP Tier Gating |
| 40 | Credit enable on profile (limit, cycle), headroom shown, auto-pause at 15+ days unless disabled, collection receipt references settled transactions | UJ 159-167 | EXP Key Flows J7, Players |

### Consoles, Pi, owner, web
| # | Requirement | Source | → Spine section |
|---|---|---|---|
| 41 | MVP console as a timed device: timer and warnings on the floor tile, optional 5-min WhatsApp/SMS (consent), time-up alert on the Hub and staff walk over | PRD 302-305 | EXP Floor tile states, State Patterns |
| 42 | Pi (v1.1): pairing shows TV number; idle screen with QR + ad slot; phone QR start page (login, plan); warnings by briefly switching TV input at 5 and 1 min; time-up lock; controller-check prompt | PRD 311-319 | EXP IA Pi/QR, Key Flows (new) |
| 43 | Owner app content: Today fields (open/closed, live sessions, revenue, cash, staff, last sync), Cash recon variance highlighted, Credit "send reminder now", Audit, Exports, alert list, command list, MFA (TOTP, backup codes), Subscription, Branches, Team | PRD 331-375 | EXP IA Owner app, State Patterns |
| 44 | Owner home "needs attention" section for variance alerts | UJ 125 | EXP IA Owner Today |
| 45 | Marketing pages (Home three-step story, Pricing partner tab, Downloads, Help, Legal) and Get-started fields (name, phone, email, café name, city, OTP) | PRD 385-396 | EXP IA Marketing, Key Flows J1 |
| 46 | Onboarding step content: step 1 fields, step 4 devices arriving + drag into groups, step 8 test print, step 10 checklist; second-branch template reuse; CSV dry-run conflicts; rewarded ads (30 s, reward and cap set by the café, blocked categories) | PRD 402-415, 466, 474-476 | EXP Key Flows J1, Ads |

## 2. Qualitative ideas dropped (8)

1. "The kiosk only asks; the Hub decides" (UJ 69): the kiosk holds no data and every action is a request to the Hub (PRD 253). This principle should frame the kiosk's pending and refusal states.
2. "The Kiosk owns the PC between sessions and stays out of the way during one" (PRD 253): a natural kiosk posture line for Brand & Style.
3. Marketing promise "run your cafe offline, see it from anywhere, pay less than you do now" (PRD 379), and "onboarding ends with a test session, not a settings page".
4. Home hero with a floor-view illustration and the three-step story install, pair, play (PRD 385).
5. Counter staff pain is "software that needs training" (PRD 51) and player pain is "surprise end of session" (PRD 53). Neither appears as a learnability or no-surprise principle.
6. "Owners compose roles, they do not invent permissions" (PRD 202), and "inventory is deliberately simple" (PRD 143), as a simplicity posture for staff screens.
7. "Nothing in the day can change expected cash except a recorded sale, deposit, expense or refund" (UJ 129): this is the honesty rule behind the variance UI.
8. Pi: "CEC is best effort, the smart plug is the guarantee" (PRD 321), which shapes the console time-up messaging.

## 3. Spine vs source contradictions not backed by a memlog override (7)

| # | Spine | Source | Severity |
|---|---|---|---|
| 3.1 | EXP `lock-screen` (l.79) and J3 (l.193): grace, then lock, then **always** cleanup to attract | PRD 277 and UJ 85: allow-listed processes are killed only if configured; otherwise the game is left paused for staff | **Error** |
| 3.2 | EXP Ads (l.278) and Open Question 4: no kiosk attract-screen ads in Phase 1 | PRD 35 (decided), 270, 464, 473 and UJ 77: attract ads every 15 min on Free. Memlog l.30 records this only as an *assumption*, not an override | **Error** until confirmed |
| 3.3 | EXP `hub-header` (l.84): branch, rule, offline, clock, staff | PRD 245: cloud backup status visible in the Hub header (paid); PRD 248: internet **and** cloud reachability | Error (omission against an acceptance criterion) |
| 3.4 | EXP Upstream Updates Required (l.309-316) and Open Question 5 still list brand, login, language and platform as pending | Memlog l.54-55: already applied upstream | Stale |
| 3.5 | EXP Open Question 3 (UPI provider) still open | PRD 537 (second pass) resolves it: Razorpay only. PRD 152 and 426 still say Razorpay/PhonePe, so the conflict is inside the source | Stale (and the source needs cleanup) |
| 3.6 | EXP Foundation (l.22): Android Hub labelled "Free tier" | PRD 34 and 469: Free is Android-only, but nothing restricts paid tiers from using Android | Minor |
| 3.7 | EXP J3 step 6 and `time-chip`: the amber cue is on the chip dot | UJ 82: "the HUD turns amber at 10". `hud-pill` has no amber state | Minor |

Residual in the sources (not a spine error): PRD 7 still cites "Gamezone SaaS — Low-Level Architecture", and the UJ title is "Arena360" with no space. The UJ table lists 8 journeys but has no Journey 8 body. PRD 37 and 525 say player sign-up needs **no OTP**, while PRD 272 says "OTP optional per branch".

## 4. Is EXPERIENCE.md Open Question 5 resolvable?

**Yes, mostly.** The updated sources now state:
- Sign-up fields are username, password, phone and name, plus consent text (PRD 272).
- Duplicates are blocked by **both** username and phone; each is unique per branch (PRD 272, 87).
- "QR from Hub" and "staff starts from Hub" remain login paths (PRD 271).

Still open:
- **OTP.** PRD 272 says "optional per branch", but PRD 37 and 525 say there is no OTP for players. The source has to pick one.
- **QR from Hub.** The flow is unspecified: what the Hub shows, and whether the player scans on the kiosk or the kiosk displays a code.
- **Consent wording and scope.** Whether consent covers WhatsApp receipts and reminders (UJ 62, 163) is not stated.

Suggested resolution: close Open Question 5 with the three facts above and open a narrower question on OTP and the QR-login mechanics.
