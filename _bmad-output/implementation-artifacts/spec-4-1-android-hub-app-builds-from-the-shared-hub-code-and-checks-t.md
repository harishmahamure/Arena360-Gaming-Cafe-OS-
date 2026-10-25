---
title: 'Story 4.1: Android Hub app builds from the shared Hub code and checks the device spec on first run'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'b736a8b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-2-app-skeletons-for-every-surface-build-in-ci.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Free cafés need the Hub on an Android tablet or phone, but the repo only builds the Hub for Windows. A device that is too small must be told so before it sets up and fails later.

**Approach:**
- `apps/hub` becomes a Tauri 2 mobile app: the same SvelteKit UI, and `arena-hub` run in-process on its own Tokio thread. The Gradle project is committed with the stack pins (targetSdk 36, minSdk 28, NDK 28+, Kotlin 2.1+), and CI builds a debug APK and a release AAB.
- The shell checks RAM and free storage before anything creates the branch database, and the UI shows one plain screen when a requirement fails. An old Android System WebView gets its own screen.
- Build-time checks (pin table, backup exclusions, 16 KB ELF alignment, ABIs, no keystore in the repo) fail CI.

## Boundaries & Constraints

**Always:**
- Reuse the Windows Hub code: no second UI, no Kotlin screens. Strings come from the message catalogue.
- Device floor: 3 GB RAM, 20 GB free on the app data volume. The check runs only while `branch.db` does not exist and never creates it.
- `/healthz` reports `platform` (`windows` or `android`); the default platform follows the build target. `--platform android` stays a `dev-fixtures` flag.
- Android auto-backup and device transfer exclude the app's private data; cleartext is allowed to loopback only.

**Never:**
- A keystore or key file in the repo; ad code; the foreground service, Keystore, mDNS or pairing work (Stories 4.2 to 4.4).
- A new Hub migration (none needed).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Meets spec | first run, RAM and storage at or above the floor | Hub starts in-process, "Pair this Hub" |
| Too little RAM or storage | first run, below the floor | one screen per failed requirement with the value found, "Check again", no Hub, no `branch.db` |
| Check again | device now passes | Hub starts, pairing screen, once only |
| Existing branch | `branch.db` exists | no check; the Hub starts (free space dipping later never bricks a café) |
| Probe unreadable | `/proc/meminfo` or `statvfs` fails | Hub starts and the shell logs it |
| Old WebView | Chrome major below 111 | "Update Android System WebView" screen with "Check again"; the Rust core is running |
| Android 8.1 or older | install | refused by minSdk 28 |
| Windows or browser | no Android user agent | no change |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/device_spec.rs`: floor constants, `check_device`, `meminfo_total_bytes`, generated TS types.
- `crates/arena-hub`: `src/lib.rs` (`start`, was `main`), thin `src/main.rs`, `config::DEFAULT_PLATFORM`; `arena-core` `HubStatus.platform`; `arena-api` `/healthz` `platform`.
- `apps/hub/src-tauri`: `lib.rs` (`run`, mobile entry point), `spec.rs` (probes and the first-run `Gate`), `android.rs` (in-process Hub, `device_spec` command), `gen/android` (Gradle project, manifest, backup and network rules), `tauri.conf.json` (`minSdkVersion`).
- `apps/hub/src`: `lib/android.ts`, `lib/DeviceGate.svelte`, `routes/+layout.js` and `+layout.svelte`; `tests/android.spec.ts`.
- `scripts/check-android.mjs` (manifest rules, AAB inspection), `check-pins.mjs` (Android pin readers), `check-secrets.mjs` (keystore files); `.cargo/config.toml` (16 KB link); `.github/workflows/ci.yml` (`android` job); `README.md` (pin rows, Android Hub section).

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto`: floor, check and meminfo parsing with unit tests.
- [x] `arena-hub` lib split, `platform` in `/healthz`, default platform per target; tests.
- [x] Tauri shell lib, spec probes, first-run gate, in-process Hub, Android project with pins, backup and network rules, signing from `keystore.properties`.
- [x] Hub UI: device gate screens, strings, Playwright tests with axe.
- [x] `check:pins` Android rows, `check-android.mjs`, keystore secret rule, CI `android` job, README.
- [x] Verify (see Verification), review, triage.

