---
title: 'Story 3.34: Forced End with Manager PIN and Reason'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_revision: 'fb616274a813bb86365b80d0d02ba9e7e0121462'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-33-group-sessions-under-one-bill-and-one-payer.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Ending a player's paid time early is one tap, so early ends are neither rare, nor accountable, nor explained to the player.

**Approach:** Two new staff intents, `session.force_end` and `session.force_end.reverted`, both requiring the existing `session.force_end` permission, so the kind-based PIN elevation (AD-30) makes a counter member get a manager's PIN. `session.force_end` is the ordinary end (a pack session deducts only the minutes played, a wallet session pays only for the time used) plus a required reason, and it marks the session `end_reason = 'forced'` with `forced_reason`, `forced_note`, `forced_left_min` (minutes left at the time) and `forced_pack_min` (pack minutes the end took). The kiosk locks with "Your session was ended by staff. Please see the counter." (`ended_by_staff`). Its undo needs the same permission and restores the session, including pack minutes and the wallet debit.

## Boundaries & Constraints

**Always:**
- `session.force_end {session_id, note?}` (permission `session.force_end`, `base_rev` = the session rev) takes one of `rule_violation`, `hardware_issue`, `closing`, `other` in `Intent.reason`; none is `REASON_REQUIRED`. `other` needs a note of 1 to 140 characters and any other reason takes none (`VALIDATION_FAILED` on `note`). The session must be active, paused or in grace.
- Elevation reuses the existing ticket path: the actor is a staff member, the elevating manager holds `session.force_end`, and an unusable ticket is `PIN_REQUIRED`. The audit shows the actor, `elevated_by`, the reason (on the change) and `forced_*` on the session row.
- `session.force_end.reverted {session_id}` (permission `session.force_end`, `base_rev` = the session rev) works only on a force-ended session, by the staff member who ended it, inside the 8 s window, with no receipt, drawer kick or invoice recorded and the PC free. Postpaid: the open bill is dropped. Pack, pass, wallet: the settled zero or wallet bill, its payment and wallet debit are dropped and the pack minutes are given back. Block: the sold bill is kept. An earlier member of a group is still `UNDO_NOT_ALLOWED`.
- `reader.ending` reports a forced prepaid end as undoable (`irreversible` only with a recorded effect).
- The Hub locks the PC with `ended_by_staff` after a forced end.
- No code comments beyond doc comments. Every new string is in `packages/ui/src/messages/en.json`.

**Never:**
- Server-side refusal of a plain `session.end` on a prepaid session with time left (the UI routes it to Force end), console sessions (3.35), kiosk-side changes, a new permission.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| Counter forces an end | no ticket | refused | `PERMISSION_DENIED`; the UI asks for the PIN first |
| Manager PIN | ticket for `session.force_end` | ends, records reason and approver | wrong PIN `PIN_REJECTED`, session keeps running |
| No reason | reason absent | nothing written | `REASON_REQUIRED` on `reason` |
| Other without note | reason `other`, no note | nothing written | `VALIDATION_FAILED` on `note` |
| Pack session | 78 min left, 13 min played | pack loses 13, bill zero, undo gives 13 back | none |
| Wallet session | 30 min played | wallet pays 30 min, undo refunds | none |
| Undo late or by another member | past 8 s | session stays ended | `UNDO_NOT_ALLOWED` |
| Undo of a plain end as forced | ended by `session.end` | refused | `UNDO_NOT_ALLOWED` |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0041_forced_end.sql`, `src/sessions.rs` (`mark_forced`, `forced_pack_minutes`, `reopen_session` clears `forced_*`, `find_ending` knows forced), `src/player_packs.rs` (`restore_pack_minutes`).
- `crates/arena-core/src/apply/force_end.rs` -- load, base_rev, validate and mutate of both intents, built on `session::mutate_end`, `mutate_settle_reverted` and `mutate_end_reverted`; `session.rs` -- `load_end_reverted_of`, `SettleRevertTarget::of`.
- `crates/arena-proto` -- `SessionForceEnd`, `SessionForceEndReverted`, `ReasonCode` (`rule_violation`, `hardware_issue`, `closing`), `IntentKind::reasons`; `pnpm run gen:types`.
- `crates/arena-api/src/ui_auth.rs` -- lock reason `ended_by_staff` after a forced end, the end outcome and reopening for both intents.
- `apps/hub/src/lib/tile-actions.svelte.ts`, `TileSheet.svelte`, `undo.svelte.ts`, `UndoDock.svelte`, `audit/labels.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-store` -- migration, forced marks, pack restore, `find_ending`
- [x] `arena-proto` -- intents, reasons, generated types
- [x] `arena-core` -- `force_end.rs`, wiring in `apply`, proptest op `ForceEnd`
- [x] `arena-core` tests -- reason and note rules, postpaid, pack, wallet, block, undo window and kind, counter without permission, manager PIN records approver
- [x] `arena-api` -- `ended_by_staff` lock and undoable outcome (test)
- [x] Hub UI -- End on a prepaid session with time left opens the reason picker then the inline PIN, Force end beside End on other sessions, "Session ended early" undo toast needing the same approval, audit labels
- [x] Playwright `force-end.spec.ts`

**Acceptance Criteria:**
- Given a prepaid session with time left, when staff press End, then the reason picker and the inline PIN open in the sheet; a postpaid session offers Force end beside End.
- Given a manager PIN and a reason, when `session.force_end` applies, then the reason and approver are recorded, only the time used comes off the pack or wallet and the kiosk locks with "Your session was ended by staff. Please see the counter."
- Given no reason, or a PIN without the permission, then the end is refused and the session continues.
- Given the audit, then the actor, approving manager, reason, note and minutes left are visible.
- Given the Undo toast of a forced end, then Undo needs the same elevation and restores the session and its remaining time.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `check:strings`, `check:tokens-used`, `check:contrast`, `check:hex`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test force-end undo pause group-bill transfer session-end session-start extend prepaid-start tile-sheet floor audit` in `apps/hub`.

## Auto Run Result

Status: review
Verified: fmt, clippy; workspace tests (arena-core 653 lib tests incl. 6 `force_end` and 1 rbac, proptest with a `ForceEnd` op, arena-api 221 incl. the forced-end lock); hub and kiosk check; `check:strings`, `check:tokens-used`, `check:contrast`, `check:hex`, `check:protocol`, `check:layers`, `format:check`; Playwright 250 (force-end, undo, pause, group-bill, transfer, session-end/start, extend, prepaid-start, tile-sheet, floor, audit). Known unrelated failures as before: arena-cloud (2) and arena-tasks (2). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: a separate intent kind (`session.force_end`) instead of a `forced` flag on `session.end`, because elevation is matched by kind and the manager must hold `session.force_end`; the reason picker comes before the PIN (the same order as Pause) and the ticket is spent at once; the minutes left are taken from `ends_at` (to the pause moment when paused, 0 for postpaid); after an undo the time between the end and the undo counts as played, as for a postpaid undo.
Deferred: server-side refusal of a plain `session.end` on a prepaid session with time left; a forced end on a group member other than the last cannot be undone; the audit viewer shows `forced_*` through the session row image only, with no dedicated summary.
