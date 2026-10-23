# Re-validation — Epics 1–9 (2026-10-23)

Source: _bmad-output/planning-artifacts/epics.md, reassembled after the fixes. This was a read-only check; epics.md was not edited.
Inputs: epic-drafts/validation-e1-5.md (33 findings) and validation-e6-9.md (26 findings). The 7.1/8.11/8.38 tier-lock finding was checked in two parts, Epic 7 and Epic 8, so there are 60 checks in all. Old story numbers were mapped to new ones through epic-fix/renumber-e1-5.md and renumber-e6-9.md.

## Summary
- Original findings: 60 checks. RESOLVED 60, OPEN 0.
- Fresh forward-dependency and table-creation scan of Epics 1–9: 17 new issues (3 M, 14 L).
- Stale story-number references inside Epics 1–9: none.

## New issues (all ranges)
| Story | Sev | Issue | Suggested fix |
|---|---|---|---|
| 2.31 | M | "Open dashboard" opens the Owner app dashboard, which is built in Epic 8 (8.10/8.14). Epic 2 depends only on Epic 1. | End 2.31 on the done screen with an Account → Branches link, and let 8.14 switch the action to "Open dashboard". |
| 6.2 | M | The AC about a standby, promoted or restored Hub depends on Epic 9 (9.9–9.13, 9.19–9.22). | Keep the testable cache rules in 6.2 and move that clause to 9.20. |
| 8.20 | M | "Hub self-locks per Story 2.11", but 2.11 has no self-lock. Demotion comes in 9.3 and the lock screen in 9.24. | Cite the Story 1.6 demotion (NOT_PRIMARY) and say the self-lock screen arrives in Epic 9, or test it with a fixture. |
| 3.28 | L | Extend prices at "the resolved rate", but the resolver arrives in 3.29 and is wired into extend in 3.30. | Price at the group's active rate in 3.28. |
| 3.23 | L | The timer engine relies on prepaid `ends_at` sessions (3.24) and the `grace` state (3.26). | State that 3.23 is tested with fixture sessions, and that 3.26 changes the `ends_at` action to grace. |
| 2.18/2.20/2.21 | L | The `devices_meta.group_id` FK points to `device_groups` (2.20), and the 2.21 console TV number has no column. | 2.20 adds the FK; 2.21's migration adds `tv_number`. |
| 4.14 | L | Uses plugin event `ad.requested{slot}`, which is missing from the Story 4.10 event contract. | Add it to the 4.10 list, or have 4.14 extend the plugin. |
| 5.13/5.23 | L | Served-mode stock is held back from sale, but no storage is named for the held amount. | Name it in 5.13 (e.g. `stock.reserved_qty`, or a flag on `order_lines`). |
| 6.36 | L | "(or a promoted standby takes over)" points to the Epic 9 promote. | Drop it or mark it "(Epic 9)". |
| 6.10 | L | `player.create` "creates … credit balance", but 3.18 now creates that row only on first enable. | Drop "credit balance". |
| 7.4 | L | The POS "open-tickets strip" is used (also in 7.5), but no story creates it. | Say 7.4 adds the strip. |
| 7.7 | L | Assumes a high-contrast toggle on the kitchen-display entry. Story 1.21's per-device setting doesn't cover `kitchen_displays`. | Add the toggle and its column in 7.6, or have 7.7 add it. |
| 8.2 | L | The fixture sets `lease` fork fields that no story creates. 9.23 assumes `fork_parent_epoch`/`fork_parent_seq` exist. | 8.2's migration adds the nullable fork columns. |
| 9.9 | L | Pending key requests (read, approved, expired and replay-checked in 9.10) have no table. | Name a gz_control `kek_requests` table in 9.9. |
| 9.8 | L | Refuses a "Manager-level owner app user", but Owner app team logins arrive in Epic 14. | Drop the clause (keep MFA_REQUIRED), or use a fixture non-owner session. |
| 8.14 | L | Filters Reports, Alerts and Manage to live branches before those screens exist (8.15/8.23/8.33). | Put the filter in a shared branch picker in 8.14 that later screens use. |
| 9.4/9.18/9.20 | L | The Backup and Standby panels are used but never created. Names differ: "Hub → System → Backup" vs "Settings → System"; "Settings → Standby" vs "System → Standby". | 9.4 creates the Backup panel under Settings → System; 9.18 creates the Standby panel with one name. |

