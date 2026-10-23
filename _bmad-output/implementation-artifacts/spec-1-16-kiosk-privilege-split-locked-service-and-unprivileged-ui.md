---
title: 'Story 1.16: Kiosk privilege split — locked service and unprivileged UI'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '9cd8909cda467dcc22dee5f0b0778915689b0845'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-9-lan-protocol-envelope-hello-handshake-and-client-classes.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The kiosk is still a placeholder Tauri app. Nothing enforces anything, there is no protected place for the device token, and no channel separates the privileged parts from the player-facing UI.

**Approach:**
- **`arena-kiosk-svc`:** a LocalSystem Windows service that holds the device token (DPAPI) and the pinned-TLS Hub connection (the 1.9 `HubClient` with jittered reconnect), plus an offline intent queue.
- **IPC:** the unprivileged Tauri app (`arena-kiosk-ui`) talks only to the service, over an ACL'd named pipe. A watchdog restarts the app when it stops answering pings.
- **Installer:** checks the OS, architecture and RAM, registers the service and bundles the WebView2 bootstrapper.
- **Dev mode:** a cross-platform console mode and a Unix-socket transport, so the logic is testable on CI's Linux and macOS.

## Boundaries & Constraints

**Always:**
- **Crate split:**
  - `arena-kiosk-core` (proto only) gains the platform-neutral logic:
    - **reconnect policy:** `Backoff`, jittered full-jitter exponential, min 2 s, max 30 s, reset on a successful hello;
    - **IPC message types:** `IpcMsg { Ping{n}, Pong{n}, HubStatus{state: connected|reconnecting|token_rejected}, AppState{json}, RestoreState{json}, Request{rid, kind, body}, Reply{rid, ...} }` as JSON lines;
    - **watchdog state machine:** 2 s ping, 3 misses → restart, pure, driven by an injected clock;
    - **queue ordering rules.**
  - `arena-kiosk-svc` holds the OS-specific parts behind traits: `SecretStore`, `IpcServer`, `AppLauncher`, `ServiceHost`.
- **Secret store:**
  - Windows: `CryptProtectData`/`CryptUnprotectData` with machine scope (`CRYPTPROTECT_LOCAL_MACHINE`) through the `windows` crate (exact pin, README row). The blob lives at `%PROGRAMDATA%\Arena360\Kiosk\token.bin`.
  - Non-Windows (dev and tests only): a 0600 file plus a `warn` log line saying it is not encrypted.
  - Config lives in `config.json` in the same directory: `{hub_addr, hub_fingerprint, device_id}`. The token never goes in it.
- **Dev provisioning:** `arena-kiosk-svc dev provision --hub <host:7400> --fingerprint <hex> --token <token>` (feature `dev-fixtures`). It writes the config and the encrypted token. Real pairing is Epic 2.
- **Hub connection:**
  - The service connects with `HubClient` using the pinned fingerprint and sends `hello{class: paired, versions: {app: <crate version>}}`.
  - A fingerprint mismatch is refused and logged at `error` with both fingerprints. It never falls back to an unpinned connection.
  - **`TOKEN_REVOKED`/`TOKEN_INVALID`:** stop reconnecting, report `HubStatus{token_rejected}` and keep retrying every 30 s. A re-provision is needed to recover.
  - **Unreachable:** `Backoff` reconnects, and the app is told `reconnecting`.
- **Hub side:**
  - `HubSnapshot.devices[]` gains `online: bool`, true when that device has an active paired session in the registry. The value is read through a registry handle given to the `/ui/snapshot` handler.
  - The Hub floor shows "Online" or "Offline" as text next to each device (catalogue).
- **Offline intent queue:**
  - New intent kind `kiosk.log{level: info|warn|error, code, detail}` (requirement `Device`, added to the paired scope). It writes a new `device_events(event_id PK, branch_id, device_id, at, level, code, detail)` table through `apply()`, with change rows (migration).
  - The service persists queued intents in its own SQLite file (`queue.db`, rusqlite): each event's ULID idempotency key and `issued_at` are set when the event is created.
  - On connect it replays the queue oldest-first, one at a time, and deletes each entry on `Applied` or on a deterministic rejection. Transient errors stop the replay until the next connect.
  - Entries older than the 7-day TTL are dropped with a `warn` log line.
  - Dedupe (1.8) guarantees exactly-once application.
- **IPC:**
  - Windows: named pipe `\\.\pipe\arena-kiosk-<branch-agnostic constant>`, created with a security descriptor allowing only SYSTEM and the configured kiosk user (config `player_user`, default `player`). Anything else is denied, which the security-descriptor builder tests check.
  - Non-Windows: a Unix socket at `<data>/kiosk.sock`, mode 0600.
  - The app side (`arena-kiosk-ui`, Rust): connects, answers `Ping` with `Pong`, sends `AppState` snapshots (a JSON blob the UI provides) whenever it changes, and forwards `HubStatus` to the webview as a Tauri event. The webview has no Hub socket, no token and no admin rights.
  - The pipe accepts one app connection at a time, and the newest connection wins.
