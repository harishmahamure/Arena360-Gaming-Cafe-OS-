---
title: 'Story 4.11: An always-visible banner in a permanent slot that nothing covers'
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-10-in-house-admob-plugin-and-the-ads-gate-with-no-ad-code-in-ru.md'
warnings:
  - 'The Kotlin pause/resume and banner request changes were not built or run: no Android SDK, emulator or device here.'
deferred: []
---

<intent-contract>

## Intent

**Problem:** A Free Android Hub must show its banner in a place that is always visible, but a banner floating over the app can cover a sale or be covered by a sheet.

**Approach:** While the ads gate is on, the layout reserves a fixed 60px `ad-banner` strip (`AdSlot.svelte`) and hands its rectangle to the plugin; every overlay stops above it through `--ad-reserved`; the plugin pauses with the activity.

## Boundaries & Constraints

**Always:**
- The strip exists only while the gate is on and keeps its height with no ad (label only).
- No overlay (More sheet, lock screen, phone tile sheet, Undo dock) draws over the strip; the strip never draws over app content.
- Below 840px the strip sits between main and the bottom bar; at 840px and wider under main and the tile-sheet column, never under the rail, with 24px above it.

**Never:**
- No content-rating UI, interstitial rules, impression counts or upgrade handling here (Stories 4.12 to 4.15).
- No ad symbol outside the Kotlin plugin; the Hub UI only calls `ads.banner.*`.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Free Android, any screen | gate on | strip drawn, rect placed with the plugin |
| No fill / WAN down / load error | `ad.failed` | strip keeps 60px and the label |
| Paid, Windows, tier unknown | gate off | no strip, no `data-ads`, no plugin call |
| Window resized or rotated | gate on | strip re-places with the new width |
| Activity paused | `onPause` | banner stops refreshing, no request; requested on resume |

</intent-contract>

## Code Map

- `apps/hub/src/lib/AdSlot.svelte` -- the strip, the rect hand-off (mount, resize, orientation, visual viewport), `hide` on unmount.
- `apps/hub/src/routes/+layout.svelte` -- `adsOn` mirror of the gate, `data-ads` on the root, `--ad-slot-height` / `--ad-reserved` / `--ad-gap` / `--ad-shrink`, grid areas for phone and wide shells and the sign-in gate; bottom-bar items 1px shorter.
- `LockOverlay.svelte`, `MoreSheet.svelte`, `UndoDock.svelte`, `routes/floor/+page.svelte`, `TileSheet.svelte` -- stop above the slot, 24px settle-region gap, scroll padding.
- `routes/dev/ads/+page.svelte` -- the gallery's banner button re-places into the permanent slot.
- `ArenaAds.kt`, `MainActivity.kt` -- `pause()` / `resume()`, no request while paused.
- `packages/ui/src/messages/en.json` -- `hub.ads.label`.
- `apps/hub/tests/ad-slot.spec.ts` (new), `ads.spec.ts` (updated for the slot).

## Tasks & Acceptance

**Execution:**
- [x] `AdSlot` strip with the rect hand-off, drawn only while the gate is on, in the shell and the sign-in gate.
- [x] Layout grid below and above 840px, root properties for overlays, scroll padding.
- [x] Lock screen, More sheet, phone tile sheet (full and picking card), Undo dock and the wide tile sheet height stop above the slot; settle region keeps 24px.
- [x] Kotlin banner `pause` / `resume` wired from the activity; request deferred while paused.
- [x] Layout test at 412 x 915, 800 x 1280 and 1280 x 800 on Floor, a tile with the settle bar, the More sheet, the lock screen and Settings; no-ad, gate-off and non-Android tests.
- [x] README banner-slot note and deferred work.

**Acceptance Criteria:** those of Story 4.11 in `epics.md`.

## Verification

**Commands:**
- `pnpm --filter @arena/hub check`, `npx playwright test` in `apps/hub`, `check:strings`, `check:hex`, `check:layers`, `check:ads`, `check:pins`, `check:contrast`, `check:secrets`, prettier.

## Verification Log

Pass: all of the above (Hub Playwright: 1148 passed, 60 skipped). Not run: everything Android (Gradle, `ArenaAds.kt`, `MainActivity.kt`, the native banner over the strip, pause on screen-off, a banner tap and return); AC 6 and 7 (background pause, banner tap) are Kotlin behaviours only checked by reading.
