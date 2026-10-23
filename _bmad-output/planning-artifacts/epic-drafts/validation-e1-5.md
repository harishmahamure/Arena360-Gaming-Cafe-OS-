# Validation — Epics 1–5 (step-04 §2, §3, §5)

Source: _bmad-output/planning-artifacts/epics.md (lines 1185–5969). Read-only check; epics.md not edited.
Checks: (a) forward dependency · (b) table created by first story that needs it / no early use · (c) single-agent size, G/W/T ACs, Covers line · (d) Epic 1 Story 1 = scaffold, no third-party starter.
Mechanical scan: all 140 stories in Epics 1–5 have a Covers line and Given/When/Then ACs.
Format: story | check | problem | suggested fix. Severity: H = blocks a dev agent, M = it would trip a dev agent, L = wording.

## (d) Scaffold
- PASS: Story 1.1 is the greenfield monorepo scaffold; Additional Requirements state there is no third-party starter.

## (a) Forward dependencies
- 1.5 | a | L | The apply() order lists the TTL/dedupe step, but idempotency and TTL arrive in 1.6. | Mark TTL/dedupe as a pass-through stub in 1.5 that 1.6 fills.
- 1.7 | a | M | AC "token is revoked on the Hub" has no revoke intent or UI until Epic 2 (2.18 "device is revoked"). | Add a `device.revoke` intent (or a dev-fixture revoke) to 1.7, or move the AC to the story that adds revoke.
- 1.16 | a | H | The escape suite fails on "a non-allowed process window", but the process allow/block list is built in 1.17. | Put 1.17 before 1.16, or move that assertion into 1.17.
- 1.18 | a | M | The verifier is pushed "when each manager's PIN is set or rotated", but real PIN set/rotate is 2.13. In Epic 1 only dev-fixture PINs exist. | In Epic 1, push the verifier on Hub start and kiosk connect. Add the set/rotate trigger to 2.13.
- 2.3 | a | M | A correct OTP "lands on password + TOTP setup (Story 2.4)", which is a later story. | Land on an account-setup route stub that 2.4 fills, or merge the hand-off into 2.4's Given.
- 2.4 | a | L | The "SMS code as a fallback" toggle is described as an option at sign-in "(Story 2.5)". | Move the toggle AC into 2.5.
- 2.11 | a | H | Licence and clock problems show in "the header alert list", but the alert list and badge service is built in 3.25. 3.25 claims to "extend the Story 2.11 alert list". | Have 2.11 show a header status-badge/banner, and let 3.25 migrate it into the alert list. Or create a minimal alert list in 2.11 and reword 3.25.
- 2.15 | a | M | Uses the `tier-lock` component (UX-DR24) for USER_LIMIT before 2.18, the first story that covers UX-DR24. | Add UX-DR24 (build tier-lock) to 2.15's Covers, or build tier-lock in 1.3 with the shared components.
- 3.4 | a | M | A collapsed zone shows `alert-badge` counts and updates when a device "raises an alert". The alert service is 3.25. | Move the alert-count AC to 3.25, or move 3.25 before 3.4.
- 3.5 | a | H | Restart/Shut down "on a PC with an active session" is refused PC_BUSY, but sessions and the `sessions` table are created in 3.9. | Move the PC_BUSY AC to 3.9 (session start), or to a later story.
- 3.7 | a | L | The lookup index "updates on every create/edit/erase" and erased players are hidden. Edit is 3.8 and erasure is Epic 9. | Scope 3.7 to create. Add the edit hook in 3.8 and the erase hook in the Epic 9 erasure story.
- 3.19 | a | H | Credit reopens when "a collection brings oldest unpaid credit under 15 days", but `credit.collect` is 3.20. FIFO allocation of collections is also defined before collections exist. | Move the reopen-on-collection AC into 3.20, or swap 3.19 and 3.20.
- 4.3 | a | H | Requires "a paired Android Hub", but Android pairing (Keystore, /v1/pair on Android) is 4.4. | Swap 4.3 and 4.4.
- 4.3 | a | L | "Tunnel remote devices or standby pairing … read 'Needs the Windows Hub'": those sections come from later epics. | Move this AC to the stories that introduce Tunnel and standby.
- 4.6 | a | H | A Bluetooth print failure is "recorded in the outbox result", but `side_effect_outbox` is created in 5.17 (Epic 5). Epic 4 depends only on Epics 1–3. | Record the failure on the printer task / setup status in 4.6, or make Epic 4 depend on Epic 5 (5.17).
- 4.13 | a | M | Uses kiosk alerts "call staff, login lockout", which are Epic 6 features. | Use time-up or the 3.25 fixture alert kind in this AC.
- 5.7 | a | M | Expired stock is "disabled in the POS grid with an 'Expired' tag", but the POS grid is built in 5.9 (which already has this rule). | Drop the POS-grid clause from 5.7 and keep the `catalog.changed` availability.
- 5.10 | a | M | The float prefill is "last closing count minus any deposit recorded at that close". Deposit-at-close is 5.26 and shift close is 5.26. The cash sale event also cites a "POS settle", which is 5.11. | Prefill ₹0 / the last float in 5.10. Add the carry-over and deposit rule in 5.26.
- 5.13 | a | L | Says "the correction is a void or a refund", but void is 5.23 and refund is 5.24. | Keep only "cancel is not offered for served lines".
- 5.22 | a | L | "visible in the shift report": the shift/Z-report is 5.27. | Remove; 5.27 already lists discounts.
- 5.23 | a | M | "voids are counted per staff and excluded from sales" in reports and the shift report, which are 5.27, 5.29 and 5.30. | Move the AC to 5.27/5.29/5.30, which already cover voids.
- 5.27 | a | H | "Reports → Shifts" past-shift view, but the Reports screen and its list are created in 5.29. | Move 5.29 (Reports screen shell) before 5.27, or let 5.27 create the Reports screen with Shifts.
- 5.28 | a | H | "Reports → GST" before the Reports screen exists (5.29). | Same fix as 5.27: reorder so the Reports screen comes first.
- 5.30 | a | L | "Erased player" rows depend on erasure in Epic 9. | Reword to "a player with no personal data (erasure lands in Epic 9)", or move to Epic 9.

