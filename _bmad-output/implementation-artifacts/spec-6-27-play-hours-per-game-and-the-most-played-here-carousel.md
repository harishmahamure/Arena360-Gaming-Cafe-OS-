---
title: 'Story 6.27: Play hours per game and the "Most played here" carousel'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '60834d2e49b804567e8bc365a1ade1f3c5563899'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-26-launcher-cards-open-the-player-s-own-steam-epic-or-riot-acco.md'
warnings: []
deferred:
  - summary: >-
      A play is attributed by comparing the kiosk-clock start with Hub-clock session times, with no skew tolerance.
    evidence: |-
      A kiosk clock behind the Hub can name the previous session on that PC for a game started just after a new one begins. Settling it needs the real skew between kiosk and Hub clocks.
    location: >-
      crates/arena-core/src/apply/kiosk_log.rs (record_play)
    severity: medium (unverified)
  - summary: >-
      A play still running when the kiosk service restarts or the PC loses power is never recorded (tracking is in memory), and a game started from a launcher link is timed by the launcher's handler process.
    evidence: |-
      GameState keeps tracked plays in memory only; the cleanup `programs` step is the only session-end hook. The launcher-handler limit is the same as launch time in 6.25 and is in Design Notes.
    location: >-
      crates/arena-kiosk-svc/src/games.rs
    severity: medium
  - summary: >-
      `ended_at` of a play is not bounded against the Hub clock, so a far-ahead kiosk clock writes future-dated rows.
    evidence: |-
      KioskGamePlayRecorded::validate checks the fields against each other only.
    location: >-
      crates/arena-proto/src/kiosk.rs
    severity: low
  - summary: >-
      The `most_played` app request to the Hub (app.rs dispatch, player_session::most_played) has no service-level test.
    evidence: |-
      The sibling player_extend_options forwarding has none either; the Hub frame and the UI are tested on both sides of it.
    location: >-
      crates/arena-kiosk-svc/src/app.rs
    severity: low
---

<intent-contract>

## Intent

**Problem:** The Hub knows when a game was launched but not for how long it was played, so a player cannot see what is popular at this café and an owner has no hours per game.

**Approach:** The kiosk service times each game it starts and queues a `game.play_recorded` device intent when the game's process exits or the session ends; the Hub writes a `game_plays` row (new change-log table) naming the live player and session. A new LAN frame `games.most_played` returns the branch's top 10 games by played minutes over the last 7×24 h (recomputed at most every 15 min) and the live player's last-played time per game. The service answers a `most_played` app request from it; the Games tab shows a `most-played-carousel` above the grid.

## Boundaries & Constraints

**Always:** the Hub names the player and session (the kiosk never does); a play is written once (the service stops tracking a game when it records it); a play is recorded even when the Hub is unreachable (queued, replayed once); the carousel lists only games installed on this PC and ranks what is shown "#1…" in order; activating a tile launches through the existing `game_launch`; no carousel when the window has no minutes.

**Never:** a second mutation path (the write is an intent through `apply()`); a player id or name in the `most_played` answer other than the live player's own last-played times; counting an app or a launcher as game hours; a new permission.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Game exits | game running 25 min, process exits | one `game.play_recorded{game_id, started_at, ended_at, minutes: 25}` queued; tracking dropped |
| Session ends | game still running at cleanup | recorded with `ended_at` = now before programs are ended; process exit later records nothing |
| Hub unreachable | any play ends | intent waits in the queue, applied once on reconnect |
| Most played | plays across PCs in 7 days | top 10 `{game_id, minutes}` by minutes (ties by game id), only minutes > 0; cached 15 min per Hub |
| Last played | live player has plays | `last_played: [{game_id, at}]` for that player, any window |
| Carousel | top list has an uninstalled game | skipped, later tiles move up; ranks are "#1…" over the shown tiles |
| None | no minutes in the window, or Hub unreachable | no carousel |
| Arrow keys | focus in carousel | Left/Right move focus tile to tile and scroll it into view |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{kiosk,intent,scope,lib,frame}.rs` -- `KioskGamePlayRecorded`, `IntentKind::KioskGamePlayRecorded` (`game.play_recorded`, device actor, paired scope), `MostPlayed*` types, frame `games.most_played` / `games.most_played_result`; model on `KioskLauncherOpened` (6.26) and `player.extend_options`.
- `crates/arena-store/migrations/0065_game_plays.sql`, `src/{migrations,branch,lib}.rs`, new `src/game_plays.rs` -- the table (like `device_events`), `Table::GamePlays`/`RowKey::GamePlays`, insert, `top_played`, `last_played`.
- `crates/arena-core/src/apply/{kiosk_log,mod,mutate,validate}.rs`, `tests/proptest_apply.rs` -- apply the intent next to `launcher.opened` (`live_session_on_device`), proptest op.
- `crates/arena-core/src/{devices,game_plays}.rs` -- `DeviceDirectory::most_played` with the 15-minute cache.
- `crates/arena-api/src/lan/{session,mod}.rs`, `src/ui_events.rs` -- frame handler; device event rate limit covers the new intent.
- `crates/arena-kiosk-svc/src/{games,intents,app,player_session or games}.rs`, `cleanup.rs` -- play tracking in `GameState`, record on exit (run loop sweep) and at session end, `most_played` request.
- `apps/kiosk/src-tauri/{ipc-names.json,src/main.rs,src/bridge.rs}`, `apps/kiosk/src/lib/{bridge,games,Games,Shell}.svelte|ts`, new `MostPlayed.svelte`, `routes/+page.svelte`, `packages/ui/src/messages/en.json`, `docs/kiosk-accessibility.md`, `apps/kiosk/tests/most-played.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: intent, payload + validation, frame and result types, generated TS
- [x] Store: migration, table registration, helpers
- [x] Core: apply `game.play_recorded`; `most_played` read with 15-min cache; proptest op
- [x] API: `games.most_played` frame; rate limit
- [x] Kiosk service: track plays, record on exit and session end, `most_played` request
- [x] Tauri shell + app: command, carousel component, Games tab wiring, strings, a11y doc
- [x] Tests: Rust (proto, store, core, api lan, kiosk-svc) and Playwright (`most-played.spec.ts`); run the I/O matrix cases

