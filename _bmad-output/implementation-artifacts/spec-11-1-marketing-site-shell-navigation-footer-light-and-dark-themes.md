---
title: 'Story 11.1: Marketing site shell — navigation, footer, light and dark themes and high contrast'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '599f7859'
review_loop_iteration: 1
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md'
deferred:
  - 'The header and footer link to /product, /pricing, /downloads, /help, /faq and /legal/*; those pages arrive in Stories 11.2 to 11.7, so the links 404 (in the same shell) until then.'
  - 'The deploy to Cloudflare Workers static assets is configured (`apps/marketing/wrangler.jsonc`, staging via `pnpm build:staging`) but not run: there is no Cloudflare account or pipeline for it in this repository, and `wrangler` could not be added offline.'
  - '`check:deploy` and `check:layers` fail on this machine for reasons outside this story (no `kubectl` on PATH; two forbidden Rust crate edges). The same failures exist at the baseline revision.'
---

<intent-contract>

## Intent

**Problem:** The marketing app is a placeholder; every later page needs a shared shell, a theme that never flashes, and a phone menu that is usable by keyboard.

**Approach:** An Astro layout renders a sticky header and a footer around each page. The interactive parts are Svelte islands in `@arena/ui` (`SiteNav`, `SiteThemeToggle`, `ContrastSwitch`, `LinkButton`) that share one theme state (`site-theme.svelte.js`). High contrast is a separate on/off switch under its own key (`arena.contrast`), defaulting from `prefers-contrast: more`; an inline head script paints the stored result before first paint.

## Boundaries & Constraints

**Always:** every visible string comes from `en.json` (`site.*`); Sora comes from the bundled font; tokens only; no translucency or blur; the 404 page uses the same shell.

**Never:** no third-party font, CDN or script; no new page content beyond a Home heading placeholder and the 404 (Stories 11.2 onward).

## I/O & Edge-Case Matrix

| Scenario | State | Expected Output / Behavior |
|----------|-------|---------------------------|
| First visit | nothing stored, OS light | `data-theme="light"`; System selected; High contrast off |
| OS prefers more contrast | nothing stored | `hc`; the switch reads on; choosing off sticks (`arena.contrast=off`) |
| Mode chosen | `arena.theme=dark` | dark before first paint, even with no script bundle loaded |
| Phone menu | < 768 px | full-screen dialog; Tab and Shift+Tab stay inside; Esc, close or a link closes; focus returns to the hamburger |
| Unknown path | any | 404.html in the same shell with Home and Help links |
| Staging build | `astro build --mode staging` | staging origin, `noindex`, robots.txt disallows all |

</intent-contract>

## Code Map

- `apps/marketing/src/layouts/Layout.astro`, `components/SiteFooter.astro`, `styles/site.css`, `pages/{index,404}.astro`, `pages/sitemap.xml.ts`, `pages/robots.txt.ts`, `lib/site.ts`, `wrangler.jsonc`.
- `packages/ui/src/components/{SiteNav,SiteThemeToggle,ContrastSwitch,LinkButton}.svelte`, `site-theme.svelte.js`, `theme.js` (`resolveSiteTheme` and the contrast key), `messages/en.json` (`site.*`), `messages/index.js` (`PUBLIC_PSEUDO` builds start in the pseudo-locale).
- Tests: `apps/marketing/tests/*.spec.ts` (shell, theme, phone nav, axe in light, dark and hc at 390 and 1440 px, pseudo-locale overflow) served by `tests/serve.mjs`; `scripts/marketing-site.test.mjs`; CI runs the suite in the a11y job.

## Tasks & Acceptance

**Execution:**
- [x] Static Astro build, islands from the shared UI package, Sora from the site, Workers static assets config with staging
- [x] 80 px desktop header (wordmark, five links, one Get started) and 64 px sticky phone header with hamburger and a focus-trapping full-screen menu
- [x] Footer with Legal, Help, FAQ, Downloads and the System · Light · Dark toggle plus a separate High contrast switch (also in the phone menu)
- [x] Inline head script paints the stored theme before first paint; `prefers-contrast: more` defaults High contrast on
- [x] Light mode links underlined in `accent-text`; hc tokens with 2 px white outlines
- [x] Skip link, landmarks, one h1, title, description, canonical, Open Graph, sitemap.xml, robots.txt, 404 page
- [x] Axe (zero serious or critical) in all three themes at 390 and 1440 px, 7:1 in hc, 44 px targets, no sideways scroll at +30% strings

**Acceptance Criteria:**
- Given any page, when it renders, then the above holds in light, dark and high contrast (39 Playwright tests, all passing).

## Verification

- `pnpm --filter @arena/marketing test` (39 passed), `node --test scripts/marketing-site.test.mjs`, `pnpm run check:strings`, `check:tokens-fresh`, `check:contrast`, `check:hex`, `check:no-blur`, `check:pins`, `pnpm --filter @arena/onboarding check` (consumes the shared UI package).

## Review notes (2026-10-31)

Fixed: a stored legacy `arena.theme=hc` no longer defeats an explicit High contrast Off (the island migrates it to `arena.contrast=on`; the head script and `resolveSiteTheme` agree); the 404 page has no canonical or `og:url` and carries `noindex`; the sitemap skips `_`-prefixed pages; `PUBLIC_PSEUDO` only accepts `1` or `true`; the phone-menu scroll lock restores the previous `overflow`; the test server rejects malformed URLs and sibling-directory paths. Added a staging-build test and 404 and `routeOf` assertions.

Deferred (not blocking): no-JS visitors always get the dark default; `forced-colors` is not mapped to hc; the open phone menu is not run through axe; the 4 px focus ring and 3:1 non-text pairs in hc have no Playwright assertion; no cross-tab `storage` sync; 768 to 1023 px header fit is untested; no `_headers`, favicon or `og:image`.
