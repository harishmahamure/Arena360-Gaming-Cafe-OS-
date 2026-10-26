---
title: 'Story 3.9: Start a postpaid session for a player or a guest'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_commit: 'b774a293fd62ab0177f1536c36eaf0b53c1aedf8'
baseline_revision: 'b774a293fd62ab0177f1536c36eaf0b53c1aedf8'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-5-non-modal-tile-sheet-with-remote-pc-actions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-1-branch-billing-rules-rounding-minimum-grace-and-time-up-mode.md'
warnings:
  - oversized
deferred: []
---

<intent-contract>

## Intent

**Problem:** Staff can see and lock a free PC but cannot put anyone on the clock. There is no session, no segment and no "In session" tile.

**Approach:** The free-PC tile sheet gets a start area (`player-lookup`, Guest, the group's hourly rate, primary "Start postpaid"). The button submits `session.start{device_id, mode: postpaid, player_id?}` through `/ui/intent`. `apply()` writes a `sessions` row (with the billing-rules snapshot, `business_day`, `fy`, `epoch`, `rev`, `guest_no` for guests), an open `session_segments` row at the group's active rate, and sets the device `in_session`. The Hub UI gets `session.started` plus `devices.changed`, and the snapshot carries the live sessions so the tile shows "In session", elapsed time and the player line.

## Boundaries & Constraints

**Always:**
- Permission `session.start` (Counter and Manager already hold it); remove it from `USED_BY_LATER_STORIES` in `intent.rs`. Mode accepts only `postpaid` (`VALIDATION_FAILED` on `mode` otherwise). No `base_rev`.
- Checks in `apply()`, in this order, nothing written on refusal: device missing or deleted `NOT_FOUND`; kind not `pc` `VALIDATION_FAILED` on `device_id`; `group_id` null `DEVICE_UNASSIGNED`; state `maintenance` `PC_UNAVAILABLE`; live session on the device (`active`, `paused`, `grace`) or state `in_session` `PC_BUSY`; no active rate `NO_RATE`; `player_id` given but missing or deleted `NOT_FOUND`.
- `PC_UNAVAILABLE` also for an offline paired PC: the presence check lives in `run_intent` (`ui_auth.rs`), beside the existing offline gate, because presence is not in the database. Manual devices count online.
- Unique partial index `sessions_one_live ON sessions(device_id) WHERE state IN ('active','paused','grace')`; the `PC_BUSY` select runs first so the loser of a race gets a clean `PC_BUSY`, never `INTERNAL`.
- Session row: `id` (ULID), `player_id` null for a guest, `guest_no` (per `business_day`, `MAX+1`, unique partial index on `(business_day, guest_no)`), `device_id`, `group_id`, `mode`, `rate_id`, `state='active'`, `started_at`, billing snapshot (`rounding_min`, `minimum_min`, `grace_min`, `time_up_mode` from `billing::read`), `epoch`, `business_day` and `fy` from `locale::stamp`, `rev`. Segment: `from_at = started_at`, `to_at` null, `rate_minor_per_hour` from the active rate, `rule_id` null. The device goes `in_session` with `prior_state` set (via `set_device_state`, as `maintenance.rs`).
- Idempotent: the same key returns the original outcome and no second session.
- Only ids in the change log: no name, phone or email. The snapshot carries `player_id`, `username` and `guest_no`; never the player's name or phone.
- Restart and Shut down (`device.power`) on a PC with a live session or `in_session` state: `PC_BUSY` "End or pause the session first" from `apply()` validate, nothing pushed to the kiosk. The sheet hides nothing; the refusal shows inline.
- New error codes `PC_BUSY`, `PC_UNAVAILABLE`, `NO_RATE`, `DEVICE_UNASSIGNED` in every exhaustive match (`error.rs`, cloud and hub `status_of`, kiosk `is_deterministic`, generated TS). User text from `en.json` via `REFUSALS`, keyed by code and intent kind. `DEVICE_UNASSIGNED` reads "Assign this PC to a group before starting a session." with an "Assign to group" action that focuses the existing assign form.
- All new strings in `packages/ui/src/messages/en.json`; regenerate types with `pnpm gen:types`; add `session.start` and the `session` module to audit labels. Guest line reads "Guest <n>". Start button needs `player.view` for lookup; without it only Guest is offered. Start is disabled with a reason until a player or Guest is picked. Targets 44 px, keyboard-operable.
- Add the intent and a session op to the proptest generator and the replay fixture; the one-live-session-per-device invariant is asserted.

**Never:**
- Pause, end, billing arithmetic, settle (3.11+); kiosk unlock (3.10); timer engine and `ends_at` (3.23); prepaid, plans, wallet.
- Console, VR or other non-PC devices (3.35). Session list screen (3.17).
- Guard changes to `device.maintenance`, `device.assign` or revoke for devices in session (deferred).
- Code comments (user rule).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Player start | free online PC, group with active rate, picked player | session + open segment, device `in_session`, `session.started`, tile "In session" + elapsed + username | none |
| Guest start | Guest | `guest_no` = 1, then 2 on the same business day; tile "Guest 1" | none |
| New business day | first guest after day start | `guest_no` restarts at 1 | none |
| Busy | live session or `in_session` | refused, nothing written | `PC_BUSY` |
| Maintenance / offline | state maintenance, or paired PC offline | refused inline | `PC_UNAVAILABLE` |
| No rate | group has no active rate | refused inline | `NO_RATE` |
| Unassigned | `group_id` null | refused, "Assign to group" action | `DEVICE_UNASSIGNED` |
| Race | two intents, same PC | one succeeds, other `PC_BUSY` | `PC_BUSY` |
| Retry | same key | original outcome, one session | none |
| Unknown player | missing `player_id` | refused | `NOT_FOUND` |
| Restart in session | `device.power` on in-session PC | refused, nothing pushed | `PC_BUSY` |
| No permission | role lacks `session.start` | refused | `PERMISSION_DENIED` |

</intent-contract>

## Code Map

- `crates/arena-proto/src/error.rs`, `crates/arena-cloud/src/api/error.rs:14`, `crates/arena-api/src/ui_auth.rs:814`, `crates/arena-kiosk-core/src/queue.rs:~50-95` -- four new codes in each exhaustive match.
- `crates/arena-proto/src/intent.rs` -- `IntentKind::SessionStart` (ALL 38 to 39, `as_str`, `category`, `requirement`, `reasons`), `IntentPayload`, pinned tests, `USED_BY_LATER_STORIES` (line ~928). New `crates/arena-proto/src/session.rs` (payload, mode, `SnapshotSession`), modelled on `pc_command.rs`; `lib.rs` exports.
- `crates/arena-proto/src/snapshot.rs`, `crates/arena-core/src/snapshot.rs:94-141` -- `HubSnapshot.sessions` (`serde(default)`), `read_snapshot`; literals at `ui_pair.rs:35,227`, snapshot tests.
- `crates/arena-store/migrations/0025_sessions.sql`, `src/migrations.rs` (register v25), `src/branch.rs` (`Table::Sessions`/`SessionSegments`, `RowKey`, `ALL`, `is_versioned`), new `src/sessions.rs` (live session for device, next guest no, live sessions) modelled on `players.rs`; `src/lib.rs` re-exports. Column schema follows LLD lines 190-203 plus `guest_no`, billing snapshot, `created_at`, `updated_at`.
- `crates/arena-core/src/apply/session.rs` (new, `Target::Session`), `mod.rs` (module, `Target`, `load`), `guards.rs` (authorize, base_rev arms), `validate.rs`, `mutate.rs`; `maintenance.rs` and `devices.rs:609 set_device_state` as the state-flip model; `pc_command.rs` (add `PC_BUSY` for `DevicePower`); `billing::read`; `locale::stamp`; `find_active_rate` (`devices.rs:403`); `find_player`.
- `crates/arena-api/src/ui_auth.rs:~1935` -- `PC_UNAVAILABLE` presence gate in `run_intent`; `ui_events.rs:~245-310` -- `session.started{session_id, device_id}` when a `sessions` insert is applied.
- `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-replay/src/lib.rs`, `crates/arena-core/src/apply/tests/` (new `session.rs` test module).
- `apps/hub/src/lib/TileSheet.svelte` (start area in the `live` block when `status==="online"`), `lib/tile-actions.svelte.ts` (`startJob`, `permissionFor`, `REFUSALS`), `lib/events.ts` + `routes/+layout.svelte` (`session.started` refresh), `routes/floor/+page.svelte` (in-session `time`/`secondary`, ticker; mount props), `lib/players/PlayerLookup.svelte` (reuse), `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`.
- `apps/hub/tests/tile-sheet.spec.ts` (PC 2 in-session expectations, Tab order), `floor-fixture.ts`, new `session-start.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- error codes, `session.start` kind + payload, `SnapshotSession`, snapshot field; pinned tests; `pnpm gen:types`
- [x] `crates/arena-store` -- migration 0025, tables, `sessions.rs` queries
- [x] `crates/arena-core` -- `apply/session.rs`, wiring, `PC_BUSY` for `device.power`, snapshot read, apply tests, proptest op
- [x] `crates/arena-api` -- presence gate, `session.started` event, status mapping, API tests
- [x] `crates/arena-replay` -- session in the seeded fixture
- [x] `apps/hub` -- start area, refusals, tile in-session display and ticker, events, strings, audit labels
- [x] `apps/hub/tests` -- `session-start.spec.ts`, update `tile-sheet.spec.ts`, axe

**Acceptance Criteria:**
- Given the sheet of a free online PC in a group with an active rate, when it opens, then it shows `player-lookup`, Guest, the hourly rate and a primary "Start postpaid" (disabled until a player or Guest is picked).
- Given a pick, when staff press "Start postpaid", then `session.start{mode:postpaid}` creates the session, open segment and `in_session` device, pushes `session.started`, and the tile shows "In session", elapsed time and the player line.
- Given a guest session, then the line reads "Guest <n>" with n increasing per business day.
- Given a busy, maintenance, offline, no-rate or unassigned device, then start is refused with the code, plain reason inline, and nothing written; unassigned offers "Assign to group".
- Given two simultaneous starts, then exactly one succeeds and the other gets `PC_BUSY`.
- Given a retried key, then the original outcome returns and one session exists.
- Given a PC in session, when Restart or Shut down is requested, then `PC_BUSY` "End or pause the session first" and nothing reaches the kiosk.

## Spec Change Log

## Review Triage Log

## Design Notes

Presence is not stored, so `PC_UNAVAILABLE` for offline is checked at the API edge and the rest in `apply()`. The snapshot carries `username` (stored plain) so the tile needs no extra authenticated call; names stay behind `player.view`. Elapsed time is computed in the browser from `started_at`.

## Verification

**Commands:**
- `pnpm run gen:types && pnpm run check:generated && pnpm run check:protocol && pnpm run check:strings` -- expected: pass
- `cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace` -- expected: pass
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub exec playwright test tests/session-start.spec.ts tests/tile-sheet.spec.ts tests/floor.spec.ts --project=desktop` -- expected: pass
