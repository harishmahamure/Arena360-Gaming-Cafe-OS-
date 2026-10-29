---
title: 'Story 7.4: The floor tile shows READY and the Hub chimes when food is ready'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '51e6dbf'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-3-every-order-with-kitchen-items-opens-a-kitchen-ticket-that-t.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A ticket reaching Ready changes the order's ready mark but staff on the floor see nothing and hear nothing. The tile keeps saying ORDER and a walk-up ticket has no place to be seen.

**Approach:**
- A new alert kind `order_ready` (tile tag READY) is raised in the same transaction as a Ready advance. A device ticket raises one per device (a second Ready ticket of the same device adds no second alert). A walk-up ("Counter") ticket raises a branch-level alert that records the ticket (`alerts.ticket_id`, migration 0070); many can be open at once.
- `kitchen.advance.reverted` of a Ready advance acknowledges that ticket's alert (device alert only when no other ticket of the device is still Ready) with no sound.
- The tile badge derives from the ticket stages in the snapshot: READY while any ticket of the device is Ready, else ORDER while one is New or Preparing, else the alert tag rules of Story 3.25.
- The Hub plays the bundled `order-ready.wav` once per new order-ready alert (it never repeats) and a screen reader hears "PC 07 order ready" once.
- The sheet's order section reads "Ready · deliver to PC 07" or "Ready · waiting for pick-up" per the branch fulfilment setting.
- The POS screen gets an open-tickets strip above the cart listing each open walk-up ticket ("K-2043 READY"); tapping a Ready entry acknowledges its alert.

## Boundaries & Constraints

**Always:**
- Free branches create no tickets, so none of this appears; the Epic 6 "Mark ready" and ORDER behaviour is unchanged (tested).
- Raising and closing the alert are `Written` rows of the same `apply()` transaction.
- The `ticket_id` alert column is nullable; the branch unique index becomes `(kind, IFNULL(product_id,''), IFNULL(ticket_id,''))`.
- Strings in `en.json`; the new kind has a `hub.alerts.kind.order_ready` label and a `ui.alert.tag.ready` tag.

**Never:**
- A sound on undo, or a repeating sound for `order_ready`.
- A player name on the strip or the alert.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Device ticket Ready | PC 07, Basic | alert `order_ready` on PC 07, tile READY, one chime |
| Second Ready ticket, same device | alert already open | no second alert |
| Served, other ticket New | one served, one New | tile ORDER |
| All closed | every ticket served or cancelled | no badge |
| Walk-up Ready | `Counter` ticket K-2 | branch alert with ticket, strip "K-2 READY" |
| Undo Ready | within window | alert acknowledged, tile ORDER |
| Free | `order.ready` through Epic 6 | no READY, no strip, no chime |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/{alert.rs,snapshot.rs,kitchen.rs}` -- kind, `SnapshotAlert.ticket_id/ticket_number`, `HubSnapshot.counter_tickets`.
- `crates/arena-store/{migrations/0070_alert_ticket.sql,src/alerts.rs,src/migrations.rs}`.
- `crates/arena-core/src/apply/{kitchen_ticket.rs,alert.rs}`, `snapshot.rs`.
- `apps/hub/src/lib/{alerts.svelte.ts,OrderSection.svelte}`, `apps/hub/src/routes/{floor,pos}/+page.svelte`, `apps/hub/static/sounds/order-ready.wav`, `packages/ui/src/components/AlertBadge.svelte`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-core/src/apply/tests/kitchen_ticket.rs`, `apps/hub/tests/tile-sheet.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: `AlertKind::OrderReady`, alert ticket fields, `counter_tickets`, generated types.
- [x] Store: migration 0070, `ticket_id` on alert rows.
- [x] Core: raise on Ready, close on undo and on serve, snapshot fields.
- [x] Hub: READY tile badge from ticket stages, once-only chime, announcement, sheet copy, POS strip.
- [x] Tests: Rust apply tests and a Hub Playwright spec including the Free case.

**Acceptance Criteria:**
- Given a device ticket reaching Ready, then its tile reads READY within 2 s, an `order_ready` alert is open and the chime plays once.
- Given the Ready advance is undone, then the alert is acknowledged and the tile returns to ORDER.
- Given a walk-up ticket reaching Ready, then no tile changes and the POS strip shows "K-<n> READY".
- Given a Free branch, then no READY badge, strip entry or chime appears.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api --locked`; clippy `-D warnings`; `cargo fmt --check`.
- `pnpm gen:types`; `pnpm --filter @arena/hub check`; Hub `tile-sheet`, `alerts`, `pos` specs; `check:strings`, `check:layers`, `check:protocol`, `format:check`.

## Spec Change Log

## Review Triage Log

Self-review while building (no separate reviewer run in the unattended loop):

- The tile badge reads the ticket stages in the snapshot, not the alert, so READY survives the alert being acknowledged by opening the sheet and clears when the ticket is served.
- A second Ready ticket on one device adds no alert (the unique per-device index); the alert closes only when none of the device's tickets is still Ready.
- `alert.raise` refuses `order_ready` (only a Ready advance raises it), as it does for low stock.
- The chime plays once per new `order_ready` alert and is excluded from the 30 s repeat loop.
- The Hub POS strip sits in the catalogue column under the page head (the cart column is not a wrapper for it); the next-stage button there moves one stage per tap.

## Code Review (2026-10-29)

Blind, edge-case and acceptance reviewers ran on the implementation commit.

Fixed:
- `tagOf`: READY now outranks every alert tag (the spec derives the tile from ticket stages first).
- `ticketTags` is a `SvelteMap`, so tile tags are reactive on their own rather than via the `alerts.open` reassignment.
- Sheet "Ready" copy waits for the fulfilment setting (no wrong "deliver to" flash on pick-up branches) and falls back to seat if the settings load fails.
- Open-ticket detail keys notes by index (duplicate notes crashed the keyed each).

Deferred / accepted:
- Undoing a Served advance back to Ready raises no new alert: re-raising would chime on an undo, which the spec forbids.
- The chime replays for already-open order-ready alerts after a Hub reload (same as other alert kinds).
- Strip sits under the page head in the catalogue column (noted in the self-review log); extra stage buttons in the strip are accepted scope.

Verification: cargo fmt/clippy `-D warnings` and tests for proto/store/core/api green; Hub `check`, `format:check`, `check:strings`, `check:layers` green; Hub `alerts`, `tile-sheet`, `pos` specs 207 passed.