**Acceptance Criteria:**
- Given a game launched in a session, when its process exits or the session ends, then one `game_plays` row (player or guest, device, library entry, started, ended, minutes) exists via `game.play_recorded`.
- Given plays in the last 7×24 h, when a kiosk asks for most played, then it gets the branch's top 10 by minutes and the live player's last-played times, recomputed at most every 15 minutes.
- Given the Games tab with plays, when it renders, then "Most played here" with "Top 10 by play hours · last 7 days · <branch>" sits above the grid, tiles ranked "#1"…, activating one launches the game, arrow keys scroll.
- Given no plays in the window, then the carousel is hidden; given a top game not installed here, then it is skipped and the next fills the place.

## Spec Change Log

- The Hub names a play's session as the one the device was running when the play started (it may have ended since), falling back to the live session: a play recorded at session end reaches the Hub after the session has ended, and the live session alone would have left exactly those plays without a player.
- A play is recorded for every game that stops, including one under a minute (`minutes: 0`); only minutes above zero count in the top list. `last_played` includes such a play.
- `game_plays.game_id` has no foreign key: a play of a library entry deleted since is still a play.

## Review Triage Log

### 2026-10-29 — Review pass
- verdicts: 4 patched, 4 deferred, remainder rejected (blind hunter 15 findings, edge-case hunter 10, verification gap 3 + 3 other; intent audit was descriptive)
- findings:
  - `[medium]` `[patch]` record_running stamps already-exited games with the cleanup time — `reap` runs first; test rewritten.
  - `[low]` `[patch]` a stale `loadMostPlayed` answer can repopulate the carousel after the session ends — per-run `live` flag in +page.svelte.
  - `[medium]` `[patch]` the `games::run` loop was untested — added `the_run_loop_records_an_exited_game_by_itself_within_one_poll`.
  - `[low]` `[patch]` the live-session fallback in record_play was untested — added `a_play_that_starts_before_every_session_falls_back_to_the_live_one`.
  - `[maybe-false]` `[defer]` clock skew in session attribution — needs the real kiosk/Hub skew to settle.
  - `[medium]` `[defer]` plays lost on service restart or power-off; launcher-link games timed by the handler process — in `deferred`.
  - `[low]` `[defer]` `ended_at` unbounded against the Hub clock — in `deferred`.
  - `[low]` `[defer]` `most_played` service forwarding untested — follows the existing `player_extend_options` pattern.
  - `[low]` `[reject]` owner-facing hours per game, last_played unused in the UI, fixed "Top 10" caption, aria wording of the rank, follower test, frontend test gaps, retention/index, rate-limit bucket sharing, queue-push failure drops a play, deleted-library games in the top list, spec bookkeeping — owner reporting is later work (story: "for later owner reporting"), last_played is consumed by Story 6.29, the caption is the story's exact text; the rest are cosmetic or follow existing patterns and need more than a direct fix.
  - `[low]` `[reject]` "next one fills the place" only drops uninstalled games from the Hub's top 10 — the story has the Hub return the top 10; the reading is in Design Notes.

## Design Notes

"The next one fills the place": the Hub returns the top 10 as specified; the app drops games this PC does not have and renumbers, so later tiles move up (it never fetches an eleventh). A game started from a launcher link is tracked by the launcher's handler process, so its recorded minutes may be short (same limit as launch time in 6.25).

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-kiosk-svc --lib` plus `cargo test -p arena-core --test proptest_apply` -- expected: pass (known Windows-privilege kiosk-svc failures unchanged)
- `cargo fmt --check && cargo clippy -D warnings` on touched crates -- expected: clean
- `pnpm --filter @arena/kiosk check` and the Playwright kiosk suite; `pnpm check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` -- expected: pass

## Auto Run Result

Status: review

- **Summary:** the kiosk service times each game and queues `game.play_recorded` when the process exits or at session end; the Hub writes `game_plays` (migration 0065) naming the player and session; `games.most_played` returns the top 10 by minutes over 7 days (15-minute cache) plus the live player's last-played times; the Games tab shows the "Most played here" carousel.
- **Review:** 4 patches applied, 4 deferred, the rest rejected with reasons in the triage log. Follow-up review recommended: false.
- **Verification:** `cargo fmt --check` clean; clippy `-D warnings` clean on proto, store, core, api and kiosk-svc libs; proto 242, store 90, core `game_play` 8, kiosk-svc `games` 27 and `cleanup` 46 pass, api `lan` games test passes; `pnpm --filter @arena/kiosk check` 0 errors; `most-played.spec.ts` 9 pass (full kiosk suite 271 pass before the patches; 4 `theme.spec.ts` tests fail as before). `arena-replay` `replaying_the_seeded_log…` fails at `library_entries has no row` (a table from 6.3, not touched here).
- **Residual risks:** the real Tauri shell, a Hub-to-kiosk round trip and real Windows process exit were not exercised; see `deferred`.
