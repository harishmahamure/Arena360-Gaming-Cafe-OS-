---
title: 'Story 3.24: Start a Prepaid Session on a Pack or the Wallet'
type: 'feature'
created: '2026-10-27'
status: 'in-review'
baseline_revision: '7afb346b5d2d6233d2516bcb435f03d459a32710'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-23-session-timer-engine-with-time-warnings.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-22-sell-a-pack-or-pass-at-the-counter-and-see-owned-plans-on-th.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Only postpaid sessions can start. A player who bought a pack (3.22) or holds a wallet balance (3.16) cannot play on it: no session gets an `ends_at`, so the timer engine (3.23) has nothing to count down, and nothing deducts the pack or debits the wallet at the end.

**Approach:** `session.start` accepts `mode: pack` (with `pack_id`) and `mode: wallet`, both needing a profiled `player_id`. A pack session gets `ends_at` = start + minutes left. A wallet session gets `ends_at` = the last whole-rounding-unit moment at which the postpaid bill formula (`billing::bill`: minimum on the first segment, rounding on the last, half-up on the amount) still costs no more than the player's available balance (balance minus the `reserved_minor` of the player's other live wallet sessions); that cost is stored as the session's `reserved_minor` (new column). Every wallet spend that checks the balance (`load_payer` for settle and plan sale, and credit collect) sees balance minus reserved, so it is refused `INSUFFICIENT_BALANCE` when it would cut into a reservation. Ending a pack or wallet session, by staff `session.end` or by `session.expire`, runs one shared prepaid close-out that writes the order and settles it at once: pack minutes used (unpaused time rounded up to whole minutes, capped at minutes left) come off `player_packs.minutes_left` and the order is a settled zero bill; a wallet session writes the bill lines, one wallet payment, a `bill` wallet txn and the wallet debit (capped at the balance so it never goes below zero). The Hub tile sheet's start area lists, for a picked profiled player, Pay later, each owned pack valid for the device's group (hours left, expiry; expired ones greyed and not selectable) and Wallet (balance, hourly rate).

## Boundaries & Constraints

**Always:**
- Money is integer minor units. `session.start` takes no `base_rev`. Pack and wallet starts need `player_id` (a guest is `VALIDATION_FAILED` on `player_id`). A pack session needs no group rate (segment rate 0, `rate_id` null); wallet and postpaid still need one (`NO_RATE`).
- Pack checks, in order: owned by the player and kind `pack` (`NOT_FOUND` on `pack_id`); group not in the pack's `group_ids` -> `PLAN_NOT_FOR_GROUP`; `expires_at <= now` or no minutes left -> `PACK_EXPIRED`; a pack already on a live session -> `VALIDATION_FAILED` on `pack_id`. All carry an inline reason.
- Wallet below the cost of the first (minimum) bill -> `INSUFFICIENT_BALANCE` on `player_id`, message "Balance {amount}. Top up at counter." naming the rate and minimum, with money formatted the way existing core messages do.
- Prepaid close-out is shared by `session.end` and `session.expire`; staff end is on the session `rev` as before, expiry still has no `base_rev`. The order is `kind='session'`, `settled_by` is the actor (`hub` on expiry), no unpaid state, no Undo (`session.end.reverted` already refuses non-postpaid; End reports `irreversible: true` so the Undo toast is not offered).
- New codes `PACK_EXPIRED` and `PLAN_NOT_FOR_GROUP` go wherever `PASS_ALREADY_ACTIVE` is registered. Migration `0035_prepaid_sessions.sql` adds `sessions.reserved_minor INTEGER NOT NULL DEFAULT 0`. `session.start` with the new fields joins the Story 1.8 proptest generator and `arena-replay`. No code comments beyond doc comments in the existing style (user rule).

**Never:**
- Pass and block sessions (3.27), grace and clean-up at time-up (3.26), alerts (3.25), extend (3.28), pausing a prepaid session, kiosk-originated starts (Epic 6), receipts or invoice numbers (Epic 5), changing the timer engine.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pack start | owned pack, group allowed, 90 min left | session `mode=pack`, `pack_id`, `ends_at` = now + 90 min | No error |
| Wallet start | balance 10000, rate 6000/h, rounding 15, minimum 30 | `ends_at` = now + 90 min (100 min of balance, floored to the 15 min unit); `reserved_minor` 9000 | No error |
| Wallet short | balance below the minimum bill | nothing written | `INSUFFICIENT_BALANCE`, "Balance ... Top up at counter." |
| Other group / expired / empty pack | pack not for the group; `expires_at` passed; 0 minutes | nothing written | `PLAN_NOT_FOR_GROUP` / `PACK_EXPIRED` |
| Pack busy, other player's pack, guest | pack on a live session; foreign pack; no `player_id` | nothing written | `VALIDATION_FAILED` / `NOT_FOUND` / `VALIDATION_FAILED` |
| Second wallet session | balance 10000, first reserved 6000 | second sized on 4000 | `INSUFFICIENT_BALANCE` if under the minimum |
| End a pack session at 12m10s | 90 left | `minutes_left` 77, settled zero bill, device freed | No error |
| End or expire a wallet session | 41 min used | bill by the formula, wallet debited by it, payment + txn rows, order settled | Debit capped at the balance |
| Concurrent spend | wallet payment over balance minus reserved | nothing written | `INSUFFICIENT_BALANCE` |
| Expire a pack session at `ends_at` | `minutes_left` goes to 0 | same close-out as staff end, `ended_by='hub'` | `STALE` if already ended |
| Undo of a prepaid End | `session.end.reverted` | nothing written | `UNDO_NOT_ALLOWED` |

