---
title: 'Story 3.31: A Rule Change Mid-Session Opens a New Rate Segment'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'd5c27c27acf0b9a0d817d385f44782442219cb2f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-30-the-active-rule-prices-new-sessions-and-shows-in-the-header.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-26-time-up-on-pcs-grace-then-clean-up-or-staff-handles.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** A session prices at the rule in effect when it starts (3.30). When a rule begins, ends or is edited while a postpaid or wallet session runs, the session keeps its old rate, so the player pays something other than the price on the board.

**Approach:** A system-only `session.rate_flip` intent closes the open segment and opens a new one at the resolved rate and `rule_id` from the boundary instant. The Hub timer task fires it when a boundary passes, when a rule is written, and after a restart (missed boundaries are applied in order at their true time). A wallet flip recomputes `ends_at` and `reserved_minor` from the reserved balance. `bill-breakdown` lines name the rule.

## Boundaries & Constraints

**Always:**
- Only a postpaid (active or paused) or wallet (active) session flips, and only for its own group's active rules; the base is the session's own `rate_id` row, so editing the group's base rate never reprices a running session.
- `pricing::due_flip` is the one rule for what flips and when: the first boundary in `(open segment from_at, now]` at which the winning rule differs from the segment's `rule_id`; otherwise `now` when the resolved rule or rate differs (an edit or deactivation); otherwise none. The task and `apply()` both use it, so they cannot disagree.
- A wallet session flips only before its own end (`ends_at` less `extended_min`); the new end is the latest whole-rounding-unit moment whose bill stays within `reserved_minor`, never below the flip instant, plus the minutes already extended.
- Paused time at a boundary stays split: the closed segment takes the pause up to the boundary, the new one only the pause after it; `paused_total_ms` and `ends_at` still move by the whole pause at resume.
- No code comments beyond doc comments (user rule).

**Never:**
- Flips on pack, pass or block sessions (plan-priced), kiosk-side repricing, transfer (3.32), group bills (3.33).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| Rule begins | postpaid session, rule starts at 16:00 | segment closes at 16:00:00, next opens at the rule rate with its `rule_id` | none due → `STALE` |
| Rule ends | session in the rule | new segment at the base rate, `rule_id` null | same |
| Rule edited | active rule's adjustment changes | flip at the apply time, same `rule_id`, new rate | same |
| Missed boundaries | Hub down across begin and end | two flips in order, each at its true time | none |
| Paused across boundary | paused 15:50, boundary 16:00, resume 16:20 | both segments exclude their own pause; bill matches golden | none |
| Wallet, rate drops | reserve fits more time | `ends_at` later, `reserved_minor` reduced | none |
| Wallet, rate rises | reserve fits less time | `ends_at` earlier, never before the flip instant; wallet never below zero | none |
| Past own end | wallet session in extended time or grace | no flip | not due |

</intent-contract>

## Code Map

- `crates/arena-core/src/pricing.rs`: `due_flip`, `boundaries`, `winner_at`, `adjusted`; tests in `pricing/tests.rs`.
- `crates/arena-core/src/billing.rs`: `bill` (`open_pause` counts from the last segment's start), new `wallet_end_after_flip`.
- `crates/arena-core/src/apply/rate_flip.rs` (new): `pending`, `load`, `validate`, `mutate`; wired in `apply/{mod,validate,mutate,guards}.rs` like `session_expire.rs`; tests in `apply/tests/rate_flip.rs`.
- `crates/arena-store/src/{sessions,devices}.rs`: `find_rate`, `fold_pause` (segment pause from `max(paused_at, from_at)`), `add_segment_pause`, `reprice_wallet_session`; `OrderLineRow.rule_id`.
- `crates/arena-proto/src/{intent,session,lib}.rs`: `SessionRateFlip`, `IntentKind::SessionRateFlip` (System), `BillLine.rule_name`; generated types; `en.json` audit label; `apps/hub/src/lib/audit/labels.ts`.
- `crates/arena-core/src/snapshot.rs`: `bills` names the rule; `snapshot` reader `rate_flips(now)`.
- `crates/arena-tasks/src/session_timer.rs`: rescans on boundary, `sessions` or `pricing_rules` writes and resync, submits flips.
- `packages/ui/src/components/BillBreakdown.svelte`, `messages/en.json` (`hub.bill.line_rule`); `crates/arena-core/tests/proptest_apply.rs` (`RateFlip` op).

## Tasks & Acceptance

**Execution:**
- [x] `pricing.rs` -- `due_flip` and golden tests (begin, end, edit, missed, wrap midnight, no rule)
- [x] `billing.rs` -- `bill` open pause from the last segment's start; `wallet_end_after_flip` with tests
- [x] store + proto -- `find_rate`, pause split, wallet reprice, `session.rate_flip` intent, `BillLine.rule_name`, `OrderLineRow.rule_id`; `gen:types`
- [x] `apply/rate_flip.rs` -- load/validate/mutate (postpaid, paused, wallet); register the intent; proptest op
- [x] timer task -- scan and submit flips; restart and missed-boundary tests
- [x] snapshot and UI -- rule name on lines, string, Playwright check

**Acceptance Criteria:**
- Given a running session when a rule begins or ends for its group, then the open segment closes at the boundary and a new one opens at the new rate recording the `rule_id`.
- Given a session that crossed a boundary, then its bill shows one line per segment ("11 min × ₹80/hr (Happy hour)") and golden totals apply the minimum to the first segment and rounding to the last.
- Given a wallet session when the rate changes, then `ends_at` and `reserved_minor` are recomputed so the wallet never goes below zero and kiosks get the new lock time from the next session screen push.
- Given a paused session when a boundary passes, then on resume both segments exclude their own paused time.
- Given the Hub restarts, then missed boundaries are applied at their true times, in order.

## Spec Change Log

## Review Triage Log

Code review: no blocking findings. fmt, clippy, tests (only the known unrelated arena-cloud and arena-tasks failures) and the repo checks pass.

## Design Notes

`wallet_end_after_flip`: closed segments plus a new segment of `k` whole minutes from the flip; the largest `k` whose `bill` total is within the reserve, rounded down so the total minutes are a whole rounding unit; ends at `flip + k min`.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test rule-chip tile-sheet` in `apps/hub`.

## Auto Run Result

Status: done
Verified: fmt, clippy, workspace tests (arena-core 615 incl. 13 `rate_flip` apply tests, `due_flip` and `wallet_end_after_flip` goldens, 4 timer-task tests, proptest `RateFlip` op), hub and kiosk check, string/token/contrast/protocol/layer checks, prettier and `gen:types` pass; `rule-chip` and `tile-sheet` Playwright specs pass on desktop and phone (56). Known unrelated failures as before: arena-cloud (`a_hung_run_times_out_with_its_own_code`, `unreachable_at_start_retries_then_exits_1_without_logging_the_token`) and arena-tasks (`trusted_time_is_cloud_time_plus_monotonic_elapsed`, `a_printer_that_stops_reading_times_out`). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: a postpaid flip writes only `session_segments` (no `sessions.rev` bump, so a staff End with `base_rev` is not made stale); a wallet flip also rewrites `sessions` (`ends_at`, `reserved_minor`); the Hub UI reloads on `session_segments` writes so timer-driven flips show; no closing-time cap on a lengthened wallet end; a wallet rise with no reserve ends the session at the flip instant.
Left for later: kiosk lock-time push relies on the existing periodic session tick (not changed or tested); transfer (3.32) and group bills (3.33).
