# Renumber map — Epics 10–14 (2026-10-23)

Old id → new id, in new order. "—" in Old marks a story added by this fix. All story references inside epic-10.md … epic-14.md were rewritten with this map; no file outside them was edited. No story in Epics 1–9 or the header cites an Epic 10–14 story number, so the map needs no edits elsewhere.

## Epic 10

| Old | New | Title (new) | Change |
| --- | --- | --- | --- |
| 10.1 | 10.1 | The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans | unchanged |
| 10.2 | 10.2 | A branch subscription is created at a PPP band that is locked to it | unchanged |
| 10.3 | 10.3 | Razorpay webhooks are verified, applied once, and issue the entitlement | unchanged |
| 10.4 | 10.4 | Wizard step 2 takes payment through Razorpay Checkout | unchanged |
| — | 10.5 | Upgrade links and Add branch reuse the same checkout | NEW (split from 10.4: Upgrade links and Add branch entry points) |
| 10.5 | 10.6 | A paid licence reaches the Hub within one heartbeat and staff see the plan | renumbered |
| 10.6 | 10.7 | Every subscription charge gets a GST tax invoice from Arena 360 | renumbered |
| 10.7 | 10.8 | Account → Subscription shows each branch's plan, invoices and payment method | renumbered |
| 10.8 | 10.9 | Change tier and add-ons mid-cycle with proration | renumbered |
| 10.9 | 10.10 | Switch billing cycle and payment method, with card or eNACH for renewals over ₹15,000 | renumbered |
| 10.10 | 10.11 | Renewals that need approval go to grace, then lapse, on cloud time | renumbered |
| 10.11 | 10.12 | A lapse or downgrade never ends a running session, and over-cap devices go read-only | renumbered |
| 10.12 | 10.13 | A lapse or downgrade keeps open orders, signed-in staff and the main Hub working | renumbered |
| 10.13 | 10.14 | Cancel keeps the plan to the end of the period, then the branch drops to Free | renumbered |
| 10.14 | 10.15 | Full refund within 7 days of the first payment, issued through the ops path | renumbered |
| 10.15 | 10.16 | Owner starts moving a Free Android Hub to a Windows Hub after upgrading | renumbered |
| 10.16 | 10.17 | The Android Hub hands over a final snapshot and steps down | renumbered |
| 10.17 | 10.18 | The Windows Hub imports the snapshot as the next epoch | renumbered |
| 10.18 | 10.19 | Kiosks follow the new Windows Hub without pairing again | renumbered |
| 10.19 | 10.20 | Billing, tier changes and the Hub move proven end to end | renumbered |

## Epic 11

