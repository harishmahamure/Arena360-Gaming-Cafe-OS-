---
title: 'Story 7.7: The kitchen board shows open tickets in four columns'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '966d5be19b030aa475809357c74f33d474845c77'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-6-pair-a-kitchen-tablet-by-code-as-a-scoped-display.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-3-every-order-with-kitchen-items-opens-a-kitchen-ticket-that-t.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A paired kitchen display connects to the Hub but only says "Connected to the Hub." It has no feed of tickets and no board, so the kitchen still cannot see what to cook next.

**Approach:**
- A `kds` session reads the board with a new `kitchen.board` request (reply `kitchen.board_result`) and is pushed the whole board as `event{kind:"kitchen.board"}` after any change to `kitchen_tickets` or `kitchen_ticket_lines`. The board is `{branch_name, now_ms, tickets[]}`; each ticket carries the seat, number, stage, `placed_at`, `stage_at` (when it reached its stage), `rev` and its lines with notes. The feed holds every open ticket, tickets Served in the last 30 minutes and tickets Cancelled in the last 60 seconds. It never names a player and it is not gated by the licence flag (a lapse keeps showing the tickets that exist).
- The Kitchen app renders it: `kitchen-top-bar` (64 px: branch name, Hub-time clock, "<n> open · oldest <m> min", CONNECTED / OFFLINE badge) over four columns New, Preparing, Ready, Served with counts, tickets newest first, each column scrolling on its own. A `kitchen-ticket` shows the seat in `hub-numeric`, `K-<n>`, the tabular elapsed time since placement, "qty× name" lines with notes indented in muted ink. Ready gets a green edge and READY tag; Served is muted; Cancelled is muted with struck-through lines and a CANCELLED badge, stays in the column it was last seen in, has no control, and leaves after 60 s. Served leaves 30 minutes after it was served.
- All times are the Hub's: the page keeps `board.now_ms` plus the time since it arrived, never the tablet's date. High contrast (the existing per-display setting) gives 2 px outlines and the yellow focus ring. One `h1`, each column a labelled region, tickets focusable (44 px), new tickets and stage changes announced in a polite live region.

## Boundaries & Constraints

**Always:**
- Only a `kds` session may send `kitchen.board`; any other class gets `FORBIDDEN`. The `kds` intent scope is unchanged.
- The feed is read on a blocking task from the Hub's read-only connection and answers whatever tier the branch holds now.
- All user-visible strings are in `packages/ui/src/messages/en.json`; the board uses kitchen tokens (gutter 24, column gap 24, ticket padding 16, ticket gap 16).

**Never:**
- No tap, advance, Undo, chime or mute control on the board (Stories 7.8 and 7.9), no cancel control at any time, no cancel slip.
- No player name on a ticket. No use of the tablet's clock for an elapsed time.
- No LAN-loss queueing or replay on the display (Story 7.10).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Read the board | `kds` sends `kitchen.board` | `kitchen.board_result` with open, recent Served and recent Cancelled tickets, newest first | Non-`kds` class: `FORBIDDEN` |
| Ticket changes | any `apply()` touching ticket rows | every connected display gets the whole board as an event | read failure drops the displays (they reconnect and re-read) |
| Served ages out | Served ticket older than 30 min | not in the feed; the page drops it on its next second | n/a |
| Cancelled | ticket cancelled | greyed, CANCELLED badge, gone after 60 s | n/a |
| Lapse to Free | branch loses the `kitchen` flag with open tickets | feed still lists them | n/a |
| Hub lost | socket closes | board stays, badge says OFFLINE, page reconnects and re-reads | n/a |

</intent-contract>

## Code Map

