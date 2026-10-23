# Renumber map — Epics 6–9 (2026-10-23)

Old number → new number. Stories not listed keep their number.

## Epic 6 — no change (38 stories, 6.1–6.38)

## Epic 7 — no change (11 stories, 7.1–7.11)

## Epic 8 — 40 → 42 stories

| Old | New | Title |
|---|---|---|
| 8.1–8.10 | 8.1–8.10 | unchanged |
| — | 8.11 | NEW: The Owner app offers a high-contrast theme (FR202) |
| 8.11 | 8.12 | On Free, the Owner app shows only Subscription, pairing and support |
| 8.12 | 8.13 | Today shows every branch with its figures, cash variance and data age |
| — | 8.14 | NEW: The Owner app stays in setup mode until a branch goes live, then opens its dashboard (FR11) |
| 8.13 | 8.15 | Reports — revenue and cash reconciliation |
| 8.14 | 8.16 | Reports — stock |
| 8.15 | 8.17 | Owner reports export to CSV and PDF and match the Hub's Z-reports exactly |
| 8.16 | 8.18 | The cloud accepts owner command kinds, checks them, and fans "all branches" out |
| 8.17 | 8.19 | The Hub applies owner commands exactly once, and the owner's change wins |
| 8.18 | 8.20 | Commands are swept, expire after 7 days, and are fenced by epoch |
| 8.19 | 8.21 | Preview the target branches and follow each command row to Applied |
| 8.20 | 8.22 | Commands queue on the owner's device while it is offline |
| 8.21 | 8.23 | Manage → Pricing — plans and rates, pricing rules and coupons |
| 8.22 | 8.24 | Manage → Products and bundles |
| 8.23 | 8.25 | Step-up MFA for sensitive commands |
| 8.24 | 8.26 | Manage → Staff users and roles by command |
| 8.25 | 8.27 | Manage → Devices and groups, and revoking a device |
| 8.26 | 8.28 | The cloud sends player and owner messages through a swappable messaging adapter, exactly once |
| 8.27 | 8.29 | The Hub drains its notification outbox after the undo window |
| 8.28 | 8.30 | Low-balance, session-ending and console 5-minute messages |
| 8.29 | 8.31 | Credit reminders go out on cycle days, and Free shows an overdue list |
| 8.30 | 8.32 | Reports — credit, with a one-tap reminder |
| 8.31 | 8.33 | Owner alerts by web push and WhatsApp, and the Alerts tab |
| 8.32 | 8.34 | The Hub raises owner alerts for cash variance, low stock, overdue credit and large refunds or discounts |
| 8.33 | 8.35 | The cloud alerts the owner when a branch goes offline during opening hours |
| 8.34 | 8.36 | The audit builder rebuilds a branch month on demand |
| 8.35 | 8.37 | The builder serves audit queries from a local cache |
| 8.36 | 8.38 | Audit search in Reports with "Preparing audit…" |
| 8.37 | 8.39 | A Cloudflare Tunnel for entitled branches, run by the Windows Hub |
| 8.38 | 8.40 | Live view — a read-only floor through the Tunnel |
| 8.39 | 8.41 | Remote kiosks connect through the Tunnel with their device token |
| 8.40 | 8.42 | The owner's remote experience proven against its targets |

## Epic 9 — 25 → 27 stories

| Old | New | Title |
|---|---|---|
| 9.1–9.7 | 9.1–9.7 | unchanged |
| 9.8 (split) | 9.8 | Owner replaces a failed Hub and gets a new pairing code (`hub.revoke`) |
| 9.8 (split) | 9.9 | A replacement Hub opens in "Restore this branch" and serves nothing until restored |
| 9.8 (split) | 9.10 | Owner releases the branch key to the replacement Hub (`backup.restore`) |
| 9.9 | 9.11 | Upgrading from Free uploads a full snapshot before the first batch |
| 9.10 | 9.12 | Fetch a paid branch's cloud backup onto a replacement Hub |
| 9.11 | 9.13 | Rebuild the branch from the fetched backup as a new epoch |
| 9.12 | 9.14 | The Hub emits a yearly GST archive kept for 8 years |
| 9.13 | 9.15 | Backups are kept for the plan's retention and GST records for 8 years |
| 9.14 | 9.16 | Cloud backup status in the Hub header and System screen |
| 9.15 | 9.17 | Owner sees backup status and sends "Backup now" |
| 9.16 | 9.18 | Owner issues a standby code and a second PC pairs as standby |
| 9.17 | 9.19 | The standby bootstraps from the main Hub and follows it live |
| 9.18 | 9.20 | The standby keeps its own catalogue, media and licence and shows its status |
| 9.19 | 9.21 | Promote the standby to main with a manager PIN |
| 9.20 | 9.22 | Force promote after a data-loss warning |
| 9.21 | 9.23 | The new main Hub lists what was set aside after a promote or restore |
| 9.22 | 9.24 | A returning old main Hub locks itself |
| 9.23 | 9.25 | Make the old main Hub the standby |
| 9.24 | 9.26 | Failover and split-brain proven on the test LAN |
| 9.25 | 9.27 | Restore drill and privacy proof |

References to old 9.8 should point to 9.8 (Hub revoke / pairing code), 9.9 (restore state) or 9.10 (key release) depending on what they cite; "KEK release reused" → 9.10.

## Numeric references in other epics that need remapping

- epic-11.md: line 5 and line 849 "Story 8.35" → 8.37
- epic-12.md: line 5 "Story 8.28" → 8.30, "Story 8.39" → 8.41; line 459 "Story 9.16" → 9.18, "Story 9.19" → 9.21
- epic-13.md: line 5 "Stories 8.16–8.17" → 8.18–8.19, "Story 8.13" → 8.15, "Story 8.19" → 8.21, "Story 8.21" → 8.23, "Stories 8.26–8.27" → 8.28–8.29, "9.17" → 9.19 (8.5–8.7, 9.1, 9.2, 9.4 unchanged); line 522 "Story 8.27" → 8.29; line 561 "Story 9.17" → 9.19; line 926 "Story 8.19" → 8.21; line 928 "Story 8.17" → 8.19; line 934 "Story 8.11" → 8.12; line 954 "Story 8.13" → 8.15; line 984 "Story 9.19" → 9.21
- epic-14.md: 575 "8.13" → 8.15; 599, 663, 745 "8.15" → 8.17; 697 "8.33" → 8.35; 713 "8.31" → 8.33; 965 "8.19" → 8.21; 1005 "8.32" → 8.34; 1071 "8.17" → 8.19; 1157, 1177 "9.16" → 9.18; 1195 "9.17" → 9.19; 1243 "9.18" → 9.20; 1263 "9.19" → 9.21; 1267 "9.20" → 9.22; 1275 "9.22" → 9.24; 1313 "8.39" → 8.41; 1339 "9.21" → 9.23 (8.4, 8.5, 8.7, 8.9 unchanged)
- epic-10.md line 571 "Story 9.4" and other refs to 8.1–8.10, 9.1–9.7: unchanged
