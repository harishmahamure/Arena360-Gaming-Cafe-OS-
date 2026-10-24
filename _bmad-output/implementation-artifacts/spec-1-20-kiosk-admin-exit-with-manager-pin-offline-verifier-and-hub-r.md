---
title: 'Story 1.20: Kiosk admin exit with manager PIN, offline verifier and Hub remote unlock'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'e9876d86c38a3137b06261a141a5a3f17cbe1d5e'
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
- The 1.15 audit viewer (`apps/hub/src/routes/system/audit`) -- add the labels and flag marker.
- `crates/arena-store/migrations/` -- next migration is `0010_*.sql` (tables `device_admin_exits`, `verifier_history`; `hub_meta` verifier counter).
- `crates/arena-proto/src/permission.rs` -- `device.remote` already exists and is seeded for Owner and Manager (`arena-store/src/staff.rs`). Reuse it; do not add a permission.
- `crates/arena-core/src/apply/guards.rs` -- where `cached_verifier` elevation is currently rejected; allow it only for `device.admin_exit`.
- `crates/arena-kiosk-svc/src/{secret.rs,winsec.rs,queue.rs,hub_link.rs,launcher.rs,procs/,lockdown/,ipc/}` -- reuse the DPAPI wrapper, queue, link, launcher and enforcement; don't duplicate them.

## Tasks & Acceptance