## Per-range detail

### Re-validation — Epics 1–3 (epics.md lines 1187–4285)

#### Original findings (validation-e1-5.md, Epics 1–3)

1.5 → 1.7 | RESOLVED | TTL/dedupe is an explicit pass-through stub in 1.7; 1.8 fills it.
1.7 → 1.9 | RESOLVED | 1.9 adds the `device.revoke` intent (dev-fixture `arena-hub dev revoke-device` in Epic 1); Epic 1 notes agree.
1.16 → 1.19 | RESOLVED | Allow/block list is now 1.18, before the escape suite 1.19.
1.18 → 1.20 | RESOLVED | 1.20 pushes the verifier on Hub start and kiosk connect (dev-fixture PINs); 2.14 adds the push on PIN set/rotate.
1.17 → 1.18 (b) | RESOLVED | Storage named: settings keys `kiosk.process_allow` / `kiosk.process_block`, seeded by 1.18's migration.
1.1 (c) → 1.1/1.2/1.3 | RESOLVED | Split into workspace + pins CI (1.1), app skeletons (1.2), type generation + string lint (1.3).
2.1 (c) → 2.1/2.2 | RESOLVED | Split into cloud deploy + secrets (2.1) and error envelope + local dev + linkage spike (2.2).
2.3 → 2.4 | RESOLVED | Correct OTP lands on an account-setup placeholder route that 2.5 fills.
2.4 → 2.5 | RESOLVED | SMS-fallback toggle now appears only in 2.6 (Account → Security).
2.11 → 2.12 | RESOLVED | 2.12 adds its own header `status-badge` slot; 3.25 migrates both conditions into the alert list and removes the slot.
2.14 (c) → 2.15 | RESOLVED | Epic 2 notes and 2.15 AC agree: setup kinds only; `branch.bootstrap` travels in the /v1/pair response (2.13), not the mailbox.
2.15 → 2.16 | RESOLVED | 2.16 builds the `tier-lock` component (gallery, themes, "Available soon" until 10.5) before 2.19/2.21/2.22/2.30 use it.
2.17/2.23 (b) → 2.18/2.24 | RESOLVED | 2.18 creates `devices_meta` and the `device.register` intent; 2.24 adds `state` by migration with backfill.
3.4 | RESOLVED | Retitled "Collapsible zones with per-state counts"; alert-count AC moved to 3.25.
3.5 | RESOLVED | PC_BUSY on Restart/Shut down moved to 3.9 (after the `sessions` table exists).
3.6 (b) | RESOLVED | 3.6 creates only `players`, `consents`, `player_keys`, `phone_lookup`; `wallets` created in 3.16 (with backfill), `credit_balances` in 3.18 (row on first enable).
3.7 | RESOLVED | 3.7 scoped to create; 3.8 updates the index on edit; erasure filter handed to 9.1, which does add `players.erased_at` and the filter.
3.19 → 3.20 | RESOLVED | Collect is now 3.19; aging + auto-pause + reopen-on-collection is 3.20 and cites 3.19.

#### New issues

NEW: 2.31 | M | "Open dashboard … opens the Owner app dashboard with the branch shown as Live" depends on the Owner app Today/dashboard, delivered in Epic 8 (8.13/8.14). Epic 2 depends only on Epic 1. | Have 2.31 end on the done screen with a link to Account → Branches (showing Live) and let 8.14 switch the primary action to "Open dashboard".
NEW: 3.28 | L | Extend shows "live prices at the resolved rate", but `pricing::resolve` is built in 3.29 and wired into extend in 3.30. | Say 3.28 prices at the group's active rate; 3.30 already states start/extend use the resolver.
NEW: 3.23 | L | Timer engine is specified on prepaid `ends_at` sessions and a heap of active/paused/grace sessions, but prepaid starts arrive in 3.24 and `grace` in 3.26. Testable only through the injected-clock fixture. | Note that 3.23 is tested with fixture sessions (postpaid `closing_at` is the only real producer until 3.24), and that 3.26 changes the `ends_at` action from end to grace.
NEW: 2.18 / 2.20 / 2.21 | L | `devices_meta` (2.18) has a `group_id` column referencing `device_groups`, which 2.20 creates; and 2.21 stores a console TV number with no column in the 2.18 schema and no migration named. | State that 2.20 adds the `group_id` FK when it creates `device_groups`, and that 2.21's migration adds `tv_number` (unique per branch) to `devices_meta`.

