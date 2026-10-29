---
title: 'Story 7.9: New tickets chime and hold a NEW badge, with a mute toggle'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '51c30d5'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-8-one-tap-advances-a-ticket-with-undo.md'
---

## Intent

**Problem:** A new order reaches the kitchen board silently (only a live-region announcement), so staff can miss it in a rush.

**Approach:** The display half only; the Hub already pushes the whole board after every ticket change.
- A ticket that arrives in New takes a "NEW" `alert-badge` and a danger left edge. It keeps them until a board (or a successful local advance) shows it past New; Undo back to New never restores them. Tickets already in New when the board first loads are badged but do not chime.
- The bundled chime (`apps/kitchen/static/sounds/new-ticket.wav`, served by the Hub at `/kitchen/sounds/new-ticket.wav`) plays once per burst: arrivals within 1 s of the last chime share it.
- A 44px toggle in the top bar mutes the chime (struck-through bell, "Chime off"); badges stay. The choice is kept in `localStorage` (`arena.kitchen.chime-muted`); unavailable storage means unmuted.
- If the browser's autoplay policy is not "allowed" (or a play is refused with `NotAllowedError`), a "Tap to turn on the chime" bar shows under the top bar until the first tap anywhere, which plays the chime silently to unlock audio.

## Code Map

- `apps/kitchen/src/lib/board.ts` -- `freshTickets` (badge set and arrivals).
- `apps/kitchen/src/lib/chime.ts` (new) -- sound URL, mute storage, `createChime` (play, needsTap, unlock).
- `apps/kitchen/src/lib/KitchenTicket.svelte`, `BoardScreen.svelte`, `routes/+page.svelte` -- badge and edge, toggle and bar, state and wiring.
- `apps/kitchen/static/sounds/new-ticket.wav` (new), `crates/arena-api/src/lan/kds_pair.rs` -- `.wav` is served as `audio/wav`.
- `packages/ui/src/messages/en.json` -- `kitchen.chime.*`.
- `apps/hub/tests/kitchen-board/` -- unit specs for `freshTickets`; browser specs with a stubbed audio element.

## Acceptance Criteria

- [x] A new ticket shows at the top of New with the NEW badge and danger edge, the chime plays once and the arrival is announced.
- [x] Advancing to Preparing clears the badge and edge; Undo back to New does not restore the badge or chime.
- [x] The mute toggle stops the chime, shows "Chime off" with a struck-through bell, keeps badges; unmuting restores the chime for later tickets.
- [x] The mute choice survives a reload; with storage unavailable the board starts unmuted.
- [x] A browser that blocks audio shows the one-time tap bar until the first tap; badges work either way.
- [x] Several tickets arriving within one second chime once, each with its own badge.

## Verification

- `pnpm --filter @arena/kitchen check` and `build`; `pnpm --filter @arena/hub test:kitchen` (53 passed, including axe runs with the badge, toggle and bar).
- `cargo fmt --all --check`, `cargo clippy --locked -p arena-api --all-targets -- -D warnings`, `cargo test -p arena-api --lib kds_pair`.
- `pnpm check:strings`, `check:layers`, `check:protocol`, `check:hex`, `check:generated`, `format:check`.

## Notes

- The chime is a short generated two-note sound (no third-party asset).
- A refused advance keeps the badge (the badge is cleared only when the Hub accepts the advance or a board shows the ticket past New).
- The top bar's toggle is now the first Tab stop; existing keyboard specs focus it first.
- Residual risk: the audio unlock is exercised only against a stubbed audio element; the real autoplay policy of the kitchen tablet's browser (e.g. Safari) is covered by Story 7.11's hardware run.

### Review Findings

Code review (2026-10-29): no blocking findings. 0 decision-needed, 0 patch, 0 defer.

Rejected:
- Concurrent `unlock()` calls (window pointerdown plus the button click): the second call's play is aborted by the first's pause, reports `AbortError`, which is treated as allowed, so the outcome is the same. Not a defect.
- Muting while the tap bar shows leaves the bar up: harmless, and unmuting runs `unlock()` from a tap. Low and not worth a new branch.
