# Renumber map — Epics 1–5 (2026-10-23)

Old number → new number and title, for every story in Epics 1–5 that moved or was split. Stories not listed kept their number. Use this map to update references to Epic 1–5 stories in Epics 6–14.

## Epic 1

| Old | New | Title |
|---|---|---|
| 1.1 (split, part 1 of 3) | 1.1 | Monorepo workspace with pinned toolchains and CI gates |
| 1.1 (split, part 2 of 3) | 1.2 | App skeletons for every surface build in CI |
| 1.1 (split, part 3 of 3) | 1.3 | Generated protocol types and the message catalogue lint |
| 1.2 | 1.4 | Design tokens generated from DESIGN.md with a contrast gate |
| 1.3 | 1.5 | Shared base components meet the accessibility floor |
| 1.4 | 1.6 | Hub service starts as the single writer of its branch database |
| 1.5 | 1.7 | Every change goes through apply() into a gap-free change log, starting with a seeded branch |
| 1.6 | 1.8 | Idempotent intents, optimistic row versions and property tests on apply() |
| 1.7 | 1.9 | LAN protocol envelope, hello handshake and client classes |
| 1.8 | 1.10 | Hub app shell with left rail, header and floor placeholder on the seeded branch |
| 1.9 | 1.11 | Staff sign in to the Hub with roles and permissions |
| 1.10 | 1.12 | Branch locale settings — currency, number format and timezone |
| 1.11 | 1.13 | Inline manager PIN elevation |
| 1.12 | 1.14 | Hub idle lock with PIN unlock in place |
| 1.13 | 1.15 | Audit log viewer on the Hub |
| 1.14 | 1.16 | Kiosk privilege split — locked service and unprivileged UI |
| 1.15 | 1.17 | Kiosk shell replacement and Windows lockdown |
| 1.17 | 1.18 | Kiosk process allow/block list |
| 1.16 | 1.19 | Automated 30-minute kiosk escape suite on Windows 10 and 11 |
| 1.18 | 1.20 | Kiosk admin exit with manager PIN, offline verifier and Hub remote unlock |
| 1.19 | 1.21 | High-contrast theme as a per-device branch setting |
| 1.20 | 1.22 | The branch runs with the WAN down and shows which link is offline |

## Epic 2

| Old | New | Title |
|---|---|---|
| 2.1 (split, part 1 of 2) | 2.1 | Arena 360 cloud runs in India behind Cloudflare |
| 2.1 (split, part 2 of 2) | 2.2 | Cloud error envelope, local dev mode and single SQLite linkage |
| 2.2 | 2.3 | Control data is backed up nightly and the cloud is observable |
| 2.3 | 2.4 | Owner signs up with phone OTP |
| 2.4 | 2.5 | Owner sets a password and enrols TOTP with backup codes |
| 2.5 | 2.6 | Owner signs in on a new device with TOTP, backup code or SMS fallback |
| 2.6 | 2.7 | Resumable setup wizard and step 1 — café and first branch |
| 2.7 | 2.8 | Step 2 — Free tier issues a signed licence |
| 2.8 | 2.9 | Step 3 — single-use 8-character Hub pairing code |
| 2.9 | 2.10 | Hub installer pairs with the code and retries in place |
| 2.10 | 2.11 | Hub checks in by heartbeat and the wizard advances by itself |
| 2.11 | 2.12 | Hub verifies its licence against trusted time and enforces its limits |
| 2.12 | 2.13 | branch.bootstrap applies the onboarding profile at first pairing, with branch code lock |
| 2.13 | 2.14 | Owner and staff set their own PIN and password at the Hub with a one-time setup code |
| 2.14 | 2.15 | Owner changes reach the Hub through the command mailbox within one poll |
| 2.15 | 2.16 | Step 7 — invite staff by phone with a role and a one-time setup code |
| 2.16 | 2.17 | Kiosk finds the Hub on the LAN or by typed IP |
| 2.17 | 2.18 | Kiosk pairs with a 6-digit code from the Hub (or a manager PIN) and shows as Unassigned |
| 2.18 | 2.19 | Device cap and pairing rate limits at the Hub |
| 2.19 | 2.20 | Device groups with six swatches, held at the Hub |
| 2.20 | 2.21 | Register consoles, VR rigs and simulators manually |
| 2.21 | 2.22 | Step 4 — owner groups devices in the web wizard as they arrive |
| 2.22 | 2.23 | Floor zones and grid layout per branch |
| 2.23 | 2.24 | Device status and maintenance |
| 2.24 | 2.25 | Hourly postpaid rate per device group |
| 2.25 | 2.26 | Branch operating settings — opening hours, fulfilment, kiosk volume, backup location, ad consent |
| 2.26 | 2.27 | Printer detection and test print at the Hub |
| 2.27 | 2.28 | Wizard steps 5, 6 and 8 follow the Hub's setup progress |
| 2.28 | 2.29 | Step 9 — 2-minute test session locks a kiosk at zero |
| 2.29 | 2.30 | Owner manages branches — create, rename, timezone, new Hub pairing code |
| 2.30 | 2.31 | Step 10 — go-live checklist marks the branch live |

## Epic 3

| Old | New | Title |
|---|---|---|
| 3.20 | 3.19 | Collect credit with partial payments and Undo |
| 3.19 | 3.20 | Credit aging shown first, and auto-pause at 15+ days overdue |

## Epic 4

| Old | New | Title |
|---|---|---|
| 4.4 | 4.3 | The Android Hub pairs with the wizard code and keeps its secrets in the Android Keystore |
| 4.3 | 4.4 | Kiosks find and pair with an Android Hub on the LAN |

## Epic 5

| Old | New | Title |
|---|---|---|
| 5.29 | 5.27 | Reports screen with daily sales and sessions |
| 5.27 | 5.28 | Z-report at shift close |
| 5.28 | 5.29 | GSTR-1 export |
| 5.37 (split, part 1 of 2) | 5.37 | Paid Hubs keep their setup template in the cloud |
| 5.37 (split, part 2 of 2) | 5.38 | A second branch starts from the tenant's existing setup |
| 5.38 | 5.39 | POS and money proven under load, offline and on real hardware |

## Notes

- Swaps: 1.16 ↔ 1.17 (allow/block list now precedes the escape suite, shifted to 1.18/1.19 by the 1.1 split); 3.19 ↔ 3.20 (collect before aging); 4.3 ↔ 4.4 (Android pairing before kiosk discovery); the Reports screen (old 5.29) moved before the Z-report (old 5.27) and GSTR-1 export (old 5.28).
- Story 3.4 was retitled from "Collapsible zones that never hide an alert" to "Collapsible zones with per-state counts" (number unchanged); its alert-count criterion moved to Story 3.25.
- Old Story 1.1 references in later epics: the app skeletons (Kitchen, owner PWA, partner portal, admin, onboarding, Astro marketing) are now Story 1.2; crates and toolchain pins stay 1.1; type generation and the string lint are 1.3.
