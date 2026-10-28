---
title: 'Story 4.15: Upgrading removes ads within one heartbeat, and a lapse brings them back only after expiry'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_revision: '7c2c82d5d46ab4a9835009209e4f69abf31d9de9'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-14-ad-requests-and-impressions-are-recorded-without-ids-and-sen.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The ads gate (Story 4.10) follows the Hub's effective tier, but a paid Android Hub that restarts offline could still report the pairing-time tier (Free), a gate-off left a preloaded interstitial alive, and a tier change left no audit row.

**Approach:** Reuse the gate and the heartbeat's `settle` (licence against trusted time). Start with an unknown tier until the cached licence is evaluated; on gate-off call a new plugin `stop` (banner destroyed, interstitial dropped) and make the interstitial policy forget a loaded ad; add the setting `licence.tier` to `licence.apply` so each effective-tier change is one system change record.

## Boundaries & Constraints

**Always:**
- Paid features reach the Hub only through the licence token; the tier is Free for a tampered, foreign, unreadable or ended licence.
- Expiry and "still current" follow trusted time, never the wall clock.
- The audit row is the change record of `licence.apply` (actor `system`/`hub-licence`); it exists only when the tier changes.

**Never:**
- No ad symbol in Rust; no new ad endpoint; no running session or order touched by a tier change (Epic 10).

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Comp Basic granted | heartbeat answer carries it | tier Basic in that heartbeat; banner destroyed, slot gone, focus kept, interstitial dropped, no request |
| Paid, WAN down at start | valid cached paid licence | tier unknown until evaluated, then paid; never Free |
| Entitlement ended | cloud signs Free | ads back after that heartbeat |
| Paid licence passes `exp` | trusted time, WAN down | tier Free, gate on |
| Wall clock moved forward | cloud time held, licence current | still paid |
| Tampered licence | any | Free, standing unverified, ads on |

</intent-contract>

## Code Map

- `crates/arena-hub/src/lib.rs` -- initial `HubStatus.tier` is `None` (was the pairing-time tier).
- `crates/arena-proto/src/{entitlement,kitchen,lib}.rs` -- `LicenceApply.tier`, `KEY_LICENCE_TIER`, six rows; `packages/ui/src/generated/LicenceApply.ts`.
- `crates/arena-tasks/src/heartbeat.rs` -- `settle` (unchanged logic) and new tests.
- `apps/hub/src/lib/{ads,interstitial}.ts`, Kotlin `ArenaAds.stop`.
- Tests: core `apply/tests.rs`, heartbeat tests, `ads.spec.ts`, `ads-lapse.unit.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto -- `licence.tier` row and generated types
- [x] Hub start -- unknown tier until the cached licence is evaluated
- [x] Plugin and page -- `stop`, interstitial forgotten, slot and focus
- [x] Tests -- apply (one row per change), heartbeat (upgrade, expiry on trusted time with the wall clock moved, tampered), page (stop, focus, lapse)
- [x] README

**Acceptance Criteria:** those of Story 4.15 in `epics.md`.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`: pass.
- `cargo test -p arena-proto -p arena-core -p arena-store -p arena-api -p arena-hub`: pass. `arena-tasks`: all pass except `trusted_time_is_cloud_time_plus_monotonic_elapsed` (`Instant::checked_sub` of 8 days fails on a freshly booted Windows host) and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-cloud` `backup::a_hung_run_times_out…` and `binary::unreachable_at_start…` fail in this environment; none touch this change.
- Hub Playwright `ads`, `ad-slot`, `interstitial`, `ad-metrics`, `ads-lapse`: pass. `pnpm --filter @arena/hub check`, `check:strings`, `check:ads`, prettier: pass. `check:generated` passes once `LicenceApply.ts` is committed.
- Not run: the Kotlin `stop()` (no Android toolchain).
