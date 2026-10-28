---
title: 'Story 5.23: Void an unsettled order with manager PIN'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: 'b9afed1e979ef352d9a3fccc9a54ee26a11f4141'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-13-take-stock-at-served-mark-items-served-and-cancel-unserved-l.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Products placed on a running session's bill can only be cancelled line by line, and only while unserved. A wrongly entered order, or one the customer walked away from, cannot be removed whole, and nothing records who approved removing it.

**Approach:** `order.void {order_id, note?}` (permission `pos.void`, on the actor or on the manager whose PIN elevated the intent; `reason`: `mistake`, `changed_mind`, `other`) cancels an open `session` order. Every line's stock goes back (open holds close; stock a line took, at placement or when served, returns through a `return` move), the order is marked `void` with the reason, the note of an `other` reason and the approver, and it is detached from its session so the session can open a fresh bill. The lines stay on the voided order for the record. The tile sheet's order section offers Void order, which opens the reason picker and, without `pos.void`, the inline PIN.

## Boundaries & Constraints

**Always:**

- Only an `open` order of kind `session` on a session, with at least one product line and no play-time line, can be voided. A settled order is `ORDER_SETTLED` (new code, HTTP 409); an already voided one is `STALE`; an unknown one `NOT_FOUND`; an order that carries play time (an ended session's or a block session's bill) or has no items, or a `base_rev`, is `VALIDATION_FAILED` on `order_id` / `base_rev`. Nothing is written on any refusal.
- The reason is required (`REASON_REQUIRED`) and must be one of the three (`VALIDATION_FAILED` on `reason`). `other` needs a note of 1 to 140 plain characters (`VALIDATION_FAILED` on `note`); the note is stored only for `other`.
- `pos.void` is checked on the actor, or on the elevating manager (`PERMISSION_DENIED` otherwise); the owner always may. The approver (elevating manager, else the actor) is stored as `voided_by`; the audit also carries `elevated_by` on the intent.
- Stock: a held line closes its holds; a line without holds returns its items (a bundle's tracked parts times the quantity) through `return` moves into a fresh lot at the average cost. Voiding never writes waste.
- The order keeps `total_minor` and its lines; `session_id` is cleared into `void_session_id`. Reports exclude `status = 'void'` (Stories 5.27, 5.28).

**Never:**

- A void with an Undo twin (a void is final; settled orders are refunded, Story 5.24); a void of play time; a void of a counter sale or plan sale (they settle at once).
- The customer's phone or name in a void row.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Held + served lines | Burger held, Cola served, void as owner | holds closed, Cola `return` move, order `void`, reason stored | none |
| Served kitchen line | Burger marked served, then void | units returned | none |
| Order mode | kitchen line took stock at placement | units returned | none |
| No reason | reason absent | refused | `REASON_REQUIRED` |
| Other, no text | reason `other`, blank or 141-char note | refused | `VALIDATION_FAILED` `note` |
| Counter, no PIN | actor without `pos.void` | nothing written | `PERMISSION_DENIED` |
| Counter + manager PIN | elevated by a manager with `pos.void` | voided, `voided_by` = manager, actor stays | none |
| Settled order | order settled | refused | `ORDER_SETTLED` |
| Voided twice | order already `void` | refused | `STALE` |
| Play time on the bill | ended session's order | refused | `VALIDATION_FAILED` `order_id` |
| Fresh bill | place again after a void | a new order opens; the old placement's Undo is refused | `VALIDATION_FAILED` |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0056_order_void.sql` -- `orders.voided_at/voided_by/void_reason/void_note/void_session_id`; `src/sessions.rs` -- `void_order`, `OrderVoid`.
- `crates/arena-proto/src/pos.rs` -- `OrderVoid`, `OrderVoidEvent`, `VOID_NOTE_MAX_CHARS`; `intent.rs` -- kind `order.void`, requirement `pos.void`, `ReasonCode::VOID`; `error.rs` -- `ORDER_SETTLED`; `session.rs` -- `SnapshotBill.items_order_id`.
- `crates/arena-core/src/apply/order_void.rs` -- load, validate, mutate; `order_line.rs` -- `give_back` shared with cancel; `snapshot.rs` -- `items_order_id` on a running postpaid session's bill.
- `crates/arena-api/src/ui_events.rs` -- `order.voided` event; `ui_auth.rs` -- the kiosk's "Ordered" count follows, `ORDER_SETTLED` is 409.
- Hub: `lib/OrderSection.svelte` (Void order, reason picker), `lib/TileSheet.svelte`, `lib/tile-actions.svelte.ts` (`voidJob`, refusals, `pos.void`), `lib/events.ts`, `routes/+layout.svelte`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/order_void.rs`, store `migrations.rs`, API test in `ui_auth.rs`, proptest `Op::Void` and `assert_voids_hold`, Playwright `tile-sheet.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Store: migration 0056, `void_order`
- [x] Protocol: `OrderVoid`, kind, reasons, `ORDER_SETTLED`, snapshot `items_order_id`, generated TS
- [x] Core: `order.void` (validate, stock back, mark void, detach), `give_back` shared with cancel
- [x] API: `order.voided` event, kiosk count, status mapping
- [x] Hub UI: Void order with reason picker and inline PIN, refusals, event
- [x] Tests: Rust matrix, migration, API, proptest op and invariant, Playwright
- [x] Verification gate

**Acceptance Criteria:**
- Given a placed, unsettled order on a session bill, when staff choose Void, then a reason picker (Entered by mistake · Customer changed mind · Other with text) and, without `pos.void`, an `inline-pin` open in place.
- Given a valid PIN and reason, when `order.void` applies, then the order is marked voided, its holds close and decremented stock returns with reason `return`, the lines leave the session bill, and the audit records reason and approver.
- Given a settled or already voided order, when a void is attempted, then it is rejected `ORDER_SETTLED` or `STALE` and nothing changes.

## Spec Change Log

## Review Triage Log

## Design Notes

The order is the session's one bill, so voiding detaches it (`session_id` cleared, kept as `void_session_id`) instead of widening `orders_one_per_session`: every query that joins an order to its session, and every "open orders" read, then ignores a voided order without a change. A running postpaid session's order holds product lines only (End appends the time lines), so "no play time" is exactly "the session is still running"; an ended or block session's bill is settled, not voided. The snapshot's running bill names its order in `items_order_id` because its `order_id` stays empty (no settle bar before End).

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay`, `arena-tasks`, `arena-hub`, `arena-cloud`, `arena-kiosk-core`; `cargo test` for the same crates -- expected: pass. (`arena-cloud`'s `unreachable_at_start_retries_then_exits_1_without_logging_the_token` fails on a clean checkout too; `a_hung_run_times_out_with_its_own_code` is flaky under a parallel run and passes alone.)
- `pnpm gen:types`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`, Hub `svelte-check`; Playwright `tile-sheet` in `apps/hub` -- expected: pass.
