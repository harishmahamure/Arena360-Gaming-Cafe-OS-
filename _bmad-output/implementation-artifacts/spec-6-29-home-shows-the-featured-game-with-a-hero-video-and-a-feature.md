---
title: 'Story 6.29: Home shows the featured game with a hero video and a featured row'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '4440e9297333b79c9cc87372a23fb0550790ea6b'
review_loop_iteration: 0
followup_review_recommended: false
warnings: []
deferred:
  - summary: >-
      Nothing in the kiosk app fetches `/media/<sha256>` from the Hub yet: the page asks for it relative to its own origin and the Tauri CSP is `default-src 'self'`. In the real shell the hero (like the attract screen and every tile) therefore falls back to solid until a media transport exists; the browser tests serve the files by route.
    evidence: |-
      `mediaUrl` in attract.ts returns `/media/<sha>`; tauri.conf.json csp has no media-src or Hub origin; attract.spec.ts notes "the kiosk media transport".
    location: >-
      apps/kiosk/src-tauri/tauri.conf.json, apps/kiosk/src/lib/attract.ts
    severity: medium
  - summary: >-
      The Games tab grid and the "Most played here" carousel still draw tiles without covers; only the Home row passes the Hub's cover to its tiles.
    evidence: |-
      Games.svelte and MostPlayed.svelte do not pass `cover` to GameTile; the Hub's `media` list is now available in the same view.
    location: >-
      apps/kiosk/src/lib/Games.svelte, apps/kiosk/src/lib/MostPlayed.svelte
    severity: low
  - summary: >-
      "You last played N days ago" uses the PC's wall clock against the Hub's play time, so a badly set PC clock shifts the day count.
    evidence: |-
      Home.svelte calls Date.now(); the Hub's `last_played[].at` is Unix ms from the Hub clock. The count is floored and never negative.
    location: >-
      apps/kiosk/src/lib/Home.svelte
    severity: low
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-28-the-apps-tab.md'
---

<intent-contract>

## Intent

**Problem:** The Home tab is an empty placeholder: a player sees no hero, no featured game and no way to start the café's favourite game in one tap.

**Approach:** Home draws a `hero` behind the shell: the branch's most-played game of the last 7 days that this PC has (else the first library game with media, else the first game), with the Hub's video, else cover, else solid `bg`. Its copy (eyebrow, `kiosk-display` title, meta line, Play now, Game details) is in the scrim's zone. Below the bar a featured row shows the featured game as `game-tile-featured`, the next most-played games, and an `all-games-tile`. The Hub's `games.most_played_result` now also carries the stored media of the library games (`media`); the kiosk service's `games` cards carry launcher and age rating.

## Boundaries & Constraints

**Always:** Play now launches through the same `game_launch` as Story 6.23 and the video is unloaded from the click; the video is muted and local; the pause is kept for the session; copy stays in the left-45% × bottom-50% zone; the media chain is video → cover → `bg` (solid in high contrast); the Hub names the player for last-played, never the kiosk.

**Never:** a new permission or mutation path (the media list is a read added to an existing read frame); an internet media URL; the featured outline reused as a focus ring; the video playing while a game runs.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Most played | Hub top has games this PC has | hero = the first of them; eyebrow "Most played at <branch> this week" |
| No plays | top empty or Hub unreachable | hero = first game with media, else first game; eyebrow "Featured at <branch>" |
| Last played | player played it 2 days ago | meta "genre · launcher · You last played 2 days ago"; none if never |
| Play now | any | `game_launch`; hero video stops entirely |
| Game details | any | card: genre, age rating, launcher ("Not listed" if none); Esc closes, focus back |
| Row | ranked games | featured (190×260, magenta outline) then next most played, then All games; 4 / 6 / 8 tiles at 1366 / 1920 / 2560 |
| All games | activated | Games tab, no filter |
| Pause | video-control | holds the frame across tabs for the session |
| No games | empty library | solid `bg` + "No games here yet — ask at the counter.", no Play now, no row |

</intent-contract>

## Code Map

- `crates/arena-proto/src/kiosk.rs`, `lib.rs` -- `GameMediaRef`, `MostPlayed.media` (serde default), `MAX_GAME_MEDIA`.
- `crates/arena-core/src/{game_plays,devices}.rs` -- `game_media`: enabled catalogue-linked library games with a stored video or cover, in name order.
- `crates/arena-api/src/lan/games.rs` -- fills `media` from the Hub's media cache.
- `crates/arena-kiosk-svc/src/games.rs` -- `Card` gains `launcher`, `age_rating`.
- `apps/kiosk/src/lib/{games.ts,Home.svelte,GameTile.svelte,Shell.svelte}`, `routes/+page.svelte` -- featured rules, Home, featured tile, the most-played ask on Home too.
- `packages/ui/src/components/GameMedia.svelte` -- `bind:paused`, hero control bottom-right; `messages/en.json`; `docs/kiosk-accessibility.md`; generated types.

