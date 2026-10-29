---
title: 'Story 8.13: Today shows every branch with its figures, cash variance and data age'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '5a3abef'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-9-the-owner-read-api-serves-the-live-summary-and-rollup-ranges.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-12-on-free-the-owner-app-shows-only-subscription-pairing-and-su.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Today is a placeholder. The owner cannot see a branch's state, money and staff, nor how old those numbers are, and the cloud never learns whether the app's data age is within cadence.

**Approach:**
- Today draws one `card` per paid branch from `GET /v1/branches/{id}/live`: open/closed, sessions live, revenue, cash in drawer with the signed per-shift variance, staff on shift and last sync, each a `metric-figure` with its data age ("as of 2 min ago"). Over threshold the variance carries a warning edge and "Over threshold".
- Offline branch (`branch_online` false): muted figures, "Branch offline" badge, last values kept. Age past 30 min while the branch is open turns to warning ink with the word "late".
- "Needs attention" lists every branch's `attention` items, naming the branch, "All quiet." when none.
- Re-fetch at the tier cadence (Basic 15 min, Premium 5 min) while visible, by a Refresh button, and by pull-to-refresh; no reload and focus stays put.
- Each open reports `age_s` per branch to the cloud (`POST /v1/branches/{id}/data-age`), kept as the per-branch summary-age metric with a p99.
- The Hub summary lists recently counted shifts (with `variance_minor` and `closed_at`) after the open one, so the variance is known for every counted shift.

## Boundaries & Constraints

**Always:**
- Free branches stay locked (Story 8.12); no `live` call for them. A cached copy shows at once; a failed refresh keeps the figures.
- Money in integer minor units, formatted by the branch currency. Strings only from the catalogue.
- The data-age report holds the branch id (path), the age and the cloud's own tier; nothing else.

**Never:**
- Setup mode or `live_at` handling (Story 8.14), Reports/Alerts/Manage content, a new Hub endpoint, an opening-hours field in the summary.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Normal | online branch, summary 2 min old | card with seven figures, six with "as of 2 min ago" |
| Short count | last counted shift variance −2500 | "₹25.00 short", no flag |
| Over threshold | attention `shift_variance` for that shift | "+₹40.00 over", warning edge, "Over threshold" |
| Offline | `branch_online` false | muted, "Branch offline", last values, growing age |
| No summary | `as_of` null | "Waiting for the first update from this branch." |
| Stale open | `open` and age over 30 min | age in warning ink with "late"; closed branch: no warning |
| Free in mixed tenant | free + paid | Free: name and tier-lock, no call; paid: card |
| Failed refresh | network down | figures kept, no error shown |
| Report | open of Today | one `POST data-age` per branch per open |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/summary.rs` -- `SummaryShift.closed_at`; `src/owner_read.rs` -- `DataAgeReport`; generated TS.
- `crates/arena-store/src/summary.rs`, `crates/arena-core/src/summary.rs` -- counted shifts join `shifts`.
- `crates/arena-cloud/src/data_age.rs`, `src/api/owner_read.rs` -- `DataAgeMetrics` and `POST /v1/branches/{id}/data-age`.
- `apps/owner/src/lib/{today.svelte.ts,TodayBoard.svelte,TodayCard.svelte,MetricFigure.svelte,Section.svelte}`, `src/routes/+page.svelte`; strings `owner.today.*` in `packages/ui/src/messages/en.json`.
- Tests: `apps/owner/tests/today.spec.ts` (+ `stub.ts`, `shell.spec.ts`, `offline.spec.ts`), `crates/arena-cloud/tests/owner_read.rs`, `crates/arena-core/src/apply/tests/summary.rs`, `data_age.rs` unit tests.

## Tasks & Acceptance

**Execution:**
- [x] Hub summary carries counted shifts with signed variance and `closed_at`.
- [x] Cloud: `POST /v1/branches/{id}/data-age`, `DataAgeMetrics` (per branch, p99 over the last 200 opens, logged), tenant/entitlement/rate checks reused from the owner read API.
- [x] Owner app: Today board and cards, metric-figure, offline/late/over-threshold states, Needs attention, cadence + Refresh + pull-to-refresh, cached first render, age report.
- [x] Tests for each row of the matrix, cadence (fake clock), focus kept, report body, two columns/one column, a11y suite on Today in all themes.
- [ ] Deferred: Lighthouse CI run (3-year-old phone, throttled 4G, first render under 2 s) and the Grafana p99 dashboard -- no Lighthouse or exporter pipeline exists yet (the Story 2.3 exporter is deferred); the metric is kept and logged in process like the other Epic 8 metrics.

**Acceptance Criteria:**
- Given the Story 8.13 criteria in `epics.md`, when Today runs against a stubbed `/v1`, then each UI criterion is met and the suite passes; the cloud criterion is met by the data-age route and metric.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy -p arena-proto -p arena-cloud -p arena-core -p arena-store --all-targets -- -D warnings`; `cargo test -p arena-proto -p arena-store -p arena-cloud`; `cargo test -p arena-core --lib summary`; `pnpm --filter @arena/owner check`; `ARENA_PW_PORT=5495 pnpm --filter @arena/owner test`; `pnpm run check:strings check:layers check:protocol check:no-manifest format:check`; `pnpm run gen:types` then `check:generated` (clean once the new generated files are committed).

## Auto Run Result

Status: review (implemented and verified).

- Summary: Today shows a card per paid branch with live figures, signed shift variance, data ages, offline/late/over-threshold states and a cross-branch Needs attention list; it refreshes at the tier cadence, by button and by pull; opens report their data age to the cloud.
- Decisions: (1) The summary had no per-shift variance except as an over-threshold attention item, so the Hub now lists counted shifts (last 24 h, newest first) after the open shift; `staff_on_shift` is unchanged. (2) Over threshold = an `attention` item `shift_variance` whose ref is that shift, so the app needs no threshold. (3) The summary has no opening hours; "during opening hours" is approximated by the summary's own `open` flag (a shift is open or a session is running), so a closed, quiet branch never shows a late warning. (4) Attention items have no timestamp, so they are ordered by the branch heard from most recently (summary `as_of`), then the Hub's order. (5) Every paid branch gets a card regardless of `live`; setup mode is Story 8.14. (6) Figure age is measured from the summary's `as_of` (Hub clock) against the device clock, so a skewed Hub clock skews the age.
- Verification: fmt and clippy clean; arena-proto, arena-store, arena-cloud (incl. 14 owner_read tests) and arena-core summary tests pass except `arena-cloud` `tests/binary.rs` `unreachable_at_start_retries_then_exits_1_without_logging_the_token` and `tests/backup.rs` hung-run test, which fail on a clean checkout (noted in Story 8.9); owner app `check` 0 errors, Playwright 170 passed (desktop and phone, includes axe in light/dark/HC on Today); `check:strings`, `check:layers`, `check:protocol`, `check:no-manifest`, `format:check` ok. `test:scripts` fails on `missing pin row: zstd (Cargo.toml =0.13.1)`, unrelated and untouched.
- Residual risks: Lighthouse and Grafana not done (above); the p99 is per process; the 30 s tick means a re-fetch lands up to 30 s after the cadence.

## Code Review Result

Reviewed 2026-10-30 against the diff of 95a2043: no blocking findings. fmt, clippy, owner_read and summary tests, owner `check`, Playwright (170), `check:strings` and `format:check` pass. Non-blocking notes: the data-age p99 is per process; reports share the owner read rate bucket.
