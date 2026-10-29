---
title: 'Story 7.3: Every order with kitchen items opens a kitchen ticket that the Hub can move'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '5ddd8f4'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-2-kitchen-notes-on-order-lines-from-the-pos-and-the-kiosk-cart.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A kitchen order is only lines on a bill. The counter and the kitchen have no shared state for "being made", "ready" and "served".

**Approach:**
- A placement (`order.place`, `pos.sale`, the kiosk orders that use them) with kitchen-flagged lines on a branch with the `kitchen` flag opens one ticket in the same transaction. It holds only the kitchen lines and their notes, the seat label, and a branch counter number `K-<n>`.
- The ticket stage moves New, Preparing, Ready, Served one step at a time by `kitchen.advance`; `kitchen.advance.reverted` takes the latest move back within the Undo window.
- Ready runs the `order.ready` effect for the ticket's order; Served serves each unserved line (taking held stock). A walk-up ticket only changes stage.
- Cancelling the last open line of a ticket cancels it; voiding the order cancels its tickets; undoing the placement removes its ticket.
- The Hub tile sheet lists the open tickets with Mark ready and Mark served acting on the ticket.

## Boundaries & Constraints

**Always:**
- **Tables.** Migration `0069_kitchen_tickets.sql`: `kitchen_tickets` (id, order_id, device_id, seat, number, stage, placement_id, placed_at, advanced_at, advanced_by, cancel_reason, rev) and `kitchen_ticket_lines` (id, ticket_id, order_line_id, label, qty, notes). Both are change-log tables. Neither `order_id` nor `order_line_id` has a foreign key.
- **Number.** `K-<n>` is one past the highest ticket number of the branch, taken in `apply()`.
- **Seat.** The device's `TV <n>` or floor label for a session, counter or kiosk order; `Counter` for a walk-up sale. Never a player name.
- **Flag.** Without the `kitchen` flag nothing is created and Epics 5 and 6 behave as before.
- **Advance.** `kitchen.advance{ticket_id, to}` with `base_rev` is `STALE` on a different `rev`, `BAD_STAGE` (new code, HTTP 422, deterministic) unless `to` is exactly the next stage. Permission `kitchen.use` (Kitchen, Counter, Manager, Owner; a device actor for the `kds` class). Registered with the Story 7.1 gate as `ExistingWork`: a ticket that does not exist on a branch without the flag is `TIER_LOCKED`, one created before a lapse still moves.
- **Undo.** `kitchen.advance.reverted` with the `rev` the advance left: same actor, 8 s, a stage to go back to (`BAD_STAGE` on New/Cancelled; `UNDO_NOT_ALLOWED` otherwise). Ready clears the order's ready mark when this advance set it. Served reopens the holds, gives the stock back (`sale_reverted` moves) and unserves the lines. The undo leaves no advance to take back.
- **Order intents.** While a ticket of the order is New or Preparing, `order.ready` is `BAD_STAGE`. `order.line_served` on a ticket line needs the ticket Ready (`BAD_STAGE`), and serving the last unserved line serves the ticket.
- **Cancel / void / undo.** `order.line_cancel` removes the line from its ticket; the last one cancels the ticket (reason kept, no lines); a Ready ticket whose remaining lines are all served becomes Served. `order.void` cancels the order's live tickets. `order.place.reverted` and `pos.sale.reverted` delete the ticket they opened and are `UNDO_NOT_ALLOWED` once it is past New.
- **kds scope.** `KitchenAdvance` and `KitchenAdvanceReverted` only. No cancel.
- Strings in `en.json`; every new intent and its twin in the Story 1.8 `proptest` generator.

