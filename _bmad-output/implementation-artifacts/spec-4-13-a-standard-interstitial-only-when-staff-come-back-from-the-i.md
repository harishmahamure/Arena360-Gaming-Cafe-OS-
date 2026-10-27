---
title: 'Story 4.13: A standard interstitial only when staff come back from the idle lock'
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-12-safe-ad-categories-a-per-branch-content-rating-and-the-ads-c.md'
warnings:
  - 'Nothing Android was built or run: the Kotlin plugin needed no change, and the behaviour of the SDK full-screen activity (alert sound, click-through return) is only covered by Playwright tests that play the plugin events.'
deferred: []
---

<intent-contract>

## Intent

**Problem:** A Free Android Hub may show a full-screen ad, but never in the middle of a sale or a session being handled.

**Approach:** One trigger only: a PIN unlock after the idle lock. `InterstitialPolicy` preloads one standard interstitial while the lock screen shows, and shows it on unlock only if it is loaded, 15 minutes have passed since the last one, and no ad-exclusion surface (a shared registry) was open when the lock started. The idle countdown is held while it shows and restarts from zero when it ends.

## Boundaries & Constraints

**Always:**
- Unlock never waits for an ad; a skipped ad is not delayed.
- Google owns the close control and timing: no custom skip UI.
- No ad symbol in Rust or the shared UI (the registry only holds surface names).

**Never:**
- No trigger from a settle, its undo window or a closed bill.
- No impression counts or upgrade handling (Stories 4.14 and 4.15).

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Idle lock, nothing open | gate on | one `preload`; on PIN unlock with `ad.loaded`, `show` |
| Nothing loaded / last ad under 15 min ago | unlock | Hub returns at once, no show |
| Tile sheet, settle bar, undo toast, inline PIN, More sheet, alert list open at lock | unlock | no preload, no show, unlock returns to the surface |
| Alert raised while the ad shows | `devices.changed` | sound plays, badge waiting when the ad closes |
| Ad dismissed / failed / foreground return / 2 min | ending | focus back on the pre-lock element, idle timer from zero |
| Paid / Windows | gate off | nothing sent to any plugin |

</intent-contract>

## Code Map

- `packages/ui/src/ad-exclusion.ts`, `index.js` -- `registerAdExclusion`, `openAdExclusions`.
- `InlinePin.svelte`, `UndoToast.svelte`, `TileSheet.svelte`, `SettleBar.svelte`, `MoreSheet.svelte`, `routes/+layout.svelte` (alert list) -- register while open.
- `apps/hub/src/lib/interstitial.ts` -- `InterstitialPolicy`.
- `apps/hub/src/routes/+layout.svelte` -- snapshot of open surfaces at lock, `unlock()` after a PIN unlock, `adShowing` holds the idle tracker, focus return.
- `apps/hub/tests/interstitial.unit.spec.ts`, `interstitial.spec.ts` (new).
- `README.md`, `deferred-work.md`.

## Tasks & Acceptance

**Execution:**
- [x] Shared ad-exclusion registry and registrations for every existing surface.
- [x] `InterstitialPolicy` (preload on lock, show on PIN unlock, 15-minute spacing, never waits).
- [x] Layout wiring: lock-time snapshot, idle countdown held during the ad, focus return, foreground return and safety limit.
- [x] Unit and Playwright tests (preload, show, skip cases, spacing, exclusion, alert during ad, paid gate off).
- [x] README and deferred work.

**Acceptance Criteria:** those of Story 4.13 in `epics.md`.

## Verification

**Commands:**
- `pnpm --filter @arena/hub check`, `npx playwright test` in `apps/hub`, `check:strings`, `check:layers`, `check:ads`, `check:hex`, prettier.

## Verification Log

Pass: `pnpm --filter @arena/hub check` (0 errors), `check:strings`, `check:layers`, `check:ads`, `check:hex`, `format:check`, Hub Playwright (1199 passed, 60 skipped, 1 failed: `players.spec.ts` `Ctrl+K from another screen`, the same flaky test noted on Story 4.12, unrelated). New tests: 34 across both projects. Not run: everything Android.
