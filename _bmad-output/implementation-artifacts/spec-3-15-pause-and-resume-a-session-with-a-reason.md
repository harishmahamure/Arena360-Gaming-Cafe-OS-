---
title: 'Story 3.15: Pause and Resume a Session with a Reason'
type: 'feature'
created: '2026-10-27'
status: 'in-review'
baseline_revision: 'f1b3d2ae91daece57e01bd81a800cf1bc6697410'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-14-undo-for-end-and-settle.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A player who steps away, or a PC that breaks, keeps being billed: a session can only run or end. `LockReason::Paused`, `Permission::SessionPause`, the `paused` state in `sessions` and `paused_total_ms` exist, but no intent drives them.

**Approach:** Add `session.pause` (reason required) and `session.resume`, both guarded by `base_rev` and `session.pause`. Paused time is tracked per segment so billing excludes it; the kiosk is covered with "Session paused — see the counter" and unlocked on resume; the Hub tile shows the paused state with a frozen timer and the reason, and the sheet offers Pause (reason picker in place) and Resume.

## Boundaries & Constraints

**Always:**
- `session.pause {session_id, reason, note?}`: `reason` is `player_stepped_away | hardware_issue | power_cut | other`; `other` needs a trimmed `note` of 1–140 chars without control characters; any other reason takes no note. Allowed only on an `active` postpaid session. Sets `state='paused'`, `paused_at=now`, stores `pause_reason`/`pause_note` (new `sessions` columns).
- `session.resume {session_id}`: allowed only on a `paused` session. Adds `now - paused_at` to `sessions.paused_total_ms` and to `paused_ms` of the open segment (new `session_segments.paused_ms`), shifts `ends_at` by the same amount when set, clears `paused_at`/reason/note, `state='active'`.
- Both need `base_rev` (the session `rev`): mismatch `STALE`, missing `VALIDATION_FAILED`. Pause on a paused session, resume on an active one, or on an ended/non-postpaid one: `VALIDATION_FAILED` on `session_id` with a plain message; nothing changes.
- Billing excludes `paused_ms` per segment (before minimum and rounding); a pause still open when the bill is computed (live snapshot bill, End) is excluded up to the bill's end time. `session.end` on a paused session folds the open pause into `paused_ms`/`paused_total_ms` first and clears `paused_at`, reason and note; End Undo therefore reopens it as `active`.
- The unique "one live session per device" index and the device state `in_session` are unchanged by pause. `session.start` on a paused PC is still `PC_BUSY`.
- Kiosk: on commit of a pause the Hub pushes `lock` with `LockReason::Paused`; the kiosk shows `player.lock.paused` as "Session paused — see the counter" and clears its lock deadline, keeping its session. On commit of a resume the Hub pushes the session screen like a start (unlock, within 2 s). `SessionScreen.elapsed_ms` excludes paused time.
- Hub: `SnapshotSession` carries `paused_at`, `paused_total_ms`, `pause_reason`, `pause_note`; the tile uses state `paused` with the pause icon, a frozen timer (elapsed minus pauses, measured to `paused_at` while paused) and the reason (note for Other) as its secondary line. The sheet shows Pause (opens `ReasonPicker` in place; Other reveals a required note) when active and Resume when paused, with the End button available in both; refusals show the plain reason inline.
- Audit lists both intents with labels. Every new intent is added to the Story 1.8 proptest generator and `arena-replay`.
- Money is integer minor units; no code comments (user rule).

**Never:**
- Pause for wallet, pack, pass, block or console sessions, timer-engine warnings or `ends_at` recomputation beyond the shift above (3.23+), pause undo, auto-pause for credit (3.20), kiosk resync of a paused state after the kiosk restarts mid-pause.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pause | active session, reason `hardware_issue`, rev matches | state `paused`, `paused_at` set, kiosk covered, tile paused with reason | No error |
| Pause Other | reason `other`, note "Fan noise" | stored and shown on the tile | No error |
| Other without note | reason `other`, empty or blank note | nothing changes | `VALIDATION_FAILED` on `note` |
| Note on a listed reason | `power_cut` with a note | nothing changes | `VALIDATION_FAILED` on `note` |
| Resume | paused 20 min, rev matches | active, `paused_total_ms` +20 min, kiosk unlocked | No error |
| Pause twice | already paused | nothing changes | `VALIDATION_FAILED` on `session_id` |
| Resume active | session is active | nothing changes | `VALIDATION_FAILED` on `session_id` |
| Stale | wrong `base_rev` | nothing changes | `STALE` |
| Golden bill | 60 min, 20-min pause, ended | 40 billable minutes before minimum and rounding | No error |
| End while paused | paused, then End | pause folded, bill excludes it to End time | No error |

</intent-contract>

