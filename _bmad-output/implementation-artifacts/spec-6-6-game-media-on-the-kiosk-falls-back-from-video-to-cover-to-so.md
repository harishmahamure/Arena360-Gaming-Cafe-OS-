---
title: 'Story 6.6: Game media on the kiosk falls back from video to cover to solid'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '3d7319ed49b153940663ed810b41ce522277e4f8'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Kiosk screens (attract, Home, Games) need game art and trailers, but nothing in the shared UI loads them safely: a missing, undecodable or blocked file would show a broken player, a video could play twice at once or behind a running game, and copy over art has no guaranteed contrast.

**Approach:** A shared `GameMedia` component in `@arena/ui` shows the first usable step of video → cover → icon → solid for one library entry, draws the scrim under the copy, and exposes one 44px `video-control`. A pure module (`game-media.ts`) holds the URL rule, the chain and the one-video-at-a-time claims. A `check:no-blur` gate fails the build on `backdrop-filter` in the branch surfaces and the shared package. The gallery gets a fixture route and a Playwright suite.

## Boundaries & Constraints

**Always:**

- **Chain.** `video` (curated, flash-screened, from the Hub) → `cover` → `icon` (launcher / executable icon the kiosk extracted locally) → solid `bg`. Any load or decode error, or a refused `play()`, skips straight to the next step with no visible error; the failed URL is remembered for that component.
- **URLs.** Only Hub media URLs are used: `/media/<64 lower-case hex>` relative to the page, or an absolute `http(s)` URL on port `7401` with that path and no query or fragment. Anything else is never loaded (it counts as absent).
- **Video.** Muted (property, default and volume 0), looping, inline, `preload=auto`, no picture-in-picture or remote playback; invisible until a frame is ready so the cover beneath never flickers. One video plays on the screen at a time: the newest claim plays, the older ones show their cover and resume when it is released.
- **Scrim.** Drawn whenever media is drawn (`hero`: bottom 0.92 → 0.15 at 55% plus left 0.85 → 0.75 at 45% → 0 at 70%; `tile`: 0.85 to 25%, 0 at 55%), from `--scrim-base`; copy sits in its copy zone (hero: left 45% × bottom 50%; tile: bottom 25%). No `backdrop-filter` or live blur anywhere.
- **Video control.** 44px round, `bg` fill, `border-strong` ring, the icon of the next action; visible while a video is loaded, a Tab stop, activated by click, Enter or Space; pauses on the current frame (stays paused for that video) and switches to Play, which resumes.
- **`gameRunning`.** When true the video element is removed and its file released (`src` cleared, `load()`); the cover shows until it is false again.
- **High contrast.** Resolved from the nearest `[data-theme]` (or the `highContrast` prop): solid `bg`, no image, video, scrim or control; copy still renders.
- **Cache.** Media is addressed by content hash, so a screen that shows it again reuses the WebView's cached file (the Hub sends `immutable`); the component never adds cache-busting.
- **Gate.** `pnpm check:no-blur` fails on `backdrop-filter` (any prefix or case) in `apps/{hub,kiosk,kitchen}/src` and `packages/ui/src`; comment lines and `blur-ignore` lines are exempt. It is part of `pnpm check` and CI.

**Never:** an internet or `data:` URL for media; audio from a video; a second playing video; a visible media error; a kiosk screen or launching (Stories 6.7, 6.23); wiring the real `gameRunning` source (6.23).

## I/O & Edge-Case Matrix

| Scenario                | Input / State                | Expected Output / Behavior                           | Error Handling |
| ----------------------- | ---------------------------- | ---------------------------------------------------- | -------------- |
| Video ok                | video + cover                | video plays muted, loops, under scrim, control shown | —              |
| Video undecodable / 404 | bad video, cover ok          | cover shown, no control                              | silent         |
| Cover fails             | no video, cover 404, icon ok | small centred icon                                   | silent         |
| Nothing                 | all absent or failed         | solid `bg` with scrim and copy                       | —              |
| Internet URL            | `https://cdn…/a.mp4`         | never requested; next step                           | —              |
| Two components          | both with a video            | newest plays; the other shows its cover              | —              |
| Game running            | `gameRunning` true           | video removed, cover only; back when false           | —              |
| High contrast           | `data-theme="hc"`            | solid `bg`, no scrim, video, control                 | —              |
| Shown again             | same cover in two views      | one fetch                                            | —              |

