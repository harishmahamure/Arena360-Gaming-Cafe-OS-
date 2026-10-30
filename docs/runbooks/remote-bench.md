# Owner remote-experience bench (Story 8.42)

The per-release proof that what the owner is promised is what the owner gets: Today's speed and
data age, command delivery, the catch-up after an outage, the R2 cost per branch and the audit
time. It is part of `.github/workflows/release-bench.yml` (manual run and release tags). The last
job, `remote-report`, merges every result with `scripts/remote-report.mjs` and fails the release
when a budget is missed **or a result is missing**.

## What is measured

| Measure                                    | Budget                                          | Where it comes from                                                                                                   |
| ------------------------------------------ | ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Today first render, reference phone        | < 2 s                                           | `apps/owner/tests/first-render.spec.ts`, Playwright project `reference-phone` (profile below)                          |
| Today data age within cadence              | ≥ 99% of 1,000 opens, Basic (≤ 15 min) and Premium (≤ 5 min) | `todays_data_age_stays_inside_the_cadence_on_99_percent_of_opens`                                   |
| Owner command, post to applied             | p95 < 2 min over 200 commands                   | `owner_commands_reach_applied_at_the_branch_in_under_two_minutes_at_p95`                                              |
| Catch-up after an 8-hour outage, cloud side | ≤ 10 min, no gap, no duplicate                 | `a_branch_back_from_an_eight_hour_outage_catches_up_with_no_gap_and_no_duplicate`                                      |
| R2 PUTs and cost per branch per month      | average < Rs 10, published                      | `a_months_r2_puts_cost_each_branch_under_ten_rupees_on_average` (`ARENA_COST_DAYS=30` on the rig)                      |
| Audit first results, busy month            | < 10 s                                          | `a_busy_month_asked_for_the_first_time_shows_its_first_results_in_under_ten_seconds` (`ARENA_AUDIT_ROWS=150000` on the rig) |

The `arena-cloud` tests run against the real router, the real presign, heartbeat, command and audit
paths and an in-memory data bucket, on a clock the test moves. They also run, smaller, in the
ordinary suite; the `remote-cloud` job runs the full sizes with `--release`. Every `ARENA_*`
variable named in `crates/arena-cloud/tests/remote_experience.rs` moves a size or a model input
(`ARENA_TODAY_OPENS`, `ARENA_TODAY_LAG_MS`, `ARENA_COMMANDS`, `ARENA_COMMAND_POLL_S`,
`ARENA_OUTAGE_HOURS`, `ARENA_OUTAGE_ROWS`, `ARENA_COST_DAYS`, `ARENA_R2_PUT_PAISE_PER_MILLION`);
`ARENA_REMOTE_OUT=<folder>` writes each result as JSON. The report accepts a run only at the story's
sizes (1,000 opens, 200 commands, 8 hours).

### Models the numbers rest on

- **Data age.** The Hub builds its summary every `summary_interval_s` and the file lands
  `ARENA_TODAY_LAG_MS` (1 s) later; the owner opens at scattered times. About `lag / cadence` of opens
  are older than the cadence, so Premium (300 s) sits near 99.7%. A slower upload shows up here.
- **Commands.** The Hub fetches its mailbox every 60 s (`COMMAND_POLL_INTERVAL` in `arena-tasks`) and
  applies a command in 0.5 s; posts arrive about one every three seconds. The model's p95 is about
  one poll interval. The Hub's own poll interval is pinned by an `arena-tasks` test so the model and
  the Hub cannot drift apart.
- **R2 cost.** A busy branch: every upload cycle sends a batch and the summary, every day a rollup
  and a snapshot. A link issued stands for a PUT. The price is `R2_CLASS_A_PAISE_PER_MILLION`
  (`crates/arena-cloud/src/presign.rs`, 4.50 US dollars a million at about Rs 83); the same figure
  is logged with every issued link as `cost_estimate_paise`, which is how the per-branch cost reaches
  the metrics dashboard once the Story 2.3 exporter ships.
- **Reference phone.** A Moto G4 with the CPU slowed 4x, a cold cache and a link of 9 Mbit/s down,
  1.5 Mbit/s up and 100 ms round trip; each cloud answer is held for the round trip plus 130 ms in
  the cloud. The job also runs a slower link (1.6 Mbit/s, 170 ms) for the record and does not fail on
  it. `vite preview` sends no compression where Cloudflare sends brotli, so the figure is, if
  anything, pessimistic on bytes.

## Not proven here

Two of the story's checks need a Hub with 20 kiosks and a real network that is taken away; nothing
in this repository runs them yet, and the report lists them as **not run** (`soak.json` and
`cloud-down.json`; `--strict` makes them required):

- an 8-hour WAN outage soak with 20 kiosks (1,000 sessions, 500 orders), then the Hub catching up
  within 10 minutes with no gap and no duplicate send. The cloud's side is the catch-up test above;
  the Hub's side is covered per piece by the `arena-tasks` upload tests and the 8-hour WAN-down
  soak (`scripts/wan-down/soak.sh`, Story 1.22), which does not talk to a cloud;
- the cloud stopped for an hour while the branch trades, then catching up.

The R2 cost is a model of the PUT counts, not a month of staging traffic, and the nightly staging
run of the story (an internal café on Basic and Premium comp entitlements) does not exist yet.
