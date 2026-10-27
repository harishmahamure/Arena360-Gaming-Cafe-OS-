---
title: 'Story 3.14: Undo for End and Settle'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: '2de75054e483aaa4f116f6436d1590ae031e946f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-13-settle-bar-and-payment-split-at-the-counter.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-12-settle-a-bill-into-payment-records-with-reference-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** A mis-tapped End or Settle is final: Story 3.11/3.12 commit the bill and payments with no way back, so one wrong tap costs the café or the player money.

**Approach:** Add the compensating intents `session.end.reverted` and `session.settle.reverted` (8 s window, same staff, forward `rev`), a shared irreversible-effect guard, and an `undo-toast` in the Hub that counts down and submits the undo. Precedent for a window-limited compensating intent: `stock.in.reverted` (`crates/arena-core/src/apply/stock.rs`, `STOCK_UNDO_MS`).

## Boundaries & Constraints

**Always:**
- Window is `UNDO_WINDOW_MS` (8 000 ms, already in `arena-proto/src/session.rs`) measured from `ended_at` (End) or `settled_at` (Settle) against the apply clock. Past it: `UNDO_NOT_ALLOWED` (new `ErrorCode`, wired everywhere `Stale`/`Expired` are listed, including generated TS and the cloud/kiosk error maps).
- Undo must come from the same staff actor as the forward action (`ended_by` is a new `sessions` column set by `end_session`; `orders.settled_by` already exists), else `UNDO_NOT_ALLOWED`. The toast is local to the window that committed the forward action; that is the "same Hub window" rule (apply has no window id).
- Both reverted intents carry `base_rev` = the rev read after the forward action (session rev for End, order rev for Settle); a mismatch is `STALE`. Both need the same permission as the forward intent (`session.end` / `pos.sell`).
- `session.end.reverted` is allowed only if the session is `ended`, postpaid, its order is `open` with no payments, and no live session exists on the device (else `UNDO_NOT_ALLOWED`). It reopens the segments closed at End (`to_at` NULL), sets the session back to `active` (clears `ended_at`, `end_reason`, `ended_by`), deletes the order and its lines (delete changes carry `before`), sets the device `in_session` again with the prior state restored as at start, and the kiosk unlocks to the session screen (push as `session.start` does). Time between End and Undo is billed.
- `session.settle.reverted` is allowed only if the order is `settled` (else `STALE`). It deletes its payment rows, sets the order `open` and clears `settled_at`/`settled_by`; the tile's UNPAID line returns.
- Shared irreversible-effect guard: a registry in `arena-proto` of effect kinds (`receipt_printed`, `drawer_kicked`, `invoice_number_assigned`) and a store table recording an effect against a forward intent id/order. If any effect is recorded for the forward action, Undo is rejected `UNDO_NOT_ALLOWED`; the Hub toast closes (or is not shown) when the hub event reports an effect. With none recorded, Undo is allowed. Nothing in this story records an effect except test fixtures; no invoice numbering.
- A new money action (another End or Settle) replaces the toast; the earlier action is final in the UI (server still enforces only the window and staff).
- Audit lists both the forward and the reverted intent and links them (reverted row references the forward `intent_id`, shown by the audit viewer).
- Every new intent and its `.reverted` twin is added to the Story 1.8 proptest generator and `arena-replay`.
- Money is integer minor units; the kiosk lock/unlock state follows the device state exactly as before.

**Never:**
- Wallet, credit, pack undo (3.16+), forced-end PIN undo (3.34), receipts, invoice numbers, shift binding, code comments (user rule).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Undo End | ended 3 s ago, same staff, rev matches, order open and unpaid | session active, segments reopened, order+lines deleted, device in_session, kiosk unlocked | No error |
| Undo Settle | settled 3 s ago, same staff, rev matches | payments deleted, order open, UNPAID returns | No error |
| Late | 9 s after the forward action | nothing changes | `UNDO_NOT_ALLOWED` |
| Other staff | different actor | nothing changes | `UNDO_NOT_ALLOWED` |
| Stale rev | `base_rev` not the current rev | nothing changes | `STALE` |
| Effect recorded | receipt/drawer/invoice effect on the forward action | nothing changes; toast closes | `UNDO_NOT_ALLOWED` |
| Device re-used | End undone but device already has a new live session | nothing changes | `UNDO_NOT_ALLOWED` |
| Order already paid | End undo after the bill was settled | nothing changes | `UNDO_NOT_ALLOWED` |
| Replace | End, then Settle within 8 s | toast shows the Settle; End is final in the UI | No error |