**Execution:**
- [x] Proto, store and core: the intents, tables, verifier building and history, the `cached_verifier` acceptance and flagging, with tests.
- [x] API: verifier push, PIN verify request, remote unlock push and ack, with tests.
- [x] kiosk-core: exit state machine and IPC; kiosk-svc: verifier store, online/offline paths, throttle, grant and re-lock with fakes, with tests.
- [x] Kiosk app panel and badge; Hub floor remote unlock with Playwright (stubbed ack and timeout); audit labels.
- [x] Catalogue keys, README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` plus the dev-fixtures runs, then every non-Windows matrix row passes. The Windows code compiles in CI.
- Given `pnpm check` and the Hub and kiosk Playwright suites, then all pass.

## Implementation Notes

- **Verifier history is derived metadata, not a change-log table.** `verifier_history(version, valid_from, digest)` and `hub_meta.verifier_counter` are written inside the same `apply()` transaction as the `branch.bootstrap` / `branch.seed_defaults` / `staff.upsert` / `role.upsert` that changed the list (SHA-256 digest over `staff_id, pin_hash, pin_verifier_version` of active staff with a PIN whose role allows `device.remote`, at most 50). Version = `max(pin_verifier_version over all staff) + counter`, floored at previous + 1 so it never goes down. The writer's Hub-start `seed_defaults()` also refreshes it (an upgraded branch gets its first version there). Replay does not rebuild it (a restored or replayed Hub treats exits before its first version as not stale).
- **`device_admin_exits`** is a change-log table (`Table::DeviceAdminExits`). `exit_id` is minted by the kiosk for `hub` / `cached_verifier` exits and by the writer for `remote`. `staff_id` has no FK (an owner actor can be the approver of a remote unlock). Relock is an update setting `relocked_at = issued_at`; only the kiosk that made the exit may relock it (else `PERMISSION_DENIED`).
- **apply()**: `IntentKind::elevator_permission()` (only `device.admin_exit` → `device.remote`) routes to `admin_exit::permission`: device actor (rbac), `elevated_by{pin, mode}` with mode matching the payload's (`hub`/`cached_verifier`; `remote` is refused), the elevator holds `device.remote` with a PIN. No `verifier_version` equality check (a mismatch flags instead). Decision: a manager who lost the permission since is accepted only on a `cached_verifier` exit whose verifier was already stale at `issued_at` (flagged), so an exit the kiosk legitimately granted offline is never lost from the audit; a `hub` exit by such a member is refused. Every other kind with `cached_verifier` is refused as before.
- **LAN**: `bind_to_session` keeps `elevated_by` only for `device.admin_exit`. New frames `kiosk.verify_admin_pin` (paired only; Debug redacts the PIN), `kiosk.admin_pin_ok`, `kiosk.admin_unlock_ack` (checked against the exit row: same device, mode `remote`) and a generic `ack` reply. `HubEvent::Verifier{verifier}` (after each paired hello, and from the fan-out after staff/role changes when the version changed) and `HubEvent::AdminUnlock{exit_id}` (from the fan-out on a `remote` insert, to that device's paired sessions only). Hub-side per-device throttle (5 wrong → 60 s `AUTH_LOCKED`) on top of the kiosk's; when the Hub pauses, the kiosk pauses too (it does not fall back to the offline check). `LanHub::with_staff` wires the `StaffDirectory` (the 1.13 `match_pin` helper). `verifier_version` in `kiosk.admin_pin_ok` is the manager-verifier version (not the member's `pin_verifier_version`), so hub and cached exits use one stale check.
- **`/ui/intent`** refuses `device.admin_unlock` for a device without a live paired session (`VALIDATION_FAILED` on `device_id`), so no exit is recorded that nobody can grant. The ack reaches `/ui/events` as `event: device.admin_unlock_ack {exit_id, device_id}`; the floor keeps acks for 60 s because one can arrive before the intent's HTTP answer.
- **kiosk-core**: `admin_exit` (Throttle 5/60 s, `ActiveExit`, `replaces`, `countdown`), IPC `admin_exit{rid, pin: SecretPin}` (redacted Debug, zeroed on clear/drop), `admin_exit_started{until}`, `admin_exit_ended`; status via `request{kind: admin_exit_status}`.
- **kiosk-svc**: `admin_exit` module; `Ctx.admin` (throttle, active exit watch, cached verifier, the live `HubClient` shared from `hub_link`) and `Ctx.launcher`. `AppLauncher::start_desktop/stop_desktop` (default: none); Windows `SessionLauncher` starts `%SystemRoot%\explorer.exe` as the player in its own kill-on-close job and ends the job at relock. The enforcer skips every check while an exit runs and, when it ends, closes any `explorer.exe` in the player's session (same safety rules and pid re-check) and sweeps. The running exit is recorded in `queue.db kv admin_exit`; any recorded exit re-locks at service start. Online-path failures other than `PIN_REJECTED`/`AUTH_LOCKED` (closed, `HUB_SLOW`, an older Hub) fall back to the offline check. New dependency: `argon2` (workspace pin) in `arena-kiosk-svc`.
- **Kiosk app**: the shell's `admin_exit` / `admin_exit_status` commands and `admin-exit` / `admin-exit-ended` events (`ipc-names.json`). During an exit the shell suspends the keyboard hook and the re-cover-on-focus-loss, and shrinks the same window into the non-topmost bottom-right badge (no second window); at the end it covers every monitor and goes topmost again.
- **Windows code** type-checked locally (`cargo clippy --target x86_64-pc-windows-msvc -p arena-kiosk-svc --all-targets --features dev-fixtures -- -D warnings` with the rustup 1.95 toolchain and stub `CC`/`AR`), not run on Windows.

## Spec Change Log

## Review Triage Log

| # | Source | Finding | Verdict | Route | Evidence |
|---|--------|---------|---------|-------|----------|
| 1 | blind, gap, edge | A stale `cached_verifier` exit accepts any staff id | high | patch | `permission()` checks only that the staff row exists and that the version is stale. Fixed: per-version staff ids in `verifier_history`. |
| 2 | blind, edge, gap-other | On reconnect only `AdminExitStarted` is sent, never `AdminExitEnded`, so the shell stays unlocked after a restart re-lock or a dropped Ended message | high | patch | `app.rs` Conn handler sends only `Some(e)`. |
| 3 | blind, edge | Concurrent PIN attempts bypass the 5-attempt / 60 s throttle (kiosk and Hub) | medium | patch | `gate()` / `paused_for` is checked before an awaited verification; failures are counted only afterwards. |
| 4 | edge | A grant while an exit is running re-locks and restarts, so the enforcer can kill the new Explorer; the remote exit_id is not validated | medium | patch | `grant` does not check `active`. |
| 5 | edge | A `start_desktop` error still grants the exit; a `kv_set` failure still grants it | medium | patch | In `grant`, `Ok(Err)` is only logged. |
| 6 | (verification run) | The runtime test assertion that "2222" never appears in the logs is flaky | medium | patch | It failed in the workspace run and passed 5 of 5 in isolation, so a timestamp or id collides. |
| 7 | gap | No backend test for the audit `flagged` column or the `admin_exit` detail | medium | patch | Only stubs and the `Names` unit test cover them. |
| 8 | gap | No test for the `device.admin_unlock_ack` SSE arm | medium | patch | grep shows no test reaches the arm. |
| 9 | gap | No test for resending `admin_exit_started` on reconnect | medium | patch | Grouped with #2's tests. |
| 10 | gap | No test runs the enforcer's exit-ended arm | medium | patch | The tests call `close_named` directly. |
| 11 | blind | The README says the PIN is "cleared right after the check" while clones and buffers remain | low | patch | `SecretPin: Clone`; IPC and JSON buffers. A wording fix. |
| 12 | blind | A `hub`-mode exit is accepted on the kiosk's word, with no proof bound to the PIN check | medium | defer | Inherent to AD-30 offline trust: a compromised kiosk can claim a cached exit anyway. A single-use ticket is a design change. |
| 13 | blind | The kiosk sets `issued_at`, which drives `started_at` and the stale check | medium | defer | Offline replay needs the kiosk's time (frozen intent). Bounding it against receive time needs a design decision. |
| 14 | blind | No reset when the Hub's verifier version goes down (restore or rebuild) | medium | defer | `replaces()` accepts only a strictly higher version. An epoch is needed. |
| 15 | blind, edge | A remote exit is recorded but never delivered when the session drops between the presence check and the push, and no reconciliation exists | low | defer | A narrow race; the Hub side needs a sweeper of expired exits. |
| 16 | blind | Offline verifier hashes are brute-forceable from a compromised kiosk | medium | defer | Mandated by the frozen intent; needs a PIN-strength or rotation policy. |
| 17 | blind | Only 4 intent kinds refresh the verifier | low | reject | Those are the only kinds that write staff, roles or PINs today. No named divergence. |
| 18 | blind | More than 50 managers are silently dropped | low | reject | The 50-entry cap is spec-mandated; it would only show up in unusual deployments. |
| 19 | blind | The kiosk ignores the Hub's lockout duration | low | reject | Both sides use 60 s. |
| 20 | edge | A failed `stop_desktop` is not logged | low | reject | The function returns `()`; negligible. |
| 21 | edge | Relock `issued_at` can be before `started_at` | low | reject | Needs a misbehaving clock; the fix adds a guard. |
| 22 | edge | Remote exits are recorded as `device.admin_unlock`, not `device.admin_exit` | false | reject | The frozen intent specifies that remote unlock submits `device.admin_unlock` with a `mode=remote` row. |
| 23 | edge | The webview's status reply does not call `on_admin_exit` | low | patch | Fixed by #2: the service sends its state on every connect. |
| 24 | gap-other | A restart re-lock does not fire the enforcer's close-Explorer arm | low | reject | The job's kill-on-close at the old service exit plus the sweep handle it. |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-kiosk-svc --features dev-fixtures && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub exec playwright test` -- expected: pass
