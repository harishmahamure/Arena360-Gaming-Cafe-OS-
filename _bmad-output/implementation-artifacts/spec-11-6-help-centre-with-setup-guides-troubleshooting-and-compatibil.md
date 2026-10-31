---
title: 'Story 11.6: Help centre with setup guides, troubleshooting and compatibility lists'
type: 'feature'
created: '2026-10-31'
status: 'review'
review_loop_iteration: 0
baseline_revision: '55e54aea'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
deferred:
  - 'The hardware data file `apps/marketing/src/data/hardware.json` ships with no rows: no printer or drawer has been printed from on reference hardware yet (`docs/compat/hardware.md` is empty), and the page does not publish a result nobody measured. It says "No devices are listed yet". Rows are added to the file (category `printer` or `drawer`, `tested_on` windows and/or android) as the monthly hardware bench (Story 5.39) produces them; `scripts/hardware-compat.mjs` does not write this file yet.'
  - 'Article text is first-draft launch content written from the product as specified; the owner of the Help centre should read each article against the shipped Hub and Kiosk and move its `reviewed` date.'
  - 'The support channel block names the three plans and their channels (community and Help centre on Free, email within 24 h on Basic, chat within 4 h on Premium) but links to no contact form or address: none exists yet and the page must not ask for personal data.'
  - 'The search index is built at deploy and embedded in the Help page, so it is searched in the browser with no request. It lists articles only; the hardware list is filtered by its own brand and connection controls.'
---

<intent-contract>

## Intent

**Problem:** `/help` does not exist (the nav, the footer, the 404 page and the Downloads requirements link all point at it), so an owner cannot find setup steps, a fix, or which printers work.

**Approach:** Markdown articles in an Astro content collection (`src/content/help`, file name = stable slug), a Help page with three categories and a static search, one page per article, and a hardware compatibility table fed by a versioned data file with a `category` field.

## Boundaries & Constraints

**Always:** each article has title, summary, category and last-reviewed date in its front matter; the build fails on a link to a missing slug (`/help/<slug>`) or a slug that is not lower-case words; the hardware file is parsed on every build and a bad row fails it; `category` is `printer|drawer`; the "Tested device" badge shows only when `tested_on` has both windows and android; search runs in the browser over an index embedded at build time; every article ends with the plan support channels and asks for no personal data; the `#hardware-compatibility` anchor stays (Downloads links to it); words in the catalogue (`site.help.*`); tokens only.

**Never:** a TV, HDMI-CEC or smart-plug row or note; a hardware result that was not measured; a form, input or request for personal data on an article.

## I/O & Edge-Case Matrix

| Scenario | State | Expected Output / Behavior |
|----------|-------|---------------------------|
| Open Help | any | Setup, Troubleshooting, Hardware compatibility and a search box |
| Search | "printer" | the two printer articles, title matches first, under 200 ms, no request |
| No match | "zxqv" | "No articles match — try Troubleshooting or contact support" |
| Broken link | article links `/help/nope` | build fails naming `article -> /help/nope` |
| Filter | brand and connection chosen | only matching rows; "No devices match these filters" when none |
| Tested | `tested_on` windows + android | "Tested device" badge; one Hub only: no badge |
| Empty file | `rows: []` | "No devices are listed yet", no table |
| 390 px | any | nothing scrolls sideways; the table scrolls inside its own region |

</intent-contract>

## Code Map

- `apps/marketing/src/content.config.ts`, `src/content/help/*.md` -- the `help` collection and the eleven launch articles (6 Setup, 5 Troubleshooting).
- `src/lib/help.ts` -- pure rules: categories, link check (`brokenLinks`, `assertHelp`), search (`searchHelp`, `searchIndex`), hardware schema (`parseHardware`, `isTested`, `filterHardware`). `src/lib/help-content.ts` -- reads the collection and the data file at build time and runs the checks.
- `src/data/hardware.json` -- the versioned hardware file (`version`, `rows`).
- `src/pages/help/index.astro`, `src/pages/help/[slug].astro`, `src/pages/sitemap.xml.ts` (lists the articles), `src/components/HelpSearch.svelte`, `HardwareTable.svelte`, `src/styles/help.css`.
- `packages/ui/src/messages/en.json` -- `site.help.*`.
- Tests: `tests/help.spec.ts` (new), `a11y.spec.ts` and `pseudo.spec.ts` (now cover `/help`; a11y also an article), `tests/fixtures/hardware.json` (builds read it through `PUBLIC_HARDWARE_DATA` in `tests/serve.mjs`), `scripts/marketing-site.test.mjs` (link check, search, schema).

## Tasks & Acceptance

**Execution:**
- [x] Content collection with title, summary, category, reviewed date; slug from the file name
- [x] Launch articles: install the Hub (Windows, Android), pair a kiosk, printer and cash drawer, standby PC, invite staff; kiosk cannot find the Hub, printer offline, Hub slow, branch offline in the Owner app, restore from an export
- [x] Build fails on an internal link to a missing slug
- [x] Help page with three categories, search box and article lists (title, summary, reviewed date)
- [x] Hardware compatibility table from the versioned data file with `category`, a Tested device badge, filters by brand and connection; no TV, CEC or smart-plug content
- [x] Static search index built at deploy, searched in the browser, with the no-match message
- [x] Article pages end with the support channel for each plan and no personal-data fields
- [x] Articles in the sitemap; `/help#hardware-compatibility` resolves for Downloads

**Acceptance Criteria:**
- Given Help opens, then it shows Setup, Troubleshooting, Hardware compatibility and a search box (help.spec.ts).
- Given the build, then the launch article set exists, each with title, summary, reviewed date and slug, and a link to a missing slug fails it (help.spec.ts, marketing-site.test.mjs).
- Given the data file, then rows list brand, model, connection, paper, result and date checked, the badge marks only both-Hub rows, and the table filters by brand and connection (help.spec.ts, marketing-site.test.mjs).
- Given a search term, then matches return within 200 ms with no request, and no match shows the stated message (help.spec.ts).
- Given any article, then it ends with the Free, Basic and Premium support channels and holds no form or input (help.spec.ts).

## Spec Change Log

## Review Triage Log

## Verification

- `pnpm --filter @arena/marketing test` (132 passed, incl. axe in light, dark and hc at 390 and 1440 px for `/help` and an article and the pseudo-locale overflow check for `/help`; the known theme-toggle tab contrast flake showed once on `/downloads` and passed on rerun), `node --test scripts/release-manifest.test.mjs scripts/marketing-site.test.mjs scripts/check-strings.test.mjs` (35 passed), `pnpm run check:strings`, `check:tokens-fresh`, `check:contrast`, `check:hex`, `check:no-blur`, `check:pins`, `check:secrets`, `check:no-remote`, `prettier --check`, `tsc --noEmit` in `apps/marketing` (clean apart from ignored `dist*` output).
