---
title: 'Story 7.8: One tap advances a ticket, with Undo'
type: 'feature'
created: '2026-10-29'
status: 'in-review'
baseline_commit: '1ecb653'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-7-the-kitchen-board-shows-open-tickets-in-four-columns.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-3-every-order-with-kitchen-items-opens-a-kitchen-ticket-that-t.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The kitchen board (7.7) is read-only. Staff cannot move a ticket from the tablet, so the Hub counter still has to advance every ticket.

**Approach:**
- The Hub side already exists (Story 7.3): a `kds` session may submit `kitchen.advance` and `kitchen.advance.reverted`, and the board is pushed after each change. This story adds the display half and closes one Hub gap: an Undo after its 8 s window is rejected `EXPIRED` (it was `UNDO_NOT_ALLOWED`); a later advance of the same ticket stays `STALE`.
- Tapping (or pressing Enter/Space on) a New, Preparing or Ready ticket sends `intent.submit{kitchen.advance{ticket_id, to: next}}` with `base_rev` = the ticket's `rev` and fresh ULIDs for the intent and idempotency key. The ticket moves at once to the top of the next column, and an `undo-toast` shows at the bottom ("PC 12 · #K-2041 moved to Preparing", seconds left, Undo).
- Undo inside 8 s sends `kitchen.advance.reverted` with `base_rev` = the rev the advance left (taken from the board push that shows the new stage, else the tapped rev + 1). Advancing another ticket replaces the toast, so the earlier advance is final. If the Hub refuses the advance (`STALE`, `BAD_STAGE`, `NOT_FOUND`, `TIER_LOCKED`) the ticket snaps to the Hub's board and the toast reads "Already moved at the counter" with no Undo.

## Boundaries & Constraints

**Always:**
- Served and Cancelled tickets are not actionable. A lapsed branch keeps working for tickets created before the lapse; the Hub decides (`TIER_LOCKED` otherwise) and the display snaps to the Hub's board.
- Strings live in `packages/ui/src/messages/en.json`; the toast is the shared `UndoToast`.
- Only one toast at a time; Undo is single-flight.

**Never:**
- No chime, NEW badge or mute (7.9), no LAN-loss queueing or replay (7.10): while offline a tap is refused with a note and the ticket stays put.
- No cancel control. No change to the intent scope of `kds`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Advance | tap New/Preparing/Ready | optimistic move to top of next column, toast with Undo | n/a |
| Undo in window | Undo within 8 s | ticket returns to its previous column, toast closes | n/a |
| Replace | advance a second ticket | new toast replaces the old; first advance final | n/a |
| Served tap | tap a Served or Cancelled ticket | nothing | n/a |
| Lost race | Hub or another display moved it | snaps to the Hub's state, "Already moved at the counter", no Undo | `STALE`/`BAD_STAGE`/`NOT_FOUND`/`TIER_LOCKED` |
| Late undo | Undo after window or after a later advance | board keeps the Hub's state, "Too late to undo" | `EXPIRED`/`STALE` |
| Offline | tap with no socket | ticket stays, "Offline — not moved" | n/a |

</intent-contract>

## Code Map

