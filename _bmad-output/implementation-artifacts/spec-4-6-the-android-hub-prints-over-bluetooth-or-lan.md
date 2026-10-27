---
title: 'Story 4.6: The Android Hub prints over Bluetooth or LAN'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: 'a3d73fdacdbc06b1497b793d50f27ba63e4b774f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-27-printer-detection-and-test-print-at-the-hub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-4-kiosks-find-and-pair-with-an-android-hub-on-the-lan.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub's printer adapter (Story 2.27) knows USB through the Windows spooler and LAN `:9100`. An Android Hub has no spooler, so a Free café on Android cannot finish the printer step.

**Approach:** Add a Bluetooth printer kind behind the same adapter trait. A Kotlin `BluetoothBridge` (SPP over a fresh RFCOMM connection, paired printers only) is reached from Rust through JNI (`bt_jni.rs`) as a `BluetoothLink`. `AndroidPrinter` combines it with the LAN scanner and sender; USB is not supported. All prints go through one dispatch that keeps the last job failure and raises a "Printer offline" alert when a Bluetooth printer drops.

## Boundaries & Constraints

**Always:**
- The Windows adapter, its endpoints and its tests behave as before (new fields are optional/defaulted).
- Bluetooth permission is `BLUETOOTH_CONNECT` (Android 12+) and `BLUETOOTH` up to API 30 only; the Hub never scans, pairs or asks for location.
- A job to a printer that is off or out of range fails within 10 s, never blocks the Hub, and the next job opens a new connection.
- The setup status (`printer.status`) records test prints only; the last job failure is separate.
- Every new string lives in `en.json`.

**Never:**
- USB printing on Android; pairing or scanning from the Hub; a second printer UI.
- A Bluetooth dependency of any other Hub flow.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Find, permission granted | Android | Paired Bluetooth printers and LAN printers listed, "USB: Not supported on Android" |
| Find, permission not granted | Android 12+ | One-line reason, system prompt; if refused: "Bluetooth permission is off" + "Open app settings", LAN list still shown |
| Radio off / no Bluetooth / none paired | Android | Its own plain note |
| Test print, Bluetooth or LAN, 58 or 80 mm | selected | Story 2.27 page printed, status `ok` or `failed` |
| Printer off or out of range | Bluetooth | `failed` within 10 s, plain error |
| Drops mid-print | Bluetooth | Job failure kept, `printer_offline` alert in the header list, next print reconnects |
| Windows Hub | any | No Bluetooth notes; USB listed |

</intent-contract>

## Code Map

- `crates/arena-proto/src/printer.rs`, `alert.rs` -- `PrinterKind::Bluetooth`, `BluetoothAccess`, `PrinterJobFailure`, `PrinterHealth`, Bluetooth error kinds, address parsing, `AlertKind::PrinterOffline`.
- `crates/arena-core/src/printer/mod.rs` -- `PrinterTarget::Bluetooth`, Bluetooth errors, `bluetooth_access()` and `usb_supported()` on `Printer`.
- `crates/arena-tasks/src/printer/bluetooth.rs` -- `BluetoothLink`, `AndroidPrinter`, `FakeBluetooth`; `fake.rs` scripted access.
- `crates/arena-api/src/ui_printer.rs`, `ui_auth.rs` -- find reports access, shared `dispatch`, `GET /ui/printer/health`, offline alert.
- `crates/arena-hub/src/lib.rs` -- `StartOptions.bluetooth` selects `AndroidPrinter`.
- `apps/hub/src-tauri/src/bt_jni.rs`, `android.rs`, `lib.rs`; Kotlin `BluetoothBridge.kt`, `HubBridge.kt`, `MainActivity.kt`, `HubService.kt`, `AndroidManifest.xml`.
- `apps/hub/src/lib/printer.ts`, `service.svelte.ts`, `alerts.svelte.ts`, `routes/settings/printer` and `kitchen` pages, `en.json`.
- `scripts/check-android.mjs` (+ test) -- Bluetooth permission and bridge rules.

## Tasks & Acceptance

**Execution:**
- [x] Protocol types, core target and errors, generated TypeScript.
- [x] `BluetoothLink`, `AndroidPrinter`, `FakeBluetooth` with tests (listing, permission, drop, timeout, backstop).
- [x] API: access in find, dispatch with failure record and offline alert, health endpoint; tests including the Android adapter over the fake.
- [x] Kotlin bridge, JNI, Hub wiring, manifest, permission prompt via `HubBridge`.
- [x] Settings UI (Printer and the KOT picker): ask, notes, settings link, USB note, last failure; Playwright tests; alert list test.
- [x] `check-android.mjs` Bluetooth rules and tests; README; `deferred-work.md`.

**Acceptance Criteria:** those of Story 4.6 in `epics.md`. Device-only criteria (real SPP printer, permission prompt, 10 s bound on hardware) cannot run here and are recorded as not run.

## Verification

**Commands:**
- `cargo test --locked -p arena-proto -p arena-core -p arena-tasks -p arena-api`, `cargo clippy` on the same crates with `--all-targets --all-features -D warnings`, `cargo fmt --check`.
- `pnpm --filter @arena/hub check`, Playwright `printer`, `kitchen`, `alerts`, `pnpm run check:strings`, `format:check`, `check:generated` (after commit), `node scripts/check-android.mjs manifest`, `node --test scripts/check-android.test.mjs`.

## Verification Log

Pass: Rust tests for arena-proto, arena-core, arena-tasks, arena-api (227 api tests), clippy with `-D warnings`, `cargo fmt --check`, `pnpm --filter @arena/hub check` (0 errors), Playwright `printer`, `kitchen`, `alerts` on desktop and phone, `check:strings`, `check:protocol`, `check:layers`, `check:pins`, `format:check`, `check-android` manifest check and its tests (26). Not run: Gradle/Kotlin build, `bt_jni.rs` on the Android target and every device-only criterion. Pre-existing failures independent of this story: `arena-hub` `dev_seed`/`lan`/`printer` integration tests (`USER_LIMIT`, reproduced with the work stashed) and `check:tokens-used` (`--sticky-bottom`, Story 4.5).