</intent-contract>

## Code Map

- `packages/ui/src/game-media.ts` (new) -- `isHubMediaUrl`, `mediaStage`, `claimVideo` / `isActiveVideo` / `onVideoClaims`, `nextClaimId`.
- `packages/ui/src/components/GameMedia.svelte` (new), `index.js` exports, `messages/en.json` (`ui.game_media.*`, `gallery.game_media.*`).
- `scripts/check-no-blur.mjs` (+ test), `package.json`, `.github/workflows/ci.yml`, `README.md`.
- `apps/gallery/src/routes/game-media/+page.svelte` (fixture), `apps/gallery/tests/game-media.spec.ts`.
- `scripts/game-media.test.mjs` (URL rule, chain, claims).

## Tasks & Acceptance

**Execution:**

- [x] `@arena/ui` -- `game-media` rules module and `GameMedia` component (chain, scrim, video-control, `gameRunning`, HC)
- [x] Lint -- `check:no-blur` script, test, `pnpm check` and CI wiring, README
- [x] Gallery -- fixture route and Playwright suite (chain, muted loop, control by Tab / Enter / Space, one video, game running, HC, cache by hash, no `backdrop-filter`, axe in dark and HC)
- [x] Unit tests for the URL rule, the chain and the claims

**Acceptance Criteria:**

- Given a `game-media` for a library entry, then it plays the curated video, else shows the cover, else the icon, else solid `bg`, and any error falls to the next step with nothing visible.
- Given a playing video, then it is muted, loops, uses only Hub media URLs and only one plays at a time.
- Given media behind copy, then the scrim is drawn with the copy in its zone and no `backdrop-filter` is used; the lint fails the build on it.
- Given the `video-control`, when activated by click, Enter or Space, then the video pauses on its frame and the button shows Play; it is always visible while video plays and reachable by Tab.
- Given `gameRunning`, then the video is unloaded and only the cover shows until it is false.
- Given the HC theme, then media is solid `bg` with no scrim, video or control.
- Given media fetched before, then showing it again does not refetch (two fixture views).

## Spec Change Log

## Review Triage Log

## Design Notes

The component takes ready URLs and does no fetching, so the shell decides how a request reaches `:7401` with the device token (Story 6.2 requires a bearer header, which a bare `<video src>` cannot send): that belongs with the kiosk wiring in 6.23. Under a playing video the cover (or icon) stays drawn and the video fades in on its first frame, so a slow or failed video never flashes. The icon is shown small and centred because it is launcher art, not a backdrop.

## Verification

**Commands:**

- `node --test scripts/game-media.test.mjs scripts/check-no-blur.test.mjs` -- expected: pass
- `pnpm check:no-blur`, `pnpm check:strings`, `pnpm check:hex`, `pnpm check:tokens-fresh`, `pnpm format:check` -- expected: pass
- `pnpm --filter ./apps/gallery check`, `pnpm --filter ./apps/kiosk check`, `pnpm --filter ./apps/hub check` -- expected: 0 errors
- `playwright test tests/game-media.spec.ts tests/a11y.spec.ts --project=chromium` in `apps/gallery` -- expected: pass

## Implementation Notes

- Not run / pre-existing: `check:tokens-used` unknown tokens in `apps/hub` (`--sticky-bottom`, `--ad-shrink`, `--ad-reserved`) and the deploy / kustomize script tests (12 failures) fail identically at the baseline commit.
- Deferred: authenticating `:7401` requests from a `<video>` / `<img>` (kiosk wiring, 6.23); extracting launcher / executable icons on the kiosk (the component only shows one it is given); the real `gameRunning` source (6.23).