- **Watchdog:**
  - The service pings every 2 s. After 3 consecutive missed pongs, `AppLauncher::restart()` kills the app process and relaunches it.
    - Windows: in the active console session via `WTSGetActiveConsoleSessionId` + `WTSQueryUserToken` + `CreateProcessAsUserW`.
    - Dev/test: a mock launcher.
  - On reconnect the service sends `RestoreState{last AppState}`.
  - The kiosk window itself covers the screen fully. The guarantee that the desktop is never exposed while the app restarts comes from Story 1.17's shell replacement (no Explorer), documented in the README.
- **Kiosk UI (`apps/kiosk`):**
  - Shows a full-screen placeholder plus the catalogue "Reconnecting…" line while `HubStatus=reconnecting`. The line is non-blocking, in a status live region.
  - It reports its route as `AppState` and restores it on `RestoreState`.
- **Service host:**
  - `arena-kiosk-svc` runs `service install|uninstall|run` on Windows (windows-service 0.8 with LocalSystem and auto-start) and `--console` everywhere.
  - Logs are tracing JSON in `<data>/logs`, with the same 14 d / 50 MB ring as the Hub (reuse or extract the 1.6 ring into a shared helper crate only if the layer table allows; otherwise copy behind a small module).
- **Installer (CI, windows-latest):**
  - The Kiosk NSIS build uses `webviewInstallMode: {type: "embedBootstrapper"}` and the service exe as an installer-only `externalBin` (the 1.10 pattern).
  - The hooks abort on Windows build < 17134, on a non-64-bit OS, or on < 4 GB RAM (`GlobalMemoryStatusEx` via System plugin, or a PowerShell call), each with a message naming the requirement. The strings are duplicated from the catalogue with a key comment.
  - After install the hooks run `arena-kiosk-svc service install`. Before an upgrade or uninstall they stop and uninstall it.
  - It joins `ci-ok`.
- **Benchmark:** `release-bench.yml` gains a kiosk job and `scripts/bench/kiosk-idle.ps1`: service plus app plus WebView2 working set < 80 MB idle with no session, and GPU 0.