## Tasks & Acceptance

**Execution:**
- [x] Protocol and Hub: media refs in `games.most_played_result`, core lookup, tests
- [x] Kiosk service: launcher and age rating on cards, test
- [x] Kiosk app: featured rules, Home (hero, details, row), featured tile, pause kept, strings, a11y doc
- [x] Tests: `home.spec.ts` (browser) and the Rust tests; existing specs updated for the wider shapes

**Acceptance Criteria:**
- Given Home, when it renders, then the hero shows the most-played game of the last 7 days (else the first library game with media) with the eyebrow, `kiosk-display` title, meta line (omitted last-played if never played), Play now and Game details inside the copy zone, ≤ 864px wide (≤ 720px at 1366).
- Given Play now, when activated, then the game launches as in 6.23 and the hero video stops entirely.
- Given Game details, when activated, then a card shows genre, age rating and launcher; Esc closes it and focus returns to Game details.
- Given the featured row, when it renders, then it shows the featured tile (190×260, 3px magenta outline offset 3px), then the next most-played installed games, ending with All games, which opens Games with no filter.
- Given the video control at the hero's bottom-right, when the player pauses, then the hero holds its frame for the rest of the session's Home view.
- Given a library with no games, when Home renders, then solid `bg` with "No games here yet — ask at the counter." and no Play now.

## Design Notes

Genre is the library `category`. The hero sits in a fixed layer behind the shell (the shell isolates its stacking context); the content over it lets clicks through except its own parts. The video-control moved to the bottom-right of every `hero` scrim (login and Home, as DESIGN.md says). The Hub's media list is keyed by library entry id (the id plays are recorded with), found through each entry's catalogue id.

## Verification

**Commands:**
- `cargo test -p arena-core --lib game_`, `cargo test -p arena-proto -p arena-kiosk-svc --lib games`, `cargo test -p arena-api --test lan most_played` -- expected: pass
- `cargo fmt --check`, `cargo clippy -p arena-proto -p arena-core -p arena-api --all-targets -D warnings`, `cargo clippy -p arena-kiosk-svc --lib -D warnings` -- expected: clean
- `pnpm --filter @arena/kiosk check`, Playwright in `apps/kiosk` and `apps/gallery` `game-media.spec.ts`, `pnpm check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` -- expected: pass

## Auto Run Result

Status: in-review

- **Summary:** Home now shows the hero (most-played game of the week, else first game with media) with video/cover/solid, eyebrow, title, meta line, Play now and a Game details card, plus the featured row ending in All games. The Hub's most-played answer carries the library games' media; game cards carry launcher and age rating.
- **Verification:** arena-core 1158 lib tests pass; new core, proto and card tests pass; clippy and fmt clean; kiosk `check` 0 errors; full kiosk Playwright run: all pass except the 4 `theme.spec.ts` tests, which fail the same way without this story; gallery `game-media` 12 pass; strings, hex, blur, layers, pins, protocol and prettier pass. `check:generated` only reports the uncommitted regenerated types. `check:tokens-used` fails on the Hub's `--ad-reserved`, and the kiosk-svc lib tests for config, pipe DACL, lockdown and procs fail; both are unchanged from before this story.
- **Residual risks:** the real Tauri shell and a real Hub media fetch were not exercised (see `deferred`); no review pass has run yet.

## Review Result

Code review (blind, edge-case, verification-gap, acceptance layers) ran. Fixed: the running featured tile now announces "running" (`player.home.featured_tile_running`), `aria-controls` only points at the details panel while it is open, and a second activation while a launch is in flight is ignored. Checked after the fixes: kiosk `check` 0 errors, strings and prettier pass, Playwright `home`, `games` and `most-played` specs pass (45).

Deferred (not blocking):
- `game_media` caps at 64 media-bearing games in name order; a very large library can drop the featured game's media (hero falls to solid).
- `lan.rs` has no test with a populated media cache for `games.most_played` (the Hub wiring is a short closure; core is tested with a fake lookup).
- A failed most-played poll replaces the last good view with `null`, so the hero can swap to the fallback until the next answer.
- Launcher `none` shows "Installed on this PC" in details rather than "Not listed" (the spec's "if none" is read as no launcher listed).
- `daysSince` counts elapsed 24-hour periods, not calendar days.
- Focus drops to the page when the hero game changes under an open details panel; "All games" does not move the roving tab focus.
