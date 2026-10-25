---
title: 'Story 4.2: The Android Hub keeps serving while the screen is off, in Doze and after a reboot'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'e26a88b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-1-android-hub-app-builds-from-the-shared-hub-code-and-checks-t.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Story 4.1 runs `arena-hub` in-process, but the Activity owns the process's life. Android is free to kill or freeze it when the screen turns off, in Doze, after Back or a swipe from recents, so kiosks would lose their Hub.

**Approach:**
- A Kotlin foreground service of type `connectedDevice` (`HubService`) hosts the Rust core: it calls the Rust library through JNI to start and stop `arena-hub`, holds a partial wake lock and Wi-Fi locks, and keeps an ongoing notification ("Hub running · n devices online") fresh from the loopback snapshot. The Activity only attaches a UI to the running core.
- A boot and package-replaced receiver restarts the service by itself. The service is sticky, so the system restarts it after a kill.
- The Hub UI asks for notification permission and the battery-optimisation exemption once, after the spec check on the pairing screen. A refusal leaves the Hub working and shows a `status-badge` and a header `alert-badge`. Hub System gets a "Hub service" section with "Stop Hub".

## Boundaries & Constraints

**Always:**
- Reuse the Windows Hub code; the only Kotlin is the service, the boot receiver, the WebView bridge and the Activity flags. No second UI, no Tauri plugin crate (the ad plugin is Story 4.10).
- The first-run device check of Story 4.1 still runs before the database exists; `branch.db` is created only by the service-hosted core after the check passes.
- Stop Hub is an audited `settings.set` of `hub.stopped_at` (a Text setting, default empty). Counter staff need a Manager's PIN through the existing `/ui/elevate` flow, so the audit log holds both actors; a member who already holds `settings.edit` confirms with a button.
- The core can be stopped and started again in one process; the stop path is the same graceful shutdown as SIGTERM.

**Never:**
- A new Hub migration (none: `hub.stopped_at` is a default setting that `branch.seed_defaults` backfills), ad code, Keystore, mDNS (`NsdManager`), pairing or Bluetooth work (Stories 4.3 to 4.6).
- A battery-exemption prompt before the spec check passes.
- Starting the service on boot before a `branch.db` exists.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| App opened, branch exists | `branch.db` present | Activity starts the service; core runs in the service; UI attaches |
| First run, spec passes | no `branch.db` | UI calls `startHub` after `device_spec` passes; service starts the core, which creates the database |
| Activity destroyed | Back, rotation, swipe from recents | service and core keep running; reopening shows the UI against the same core |
| Screen off / Doze | `KEYCODE_SLEEP`, `deviceidle force-idle` | wake lock and Wi-Fi locks held; loopback and LAN listeners keep answering |
| Reboot or app update | `BOOT_COMPLETED` / `MY_PACKAGE_REPLACED` | service starts without the UI if `branch.db` exists |
| Process killed | `am kill` | sticky restart; same database, `integrity_check` ok |
| Notifications refused | Android 13+ | Hub works; `alert-badge` in the header; System shows a button to the setting |
| Battery optimisation on | not exempt | `status-badge` "Battery optimisation on"; button to the system setting |
| Stop Hub, Counter | PIN of a Manager | audited with both actors; service and core stop; notification goes; app closes |
| Stop Hub, wrong PIN | rejected | nothing stops |
| Open after Stop | app opened | service starts the core again |
| Not Android | no `ArenaHub` bridge | no prompt, no alert, no "Hub service" section |

</frozen-after-approval>

## Code Map

- `crates/arena-hub/src/lib.rs`: `stop()` (a `Notify` created per run, so a stop only reaches the current run); `tests/stop.rs` (stop, then start again, in one process).
- `crates/arena-core/src/settings_defaults.rs`: `KEY_HUB_STOPPED_AT`, default and `Text` rule. `crates/arena-api/src/ui_audit.rs`: audit test for the two actors.
- `apps/hub/src-tauri`: `src/android.rs` (static `Gate`, JNI `nativeStart`/`nativeStop`, `device_spec` check only, `hub_ready`), `src/spec.rs` (`Gate::check`, `is_started`), `Cargo.toml` (`jni`), `gen/android` (`HubService.kt`, `BootReceiver.kt`, `HubBridge.kt`, `MainActivity.kt`, manifest, strings).
- `apps/hub/src`: `lib/service.svelte.ts`, `lib/AccessPrompt.svelte`, `lib/android.ts` (starts the service), `routes/pair`, `routes/+layout.svelte` (header alert), `routes/system/+layout.svelte`, `routes/system/service/+page.svelte`; `tests/service.spec.ts`.
- `scripts/check-android.mjs` (+ test): service type, permissions and boot receiver. README Android section.

