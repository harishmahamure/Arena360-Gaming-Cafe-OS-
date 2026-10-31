---
title: 'Story 11.4: Product pages for Hub, Kiosk, Console and Owner with demo videos'
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: 'c30bff40'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
deferred:
  - 'The 16 media files (poster.jpg, demo.webm, demo.mp4, captions.en.vtt, transcript.txt for each of Hub, Kiosk, Console and Owner) are not produced by this story. Until they are uploaded to `marketing/product/<id>/` in R2 behind `media.arena360.cloud`, every demo shows its poster area with "Video unavailable — read the walkthrough below". The R2 bucket also needs a CORS rule allowing the site origins (captions load cross-origin) and the `media.arena360.cloud` custom domain.'
  - 'The step illustrations are simple token-drawn SVG motifs (`StepArt.astro`), not screenshots. Real stills can replace them without changing the page structure.'
  - 'When a clip fails after the viewer pressed play, the player is replaced by the poster and message, so keyboard focus that was on the player returns to the page.'
---

<intent-contract>

## Intent

**Problem:** `/product` does not exist (the nav and Home link to it and get the 404), so a prospective owner cannot see how the Hub, Kiosk, Console time and Owner app work before installing.

**Approach:** A static Astro page: an overview with an in-page link list, then one section each (`#hub`, `#kiosk`, `#console`, `#owner`) with a demo video and a 3-5 step illustrated walkthrough. Structure and media URLs are in `lib/product.ts`; words are in the catalogue (`site.product.*`). The video is a Svelte island around the native `<video controls preload="none">`, so nothing loads or plays before the press and all controls are the browser's own.

## Boundaries & Constraints

**Always:** media from the shared `marketing/` R2 prefix via `mediaUrl`; English WebVTT captions as the `default` track; a text transcript link per video; the poster and the message "Video unavailable — read the walkthrough below" when the clip cannot load or play; the video frame keeps dark tokens in light and dark (High contrast swaps in `hc`); tokens only; Console says plainly what exists today with no date.

**Never:** autoplay, muted-by-default or looping demos; a custom player that replaces the native controls; moving focus on play; `backdrop-filter`; a typed price or a promised date.

## I/O & Edge-Case Matrix

| Scenario | State | Expected Output / Behavior |
|----------|-------|---------------------------|
| Open | `/product` | overview, link list to four anchors, four sections in order |
| Deep link | `/product#console` | the Console section is in view |
| Idle | page loaded | poster shown, `preload="none"`, no media requested, not playing |
| Press play | clip loads | native player plays with sound, English captions on |
| Clip fails | request aborted | poster and "Video unavailable — read the walkthrough below"; no player |
| No playable type | `canPlayType` empty | same message at once |
| No JavaScript | scripts off | native player with poster and captions track, transcript link |
| 390 px | any | steps in one column; no sideways scroll |

</intent-contract>

## Code Map

- `apps/marketing/src/pages/product.astro`, `styles/product.css`, `components/ProductVideo.svelte`, `components/StepArt.astro`, `lib/product.ts` (sections, steps, `demoMedia`), `lib/site.ts` (`MEDIA_ORIGIN`, `mediaUrl`).
- `packages/ui/src/messages/en.json` (`site.product.*`).
- Tests: `apps/marketing/tests/product.spec.ts` (new), `a11y.spec.ts` and `pseudo.spec.ts` (now cover `/product`), `scripts/marketing-site.test.mjs` (media prefix).

## Tasks & Acceptance

**Execution:**
- [x] Product overview with an in-page link list; one section each for Hub, Kiosk, Console and Owner reachable as anchors
- [x] Each section: illustrated walkthrough of three to five steps and a 90-second demo video
- [x] Video shows its poster and does not load or play until pressed; plays with sound after the press; English WebVTT captions on by default; transcript link; served from the `marketing/` R2 prefix
- [x] A failed or unplayable video keeps the poster with "Video unavailable — read the walkthrough below"
- [x] Console section says console time runs from the Hub tile at launch and that the TV and QR self-start arrive with Pi support, with no date
- [x] Native, labelled, Tab-reachable controls; pressing play moves no focus
- [x] Media frame stays dark in light mode

**Acceptance Criteria:**
- Given Product, then the overview, link list and four anchored sections with 5, 4, 3 and 4 steps exist (product.spec.ts).
- Given a demo, then it has its poster, `preload="none"`, no autoplay or mute, requests no clip, has a default English captions track and a transcript link, from `marketing/product/<id>/` (product.spec.ts, marketing-site.test.mjs).
- Given a failing clip or an unplayable browser, then the poster area shows the message and no player (product.spec.ts).
- Given the Console section, then the note names the Hub tile and Pi support and gives no date (product.spec.ts).
- Given keyboard use, then the video is reachable by Tab and Space leaves focus on it (product.spec.ts).

## Verification

- `pnpm --filter @arena/marketing test` (85 passed, including axe in light, dark and hc at 390 and 1440 px and the pseudo-locale overflow check for `/product`), `node --test scripts/marketing-site.test.mjs` (10 passed), `pnpm run check:strings`, `check:tokens-fresh`, `check:contrast`, `check:hex`, `check:no-blur`, `check:pins`, `pnpm --filter @arena/onboarding check`, `prettier --check` on `apps/marketing` and the catalogue, `tsc --noEmit` in `apps/marketing` (no errors in `src` or `tests`; the only output is from the git-ignored `dist-pseudo` build folder).
