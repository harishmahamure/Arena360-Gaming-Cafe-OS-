---
title: 'Story 3.13: Settle bar and payment split at the counter'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_revision: 'adadc1c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-12-settle-a-bill-into-payment-records-with-reference-rules.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 3.12 can settle an order, but staff have no screen for it: an ended session leaves an UNPAID tile and nothing in the tile sheet takes payment.

**Approach:** The tile sheet of a device with an unpaid bill shows a `settle-bar` (total, Cash · UPI · Card, a primary Settle) with a `payment-split` (one row per method used: amount, plus a reference for UPI and card). The remainder recomputes live; Settle sends the 3.12 `session.settle` with the order `order_rev`.

## Boundaries & Constraints

**Always:**
- The unpaid bill is the snapshot bill of that device with an `order_id`. The bar shows whatever the device state (free, offline, maintenance).
- Choosing a method adds its row (one row per method) prefilled with the remainder and focuses the amount. Amounts are decimal in major units, parsed to integer minor units with the branch currency exponent; blank, zero, negative or too many decimals are invalid.
- Remainder: "Remaining ₹x" (warning) while non-zero, "Paid in full" (ok) at zero. Cash above the remainder shows "Change ₹x" and the sent cash amount is reduced by the change (cash that is entirely change is dropped). Non-cash above the bill shows "Only cash can be more than the bill." and Settle stays disabled.
- Settle is disabled with the inline reason "Remaining ₹x" until the remainder is zero. With a zero remainder, Settle or Ctrl+Enter validates the rows: UPI needs exactly 4 digits, card a non-blank reference of at most 64 characters, every amount above 0; failures show inline errors and focus the first invalid field, nothing is sent. Ctrl+Enter does nothing while the remainder is non-zero.
- Method buttons are reachable by Tab and Left/Right arrows. No QR anywhere.
- Permission `pos.sell` goes through the existing sheet approval (inline PIN). After success the bar is replaced by "Bill settled · ₹x".

**Never:**
- Undo toast and `session.settle.reverted` (3.14); wallet, credit, packs (3.16+); POS screen and shift binding (Epic 5); receipts and print.
- Code comments (user rule).

## Code Map

- `apps/hub/src/lib/settle.ts` -- pure money helpers: `parseAmount`, `amountText`, `tally`, `rowErrors`, `toPayments`.
- `apps/hub/src/lib/SettleBar.svelte` -- the settle bar and payment split.
- `apps/hub/src/lib/TileSheet.svelte`, `tile-actions.svelte.ts`, `routes/floor/+page.svelte` -- `unpaid` prop, `settleJob`, Ctrl+Enter, settled state.
- `packages/ui/src/components/Button.svelte` -- `reasonParams` for a disabled reason with placeholders.
- `packages/ui/src/messages/en.json` -- `hub.settle.*`, `hub.sheet.done.settled`.
- `apps/hub/tests/settle.spec.ts` -- unit and browser tests.

## Tasks & Acceptance

**Execution:**
- [x] Pure helpers for amounts, remainder, change, reference rules and payment rows
- [x] `SettleBar` with `payment-split`, live remainder, inline errors and keyboard flow
- [x] Tile sheet wiring: unpaid bill, `settleJob`, PIN approval, Ctrl+Enter, settled state
- [x] Messages and the `Button` reason parameters
- [x] Tests: helpers, split by keyboard, reference errors, change, overpay, axe

**Acceptance Criteria:**
- Given an unpaid bill, the sheet shows the total in `hub-numeric`, Cash · UPI · Card, no QR, and Settle disabled with "Remaining ₹x".
- Given rows, the remainder updates live, shows "Paid in full" at zero and "Change ₹x" when cash exceeds it.
- Given a UPI row without 4 digits or a card row without a reference, Settle shows the inline error and sends nothing.
- Given a zero remainder, Settle or Ctrl+Enter sends `session.settle` and the bar shows the settled state; Ctrl+Enter does nothing while the remainder is non-zero.

## Verification

**Commands:**
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:generated` -- pass.
- `pnpm exec playwright test settle.spec.ts` in `apps/hub` -- pass (20 tests, desktop and phone).
