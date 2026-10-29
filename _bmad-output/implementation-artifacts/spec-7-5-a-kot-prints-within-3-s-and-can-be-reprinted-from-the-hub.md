---
title: 'Story 7.5: A KOT prints within 3 s and can be reprinted from the Hub'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '0d93db8'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-4-the-floor-tile-shows-ready-and-the-hub-chimes-when-food-is-r.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A kitchen ticket exists only on the Hub screen. A cook who is not looking at it gets no slip, and a slip that failed to print (printer offline, paper out) leaves no way to get it again.

**Approach:**
- A pure `kot` renderer (`printer/receipt/kot.rs`) turns a ticket into ESC/POS bytes in Font A at 80 mm (48 columns) or 58 mm (32): "KOT", the seat in double width and height, `K-<n>`, "Order <last 6 of the order id>", branch-local time, each line as `qty x name` (the `×` is not in PC437) with its notes indented below, a `-` rule; no prices, no player name. A reprint adds "REPRINT <hh:mm>".
- Opening a ticket queues a `kot` row on `side_effect_outbox` (migration 0071) in the same transaction when a KOT printer is set up: KOT mode `receipt` with a receipt printer chosen, or `another` with a KOT printer chosen. Mode `none` (or nothing chosen) queues nothing.
- The print task sends `kot` rows after commit, KOTs ahead of receipts. It marks a row `sending` first, so a restart that finds one half-sent records `kot.print_failed` and never sends it twice. Results are the system intents `kot.printed` and `kot.print_failed`.
- A failure raises the one "Printer offline" alert; the ticket and order are untouched and the ticket row reads "KOT not printed" with Reprint KOT. `kot.reprint` (permission `kitchen.use`, "existing work" tier gate) queues the same slip marked REPRINT and touches the ticket row so the audit names who asked.
- A ticket cancelled, or removed with its undone placement, before its slip printed closes the queued row; no cancel slip ever prints.

## Boundaries & Constraints

**Always:**
- Raising the row, the alert and the result are part of `apply()`'s transaction; nothing in `apply()` touches a printer.
- A first successful print writes no change row, so a ticket's `rev` (and the Undo of a stage change) is untouched.
- Free branches open no tickets, so no `kot` row is ever queued there.

**Never:**
- A cancel slip, a price or a player name on the slip.
- A second send of a `sending` row after a restart.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Ticket on Basic, receipt printer chosen | placement with kitchen lines | one `kot` row queued; slip prints; `kot.printed` |
| KOT printer None, or none chosen | placement | nothing queued; ticket flows to Hub only |
| Printer offline | send fails | `kot.print_failed`, one Printer offline alert, ticket row "KOT not printed" |
| Reprint | ticket, printer set up | new row marked REPRINT; audit names the actor |
| Reprint, cancelled ticket / no printer | | `STALE` / `VALIDATION_FAILED`, nothing queued |
| Free / lapsed, unknown ticket | | `TIER_LOCKED`; an existing ticket still reprints |
| Restart with a `sending` row | | recorded failed ("reprint if it did not come out"), not re-sent |
| Cancel at the counter | last line cancelled | queued slip closed; no cancel slip |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/{kitchen.rs,intent.rs,lib.rs}` -- `KotReprint`, `KotPrinted`, `KotPrintFailed`, `KitchenTicketView.kot`, gate.
- `crates/arena-store/{migrations/0071_kot_outbox.sql,src/outbox.rs,src/kitchen_tickets.rs,src/lib.rs}` -- `kot` kind, `sending` state, dismiss/list/latest-state queries.
- `crates/arena-core/src/printer/receipt/kot.rs`, `printer/job.rs`, `printer/golden/kot_*.escpos.txt` -- renderer and golden files.
- `crates/arena-core/src/apply/{kot.rs,kitchen_ticket.rs,mod.rs,mutate.rs,validate.rs,guards.rs}`, `snapshot.rs` (`kot_job`, ticket `kot` state).
- `crates/arena-tasks/src/receipts.rs`, `crates/arena-core/src/writer.rs` (`mark_outbox_sending`).
- `apps/hub/src/lib/{OrderSection.svelte,CounterTickets.svelte,TileSheet.svelte,tile-actions.svelte.ts,audit/labels.ts}`, `packages/ui/src/messages/en.json`.
- Tests: `apply/tests/kot.rs`, `printer/receipt/kot.rs`, `arena-tasks` receipts tests, proptest `KotReprint` op, Hub `tile-sheet` and `pos` specs.

## Tasks & Acceptance

**Execution:**
- [x] Proto: three intents, ticket `kot` state, tier gate, generated types.
- [x] Store: migration 0071, `sending` state, queries; cancelled tickets close queued rows.
- [x] Renderer with golden files at both widths (one line, many lines, long names, several notes, Counter, TV 3, reprint).
- [x] Core: queue on open, `kot.reprint`, results, snapshot state, proptest op.
- [x] Print task: send KOTs to the printer the KOT mode names at its paper width; restart safety.
- [x] Hub: "KOT not printed" and Reprint KOT in the tile sheet and the POS strip; strings and audit labels.
- [x] Tests: Rust apply, renderer, task; Hub Playwright.

**Acceptance Criteria:**
- Given a ticket created on a branch with a KOT printer, then a `kot` row is queued with the commit and the slip prints and is recorded `kot.printed`.
- Given the KOT printer is None, then nothing is queued.
- Given a print failure, then the Printer offline alert is raised once, the ticket and order are unaffected and the ticket row reads "KOT not printed" with Reprint KOT.
- Given Reprint KOT, then the slip prints marked REPRINT with the reprint time and the audit records who reprinted it.
- Given a ticket cancelled at the counter, then no KOT or cancel slip prints.
- Given a Free or lapsed branch, then `kot.reprint` of an unknown ticket is `TIER_LOCKED` and no `kot` row is ever queued on Free.
- Given a restart with a half-sent KOT, then it is recorded failed for Reprint and never sent twice.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-api --locked`; clippy `-D warnings` on those crates; `cargo fmt --all --check`.
- `pnpm gen:types`; `pnpm --filter @arena/hub check`; Hub `tile-sheet`, `pos`, `alerts` specs; `check:strings`, `check:layers`, `check:protocol`, `format:check`.

## Spec Change Log

## Review Triage Log

Self-review while building (no separate reviewer run in the unattended loop):

- The result of a first print writes nothing, so it cannot stale a kitchen display's `kitchen.advance` or block the Undo of the placement; only a failure or a reprint's success touches the ticket row.
- The `×` in "qty × name" prints as `x`: PC437 has no multiplication sign.
- A reprint touches the ticket, not the order, so it cannot stale `order.place.reverted`.

### Code review (2026-10-29)

Three-layer review (blind, acceptance, build checks). No blocking findings; tests, clippy, fmt, svelte-check, strings and prettier green. Fixed: the reprint toast said "sent to the printer" when the slip is only queued. Remaining non-blocking items recorded in deferred-work.md.
