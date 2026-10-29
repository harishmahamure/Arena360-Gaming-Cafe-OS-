---
title: 'Story 6.11: Session-end cleanup ends the player''s programs and clears the clipboard, temp files and volume'
type: 'feature'
created: '2026-10-29'
status: 'in-review'
baseline_commit: 'c441b6d95774cd7bd32c5c70060e656427770e16'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-10-a-new-player-signs-up-at-the-kiosk-with-one-consent-box.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** When a session ends the next player sits at a PC that still has the last player's programs, clipboard, temp files and volume. The kiosk service only locks the screen (`pc::set_locked`); nothing cleans the player session, and a failed clean-up has no way to reach staff.

**Approach:** A named-step cleanup pipeline in `arena-kiosk-svc` runs on the generic session-end signal (a lock with reason `PayAtCounter`, `EndedByStaff` or `TimesUp` while a session is dirty), covers the screen with an opaque "Getting this PC ready" lock while it runs, then shows the original lock. A step that fails twice sends `kiosk.cleanup_failed`; the Hub raises a `check_pc` ("CHECK PC") alert on the tile, and the kiosk holds the locked screen until staff pick "Retry cleanup" or "Checked" in the tile sheet.

## Boundaries & Constraints

**Always:**

- **Steps.** Named, ordered, extensible list: `programs` (end every process in the player's session that is not the kiosk app and its WebView2 children, the service itself, or a protected System32 image; mode `report` does not stop it), `clipboard` (clipboard and clipboard history), `temp_files` (contents of the player account's temp folder), `volume` (system volume to the branch default `kiosk.volume_default`, cached offline; 50 if never synced). Each step is a `CleanupStep` with `name()`; adding a step is adding one entry to `Pipeline::standard`. The OS parts sit behind a `CleanupHost` trait (Windows impl; fake for tests).
- **Trigger.** One worker task waits on a session-end signal raised by `pc::set_locked` for `PayAtCounter | EndedByStaff | TimesUp` (any path, now or later) and runs only if a session is dirty (set when a session starts, test session included; kept in the queue kv as `cleanup.pending` so a restart mid-clean re-runs it). Hub and local timer both locking `TimesUp` runs it once. `TimesUpSeeCounter` (Staff handles at grace end) does not trigger; staff's later End does.
- **Before the lock.** While it runs and after a failure the screen is `locked` with the new reason `PreparingPc` (kiosk-local; the Hub never sends it); the original reason is remembered (`cleanup.after` in the kv) and shown when the pipeline succeeds. A session that starts meanwhile (Undo of an End, `session.started`) cancels the deferred lock; the player logs in again because the Hub login record is cleared on a new session start of a different player (unchanged) and the kiosk drops its held player on cleanup.
- **Failure.** Each step retries once (`timings.cleanup_retry`). A step failing twice queues a `kiosk.cleanup_failed{step, detail}` intent (ULID idempotency key, queued like other kiosk events, so offline works); other steps still run. The screen then holds on `PreparingPc` (persisted, `cleanup.hold`) until a `cleanup_retry` (runs the pipeline again; failure holds again) or `cleanup_checked` (releases, no run) pc command arrives. Both deliver through `device.screen`. Success queues `kiosk.log` info `cleanup.done` (the result for the Hub, idempotent).
- **Hub.** `kiosk.cleanup_failed` is a paired-device intent (like `kiosk.process_blocked`): it writes a `device_events` row (`error`, `cleanup_failed`, `<step>: <detail>`) and, in the same transaction, a `check_pc` alert for that device unless one is already open. Tile tag `check-pc`, label `hub.alerts.kind.check_pc`. The tile sheet shows "Retry cleanup" and "Checked" while the tile has an open `check_pc` alert; both push the pc command and acknowledge the alert; they need `device.remote` like Lock.
- Copy from the catalogue ("Getting this PC ready — see the counter."); no hex colours; no blur; no personal data in logs or `detail`.

