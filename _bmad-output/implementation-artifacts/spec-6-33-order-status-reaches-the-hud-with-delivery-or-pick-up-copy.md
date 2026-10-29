---
title: 'Story 6.33: Order status reaches the HUD with delivery or pick-up copy'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '0453fbd00cd587754943757c0db08a9f44f3650c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-32-pay-now-from-the-wallet-pay-at-counter-and-unpaid-order-line.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-16-staff-messages-reach-the-seat-and-stay-in-the-messages-bell.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A player who ordered sees only a count on the session screen. Staff cannot tell the seat the food is ready, and a cancelled line vanishes without a word.

**Approach:** A new intent `order.ready{order_id}` (Mark ready on the tile sheet's order section) stamps the open bill `ready_at`; placing more lines clears it. The Hub's 5 s session tick carries each open order of the PC as `orders[{order_id, items, unserved, ready}]` (`ready` is the branch's fulfilment, `seat` or `counter`, once marked). The kiosk service compares each tick with the last one it saw and raises order notes: "Ordered: n items", "Order's ready — on its way to you." / "…grab it at the counter.", and the HUD pill shows an "Ordered" tag while any line is unserved. A cancelled line is a `order.line_cancelled` event pushed to the PC with the item name. Notes are info `hud-toast`s (8 s, no focus) kept in the `messages-bell`.

## Boundaries & Constraints

**Always:** The Hub decides; the kiosk only renders. `order.ready` needs `pos.sell`, takes no `base_rev`, and is refused on a closed bill, an order with no unserved line, or one already ready. A tick after a reconnect (or service restart) raises at most one note per order, from its latest state; unchanged state raises nothing. Notes clear with the session as staff messages do. Copy lives in `en.json` (`player.order.*`).

**Never:** A new permission, kitchen tickets or KOT (Epic 7), a ready push to browsers other than the existing change feed, persisting notes across a service restart, replaying every missed toast.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Order commits | kiosk or staff `order.place` adds 3 items | within 5 s toast "Ordered: 3 items", bell row, HUD pill "Ordered" |
| Mark ready, seat | fulfilment `seat`, unserved line | `order.ready` applies; toast "Order's ready — on its way to you." |
| Mark ready, counter | fulfilment `counter` | toast "Order's ready — grab it at the counter." |
| Last line served | unserved 0 | "Ordered" tag clears; no toast |
| Line cancelled | unserved line cancelled at the Hub | toast "<item> was cancelled — see the counter." |
| Reconnect | Hub unreachable, then orders changed | one note per order from latest state, none replayed |
| Already ready / nothing unserved / closed bill | `order.ready` | `VALIDATION_FAILED` on `order_id`, nothing written |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0066_order_ready.sql`, `migrations.rs`, `src/sessions.rs` -- `orders.ready_at`, `OrderRow.ready_at`, `mark_order_ready`, `list_order_states` (items/unserved/ready per open order of a session or device).
- `crates/arena-proto/src/{pos.rs,intent.rs,session.rs,frame.rs,lib.rs}` -- `OrderReady` payload, `IntentKind::OrderReady` (PosSell), `OrderStatus` and `SessionScreen.orders`, `HubEvent::OrderLineCancelled`.
- `crates/arena-core/src/apply/{order_ready.rs,mod.rs,validate.rs,mutate.rs,guards.rs,order_place.rs,order_line.rs}`, `session_screen.rs` -- the intent, clear `ready_at` on place, orders in the tick.
- `crates/arena-api/src/{ui_auth.rs,lan/registry.rs}`, `lan/session.rs` -- push `order.line_cancelled`.
- `crates/arena-kiosk-core/src/ipc.rs`, `crates/arena-kiosk-svc/src/{pc.rs,app.rs,session.rs,hub_link.rs}` -- diff ticks into coded notes.
- `apps/kiosk/src-tauri/src/bridge.rs`, `apps/kiosk/src/lib/{bridge.ts,messages.ts,MessagesBell.svelte,Hud.svelte,hud.ts}`, `routes/+page.svelte` -- coded notes, toast copy, HUD pill tag.
- `apps/hub/src/lib/{OrderSection.svelte,tile-actions.svelte.ts,TileSheet.svelte,audit/labels.ts}`, `packages/ui/src/messages/en.json`, generated types.
- Tests: `arena-core` `order_ready`, `proptest_apply.rs`, `arena-kiosk-svc` pc diff, `arena-api` lan, kiosk `hud`/`messages-bell` specs, hub `tile-sheet.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Store: migration, `ready_at`, order states query
- [x] Protocol: `order.ready`, `OrderStatus`, `SessionScreen.orders`, `order.line_cancelled` event; regenerate TS types
- [x] Core: `order.ready` validate/mutate/audit, clear on place, orders in `session_screens`, proptest op
- [x] Hub LAN: push `order.line_cancelled` to the PC after a committed cancel
- [x] Kiosk service/IPC/Tauri: tick diff into coded notes, cancel event, bell and toast plumbing
- [x] Kiosk app: toast and bell copy, HUD pill "Ordered" tag, strings
- [x] Hub app: Mark ready in the order section, strings, audit label
- [x] Tests for each layer

**Acceptance Criteria:**
- Given an order commits, then within 5 s the kiosk shows the info toast "Ordered: <n> items" and the `hud-pill` an "Ordered" tag while any line is unserved.
- Given Mark ready, then the kiosk shows within 5 s the seat or pick-up copy for the branch fulfilment.
- Given the last line served, then the tag clears.
- Given an unserved line cancelled, then the kiosk shows "<item> was cancelled — see the counter."
- Given any order toast, then it auto-dismisses after 8 s, never takes focus and is kept in `messages-bell`.
- Given a reconnect, then each open order's latest status is shown once without replaying missed toasts.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-kiosk-core`; `cargo fmt --check`; `cargo clippy` on touched crates
- `pnpm gen:types`, `pnpm --filter @arena/kiosk check`, `@arena/hub check`, Playwright kiosk `hud messages-bell`, hub `tile-sheet`; `pnpm check:strings check:hex check:no-blur check:protocol format:check`

## Auto Run Result

Status: review

- **Summary:** `order.ready{order_id}` (Mark ready in the tile sheet's order section, `pos.sell`) stamps the open bill `orders.ready_at` (migration 0066); placing more lines clears it. The 5 s session tick carries each open order as `orders[{order_id, items, unserved, ready}]` (`ready` = the branch's `seat` or `counter` fulfilment), and a committed `order.line_cancel` pushes `order.line_cancelled{item}` to the PC. The kiosk service diffs each tick against the last one and raises coded notes (ordered, ready seat/counter, cancelled) as info HUD toasts kept in the messages bell; the HUD pill shows "Ordered" while any line is unserved.
- **Verification:** `cargo test` for arena-proto, arena-store, arena-core (1189 + proptest), arena-kiosk-core, arena-api (291 + lan 81) pass; fmt and clippy `-D warnings` clean on touched crates; kiosk and hub svelte-check clean; strings, hex, no-blur, protocol, layers, pins, prettier pass; kiosk Playwright `hud`, `messages-bell` (65) and hub `tile-sheet` (75) pass.
- **Residual risks:** `arena-kiosk-svc` has the same Windows-only test failures as before and `runtime.rs` (`cfg(unix)`) was updated by search-and-replace without compiling; Mark ready bumps the order `rev`, so a settle from a stale sheet answers `STALE` until the snapshot refreshes; a PC's unpaid counter orders reach the tick only while a session is on screen; a cancel event is not replayed after an outage; the real Tauri shell and a real Hub were not exercised; no review pass has run.

## Review Triage Log (code review)

- Fixed: a guest's session no longer picks up another guest's unpaid counter order (counter-order match requires a player id).
- Accepted/deferred: ready push reuses `session.started` like the existing cancel push (same pattern); restart re-toasts per spec test; unknown-code blank copy (same-release apps); counter-order push with no live session waits for a tick; undo of place does not restore `ready_at`; end-to-end kiosk-svc runtime tests for order notes.