| Old | New | Title (new) | Change |
| --- | --- | --- | --- |
| 11.1 | 11.1 | Marketing site shell — navigation, footer, light and dark themes and high contrast | unchanged |
| 11.2 | 11.2 | Home tells the install, pair, play story | unchanged |
| 11.3 | 11.3 | Pricing — Cafés tab with Monthly · Yearly, equal plan cards, add-ons and FAQ | unchanged |
| 11.4 | 11.4 | Product pages for Hub, Kiosk, Console and Owner with demo videos | unchanged |
| 11.5 | 11.5 | Downloads with installers, system requirements and checksums | unchanged |
| 11.6 | 11.6 | Help centre with setup guides, troubleshooting and compatibility lists | unchanged |
| 11.7 | 11.7 | FAQ and Legal pages | unchanged |
| 11.8 | 11.8 | Get started hands off to sign-up, and funnel events are counted without personal data | unchanged |
| 11.9 | 11.9 | Admin access through Cloudflare Access, with staff accounts and every admin action audited | unchanged |
| — | 11.10 | The admin app shell, the Audit viewer and Staff management | NEW (split from 11.9: admin app shell, Audit viewer, Staff management) |
| 11.10 | 11.11 | A reseller or installer applies with phone OTP and sees "Under review" | renumbered |
| 11.11 | 11.12 | Pricing — Partners tab with terms, a worked payout example and Apply | renumbered |
| 11.12 | 11.13 | Staff approve or reject partners, and approval goes out on WhatsApp | renumbered |
| 11.13 | 11.14 | An approved partner submits KYC, stored encrypted and readable only by admin | renumbered |
| 11.14 | 11.15 | A referral link or code is captured at owner sign-up and locked after the first payment | renumbered |
| 11.15 | 11.16 | Partner portal home — referral card, metrics and referred cafés | renumbered |
| 11.16 | 11.17 | Commission accrues on paid revenue net of refunds | renumbered |
| 11.17 | 11.18 | The monthly payout job on the 1st writes each partner's statement with rollover, TDS and clawback | renumbered |
| 11.18 | 11.19 | Payouts are held until KYC is verified and, for GST-registered partners, a GST invoice is attached | renumbered |
| 11.19 | 11.20 | Partner portal Payouts and GST invoice upload | renumbered |
| 11.20 | 11.21 | Staff mark payouts paid after a manual bank transfer | renumbered |
| 11.21 | 11.22 | Staff curate game records in the admin app | renumbered |
| 11.22 | 11.23 | Every catalogue video is flash-analysed before anyone can approve it | renumbered |
| 11.23 | 11.24 | Staff upload game media and a second person approves screened videos | renumbered |
| 11.24 | 11.25 | Staff approve or reject 7-day refunds in the admin app | renumbered |
| 11.25 | 11.26 | Staff grant and revoke comp licences in the admin app | renumbered |
| 11.26 | 11.27 | Support sees a tenant and branch summary, and searches audit only with a reason | renumbered |
| 11.27 | 11.28 | The Hub builds a diagnostics bundle with no personal data and sends it only when staff press Send | renumbered |
| 11.28 | 11.29 | Kiosk logs are collected over the LAN into the diagnostics bundle | renumbered |
| 11.29 | 11.30 | Support requests and downloads diagnostics bundles from the admin app | renumbered |
| 11.30 | 11.31 | Launch metrics are computed nightly and shown in the admin app | renumbered |
| 11.31 | 11.32 | Partner money and admin access proven end to end | renumbered |
| — | 11.33 | An external penetration test of the kiosk lockdown passes before GA | NEW (NFR12 GA gate: external kiosk-lockdown pen test) |

## Epic 12

| Old | New | Title (new) | Change |
| --- | --- | --- | --- |
| 12.1 | 12.1 | A signed, read-only Pi image is built in CI | unchanged |
| 12.2 | 12.2 | The Pi boots into a TV screen, finds the Hub and recovers on its own | unchanged |
| 12.5 | 12.3 | TV control adapters — HDMI-CEC, and a power-switch interface for smart plugs | moved before old 12.3/12.4 (CEC adapter first) |
| 12.3 | 12.4 | Pair a Pi with a 6-digit code and see it on the floor within a minute | renumbered |
| 12.4 | 12.5 | Link a Pi to a console so the TV shows its TV number | renumbered |
| 12.6 | 12.6 | The idle TV shows branding, console rates and a "Scan to start" QR | unchanged |
| 12.7 | 12.7 | The phone start page requests a console plan with no login | unchanged |
| 12.8 | 12.8 | Staff confirm or refuse a QR request from the console tile | unchanged |
| 12.9 | 12.9 | The TV follows the console session — console input at start, idle at end | unchanged |
| 12.10 | 12.10 | The phone shows a live timer for its TV | unchanged |
| 12.11 | 12.11 | TV warnings at 5 and 1 minute on the Pi input | unchanged |
| 12.12 | 12.12 | Time-up locks the TV with no grace, then the branch power action | unchanged |
| 12.13 | 12.13 | Remote TV commands from the tile sheet — power, input and message | unchanged |
| 12.14 | 12.14 | The Pi enforces the session while the Hub is unreachable | unchanged |
| 12.15 | 12.15 | CI drives a hardware lab rig of real Pis and TVs | unchanged |
| 12.16 | 12.16 | Console TVs proven on the rig against their targets | unchanged |

## Epic 13