- `crates/arena-core/src/apply/kitchen_ticket.rs` -- `validate_advance_reverted`: window closed now `ErrorCode::Expired`; advance/undo already exist (7.3).
- `crates/arena-core/src/apply/tests/kitchen_ticket.rs` -- window test expects `Expired`.
- `crates/arena-proto/src/scope.rs` -- `KDS_SCOPE` already holds both intents (read-only).
- `crates/arena-api/tests/lan.rs` -- a `kds` session advances, is pushed the board, undoes, is refused `STALE`/`BAD_STAGE`/`EXPIRED`.
- `apps/kitchen/src/lib/ulid.ts` (new), `display.ts` (`Connection` with `advance`, `revert`, `refresh`), `board.ts` (`nextStage`, `applyPending`, pinning), `KitchenTicket.svelte` (tap/Enter), `BoardScreen.svelte` (passes the handler), `routes/+page.svelte` (optimistic state, toast, timer, focus).
- `packages/ui/src/components/UndoToast.svelte` -- reused as is.
- `packages/ui/src/messages/en.json` -- toast, hint and notice strings.
- `apps/hub/tests/kitchen-board/` -- unit specs for the new `board.ts` rules and browser specs for tap, Undo, replace, race and offline; the stub answers `intent.submit`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-core/**` -- Undo window closed returns `EXPIRED`; update the test -- AC late undo
- [x] `crates/arena-api/tests/lan.rs` -- kds advance, push, undo and refusals over the socket
- [x] `apps/kitchen/**`, `packages/ui/src/messages/en.json` -- tap, optimistic move, toast, Undo, snap, offline
- [x] `apps/hub/tests/kitchen-board/**` -- unit and browser tests for every matrix row

**Acceptance Criteria:**
- [x] Given a ticket in New, Preparing or Ready, when staff tap it or press Enter, then `kitchen.advance{ticket_id, to: next}` is sent with a fresh ULID idempotency key and `base_rev`, the ticket is at the top of the next column at once and the toast shows.
- [x] Given the toast, when Undo is pressed within 8 s, then `kitchen.advance.reverted` is sent with the advance's resulting rev and the ticket returns to its previous column.
- [x] Given a toast, when another ticket is advanced, then the new toast replaces it.
- [x] Given a Served ticket, when tapped, then nothing happens.
- [x] Given `STALE` or `BAD_STAGE`, then the ticket snaps to the Hub's state and the toast reads "Already moved at the counter" with no Undo.
- [x] Given an advance to Ready, then the HUD ready copy and the tile READY come from the Hub's existing `order.ready` effect (covered by Story 7.3/7.4 tests).
- [x] Given a lapsed branch, then an old ticket advances and any other ticket is `TIER_LOCKED` with the board snapped.
- [x] Given an Undo after the window or after a later advance, then the Hub answers `EXPIRED` or `STALE` and the board keeps the Hub's state.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Resulting rev.** `intent.result` carries no ticket rev, so the display takes the undo's `base_rev` from the next board push (the ticket at its new stage) and falls back to the tapped rev + 1.
- **Pinning.** The board sorts by placement time; the ticket just advanced is pinned first in its column on this display until another advance or a snap.
- **Pending moves.** A move stays shown until the board shows the ticket off its old stage or the Hub refuses it, so a push about another ticket cannot undo it early.

## Verification

**Commands:**
- `cargo test -p arena-core -p arena-api --locked`; `cargo clippy --locked -p arena-core -p arena-api --all-targets -- -D warnings`; `cargo fmt --all --check`
- `pnpm --filter @arena/kitchen check`; `pnpm --filter @arena/kitchen build`; `pnpm --filter @arena/hub test:kitchen`
- `pnpm check:strings`, `check:layers`, `check:protocol`, `check:hex`, `format:check`

## Auto Run Result

Status: in-review (implementation and verification complete; the multi-reviewer pass is left to the next loop iteration).

- **Change.** Tapping, or pressing Enter or Space on, a New, Preparing or Ready ticket sends `kitchen.advance` and moves it to the top of the next column at once, with the shared `undo-toast` for 8 s. Undo sends `kitchen.advance.reverted` based on the rev the board shows at the new stage. A refused move (`STALE`, `BAD_STAGE`, `NOT_FOUND`) snaps the ticket to the Hub board and reads "Already moved at the counter". The Hub now answers an Undo after its window with `EXPIRED` (it was `UNDO_NOT_ALLOWED`).
- **Verification.** `cargo fmt --check` and `cargo clippy -D warnings` (arena-core, arena-api) clean; `cargo test` on proto, store, core (1250) and api (lan 101, 2 new): 0 failed. Kitchen svelte-check 0 errors, 0 warnings; build ok; `pnpm --filter @arena/hub test:kitchen` 37 passed (unit and browser, including two axe runs); `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `check:generated`, `format:check` ok.
- **Residual risks.** The browser specs stub the Hub socket, so no run drove a real Hub end to end from the tablet; a tablet that reloads mid-window loses its Undo; Undo works only for the display that made the advance (the Hub checks the actor).