## (b) Table / entity creation
- 3.6 | b | M | `player.create` creates `wallets` and `credit_balances` rows and tables before they are needed (wallet in 3.16, credit in 3.18). | Create `wallets` in 3.16 and `credit_balances` in 3.18, with rows created lazily or by backfill.
- 5.3 | b | L | Creates the `stock` table (qty 0) with products, before stock-in (5.5) needs it. | Create `stock` in 5.5, or keep it and state in 5.3 why availability needs it now.
- 2.17 / 2.23 | b | M | 2.17 writes a `devices_meta` row via `device.register` without saying it creates the table or the intent. 2.23 says `devices_meta.state` is set "when the schema is created". 1.7 created `devices` separately. | Say in 2.17 that it creates the `devices_meta` table and the `device.register` intent. Have 2.23 add the `state` column by migration.
- 1.17 | b | L | Allow/block lists are "held at the Hub", with no table or settings key named. | Name the storage (for example settings keys or a `process_rules` table created in 1.17).

## (c) Size / AC / Covers
- 1.1 | c | M | Oversized for one dev agent: 11 crates, 9 app skeletons (including Pi, partner, admin and marketing for Epics 7–12), version-pin CI, type generation, string lint. The scope is mandated by "STARTER / SCAFFOLD". | Split into 1.1a workspace + apps + pins CI, 1.1b ts-rs type generation + freshness gate, 1.1c message catalogue + string lint.
- 2.1 | c | M | Large: K8s deploy for staging and prod, Turso, the SOPS/age secrets pipeline, the error envelope, the local dev mode and the SQLite linkage spike. | Split into cloud deploy + secrets, and local dev + error envelope + linkage spike.
- 5.37 | c | M | Spans the Hub upload task, a cloud R2 endpoint with entitlement check, the wizard step-4 UX and the bootstrap extension. | Split into Hub/cloud template upload, and wizard "Start from <branch>" + bootstrap apply.
- 2.14 | c | L | The command catalogue AC omits `branch.bootstrap`, but the Epic 2 notes list it as a setup kind (it travels in the /v1/pair response in 2.12). | Make the notes and the 2.14 AC agree.
- 5.17 | c | L | Reprint is offered from "POS history", which no story defines. | Define a POS history view (for example in 5.11) or drop it from the AC.

## Counts
(a) 24 · (b) 4 · (c) 5 · (d) 0 — total 33