**Never:**
- A change to price, tax, stock or the amount because of a ticket (the stage moves serve and give back stock only as `order.line_served` does).
- A player name on a ticket.
- A KOT print (Story 7.5), the board (7.7) or the chime (7.4).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Placement | Basic, burger + cola | one ticket K-1 with the burger line | N/A |
| Plain placement | cola only | no ticket | N/A |
| Walk-up | `pos.sale` with a burger | ticket, seat `Counter`; advances change only the ticket | N/A |
| Skip | New to `ready` | rejected | `BAD_STAGE` |
| Stale | wrong `base_rev` | rejected | `STALE` |
| Ready | Preparing to `ready` | stage, order stamped ready | N/A |
| Served | Ready to `served` | lines served, stock taken | `OUT_OF_STOCK` when short |
| Undo Served | within 8 s, same actor | stock and holds back, line unserved | `UNDO_NOT_ALLOWED` after |
| Order ready early | ticket New | rejected | `BAD_STAGE` |
| Last line cancelled | cancel | ticket Cancelled with the reason | N/A |
| Free | ticket id that does not exist | rejected | `TIER_LOCKED` |
| Lapse | ticket made on Basic, flag removed | still moves | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/{kitchen.rs,intent.rs,scope.rs,error.rs,session.rs}` -- stages, payloads, views, gate arm, intent kinds, `BAD_STAGE`, `SnapshotBill.tickets`.
- `crates/arena-store/{migrations/0069_kitchen_tickets.sql,src/kitchen_tickets.rs,src/branch.rs}` -- tables, rows, queries.
- `crates/arena-core/src/apply/{kitchen_ticket.rs,order_line.rs,order_place.rs,pos_sale.rs,order_void.rs,order_ready.rs}`, `rbac.rs`, `snapshot.rs`.
- `crates/arena-api/src/ui_auth.rs` -- the Ready push to the PC, `BAD_STAGE` status.
- `apps/hub/src/lib/{OrderSection.svelte,TileSheet.svelte,tile-actions.svelte.ts}`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-core/src/apply/tests/kitchen_ticket.rs`, `crates/arena-core/tests/proptest_apply.rs`, `apps/hub/tests/tile-sheet.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: stages, `KitchenAdvance`/`KitchenAdvanceReverted`, ticket views, intent kinds, gate, kds scope, `BAD_STAGE`, generated types.
- [x] Store: migration 0069, ticket and line tables registered in the change log, queries.
- [x] Core: ticket creation in `order.place` and `pos.sale`, advance and undo, line serve/cancel, void, placement undo, `order.ready` guard, permissions.
- [x] Snapshot and Hub: tickets on the bill, ticket rows with Mark ready and Mark served, audit labels.
- [x] Tests: Rust unit tests, proptest ops and invariants, Hub Playwright.

**Acceptance Criteria:**
- Given a Basic branch, when an order with kitchen lines commits, then one ticket `K-<n>` with only those lines, their notes and the seat label exists; a Free branch creates none.
- Given a ticket in New or Preparing, when staff press Mark ready (two steps from New), then it is Ready and the order is stamped ready; Mark served serves its lines in the same transaction (a walk-up ticket changes stage only).
- Given a skipped stage or a stale revision, then `BAD_STAGE` or `STALE` and nothing changes; the Undo within its window reverses the stage's effects.
- Given the last open line of a ticket is cancelled, then the ticket is Cancelled with the reason and no lines.
- Given a Free branch, then `kitchen.advance` on a missing ticket is `TIER_LOCKED`, and a ticket made before a lapse still finishes.
- Given the proptest generator, then a ticket moves one step at a time, Served implies every live line served, and a cancelled ticket has no lines.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-replay -p arena-cloud -p arena-kiosk-core --locked`; clippy with `-D warnings` on the same crates; `cargo fmt --check`.
- `pnpm gen:types`; `pnpm --filter @arena/hub check`; Hub `tile-sheet`, `audit`, `pos`, `kitchen-notes` specs with a unique `ARENA_PW_PORT`; `check:strings`, `check:layers`, `check:protocol`, `format:check`.

## Spec Change Log

## Review Triage Log

Self-review while building (no separate reviewer run in the unattended loop):

- The AC says Mark ready moves a New ticket to Ready while the property says one step at a time. Resolved: the server is strict; the Hub's Mark ready on a New ticket sends `preparing` then `ready`.
- `order_line::validate_served` rejected any `base_rev`, which the ticket path needs; the base_rev check stays on the line intent and the stock checks moved to `check_servable`.
- A reverted placement or sale would leave a ticket behind; both now delete it and refuse once it is past New.

Deferred (deferred-work.md): see the Story 7.3 entry.

## Code Review (unattended)

Reviewed the 7.3 commit (core apply paths, store, scope, API push, Hub). No blocking findings; no code changes.
Checks: `cargo fmt --check`, clippy `-D warnings` on the story crates, Rust unit/integration tests, `pnpm --filter @arena/hub check`, `check:strings`, `check:layers`, `check:protocol`, `format:check` all pass.
Not caused by this story (the `arena-replay` one fails identically on the parent commit): `arena-cloud` `binary` timeout test (known sandbox failure) and `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings`, whose seed has no `library_entries` row since Story 6.3 and also seeds no kitchen ticket rows.
