---
title: 'Story 6.37: Kiosk screens reflow at every supported width, by keyboard and in high contrast'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '73c23e53c62ed5d894b2e22b2952bec9c8ab253e'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: []
deferred: []
---

## Intent

**Problem:** The kiosk was built screen by screen; nothing proved it fits every café monitor, works with only a keyboard, or stays clean in high contrast, and the CI blur gate did not cover `filter: blur` or unmuted autoplay.

**Approach:** Most of the layout already existed (Home's tile counts and hero widths). This story adds the missing rules (Games columns 5/7/9, the 2560px centred cap, the ultrawide hero in the left 45%, a top bar that no longer collides at 1366), extends `check-no-blur` to `filter: blur(...)` and autoplaying unmuted media, and adds three Playwright suites that prove it: reflow at four widths (plain and +30% pseudo-locale), the keyboard-only journey, and high contrast with axe.

## Tasks

- [x] Games grid: 5 / 7 / 9 explicit columns at 1366 / 1920 / 2560+
- [x] Shell: content capped at 2560px and centred; hero copy kept to the left 45% of it on 21:9
- [x] Top bar: under 1600px the nav sits in the space between logo and controls (fixes the Shop tab meeting the volume button at 1366)
- [x] `check-no-blur`: also fails `filter: blur(...)` and autoplaying unmuted `<video>`/`<audio>`, with tests
- [x] Playwright: `reflow.spec.ts` (11 screens × 4 widths × plain/+30%, grid rules, ultrawide cap, no scaling)
- [x] Playwright: `keyboard.spec.ts` (login → plan picker → Home → Games → launch → Shop order → Account → Log out; Shift+Tab; no extra shortcuts)
- [x] Playwright: `high-contrast.spec.ts` (no media/scrims/shadows/translucency, 2px white outline, axe) for every screen
- [x] `docs/kiosk-accessibility.md` section

## Acceptance Criteria

- Given 1366 / 1920 / 2560 / 21:9, then Home shows 4 / 6 / 8 / 8 tiles + All games, Games 5 / 7 / 9 / 9 columns, hero copy ≤ 720 / 864 / 864 px (left 45% on 21:9), content capped at 2560px and centred, 56px margins, reflowed and never scaled.
- Given the visual suite at the four widths on Home, Games, Apps, Shop with cart, Account, login, sign-up, plan picker, HUD, warning toast and grace lock, then nothing clips, overlaps or scrolls sideways, including with a +30% pseudo-locale.
- Given keyboard only, then the whole journey works with Tab/Shift+Tab, Enter, Esc and arrows, Ctrl+Shift+E is the only player shortcut, and the focus ring is visible at every stop.
- Given HC, then media, scrims, shadows and translucency are gone, every interactive surface has a 2px white outline, and axe reports no serious or critical violation.
- Given any kiosk or HUD stylesheet, then `backdrop-filter`, `filter: blur` and autoplaying unmuted media fail CI.

## Verification

- `pnpm --filter @arena/kiosk check`; `pnpm exec playwright test` in `apps/kiosk` (501 tests; the 4 `theme.spec.ts` tests fail only on Windows, where their fixture path becomes `C:\C:\…`, unchanged by this story); `node --test scripts/check-no-blur.test.mjs`; `pnpm check:no-blur check:strings check:hex check:layers check:contrast format:check`
- `check:tokens-used` reports `--ad-reserved` in `apps/hub` (pre-existing, not touched here).

## Notes

- The Ctrl+Shift+E chord is the Rust keyboard hook (Story 6.14), so the keyboard suite proves the page adds no shortcut of its own; the admin-exit chord (Story 1.20) is staff-only.
- Overlap checks ignore controls under a fixed panel smaller than the screen (the cart is meant to sit over the Shop) and the most-played carousel, which scrolls on purpose.
- The HUD window is checked at each width as a page; its real size is set by the shell (`hud_layout`).
- The pseudo-locale is a test-only DOM pass that appends 30% filler to every text node; there is no second catalogue.
