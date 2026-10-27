---
title: 'Story 4.5: The Hub works on an Android phone and tablet by touch'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '1e401824d3e46b5485fc0a35bb80b55ee6404f48'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-4-kiosks-find-and-pair-with-an-android-hub-on-the-lan.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub UI is built for the Windows counter. Below 840px it already swaps the rail for a bottom bar, but the header is the desktop one, the tile sheet is a floating card with a settle bar that scrolls away, Android Back exits the app, system bars and the keyboard cover controls, and nothing proves 44px targets on a phone.

**Approach:** Keep one UI. Add a shared viewport flag, a Back-handler stack fed by the Kotlin back callback, a phone header (64px, chips in a row below), a full-height tile sheet with its settle bar pinned, scroll padding for sticky bars and the keyboard, native inset and font-scale handling in `MainActivity`, and an automated 44px and axe check on a phone and a tablet.

## Boundaries & Constraints

**Always:**
- Desktop (≥ 840px) markup, testids and behaviour are unchanged; the sheet docks right as in Story 3.5.
- Tokens only (`--space-9` = 64px for the phone header); every new string lives in `en.json`.
- Android Back closes the topmost layer only (More sheet, alert list, tile sheet; the lock screen swallows it); with nothing open it does nothing and never exits or navigates.
- Text size follows the system font scale but is capped at 130% (`WebSettings.textZoom`).

**Never:**
- A second Hub UI or a Kotlin screen; location or other new permissions; changing Story 3.5 sheet actions or the settle logic.
- Confirm modals; stacking modals.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Phone header | < 840px, signed in | 64px; branch, alert badge count, staff; no clock, no backup status; `chip-rule` and `chip-offline` in a row below |
| Open a tile on a phone | < 840px | Full-height sheet over the floor, settle bar pinned at the sheet bottom; floor behind is inert |
| Back with More sheet / tile sheet / alert list open | Android | Topmost closes; focus returns to its opener |
| Back on the floor, nothing open, or on the lock screen | Android | Nothing happens |
| Field focused, keyboard opens | Android | Field scrolls into view above the keyboard and above sticky bars |
| Rotate or cross 840px | any | Same screen, scroll, open sheet and field text; More sheet closes only when the bar disappears |
| Target and axe audit | 412×915 and 1280×800 | Every control ≥ 44×44, zero serious/critical axe violations |

</intent-contract>

## Code Map

- `apps/hub/src/app.html` -- viewport meta (`viewport-fit=cover`).
- `apps/hub/src/lib/viewport.svelte.ts` (new) -- shared `viewport.phone` flag from `matchMedia("(max-width: 839.98px)")`.
- `apps/hub/src/lib/back.ts` (new) -- Back-handler stack; `arena:back` window event from Kotlin runs the top handler.
- `apps/hub/src/routes/+layout.svelte` -- phone header, chip row, back registrations (More sheet, alert list, lock), scroll padding, keyboard scroll-into-view.
- `apps/hub/src/lib/MoreSheet.svelte`, `AlertList.svelte`, `LockOverlay.svelte`, `TileSheet.svelte` -- register Back handlers.
- `apps/hub/src/routes/floor/+page.svelte` -- phone full-height sheet, inert floor; `TileSheet.svelte` -- pinned settle bar region.
- `apps/hub/src-tauri/gen/android/app/src/main/java/in/arena360/hub/MainActivity.kt`, `AndroidManifest.xml` -- insets, IME, Back callback, textZoom, `fontScale` config change.
- `apps/hub/tests/touch.spec.ts` (new) -- target-size and axe audit, header, sheet and Back behaviour; `back.ts` unit tests.
- `scripts/check-android.mjs` and its test -- manifest keeps `fontScale` in `configChanges`.
- README Android section; `deferred-work.md` for emulator-only criteria.

## Tasks & Acceptance

**Execution:**
- `viewport.svelte.ts`, `back.ts` -- shared flag and Back stack with unit tests.
- `+layout.svelte`, sheets -- phone header, chip row, Back registrations, scroll padding.
- `floor/+page.svelte`, `TileSheet.svelte` -- full-height phone sheet, pinned settle bar, inert floor, focus return.
- `MainActivity.kt`, manifest -- system-bar, cutout and IME insets as padding; Back callback to `arena:back`; text zoom cap.
- `touch.spec.ts` -- 44px and axe audit on phone and tablet; fix every finding in the UI.
- README, `check-android.mjs`, `deferred-work.md`.

**Acceptance Criteria:** those of Story 4.5 in `epics.md`. Device-only criteria (real Back gesture, cutouts and gesture bar, on-screen keyboard, system font scale, rotation on hardware) cannot run here and are recorded as not run.

## Verification

**Commands:**
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `pnpm run format:check`, Playwright `touch`, `shell`, `tile-sheet`, `lock`, `devices-display` specs, `node scripts/check-android.mjs manifest`.

## Verification Log

Pass: `pnpm --filter @arena/hub check`, `check:strings`, `format:check`, `node scripts/check-android.mjs manifest`, `check-android` script tests (23), Playwright `touch`, `back.unit`, `shell`, `tile-sheet`, `lock`, `devices-display` (195 passed, 35 skipped by project), and earlier the full suite on desktop and phone projects. Not run: Gradle/Kotlin build and all device-only criteria (real Back gesture, cutouts, on-screen keyboard, system font scale, rotation on hardware); recorded in `deferred-work.md`.