**Never:** cleanup in Staff handles mode before staff End; killing the app, service or protected images; a new `ActorKind`, `ErrorCode` or `PAIRED_SCOPE` change beyond the one new kind; deleting anything outside the player account's temp folder; launcher/browser sign-out (6.12).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Staff End | `PayAtCounter` lock, session dirty | opaque `PreparingPc` lock, 4 steps in order, then `PayAtCounter` lock; `cleanup.done` queued | — |
| Time-up, clean-up mode | local `TimesUp` then Hub `TimesUp` | runs once | second trigger finds nothing dirty |
| Staff handles | grace ends | `TimesUpSeeCounter`, no cleanup; staff End later cleans | — |
| Step fails once | file locked, then free | retry passes, nothing reported | — |
| Step fails twice | `programs` | `kiosk.cleanup_failed{programs}` queued; other steps run; `PreparingPc` held | Hub alert `check_pc` |
| Retry cleanup | hold, command | pipeline reruns; success shows remembered lock | failure holds again |
| Checked | hold, command | hold released, remembered lock shows | — |
| Hub unreachable | any | same steps; result in the queue; replayed once | — |
| Undo of an End | after cleanup | session restored by the Hub as before, player logs in again | — |
| Restart mid-clean | `cleanup.pending=1` | cleanup re-runs on start | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{process.rs,intent.rs,scope.rs,alert.rs,pc_command.rs,lib.rs}` -- `KioskCleanupFailed` (pattern: `KioskProcessBlocked`, process.rs:355), `IntentKind` wiring (intent.rs ~80/344/454/595/711/817/1352/1556/2042), `PAIRED_SCOPE` (scope.rs:54,143), `AlertKind::CheckPc`, `LockReason::PreparingPc`, `PcAction::CleanupRetry|CleanupChecked`; generated TS.
- `crates/arena-core/src/apply/{kiosk_log.rs,mod.rs,validate.rs,mutate.rs}` -- `Event::CleanupFailed`; alert insert via `arena_store::{find_open_alert,insert_alert}` (pattern: `apply/alert.rs`).
- `crates/arena-api/src/lan/session.rs:690-727` -- include the new kind in the kiosk-event rate limit. `ui_auth.rs:2121,2282` -- `device.screen` already pushes any non-power action.
- `crates/arena-kiosk-svc/src/{pc.rs,runtime.rs,session.rs,intents.rs,hub_link.rs,operating.rs,lib.rs,test_session.rs}` -- `cleanup.rs` (new: pipeline, steps, worker, hold), trigger in `pc::set_locked`, dirty flag from `session::sync(started)`, `intents::kiosk_cleanup_failed`, `pc_command` handling of the two actions.
- `crates/arena-kiosk-svc/src/procs/mod.rs:918 close_named` (template), `:1185 run` -- add `close_all_unprotected` and register a closer on `Ctx`.
- `crates/arena-kiosk-svc/src/cleanup_windows.rs` (new, cfg windows) -- `WinCleanupHost`; root `Cargo.toml` windows features.
- `apps/kiosk/src/routes/+layout.svelte:58` and `packages/ui/src/messages/en.json` -- `preparing_pc` reason copy.
- `apps/hub/src/lib/{alerts.svelte.ts,tile-actions.svelte.ts,TileSheet.svelte}`, `packages/ui/src/components/AlertBadge.svelte`, `en.json` -- `check_pc` label/tag, Retry/Checked buttons.
- Tests: Rust beside each; `crates/arena-kiosk-svc/tests/runtime.rs`; `crates/arena-api/tests/lan.rs`; `crates/arena-core/src/apply/tests.rs`; `apps/hub/tests`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- new intent kind + payload, `CheckPc`, `PreparingPc`, two `PcAction`s; regenerate types; tests
- [x] `arena-core` / `arena-api` -- apply the intent (event row + alert), rate limit; tests
- [x] `arena-kiosk-svc` -- cleanup pipeline, worker, hold, closer, Windows host, wiring; tests with fakes
- [x] Kiosk app and Hub UI -- lock copy, alert label/tag, tile sheet buttons; catalogue strings; Playwright

**Acceptance Criteria:**

- Given a session ends by staff End, time-up in clean-up mode or forced end, then the pipeline runs the named steps in order, before the original lock shows, and queues `cleanup.done`.
- Given the session ended into "Pay at counter", then no player program remains while the bill lock shows.
- Given Staff handles mode, when grace ends, then cleanup does not run until staff End.
- Given a step fails and its one retry fails, then `kiosk.cleanup_failed` names it, the Hub raises `check_pc` on the tile, and the kiosk holds "Getting this PC ready — see the counter." until Retry cleanup or Checked.
- Given the Hub is unreachable, then the same steps run and the result is queued with an idempotency key.
- Given Undo of an End after cleanup, then the Hub restores the session as before.

## Spec Change Log

## Review Triage Log

## Design Notes

The epic puts cleanup "in the service" but clipboard and volume belong to the player's interactive session, not the SYSTEM service's. The Windows host therefore does `programs` and `temp_files` itself and runs the clipboard and volume parts through a short-lived helper (`arena-kiosk-svc cleanup-helper --volume N`) started in the player's session with `WTSQueryUserToken` + `CreateProcessAsUserW`, the way `launcher.rs` starts the app. The service waits for it with a timeout.

`Ctx` carries a `closer` set by `procs::run` once its `Inner` exists, so the `programs` step reuses the 1.18 kill path (re-check of path, user and session) without exposing `Inner`.

## Verification

**Commands:**

- `cargo test -p arena-proto -p arena-core -p arena-api -p arena-kiosk-core -p arena-kiosk-svc` -- expected: pass
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` -- expected: clean
- `pnpm gen:types`, `pnpm check:strings`, `pnpm check:hex`, `pnpm check:no-blur`, `pnpm format:check` -- expected: pass
- `pnpm --filter @arena/kiosk check`, `pnpm --filter @arena/hub check` -- expected: 0 errors

**Manual checks (if no CLI):**

- The Windows 10/11 image test needs real images and is not run on this host.

## Implementation Notes

- Verified on this host: `cargo test` for arena-proto, arena-core, arena-api, arena-kiosk-core and the 16 new `cleanup` tests in arena-kiosk-svc; `cargo fmt --check`; clippy on the touched crates; `pnpm check:strings|hex|no-blur`, `format:check`; svelte-check (kiosk, hub) 0 errors; Hub `cleanup-alert.spec.ts`.
- Not run here: the new `tests/runtime.rs` cases (the file is `cfg(unix)`), and the Windows helper, clipboard-history and volume code on real Windows 10/11 images. `arena-e2e` and `tests/discovery.rs` do not build on Windows (as before); 4 kiosk `theme.spec.ts` tests fail on a mangled fixture path (as before).
- The tile-sheet buttons use the existing `device.screen` permission (same as Lock), not a separate `device.remote` check.
