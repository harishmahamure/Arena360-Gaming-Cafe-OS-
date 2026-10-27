---
title: 'Story 3.16: Wallet Top-up and Paying from the Wallet'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'cfd2631'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-14-undo-for-end-and-settle.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-12-settle-a-bill-into-payment-records-with-reference-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Regulars cannot prepay. `Permission::WalletTopup` and `SessionMode::Wallet` exist but there is no wallet, no ledger and no way to pay a bill from stored value, so every rupee of prepaid money is untraceable.

**Approach:** Add `wallets` and `wallet_txns` (migration backfills a zero wallet per existing player; `player.create` now creates one). `wallet.topup` takes the amount and cash/UPI/card payments, writes a settled top-up order, its payments, a positive ledger row and the balance in one transaction; `wallet.topup.reverted` is its 8 s Undo. `session.settle` gains a `wallet` payment method for profiled players that appends a negative ledger row, with `INSUFFICIENT_BALANCE` as the guard. The Hub shows the balance and ledger on the Players profile, a Top up flow in the same `settle-bar`, and a wallet row in `payment-split`.

## Boundaries & Constraints

**Always:**
- `wallets {player_id PK, balance_minor >= 0 CHECK, rev, created_at, updated_at}`; `wallet_txns {id, player_id, delta_minor != 0, reason topup|bill, method (cash|upi|card, null for a bill spend), staff_id, order_id, business_day, fy, rev, created_at, updated_at}`. Both are change-log tables (replayable). Balance equals the sum of a player's deltas.
- `wallet.topup {player_id, amount_minor, payments}` needs `wallet.topup`, no `base_rev`. Amount 1..=10_000_000 minor; payments are cash/UPI/card only with the Story 3.12 reference rules and must sum to the amount (`PAYMENT_SHORT`); `wallet` method is `VALIDATION_FAILED`. Unknown player `NOT_FOUND`. Writes order (`kind='topup'`, no session, status settled, settled_at/by), payments, one `wallet_txns` (`topup`, method = the single method or `mixed` for a split), balance += amount (wallet row created if missing).
- `wallet.topup.reverted {order_id}` needs `wallet.topup`, `base_rev` = order rev (`STALE`), same staff, 8 s window, no recorded effect, and balance >= amount; else `UNDO_NOT_ALLOWED`. Deletes payments, txn and order (with `before` images) and lowers the balance.
- `session.settle` accepts method `wallet` only on a bill with a player (`VALIDATION_FAILED` on `payments[i].method` for a guest), at most one wallet row, no reference. Amount above the balance: `INSUFFICIENT_BALANCE`, nothing written. Settle appends a negative `bill` txn linked to the order and lowers the balance in the same transaction; `session.settle.reverted` deletes that txn and restores the balance.
- `SnapshotBill` carries `wallet_minor` (null for guests). `PlayerProfile` carries `wallet_minor`. `GET /ui/players/wallet?id=` returns the balance and the transactions newest first with the staff name. SSE `wallet.changed {player_id}` fires on any `wallets` change.
- Every new intent and its `.reverted` twin goes into the Story 1.8 proptest generator and `arena-replay`. Money is integer minor units. No code comments (user rule).

**Never:**
- Wallet sessions, packs, credit, guest conversion (3.17+), refunds to wallet, loyalty credit, receipts, invoice numbers.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Top up | player, 500.00 by cash | txn +, balance +, toast Undo | No error |
| Top up split | 300 cash + 200 UPI 1234 | one txn method `mixed`, two payments | No error |
| Undo top-up | within 8 s, same staff, balance >= amount | rows deleted, balance back | No error |
| Undo, spent | balance below the amount | nothing changes | `UNDO_NOT_ALLOWED` |
| Pay from wallet | bill 120, balance 200, wallet 120 | txn -120, balance 80 | No error |
| Wallet short | wallet 150, balance 100 | nothing written | `INSUFFICIENT_BALANCE` |
| Guest bill | wallet row sent | nothing written | `VALIDATION_FAILED` |
| Wallet in top-up | method wallet | nothing written | `VALIDATION_FAILED` |
| Undo settle | settle with wallet, undo | txn deleted, balance restored | No error |

</intent-contract>

## Tasks & Acceptance

**Execution:**
- [x] Protocol (`WalletTopup`, `WalletTopupReverted`, `PaymentMethod::Wallet`, `INSUFFICIENT_BALANCE`, snapshot/profile fields, events), migration, store tables and helpers
- [x] Apply logic for top-up, its revert, the wallet payment in settle and its revert; `player.create` creates the wallet
- [x] API: wallet read route, top-up outcome, `wallet.changed` event
- [x] Rust unit tests for every matrix row, proptest and replay entries
- [x] Hub: wallet row in `payment-split`, Top up flow, balance and Wallet list on the profile, strings, audit labels
- [x] Playwright spec for top-up, undo, wallet payment, guest bill and axe

**Acceptance Criteria:**
- Given a player profile, when staff choose Top up and enter an amount, then the same `settle-bar` shows cash, UPI and card only.
- Given the top-up settles, then a `wallet_txns` row is appended, `wallets.balance_minor` rises, payments are recorded, `wallet.changed` updates open views and the toast offers Undo.
- Given a profiled bill with a balance, then `payment-split` offers a wallet row capped at min(balance, remainder), and settling appends a negative txn in the same transaction.
- Given a wallet payment above the balance, then `INSUFFICIENT_BALANCE` and nothing is written.
- Given the Players profile, then it shows the balance and a Wallet list newest first whose running balance equals the sum of deltas.
- Given a guest bill, then no wallet option is offered.

## Spec Change Log

## Review Triage Log

Review 2026-10-27: no blocking findings. Deferred (non-blocking): pre-0030 backfill untested; top-up undo deletes the ledger row.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; `pnpm run check:generated`.
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`.
- `pnpm exec playwright test wallet.spec.ts settle.spec.ts undo.spec.ts players.spec.ts` in `apps/hub`.

## Auto Run Result

Status: review
Verified: `cargo fmt --check`, clippy `-D warnings` (proto/store/core/replay/api/kiosk-core/cloud, all targets), `cargo test` for proto, store, core (402 lib tests + proptest with the new `WalletTopup`/`WalletPay` ops and replay of both new tables), replay, api (207 lib tests incl. top-up outcome, wallet route and `wallet.changed`), kiosk-core; `pnpm run gen:types`; `pnpm --filter @arena/hub check`, check:strings, check:tokens-used, check:contrast, check:protocol, check:layers; Playwright `wallet`, `settle`, `undo`, `session-end`, `pause`, `players`, `tile-sheet`, `session-start`, `audit`, `floor` specs (230 passed). `check:generated` only reports the regenerated TS as new/stale until it is committed.
Not run: `arena-e2e` (Unix-only, does not build on this Windows machine). `arena-cloud --test binary unreachable_at_start_retries_then_exits_1_without_logging_the_token` fails here because connecting to `127.0.0.1:9` times out instead of being refused; it is unrelated to this change. `arena-core` `commands::owner_staff_and_device_edits_win_over_a_stale_base_rev` is flaky and also fails on the untouched baseline.
Left for later: the 0030 migration's zero-wallet backfill for players that already exist is plain SQL with no test that opens a pre-0030 database; undoing a top-up deletes the ledger row rather than appending a reversing one (as Story 3.14 does for payments); the wallet list shows staff by display name only; wallet-paid sessions and wallet extension are Stories 3.24 and 3.28.
