---
title: 'Story 3.32: Transfer a Session to Another Device'
type: 'feature'
created: '2026-10-27'
status: 'in-review'
baseline_revision: '7a64a9c436f3e4b4950d6fda5c4f9d3b5e1145dd'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-31-a-rule-change-mid-session-opens-a-new-rate-segment.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-10-the-pc-unlocks-to-a-session-screen-when-a-session-starts.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** When a player's PC breaks, staff can only end the session and start a new one, which splits the bill and loses the player's time.

**Approach:** A staff-only `session.transfer {session_id, device_id}` (permission `session.transfer`, already held by Counter, `base_rev` required) moves a running or paused session to a free, online, assigned PC: it closes the open segment, opens the next on the new device at the new group's resolved rate, frees the old device and marks the new one `in_session`. The Hub then locks the old PC and unlocks the new one (or locks it as Paused), and the floor offers a Transfer mode that highlights the valid targets.

## Boundaries & Constraints

**Always:**
- Only a session in state `active` or `paused` transfers (any mode); a `grace`, `expired` or ended session is `VALIDATION_FAILED` on `session_id`; a missing session is `NOT_FOUND`.
- Target checks, in this order: missing or deleted device `NOT_FOUND`; same device as the session's `VALIDATION_FAILED`; no group `DEVICE_UNASSIGNED`; kind differs from the source device's kind `GROUP_KIND_MISMATCH`; `maintenance` `PC_UNAVAILABLE`; `in_session` or any live session `PC_BUSY`; for a postpaid or wallet session the new group has no active rate `NO_RATE`; for a pack or pass session the player pack's `group_ids` lacks the new group, and for a block session the new group differs from `sessions.group_id`, `PLAN_NOT_FOR_GROUP`. A refusal writes nothing.
- A target that is offline (paired but not connected) is refused `PC_UNAVAILABLE` in `run_intent` before the writer, exactly like `session.start` does.
- Postpaid and wallet: the new segment's rate and `rule_id` come from `pricing::priced` for the new group's active rate at the transfer instant; `sessions.rate_id` and `sessions.group_id` move to it so 3.31 flips use the new group's rules. A wallet session whose rate differs from the open segment's re-sizes `ends_at` and `reserved_minor` with `billing::wallet_end_after_flip` (never below zero, never before the transfer instant); an equal rate leaves them untouched.
- Pack, pass and block sessions open the new segment at rate 0 with no `rule_id`, as `session.start` does; `ends_at` is unchanged. A block session's open order moves to the new device (`orders.device_id`).
- A paused session stays paused: the closed segment takes the pause up to the transfer, the new one only the pause after it (reuse `add_segment_pause` as `rate_flip::mutate` does); `paused_at` is unchanged.
- The old device returns to its `prior_state` (never `in_session`) else `online`; the new device's `prior_state` is the state it had.
- After commit the Hub pushes the old PC `lock` (reason `LockedByStaff`, cmd id `<key>:old`) and the new PC the session screen (`push_session_started`, cmd id = the idempotency key) or, when paused, `lock` with reason `Paused` (cmd id = the key). A retry of the same key pushes again.
- No code comments beyond doc comments (user rule). Every new string goes in `packages/ui/src/messages/en.json`.

**Never:**
- Console sessions (3.35), group bills (3.33), forced end (3.34), a new `LockReason`, or kiosk-side changes.
- Repricing the time already used: closed segments keep their rate.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| Same-group move | active postpaid on A, B free online in the same group | A's segment closes, new segment on B at the same rate, A free, B `in_session`; old PC locks, new PC unlocks | none |
| Other group | B in a group with a different rate | new segment at B's group rate and rule, `sessions.group_id`/`rate_id` follow | none |
| Paused | paused session moved | B stays locked "Paused"; resume later unlocks B; pause split across both segments | none |
| Wallet, new rate higher | reserve fits less time | `ends_at` earlier (≥ now), `reserved_minor` re-sized | none |
| Pack not allowed | pack whose plan excludes B's group | refused | `PLAN_NOT_FOR_GROUP` |
| Busy / maintenance / offline / unassigned | B has a session / in maintenance / not connected / no group | refused, nothing changes | `PC_BUSY` / `PC_UNAVAILABLE` / `PC_UNAVAILABLE` / `DEVICE_UNASSIGNED` |
| Other kind, same device, ended session | console target / A itself / ended | refused | `GROUP_KIND_MISMATCH` / `VALIDATION_FAILED` / `VALIDATION_FAILED` |
| Stale tile | `base_rev` behind | refused | `STALE` |
| New kiosk silent | no ack in 5 s | new tile shows "Failed — retry"; session stays on B; Retry resends the same key and the Hub pushes again | UI only |

</intent-contract>

## Code Map