</intent-contract>

## Code Map

- `crates/arena-proto`: `session.rs` (`SessionStart.pack_id` optional, `EndOutcome`), `error.rs` (`PackExpired`, `PlanNotForGroup`, both places `PassAlreadyActive` appears), `crates/arena-api/src/ui_auth.rs` (~L868 refusal mapping, ~L1982 start online check, `end_outcome` ~L2253), `crates/arena-cloud/src/api/error.rs`, `crates/arena-kiosk-core/src/queue.rs`.
- `crates/arena-store`: `migrations/0035_prepaid_sessions.sql`, `sessions.rs` (`NewSession` gains `pack_id`, `ends_at`, `reserved_minor`, optional `rate_id`; `LiveSession.pack_id`/`reserved_minor`; a query for the sum reserved by a player's live wallet sessions), `player_packs.rs` (`find_player_pack`, `deduct_pack_minutes`, pack-in-use check), `migrations.rs`.
- `crates/arena-core`: `apply/session.rs` (`load`/`validate`/`mutate` for start, `validate_end`/`mutate_end` branch for prepaid, `load_payer` subtracts reserved, `insert_payments`/`record_payer_txns` reusable), new `apply/prepaid.rs` (pack and wallet start checks, close-out), `apply/session_expire.rs` (call the close-out), `apply/credit_collect.rs` (~L86 balance check), `billing.rs` (wallet horizon from the same formula as `bill`), `apply/{mod,validate,mutate}.rs`, `snapshot.rs`, `session_screen.rs` (`timed_sessions` already tracks any `ends_at`).
- Tests: `apply/tests/` (new `prepaid.rs`), `billing.rs` unit tests, `crates/arena-core/tests/proptest_apply.rs`, `crates/arena-replay`, `crates/arena-api` tests.
- Hub: `apps/hub/src/lib/tile-actions.svelte.ts` (`startJob` mode and `pack_id`), `lib/TileSheet.svelte` (start area ~L439, `device.group_id`), new `lib/players/StartOptions.svelte` (reads `loadWallet`, `OwnedPlanView`, `termsText`/`timeText` in `lib/plans.ts`), `packages/ui/src/messages/en.json`, `packages/ui/src/generated` (`pnpm run gen:types`), `apps/hub/tests/prepaid-start.spec.ts` modeled on `session-start.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol and error codes, migration, store queries and `NewSession`/`LiveSession` fields, generated types
- [x] `billing::wallet_horizon` (largest whole-unit minutes whose bill fits an amount, plus its cost), unit tests including rounding 0/1/15 and the half-up edge
- [x] Start validation and mutation for pack and wallet; `load_payer` and credit collect subtract reserved
- [x] Shared prepaid close-out used by `session.end` and `session.expire`; End outcome marked irreversible
- [x] Apply tests for every matrix row, proptest op and replay coverage, API outcome test
- [x] Hub `StartOptions`, `startJob` with mode, catalogue keys, Playwright spec on desktop and phone

**Acceptance Criteria:**
- Given a free PC and a profiled player, when the start area shows, then it lists Pay later, each unexpired owned pack allowed for the device's group (hours left, expiry) and Wallet (balance, hourly rate); expired packs are greyed and cannot be selected, and packs of other groups are not listed.
- Given a pack is chosen, when `session.start{mode: pack}` applies, then `ends_at` = start + minutes left, the timer engine tracks it and the kiosk unlocks counting down to it.
- Given Wallet is chosen, when `session.start{mode: wallet}` applies, then `ends_at` is where the postpaid bill formula reaches the available balance, and a balance below the minimum bill is refused with "Balance ... Top up at counter." naming the rate.
- Given a pack of another group or an expired pack, when start is attempted, then it is refused `PLAN_NOT_FOR_GROUP` or `PACK_EXPIRED` and the sheet shows the reason inline.
- Given a pack or wallet session ends by time-up or staff, when it applies, then used minutes are deducted from the pack, or the wallet is debited by the bill (never below zero), and the bill is settled with no unpaid state.
- Given a concurrent wallet spend that would leave less than a running session's reserved amount, when it reaches `apply()`, then it is rejected `INSUFFICIENT_BALANCE`.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test prepaid-start session-start session-end session-timer` in `apps/hub`.

## Auto Run Result

Status: in-review
Verified: fmt, clippy, hub and kiosk check, string/token/contrast/protocol/layer checks and prettier pass; arena-core, arena-store, arena-proto, arena-api, arena-replay and arena-kiosk-core tests pass (apply matrix in `apply/tests/prepaid.rs`, proptest `PrepaidStart`, API outcome test); `prepaid-start`, `session-start` Playwright specs pass on desktop and phone. Unrelated timing failures as before: arena-cloud (`a_hung_run_times_out_with_its_own_code`, `unreachable_at_start_retries_then_exits_1_without_logging_the_token`), arena-tasks (`trusted_time_is_cloud_time_plus_monotonic_elapsed`, `a_printer_that_stops_reads_times_out`). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: a wallet debit short of the bill is taken off the last order line so lines, total and payment agree; a wallet needs at least one rounding unit of balance; a wallet top-up Undo is refused when it would cut into a running reservation; a prepaid close-out emits no `bill.settled` event (Epic 5 hook).
Left for later: pass and block starts (3.27), grace at time-up (3.26), extend (3.28).