## Code Map

- `crates/arena-proto/src/session.rs`, `intent.rs`, `lib.rs`, `permission.rs` -- `PauseReason`, `SessionPause`, `SessionResume`, kinds/payloads/categories/requirements (both `Permission::SessionPause`, already granted to roles in `rbac.rs`), `SnapshotSession` fields; regenerate `packages/ui/src/generated/*`.
- `crates/arena-store/migrations/0029_pause.sql`, `src/migrations.rs`, `src/sessions.rs`, `src/lib.rs` -- `sessions.pause_reason/pause_note`, `session_segments.paused_ms`; `LiveSession` (`paused_at`, reasons), `SegmentRow.paused_ms`, `pause_session`, `resume_session`, `fold_pause`; `end_session` clears pause fields.
- `crates/arena-core/src/billing.rs` -- `bill` takes per-segment `paused_ms` plus the open `paused_since`; golden tests.
- `crates/arena-core/src/apply/session.rs`, `mod.rs` (`Target`), `guards.rs`, `validate.rs`, `mutate.rs` -- load/base_rev/validate/mutate for both, mirroring `load_end` etc.; `mutate_end` folds the open pause.
- `crates/arena-core/src/snapshot.rs`, `session_screen.rs` -- bill and screen use the new inputs; snapshot session fields.
- `crates/arena-api/src/ui_auth.rs` (~L2003 push block) -- push `Lock(Paused)` after pause, session screen after resume (find the device through the screen whose `session_id` matches).
- `crates/arena-kiosk-svc/src/pc.rs` -- `set_locked` clears the lock deadline for `Paused`.
- `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-replay/src/lib.rs`, `crates/arena-core/src/apply/tests/session.rs` -- generator, replay and unit tests for the matrix.
- `packages/ui/src/messages/en.json` -- `player.lock.paused` text, `hub.session.pause*`, `hub.pause.reason.*`, refusals, audit labels.
- `apps/hub/src/lib/tile-actions.svelte.ts`, `TileSheet.svelte`, `routes/floor/+page.svelte`, `lib/audit/labels.ts`; `apps/hub/tests/pause.spec.ts` (fixtures as `session-end.spec.ts`).

## Tasks & Acceptance

**Execution:**
- [x] Protocol, migration, store helpers and billing as in the Code Map
- [x] Apply logic for both intents, End folding the pause, kiosk push and kiosk lock deadline
- [x] Rust unit tests for every matrix row, proptest and replay entries
- [x] Hub tile paused state, sheet Pause/Resume with `ReasonPicker`, strings, audit labels
- [x] Playwright spec for pause, Other note, resume, refusal and axe

**Acceptance Criteria:**
- Given an in-session tile's sheet, when staff press Pause and choose a reason (Other with required text), then `session.pause` applies and the tile shows the paused token, a pause icon, a frozen timer and the reason as its secondary line.
- Given a paused session, then the kiosk covers the screen with "Session paused — see the counter".
- Given staff press Resume, then the timer continues from where it stopped, `paused_total_ms` grows by the paused duration and the kiosk unlocks within 2 s.
- Given a session paused and resumed, then ended, then the bill excludes paused time per segment (60 min with a 20-min pause bills 40 min before minimum and rounding).
- Given pause on a paused session or resume on an active one, then it is rejected with a plain reason and nothing changes.

## Spec Change Log

## Review Triage Log

## Design Notes

Per-segment `paused_ms` (not only the session total) keeps billing right once a rate change opens a second segment (3.31): a pause belongs to the segment open when it ended. The reason lives in the payload, not the generic `ReasonCode` table, because the pause reasons are not the device-revoke ones.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` -- pass (TS regenerated and committed; `pnpm run check:generated` pass).
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast` -- pass.
- `pnpm exec playwright test pause.spec.ts session-end.spec.ts undo.spec.ts` in `apps/hub` -- pass.

## Auto Run Result

Status: in-review
Verified: `cargo fmt --check`, clippy `-D warnings` (proto/store/core/replay/api/kiosk-core), `cargo test` for those crates (205 arena-api lib tests passed on two full runs; one earlier run had a flaky `a_component_change_also_reports_the_bundles_that_hold_it`, which passes alone with and without this change), `pnpm --filter @arena/hub check`, check:strings, check:tokens-used, check:contrast, Playwright `pause`, `session-end`, `undo`, `settle`, `tile-sheet`, `session-start` specs (146 passed). `check:generated` only reports the regenerated TS as new/stale until it is committed.
Left for later: the `arena-kiosk-svc` paused-session integration test (`runtime.rs`) is Unix-only and was not compiled or run on this Windows machine; the independent code review (step 4) has not run; kiosk resync of a paused state after a kiosk restart.