**Acceptance Criteria:**
- Given the monorepo, when the CI Android job runs, then it builds a debug APK and a release AAB from the Hub SvelteKit front end and the `arena-hub` crates, for `arm64-v8a` and `x86_64`; the AAB is signed when `deploy/secrets/android-upload.enc.yaml` exists and is built unsigned, with a warning, when it does not.
- Given a change to targetSdk, minSdk, compileSdk, Kotlin, the NDK or the WebView minimum, when `pnpm run check:pins` runs without a README pin-table update, then it fails.
- Given the release AAB, when `check-android.mjs aab` runs, then it fails on any native library not 16 KB aligned or on a missing required ABI.
- Given the app on Android, when it starts, then `arena-hub` runs in-process on its own Tokio thread with the Story 1.6 store settings, and `/healthz` reports `platform: android`.
- Given a device under 3 GB RAM or 20 GB free, when the app first opens, then one screen names each failed requirement and the value found with "Check again", before the pairing screen, and `branch.db` is not created; a device that passes goes to "Pair this Hub".
- Given the manifest, when `check-android.mjs manifest` runs, then it passes only with `allowBackup="false"`, cloud-backup, device-transfer and full-backup rules excluding the file, database and sharedpref domains, and cleartext limited to loopback.
- Given an Android System WebView older than the README minimum, when the UI loads, then a plain screen asks the user to update it and the core keeps running.
- Given the message-catalogue lint and the axe scan, when they run on the Hub UI, then they pass and every new string is in the catalogue.

## Spec Change Log

## Design Notes

- **RAM floor:** Android reports `MemTotal` below the marketed size (a 3 GB phone reports about 2.6 to 2.9 GB), so the check passes from 2.5 GB reported and the screen says "3 GB". Storage is 20 GB decimal of `f_bavail * f_frsize`.
- **First run only:** the check is skipped once `branch.db` exists, so a running café is never locked out because free space dipped.
- **Fail open on an unreadable probe**, with a log line: blocking a good device with an unexplained screen is worse than a missed check.
- **Signing:** CI reads the upload key from SOPS. Until an operator adds `deploy/secrets/android-upload.enc.yaml` and pins `SOPS_SHA256` in `ci.yml`, the AAB is unsigned. That is stated, not hidden.
- **Cleartext:** the Hub UI reads `http://127.0.0.1:7409` from the WebView, so a network security config allows cleartext to loopback only.

## Verification

**Commands:** see the Verification Log below.

**Exercised locally on an arm64 Android 16 emulator (2 GB RAM, 6 GB data partition, NDK 27, debug APK, `aarch64` only):**
- The debug APK builds from the shared UI and `arena-hub` crates; `aapt2` reports targetSdk 36 and the arm64 library passes `check-android.mjs` 16 KB alignment.
- On first run the storage check fails ("This device has 5 GB free. The Hub needs 20 GB free.") with "Check again", and no `branch.db` exists afterwards.
- With a `branch.db` present the in-process Hub starts, `/healthz` answers `platform: android`, `role: primary`, and the UI shows "Pair this Hub". Running it found two Android-only defects, both fixed: `File::try_lock` is unsupported on Android (the writer lock now uses `flock`), and a synchronous command blocked the main thread (ANR).

**Not exercised (no passing device image fits the free disk, NDK 28 is not installed locally):** the first-run PASS path through the real probes (unit-tested through `Gate` only), RAM failure on a device, the Old-WebView screen on a device (Playwright only), `x86_64`, NDK 28, the release AAB and its 16 KB inspection, the signing and SOPS step, the CI job itself, and the minSdk install refusal. None of this is claimed as tested.