- `crates/arena-proto/src/kitchen.rs`, `frame.rs`, `lib.rs` -- `KitchenBoard`, `KitchenBoardTicket`, keep constants, `kitchen.board` / `kitchen.board_result` frames, `HubEvent::KitchenBoard`.
- `crates/arena-store/src/kitchen_tickets.rs`, `lib.rs` -- `list_board_tickets` and `Store::board_tickets`.
- `crates/arena-core/src/devices.rs`, `snapshot.rs` -- `DeviceDirectory::kitchen_board`, `board_ticket`.
- `crates/arena-api/src/lan/kitchen_board.rs` (new), `session.rs`, `registry.rs`, `mod.rs` -- request handler (`kds` only), board push after ticket changes.
- `apps/kitchen/src/lib/board.ts` (rules), `BoardScreen.svelte` (top bar and columns), `KitchenTicket.svelte`, `display.ts` (`onBoard`), `routes/+page.svelte`, `routes/+layout.svelte`; `packages/ui/src/messages/en.json`; generated types.
- Tests: `arena-core` apply tests (board ageing, lapse), `arena-api/tests/lan.rs` (read, push, scope, lapse), `apps/hub/tests/kitchen-board/` (unit and browser specs) with `apps/hub/playwright.kitchen.config.ts` (`pnpm --filter @arena/hub test:kitchen`).

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/**` -- board types, frames, event, constants -- contract
- [x] `crates/arena-store/**`, `crates/arena-core/**` -- board query and `kitchen_board` -- read model
- [x] `crates/arena-api/**` -- `kitchen.board` for `kds`, push after ticket changes -- transport
- [x] `apps/kitchen/**`, `packages/ui/src/messages/en.json` -- board, ticket, top bar, live region, HC -- display
- [x] Rust and browser tests for every matrix row

**Acceptance Criteria:**
- [x] Given a paired display connected to the Hub, when the board renders in tablet landscape, then it shows the 64 px top bar and four columns with counts, using the kitchen spacing tokens.
- [x] Given a ticket, when it renders, then it shows the seat, `K-<n>`, the Hub-time elapsed, "qty× name" lines with indented muted notes and no player; Ready has the READY tag and green edge; Served is muted.
- [x] Given more tickets than fit, then newest first and each column scrolls alone; an empty column shows its header only.
- [x] Given the open count, then it is New, Preparing and Ready only and "oldest" is the oldest of those, on the Hub's clock.
- [x] Given Served tickets, then they leave 30 minutes after they were served.
- [x] Given a cancelled ticket, then it is muted with a CANCELLED badge, struck lines when it has any, has no control and leaves after 60 s.
- [x] Given high contrast for the display, then the board uses 2 px outlines and the yellow focus ring.
- [x] Given a lapse to Free, then the feed still serves the tickets that exist.
- [x] Given a screen reader or keyboard, then there is one `h1`, labelled column regions, focusable 44 px tickets and a polite live region for new tickets and stage changes.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Cancelled tickets keep no lines.** Story 7.3 deletes a cancelled ticket's lines (the ticket keeps only its reason), so a real cancelled ticket shows its seat, number and the CANCELLED badge with no lines to strike through. The display strikes lines when the feed carries any; keeping the lines on cancel would change the apply rules and is left to a later decision.
- **Column of a cancelled ticket.** The feed has no previous stage, so the page remembers the column of each ticket it has seen and keeps a cancelled one there (New when it never saw it).
- **Clock.** The top-bar clock shows the Hub's time in the tablet's time zone (a `kds` session cannot read `branch.tz`).
- **Whole board pushes.** The board is small (open tickets plus the last half hour), so a push carries all of it and the page never merges.
- **Tests live in the Hub package.** The Kitchen app has no test runner of its own and adding one changes the lockfile; the Hub already carries Playwright, so `playwright.kitchen.config.ts` builds and previews the Kitchen app from there.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --locked`
- `cargo clippy --locked -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --all-targets -- -D warnings`; `cargo fmt --all --check`
- `pnpm gen:types`; `pnpm --filter @arena/kitchen check`; `pnpm --filter @arena/kitchen build`; `pnpm --filter @arena/hub test:kitchen`
- `pnpm check:strings`, `check:layers`, `check:protocol`, `check:hex`, `format:check`

## Auto Run Result

Status: in-review (implementation and verification complete; the multi-reviewer pass is left to the next loop iteration).

- **Change.** A `kds` session reads the kitchen board with `kitchen.board` and is pushed the whole board after any ticket change. The Kitchen app shows the top bar and four columns (New, Preparing, Ready, Served) with Hub-time elapsed, Ready and Cancelled cues, Served and Cancelled ageing, high contrast, keyboard focus and a polite live region.
- **Verification.** `cargo fmt --check`, `cargo clippy -D warnings` (proto, store, core, api, hub) clean; `cargo test` on those crates: every test binary passed, 0 failed (arena-core 1250 lib tests including 2 new board tests; arena-api lan 99 including 3 new). Kitchen svelte-check 0 errors and 0 warnings; Kitchen build ok; `pnpm --filter @arena/hub test:kitchen` 21 passed (8 unit, 13 browser including axe); `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `format:check` ok. `check:tokens-used` reports the pre-existing `--ad-reserved`; `check:generated` compares against git, so it passes once the new generated files are committed.
- **Residual risks.** The built-in `/kitchen` fallback page shows no board (the build must be served from `ARENA_KITCHEN_DIR`); a real cancelled ticket has no lines to strike through; the browser spec stubs the Hub's socket, so no run drove a real Hub end to end.

## Review Result

Code review (blind, edge-case, verification-gap, acceptance layers) found no blocking defects. Patched: the page and Hub now agree on the Served/Cancelled ageing edge (`<=`), the display forgets remembered columns when its token is revoked, the Kitchen and `test:kitchen` checks run in the CI `a11y` job, and a test shows a phone is never pushed the board. Left as designed or deferred: cancelled tickets carry no lines and fall back to New when unseen (spec Design Notes), elapsed time keeps counting on Served and Cancelled cards, per-ticket line queries, unused `kitchen.board.columns` and `kitchen.ticket.number` strings, the push's `epoch_ms()` matches the other registry pushes.
