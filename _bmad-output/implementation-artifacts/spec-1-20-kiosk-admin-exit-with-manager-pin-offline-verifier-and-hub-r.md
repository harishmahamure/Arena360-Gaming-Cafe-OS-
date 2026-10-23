---
title: 'Story 1.20: Kiosk admin exit with manager PIN, offline verifier and Hub remote unlock'
type: 'feature'
created: '2026-10-23'
status: 'draft'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-13-inline-manager-pin-elevation.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-18-kiosk-process-allow-block-list.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A locked-down kiosk can't be serviced without reinstalling. Nothing lets a manager drop it to the desktop at the PC, while the Hub is down, or remotely, and nothing logs those exits.

**Approach:** Add three audited ways to unlock a kiosk:
- **At the PC:** a hidden Ctrl+Alt+Shift+A combination opens an `InlinePin` in the kiosk. The Hub verifies the PIN; when the Hub is down, a cached, versioned, DPAPI-protected verifier pushed by the Hub is used instead.
- **Remotely:** from the Hub floor entry, using the 1.13 elevation flow.

Every exit grants 15 minutes of Explorer and then re-locks. It is recorded as a `device.admin_exit` intent that carries `elevated_by` with its mode.

## Boundaries & Constraints

**Always:**
- **Permission:** `device.remote` is the admin-exit permission. It is held by Owner and Manager.
- **Verifier (Hub):**
  - `ManagerVerifier { version: i64, entries: [{staff_id, pin_hash, pin_verifier_version}] }` covers active staff whose role holds `device.remote`. The `pin_hash` values are the existing argon2id PHC strings, so no plaintext is involved.
  - `version` = max(`pin_verifier_version`) + a monotonic counter in `hub_meta`, bumped on any change to that staff set, their PINs or their active state.
  - The Hub pushes `event{kind:"verifier", verifier}` to paired sessions on Hub start, on each paired hello, and on change. It goes only to the `paired` class, never to browser classes.
- **Verifier (kiosk service):** stores the verifier DPAPI-protected (machine scope) in `verifier.bin`, and replaces it when a higher version arrives.
- **Combination:**
  - The 1.17 hook passes Ctrl+Alt+Shift+A. The kiosk app opens an `InlinePin` panel with the catalogue title "Admin exit". There is no on-screen hint that the combination exists anywhere else.
  - The app sends the PIN only over IPC to the service (`AdminExit{pin}`), and the service clears it right after use. The PIN never goes to the Hub as plaintext.
- **Online path:** the service sends a LAN request `kiosk.verify_admin_pin{pin_proof}`, where `pin_proof` is the PIN itself. The LAN link is pinned TLS (1.9), so this keeps "PIN never leaves the screen" in spirit, the same way the Hub UI loopback does. This trade-off is recorded in the README.
  - The Hub verifies the PIN against eligible staff (1.13 helper) and replies `{staff_id, verifier_version}` or `PIN_REJECTED`.
  - On success the service submits `device.admin_exit{mode: hub}` with `elevated_by{pin, staff_id, hub, verifier_version}` and grants the exit.
- **Offline path (Hub unreachable or `HUB_SLOW`):**
  - The service verifies argon2id against the cached verifier's entries, off the main thread, checking at most 50 entries.
  - On a match it grants the exit and queues `device.admin_exit{mode: cached_verifier}` with `elevated_by{pin, staff_id, cached_verifier, verifier_version: cached.version}`, a ULID key and `issued_at` set at the time of exit.
  - On reconnect it replays through the 1.16 queue.
- **`apply()` for `device.admin_exit`:**
  - The requirement is `Device` for the actor and `device.remote` for the elevator. The actor must be the kiosk device itself.
  - `mode=cached_verifier` is accepted only for this intent kind, per AD-30.
  - When the cached `verifier_version` is older than the version current at `issued_at`, the intent is still applied but recorded with `flagged = 1` and `flag_reason = "stale_verifier"`. Story 1.20 keeps a `verifier_history(version, valid_from)` table for this check. Otherwise `flagged = 0`.
  - It writes a `device_admin_exits(exit_id, device_id, staff_id, mode: hub|cached_verifier|remote, started_at, relock_at, flagged, flag_reason)` row (migration).
  - Re-lock is recorded with `device.admin_relock{exit_id}` (Device requirement), which sets `relocked_at`.
- **Failures:**
  - A wrong PIN, or a PIN from staff without `device.remote`, clears the cells and shows the generic catalogue message.
  - The service logs the failed attempt (device id and count, no PIN) and queues a `kiosk.log{warn, code: admin_exit_failed}`.
  - 5 consecutive failures pause the prompt for 60 s with a countdown. The counter is held in the service, so restarting the app doesn't reset it.
