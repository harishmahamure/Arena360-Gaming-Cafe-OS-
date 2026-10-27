---
title: 'Story 4.7: The Android Hub saves backups to a folder the owner chooses'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: '87c4a592892fc9db6aebd9e87fc1b57569d7c6b9'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-26-branch-operating-settings-opening-hours-fulfilment-kiosk-vol.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-6-the-android-hub-prints-over-bluetooth-or-lan.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The backup location (Story 2.26) is a typed path checked with `std::fs`. On Android the system folder picker returns a `content://` document tree, not a path, so a Free café on Android cannot finish the backup-location step of go-live, and nothing notices when a USB OTG drive is unplugged.

**Approach:** `system.backup_dir` may hold a tree URI. A Kotlin `BackupFolderBridge` (reached from Rust through `backup_jni.rs` as an `arena_core::setup::BackupFolder`) probes it with `DocumentsContract`; `MainActivity` runs the picker and keeps the grant. `watch_backup_folder` rechecks at start, on settings changes and hourly and makes setup status report the location as not set while the folder is unavailable. Settings → System on Android shows a Choose folder button (PIN first for a Counter) instead of a path field.

## Boundaries & Constraints

**Always:**
- The Windows Hub, its typed path, Browse and tests behave as before.
- No storage permission is requested; the folder is a persisted document-tree grant.
- A picked folder is saved only after the probe file is created and deleted in it; the reason is shown inline.
- An unavailable folder keeps the setting, flips `setup.backup_location_set` to false, and recovers on the next good check.
- A Counter changes the folder only with a manager's ticket; the intent then carries both actors.
- Every new string lives in `en.json`.

**Never:**
- Writing backups (Story 9.4/9.5), reading from the folder, or a second backup-location UI.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Pick writable folder | Manager, Android | grant kept, probe ok, saved, setup `backup_location_set` true, audited with staff id |
| Pick read-only / gone folder | Android | `VALIDATION_FAILED` reason inline, nothing saved, no grant change |
| Picker cancelled | Android | nothing sent |
| Drive unplugged / grant revoked | Hub start or hourly | setup status not set; System shows "Backup folder not available" + Choose again |
| Drive back / chosen again | any | next check (or the save) restores set |
| Counter | no `settings.edit` | `inline-pin` first, picker after approval, ticket on the intent |
| Windows Hub | any | typed path + Browse, no change |

</intent-contract>

## Code Map

- `crates/arena-proto/src/operating.rs`, `lib.rs` -- `is_backup_tree_uri`, `is_backup_location`.
- `crates/arena-core/src/setup.rs`, `snapshot.rs`, `settings_defaults.rs` -- `BackupFolder`, `probe_location`, `watch_backup_folder`, `SnapshotReader` availability flag and `setting_value`, `Path` rule accepts a tree URI.
- `crates/arena-api/src/ui_auth.rs` -- `with_backup_folder`; the save probe goes to the folder for a tree URI.
- `crates/arena-hub/src/lib.rs` -- `StartOptions.backup_folder`, hourly watch task.
- `apps/hub/src-tauri/src/backup_jni.rs`, `android.rs`, `lib.rs`; Kotlin `BackupFolderBridge.kt`, `HubBridge.kt`, `MainActivity.kt`, `HubService.kt`.
- `apps/hub/src/routes/settings/system/+page.svelte`, `lib/operating.ts`, `lib/service.svelte.ts`, `en.json`.
- `scripts/check-android.mjs` (+ test) -- backup folder rules.

## Tasks & Acceptance

**Execution:**
- [x] Protocol helpers and settings rule for a tree URI.
- [x] `BackupFolder`, `probe_location`, availability watch and snapshot overlay with tests.
- [x] API save probe for a tree URI; Hub wiring; tests with a scripted folder.
- [x] Kotlin bridge, picker, JNI, shell wiring.
- [x] Settings → System Android UI (choose, unavailable note, PIN for a Counter); Playwright tests.
- [x] `check-android.mjs` backup rules and tests; README; `deferred-work.md`.

**Acceptance Criteria:** those of Story 4.7 in `epics.md`. Device-only criteria (real picker, USB OTG unplug, persistent grant across reboot) cannot run here and are recorded as not run.

## Verification

**Commands:**
- `cargo test --locked -p arena-proto -p arena-core -p arena-api --lib`, `cargo clippy --locked -p arena-proto -p arena-core -p arena-api -p arena-hub -p arena-hub-ui --all-targets -- -D warnings`, `cargo fmt --check`.
- `pnpm --filter @arena/hub check`, Playwright `operating`, `pnpm run check:strings`, `check:protocol`, `check:layers`, `check:pins`, `node scripts/check-android.mjs manifest`, `node --test scripts/check-android.test.mjs`.

## Verification Log

Pass: Rust lib tests for arena-proto (169), arena-core (666), arena-api (233), clippy with `-D warnings` on the touched crates and `arena-hub-ui`, `cargo fmt --check`, `svelte-check` (0 errors), Playwright `operating` on desktop and phone (36), `check:strings`, `check:protocol`, `check:layers`, `check:pins`, `check-android` manifest check and its tests (29), prettier on touched files. Not run: Gradle/Kotlin build, `backup_jni.rs` on the Android target and every device-only criterion.