Stale numeric cross-references in Epics 1–3: none found (all story refs match the renumber map, including 10.5, 8.14 back-ref to 2.31, and 9.1).

#### Counts
Original findings (Epics 1–3): 18 — RESOLVED 18 · OPEN 0
New issues: 4 (1 M, 3 L)

### Re-validation — Epics 4–5 (epics.md lines 4286–6107)

#### Original findings (validation-e1-5.md)

4.3 (H, Android pairing) → 4.3/4.4 | RESOLVED | New 4.3 is Android pairing (Keystore, /v1/pair); new 4.4 kiosk discovery opens "Given an Android Hub paired through Story 4.3".
4.3 (L, Tunnel/standby "Needs the Windows Hub") → 4.4 | RESOLVED | AC removed from 4.4; Epic 4 notes now say the later Tunnel/standby stories own the Android state (ownership in Epics 8/9 not checked here).
4.6 → 4.6 | RESOLVED | Failure recorded on the printer task / printer status and setup status; no `side_effect_outbox` reference; alert uses Story 3.25.
4.13 → 4.13 | RESOLVED | Alert example is now "TIME UP" (Story 3.26) or the Story 3.25 fixture alert kind.
5.3 → 5.3 | RESOLVED | `stock` table now created by 5.5 at first stock-in; 5.3 treats tracked products as qty 0 until then.
5.7 → 5.7 | RESOLVED | POS-grid clause dropped; only `catalog.changed` availability with reason `expired`.
5.10 → 5.10 | RESOLVED | Prefill is last shift's opening float (₹0 first); carry-over minus deposit moved to 5.26; cash rule lists flows generically.
5.13 → 5.13 | RESOLVED | Only "cancel is not offered for served lines".
5.17 → 5.17 | RESOLVED | Reprint offered from POS "Recent sales" (defined in 5.11), the Sessions screen (Epic 3) or the open order.
5.22 → 5.22 | RESOLVED | Shift-report clause removed.
5.23 → 5.23 | RESOLVED | Report/shift-report AC removed; voids handled in 5.27/5.28/5.30.
5.27 (Z-report) → 5.28 | RESOLVED | Reports screen is now 5.27 (before); 5.28 adds the Shifts entry to it.
5.28 (GSTR-1) → 5.29 | RESOLVED | 5.29 adds the GST entry to the Story 5.27 Reports screen.
5.30 → 5.30 | RESOLVED | Uses "a fixture player whose data key is absent; erasure itself lands in Epic 9".
5.37 (size) → 5.37/5.38 | RESOLVED | Split into cloud template upload (5.37) and wizard "Start from <branch>" + bootstrap apply (5.38).

Counts: 15 RESOLVED, 0 OPEN.

#### Fresh scan — new issues only

