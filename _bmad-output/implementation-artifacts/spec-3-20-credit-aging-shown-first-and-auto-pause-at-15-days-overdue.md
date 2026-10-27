---
title: 'Story 3.20: Credit Aging Shown First, and Auto-Pause at 15+ Days Overdue'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'e0f92dc'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-18-enable-credit-for-a-player-and-settle-the-remainder-on-credi.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-19-collect-credit-with-partial-payments-and-undo.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Staff cannot see how long a player has owed (only the total), lookup hides the debt, and nothing stops credit for players who never pay. `credit_balances.credit_paused` exists (3.18) but nothing sets it.

**Approach:** Aging is derived from the ledger, never stored: `CreditAging {oldest_unpaid_day, oldest_age_days, bucket_0_7/8_14/15_plus_minor}` comes from the 3.19 oldest-first allocation of `wallet_txns` credit rows (spend age = business days between the spend's `business_day` and today). `WalletView.aging` carries it; `PlayerHit.outstanding_minor` puts the debt first in `player-lookup`. A branch rule `billing.credit_auto_pause` (Bool, default `true`, Settings → Billing) drives the system intent `credit.auto_pause {player_id?}`: with the rule on it sets `credit_paused` for debtors whose oldest unpaid spend is 15+ days old (and reopens those no longer overdue); with it off it reopens every paused player. The Hub submits it hourly while primary (`HousekeepingTask`) and, for one player, when that player is looked up or their profile is opened. A credit settle for a paused or overdue player is refused `CREDIT_PAUSED` (the once-a-day override does not lift it). A `credit.collect` that brings a paused player under 15 days (or clears the tab) sets `credit_paused = 0` in the same transaction and marks its credit ledger row `method = 'reopened'`; `credit.collect.reverted` re-pauses when it finds that mark, so the state is restored exactly.

## Boundaries & Constraints

**Always:**
- Aging is read from the ledger in FIFO order (insertion order of `wallet_txns`); no new table or column and no migration. Buckets 0–7, 8–14 and 15+ days; a partly paid spend keeps its age for the unpaid part. Business days come from the spend's stored `business_day` and the current business day, so the tz and day start are respected.
- `credit.auto_pause` is system-only (`Requirement::System`), takes no `base_rev`, writes only `credit_balances` updates, and is `STALE` when nothing needs to change (callers ignore it). The Writer's `credit_auto_pause(player_id?)` checks and submits inside one transaction, so a no-op writes nothing and creates no intent.
- Settle checks the rule inside its transaction: with the rule on, `paused` or an overdue oldest spend refuses a credit row `CREDIT_PAUSED` on `payments[i].method`, override or not. With the rule off a stale pause does not block.
- The pause never touches wallet payments, collections (a paused player can still pay) or running sessions.
- New intent goes into the Story 1.8 proptest and `arena-replay`. Money is integer minor units. No code comments (user rule).

**Never:**
- Reminders and the Overdue report (Epic 8), a stored aging table, a manual pause/unpause intent, per-player pause overrides.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Buckets | spends 16, 13 and 0 days old | 15+ = oldest, 8–14 = middle, 0–7 = newest; oldest day shown | No error |
| Partial collect | oldest spend part-paid | unpaid remainder keeps the oldest day's age | No error |
| Daily check | oldest unpaid 15+ days, rule on | `credit_paused = 1`; profile shows "Credit paused — 15+ days overdue" | No error |
| Not yet overdue | oldest unpaid 14 days | nothing written | `STALE` (ignored) |
| Paused settle | credit row for a paused player, override or not | nothing written | `CREDIT_PAUSED` |
| Overdue, not yet swept | rule on, 20 days, flag still 0 | nothing written | `CREDIT_PAUSED` |
| Collect under 15 | paused, collection leaves oldest < 15 days or 0 owed | `credit_paused = 0` in the same transaction | No error |
| Collect still overdue | paused, oldest still 15+ | stays paused | No error |
| Undo that collect | within window | outstanding, wallet and `credit_paused = 1` restored | No error |
| Rule off | any paused player | all reopened; none paused afterwards | No error |
| Lookup | hit with outstanding | "Owes ₹x" shown before name and phone | No error |

</intent-contract>

## Code Map

- `crates/arena-store/src/credit.rs`, `src/lib.rs` -- `fifo_aging`, `credit_aging(_with)`, `civil_day_number`, `list_credit_debtors`, `set_credit_paused`.
- `crates/arena-proto/src/{session,billing,players,error,intent,lib}.rs` -- `CreditAging`, `CreditAutoPause`, `CREDIT_OVERDUE_DAYS`, `WalletView.aging`, `PlayerHit.outstanding_minor`, `KEY_BILLING_CREDIT_AUTO_PAUSE`, `CREDIT_PAUSED`, intent kind.
- `crates/arena-core/src/apply/credit_pause.rs` (+ `mod`, `guards`, `validate`, `mutate`), `credit_collect.rs`, `session.rs`, `writer.rs`, `snapshot.rs`, `players.rs`, `settings_defaults.rs` -- apply logic, reopen/restore, settle refusal, Writer check-and-submit, wallet aging, lookup outstanding, setting rule.
- `crates/arena-api/src/ui_auth.rs`, `crates/arena-tasks/src/housekeeping.rs` -- lookup and profile trigger, hourly check; `crates/arena-cloud`, `arena-kiosk-core` -- error code mapping.
- `apps/hub/src/lib/players/{OutstandingSection,PlayerLookup,CreditBlock}.svelte`, `routes/players/+page.svelte`, `routes/settings/branch/billing/+page.svelte`, `lib/billing.ts`, `lib/audit/labels.ts`, `packages/ui/src/messages/en.json` -- UI.

## Tasks & Acceptance

**Execution:**
- [x] FIFO aging in the store with unit tests
- [x] Protocol types, error code, intent kind, setting key and rule, generated types
- [x] `credit.auto_pause` apply logic and Writer check-and-submit
- [x] Collect reopens and its undo re-pauses; settle refuses `CREDIT_PAUSED`
- [x] API: wallet aging, lookup outstanding, pause on lookup/profile; hourly check in housekeeping
- [x] Rust tests for the matrix, proptest op and replay entry
- [x] Hub: outstanding section first on the profile, lookup warning, Billing toggle, strings, audit label
- [x] Playwright specs for lookup, profile, paused state and the Billing toggle

**Acceptance Criteria:**
- Given a player with outstanding credit, when their row appears in `player-lookup`, then the outstanding amount shows first in warning text, before name and phone.
- Given the profile, when it opens, then an outstanding section comes first with the total, buckets 0–7, 8–14 and 15+ days from the oldest-first allocation, and the oldest unpaid date.
- Given an oldest unpaid credit 15+ days old and the rule on, when the check runs or the player is looked up, then `credit.auto_pause` sets `credit_paused`, the profile shows "Credit paused — 15+ days overdue" and new credit rows are refused `CREDIT_PAUSED` even with the override.
- Given a paused player, when a collection brings the oldest unpaid under 15 days or clears it, then credit reopens in the same transaction, and its Undo restores the paused state.
- Given the rule off, then no player is paused and players paused earlier are reopened.

## Spec Change Log

## Review Triage Log

Code review (2026-10-27): no blocking findings. cargo clippy (excluding arena-kiosk-svc tests, which fail on unrelated unused imports), core/store/api/proto/replay/proptest tests, hub svelte-check, check:strings and prettier pass. Two unrelated arena-tasks tests (heartbeat trusted time, LAN printer timeout) fail independently of this story.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test` in `apps/hub`.

## Auto Run Result

Status: done
Verified: see the commit message of this story.
Left for later: reminders on cycle days and the Overdue report use the same FIFO helper (Epic 8); pausing is evaluated per business day, so a player crossing 15 days is paused by the next hourly check or the next lookup, and a credit settle is refused in the meantime by the live check.
