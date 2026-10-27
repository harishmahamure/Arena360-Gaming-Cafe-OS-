---
title: 'Story 3.19: Collect Credit with Partial Payments and Undo'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_revision: '6cdc737'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-16-wallet-top-up-and-paying-from-the-wallet.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-18-enable-credit-for-a-player-and-settle-the-remainder-on-credi.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Credit can be put on a tab (3.18) but never collected, so `outstanding_minor` only grows and staff clear tabs on paper.

**Approach:** `credit.collect {player_id, amount_minor, payments}` (permission `credit.collect`) appends a negative `credit` row to `wallet_txns`, lowers `outstanding_minor`, records payments (cash, UPI, card or wallet) on a new settled `orders` row of kind `credit_collect` (migration `0032` widens the kind check), all in one transaction. `credit.collect.reverted` undoes it under the Story 3.14 rules. Allocation to the oldest unpaid spends first (FIFO) is derived from the ledger, not stored: `CreditCollection {cleared[], outstanding_after_minor}` is read back for the intent result (`SettleOutcome.collection`) and the `bill.settled` event. The Hub profile gets a Collect action that opens `settle-bar` prefilled with the full outstanding.

## Boundaries & Constraints

**Always:**
- Amount is 1..=outstanding (else `VALIDATION_FAILED` on `amount_minor`); payments sum to the amount (`PAYMENT_SHORT`), at most eight, no credit method, wallet at most once and within the balance (`INSUFFICIENT_BALANCE`), the same reference rules as settle. A player without a credit row is `NOT_FOUND` on `player_id`. A paused player can still pay. A refusal writes nothing. No `base_rev`.
- A wallet payment appends the usual negative `bill` wallet row and lowers the wallet; the Wallet list still ignores `credit` rows.
- Undo (`credit.collect.reverted`, forward `rev`): same staff, 8 s window, no recorded irreversible effect; refused `UNDO_NOT_ALLOWED` if the limit was lowered so that restored outstanding would exceed it. It deletes payments, ledger rows and the order, restores outstanding and the wallet.
- `session.settle.reverted` stays blocked once the credit it raised was collected.
- Migration `0032` rebuilds `orders` (children saved and restored around the drop) to allow `credit_collect`.
- New intents go into the Story 1.8 proptest and `arena-replay`. Money is integer minor units. No code comments (user rule).

**Never:**
- Aging buckets and auto-pause (3.20), receipts and invoice numbers (Epic 5), a second ledger or allocation table.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Partial collect | owes 180, collect 150 cash | outstanding 30; clears spend 1 (120) and 30 of spend 2 | No error |
| Mixed methods | wallet 40 + UPI 50 | wallet down 40, outstanding down 90 | No error |
| Over-collect | collect more than owed | nothing written | `VALIDATION_FAILED` `amount_minor` |
| Wrong total | payments short of amount | nothing written | `PAYMENT_SHORT` |
| Wallet short | wallet row above balance | nothing written | `INSUFFICIENT_BALANCE` |
| No credit | player without a row | nothing written | `NOT_FOUND` |
| No permission | kitchen role | nothing written | `PERMISSION_DENIED` |
| Undo in time | same staff, within 8 s | outstanding and wallet restored | No error |
| Undo late / other staff | past 8 s or another member | nothing written | `UNDO_NOT_ALLOWED` |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0032_credit_collect.sql`, `src/credit.rs` (`credit_collection`), `src/wallet.rs`, `src/sessions.rs`, `src/lib.rs` -- order kind, FIFO summary, order insert helper, irreversible-effect kind.
- `crates/arena-proto/src/{session,intent,lib}.rs` -- `CreditCollect`, `CreditCollectReverted`, `CreditCollection`, `ClearedSpend`, intent kinds, `SettleOutcome.collection`, `BillSettledEvent.collection`.
- `crates/arena-core/src/apply/credit_collect.rs` (+ `mod`, `guards`, `validate`, `mutate`), `snapshot.rs` -- apply logic and the reader call.
- `crates/arena-api/src/{ui_auth,ui_events}.rs` -- the result carries the collection; `bill.settled` fires for a collection.
- `apps/hub/src/lib/players/{CollectCredit,CollectionSummary,CreditBlock}.svelte`, `lib/undo.svelte.ts`, `UndoDock.svelte`, `tile-actions.svelte.ts`, `audit/labels.ts`, `packages/ui/src/messages/en.json` -- Collect UI.

## Tasks & Acceptance

**Execution:**
- [x] Migration `0032`, FIFO `credit_collection`, order insert helper, tests for the upgrade path
- [x] Protocol types, intent kinds, permission mapping, generated types
- [x] Apply logic for `credit.collect` and `credit.collect.reverted`
- [x] API: result carries the collection; `bill.settled` event with the collection
- [x] Rust tests for every matrix row, proptest op and replay entries
- [x] Hub: Collect on the credit block, prefilled `settle-bar`, collection summary, Undo, strings, audit labels
- [x] Playwright spec for prefill and limits, mixed payments, summary, Undo, refusal and permission

**Acceptance Criteria:**
- Given a player with outstanding credit, when staff choose Collect, then `settle-bar` opens prefilled with the full outstanding, editable down to any positive amount, payable by cash, UPI, card or wallet.
- Given an amount up to outstanding, when settled, then a negative credit row, payments and lowered outstanding are written in one transaction and a summary lists the cleared spends (date, bill reference, amount) and outstanding after.
- Given an amount above outstanding, then it is refused inline and nothing is written.
- Given a committed collection, then Undo within 8 s restores outstanding and the allocation exactly.
- Given the collection summary data, then it is on the `bill.settled` event and the intent result for Epic 5 receipts.

## Spec Change Log

## Review Triage Log

- Fixed: `a_collection_can_be_paid_from_the_wallet_and_by_upi` asserted payment rows in table order, which varies with random ids and failed about half the runs; it now compares the sorted methods.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test` in `apps/hub`.

## Auto Run Result

Status: review
Verified: the full Hub Playwright suite (849 passed, 11 skipped; two `players.spec.ts` tests failed under load and pass alone); `cargo fmt --check`; clippy `-D warnings` clean for every crate except `arena-kiosk-svc` tests (pre-existing unused imports in `tests/discovery.rs`; its lib and bins are clean); `cargo test --workspace --exclude arena-e2e --no-fail-fast` passes for proto, store (incl. the v31 upgrade test), core (incl. seven collection tests and the proptest with `CreditCollect`), replay, and api (incl. the collection event and permission tests); `pnpm run gen:types`; Hub check, check:strings, check:tokens-used, check:contrast, check:protocol, check:layers, format:check; Hub Playwright `credit-collect.spec.ts`, `credit`, `wallet`, `undo` and `players` specs.
Not run: `arena-e2e` (Unix-only). These fail the same way on the untouched baseline: `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`, and the Windows-admin `arena-kiosk-svc` lockdown, pipe, config, DPAPI and procs tests.
Left for later: aging buckets and auto-pause (Story 3.20); the allocation is computed from the ledger order (insertion order of `wallet_txns`), so a future import of ledger rows must keep that order.
