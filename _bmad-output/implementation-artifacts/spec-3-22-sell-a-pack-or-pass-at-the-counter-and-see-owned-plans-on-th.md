---
title: 'Story 3.22: Sell a Pack or Pass at the Counter and See Owned Plans on the Profile'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'ea44c4f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-21-time-packs-blocks-and-passes-per-device-group.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Owners can define packs, blocks and passes (3.21) but nothing sells them: a prepaid player cannot buy time, no copy of the terms is kept for them, and the profile shows nothing they own.

**Approach:** `plan.sell` (`player_id`, `plan_id`, `payments`, optional `credit_override`) settles in one intent, like `wallet.topup`: it writes a settled order of the new kind `plan_sale` with one plan line (`order_lines.plan_id`, `label`), the payments, the wallet and credit rows for those methods (the same rows `session.settle` writes), and a `player_packs` row that snapshots the plan's terms (name, price, minutes, validity, window, allowed groups) with `expires_at` = settle time + validity days and `minutes_left` = the pack's minutes (null for a pass). Wallet and credit payments, the credit pause and the override follow Story 3.18 through helpers extracted from `session.settle` (`validate_payments`, `insert_payments`, `record_payer_txns`). `plan.sell.reverted` deletes the owned copy, line, payments and ledger rows, reverses wallet and credit, and deletes the order, inside the 8 s window for the same staff, blocked once an irreversible effect is recorded, the credit was collected, or the plan was used. Both kinds require `wallet.topup` (held by Counter). A pass is refused `PASS_ALREADY_ACTIVE` while the player holds an unexpired one. Only packs and passes are sold here; a block is bought when its session starts (3.27). The wallet view (`GET /ui/players/wallet`) lists the owned plans with an `expired` flag. The Players screen gains Sell plan (a plan picker, then `settle-bar` with wallet and credit), a Packs & passes section, and, for a guest, a prompt to create or link a profile first.

## Boundaries & Constraints

**Always:**
- Money is integer minor units; the payments must equal the plan's current price (`PAYMENT_SHORT`); `plan.sell` takes no `base_rev`; `plan.sell.reverted` carries the order `rev`.
- Terms are copied at sale: a later plan edit or deactivation never changes an owned copy; a deactivated plan cannot be sold (`plan_id`).
- Migration `0034_plan_sales.sql` rebuilds `orders` for the `plan_sale` kind (as 0032 did), adds `order_lines.plan_id` and `label`, and creates `player_packs`.
- `plan.sell` joins the Story 1.8 proptest generator and `arena-replay`. No code comments (user rule).

**Never:**
- Spending a pack, window enforcement, expiry sweeps, and plan checks at session start (3.24, 3.27); selling a block; receipts or invoice numbers (Epic 5).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Cash sale | profiled player, active pack, payments = price | order + line + payment + owned copy, 8 s Undo | No error |
| Split | wallet + credit + cash | wallet and credit rows as at settle; Undo reverses all | `INSUFFICIENT_BALANCE`, `CREDIT_LIMIT`, `CREDIT_PAUSED` as at settle |
| Second pass | unexpired pass held | nothing written | `PASS_ALREADY_ACTIVE` on `plan_id` |
| Expired pass | previous pass past `expires_at` | sale allowed | No error |
| Not sellable | unknown player or plan, inactive plan, block | nothing written | `NOT_FOUND` / `VALIDATION_FAILED` with field |
| Short | payments below or above price | nothing written | `PAYMENT_SHORT` |
| Undo | same staff, in window, plan unused | all rows removed, balances restored | `UNDO_NOT_ALLOWED` / `STALE` |
| Guest | Guest picked on Players | "Create or link a profile first" | No intent sent |

</intent-contract>

## Code Map

- `crates/arena-store`: `migrations/0034_plan_sales.sql`, `player_packs.rs`, `branch.rs` (`Table::PlayerPacks`), `sessions.rs` (settlement intent kind), `lib.rs`.
- `crates/arena-proto`: `plans.rs` (`PlanSell`, `PlanSellReverted`, `OwnedPlanView`), `session.rs` (`WalletView.plans`), `intent.rs`, `error.rs` (`PASS_ALREADY_ACTIVE`).
- `crates/arena-core`: `apply/plan_sales.rs`, `apply/session.rs` (shared payment helpers), `apply/{mod,guards,validate,mutate}.rs`, `snapshot.rs`.
- `crates/arena-api/src/ui_auth.rs` (settle outcome for `plan.sell`), `crates/arena-replay`, `crates/arena-core/tests/proptest_apply.rs`.
- `apps/hub`: `lib/players/{SellPlan,PlayerPlans}.svelte`, `routes/players/+page.svelte`, `lib/{plans.ts,undo.svelte.ts,UndoDock.svelte,wallet.svelte.ts}`, `routes/+layout.svelte`, `packages/ui/src/messages/en.json`, `tests/plan-sales.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration, `player_packs` change-log table and store queries
- [x] Protocol types, intent kinds, `PASS_ALREADY_ACTIVE`, generated types
- [x] `plan.sell` and `plan.sell.reverted` apply logic with shared settle helpers
- [x] Owned plans on the wallet view, settle outcome for the sale
- [x] Rust tests (matrix, Undo, wallet view, API, migration), proptest op, replay fixture
- [x] Players screen: Sell plan, Packs & passes, guest prompt, Undo toast
- [x] Playwright spec

**Acceptance Criteria:**
- Given a profiled player, when staff choose Sell plan and pick an active pack or pass, then a bill with one plan line opens in `settle-bar` with the methods of Stories 3.13, 3.16 and 3.18.
- Given the bill is settled, when `plan.sell` applies, then a `player_packs` row (player, plan, minutes left for packs, expiry = settle time + validity days, bought at, order) is created and Undo reverts both the sale and the owned plan.
- Given the player holds an unexpired pass, when a second pass is sold, then it is refused `PASS_ALREADY_ACTIVE`.
- Given the player profile opens, then "Packs & passes" lists each owned plan with hours remaining (packs), window (passes), expiry date and allowed groups, and expired ones show greyed with an "Expired" tag and unused time.
- Given a guest, when staff try to sell a plan, then the action asks to create or link a profile first.

## Spec Change Log

## Review Triage Log

Review 2026-10-27: no blocking findings. fmt, clippy, arena-core/store/api/replay/proto tests, hub check, check:strings and prettier pass.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test` in `apps/hub`.

## Auto Run Result

Status: done
Verified: fmt, clippy, hub check, string/token/contrast/protocol/layer checks and prettier pass; arena-core, arena-store, arena-api and arena-replay tests pass (proptest at 400 cases); `plan-sales.spec.ts` passes on desktop and phone. Unrelated timing tests fail in arena-tasks (`trusted_time_is_cloud_time_plus_monotonic_elapsed`, `a_printer_that_stops_reading_times_out`) and arena-cloud (`a_double_submitted_code_signs_in_once_and_counts_no_failure`, `unreachable_at_start_retries_then_exits_1_without_logging_the_token`), crates this story only touched for one match arm; `arena-kiosk-svc` test targets do not compile for unrelated unused imports, as before.
Left for later: spending packs and windows at session start (3.24, 3.27), expiry handling, receipts and invoice numbers (Epic 5), Owner-app views of owned plans.