| Old | New | Title (new) | Change |
| --- | --- | --- | --- |
| 13.1 | 13.1 | Book a device or a group slot for a player | unchanged |
| 13.2 | 13.2 | Take a booking deposit as a prepayment | unchanged |
| 13.3 | 13.3 | A booked device shows Reserved 15 minutes before the slot | unchanged |
| 13.4 | 13.4 | Seat the booking when the player arrives | unchanged |
| 13.5 | 13.5 | No-show and late-cancellation rules release the slot | unchanged |
| 13.6 | 13.6 | Deposits are kept or returned by the no-show and cancellation rules | unchanged |
| 13.7 | 13.7 | Create a tournament that blocks devices for the event | unchanged |
| 13.8 | 13.8 | Register entrants and post entry fees to tournament income | unchanged |
| 13.9 | 13.9 | Generate a single-elimination bracket and record results | unchanged |
| 13.10 | 13.10 | Tournament play on blocked devices, and releasing them early | unchanged |
| 13.11 | 13.11 | Loyalty settings and points earned on settled bills | unchanged |
| 13.12 | 13.12 | Redeem loyalty points as wallet credit | unchanged |
| 13.13 | 13.13 | Referral bonus on a referred player's first paid session | unchanged |
| 13.14 | 13.14 | Monthly membership plans — sell, start and renew | unchanged |
| 13.15 | 13.15 | WhatsApp renewal reminder for memberships | unchanged |
| 13.16 | 13.16 | Café-owned launcher accounts kept in an encrypted Hub vault | unchanged |
| 13.18 | 13.17 | A secure sign-in helper signs a café account into a launcher and reports sign-out at session end | swapped with 13.17 and reworked (helper now owns the kiosk sign-out report) |
| 13.17 | 13.18 | The Hub lends one vault account per seat and takes it back at session end | swapped with 13.18 and reworked (lease story now consumes the sign-out report) |
| — | 13.19 | Spike — prove a Steam sign-in method for café accounts | NEW (split from 13.19: Steam sign-in spike) |
| 13.19 | 13.20 | The Steam adapter signs a café account into Steam | renumbered |
| 13.20 | 13.21 | Epic Games and Riot Client sign-in from the vault | renumbered |
| 13.21 | 13.22 | Players request a game from the kiosk and the Hub collects the requests | renumbered |
| 13.22 | 13.23 | Game request counts in Hub reports | renumbered |
| 13.23 | 13.24 | Import framework — source detection, field mapping and a dry-run report | renumbered |
| 13.24 | 13.25 | Import a ggLeap players export | renumbered |
| 13.25 | 13.26 | Import a Smartlaunch export through the same mapping and dry-run | renumbered |
| 13.26 | 13.27 | Migrated time balances and products from ggLeap and Smartlaunch | renumbered |
| 13.27 | 13.28 | Owner commands for loyalty, referral and booking rules and memberships | renumbered |
| 13.28 | 13.29 | Rollup schema v2 carries tournament income, deposits, loyalty and bookings | renumbered |
| 13.29 | 13.30 | Bookings, tournaments, loyalty and the vault proven against their invariants | renumbered |

## Epic 14

