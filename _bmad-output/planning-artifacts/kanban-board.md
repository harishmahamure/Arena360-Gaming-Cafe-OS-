---
generated: 2026-10-25
source_of_truth_for_status: _bmad-output/implementation-artifacts/sprint-status.yaml
source_of_truth_for_scope: _bmad-output/planning-artifacts/epics.md
regenerate: uv run _bmad-output/planning-artifacts/kanban/build_board.py
---

# Arena 360 Kanban board

Every story below is one of the 417 stories in `epics.md`, unchanged. This file only decides the order in which they are pulled. Status is read from `sprint-status.yaml` at generation time, so re-run the script after each landing.

## Pull rules

1. Lanes run in parallel. The global WIP limit is 4 implementers; at most one story in progress per lane, except CP which may hold two.
2. When a slot frees, walk the lanes in priority order (CP > HUB-A > KIOSK > HUB-B > OWNER > HUB-C > KITCHEN > ANDROID > WEB > PI) and pull the first story that is **pullable**: every earlier story in its lane is committed (`in-progress` counts, since story N+1 branches from N's WIP commit as the overlap rule allows), every cross-lane gate in **Waits on** is `review` or `done`, and the lane is under its WIP limit.
3. A story from Epics 12 to 14 (v1.1) is pulled only when no lane has a pullable MVP story.
4. Proof stories (titles containing "proven") close a lane's block and need everything before them in the lane landed.
5. Status is never written here. The build loop keeps updating `sprint-status.yaml` exactly as before; this file is regenerated from it.
6. Hub migrations stay contiguous: a Hub story that lands after a sibling renumbers its migration on rebase (rule from the loop state).

## Lane summary

| Lane | Scope | Stories | Landed | In progress | Backlog |
|---|---|---:|---:|---:|---:|
| CP | Control plane | 63 | 2 | 0 | 61 |
| HUB-A | Hub floor and sessions | 80 | 2 | 0 | 78 |
| KIOSK | Kiosk | 42 | 0 | 1 | 41 |
| HUB-B | Hub POS and money | 39 | 2 | 0 | 37 |
| OWNER | Owner app | 30 | 3 | 0 | 27 |
| HUB-C | Hub platform | 55 | 0 | 0 | 55 |
| KITCHEN | Kitchen | 14 | 1 | 0 | 13 |
| ANDROID | Android Hub | 21 | 0 | 0 | 21 |
| WEB | Marketing, admin and partner | 22 | 0 | 0 | 22 |
| PI | Console TVs | 17 | 0 | 0 | 17 |
| LANDED | Epic 1 and 2 stories already shipped and not re-listed | 34 | 34 | 0 | 0 |

## Pull next

Candidates in priority order. Take the top ones until the 4 implementer slots are full.

- **CP** 2.12 Hub verifies its licence against trusted time and enforces its limits (`2-12-hub-verifies-its-licence-against-trusted-time-and-enforces-i`)
- **HUB-A** 2.23 Floor zones and grid layout per branch (`2-23-floor-zones-and-grid-layout-per-branch`)
- **HUB-B** 5.3 Products with categories, barcodes, images and the kitchen flag (`5-3-products-with-categories-barcodes-images-and-the-kitchen-fla`)
- **ANDROID** 4.1 Android Hub app builds from the shared Hub code and checks the device spec on first run (`4-1-android-hub-app-builds-from-the-shared-hub-code-and-checks-t`)
- **WEB** 11.1 Marketing site shell — navigation, footer, light and dark themes and high contrast (`11-1-marketing-site-shell-navigation-footer-light-and-dark-themes`)

In flight now:

- **KIOSK** 1.19 Automated 30-minute kiosk escape suite on Windows 10 and 11

## CP: Control plane

Cloud API, auth, wizard (onboarding web), licence, mailbox, presign, rollup ingest, Razorpay, messaging adapter, admin and partner backends.

After 2.31 and before 6.1, finish Story 2.3's deferred observability ACs (Sentry and the OpenTelemetry/Grafana export), as the loop state records.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 2.10 | Hub installer pairs with the code and retries in place | MVP | review |  |
| 2 | 2.11 | Hub checks in by heartbeat and the wizard advances by itself | MVP | review |  |
| 3 | 2.12 | Hub verifies its licence against trusted time and enforces its limits | MVP | backlog |  |
| 4 | 2.13 | branch.bootstrap applies the onboarding profile at first pairing, with branch code lock | MVP | backlog |  |
| 5 | 2.14 | Owner and staff set their own PIN and password at the Hub with a one-time setup code | MVP | backlog |  |
| 6 | 2.15 | Owner changes reach the Hub through the command mailbox within one poll | MVP | backlog |  |
| 7 | 2.16 | Step 7 — invite staff by phone with a role and a one-time setup code | MVP | backlog |  |
| 8 | 2.19 | Device cap and pairing rate limits at the Hub | MVP | backlog |  |
| 9 | 2.22 | Step 4 — owner groups devices in the web wizard as they arrive | MVP | backlog |  |
| 10 | 2.28 | Wizard steps 5, 6 and 8 follow the Hub's setup progress | MVP | backlog | ~~2.20~~, ~~2.21~~, 2.23, 2.24, 2.25, 2.22 |
| 11 | 2.29 | Step 9 — 2-minute test session locks a kiosk at zero | MVP | backlog | 2.28, 3.9 |
| 12 | 2.30 | Owner manages branches — create, rename, timezone, new Hub pairing code | MVP | backlog |  |
| 13 | 2.31 | Step 10 — go-live checklist marks the branch live | MVP | backlog | 2.29, 2.30 |
| 14 | 6.1 | The control plane publishes the global game catalogue to Hubs | MVP | backlog |  |
| 15 | 8.1 | Presign issues batch and summary upload URLs under the branch's own R2 prefix | MVP | backlog | 2.31 |
| 16 | 8.5 | Rollup schema v1 and the daily rollup at close | MVP | backlog | 3.12, 5.11 |
| 17 | 8.7 | Rollups regenerate after a schema bump, and the cloud accepts only the current and previous schema | MVP | backlog | 8.5 |
| 18 | 8.9 | The owner read API serves the live summary and rollup ranges from files | MVP | backlog | 8.4, 8.6 |
| 19 | 8.18 | The cloud accepts owner command kinds, checks them, and fans "all branches" out | MVP | backlog | 2.15 |
| 20 | 8.20 | Commands are swept, expire after 7 days, and are fenced by epoch | MVP | backlog | 8.18 |
| 21 | 8.28 | The cloud sends player and owner messages through a swappable messaging adapter, exactly once | MVP | backlog |  |
| 22 | 8.3 | The heartbeat detects upload gaps and the Hub resends them | MVP | backlog | 8.2 |
| 23 | 8.35 | The cloud alerts the owner when a branch goes offline during opening hours | MVP | backlog | 8.4 |
| 24 | 8.25 | Step-up MFA for sensitive commands | MVP | backlog | 8.18 |
| 25 | 8.36 | The audit builder rebuilds a branch month on demand | MVP | backlog | 8.2 |
| 26 | 8.37 | The builder serves audit queries from a local cache | MVP | backlog | 8.36 |
| 27 | 9.3 | The cloud issues epoch counters by compare-and-set on the branch fence | MVP | backlog |  |
| 28 | 9.15 | Backups are kept for the plan's retention and GST records for 8 years | MVP | backlog | 8.8 |
| 29 | 10.1 | The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans | MVP | backlog |  |
| 30 | 10.2 | A branch subscription is created at a PPP band that is locked to it | MVP | backlog |  |
| 31 | 10.3 | Razorpay webhooks are verified, applied once, and issue the entitlement | MVP | backlog |  |
| 32 | 10.4 | Wizard step 2 takes payment through Razorpay Checkout | MVP | backlog | 10.3 |
| 33 | 10.5 | Upgrade links and Add branch reuse the same checkout | MVP | backlog |  |
| 34 | 10.6 | A paid licence reaches the Hub within one heartbeat and staff see the plan | MVP | backlog | 10.3, 2.12 |
| 35 | 10.7 | Every subscription charge gets a GST tax invoice from Arena 360 | MVP | backlog | 10.3 |
| 36 | 10.9 | Change tier and add-ons mid-cycle with proration | MVP | backlog | 10.3 |
| 37 | 10.10 | Switch billing cycle and payment method, with card or eNACH for renewals over ₹15,000 | MVP | backlog | 10.3 |
| 38 | 10.11 | Renewals that need approval go to grace, then lapse, on cloud time | MVP | backlog | 10.3 |
| 39 | 10.14 | Cancel keeps the plan to the end of the period, then the branch drops to Free | MVP | backlog | 10.11 |
| 40 | 10.15 | Full refund within 7 days of the first payment, issued through the ops path | MVP | backlog | 10.3 |
| 41 | 10.20 | Billing, tier changes and the Hub move proven end to end | MVP | backlog | 10.19, 10.15, 10.14 |
| 42 | 11.9 | Admin access through Cloudflare Access, with staff accounts and every admin action audited | MVP | backlog |  |
| 43 | 11.8 | Get started hands off to sign-up, and funnel events are counted without personal data | MVP | backlog | 11.1 |
| 44 | 11.11 | A reseller or installer applies with phone OTP and sees "Under review" | MVP | backlog |  |
| 45 | 11.13 | Staff approve or reject partners, and approval goes out on WhatsApp | MVP | backlog | 11.11, 8.28 |
| 46 | 11.14 | An approved partner submits KYC, stored encrypted and readable only by admin | MVP | backlog | 11.13 |
| 47 | 11.15 | A referral link or code is captured at owner sign-up and locked after the first payment | MVP | backlog | 10.3 |
| 48 | 11.17 | Commission accrues on paid revenue net of refunds | MVP | backlog | 11.15, 10.3 |
| 49 | 11.18 | The monthly payout job on the 1st writes each partner's statement with rollover, TDS and clawback | MVP | backlog | 11.17 |
| 50 | 11.19 | Payouts are held until KYC is verified and, for GST-registered partners, a GST invoice is attached | MVP | backlog | 11.18, 11.14 |
| 51 | 11.21 | Staff mark payouts paid after a manual bank transfer | MVP | backlog | 11.18 |
| 52 | 11.31 | Launch metrics are computed nightly and shown in the admin app | MVP | backlog | 11.8 |
| 53 | 13.29 | Rollup schema v2 carries tournament income, deposits, loyalty and bookings | v1.1 | backlog | 13.11, 13.8, 13.2 |
| 54 | 13.28 | Owner commands for loyalty, referral and booking rules and memberships | v1.1 | backlog | 8.18, 13.11, 13.14 |
| 55 | 14.17 | The cloud accepts rollup v3 and the owner API serves the new fields | v1.1 | backlog | 14.15, 14.16 |
| 56 | 14.24 | Scheduled reports are built from rollups and arrive by 8 am local | v1.1 | backlog | 14.17, 8.28 |
| 57 | 14.25 | The owner-login add-on can be bought for a branch | v1.1 | backlog | 10.9 |
| 58 | 14.26 | Invite a team member with a read-only or full role | v1.1 | backlog | 14.25 |
| 59 | 14.27 | An invited team member signs up with OTP, password and TOTP | v1.1 | backlog | 14.26 |
| 60 | 14.28 | Team roles are enforced on every owner read and command | v1.1 | backlog | 14.27 |
| 61 | 14.35 | The cloud relays WebRTC signalling and issues TURN credentials for Premium branches | v1.1 | backlog |  |
| 62 | 14.42 | A release registry with signed manifests and staged rollout by percentage | v1.1 | backlog |  |
| 63 | 14.53 | A public status page for the cloud | v1.1 | backlog | ~~2.3~~ |

## HUB-A: Hub floor and sessions

Windows Hub: devices, floor, players, sessions, wallet, credit, plans, pricing, catalogue match, bookings, tournaments, loyalty, vault, imports, back office.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 2.20 | Device groups with six swatches, held at the Hub | MVP | review | ~~2.18~~ |
| 2 | 2.21 | Register consoles, VR rigs and simulators manually | MVP | review |  |
| 3 | 2.23 | Floor zones and grid layout per branch | MVP | backlog |  |
| 4 | 2.24 | Device status and maintenance | MVP | backlog |  |
| 5 | 2.25 | Hourly postpaid rate per device group | MVP | backlog | ~~2.20~~ |
| 6 | 3.1 | Branch billing rules — rounding, minimum, grace and time-up mode | MVP | backlog |  |
| 7 | 3.2 | Floor view shows every device as a live tile grouped by zone | MVP | backlog |  |
| 8 | 3.3 | Floor view performance proven on reference hardware | MVP | backlog |  |
| 9 | 3.4 | Collapsible zones with per-state counts | MVP | backlog |  |
| 10 | 3.5 | Non-modal tile sheet with remote PC actions | MVP | backlog |  |
| 11 | 6.2 | The Hub caches the catalogue and media and serves them on the LAN | MVP | backlog | 6.1 |
| 12 | 6.3 | The branch game and app library at the Hub, applied to every kiosk | MVP | backlog | 6.2 |
| 13 | 6.5 | The Hub matches scanned games to the catalogue and the manager approves them in bulk | MVP | backlog | 6.3, 6.4 |
| 14 | 3.6 | Create a player profile with protected personal data and consent | MVP | backlog |  |
| 15 | 3.7 | Player lookup by phone, Guest, and the Players screen | MVP | backlog |  |
| 16 | 3.8 | Edit a player profile and reset their password | MVP | backlog |  |
| 17 | 3.9 | Start a postpaid session for a player or a guest | MVP | backlog | 3.7, 2.25, 3.1 |
| 18 | 3.11 | End a postpaid session and see the bill to the minute | MVP | backlog |  |
| 19 | 3.12 | Settle a bill into payment records with reference rules | MVP | backlog | 3.11 |
| 20 | 3.13 | Settle bar and payment split at the counter | MVP | backlog |  |
| 21 | 3.14 | Undo for End and Settle | MVP | backlog |  |
| 22 | 3.15 | Pause and resume a session with a reason | MVP | backlog |  |
| 23 | 3.16 | Wallet top-up and paying from the wallet | MVP | backlog |  |
| 24 | 3.17 | Convert a guest to a profile, including at settle, and list sessions | MVP | backlog |  |
| 25 | 3.18 | Enable credit for a player and settle the remainder on credit | MVP | backlog |  |
| 26 | 3.19 | Collect credit with partial payments and Undo | MVP | backlog |  |
| 27 | 3.20 | Credit aging shown first, and auto-pause at 15+ days overdue | MVP | backlog |  |
| 28 | 3.21 | Time packs, blocks and passes per device group | MVP | backlog |  |
| 29 | 3.22 | Sell a pack or pass at the counter and see owned plans on the profile | MVP | backlog | 3.21 |
| 30 | 3.23 | Session timer engine with time warnings | MVP | backlog |  |
| 31 | 3.24 | Start a prepaid session on a pack or the wallet | MVP | backlog | 3.22, 3.23 |
| 32 | 3.25 | Hub alerts with sound and a persistent badge | MVP | backlog |  |
| 33 | 3.26 | Time-up on PCs — grace, then clean up or staff handles | MVP | backlog |  |
| 34 | 3.27 | Pass and block sessions with fixed windows | MVP | backlog |  |
| 35 | 3.28 | Extend a session from the Hub, charged to wallet, pack or credit | MVP | backlog |  |
| 36 | 3.29 | Dynamic pricing rules by weekday hours and by date | MVP | backlog |  |
| 37 | 3.30 | The active rule prices new sessions and shows in the header | MVP | backlog |  |
| 38 | 3.31 | A rule change mid-session opens a new rate segment | MVP | backlog |  |
| 39 | 3.32 | Transfer a session to another device | MVP | backlog |  |
| 40 | 3.33 | Group sessions under one bill and one payer | MVP | backlog |  |
| 41 | 3.34 | Forced end with manager PIN and reason | MVP | backlog |  |
| 42 | 3.35 | Console sessions timed from the Hub, with warnings on the tile | MVP | backlog | ~~2.21~~ |
| 43 | 3.36 | Console time-up alert and controller check | MVP | backlog |  |
| 44 | 3.37 | Floor invariants, throughput and offline proof | MVP | backlog |  |
| 45 | 13.1 | Book a device or a group slot for a player | v1.1 | backlog | 3.9 |
| 46 | 13.2 | Take a booking deposit as a prepayment | v1.1 | backlog |  |
| 47 | 13.3 | A booked device shows Reserved 15 minutes before the slot | v1.1 | backlog |  |
| 48 | 13.4 | Seat the booking when the player arrives | v1.1 | backlog |  |
| 49 | 13.5 | No-show and late-cancellation rules release the slot | v1.1 | backlog |  |
| 50 | 13.6 | Deposits are kept or returned by the no-show and cancellation rules | v1.1 | backlog |  |
| 51 | 13.7 | Create a tournament that blocks devices for the event | v1.1 | backlog |  |
| 52 | 13.8 | Register entrants and post entry fees to tournament income | v1.1 | backlog |  |
| 53 | 13.9 | Generate a single-elimination bracket and record results | v1.1 | backlog |  |
| 54 | 13.10 | Tournament play on blocked devices, and releasing them early | v1.1 | backlog |  |
| 55 | 13.11 | Loyalty settings and points earned on settled bills | v1.1 | backlog | 3.12 |
| 56 | 13.12 | Redeem loyalty points as wallet credit | v1.1 | backlog |  |
| 57 | 13.13 | Referral bonus on a referred player's first paid session | v1.1 | backlog |  |
| 58 | 13.14 | Monthly membership plans — sell, start and renew | v1.1 | backlog |  |
| 59 | 13.15 | WhatsApp renewal reminder for memberships | v1.1 | backlog |  |
| 60 | 13.16 | Café-owned launcher accounts kept in an encrypted Hub vault | v1.1 | backlog | 9.2 |
| 61 | 13.18 | The Hub lends one vault account per seat and takes it back at session end | v1.1 | backlog | 13.16, 13.17 |
| 62 | 13.23 | Game request counts in Hub reports | v1.1 | backlog | 13.22 |
| 63 | 13.24 | Import framework — source detection, field mapping and a dry-run report | v1.1 | backlog |  |
| 64 | 13.25 | Import a ggLeap players export | v1.1 | backlog | 13.24 |
| 65 | 13.26 | Import a Smartlaunch export through the same mapping and dry-run | v1.1 | backlog | 13.24 |
| 66 | 13.27 | Migrated time balances and products from ggLeap and Smartlaunch | v1.1 | backlog | 13.25, 13.26, 5.3, 3.21 |
| 67 | 13.30 | Bookings, tournaments, loyalty and the vault proven against their invariants | v1.1 | backlog |  |
| 68 | 14.1 | Start a stock count and fill in the count sheet | v1.1 | backlog | 5.5 |
| 69 | 14.2 | Apply count adjustments with reasons and see variance by value | v1.1 | backlog |  |
| 70 | 14.3 | Keep supplier records and pick a supplier at stock-in | v1.1 | backlog | 5.5 |
| 71 | 14.4 | Raise a purchase order and send it to the supplier | v1.1 | backlog |  |
| 72 | 14.5 | Receive a delivery against a purchase order | v1.1 | backlog |  |
| 73 | 14.6 | Keep a petty-cash ledger separate from the drawer | v1.1 | backlog |  |
| 74 | 14.7 | Count petty cash and report its spending | v1.1 | backlog |  |
| 75 | 14.8 | Staff clock in and out, tied to their shift | v1.1 | backlog | 5.10 |
| 76 | 14.9 | Hours per staff in reports, with manager corrections | v1.1 | backlog |  |
| 77 | 14.29 | Refunds above the owner's threshold wait for approval at the Hub | v1.1 | backlog | 5.24 |
| 78 | 14.31 | Staff complete an approved refund at the counter | v1.1 | backlog | 14.30 |
| 79 | 14.33 | Tenant-wide pricing rules with branch overrides flagged | v1.1 | backlog | 8.19, 3.29 |
| 80 | 14.34 | Happy-hour automation across branches | v1.1 | backlog | 14.33 |

## KIOSK: Kiosk

Windows kiosk service and player UI.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 1.19 | Automated 30-minute kiosk escape suite on Windows 10 and 11 | MVP | in-progress |  |
| 2 | 3.10 | The PC unlocks to a session screen when a session starts | MVP | backlog | 3.9, 1.19 |
| 3 | 6.4 | Kiosks scan for installed games and launchers and report them to the Hub | MVP | backlog | ~~2.18~~ |
| 4 | 6.6 | Game media on the kiosk falls back from video to cover to solid | MVP | backlog | 6.2 |
| 5 | 6.7 | The attract screen shows branding, rates and the active rule, with no ads | MVP | backlog | 3.30 |
| 6 | 6.8 | A player logs in at the kiosk with username and password | MVP | backlog | 3.6, 3.9 |
| 7 | 6.9 | The plan picker starts a prepaid session from the kiosk | MVP | backlog | 3.24, 6.8 |
| 8 | 6.10 | A new player signs up at the kiosk with one consent box | MVP | backlog | 3.6, 6.8 |
| 9 | 6.11 | Session-end cleanup ends the player's programs and clears the clipboard, temp files and volume | MVP | backlog |  |
| 10 | 6.12 | Session-end cleanup signs out launchers, Discord and browsers | MVP | backlog |  |
| 11 | 6.13 | The player shell with top bar, pill nav and time chip | MVP | backlog | 6.8 |
| 12 | 6.14 | The overlay HUD shows time and balance without stealing focus | MVP | backlog | 6.13, 3.23 |
| 13 | 6.15 | Volume control and calling staff from the top bar | MVP | backlog |  |
| 14 | 6.16 | Staff messages reach the seat and stay in the messages bell | MVP | backlog | 3.5 |
| 15 | 6.17 | The account menu with Lock and Log out | MVP | backlog |  |
| 16 | 6.18 | "I'm done" ends a postpaid session from the HUD | MVP | backlog | 3.11 |
| 17 | 6.19 | Time warnings as HUD toasts, a chip warning, and a taskbar flash in fullscreen games | MVP | backlog | 3.23 |
| 18 | 6.20 | Per-branch warning marks and the low-balance warning | MVP | backlog | 6.19 |
| 19 | 6.21 | Extend from the kiosk with live prices | MVP | backlog | 3.28 |
| 20 | 6.22 | The grace lock with Extend, then the branch time-up mode at the kiosk | MVP | backlog | 3.26, 6.21 |
| 21 | 6.23 | The Games tab shows the library and launches a game in one click | MVP | backlog | 6.3, 6.5 |
| 22 | 6.24 | Filter and search the Games tab | MVP | backlog |  |
| 23 | 6.25 | The game booster speeds up launches and shows launch time to staff | MVP | backlog |  |
| 24 | 6.26 | Launcher cards open the player's own Steam, Epic or Riot account | MVP | backlog | 6.23 |
| 25 | 6.27 | Play hours per game and the "Most played here" carousel | MVP | backlog | 6.23 |
| 26 | 6.28 | The Apps tab | MVP | backlog |  |
| 27 | 6.29 | Home shows the featured game with a hero video and a featured row | MVP | backlog |  |
| 28 | 6.30 | The kiosk Shop shows the menu with live stock and popular items | MVP | backlog | 5.9 |
| 29 | 6.31 | Build a cart and add the order to the session bill from the seat | MVP | backlog | 5.12, 6.30 |
| 30 | 6.32 | Pay now from the wallet, Pay at counter, and unpaid order lines at the end of a prepaid session | MVP | backlog | 3.16, 6.31 |
| 31 | 6.33 | Order status reaches the HUD with delivery or pick-up copy | MVP | backlog | 7.3 |
| 32 | 6.34 | The kiosk Account page — details, wallet and packs, order history, change password | MVP | backlog | 3.16, 3.22 |
| 33 | 6.35 | PC settings saved to the player's profile | MVP | backlog |  |
| 34 | 6.36 | The kiosk keeps the session running while the Hub is unreachable | MVP | backlog | 3.23 |
| 35 | 6.37 | Kiosk screens reflow at every supported width, by keyboard and in high contrast | MVP | backlog |  |
| 36 | 6.38 | The player journey proven on reference hardware | MVP | backlog | 6.37 |
| 37 | 13.17 | A secure sign-in helper signs a café account into a launcher and reports sign-out at session end | v1.1 | backlog | 13.16 |
| 38 | 13.19 | Spike — prove a Steam sign-in method for café accounts | v1.1 | backlog | 13.17 |
| 39 | 13.20 | The Steam adapter signs a café account into Steam | v1.1 | backlog | 13.19 |
| 40 | 13.21 | Epic Games and Riot Client sign-in from the vault | v1.1 | backlog | 13.20 |
| 41 | 13.22 | Players request a game from the kiosk and the Hub collects the requests | v1.1 | backlog | 6.23 |
| 42 | 11.33 | An external penetration test of the kiosk lockdown passes before GA | MVP | backlog | 6.38 |

## HUB-B: Hub POS and money

Windows Hub: tax, products, stock, POS, shifts, receipts, printing, reports, CSV import, setup templates.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 5.1 | Tax settings — GST registration, rates, inclusive or exclusive, and simple tax outside India | MVP | review |  |
| 2 | 5.2 | Tax is computed once per rate on every bill, in minor units | MVP | review |  |
| 3 | 5.3 | Products with categories, barcodes, images and the kitchen flag | MVP | backlog |  |
| 4 | 5.4 | Bundles priced on their own | MVP | backlog |  |
| 5 | 5.5 | Stock-in with expiry lots and weighted average cost | MVP | backlog |  |
| 6 | 5.6 | Record waste with a reason | MVP | backlog |  |
| 7 | 5.7 | Expiry alerts, and expired stock held from sale until written off | MVP | backlog |  |
| 8 | 5.8 | Low-stock thresholds and alerts | MVP | backlog |  |
| 9 | 5.9 | POS screen — product grid, search or scan, and cart | MVP | backlog | 3.13, 5.3 |
| 10 | 5.10 | Open a shift with a confirmed float, and cash needs an open shift | MVP | backlog | 3.12 |
| 11 | 5.11 | Sell at the counter — a walk-up sale settles and takes stock | MVP | backlog | 5.9, 5.10 |
| 12 | 5.12 | Attach an order to a running session so it settles with the bill | MVP | backlog | 5.11, 3.9 |
| 13 | 5.13 | Take stock at served, mark items served, and cancel unserved lines into waste | MVP | backlog |  |
| 14 | 5.14 | Invoice numbers in an epoch-segmented series that never repeat | MVP | backlog |  |
| 15 | 5.15 | Receipt and notification settings | MVP | backlog |  |
| 16 | 5.16 | Receipt, tax invoice and payment receipt layouts for 58 and 80 mm | MVP | backlog | 5.14, ~~5.2~~ |
| 17 | 5.17 | Choose print or none at settle, and print through the side-effect outbox | MVP | backlog | 5.16, ~~2.27~~ |
| 18 | 5.18 | Cash drawer kicks on cash sales | MVP | backlog |  |
| 19 | 5.19 | WhatsApp receipt queued to the outbox | MVP | backlog | 5.15, 8.29 |
| 20 | 5.20 | B2B invoice with the customer's GSTIN | MVP | backlog |  |
| 21 | 5.21 | Coupons applied at settle | MVP | backlog |  |
| 22 | 5.22 | Manual discount with manager PIN and reason | MVP | backlog |  |
| 23 | 5.23 | Void an unsettled order with manager PIN | MVP | backlog |  |
| 24 | 5.24 | Refund a settled order with a credit note | MVP | backlog |  |
| 25 | 5.25 | Cash deposits and expenses | MVP | backlog |  |
| 26 | 5.26 | Close a shift with a blind count and a signed variance | MVP | backlog | 5.10, 5.25 |
| 27 | 5.27 | Reports screen with daily sales and sessions | MVP | backlog |  |
| 28 | 5.28 | Z-report at shift close | MVP | backlog | 5.26, 5.27 |
| 29 | 5.29 | GSTR-1 export | MVP | backlog |  |
| 30 | 5.30 | Players and staff reports | MVP | backlog | 3.17 |
| 31 | 5.31 | Stock reports with waste by reason | MVP | backlog |  |
| 32 | 5.32 | Export every Hub report to CSV and PDF | MVP | backlog |  |
| 33 | 5.33 | CSV import with dry-run, starting with products | MVP | backlog | 5.3 |
| 34 | 5.34 | Import players and plans by CSV | MVP | backlog | 5.33, 3.21 |
| 35 | 5.35 | Seed plans and products from a starter template | MVP | backlog |  |
| 36 | 5.36 | Export and import a branch setup file | MVP | backlog |  |
| 37 | 5.37 | Paid Hubs keep their setup template in the cloud | MVP | backlog | 5.36, 8.1 |
| 38 | 5.38 | A second branch starts from the tenant's existing setup | MVP | backlog | 5.37, 2.30 |
| 39 | 5.39 | POS and money proven under load, offline and on real hardware | MVP | backlog | 5.38 |

## OWNER: Owner app

Owner PWA: shell, dashboards, reports, commands, alerts, audit, live view, subscription, team.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 8.10 | The Owner app shell — navigation, themes, install and Free limits | MVP | review | ~~2.6~~ |
| 2 | 8.11 | The Owner app offers a high-contrast theme | MVP | review |  |
| 3 | 8.12 | On Free, the Owner app shows only Subscription, pairing and support | MVP | review |  |
| 4 | 8.14 | The Owner app stays in setup mode until a branch goes live, then opens its dashboard | MVP | backlog | 2.31 |
| 5 | 8.13 | Today shows every branch with its figures, cash variance and data age | MVP | backlog | 8.9 |
| 6 | 8.15 | Reports — revenue and cash reconciliation | MVP | backlog | 8.9 |
| 7 | 8.16 | Reports — stock | MVP | backlog | 8.9 |
| 8 | 8.17 | Owner reports export to CSV and PDF and match the Hub's Z-reports exactly | MVP | backlog | 8.15, 8.16, 5.28 |
| 9 | 8.21 | Preview the target branches and follow each command row to Applied | MVP | backlog | 8.18, 8.19 |
| 10 | 8.22 | Commands queue on the owner's device while it is offline | MVP | backlog | 8.21 |
| 11 | 8.23 | Manage → Pricing — plans and rates, pricing rules and coupons | MVP | backlog | 8.21, 3.29, 5.21 |
| 12 | 8.24 | Manage → Products and bundles | MVP | backlog | 8.21, 5.4 |
| 13 | 8.26 | Manage → Staff users and roles by command | MVP | backlog | 8.21, 2.16 |
| 14 | 8.27 | Manage → Devices and groups, and revoking a device | MVP | backlog | 8.21, 2.24 |
| 15 | 8.32 | Reports — credit, with a one-tap reminder | MVP | backlog | 8.31, 8.9 |
| 16 | 8.33 | Owner alerts by web push and WhatsApp, and the Alerts tab | MVP | backlog | 8.28, 8.34, 8.35 |
| 17 | 8.38 | Audit search in Reports with "Preparing audit…" | MVP | backlog | 8.37 |
| 18 | 8.40 | Live view — a read-only floor through the Tunnel | MVP | backlog | 8.39 |
| 19 | 8.42 | The owner's remote experience proven against its targets | MVP | backlog | 8.40, 8.33 |
| 20 | 9.8 | Owner replaces a failed Hub and gets a new pairing code | MVP | backlog | 9.3 |
| 21 | 9.10 | Owner releases the branch key to the replacement Hub | MVP | backlog | 9.9, 9.2 |
| 22 | 9.17 | Owner sees backup status and sends "Backup now" | MVP | backlog | 9.16 |
| 23 | 10.8 | Account → Subscription shows each branch's plan, invoices and payment method | MVP | backlog | 10.7, 10.10 |
| 24 | 14.18 | Reports → Players with new, returning and weekly retention cohorts | v1.1 | backlog | 14.17 |
| 25 | 14.19 | Reports → Devices with a utilisation heatmap and device health | v1.1 | backlog | 14.17, 14.14 |
| 26 | 14.20 | Reports → Staff sorted by variance, and branch comparison | v1.1 | backlog | 14.17 |
| 27 | 14.21 | Reports → Games shows play hours per game | v1.1 | backlog | 14.17 |
| 28 | 14.22 | The owner is alerted when a PC goes offline during opening hours | v1.1 | backlog | 14.14, 8.35 |
| 29 | 14.23 | Owners choose daily or weekly scheduled reports on WhatsApp | v1.1 | backlog | 14.24 |
| 30 | 14.30 | The owner approves or declines a pending refund from the Owner app | v1.1 | backlog | 14.29 |

## HUB-C: Hub platform

Windows Hub: cloud sync, outbox, alerts, Tunnel, backups, restore, standby and promote, diagnostics, updates, wasm demo.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 8.2 | The Hub uploads gap-free change-log batches at its tier cadence | MVP | backlog | 8.1 |
| 2 | 8.4 | The Hub publishes a branch summary every cadence | MVP | backlog | 8.2 |
| 3 | 8.6 | The daily rollup publishes at close and catches up missed days | MVP | backlog | 8.5 |
| 4 | 8.8 | The Hub uploads a daily snapshot after close | MVP | backlog | 8.2 |
| 5 | 8.19 | The Hub applies owner commands exactly once, and the owner's change wins | MVP | backlog | 8.18 |
| 6 | 8.29 | The Hub drains its notification outbox after the undo window | MVP | backlog | 8.28, 3.14 |
| 7 | 8.30 | Low-balance, session-ending and console 5-minute messages | MVP | backlog | 8.29, 3.23 |
| 8 | 8.31 | Credit reminders go out on cycle days, and Free shows an overdue list | MVP | backlog | 8.29, 3.20 |
| 9 | 8.34 | The Hub raises owner alerts for cash variance, low stock, overdue credit and large refunds or discounts | MVP | backlog | 8.29, 5.26, 5.8, 5.24 |
| 10 | 8.39 | A Cloudflare Tunnel for entitled branches, run by the Windows Hub | MVP | backlog | 2.12 |
| 11 | 8.41 | Remote kiosks connect through the Tunnel with their device token | MVP | backlog | 8.39 |
| 12 | 9.1 | Erase a player's personal data by crypto-shredding | MVP | backlog | 3.6 |
| 13 | 9.2 | Player keys are backed up and the branch key is escrowed | MVP | backlog | 9.1, 8.1 |
| 14 | 9.4 | One-tap full database export to a folder or USB drive | MVP | backlog | 5.32 |
| 15 | 9.5 | The Hub saves a daily export to the backup location | MVP | backlog | 9.4, ~~2.26~~ |
| 16 | 9.6 | The export format is documented and every report exports alongside it | MVP | backlog | 9.4 |
| 17 | 9.7 | Restore a branch from an export file as a new epoch | MVP | backlog | 9.4, 9.3 |
| 18 | 9.9 | A replacement Hub opens in "Restore this branch" and serves nothing until restored | MVP | backlog | 9.8 |
| 19 | 9.11 | Upgrading from Free uploads a full snapshot before the first batch | MVP | backlog | 8.8, 10.6 |
| 20 | 9.12 | Fetch a paid branch's cloud backup onto a replacement Hub | MVP | backlog | 9.11, 9.9 |
| 21 | 9.13 | Rebuild the branch from the fetched backup as a new epoch | MVP | backlog | 9.12, 9.7 |
| 22 | 9.14 | The Hub emits a yearly GST archive kept for 8 years | MVP | backlog | 5.29 |
| 23 | 9.16 | Cloud backup status in the Hub header and System screen | MVP | backlog | 8.8 |
| 24 | 9.18 | Owner issues a standby code and a second PC pairs as standby | MVP | backlog | 9.8 |
| 25 | 9.19 | The standby bootstraps from the main Hub and follows it live | MVP | backlog | 9.18 |
| 26 | 9.20 | The standby keeps its own catalogue, media and licence and shows its status | MVP | backlog | 9.19 |
| 27 | 9.21 | Promote the standby to main with a manager PIN | MVP | backlog | 9.20, 9.3 |
| 28 | 9.22 | Force promote after a data-loss warning | MVP | backlog | 9.21 |
| 29 | 9.23 | The new main Hub lists what was set aside after a promote or restore | MVP | backlog | 9.21, 9.13 |
| 30 | 9.24 | A returning old main Hub locks itself | MVP | backlog | 9.21 |
| 31 | 9.25 | Make the old main Hub the standby | MVP | backlog | 9.24 |
| 32 | 9.26 | Failover and split-brain proven on the test LAN | MVP | backlog | 9.25 |
| 33 | 9.27 | Restore drill and privacy proof | MVP | backlog | 9.13, 9.1 |
| 34 | 10.12 | A lapse or downgrade never ends a running session, and over-cap devices go read-only | MVP | backlog | 10.11, 2.19 |
| 35 | 10.13 | A lapse or downgrade keeps open orders, signed-in staff and the main Hub working | MVP | backlog | 10.12 |
| 36 | 11.28 | The Hub builds a diagnostics bundle with no personal data and sends it only when staff press Send | MVP | backlog | ~~2.11~~ |
| 37 | 11.29 | Kiosk logs are collected over the LAN into the diagnostics bundle | MVP | backlog | 11.28 |
| 38 | 14.13 | Kiosks and Pis report device health to the Hub | v1.1 | backlog | 12.2 |
| 39 | 14.14 | The Hub shows device health and records device outages | v1.1 | backlog | 14.13 |
| 40 | 14.32 | The owner sends a message to every kiosk of a branch | v1.1 | backlog | 8.19, 6.16 |
| 41 | 14.36 | Pair an off-site PC as the branch's off-site standby | v1.1 | backlog | 14.35, 9.18 |
| 42 | 14.37 | Two Hubs open an authenticated WebRTC data channel and replicate over it | v1.1 | backlog | 14.36 |
| 43 | 14.38 | An empty off-site standby bootstraps over the data channel in resumable chunks | v1.1 | backlog | 14.37 |
| 44 | 14.39 | Promote the off-site standby to main | v1.1 | backlog | 14.38, 9.21 |
| 45 | 14.40 | The café's devices reach a promoted off-site main through the Tunnel | v1.1 | backlog | 14.39, 8.41 |
| 46 | 14.41 | Off-site standby proven across NAT, relay and failover | v1.1 | backlog | 14.40 |
| 47 | 14.15 | Rollup schema v3 adds device utilisation, staff, kitchen, device and game-hours fields | v1.1 | backlog | 8.7, 13.29, 14.14 |
| 48 | 14.16 | Rollup schema v3 gains the player cohort block | v1.1 | backlog | 14.15 |
| 49 | 14.43 | The Windows Hub checks daily and installs on restart, never during an open session | v1.1 | backlog | 14.42 |
| 50 | 14.44 | The standby updates before the primary | v1.1 | backlog | 14.43, 9.19 |
| 51 | 14.45 | The Hub caches signed kiosk updates and kiosks install them between sessions | v1.1 | backlog | 14.43 |
| 52 | 14.47 | Upgrade safety is proven on every release | v1.1 | backlog | 14.45 |
| 53 | 14.48 | Spike — the Hub core compiles to WebAssembly with an in-memory SQLite | v1.1 | backlog |  |
| 54 | 14.49 | The Hub UI runs in a browser tab on the wasm core | v1.1 | backlog | 14.48 |
| 55 | 14.50 | The demo opens on a sample café with players coming and going | v1.1 | backlog | 14.49 |

## KITCHEN: Kitchen

Hub kitchen tickets, KOT printing and the kitchen display app.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 7.1 | Kitchen settings at the Hub, locked on Free | MVP | review | ~~2.26~~ |
| 2 | 7.2 | Kitchen notes on order lines from the POS and the kiosk cart | MVP | backlog | 5.12, 6.31 |
| 3 | 7.3 | Every order with kitchen items opens a kitchen ticket that the Hub can move | MVP | backlog | 7.2, 5.3 |
| 4 | 7.4 | The floor tile shows READY and the Hub chimes when food is ready | MVP | backlog | 7.3, 3.25 |
| 5 | 7.5 | A KOT prints within 3 s and can be reprinted from the Hub | MVP | backlog | 7.3, 5.17 |
| 6 | 7.6 | Pair a kitchen tablet by code as a scoped display | MVP | backlog | 7.3 |
| 7 | 7.7 | The kitchen board shows open tickets in four columns | MVP | backlog | 7.6 |
| 8 | 7.8 | One tap advances a ticket, with Undo | MVP | backlog | 7.7 |
| 9 | 7.9 | New tickets chime and hold a NEW badge, with a mute toggle | MVP | backlog | 7.7 |
| 10 | 7.10 | The board keeps working through LAN loss and syncs on reconnect | MVP | backlog | 7.8 |
| 11 | 7.11 | The kitchen flow proven on reference hardware | MVP | backlog | 7.10 |
| 12 | 14.10 | Prep stations route kitchen items into a ticket per station | v1.1 | backlog | 7.11 |
| 13 | 14.11 | Each station gets its own KOT printer and its own kitchen board | v1.1 | backlog | 14.10 |
| 14 | 14.12 | Item-level prep timing and the prep-time report | v1.1 | backlog | 14.10 |

## ANDROID: Android Hub

Tauri mobile Hub, ads, Free limits, Play rollout, migration to Windows.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 4.1 | Android Hub app builds from the shared Hub code and checks the device spec on first run | MVP | backlog | ~~1.22~~ |
| 2 | 4.2 | The Android Hub keeps serving while the screen is off, in Doze and after a reboot | MVP | backlog |  |
| 3 | 4.3 | The Android Hub pairs with the wizard code and keeps its secrets in the Android Keystore | MVP | backlog | ~~2.10~~, ~~2.11~~ |
| 4 | 4.4 | Kiosks find and pair with an Android Hub on the LAN | MVP | backlog | 4.3 |
| 5 | 4.5 | The Hub works on an Android phone and tablet by touch | MVP | backlog |  |
| 6 | 4.6 | The Android Hub prints over Bluetooth or LAN | MVP | backlog | 5.17 |
| 7 | 4.7 | The Android Hub saves backups to a folder the owner chooses | MVP | backlog | 9.5 |
| 8 | 4.9 | Free limits — 3 users, 20 devices and one branch — enforced and explained at the Hub | MVP | backlog | 2.12, 2.19 |
| 9 | 4.8 | A Free café goes from sign-up to go-live on Android with no help | MVP | backlog | 2.31, 4.4, 4.9 |
| 10 | 4.10 | In-house AdMob plugin and the ads gate, with no ad code in Rust or on Windows | MVP | backlog |  |
| 11 | 4.11 | An always-visible banner in a permanent slot that nothing covers | MVP | backlog |  |
| 12 | 4.12 | Safe ad categories, a per-branch content rating and the ads consent view | MVP | backlog |  |
| 13 | 4.13 | A standard interstitial only when staff come back from the idle lock | MVP | backlog |  |
| 14 | 4.14 | Ad requests and impressions are recorded without ids and sent with the heartbeat only | MVP | backlog | 4.10 |
| 15 | 4.15 | Upgrading removes ads within one heartbeat, and a lapse brings them back only after expiry | MVP | backlog | 10.6, 4.10 |
| 16 | 4.16 | The Android Hub ships through Google Play with a staged rollout that never interrupts a session | MVP | backlog | 4.8 |
| 17 | 4.17 | Android Hub proven on a 3 GB device with 20 kiosks, Doze and no WAN | MVP | backlog | 4.16 |
| 18 | 10.16 | Owner starts moving a Free Android Hub to a Windows Hub after upgrading | MVP | backlog | 10.6, 9.4 |
| 19 | 10.17 | The Android Hub hands over a final snapshot and steps down | MVP | backlog | 10.16 |
| 20 | 10.18 | The Windows Hub imports the snapshot as the next epoch | MVP | backlog | 10.17, 9.7 |
| 21 | 10.19 | Kiosks follow the new Windows Hub without pairing again | MVP | backlog | 10.18 |

## WEB: Marketing, admin and partner

Astro marketing site, admin app and partner portal front ends.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 11.1 | Marketing site shell — navigation, footer, light and dark themes and high contrast | MVP | backlog | ~~1.4~~ |
| 2 | 11.2 | Home tells the install, pair, play story | MVP | backlog |  |
| 3 | 11.3 | Pricing — Cafés tab with Monthly · Yearly, equal plan cards, add-ons and FAQ | MVP | backlog |  |
| 4 | 11.4 | Product pages for Hub, Kiosk, Console and Owner with demo videos | MVP | backlog |  |
| 5 | 11.5 | Downloads with installers, system requirements and checksums | MVP | backlog |  |
| 6 | 11.6 | Help centre with setup guides, troubleshooting and compatibility lists | MVP | backlog |  |
| 7 | 11.7 | FAQ and Legal pages | MVP | backlog |  |
| 8 | 11.10 | The admin app shell, the Audit viewer and Staff management | MVP | backlog | 11.9 |
| 9 | 11.12 | Pricing — Partners tab with terms, a worked payout example and Apply | MVP | backlog | 11.3 |
| 10 | 11.16 | Partner portal home — referral card, metrics and referred cafés | MVP | backlog | 11.17 |
| 11 | 11.20 | Partner portal Payouts and GST invoice upload | MVP | backlog | 11.18, 11.19 |
| 12 | 11.22 | Staff curate game records in the admin app | MVP | backlog | 11.10, 6.1 |
| 13 | 11.23 | Every catalogue video is flash-analysed before anyone can approve it | MVP | backlog | 11.22 |
| 14 | 11.24 | Staff upload game media and a second person approves screened videos | MVP | backlog | 11.23 |
| 15 | 11.25 | Staff approve or reject 7-day refunds in the admin app | MVP | backlog | 11.10, 10.15 |
| 16 | 11.26 | Staff grant and revoke comp licences in the admin app | MVP | backlog | 11.10, 10.3 |
| 17 | 11.27 | Support sees a tenant and branch summary, and searches audit only with a reason | MVP | backlog | 11.10, 8.9, 8.36 |
| 18 | 11.30 | Support requests and downloads diagnostics bundles from the admin app | MVP | backlog | 11.10, 11.28 |
| 19 | 11.32 | Partner money and admin access proven end to end | MVP | backlog | 11.21, 11.26 |
| 20 | 14.51 | The marketing Demo page with a guided tour | v1.1 | backlog | 14.50 |
| 21 | 14.52 | Compare pages against ggLeap, Smartlaunch and iCafe | v1.1 | backlog |  |
| 22 | 14.54 | A public changelog built from release notes | v1.1 | backlog | 14.42 |

## PI: Console TVs

Pi image and agent, TV control, QR self-start.

| # | Story | Title | Phase | Status | Waits on |
|---:|---|---|---|---|---|
| 1 | 12.1 | A signed, read-only Pi image is built in CI | v1.1 | backlog |  |
| 2 | 12.2 | The Pi boots into a TV screen, finds the Hub and recovers on its own | v1.1 | backlog | 12.1 |
| 3 | 12.3 | TV control adapters — HDMI-CEC, and a power-switch interface for smart plugs | v1.1 | backlog |  |
| 4 | 12.4 | Pair a Pi with a 6-digit code and see it on the floor within a minute | v1.1 | backlog | 12.2, ~~2.18~~ |
| 5 | 12.5 | Link a Pi to a console so the TV shows its TV number | v1.1 | backlog | 12.4, 3.35 |
| 6 | 12.6 | The idle TV shows branding, console rates and a "Scan to start" QR | v1.1 | backlog | 12.5, 3.30 |
| 7 | 12.7 | The phone start page requests a console plan with no login | v1.1 | backlog | 12.6, 3.24 |
| 8 | 12.8 | Staff confirm or refuse a QR request from the console tile | v1.1 | backlog | 12.7, 3.5 |
| 9 | 12.9 | The TV follows the console session — console input at start, idle at end | v1.1 | backlog | 12.8, 12.3 |
| 10 | 12.10 | The phone shows a live timer for its TV | v1.1 | backlog | 12.9 |
| 11 | 12.11 | TV warnings at 5 and 1 minute on the Pi input | v1.1 | backlog | 12.9, 3.36 |
| 12 | 12.12 | Time-up locks the TV with no grace, then the branch power action | v1.1 | backlog | 12.11 |
| 13 | 12.13 | Remote TV commands from the tile sheet — power, input and message | v1.1 | backlog | 12.3, 3.5 |
| 14 | 12.14 | The Pi enforces the session while the Hub is unreachable | v1.1 | backlog | 12.12 |
| 15 | 12.15 | CI drives a hardware lab rig of real Pis and TVs | v1.1 | backlog | 12.2 |
| 16 | 12.16 | Console TVs proven on the rig against their targets | v1.1 | backlog | 12.15, 12.14 |
| 17 | 14.46 | Pi agents update from the Hub's signed cache between console sessions | v1.1 | backlog | 14.45, 12.14 |

## LANDED: Epic 1 and 2 stories already shipped

| Story | Title | Status |
|---|---|---|
| 1.1 | Monorepo workspace with pinned toolchains and CI gates | review |
| 1.2 | App skeletons for every surface build in CI | review |
| 1.3 | Generated protocol types and the message catalogue lint | review |
| 1.4 | Design tokens generated from DESIGN.md with a contrast gate | review |
| 1.5 | Shared base components meet the accessibility floor | review |
| 1.6 | Hub service starts as the single writer of its branch database | review |
| 1.7 | Every change goes through apply() into a gap-free change log, starting with a seeded branch | review |
| 1.8 | Idempotent intents, optimistic row versions and property tests on apply() | review |
| 1.9 | LAN protocol envelope, hello handshake and client classes | review |
| 1.10 | Hub app shell with left rail, header and floor placeholder on the seeded branch | review |
| 1.11 | Staff sign in to the Hub with roles and permissions | review |
| 1.12 | Branch locale settings — currency, number format and timezone | review |
| 1.13 | Inline manager PIN elevation | review |
| 1.14 | Hub idle lock with PIN unlock in place | review |
| 1.15 | Audit log viewer on the Hub | review |
| 1.16 | Kiosk privilege split — locked service and unprivileged UI | review |
| 1.17 | Kiosk shell replacement and Windows lockdown | review |
| 1.18 | Kiosk process allow/block list | review |
| 1.20 | Kiosk admin exit with manager PIN, offline verifier and Hub remote unlock | review |
| 1.21 | High-contrast theme as a per-device branch setting | review |
| 1.22 | The branch runs with the WAN down and shows which link is offline | review |
| 2.1 | Arena 360 cloud runs in India behind Cloudflare | review |
| 2.2 | Cloud error envelope, local dev mode and single SQLite linkage | review |
| 2.3 | Control data is backed up nightly and the cloud is observable | review |
| 2.4 | Owner signs up with phone OTP | review |
| 2.5 | Owner sets a password and enrols TOTP with backup codes | review |
| 2.6 | Owner signs in on a new device with TOTP, backup code or SMS fallback | review |
| 2.7 | Resumable setup wizard and step 1 — café and first branch | review |
| 2.8 | Step 2 — Free tier issues a signed licence | review |
| 2.9 | Step 3 — single-use 8-character Hub pairing code | review |
| 2.17 | Kiosk finds the Hub on the LAN or by typed IP | review |
| 2.18 | Kiosk pairs with a 6-digit code from the Hub (or a manager PIN) and shows as Unassigned | review |
| 2.26 | Branch operating settings — opening hours, fulfilment, kiosk volume, backup location, ad consent | review |
| 2.27 | Printer detection and test print at the Hub | review |
