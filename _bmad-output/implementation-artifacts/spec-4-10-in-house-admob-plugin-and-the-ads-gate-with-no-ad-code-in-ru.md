---
title: 'Story 4.10: In-house AdMob plugin and the ads gate, with no ad code in Rust or on Windows'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-9-free-limits-3-users-20-devices-and-one-branch-enforced-and-e.md'
warnings:
  - 'The Kotlin module, the Gradle wiring and StrictMode were not built or run: no Android SDK, emulator or device here.'
deferred: []
---

<intent-contract>

## Intent

**Problem:** Free Android Hubs must show ads, but ad code may live nowhere except one Kotlin module, and a paid or Windows Hub must never initialise it or make a request.

**Approach:** A Kotlin-only Gradle module `tauri-plugin-arena-ads` (Google Mobile Ads SDK 25.5.0) reached through a WebView interface `ArenaAds`; a UI gate (`ads.ts`) that is on only for Android and an effective tier of Free (from a new `HubSnapshot.tier` and a new `entitlement.changed` event); a CI lint (`check-ads.mjs`) for source, artefacts and build ids; a debug-only gallery at `/dev/ads`.

## Boundaries & Constraints

**Always:**
- The plugin is the only code importing `com.google.android.gms.ads`; the page sees only `init`, `banner.place|hide|destroy`, `interstitial.preload|show` and the `arena:ad` events.
- Gate off (paid Android, Windows, unknown tier) means the plugin is never initialised, no request, nothing rendered. A failed licence check is Free (the effective tier).
- Debug builds carry only Google's test ids; release builds none (unset release ids are an all-zero placeholder, never a test id).

**Never:**
- No Rust crate, Tauri plugin crate, ad symbol or ad endpoint in Rust; no ad string in the Kiosk or the Windows Hub shell.
- No banner slot, content-rating UI, interstitial rules or impression counts here (Stories 4.11 to 4.15).

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Android, tier `free` | snapshot | plugin `init` once |
| Android, `basic`/`premium`/unknown | snapshot | never initialised, no slot |
| Tier moves free to paid on the stream | `entitlement.changed` | banner destroyed, listeners removed, no restart |
| Paid lapses to free | `entitlement.changed` | plugin starts again |
| Windows or browser | any | every call does nothing |
| Release build, gallery route | `init` says not debug | "opens only on a debug build" |

</intent-contract>

## Code Map

- `apps/hub/src-tauri/plugins/tauri-plugin-arena-ads/android/` -- `ArenaAds.kt` (interface, banner, interstitial, StrictMode), `AdsConfig.kt` (rating, rect, event script), unit test, `build.gradle.kts` (SDK pin, test vs release ids).
- `apps/hub/src-tauri/gen/android/{settings.gradle,app/build.gradle.kts}`, `MainActivity.kt` -- include the module, AdMob app id placeholder (test in debug), add the `ArenaAds` interface, release on destroy.
- `crates/arena-proto/src/snapshot.rs` -- `HubSnapshot.tier`, `EntitlementChangedEvent`; `arena-api` `health.rs` (snapshot `tier`) and `ui_events.rs` (`entitlement.changed` once per change).
- `apps/hub/src/lib/{ads.ts,events.ts}`, `routes/+layout.svelte` (gate), `routes/dev/ads/+page.svelte` (gallery), `en.json`.
- `scripts/check-ads.mjs` (+ test), `check-pins.mjs` (SDK pin), `package.json` `check:ads`, `ci.yml`, README.

## Tasks & Acceptance

**Execution:**
- [x] Kotlin plugin module with the six calls, six events, background SDK start, debug StrictMode, pinned SDK 25.5.0.
- [x] Debug test ids and release ids from build config; app id placeholder; id check on built variants.
- [x] `HubSnapshot.tier`, `entitlement.changed`, Rust tests (event once per change, snapshot tier).
- [x] UI gate and `AdsClient`, wired in the layout; no-ops off Android; Playwright spec.
- [x] Dev gallery on debug Android builds.
- [x] `check-ads.mjs` source/ids/artefact, unit tests, CI steps (source, ids after the Android builds, Hub/Kiosk web builds and Windows executables).
- [x] README pin row and Ads section.

**Acceptance Criteria:** those of Story 4.10 in `epics.md`.

## Verification

**Commands:**
- `cargo test --locked -p arena-proto -p arena-core -p arena-api --features arena-api/dev-fixtures`
- `cargo clippy --locked -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --features arena-hub/dev-fixtures --all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm --filter @arena/hub check`, `npx playwright test` in `apps/hub`, `node --test scripts/check-ads.test.mjs`, `check:ads`, `check:pins`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `check:secrets`, `check:contrast`, prettier.

## Verification Log

Pass: all of the above (Hub Playwright: 1128 passed, 42 skipped). `check:generated` passes once the new generated file is committed. Not run: `check:tokens-used` (fails on master on `--sticky-bottom`, known), `check:deploy` and 12 `scripts` tests (need `kubectl`), and everything Android: the Gradle build, `ArenaAds.kt`, the Kotlin unit test, StrictMode, the SDK, `check-ads.mjs ids` against real build output and the gallery on a device.

## Review Log

Code review (4 layers) fixed: StrictMode installed on the UI thread, `interstitialShow` with nothing loaded emits `ad.failed`, a later `init` rating always reaches the SDK, a banner re-requests on a new width, banner rect bounded, previous `ArenaAds` released on WebView re-create, `ads.ts` stays off if `init` throws, `check-ads ids` requires both variants with BuildConfig and manifest, empty artefact dirs reported, the stream-only entitlement test and `/dev/ads` paid-Hub test added. Remaining items are in `deferred-work.md`. Re-run: `node --test scripts/check-ads.test.mjs`, `pnpm --filter @arena/hub check`, Hub Playwright `ads.spec.ts` (26 passed). Kotlin changes are uncompiled (no Android toolchain).