| Old | New | Title (new) | Change |
| --- | --- | --- | --- |
| 14.1 | 14.1 | Start a stock count and fill in the count sheet | unchanged |
| 14.2 | 14.2 | Apply count adjustments with reasons and see variance by value | unchanged |
| 14.3 | 14.3 | Keep supplier records and pick a supplier at stock-in | unchanged |
| 14.4 | 14.4 | Raise a purchase order and send it to the supplier | unchanged |
| 14.5 | 14.5 | Receive a delivery against a purchase order | unchanged |
| 14.6 | 14.6 | Keep a petty-cash ledger separate from the drawer | unchanged |
| 14.7 | 14.7 | Count petty cash and report its spending | unchanged |
| 14.8 | 14.8 | Staff clock in and out, tied to their shift | unchanged |
| 14.9 | 14.9 | Hours per staff in reports, with manager corrections | unchanged |
| 14.10 | 14.10 | Prep stations route kitchen items into a ticket per station | unchanged |
| 14.11 | 14.11 | Each station gets its own KOT printer and its own kitchen board | unchanged |
| 14.12 | 14.12 | Item-level prep timing and the prep-time report | unchanged |
| 14.13 | 14.13 | Kiosks and Pis report device health to the Hub | unchanged |
| — | 14.14 | The Hub shows device health and records device outages | NEW (split from 14.13: Hub screens, summary devices block, device_outages) |
| 14.14 | 14.15 | Rollup schema v3 adds device utilisation, staff, kitchen, device and game-hours fields | renumbered |
| 14.15 | 14.16 | Rollup schema v3 gains the player cohort block | renumbered |
| 14.16 | 14.17 | The cloud accepts rollup v3 and the owner API serves the new fields | renumbered |
| 14.17 | 14.18 | Reports → Players with new, returning and weekly retention cohorts | renumbered |
| 14.18 | 14.19 | Reports → Devices with a utilisation heatmap and device health | renumbered |
| 14.19 | 14.20 | Reports → Staff sorted by variance, and branch comparison | renumbered |
| — | 14.21 | Reports → Games shows play hours per game | NEW (FR137: Reports → Games, play hours per game) |
| 14.20 | 14.22 | The owner is alerted when a PC goes offline during opening hours | renumbered |
| 14.21 | 14.23 | Owners choose daily or weekly scheduled reports on WhatsApp | renumbered |
| 14.22 | 14.24 | Scheduled reports are built from rollups and arrive by 8 am local | renumbered |
| 14.23 | 14.25 | The owner-login add-on can be bought for a branch | renumbered |
| 14.24 | 14.26 | Invite a team member with a read-only or full role | renumbered |
| 14.25 | 14.27 | An invited team member signs up with OTP, password and TOTP | renumbered |
| 14.26 | 14.28 | Team roles are enforced on every owner read and command | renumbered |
| 14.27 | 14.29 | Refunds above the owner's threshold wait for approval at the Hub | renumbered |
| 14.28 | 14.30 | The owner approves or declines a pending refund from the Owner app | renumbered |
| 14.29 | 14.31 | Staff complete an approved refund at the counter | renumbered |
| 14.30 | 14.32 | The owner sends a message to every kiosk of a branch | renumbered |
| 14.31 | 14.33 | Tenant-wide pricing rules with branch overrides flagged | renumbered |
| 14.32 | 14.34 | Happy-hour automation across branches | renumbered |
| 14.33 | 14.35 | The cloud relays WebRTC signalling and issues TURN credentials for Premium branches | renumbered |
| 14.34 | 14.36 | Pair an off-site PC as the branch's off-site standby | renumbered |
| 14.35 | 14.37 | Two Hubs open an authenticated WebRTC data channel and replicate over it | renumbered |
| 14.36 | 14.38 | An empty off-site standby bootstraps over the data channel in resumable chunks | renumbered |
| 14.37 | 14.39 | Promote the off-site standby to main | renumbered |
| 14.38 | 14.40 | The café's devices reach a promoted off-site main through the Tunnel | renumbered |
| 14.39 | 14.41 | Off-site standby proven across NAT, relay and failover | renumbered |
| 14.40 | 14.42 | A release registry with signed manifests and staged rollout by percentage | renumbered |
| 14.41 | 14.43 | The Windows Hub checks daily and installs on restart, never during an open session | renumbered |
| 14.42 | 14.44 | The standby updates before the primary | renumbered |
| 14.43 | 14.45 | The Hub caches signed kiosk updates and kiosks install them between sessions | renumbered |
| 14.44 | 14.46 | Pi agents update from the Hub's signed cache between console sessions | renumbered |
| 14.45 | 14.47 | Upgrade safety is proven on every release | renumbered |
| 14.46 | 14.48 | Spike — the Hub core compiles to WebAssembly with an in-memory SQLite | renumbered |
| 14.47 | 14.49 | The Hub UI runs in a browser tab on the wasm core | renumbered |
| 14.48 | 14.50 | The demo opens on a sample café with players coming and going | renumbered |
| 14.49 | 14.51 | The marketing Demo page with a guided tour | renumbered |
| 14.50 | 14.52 | Compare pages against ggLeap, Smartlaunch and iCafe | renumbered |
| 14.51 | 14.53 | A public status page for the cloud | renumbered |
| 14.52 | 14.54 | A public changelog built from release notes | renumbered |
