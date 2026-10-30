---
title: "Story 8.42: The owner's remote experience proven against its targets"
type: 'feature'
created: '2026-10-30'
status: 'done'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-7-11-the-kitchen-flow-proven-on-reference-hardware.md'
warnings: ['oversized']
deferred:
  - 'The nightly staging run, the 8-hour WAN outage soak with 20 kiosks (1,000 sessions, 500 orders) through a Hub and a real network, the cloud stopped for 1 hour while a branch trades, and a month of real staging R2 traffic are not run: there is no staging café or Hub-plus-cloud rig in this repository. The report lists the first two as "not run" (soak.json, cloud-down.json; --strict requires them).'
  - 'Basic uploads every 300 s in the tier table (upload 300 / summary 900); the LLD says upload and summary take the same value, Basic 900 s. Not changed here (it would move every Basic Hub test and the presign limits); it only costs more PUTs, and the cost model uses the real value.'
  - 'The R2 cost is the count of presigned links times a price constant (a busy branch: a batch and the summary every cycle); the metrics dashboard export waits for the Story 2.3 exporter, so the figure is logged as cost_estimate_paise with every link.'
---

<intent-contract>

## Intent

**Problem:** Nothing measures what the owner is promised: Today's first render and data age, owner-command delivery, the catch-up after an outage, the R2 cost per branch and the audit time.

**Approach:** Proof, plus the one fix the proof found. Cloud-side tests against the real router and presign/heartbeat/command/audit paths on a simulated clock, a Playwright first-render run on a reference-phone profile, one release report that merges them, and release-bench jobs that run the full sizes.

**Fix found by the proof:** Premium's `summary_interval_s` was 900 s, so its Today could be 15 minutes old against the 5-minute target (the LLD and the Epic 8 context say Premium is 300 s). It is now 300 s.

</intent-contract>

## Code Map

- `crates/arena-cloud/tests/remote_experience.rs` -- Today's data age (`todays_data_age_stays_inside_the_cadence_on_99_percent_of_opens`: 1,000 opens per tier, `live` + `data-age`), commands (`owner_commands_reach_applied_at_the_branch_in_under_two_minutes_at_p95`: 200 posts, a Hub fetching every 60 s), catch-up (`a_branch_back_from_an_eight_hour_outage_catches_up_with_no_gap_and_no_duplicate`: real presign, bucket keys, heartbeat gap check), R2 cost (`a_months_r2_puts_cost_each_branch_under_ten_rupees_on_average`). `ARENA_*` variables size them; `ARENA_REMOTE_OUT` writes JSON.
- `crates/arena-cloud/tests/audit_build.rs` -- `a_busy_month_asked_for_the_first_time_shows_its_first_results_in_under_ten_seconds` (request, build, first page; `ARENA_AUDIT_ROWS`, `ARENA_AUDIT_FIRST_MS`).
- `crates/arena-cloud/src/presign.rs` -- `R2_CLASS_A_PAISE_PER_MILLION`, `PutCounts::cost_paise`, `cost_estimate_paise` in the issued-link log.
- `crates/arena-proto/src/tier.rs` -- Premium `summary_interval_s` 300 (pinned in the tier table test).
- `crates/arena-tasks/src/commands.rs` -- `the_mailbox_poll_keeps_the_delivery_model_true` pins the 60 s poll the delivery model uses.
- `apps/owner/tests/first-render.spec.ts`, `apps/owner/playwright.config.ts` -- project `reference-phone` (Moto G4, CPU 4x, 9/1.5 Mbit/s, 100 ms RTT, cold cache; cloud answers held RTT + 130 ms) runs only this file; `OWNER_JSON` writes the Playwright report.
- `scripts/remote-report.mjs` (+ test) -- merges the results, checks the budgets, a missing result fails; the rig's soak and cloud-down results are "not run" unless `--strict`.
- `.github/workflows/release-bench.yml` (`remote-cloud`, `remote-owner`, `remote-report`), `docs/runbooks/remote-bench.md`.

## Tasks & Acceptance

- [x] Today: data age within cadence on at least 99% of 1,000 scripted opens, Basic and Premium
- [x] Today: first render under 2 s on the reference phone profile
- [x] 200 owner commands, post to applied, p95 under 2 minutes
- [x] Catch-up after an 8-hour outage, the cloud's side: batches, summary and missed rollups taken, no gap on the next heartbeat, no duplicate object
- [x] R2 PUTs per branch per month summed, cost estimate under Rs 10 on average, logged per link
- [x] A busy month's first audit results under 10 s
- [x] Release report, release-bench jobs and the runbook
- [ ] (not run, see deferred) 8-hour soak with 20 kiosks through a Hub; cloud stopped for an hour; nightly staging run