- **Granting the exit (Windows):**
  - The service starts `explorer.exe` in the player session (`CreateProcessAsUserW`) and suspends process enforcement (1.18) and the topmost and keyboard-hook lockdown in the app, via IPC `AdminExitStarted{until}`.
  - The app shows a small countdown badge in a non-topmost corner window, with a catalogue label "Admin exit: 14:59 left".
  - At 15 minutes: Explorer is killed in the player session, enforcement resumes with the 1.18 startup sweep, the app goes topmost again, and `device.admin_relock` is queued.
  - The 15-minute constant is shared through kiosk-core.
  - Behind the `AppLauncher`/`ProcessKiller` traits, with fakes for tests.
- **Remote unlock:**
  - On the Hub floor (1.10), each device entry gets a "Remote unlock" button. It needs `device.remote`, and a Counter can use it through the 1.13 `InlinePin` elevation.
  - It submits `device.admin_unlock{device_id}` (requirement `device.remote`). That writes a `device_admin_exits` row with `mode=remote` and pushes `event{kind:"admin_unlock", exit_id}` to that device's paired session.
  - The kiosk service grants the exit and replies `kiosk.admin_unlock_ack{exit_id}`.
  - The Hub UI waits up to 5 s for the ack, which arrives as an SSE event. Without it, the entry shows the catalogue "Failed — retry".
  - A device that is offline fails at once with the same message.
- **Audit:** the 1.15 viewer shows these changes with the kiosk, the approving manager, the mode (Hub, cached verifier or remote), the start and the re-lock times. A flagged exit shows a "Flagged for review" text marker. Add catalogue labels for the new intent kinds and modules.

**Never:**
- Showing the combination anywhere in the UI.
- A plaintext PIN stored anywhere or logged.
- Cached-verifier elevation for any other intent.
- Leaving enforcement suspended after the 15 minutes or after a service restart. On restart, an expired exit re-locks immediately.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Online exit | combination, manager PIN, Hub up | exit granted; `device.admin_exit` mode hub, with elevated_by; re-lock after 15 min (fake clock) | N/A |
| Offline exit | Hub down, cached verifier v3, manager PIN | granted; queued intent with mode cached_verifier; applied after reconnect with `flagged=0` | N/A |
| Stale offline | exit made with v2 cached while the Hub moved to v3 before `issued_at` | applied, `flagged=1 stale_verifier` | N/A |
| Verifier update | kiosk has v2, Hub pushes v3 | stored v3 replaces v2 | N/A |
| Wrong PIN | 9999 | cells clear; generic message; failure logged | N/A |
| No permission | Counter PIN 3333 | rejected, same message | N/A |
| Throttle | 5 failures | 60 s pause, which survives an app restart | N/A |
| Remote | a manager clicks Remote unlock with the device online | `device.admin_unlock`; the kiosk acks within 5 s; the entry shows success | N/A |
| Remote no ack | device offline, or no ack within 5 s | "Failed — retry" | N/A |
| Browser class | a kds session | never receives the verifier event | N/A |
| Restart during exit | service restarts after `relock_at` | re-locks immediately; relock queued | N/A |
| Audit | an exit is viewed | kiosk, manager, mode, start and relock shown; the flagged marker shown when set | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-api/src/lan/{session.rs,registry.rs}` -- paired-only pushes and request handling. Add the verifier push, `kiosk.verify_admin_pin`, the admin_unlock push and the ack.
- `crates/arena-core` -- the 1.13 PIN verification helpers, the elevation logic in `apply()` (`mode cached_verifier` was reserved for this story) and the 1.16 `device_events`.
- `crates/arena-kiosk-svc` -- the 1.16 queue, DPAPI store, IPC and launcher, plus 1.18's enforcement. Add verifier storage, the exit state machine and the throttle.
- `crates/arena-kiosk-core` -- the IPC types (add AdminExit, AdminExitStarted, AdminExitEnded) and the exit timer state machine (pure).
- `apps/kiosk` -- the `InlinePin` panel, the countdown badge window and the hook pass-through.
- `apps/hub/src/routes/floor/+page.svelte` -- add the Remote unlock button with elevation and an SSE ack wait.
- The 1.15 audit viewer -- add the labels and flag marker.

## Tasks & Acceptance

**Execution:**
- [ ] Proto, store and core: the intents, tables, verifier building and history, the `cached_verifier` acceptance and flagging, with tests.
- [ ] API: verifier push, PIN verify request, remote unlock push and ack, with tests.
- [ ] kiosk-core: exit state machine and IPC; kiosk-svc: verifier store, online/offline paths, throttle, grant and re-lock with fakes, with tests.
- [ ] Kiosk app panel and badge; Hub floor remote unlock with Playwright (stubbed ack and timeout); audit labels.
- [ ] Catalogue keys, README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` plus the dev-fixtures runs, then every non-Windows matrix row passes. The Windows code compiles in CI.
- Given `pnpm check` and the Hub and kiosk Playwright suites, then all pass.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-kiosk-svc --features dev-fixtures && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub exec playwright test` -- expected: pass
