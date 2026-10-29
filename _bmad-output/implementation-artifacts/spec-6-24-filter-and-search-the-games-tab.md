---
title: 'Story 6.24: Filter and search the Games tab'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '22f9178'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-23-the-games-tab-shows-the-library-and-launches-a-game-in-one-c.md'
warnings: []
deferred:
  - 'Roving tabindex on the rail: every rail item is a Tab stop (Up/Down also move), so a long rail costs the player many Tabs to reach the search field.'
  - 'The filter and search text are kept in the Games component, so they reset when the player leaves the tab.'
---

<intent-contract>

## Intent

**Problem:** A big library on the Games tab is a long wall of tiles; a player cannot narrow it by type or look a game up by name.

**Approach:** The service's `games` answer carries each game's library `category` and `tags`. The tab draws a `filter-rail` ("All", then every category and tag once, sorted), a right-aligned search field in the title row, and filters the grid in the app (pure rules in `games.ts`).

## Boundaries & Constraints

**Always:** filtering is instant and local (no Apply, no service round trip); one rail item active at a time; search is case-insensitive on the name and combines with the item; a rail item the library no longer has counts as All.

**Never:** a new service request or permission; a rail or search for a library with no games (the empty line stands).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Tab opens | categories Shooter, Sports; tags fps, Battle Royale | rail: All, Battle Royale, fps, Shooter, Sports; All active |
| Category and tag share a label (any case) | "FPS" / "fps" | one rail item matching both |
| Item chosen | Shooter | grid shows only Shooter games, no Apply |
| Search | "  st " | names containing "st", any case, within the active item |
| Nothing matches | | "No games match." and an underlined "Clear filters"; it resets to All, empty search, focus to search |
| Keyboard | Tab / arrows | rail → search → grid, focus ring on every stop |
| No games on this PC | | "No games here yet — ask at the counter.", no rail or search |

</intent-contract>

## Code Map

- `crates/arena-kiosk-svc/src/games.rs` -- `Card` gains `category`, `tags`.
- `apps/kiosk/src/lib/games.ts` -- `GameCard` fields, `railLabels`, `inLabel`, `visibleGames`.
- `apps/kiosk/src/lib/Games.svelte` -- rail, search, no-match link, arrow-key movement.
- `packages/ui/src/messages/en.json` -- `player.games.{title,filter_label,filter_all,search_label,no_match,clear_filters}`.
- `docs/kiosk-accessibility.md` -- the rail and search.

## Tasks & Acceptance

- [x] Service: category and tags on every card (+ test)
- [x] App: pure filter rules, `filter-rail`, search field in the title row, "Clear filters", keyboard movement
- [x] Tests: Playwright (`games.spec.ts`: rail items, instant single-select filter, search + item, layout, clear link, keyboard, dropped item, empty library)

## Verification

- `cargo test -p arena-kiosk-svc --lib games::` 10 pass (new `a_card_carries_the_category_and_tags...`); `cargo clippy -D warnings` clean on lib and bins (the `--all-targets` run fails on the `discovery` test target identically without this change).
- `pnpm --filter @arena/kiosk check` 0 errors, 0 warnings; Playwright `games`, `shell`, `hud` specs 75 pass.
- `check:strings hex no-blur layers pins protocol` and `format:check` pass.

Not run: the real Tauri shell with a live service answer carrying categories and tags.
