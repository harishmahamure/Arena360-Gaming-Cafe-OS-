---
title: 'Story 11.2: Home tells the install, pair, play story'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: 'ada1a7f9'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md'
  - '{project-root}/_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/mockups/key-web-marketing-home.html'
deferred:
  - 'No hero video is shipped: `HERO_VIDEO` in `apps/marketing/src/lib/site.ts` is empty until a muted clip with no audio track is cleared for use, so Home shows the still cover (an inline token-drawn SVG) under the scrim. `HeroMedia` already plays, loops while visible, stops for `prefers-reduced-motion`, offers a pause/play control and falls back to the still and then solid `bg` on error; that behaviour is covered by the `shouldPlay` unit test, not by a browser test, because there is no clip to load.'
  - 'The hero shot is a hand-built still of the Hub floor view (strings in the `site.home.shot.*` catalogue keys), not a screenshot, so it never goes stale against the product but also does not track UI changes.'
  - 'Home links to /pricing and /product, which arrive in Stories 11.3 and 11.4; until then they 404 in the same shell.'
  - 'The Lighthouse gate (`pnpm --filter @arena/marketing lighthouse`) uses Lighthouse 13.6.0 with its mobile profile (mid-range phone, slow 4G, 4x CPU) against the local static build, not against a deployed URL; there is no Cloudflare deploy in this repository (see Story 11.1).'
---

<intent-contract>

## Intent

**Problem:** Home is a heading placeholder; a prospective café owner cannot see what Arena 360 does, how fast they can start, or what it costs.

**Approach:** Home is a full-bleed page (`Layout bleed`): a dark media hero (promise in `web-hero`, lede, Get started, a Hub floor-view still), the "Install. Pair. Play." steps, three "What you get" cards and a closing Get started band. The hero's media is a Svelte island (`HeroMedia` in `@arena/ui`): video when one is configured, always over a still cover, always under the scrim.

## Boundaries & Constraints

**Always:** every visible string comes from `en.json` (`site.home.*`); prices come from the pricing snapshot (`planPrice`), never typed; tokens only; the hero keeps the dark tokens and the scrim in light and dark; high contrast replaces the hero media with solid `bg` (no scrim, no cover, no video, no control).

**Never:** `backdrop-filter` or live blur; unmuted or audio video; autoplay under `prefers-reduced-motion`; text over media outside the scrim copy zone.

## I/O & Edge-Case Matrix

| Scenario | State | Expected Output / Behavior |
|----------|-------|---------------------------|
| Desktop | ≥ 1024 px | `web-hero` 64/200 title in the left ~45%, Hub shot to its right, three-column sections 128 px apart |
| Narrower | < 1024 px | title in `web-display` 40/300; the shot stacks under the copy; phone sections 64 px apart |
| Light mode | `data-theme=light` | hero still `data-theme=dark` with the scrim; sections on "Cool paper" |
| High contrast | `data-theme=hc`, also switched at runtime | hero `data-theme=hc`, pure black, no scrim/cover/video/control |
| No video configured | `HERO_VIDEO = []` | still cover and scrim only; no `<video>`, no pause control |
| Video fails or autoplay is refused | error / rejected `play()` | video and control removed; the still shows |
| Reduced motion, scrolled away, or paused | any | `shouldPlay` is false; the video is paused |
| No JavaScript | scripts off | the static HTML has the full content; the hero is dark |

</intent-contract>

## Code Map

- `apps/marketing/src/pages/index.astro`, `components/HubShot.astro`, `styles/home.css`, `lib/price.ts` (`homePrices`), `lib/site.ts` (`HERO_VIDEO`), `layouts/Layout.astro` (`bleed`).
- `packages/ui/src/components/HeroMedia.svelte`, `hero-video.js` (`shouldPlay`), `components/LinkButton.svelte` (`size="lg"`), `messages/en.json` (`site.home.*`).
- Tests: `apps/marketing/tests/home.spec.ts` (new), `a11y.spec.ts` and `pseudo.spec.ts` (cover `/` in light, dark and hc), `shell.spec.ts` (padding checks moved to the 404, which is not full-bleed); `scripts/marketing-site.test.mjs` (`shouldPlay`, Lighthouse budget).
- Lighthouse gate: `apps/marketing/scripts/lighthouse.mjs`, `tests/static-server.mjs` (shared with `serve.mjs`), CI step in the `a11y` job.

## Tasks & Acceptance

**Execution:**
- [x] Hero: promise, lede, Get started and See pricing, Hub floor-view still on the right; `web-hero` at ≥ 1024 px, `web-display` below with the shot stacked
- [x] Sections in order: Install. Pair. Play. (three steps), What you get (three cards, prices from the snapshot), closing Get started band, then the footer; 128 px apart, 64 px on phone
- [x] `HeroMedia`: muted looping video that plays only while visible, stops for reduced motion, pause/play control, fallback to still cover then solid `bg`; no blur
- [x] Hero stays dark in light mode; high contrast makes it solid black, also when switched at runtime
- [x] Lighthouse mobile gate (LCP < 2.5 s, performance ≥ 90, accessibility ≥ 90) in CI

**Acceptance Criteria:**
- Given Home at ≥ 1024 px, then the hero shows the promise in `web-hero` on the left and the Hub shot on the right; below 1024 px the title uses `web-display` and the shot stacks (home.spec.ts).
- Given the rest of Home, then the sections follow in order, 128 px (64 px on phone) apart (home.spec.ts).
- Given the hero background, then it is muted, loops only while visible, stops for reduced motion and falls back to still then `bg`; nothing uses `backdrop-filter` (`shouldPlay` test, home.spec.ts).
- Given the light theme, then the hero stays dark with the scrim and the step and card sections use "Cool paper" (home.spec.ts).
- Given a throttled phone, then LCP is under 2.5 s and performance and accessibility are at least 90: measured LCP 1351 ms, performance 100, accessibility 100 (median of three).

## Verification

- `pnpm --filter @arena/marketing test` (48 passed: Home, shell, nav, theme, axe in light, dark and hc at 390 and 1440 px, pseudo-locale overflow), `pnpm --filter @arena/marketing lighthouse`, `node --test scripts/marketing-site.test.mjs` (8 passed), `pnpm run check:strings`, `check:tokens-fresh`, `check:contrast`, `check:hex`, `check:no-blur`, `check:no-remote`, `check:pins`, `pnpm --filter @arena/onboarding check` (consumes the shared UI package), `tsc --noEmit` in `apps/marketing` (no errors in `src` or `tests`).
- Not green, and not caused by this story: `check:tokens-used` (16 unknown tokens in `apps/hub`, same at the baseline) and `format:check` (`README.md`, `scripts/restore-drill-report*.mjs`, same at the baseline).
