---
title: 'Story 4.12: Safe ad categories, a per-branch content rating and the ads consent view'
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-11-an-always-visible-banner-in-a-permanent-slot-that-nothing-co.md'
warnings:
  - 'The Kotlin changes (configure, npa request extras) were not built or run: no Android SDK, emulator or device here.'
  - 'The AdMob console blocks cannot be set or checked from the repo; AD-CATEGORIES.md is unsigned, so `check-ads.mjs note --release` fails until a named person fills it in.'
deferred: []
---

<intent-contract>

## Intent

**Problem:** A Free café's counter screen shows ads, and the owner has no say over how strict they are and no proof that adult, gambling and alcohol ads are blocked.

**Approach:** A `branch.ads_rating` setting (G, PG default, T) edited on the Ads screen through `settings.set` (manager PIN for a Counter), carried to the plugin with the consent state so the SDK's maximum content rating follows it at once and requests are non-personalised until consent is recorded; a versioned category note with a release check.

## Boundaries & Constraints

**Always:**
- MA is never offered; a stored MA or unknown value is PG (Rust `AdRating::parse`, TS `adRatingOf`, Kotlin `AdRating.parse`).
- No ad symbol in Rust or the shared UI; the UI only calls `ads.configure` and the bridge's `configure`.
- No control turns ads off.

**Never:**
- No interstitial rules, impression counts or upgrade handling (Stories 4.13 to 4.15).

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Manager saves a rating | `settings.set` | audited, snapshot `setup.ad_rating` moves, plugin `configure`, no restart |
| Counter saves a rating | no `settings.edit` | `inline-pin`, change carries the elevation ticket |
| Stored `MA` or junk | setting row | reads as PG everywhere |
| No consent recorded | `ad_consent` missing | requests carry `npa=1` |
| Consent given | `ad_consent` given | standard requests, banner re-requested |
| Paid / Windows | gate off | nothing sent to any plugin |

</intent-contract>

## Code Map

- `crates/arena-proto/src/operating.rs`, `lib.rs` -- `KEY_ADS_RATING`, `DEFAULT_ADS_RATING`, `ADS_RATINGS`, `AdRating`, `SetupStatus.ad_rating`.
- `crates/arena-core/src/settings_defaults.rs`, `setup.rs` -- the default row, its `OneOf` rule, the snapshot field.
- `apps/hub/src/lib/ads.ts`, `routes/+layout.svelte` -- `AdsConfig`, `adRatingOf`, `ads.configure`; the layout hands the snapshot's rating and consent to the plugin and re-reads the snapshot on `settings.changed` for the ads keys.
- `apps/hub/src/routes/settings/branch/ads/+page.svelte`, `packages/ui/src/messages/en.json` -- excluded list, rating choice with `inline-pin`, consent time and staff id, "Ads keep Arena 360 free."
- `ArenaAds.kt`, `AdsConfig.kt` -- `configure`, `adRequest()` with `npa=1` until consent.
- `AD-CATEGORIES.md`, `scripts/check-ads.mjs` (+ test) -- the versioned category note and the `note [--release]` check.
- `README.md`, `deferred-work.md`.

## Tasks & Acceptance

**Execution:**
- [x] `branch.ads_rating` setting (default PG, G/PG/T only), `AdRating`, `setup.ad_rating`, generated types, Rust tests.
- [x] `ads.configure` and the layout wiring; the plugin starts with the branch's rating and consent and follows changes without a restart.
- [x] Kotlin `configure`, `npa=1` requests until consent, banner re-request on a consent change, unit test for the parsed config.
- [x] Ads screen: always-excluded list, G/PG/T in plain words, manager PIN for a Counter, consent time and staff id, no off switch, Upgrade line behind the billing flag.
- [x] `AD-CATEGORIES.md` and `check-ads.mjs note [--release]` with tests.
- [x] Playwright (client, stream-driven reconfigure, Ads screen, Counter PIN) and README/deferred work.

**Acceptance Criteria:** those of Story 4.12 in `epics.md`.

## Verification

**Commands:**
- `cargo test --locked -p arena-proto -p arena-core -p arena-api --features arena-api/dev-fixtures`, `cargo clippy` on the touched crates, `cargo fmt --check`
- `pnpm --filter @arena/hub check`, `npx playwright test` in `apps/hub`, `node --test scripts/check-ads.test.mjs`, `check:ads`, `check:generated`, `check:strings`, `check:hex`, `check:layers`, `check:pins`, `check:secrets`, prettier.

## Verification Log

Pass: cargo test (arena-proto, arena-store, arena-core, arena-api, arena-hub except dev_seed), clippy `-D warnings`, `cargo fmt --check`, `pnpm --filter @arena/hub check`, Hub Playwright (1165 passed, 60 skipped, 1 failed), `node --test scripts/check-ads.test.mjs`, `check:ads`, `check:strings`, `check:hex`, `check:layers`, `check:pins`, `check:protocol`, `check:secrets`, `check:contrast`, prettier. `check:generated` passes once the new generated file is committed. Failing on master too, not caused by this story: `arena-hub` `dev_seed` (2 tests) and Hub Playwright `players.spec.ts` `Ctrl+K from another screen` (flaky). `check-ads.mjs note --release` fails by design until a named person signs `AD-CATEGORIES.md`. Not run: everything Android (Gradle, `ArenaAds.kt`, the Kotlin unit test, the SDK's request configuration and `npa` extras on a device).