**Never:**
- Lockdown, shell replacement or the process allow-list (1.17–1.18).
- Admin exit (1.20).
- Pairing or mDNS (Epic 2).
- The app talking to the Hub directly.
- Storing the token unencrypted on Windows.
- An unpinned TLS fallback.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Connect | provisioned svc and a running seeded Hub | hello.ok; the Hub snapshot shows the device `online: true` | N/A |
| Pin mismatch | a wrong fingerprint in the config | refused; an error log line with both fingerprints; no fallback | backoff retry |
| Hub down | the Hub stops | the app gets `reconnecting`; retries are spaced 2–30 s with jitter | N/A |
| Hub back | the Hub restarts | reconnects; `connected` | N/A |
| Revoked | the device is revoked on the Hub | `token_rejected`; the svc stops fast reconnects | N/A |
| Queue offline | 3 `kiosk.log` events while the Hub is down | persisted in `queue.db`; on reconnect applied in creation order, once each | N/A |
| Duplicate replay | a crash after Applied but before the local delete | the replay returns the stored outcome; no duplicate row | N/A |
| Watchdog | the app stops answering for 3 pings | the launcher restarts it; RestoreState is sent with the last AppState | N/A |
| Pipe ACL | the SD builder for user `player` | an allow ACE for SYSTEM and player only; no Everyone or Users | N/A |
| Backoff | consecutive failures | delays stay within [2 s, 30 s]; reset after success | N/A |
| Installer | 32-bit, < 4 GB, or build < 17134 | aborts with the requirement message (Windows only; not run in CI) | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-kiosk-core/src/client.rs` (1.9) -- `HubClient` (pinned TLS, HubSlow, events, keepalive). Reuse it, and add the backoff, IPC types and watchdog modules. Layer rule: kiosk-core → proto only.
- `crates/arena-kiosk-svc/src/main.rs` -- the placeholder bin. Build out the service.
- `crates/arena-hub/src/logs.rs` -- the ring log, for the svc to reuse or mirror.
- `crates/arena-api/src/lan/registry.rs` -- the sessions per device. Expose an `online(device_id)` query for the snapshot.
- `crates/arena-proto/src/{intent.rs,scope.rs}`, `crates/arena-core/src/apply/*`, `crates/arena-store` -- the `kiosk.log` intent, its scope and the `device_events` migration.
- `apps/kiosk/{src/**,src-tauri/**}` -- the placeholder app and Tauri crate `arena-kiosk-ui`. Add the IPC client and the UI status.
- `apps/hub/src-tauri/{tauri.installer.conf.json,windows/hooks.nsh}`, `.github/workflows/{ci.yml,release-bench.yml}` -- the installer patterns from 1.10.

## Tasks & Acceptance

**Execution:**
- [x] `arena-kiosk-core`: backoff, ipc, watchdog (+ unit tests with an injected clock).
- [x] `arena-proto`, `arena-store`, `arena-core`: `kiosk.log`, `device_events` and the scope (+ tests); `arena-api`: snapshot `online` (+ test).
- [x] `arena-kiosk-svc`: secret store (DPAPI + dev file), config, queue.db, the hub connection loop, the IPC server (pipe + Unix socket), the launcher trait, the service host, the dev provision command (+ integration test on Unix: a real Hub from `arena-hub` test helpers, or an in-process `arena-api` server, covering connect, online, Hub down/up, queue replay order and once, revoked, watchdog with a mock launcher).
- [x] `apps/kiosk`: Tauri IPC client, reconnect status, AppState/RestoreState; Playwright test of the status line with a stubbed event.
- [x] Installer conf and hooks, CI installer job, bench script, README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` (+ dev-fixtures steps) on Linux and macOS, then every non-installer matrix row is covered and passes. Windows-only code compiles in CI's Windows job.
- Given `pnpm check` and the kiosk UI tests, then both pass.

## Implementation Notes

- **Shared types.** `KioskHubState`, `KioskHubStatus` (the webview's `hub-status` event payload), `KioskLog` and `KioskLogLevel` live in `arena-proto` (generated TS), so the kiosk UI uses no hand-written protocol types. `IpcMsg` is in `arena-kiosk-core::ipc` (serde-tagged `type`, snake_case; 64 KiB line cap; a bad or over-long line is skipped, the connection stays up).
- **`kiosk.log`.** New `IntentCategory::Kiosk`; requirement `Device` (the permission step already refuses an unknown or revoked device actor); in `PAIRED_SCOPE` only. `code` is 1–64 of `[a-z0-9_.-]`, `detail` ≤ 500 chars without control characters. Migration 0009 adds `device_events` with the spec's columns plus `rev/created_at/updated_at` (every change-log table is stamped); `at` = the intent's `issued_at`. `Table::DeviceEvents` is in the replay allow-list; the replay test seeds one row.
- **Presence.** `arena_api::lan::Presence` (from `LanHub::presence()`) reads the registry: online = a live, not-closing `paired` session for that device. `ui_router` takes it and fills `online` on `/ui/snapshot`. Beyond the spec: opening/closing a paired session pushes `devices.changed` on `/ui/events` (`UiAuth::with_presence`), and the Hub layout reloads the snapshot, so the floor's Online/Offline is live instead of stale until a reload.
- **Pin mismatch.** `HubClient`'s verifier keeps the presented fingerprint; `connect_ws` returns the new `ClientError::PinMismatch{expected, presented}` (the two existing pin tests now expect it), and the service logs both at `error`.
- **Hub link details.** The config and token are re-read on every attempt, so a re-provision recovers a `token_rejected` kiosk without a restart. `token_rejected` is sticky until a hello succeeds (a later unreachable Hub keeps the 30 s pace). Not provisioned: `reconnecting`, polled every 30 s. A transient replay stop resumes on the next connect or after 30 s on the same connection (so a long `NOT_PRIMARY` connection does not strand new events). Hub pushes are drained and ignored for now.
- **Watchdog.** Pure `Watchdog` in kiosk-core; a tick with no app connected also counts as a miss (a crashed app that never reconnects is restarted), with a 10 s grace after start/restart. On restart the current connection is closed and `kiosk.log{warn, app_restarted}` queued. Windows launcher: `taskkill /F /IM arena-kiosk-ui.exe`, then `WTSGetActiveConsoleSessionId` + `WTSQueryUserToken` + `CreateEnvironmentBlock` + `CreateProcessAsUserW` on `winsta0\default`. Console mode: `--app <path>` child process, else a no-op launcher.
- **Pipe.** `\\.\pipe\arena-kiosk-v1`; SDDL `D:P(A;;GA;;;SY)(A;;GRGW;;;<player SID>)` from `LookupAccountNameW`; first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE`, remote clients rejected; the next instance is created before handing one off. An unresolvable player account gives a SYSTEM-only pipe (logged). The player SID must be `S-1-…` (aliases such as `WD` are refused by the builder).
- **Logs.** The Hub ring is copied to `arena-kiosk-svc/src/logs.rs` (prefix `kiosk-`, own `Clock` trait): the layer table gives kiosk crates no path to `arena-core`.
- **Service host.** `windows-service =0.8.1`, `windows =0.62.2` (already in the lock via Tauri), both pinned with README rows. Install: auto-start, LocalSystem, restart-on-failure ×3 (5 s), args `service run`. Uninstall stops (≤15 s) then deletes; not installed is fine.
- **Installer.** Per-machine NSIS (a service needs admin); RAM via `GetPhysicallyInstalledSystemMemory` (KB of installed RAM, so a real 4 GB machine is not refused for OS-reserved memory); 64-bit via `x64.nsh` `${RunningX64}`. Hook texts are catalogue keys `kiosk.installer.*` checked by `scripts/kiosk-installer.test.mjs`.
- **Kiosk UI.** The restorable state is `{route: path + query}`; a restored route must be a same-origin path. The status line is a polite `role=status` region with the `reconnecting-indicator` pill (top-right, `pointer-events: none`). Playwright stubs `window.__TAURI_INTERNALS__` to drive the real `@tauri-apps/api` event path. Capabilities allow only `core:event:allow-listen/unlisten`.
- **Windows compile check locally:** the rustup 1.95 toolchain with the msvc target and a stub `CC`/`AR` for the C build scripts (`cargo check/clippy --target x86_64-pc-windows-msvc -p arena-kiosk-svc --all-targets --features dev-fixtures`) type-checks the Windows code; it does not link or run it.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Player can decrypt the token: machine-scope DPAPI plus a world-readable ProgramData folder (blind, edge) | high | The data-dir ACL was deferred, but the token protection depends on it. | patch (protected DACL + DPAPI entropy) |
| 2 | Player can plant `config.json` or `config.tmp` and redirect the Hub (blind) | high | Inherited Users-create rights on a predictable temp path. | patch |
| 3 | Pipe ACE `GW` includes FILE_CREATE_PIPE_INSTANCE, so a player can squat the pipe (blind, edge) | high | 0x4 bit. | patch (explicit mask) |
| 4 | Neither pipe end checks the peer process or owner | medium | A player process can kick the app off or feed it fake status. | patch |
| 5 | Player SID resolution accepts groups and aliases; the forbidden list holds aliases, not numeric SIDs (edge) | medium | Administrators and INTERACTIVE pass. | patch |
| 6 | Watchdog pongs come from the Rust shell, so a hung webview goes undetected | medium | The JS thread isn't involved. | patch (webview heartbeat) |
| 7 | Restart loop every ~14 s with no session, each logging to the Hub | medium | Floods queue.db and `device_events`. | patch (back-off, no counting without a session) |
| 8 | Player SID resolved once; config changes need a service restart (blind, edge) | medium | Pipe stays SYSTEM-only after a late provision. | patch (rebind on change) |
| 9 | Queue deletes on Unsupported/BadFrame and on clock-skew refusals (blind, edge) | medium | A Hub/kiosk version gap or a fast clock loses events. | patch |
| 10 | Launcher starts the app as whoever is at the console; taskkill leaks WebView2 children | low | Admin at console loops; leaked processes. | patch (job object, user check) |
| 11a | `kiosk.log` has no rate limit; the app can flood the queue (blind, edge) | medium | Unbounded. | patch (per-device rate + queue cap + validation) |
| 11b | `device_events` has no retention | low | Retention policy belongs with the data-lifecycle epic. | defer |
| 12 | Stale RestoreState applied on reload or IPC reconnect; state report race and drop (blind, edge) | low | bridge.rs logic. | patch |
| 13 | Release bench runs the app as the runner user | low | The measured figure is wrong. | patch |
| 14 | No Windows test of the pipe ACL, squat, round-trip or launcher (blind, verify-gap) | medium | Only SDDL parse and DPAPI. | patch (Windows test module) |
| 15 | Replay retry timer reset each loop, so it never fires under steady traffic | medium | Edge reviewer traced it. | patch |
| 16 | Service install/uninstall error paths (exists, marked for delete, partial config, access denied, stop timeout) | medium | Silent failures. | patch |
| 17 | Unix socket mode race; unbracketed IPv6 `hub_addr` | low | Direct fixes. | patch |
| 18 | `devices.changed` UI refetch and Hub wiring untested (verify-gap) | low | Pre-verified gaps. | patch |
| 19 | Transient refusal and re-provision recovery untested (verify-gap) | low | Pre-verified gaps. | patch |
| 20 | Rust and TS event/command names not pinned (verify-gap) | low | Silent mismatch. | patch |
| 21 | Installer CI does no install smoke test (verify-gap, deferred by reviewer) | low | Same convention as the Hub installer; do both together. | defer |
| 22 | Dev socket path differs between `--data-dir` and the app | low | Direct fix. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-kiosk-svc --features dev-fixtures && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `cargo check --locked --target x86_64-pc-windows-msvc -p arena-kiosk-svc` -- expected: compiles where the target is installed (otherwise verified by the CI Windows job)
- `pnpm check` -- expected: pass