## Review Triage Log

Two reviewers on Sonnet: Blind Hunter (9 findings) and Verification Gap (20).

**Patched:**
- Windows CI would not compile `mod spec` (unix-only probes): gated to `cfg(any(all(test, unix), target_os = "android"))`.
- A Hub that exits stayed marked started, so "Check again" never restarted it: the in-process thread now clears the gate on exit, and a unit test covers the restart.
- Every layout reload blocked up to 10 s waiting for a dead Hub: the wait now happens only on the call that starts it.
- An erroring shell skipped the WebView check: it now falls through; test added.
- `tracing_subscriber::init()` could panic when `arena_hub::start` runs inside another process: `try_init()`.
- SOPS step: command substitutions inside `echo` hid sops failures and empty values: values are read first and an empty one fails the step.
- `check-android.mjs`: exclude rules now parse attributes in any order and require all five domains (root, file, database, sharedpref, external); tests added.
- `keystore.properties` and `key.properties` join the keystore files `check:secrets` refuses.
- `flock` fallback made testable on any unix host: unit test for exclusion and release.
- A Linux-only test reads the real `/proc/meminfo` and `statvfs`.
- Spec Verification text now states what ran on the emulator and what did not.

**Not a defect or already covered:**
- "Real `__TAURI__` global may be missing, failing open": the emulator showed the failing-storage screen through the real `invoke`.
- "Unsigned AAB on master": stated in the spec and the CI warning; signing needs the operator's key file and `SOPS_SHA256`.
- "Pin readers can return `{}`": a README row with no source value already fails `check:pins` (extra pin row).
- Pin of AGP, Gradle wrapper and JDK: Tauri generates them; they are not a story pin.

**Deferred** (in `deferred-work.md`): Android keystore (Story 4.3), FileProvider paths, merged-manifest check on the built APK, WebView below Chrome 87 never running the bundle, a delayed-invoke "Checking" test, `aapt2` and ABI checks of the debug APK in CI, running the first-run pass path on a device image with 20 GB free.

## Verification Log

- `cargo fmt --check`: ok. `cargo clippy --locked --workspace --all-targets -- -D warnings`: ok. Android: `cargo clippy --locked --target aarch64-linux-android -p arena-hub-ui --lib -p arena-store -- -D warnings`: ok.
- `cargo test --locked --workspace --no-fail-fast`: everything ok except `arena-kiosk-svc` `wrong_and_counter_pins_are_refused_and_five_pause_across_an_app_restart` (`tests/runtime.rs:265`, an index on a missing `device_events` row), in two workspace runs; the same test and the whole `arena-kiosk-svc` suite pass alone. An earlier plain `cargo test --workspace` run also failed `arena-api` `lan` `the_hub_checks_admin_exit_pins_and_pauses_after_five_wrong` (`tests/lan.rs:1391`); it passes alone and in the second full run. Load flakes in code this story does not touch.
- `cargo test --locked -p arena-hub -p arena-api --features dev-fixtures` and clippy with dev-fixtures: ok.
- Playwright Hub suite (`pnpm --filter @arena/hub test:shell`, `ARENA_PW_PORT=5445`), final full run: 420 passed, 6 skipped, 4 failed (`audit` 347 desktop, `audit` 542 desktop, `devices-display` 303 phone, `shell` 391 phone). All four pass when rerun alone (7 passed). The first full run failed `devices-display` 303 (desktop), `tax` 212 (desktop) and `auth` 219 (phone); on rerun `tax` and `auth` passed and `devices-display` 303 failed once more alone and then passed; it passes on the baseline commit too. A different set fails each run under load; `android.spec.ts` passed 22 of 22 every time.
- Every `pnpm run check:*`, `test:scripts` (200 tests), prettier and `svelte-check`: ok.
- Not checked: a Windows build of `arena-hub-ui` (no Windows linker or SDK on this machine; the Windows CI job is the check).
