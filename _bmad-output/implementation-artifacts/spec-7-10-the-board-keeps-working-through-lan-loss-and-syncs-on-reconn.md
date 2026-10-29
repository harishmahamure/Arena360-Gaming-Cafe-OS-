---
title: 'Story 7.10: The board keeps working through LAN loss and syncs on reconnect'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: 'bc7b674'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-9-new-tickets-chime-and-hold-a-new-badge-with-a-mute-toggle.md'
---

## Intent

**Problem:** When the kitchen tablet drops off the Wi-Fi, a tap is refused with "Offline" and the status badge says Offline, so a flaky connection stops the kitchen.

**Approach:** The display half only; the Hub already applies an intent once per `idempotency_key`, checks `base_rev` and expires an `issued_at` older than its TTL.
- The LAN badge reads "RECONNECTING" in the warning state while the link is gone, and until the first board after the queue has replayed. Tickets stay, elapsed times keep counting on the Hub's clock run on the tablet's monotonic timer, and no ticket gets a marker.
- A request with no reply in 5 s closes the socket (the link reconnects by itself). Reconnect pauses are jittered, 2 to 30 s.
- A tap while the Hub is out of reach (or while the display is catching up) moves the ticket at once and queues the advance in IndexedDB (`arena.kitchen`/`queue`): its ULID key minted at the tap, the predicted `base_rev` (the board's rev plus the moves already queued for that ticket), the tap time as `issued_at` and a 24-hour expiry. A tap whose send is lost to the link is queued the same way. Undo of a move that was not sent removes it from the queue.
- On reconnect the queue replays in creation order, each move once with its own key, before the board counts as caught up. A move the Hub refuses for any reason except a lost link is dropped without a word; the board is then redrawn from the Hub. A lost link stops the replay and the moves stay queued.
- A reload keeps the queue (moves older than 24 hours are dropped on load). With the Hub still unreachable the board does not load. When it loads again, the stored moves replay first and new taps queue behind them.

## Code Map

- `apps/kitchen/src/lib/queue.ts` (new) -- `Queued`, `QUEUE_TTL_MS`, `liveEntries`, `predictedRev`, `without`, `memoryStore`, `openQueueStore` (IndexedDB, falling back to memory).
- `apps/kitchen/src/lib/display.ts` -- 5 s reply timeout, jittered reconnect pause, `advance(..., issuedAt)`.
- `apps/kitchen/src/lib/board.ts` -- `unpend` (take a chained pending move back).
- `apps/kitchen/src/routes/+page.svelte` -- queueing, `replay`, `reconciling`, Undo of a queued move, reload restore.
- `apps/kitchen/src/lib/BoardScreen.svelte`, `packages/ui/src/messages/en.json` -- `kitchen.board.reconnecting` replaces `kitchen.board.offline`; `kitchen.toast.offline` removed.
- `apps/hub/tests/kitchen-board/` -- stub gains an unreachable Hub, a silent Hub, per-ticket refusals and key counts; new browser specs and unit specs.

## Acceptance Criteria

- [x] With the Hub lost (socket closed or no reply in 5 s) the badge shows "RECONNECTING" in warning, every ticket stays, elapsed times keep counting, and no ticket carries a marker.
- [x] Taps and Undo with the Hub unreachable: the ticket moves at once, the advance is queued in IndexedDB with its ULID key, predicted `base_rev` and a 24-hour expiry, and an Undo of a not-yet-sent advance removes it.
- [x] A reload during LAN loss cannot load the board and keeps the queue; a reload with a non-empty queue replays it before new taps are sent.
- [x] When the Hub returns, the display reconnects with a jittered 2 to 30 s pause, queued advances replay in creation order and apply once per key, the board is redrawn from the Hub (new tickets arrive with NEW badges) and the badge returns to "CONNECTED" only after that.
- [x] A queued advance the Hub refuses (`STALE`, `BAD_STAGE`, `EXPIRED`, and any other refusal) is dropped silently and the ticket shows the Hub's state.
- [x] Orders placed at the counter while the display is away do not appear until it reconnects (the Hub pushes them with the reconnect board).
- [x] A browser spec cuts the display for 10 minutes while it advances 20 tickets and the counter cancels one: every valid advance is received once per key, the cancelled ticket shows Cancelled and the board matches the Hub.

## Verification

- `pnpm --filter @arena/kitchen check`; `pnpm --filter @arena/hub test:kitchen` (62 passed, including an axe run on the reconnecting board with queued moves).
- `pnpm check:generated`, `format:check`, `check:layers`, `check:protocol`, `check:strings`, `check:hex`, `check:no-blur`, `check:ads`.
- `check:tokens-used` fails on `--ad-reserved` in `apps/hub/src/routes/floor/+page.svelte`; that file is not part of this story.
- No Rust changes.

## Notes

- Dropping every refusal except a lost link is deliberate: the queue never blocks on a move the Hub will not take. A refusal of a different kind (for example `NOT_PRIMARY` after a failover) also drops the move; Epic 9's re-pairing story owns that case.
- The Hub's own intent TTL is 7 days; the display's 24-hour expiry is the stricter bound.
- The "5 s with no reply" rule is applied to the display's own requests (an advance or an Undo). Between taps, the Hub's 15 s keep-alive ping and the socket's close event reveal a lost link.
- An Undo of a move the replay is sending at that moment goes through the normal revert path; a move sent while online and acknowledged is not queued.
- Residual risk: the stub Hub stands in for the Hub's real dedupe-by-key (proven in `arena-core` by Story 1.8); a run against the real Hub on the kitchen tablet is Story 7.11's hardware suite.
