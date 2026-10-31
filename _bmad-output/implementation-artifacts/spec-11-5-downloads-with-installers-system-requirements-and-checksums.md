---
title: 'Story 11.5: Downloads with installers, system requirements and checksums'
type: 'feature'
created: '2026-10-31'
status: 'done'
review_loop_iteration: 0
baseline_revision: 'c0429149'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
deferred:
  - 'No Windows installer release pipeline exists yet. `scripts/release-manifest.mjs add` hashes an installer and writes or updates its line in `releases/manifest.json`; the pipeline that builds and signs the installers must call it and upload the manifest and files to the R2 `releases/` prefix behind `downloads.arena360.cloud`. Until then every file row reads "Coming soon".'
  - 'R2 needs a CORS rule allowing the site origins to read `releases/manifest.json`, and the `downloads.arena360.cloud` custom domain. Without CORS the browser read fails and the page shows the dated build-time list.'
  - 'The Android Hub row is a Google Play link only (`in.arena360.hub`): version, size and SHA-256 do not apply to a Play install, so none is shown. Pi shows "Coming soon" until a `pi-image` line exists.'
  - 'The requirements link points to `/help#hardware-compatibility`; Story 11.6 must keep that anchor on the Help centre. Until it exists the link reaches the 404 page.'
  - 'The signature URL is validated in the manifest but not linked from the page: the page never links a file whose checksum it does not show.'
---

<intent-contract>

## Intent

**Problem:** `/downloads` does not exist (the nav and footer link to it and get the 404), so an owner cannot fetch the Hub or Kiosk installer, check it is genuine, or see what hardware it needs.

**Approach:** A static Astro page reading `releases/manifest.json` at build time and again in the browser (Svelte island), so a new release appears without a redeploy. One row each for Windows Hub, Windows Kiosk, Android Hub (Play) and the Pi image; a requirements section. `scripts/release-manifest.mjs` is the pipeline's manifest writer.

## Boundaries & Constraints

**Always:** only complete stable manifest lines become rows (checksum, https signature, safe file name); installer URL is `releases/<product>/<version>/<file>`; newest release per product; SHA-256 in a readonly monospace field with Copy; "How to verify" shows the PowerShell `Get-FileHash` line; a missing product reads "Coming soon" with no link; a failed browser read keeps the build-time list and says "Showing the list from <date>"; words in the catalogue (`site.downloads.*`); tokens only.

**Never:** an APK link; a link to a file whose checksum is not shown; a typed version, size or date; moving focus on copy.

## I/O & Edge-Case Matrix

| Scenario | State | Expected Output / Behavior |
|----------|-------|---------------------------|
| Released | manifest has Hub and Kiosk | version, size, date, SHA-256 field, download link, verify disclosure |
| Missing | no Pi line | "Coming soon", no link, no field |
| New release | browser manifest adds Pi | Pi row gains its link without reload |
| Incomplete line | no checksum | row stays "Coming soon" |
| Fetch fails | browser read aborted | build-time rows stay; "Showing the list from 31 Oct 2026" |
| 390 px | any | rows stack; no sideways scroll |

</intent-contract>

## Code Map

- `apps/marketing/src/pages/downloads.astro`, `components/DownloadList.svelte`, `styles/downloads.css` -- page, island, styles.
- `apps/marketing/src/lib/manifest.ts` -- pure manifest rules (shared with the script test); `lib/releases.ts` -- rows, URLs, formats, `fetchManifest`; `lib/site.ts` -- `RELEASES_ORIGIN`, `RELEASES_MANIFEST_URL`, `PLAY_URL`.
- `packages/ui/src/messages/en.json` -- `site.downloads.*`.
- `scripts/release-manifest.mjs` (+ test) -- hashes an installer and adds its manifest line.
- Tests: `apps/marketing/tests/downloads.spec.ts` (new), `a11y.spec.ts`, `pseudo.spec.ts` (now cover `/downloads`), `tests/serve.mjs` (builds read the fixture `tests/fixtures/releases/manifest.json` via `PUBLIC_RELEASES_MANIFEST`).

## Tasks & Acceptance

**Execution:**
- [x] Manifest format, parser and `scripts/release-manifest.mjs` that writes it (product, channel `stable`, version, file, size, SHA-256, signature URL, published_at)
- [x] Downloads page reads the manifest at build time and again in the browser
- [x] Rows for Windows Hub, Windows Kiosk, Android Hub (Google Play link) and the Pi image with version, size, date, copyable SHA-256 and "How to verify" (`Get-FileHash`)
- [x] "Coming soon" row with no link for a product missing from the manifest
- [x] System requirements section with the stated figures, "Windows 7 is not supported", any ESC/POS printer and a Help centre compatibility link
- [x] Failed browser read keeps the build-time list with "Showing the list from <date>"; no link to a file without a shown checksum

**Acceptance Criteria:**
- Given the manifest, then Hub and Kiosk rows show version, size, date, SHA-256 and a link; Pi is "Coming soon" (downloads.spec.ts).
- Given a later manifest with Pi, then the Pi row gets its link without reload; a line without a checksum gets none (downloads.spec.ts).
- Given a failed manifest read, then the dated build-time list stays and every file link sits beside its checksum (downloads.spec.ts).
- Given the requirements section, then it states each device's needs and links to `/help#hardware-compatibility` (downloads.spec.ts).
- Given the script, then it hashes the real file and writes lines the page accepts (release-manifest.test.mjs).

## Spec Change Log

## Review Triage Log

- 2026-10-31 code review: no blocking findings. Manifest parsing, URL and PowerShell-name safety, no-checksum-no-link rule and stale fallback verified; typecheck, prettier, string check, node tests and 106 Playwright tests green.

## Verification

- `pnpm --filter @arena/marketing test` (106 passed, incl. axe in light, dark and hc at 390 and 1440 px and the pseudo-locale overflow check for `/downloads`; one a11y run flaked once on the theme-toggle tabs' contrast and passed on rerun), `node --test scripts/release-manifest.test.mjs scripts/marketing-site.test.mjs scripts/check-strings.test.mjs` (32 passed), `pnpm run check:strings`, `check:tokens-fresh`, `check:contrast`, `check:hex`, `check:no-blur`, `check:pins`, `check:secrets`, `check:no-remote`, `prettier --check`, `tsc --noEmit` in `apps/marketing` (clean). `check:tokens-used` (`--ad-reserved` in the Hub) and `check:layers` (`arena-api -> arena-store`) fail on the untouched baseline too.