Cross-references to Epic 1–5 stories were checked against the renumber map (1.1, 1.6, 1.8, 1.9, 1.14, 2.8–2.13, 2.17–2.19, 2.26–2.28, 2.30, 3.5, 3.10, 3.11, 3.13, 3.14, 3.19, 3.21, 3.22, 3.25, 3.26, 4.3, 4.5, 4.17, 5.x internal): all match. No stale numbers found. No new forward dependency on a later story or epic without stub/fixture wording (4.9's "Story 10.5 un-hides it" is a hidden-link stub; 5.14 uses a fixture epoch change).

NEW: 4.14 | Relies on a plugin event `ad.requested{slot}`, but the Story 4.10 plugin contract lists only `ad.loaded`, `ad.failed`, `ad.shown{slot}`, `ad.clicked`, `ad.dismissed` (L–M: contract gap, not a forward dependency) | Add `ad.requested{slot}` to the 4.10 event list, or have 4.14 extend the plugin with it explicitly.
NEW: 5.13 / 5.23 | Stock "reserved (excluded from available stock)" in Served mode, and released by void, with no table or column named for the reservation (L, table creation) | Name the storage in 5.13 (for example `stock.reserved_qty`, or a reservation flag on `order_lines` summed into availability).

### Re-validation — Epics 6–7 (epics.md lines 6108–7925)

#### Original findings (validation-e6-9.md)

finding | status | reason
--- | --- | ---
6.6 game-running signal / Home | RESOLVED | `gameRunning` is now a prop driven by a fixture, and 6.23 wires the real source. The cache AC is tested with two fixture views, and Home is no longer named.
6.8 plan step before 6.9 | RESOLVED | 6.8 now shows a "Logged in as <name>" holding step with its own 60 s timeout. 6.9 replaces it with the plan picker.
6.11 "later player end" | RESOLVED | It lists only the existing paths (staff End, time-up clean-up, forced end) and runs on the generic session-end event.
6.17/6.34/6.35 account-menu rows | RESOLVED | 6.18 adds "I'm done", 6.21 adds Extend, 6.34 adds Account and 6.35 adds PC settings. Each says "the account menu gains …".
6.20 Extend key caps | RESOLVED | The low-balance toast shows only the Ctrl+Shift+E (expand HUD) key caps. 6.21 adds Extend to it.
6.30/6.31 Basic+ via comp entitlement | RESOLVED | The Epic 6 and Epic 7 notes now say this, citing the renumbered Story 2.8.
6.32 open unpaid standalone order | RESOLVED | 6.32 itself adds the `open_unpaid` state, the `order.place{settle: counter}` path, the POS "Open orders" list, settlement through 5.11 and the proptest invariant. Epic 5 has no such capability, so it rightly lives here.
6.1 admin_audit | RESOLVED | 6.1 creates the append-only `admin_audit` in `gz_control` (with a trigger and a test). The Epic 6, 8 and 9 notes and 8.x/9.x stories cite Story 6.1.
Epic 7 notes, wrong Epic 6 story title | RESOLVED | The notes now cite "Build a cart and add the order to the session bill from the seat" and "Pay now from the wallet, Pay at counter, …".
7.1 TIER_LOCKED untestable | RESOLVED | 7.1 builds the gate and tests it with its own settings keys plus a test-only fixture intent. 7.2, 7.3, 7.5 and 7.6 each register their intents and have a Free-refusal AC.
7.1 tier-lock Upgrade link (Epic 7 part) | RESOLVED | The 7.1 `tier-lock` shows "Available on Basic" with no Upgrade link until Epic 10, and the Epic 7 notes say this. The 8.12/8.40 part is left to the E8–9 fork; the Epic 8 notes now state the same placeholder rule.

#### New issues (fresh scan, Epics 6–7)

NEW: 6.2 | M | The AC "Given a standby Hub, or a Hub promoted or restored … keeps or refetches its own reference cache" depends on standby, promote and restore from Epic 9 (9.9–9.13, 9.19–9.22). Epic 6 depends only on Epics 1–3 and 5, so this AC cannot be tested in Epic 6. | Keep the testable part in 6.2: the cache is outside the change log, batches and snapshots, and it refetches from the manifest when empty. Move the standby/promote/restore clause to 9.20 ("keeps its own catalogue, media"), which already owns it.
NEW: 6.36 | L | The AC "(or a promoted standby takes over)" refers to the Epic 9 promote. | Drop the parenthetical, or add "(Epic 9)". Epic 9's notes already reuse 6.36's reconnect-and-reconcile.
NEW: 6.10 | L | It says `player.create` creates "the player, wallet, credit balance and a granted consent row". After the fix, 3.18 creates the `credit_balances` row only on first enable, and 3.16 makes `player.create` create the wallet. | Change it to "the player, wallet and a granted consent row (as Stories 3.6/3.16)", and drop "credit balance".
NEW: 7.4 | L | "the POS screen's open-tickets strip shows K-2043 READY" treats the strip as existing, but no earlier story creates it (7.5 also cites it). | Say that 7.4 adds the POS open-tickets strip for walk-up "Counter" tickets.
NEW: 7.7 | L | "display set to high contrast from its Hub list entry (per device, Story 1.21)" assumes 7.6's kitchen-display list has an HC toggle. Kitchen displays live in `kitchen_displays`, not `devices`, so the Story 1.21 per-device setting doesn't cover them. | Add an HC toggle (and its storage column) to the 7.6 kitchen-display list entry, or state in 7.7 that it adds one.

Stale numeric references: none found. Every Epic 1–5 citation in Epics 6–7 matches the renumbered titles: 1.2, 1.8, 1.9, 1.18, 1.21, 2.8, 2.12, 2.19, 2.25, 2.26, 2.27, 3.1, 3.5, 3.6, 3.9–3.11, 3.14, 3.16, 3.18, 3.22–3.28, 3.25, 3.30, 3.34, 5.2 and 5.11. Epics 6–7 cite no Epic 8–9 story numbers.

Table creation: library_entries (6.3), kiosk_scans (6.4), library_installs (6.5), game_plays (6.27), player_pc_settings (6.35), the 6.1 gz_control tables, kitchen_tickets and kitchen_ticket_lines (7.3), kitchen_displays (7.6) and order_lines.kitchen_notes (7.2) are each created by their first-use story. Apart from the 6.10 note above, no new problems were found.

Counts: 11 original findings checked — 11 RESOLVED, 0 OPEN. New issues: 5 (1 M, 4 L).

### Revalidation — Epics 8–9 (epics.md lines 7926–10229)

#### Original findings (validation-e6-9.md, Epic 8/9 items)

7.1-also-8.11/8.38 tier-lock Upgrade link (8.11→8.12, 8.38→8.40) | RESOLVED | Epic 8 notes + 8.10 AC: every Owner-app tier-lock shows "Available on …" with no Upgrade link until Epic 10; 8.12 and 8.40 repeat "no Upgrade link until Epic 10".
8.10/8.11 Subscription and Support (8.10/8.11→8.10/8.12) | RESOLVED | 8.10 defines a read-only Subscription placeholder (tier from licence, "Plan changes are available soon") and a Support mailto placeholder; 8.12 cites both.
8.17 command kinds (8.17→8.19) | RESOLVED | 8.19 is scoped to rate/plan/rule/coupon/product/backup.now; 8.18 says later stories add kinds with handlers; 8.26 (staff/role), 8.27 (group/device), 8.32 (credit.remind) and 8.34 (threshold settings.set) each add theirs.
8.19 preview UI before Manage (8.19→8.21) | RESOLVED | 8.21 is tested with a fixture rate.upsert, and 8.23 is named as the first real consumer.
8.27 erasure in drain (8.27→8.29) | RESOLVED | 8.29 has an extensible per-row eligibility check; 9.1 adds the erasure rule.
8.18/8.2 fence/fork header (8.18→8.20, 8.2) | RESOLVED | 8.2 fork-header AC is fixture-driven ("real epoch change … arrives in Epic 9"); 8.20 uses a fixture CAS; 9.3 replaces it with real fork_seq.
8.29 Players → Overdue (8.29→8.31) | RESOLVED | 8.31 says it adds a new read-only Players → Overdue view.
8.26/8.31 tables (8.26→8.28, 8.31→8.33) | RESOLVED | send_ledger and message_templates (8.28) and push_subscriptions (8.33) are marked as new tables with columns.
8.34/8.35 admin app (8.34→8.36, 8.35→8.37) | RESOLVED | 8.37 defines admin-scoped endpoints behind Access with a required reason, writing to admin_audit (6.1); the UI is deferred to Epic 11.
8.37 "Windows Hub only" add-on display (8.37→8.39) | RESOLVED | 8.39 records tunnel: unsupported_platform; 8.40 shows "Needs the Windows Hub" on Live view/Today card; the Hub Settings → System line shows it too; no Subscription dependency.
9.1 erased_at / phone_lookup | RESOLVED | 9.1's migration adds players.erased_at and the lookup filter; phone_lookup is cited as created in 3.6.
9.8 (a) cloud restore option (9.8→9.9/9.12) | RESOLVED | 9.9 offers only "Restore from a file"; 9.12 adds "Restore from cloud backup".
9.8 (c) oversized (9.8→9.8/9.9/9.10) | RESOLVED | Split into 9.8 (hub.revoke + new code), 9.9 (restore state) and 9.10 (backup.restore key release).
9.11/9.16/9.19 Pis (→9.13/9.18/9.21) | RESOLVED | ACs now say "paired devices (kiosks; Pis when Epic 12 ships)"; the Epic 9 notes repeat this.
9.19 kitchen failover (9.19→9.21) | RESOLVED | Decided in the Epic 9 notes; 9.21 revokes inherited kds tokens and re-pairs each display with a fresh code at the new main's address (Story 7.6 rules); 7.10 shows a "Hub changed?" banner; 9.26 tests it.
9.20 forced-promote alert path (9.20→9.22) | RESOLVED | 9.22 writes a notify.requested owner_alert of new kind standby_forced through the 8.34 Hub path (cited by title); 8.34 makes the kind list an extensible enum.

Totals: 16 RESOLVED, 0 OPEN.

#### Fresh scan — new issues only

NEW: 8.20 | Its FENCED-fetch AC says "the Hub self-locks per Story 2.11 behaviour", but 2.11 only creates or confirms the fence on the first heartbeat. Demotion on a higher fence is in 9.3, and the self-lock is in 9.24. | Cite the Story 1.6 demotion (close the writer, NOT_PRIMARY) for this AC, and say the full self-lock screen arrives in Epic 9 (or test it with a fixture).
NEW: 8.2 | Table/column creation: the test fixture "sets the `lease` fork fields", but no story before it adds fork columns to `lease`. Story 1.6 has only the provisional epoch and hub_instance_id. 9.23 assumes `fork_parent_epoch`/`fork_parent_seq` exist. | 8.2's migration should add nullable `lease.fork_parent_epoch` and `fork_parent_seq` (or name the story that does).
NEW: 9.9 | Table creation: `POST /v1/kek/request` stores a pending key request, which 9.10 then reads, approves or denies, expires after 7 days and checks for replay (KEY_ALREADY_RELEASED). No table is named for it, unlike kek_escrow in 9.2. | Name the new gz_control table in 9.9 (e.g. `kek_requests`: branch_id, hub_instance_id, public_key, state, requested_at, decided_at).
NEW: 9.8 | Forward dependency: AC "a Manager-level owner app user … is refused FORBIDDEN". Owner app team logins and roles arrive only in Epic 14; in Epics 2–9 only the Owner signs in to the Owner app. | Drop the Manager-level clause (keep MFA_REQUIRED), or test it with a fixture non-owner session.
NEW: 8.14 | Weak forward dependency: "Reports, Alerts and Manage list only live branches" and "include it" after go-live. The Reports, Manage and Alerts screens and their branch pickers are built later (8.15, 8.23, 8.33). | Put the live-only filter in the shared branch list or picker that 8.14 provides, and have 8.15/8.23/8.33 consume it. Or move these clauses into those stories.
NEW: 9.4 / 9.18 / 9.20 | Screen naming and creation: 9.4 opens "Hub → System → Backup" as if it exists, but earlier stories use "Settings → System → Backup location" (2.26, 4.7). 9.18 says "Settings → Standby" and 9.20 says "System → Standby" for the same panel. No story states it creates the Backup or Standby panels. | 9.4 should say it adds the Backup panel under Settings → System. 9.18 should create the Standby panel with one consistent name that 9.20 reuses.

Stale numeric references checked inside Epics 8–9: no stale references found. All references to Epic 1–5 stories (1.6, 1.8, 1.9, 1.13, 1.15, 1.21, 1.22, 2.4–2.6, 2.8–2.12, 2.15–2.19, 2.26, 2.30, 2.31, 3.6, 3.7, 3.19, 3.20, 4.7, 4.14, 4.17, 5.8, 5.14, 5.15, 5.19, 5.21, 5.28, 5.32) and to Epic 8/9 stories resolve to the intended renumbered titles.

Out of scope, noted for the Epic 1–5 fork: Story 2.31's "Open dashboard" opens the Owner app dashboard, which is built in Epic 8 (8.10/8.14).