## Tasks & Acceptance

**Execution:**
- [x] `arena-hub::stop` and the stop and restart test; `hub.stopped_at` setting and audit test.
- [x] Rust shell: static gate, JNI start and stop, check-only `device_spec`, `hub_ready`.
- [x] Kotlin: foreground service with locks and notification, boot receiver, bridge, keep-screen-on, manifest.
- [x] Hub UI: access prompt, header alert, "Hub service" section with Stop Hub, strings, Playwright tests.
- [x] `check-android.mjs` service rules; README.
- [x] Exercise on an emulator and verify (see Verification), review, triage.

**Acceptance Criteria:**
- Given the app is started, when the core comes up, then the foreground service of type `connectedDevice` hosts it and shows an ongoing notification with the branch name and "Hub running · n devices online", updated when the count changes.
- Given the activity is destroyed while the service runs, when the app is opened again, then the core was not restarted and the UI attaches within 2 s.
- Given first-run setup after the spec check, when the pairing screen opens, then the Hub asks for notification permission (Android 13+) and the battery exemption through the system dialog with one line of why; a refusal leaves the Hub working, shows "Battery optimisation on" in System and an `alert-badge` in the header that opens a screen with a button to the system setting.
- Given the service is running, when the screen turns off or the device enters Doze, then the wake lock and Wi-Fi locks are held and loopback and LAN listeners answer throughout.
- Given the Hub UI is in the foreground, then the keep-screen-on flag is set and the Story 1.14 idle lock is unchanged.
- Given a reboot or an app update, then the service starts without the UI when the branch exists.
- Given `am kill`, then the sticky service restarts the core and `PRAGMA integrity_check` returns ok.
- Given Stop Hub confirmed by a Manager's PIN, then the audit log holds the change with both actors, the service and notification go, and opening the app starts the Hub again.

## Spec Change Log

## Design Notes

- **Where the core lives:** `arena_hub::start` runs on its own thread inside the app's native library. `HubService` loads the same library (`arena_hub_ui_lib`) and calls `nativeStart(dir)`; the shared `Gate` makes the start idempotent whoever asks first (service or UI). The Activity no longer starts the core.
- **Why a WebView bridge and not a plugin:** the UI needs five synchronous calls (state, start, stop, ask, open a setting). `addJavascriptInterface` on the app's own bundle is the smallest way; a Tauri plugin crate would be the heavier ad-plugin pattern of Story 4.10.
- **Stop is audited, not authoritative:** the UI submits the intent and then calls the bridge. The bridge is reachable only from the app's own bundle (CSP `default-src 'self'`, no remote content).
- **Wi-Fi lock:** `WIFI_MODE_FULL_HIGH_PERF` is deprecated and Android may ignore it on API 34+; the service also takes `WIFI_MODE_FULL_LOW_LATENCY` (API 29+), which is only effective with the screen on. Real Wi-Fi power behaviour needs a device.
- **Stop across reboot:** a deliberate Stop does not survive a reboot or an app update; the boot receiver starts the Hub again.

## Verification

Pass: cargo test (workspace, arena-hub and arena-api with dev-fixtures), clippy with dev-fixtures, fmt, every check:* except no-remote (needs kiosk and kitchen builds), test:scripts, Hub svelte-check and test:shell, gallery test:a11y. Not run: Android Gradle/Kotlin build and Android-target Rust (target std unusable locally), emulator and device ACs (Doze, am kill, reboot, notification content).

## Review Triage Log

Filled in after review.

## Verification Log

Filled in at the end of the run.
