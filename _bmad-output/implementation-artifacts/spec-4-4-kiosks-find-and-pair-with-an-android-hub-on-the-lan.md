---
title: 'Story 4.4: Kiosks find and pair with an Android Hub on the LAN'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: '29d0a1d'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-2-the-android-hub-keeps-serving-while-the-screen-is-off-in-doz.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-3-the-android-hub-pairs-with-the-wizard-code-and-keeps-its-sec.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Windows Hub advertises `_arena360-hub._tcp` with the `mdns-sd` crate. On Android that crate cannot be relied on (no multicast on Doze, no `NsdManager` integration), so a kiosk's discovery (Story 2.17) never lists a tablet Hub, and the Hub does not follow its own IP when Wi-Fi reconnects.

**Approach:**
- The Android shell supplies an `Advertiser` backed by a Kotlin `NsdBridge` (`NsdManager`) through JNI. The core's `MdnsTask` is unchanged apart from three small hooks: it accepts a boxed advertiser, it wakes at once when the shell reports a network change, and on Android it withdraws the advert while the Hub has no LAN address and re-advertises when one returns.
- The same TXT records as Windows come from `arena_proto::txt_records`, built by the existing `Advert`.
- Kotlin registers a `ConnectivityManager` network callback; on a link change it re-registers the advert and tells Rust to re-read the addresses (within 5 s).
- The snapshot's `lan` block carries the network name and a `no_lan` flag; Settings → Devices shows the network name next to the IP, and the header shows an `alert-badge` "No LAN connection" while `no_lan` is set.

## Boundaries & Constraints

**Always:**
- Windows and dev behaviour is unchanged: default advertiser is `MdnsAdvertiser`, and the "withdraw while offline" rule applies only when `HubStatus.platform` is Android.
- Listeners stay `:7400` (TLS WebSocket) and `:7401` (HTTP(S)); the LAN client-class tests are not edited.
- The advert instance, port and TXT keys (`branch`, `hub`, `proto`, `fp`, `web`) are exactly those of `Advert::new`.
- The network name is best effort: the Wi-Fi SSID needs a location permission the Hub does not request, so Kotlin returns the transport (`Wi-Fi`, `Ethernet`) and adds the SSID only when Android reveals it.
- Every new string lives in `en.json`; no hex colours, tokens only.

**Never:**
- Requesting location or Nearby-devices permission for the SSID.
- Changing the kiosk's discovery or reconnect code (it already re-resolves by mDNS before its stored IP, Story 2.17/1.20).
- Bluetooth (4.6), phone layout of the header (4.5).

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Android Hub paired, on Wi-Fi | primary, has fingerprint, branch, an address | `NsdBridge.register` called once with the Windows TXT records |
| IP changes | network callback fires | shell wakes `MdnsTask`; addresses refresh in the status; Kotlin re-registers within 5 s |
| Wi-Fi lost | no IPv4 address | advert withdrawn; snapshot `lan.no_lan = true`; header shows "No LAN connection"; local trading continues |
| Network returns | an address again | advert registered again; badge gone |
| Windows Hub with no address | platform windows | advert kept as before; `no_lan` false |
| Settings → Devices | Android | IP list plus the network name line, for the typed-IP fallback |

</intent-contract>

## Code Map

- `crates/arena-tasks/src/mdns.rs`: `Advertiser` (box impl), `MdnsTask` (`with_wake`, `with_network`, offline withdraw), unit tests.
- `crates/arena-core/src/status.rs`: `HubStatus.lan_network`.
- `crates/arena-proto/src/snapshot.rs`: `SnapshotLan.network`, `SnapshotLan.no_lan`; `pnpm gen:types`.
- `crates/arena-api/src/health.rs`: `snapshot_lan`.
- `crates/arena-hub/src/lib.rs`: `StartOptions` (advertiser, network wake and name), mDNS task wiring.
- `apps/hub/src-tauri/src/android.rs`, new `nsd_jni.rs`, `lib.rs`; Kotlin `NsdBridge.kt`; `AndroidManifest.xml` (network state permission).
- `apps/hub/src/lib/snapshot.ts`, `routes/+layout.svelte`, `routes/settings/devices-display/+page.svelte`, `packages/ui/src/messages/en.json`, Playwright tests.
- `scripts/check-android.mjs` (NSD rule) and its tests; README Android section; `deferred-work.md`.

## Tasks & Acceptance

- [x] `arena-tasks`: boxed advertiser, wake hook, network-name hook, Android offline withdraw; tests with fakes.
- [x] `arena-core`/`arena-proto`/`arena-api`: status field, snapshot `network` and `no_lan`; regenerate types; tests.
- [x] `arena-hub`: `StartOptions` carry the advertiser, wake and network name into `MdnsTask`; Hub-level test.
- [x] Android shell: `NsdBridge.kt`, `nsd_jni.rs`, `android.rs` wiring, manifest permission; `check-android.mjs` rule.
- [x] `apps/hub`: snapshot normaliser, header "No LAN connection" badge, Settings → Devices network line; en.json; Playwright tests including axe.
- [x] README and deferred-work notes.

**Acceptance Criteria:** those of Story 4.4 in `epics.md`. Emulator-only criteria (kiosk listing the tablet within 10 s, client-class tests on an emulator, re-register timing on a real network change) cannot be run here and are recorded as not run.

## Verification

- `cargo fmt --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test` for `arena-tasks`, `arena-api`, `arena-hub`; `pnpm gen:types`; `pnpm run check:*`; `pnpm --filter @arena/hub check` and the affected Playwright specs; `node scripts/check-android.mjs manifest`.

## Verification Log

Pass: `cargo fmt`, clippy `-D warnings` for arena-tasks/hub/api/core/proto (incl. `dev-fixtures`), `arena-tasks` mdns tests (new: Android withdraw and re-advertise on wake, Windows keeps its advert), `arena-api` lib tests (new `no_lan`/`network` snapshot test), `arena-proto`/`arena-core` tests, `pnpm gen:types`, `check:strings`, `check:layers`, `format:check`, `check-android` script tests and `node scripts/check-android.mjs manifest` (new NsdManager rule), `@arena/hub` svelte-check, Playwright `devices-display` and `pair-kiosk` (new network and No LAN connection test with axe). Not run: Gradle/Kotlin build, `android.rs` on the Android target (`nsd_jni.rs` type-checked on the host), emulator and real-LAN criteria (see `deferred-work.md`). Pre-existing failures: `arena-e2e` (unix-only imports on Windows), 12 kubectl-dependent `test:scripts` cases.