- `crates/arena-core/src/apply/rate_flip.rs`, `extend.rs` -- templates: `load`/`validate`/`mutate`, `open_of`, `add_segment_pause`, `reprice_wallet_session`, `Written` rows.
- `crates/arena-core/src/apply/session.rs` -- `load` (device, busy, `find_active_rate`, `pricing::priced`), `validate` (error codes), `mutate_end` (device restore via `prior_state`), `base_rev_pause`/`check_rev`.
- `crates/arena-core/src/apply/prepaid.rs` -- `find_player_pack` and `list_plan_groups` usage for the group checks.
- `crates/arena-core/src/apply/transfer.rs` (new) -- `TransferTarget`, `load`, `base_rev`, `validate`, `mutate`; wired in `apply/{mod,guards,validate,mutate}.rs` at every `SessionExtend` match arm (`Target::SessionTransfer`).
- `crates/arena-store/src/{sessions,devices}.rs`, `lib.rs` -- new `move_session(session_id, device_id, group_id, rate_id)` and `move_open_orders(session_id, device_id)`; reuse `close_segments`, `insert_segment`, `set_device_state`, `find_device_meta`, `device_has_live_session`.
- `crates/arena-proto/src/{intent,session,lib}.rs` -- `SessionTransfer {session_id, device_id}`; `IntentKind::SessionTransfer` at every `SessionExtend` site (wire name, category, `Requirement::Perm(Permission::SessionTransfer)`, the string table near line 1344); drop `SessionTransfer` from `USED_BY_LATER_STORIES`; `pnpm run gen:types`.
- `crates/arena-api/src/ui_auth.rs` -- `run_intent`: offline-target refusal beside the `SessionStart` check (~line 1986); after commit push the old and new PC as above (model on the `started`/`push_pause_state` blocks, ~lines 2035-2200); the old device is the other `devices_meta` change.
- `crates/arena-core/tests/proptest_apply.rs` -- add a `SessionTransfer` op (every story adds its intents to the generator); `apps/hub/src/lib/audit/labels.ts` and `en.json` `hub.audit.intent.session.transfer`.
- `apps/hub/src/lib/tile-actions.svelte.ts` -- `transferJob(session_id, device_id, rev)` (`track: "cmd"`, optional `moves_to` so `send` files the outcome under the target device id), `REFUSALS` entries for the transfer codes, `permissionFor("session.transfer")`.
- `apps/hub/src/lib/TileSheet.svelte` -- "Transfer" button in the `session-bill` section (active or paused only) calling a new `ontransfer` prop; shows the picking hint and Cancel while in the mode.
- `apps/hub/src/routes/floor/+page.svelte` -- `transferring` state; tiles that are free, online, assigned and of the source's kind get `data-transfer-target` and the "Move here" secondary line; activating any tile in the mode sends `transferJob` (tap, or arrows then Enter via the existing `onkeydown`), the others are still pickable so the server's reason shows inline in the source sheet; on success `activeId` becomes the target so its sheet shows "Failed — retry" if unacknowledged; Escape cancels.
- `packages/ui/src/components/FloorTile.svelte` -- target highlight (outline from tokens, not colour alone); `check:tokens-used`/`check:contrast` apply.
- `apps/hub/tests/transfer.spec.ts` (new), `floor-fixture.ts` -- Playwright.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- `SessionTransfer` intent and kind, requirement, category, audit label; regenerate types
- [x] `arena-store` -- `move_session`, `move_open_orders`
- [x] `arena-core/apply/transfer.rs` -- load, base_rev, validate (ordered checks), mutate (segments, pause split, wallet re-size, device states, rate_id/group_id), wiring, proptest op
- [x] `arena-core` tests -- `apply/tests/transfer.rs` covering every matrix row (same group, other group, paused, wallet up and down, pack/block `PLAN_NOT_FOR_GROUP`, each refusal, stale, idempotent replay, bill totals across both segments)
- [x] `arena-api/ui_auth.rs` -- offline-target refusal; post-commit lock and unlock pushes; unit test with a fake presence like the existing `session.started` tests
- [x] Hub UI -- job, refusal strings, sheet button, floor Transfer mode with keyboard path, target highlight, retry on the new tile; `en.json` strings
- [x] Playwright `transfer.spec.ts` -- targets highlighted, tap and arrows+Enter pick, refusal inline with nothing changed, failed ack shows "Failed — retry" on the new tile and Retry resends

**Acceptance Criteria:**
- Given an in-session or paused tile's sheet, when staff press Transfer, then free, online, assigned devices of the same kind are highlighted and picking one (tap or arrows + Enter) applies `session.transfer`.
- Given the transfer commits, then the old device locks and the new one unlocks (or locks as Paused) within 5 s, the current segment closes, a new one opens on the new device at its group's resolved rate, and both tiles update.
- Given a target in another group, then the rate changes from that moment and a pack not allowed there is refused `PLAN_NOT_FOR_GROUP`.
- Given a busy, offline, maintenance or other-kind target, then it is refused with the reason inline and nothing changes.
- Given the new kiosk does not acknowledge within 5 s, then the new tile shows "Failed — retry", the session stays on the new device, and Retry resends the unlock.

## Spec Change Log

## Review Triage Log

## Design Notes

The wallet re-size is the 3.31 flip with `flip.at = now`: `wallet_end_after_flip(closed_segments, now, new_rate, rounding, minimum, reserved)` then `reprice_wallet_session(ends_at + extended_min*60_000, reserved)`. Cmd ids: the unlock on the new PC carries the key so the Hub UI's `waitForAck(key)` tracks it; the old PC's lock uses `<key>:old` so its ack cannot satisfy the wait.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast` -- only the known unrelated arena-cloud and arena-tasks failures (see spec 3.31) may remain; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test transfer tile-sheet floor` in `apps/hub`.

## Auto Run Result

Status: in-review
Verified: fmt, clippy, arena-core/arena-api/arena-store/arena-proto tests (14 `transfer` apply tests, 2 `ui_auth` push tests, proptest `Transfer` op), hub check, `check:strings`; implementation run also passed kiosk check, token/contrast/protocol/layer checks, prettier and Playwright `transfer tile-sheet floor` (88). Known unrelated failures as before: arena-cloud (2) and arena-tasks (2). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: a wallet session past its own end is not re-sized on transfer; the old PC locks as `LockedByStaff`; below 840 px the fixed sheet can cover a target tile.
