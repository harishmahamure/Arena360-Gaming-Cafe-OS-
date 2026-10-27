---
title: 'Story 3.18: Enable Credit for a Player and Settle the Remainder on Credit'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_revision: 'b08b69b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-16-wallet-top-up-and-paying-from-the-wallet.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-17-convert-a-guest-to-a-profile-including-at-settle-and-list-se.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Trusted regulars cannot play now and pay later. `Permission::CreditManage` exists but there is no credit limit, no outstanding balance, and `payment-split` offers no credit method, so staff track tabs on paper.

**Approach:** Add `credit_balances` (migration `0031` also widens `wallet_txns.reason` to allow `credit`). `credit.set_limit {player_id, limit_minor, reminder_days}` (permission `credit.manage`) creates or edits the row. `session.settle` gains a `credit` payment method for a profiled, credit-enabled player: it appends a positive `credit` ledger row and raises `outstanding_minor` in the same transaction, and `CREDIT_LIMIT` refuses a spend above the limit. A manager-PIN override (`credit_override` on the settle, elevated, holder needs `credit.manage`) is allowed once per player per business day (`OVERRIDE_USED_TODAY`). The Hub shows a Credit block on the profile and a credit row in `payment-split`.

## Boundaries & Constraints

**Always:**
- `credit_balances {player_id PK, limit_minor >= 0, outstanding_minor >= 0, reminder_days JSON, credit_paused, last_override_day, rev, created_at, updated_at}` is a change-log table. Credit ledger rows live in `wallet_txns` with `reason='credit'`, `delta_minor` = the spend (positive, so outstanding = sum of a player's credit deltas), `method` null (or `override`); the wallet balance and Wallet list ignore them.
- `credit.set_limit` needs `credit.manage` (actor or the elevating manager), no `base_rev`. Limit 0..=10_000_000 minor; reminder days 1..=10 distinct ascending whole days in 1..=365 (default `[3,7,14]`). A limit below outstanding is `CREDIT_BELOW_OUTSTANDING` on `limit_minor`; an unknown player is `NOT_FOUND`.
- A `credit` payment needs a bill with a player (`VALIDATION_FAILED` on `payments[i].method` for a guest), a credit row with limit > 0 that is not paused, at most one credit row and no reference. outstanding + amount above the limit is `CREDIT_LIMIT` on `payments[i].amount_minor` unless `credit_override` is set on an elevated settle whose holder has `credit.manage` (else `PERMISSION_DENIED`) and `last_override_day` is not the current business day (else `OVERRIDE_USED_TODAY`); the override sets `last_override_day`. The check runs inside the apply transaction. A refusal writes nothing.
- `session.settle.reverted` removes the credit ledger row, lowers outstanding and, when the spend used the override, clears `last_override_day`.
- `SnapshotBill.credit` and `WalletView.credit` carry limit, outstanding, reminder days and paused. SSE `wallet.changed` also fires when `credit_balances` changes.
- New intents go into the Story 1.8 proptest generator and `arena-replay`. Money is integer minor units. No code comments (user rule).

**Never:**
- Collecting credit (3.19), aging and auto-pause (3.20), credit extension of sessions (3.28), reminders (Epic 8), a second ledger table.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Enable credit | manager, limit 500.00, default days | row created | No error |
| Limit below outstanding | outstanding 300, limit 200 | nothing written | `CREDIT_BELOW_OUTSTANDING` |
| Settle on credit | limit 500, outstanding 100, remainder 120 | credit txn +120, outstanding 220 | No error |
| Over the limit | outstanding 450, remainder 120, limit 500 | nothing written | `CREDIT_LIMIT` |
| Override | same, elevated manager, `credit_override` | settles, `last_override_day` set | No error |
| Second override today | override already used | nothing written | `OVERRIDE_USED_TODAY` |
| Guest / no credit | guest bill or player without a row | nothing written | `VALIDATION_FAILED` |
| Undo settle | settle on credit, undo | txn deleted, outstanding restored | No error |
| Converted guest | guest bill linked to a credit player | credit offered | No error |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0031_credit.sql`, `src/credit.rs`, `src/branch.rs`, `src/lib.rs`, `src/sessions.rs`, `src/wallet.rs` -- table, helpers, change-log registration, open-order join, ledger filter.
- `crates/arena-proto/src/{session,intent,error,lib}.rs` -- `CreditSetLimit`, `CreditView`, `PaymentMethod::Credit`, `credit_override`, error codes.
- `crates/arena-core/src/apply/{credit,session,guards,mutate,validate,mod}.rs`, `snapshot.rs` -- apply logic and snapshot.
- `crates/arena-api/src/{ui_auth,ui_events}.rs` -- wallet route returns credit; `wallet.changed` on credit rows.
- `apps/hub/src/lib/{SettleBar.svelte,settle.ts}`, `lib/players/CreditBlock.svelte`, `routes/players/+page.svelte`, `packages/ui/src/messages/en.json` -- credit UI.

## Tasks & Acceptance

**Execution:**
- [x] Migration, store helpers, change-log table registration, open-order credit join
- [x] Protocol types, intent kind, error codes, `PaymentMethod::Credit`, `credit_override`
- [x] Apply logic for `credit.set_limit`, credit payment in settle, override and revert; snapshot credit
- [x] API wallet route and `wallet.changed` for credit rows
- [x] Rust tests for every matrix row, proptest and replay entries
- [x] Hub: Credit block on the profile, credit row in `payment-split`, inline PIN override, strings, audit labels
- [x] Playwright spec for enable, credit settle, limit reached with override, guest bill and axe

**Acceptance Criteria:**
- Given a Manager on a profile, when they enable credit with a limit and reminder days, then a `credit_balances` row exists and a limit below outstanding is `CREDIT_BELOW_OUTSTANDING`.
- Given a bill for a credit-enabled player, when staff add a credit row, then it shows outstanding, limit and headroom and settling appends a credit ledger row and raises outstanding in one transaction.
- Given outstanding + new above the limit, then `CREDIT_LIMIT` shows "Credit limit reached, see counter." with an inline PIN override; the override works once per player per business day.
- Given a guest or a player without credit, then no credit option exists; a guest bill linked to a credit-enabled player shows it.
- Given two concurrent spends, then the limit check inside each transaction rejects the second if it would exceed the limit.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test` in `apps/hub`.

## Auto Run Result

Status: review
Verified: `cargo fmt --check`; clippy `-D warnings` clean for every crate except `arena-kiosk-svc` tests (`tests/discovery.rs` has unused imports on this Windows machine, unchanged by this story; its lib and bins are clean); `cargo test` for proto, store, core (incl. the new credit tests and the proptest with `CreditSet` / `CreditPay`, 600 cases), replay (`CreditSetLimit` replayed) and api (incl. the credit route test); `pnpm run gen:types`; `pnpm --filter @arena/hub check`, check:strings, check:tokens-used, check:contrast, check:protocol, check:layers, format:check; the full Hub Playwright suite (842 passed, 11 skipped) including the new `credit.spec.ts` (16 passed). One run of `players.spec.ts` "Ctrl+K from another screen" failed under load and passes alone.
Not run: `arena-e2e` (Unix-only). These fail the same way on the untouched baseline: `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`, and the Windows-admin `arena-kiosk-svc` lockdown/pipe tests. `arena-api` `ui_events::a_component_change_also_reports_the_bundles_that_hold_it` failed once under load and passes alone.
Left for later: nothing sets `credit_paused` yet (Story 3.20); credit collection and the allocation of spends (Story 3.19); a limit of 0 can be saved and behaves as "credit off"; staff without `credit.manage` see the credit block read-only (no PIN flow on the profile); the override is checked per player per business day from the settle intent, with a settle-undo clearing it.