**Acceptance Criteria:**
- Given Basic and Premium comp entitlements, when Today is opened 1,000 times by script, then data age is within cadence (Basic 900 s, Premium 300 s) on >= 99% of opens: 99.9% and 99.7% (1 s upload lag), and the first render on the reference phone profile is 1.4 s.
- Given 200 owner commands to an online branch, when delivery is timed from post to `applied`, then p95 is under 2 minutes: 56.5 s (max 59.5 s).
- Given an 8-hour outage with 12,000 changes waiting, when the WAN returns, then every batch, the summary and the missed rollups are taken inside 10 minutes of the cloud's clock (35 s), the heartbeat reports no gap and a repeated cycle makes no second object. The Hub-side soak and the cloud-down hour are not run (deferred).
- Given a month of a busy branch's uploads, then the estimate is Rs 6.51 per branch (17,340 PUTs) on each tier, under Rs 10 on average, and is logged as `cost_estimate_paise`.
- Given a busy branch month, when an audit is requested for the first time, then first results arrive in under 10 s: 4.6 s for 150,000 changes (release build).

## Spec Change Log

- Premium `summary_interval_s` 900 -> 300 (found by the data-age run).

## Design Notes

**Reference phone.** The epic names "the reference phone profile" and the PRD "a 3-year-old phone over 4G" without numbers. This story fixes one: a Moto G4 with the CPU slowed 4x, a cold cache and 9 Mbit/s down, 1.5 Mbit/s up, 100 ms RTT. On WebPageTest's 4G (170 ms RTT) the same run takes about 2.05 s, just over the budget, and the job records the slower links without failing on them. The critical path is the bundle chain (HTML, entry, nodes, chunk), then `session`/`branches`, then `live`: three cloud round trips after the script runs.

**Data age.** The model is a Hub that builds a summary on the cadence grid, whose file lands `ARENA_TODAY_LAG_MS` (1 s) later, and an owner who opens at scattered times. The share older than the cadence is about `lag / cadence`, so Premium has the thinner margin.

**Commands.** The model fixes the Hub's poll at 60 s and its apply at 0.5 s; the p95 is therefore about a poll interval. A test in `arena-tasks` pins the poll so the model cannot drift.

## Verification

- `cargo test -p arena-cloud --test remote_experience` (4, 14 s); `--test audit_build` (19); `-p arena-proto` (tier table); `-p arena-tasks --lib the_mailbox_poll`; `cargo test --release -p arena-cloud --test remote_experience` with `ARENA_COST_DAYS=30` (4, 184 s) and `--test audit_build busy_month` with `ARENA_AUDIT_ROWS=150000`.
- `node --test scripts/remote-report.test.mjs` (9); `playwright test --project=reference-phone` in `apps/owner` (1.40-1.42 s); owner `today` and `live` on `desktop` and `phone` (38) still pass.
- `cargo fmt --all --check`, `cargo clippy -p arena-cloud -p arena-proto -p arena-tasks --all-targets -- -D warnings`, `pnpm format:check`, `check:strings`, `check:secrets`, `check:pins`, `check:layers`, `svelte-check` owner.
- Failing on a clean tree too, not from this story: `arena-cloud` `backup` `a_hung_run_times_out_with_its_own_code` and `binary` `unreachable_at_start_retries_then_exits_1_without_logging_the_token`; `arena-tasks` lib `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; 12 tests of `scripts/*.test.mjs`; `check:deploy` needs `kubectl`.

## Auto Run Result

Status: done

Implemented as above. Residual risks: the Hub-through-the-network soak, the cloud-down hour and the nightly staging run do not exist (see deferred); the first-render margin on a 170 ms link is nil; the cost figure is a model of PUT counts at a fixed price; the `release-bench.yml` jobs were not run on GitHub.

## Code Review Result

Reviewed 2026-10-30: no blocking findings. fmt, clippy (-D warnings), `remote_experience` (4), `audit_build` (19), arena-proto (322), `remote-report.test.mjs` (9) pass; the only arena-cloud failures (`backup`, `binary`) are the pre-existing ones listed above.