</intent-contract>

## Code Map

- `crates/arena-proto/src/session.rs`, `intent.rs`, `error.rs`, `lib.rs`, `permission.rs` -- `SessionEndReverted{session_id}`, `SessionSettleReverted{order_id}`, kinds/payloads/permissions (mirror `SessionEnd`/`SessionSettle`), `UNDO_NOT_ALLOWED`, effect-kind registry; regenerate `packages/ui/src/generated/*`.
- `crates/arena-store/migrations/0028_undo.sql`, `src/sessions.rs`, `src/migrations.rs`, `src/lib.rs` -- `sessions.ended_by`, `irreversible_effects` table, reopen/delete helpers (`delete_row`, `read_row` as in `stock.rs`), effect lookup.
- `crates/arena-core/src/apply/session.rs` -- `load/base_rev/validate/mutate` for both reverts, mirroring the End/Settle functions; wire in `mod.rs` (`Target`), `guards.rs`, `validate.rs`, `mutate.rs`, `changes.rs` if needed; a guard module for effects.
- `crates/arena-api/src/ui_auth.rs` (~L2006 push block), `ui_events.rs`, `src/snapshot.rs` -- push the session screen on End undo, emit `bill.settled`-style event for the reverts, expose effect state/`undo_until` to the Hub.
- `crates/arena-core/tests/proptest_apply.rs`, `arena-replay/src/lib.rs`, `crates/arena-core/src/apply/tests/session.rs` -- generator entries and unit tests for the matrix.
- `packages/ui/src/components/UndoToast.svelte` (+ export, `types.ts`), `packages/ui/src/messages/en.json` -- `undo-toast`: `surface-2`, `border-strong`, bottom-centre above the settle/bottom bar, message with tabular amount, seconds left, accent Undo; never blocks the floor.
- `apps/hub/src/lib/tile-actions.svelte.ts`, `TileSheet.svelte`, `routes/floor/+page.svelte`, `lib/audit/labels.ts` -- toast state (one at a time), 8 s countdown, undo jobs, error mapping, audit labels; `apps/hub/tests/undo.spec.ts` (+ fixture/mocks as `settle.spec.ts`).

## Tasks & Acceptance

**Execution:**
- [x] Protocol, error code, store migration and helpers as in the Code Map
- [x] Apply logic for both reverts, the guard, kiosk unlock push and events
- [x] Rust unit tests for every matrix row, proptest and replay entries
- [x] `UndoToast` component and Hub wiring with i18n strings (`pnpm run check:strings`)
- [x] Playwright spec for the toast (countdown, undo End, undo Settle, replacement, effect closes it, axe)

**Acceptance Criteria:**
- Given End or Settle commits, when the toast shows, then it counts down 8 s and Undo submits the matching `.reverted` intent with the forward `rev`.
- Given Undo of an End, then the session is back with segments reopened, the bill deleted, the device in session and the kiosk unlocked.
- Given Undo of a Settle, then payments are reversed, the order is unpaid and UNPAID returns on the tile.
- Given a late, other-staff, effect-blocked or stale undo, then it is rejected `UNDO_NOT_ALLOWED` or `STALE` with no change.
- Given a recorded `invoice_number_assigned` effect fixture, then Undo is rejected, while a bill numbered at window close is undoable only before that moment.
- Given an undo, then the audit lists both intents, linked.

## Spec Change Log

## Review Triage Log

## Design Notes

Compensation deletes rows with `before` images (as `stock.in.reverted` deletes the lot) so replay and the gap-free change log stay exact. `ended_by` is needed because `sessions` records no ender; the actor check reads it, and `settled_by` for Settle. The ordered replacement of toasts is UI-only.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` -- pass (proto TS regenerated and committed; `pnpm run check:generated` pass).
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast` -- pass.
- `pnpm exec playwright test undo.spec.ts settle.spec.ts session-end.spec.ts` in `apps/hub` -- pass.

## Auto Run Result

Status: done
Verified: `cargo fmt --check`, clippy (proto/store/core/replay/api/kiosk-core, `-D warnings`), `cargo test` for those crates, `pnpm --filter @arena/hub check`, check:strings, check:tokens-used, check:contrast, Playwright `undo.spec.ts` + `settle.spec.ts` (42 passed).
Left for later: the `effect.recorded` event has no sender until Epic 5 records a real effect (the server already rejects Undo on a recorded effect); undoing a Settle does not re-lock the kiosk to "Pay at counter".
