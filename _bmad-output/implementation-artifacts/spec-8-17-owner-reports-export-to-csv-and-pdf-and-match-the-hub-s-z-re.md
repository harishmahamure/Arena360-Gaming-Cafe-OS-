---
title: 'Story 8.17: Owner reports export to CSV and PDF and match the Hub''s Z-reports exactly'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: 'b120d2f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-16-reports-stock.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner can read Revenue, Cash reconciliation and Stock on screen but cannot take them away, and nothing proves the owner's sums equal the branch's own Z-reports.

**Approach:** `GET /v1/branches/{id|all}/export?from&to&report=revenue|cash|stock` streams a CSV built by `arena_proto::owner_csv` from the same `OwnerRange` answers `summary` uses (one row per day per branch, major units with the currency code, a header naming report, range, branches and generated time). Reports gets Export CSV / Export PDF on each report tab and an Exports tab that lists them for the current range; PDF is the browser's print-to-PDF of a print-styled view whose header names branch, range, generated time and data age. A 30-day synthetic Hub test sums the rollups the way the owner read API does and compares them to the Z-reports.

## Boundaries & Constraints

**Always:**
- The export reads the same rollups through the same sums as `summary`; the cloud holds no copy of branch rows.
- Currencies are never mixed in a row; the currency code sits in every row.
- Free and other-café branches are refused exactly as `summary` refuses them; the app exports only the branches it shows (live, paid).
- Text cells that start with `=`, `+`, `-`, `@`, tab or return are prefixed with `'` (numbers are left alone).
- Strings only from the catalogue.

**Never:**
- A server-rendered PDF, a change to the rollup schema number, an export of audit or credit.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Revenue CSV | range with 2 rollup days and 1 missing day | header (Report, Range, Branches, Generated), then a row per day per branch; the missing day reads "No figures" |
| Cash CSV | days with shifts | shifts, float, expected, counted, variance, over threshold per day |
| Stock CSV | levels, waste, lots | Level / Waste / Expiring rows per branch |
| All branches | several live paid branches | `/branches/all/export?...&branches=<ids shown>`; setup branches are left out |
| Free / foreign branch | any | 403 `FEATURE_NOT_ENTITLED` / 404 |
| Unknown or missing `report` | any | 422 on `report` |
| Failed download | cloud unreachable or 5xx | "Couldn't prepare the export…" alert, nothing saved |
| Export PDF | any report tab or the Exports tab | the report is shown and `window.print()` runs; print media hides nav and controls and shows the header |
| 30 synthetic days | sales, undone sales, refunds, shorts, 30 closes | summed rollups equal summed Z-reports: revenue, by method, by type, refunds, voids, cash expected/counted/variance |

</intent-contract>

## Code Map

- `crates/arena-proto/src/owner_export.rs` -- `ExportReport`, `owner_csv`, `major`, `utc_minute`.
- `crates/arena-cloud/src/api/owner_read.rs` -- `export` route (`report`, optional `branches` narrowing of `all`).
- `crates/arena-core/src/apply/tests/rollup.rs` -- `thirty_days_of_rollups_sum_to_the_thirty_z_reports`.
- `apps/owner/src/lib/{api.ts,reports.ts,ReportsBoard.svelte,ExportsPanel.svelte}`, `apps/owner/src/routes/+layout.svelte` (print styles).
- `packages/ui/src/messages/en.json` (`owner.reports.export*`, `owner.reports.exports.*`, `owner.reports.print.*`).
- Tests: `apps/owner/tests/{reports-export.spec.ts,stub.ts}`, `crates/arena-cloud/tests/owner_read.rs`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- CSV builder with major units, currency code, header and formula guard
- [x] `crates/arena-cloud` -- `export` route with the same entitlement and tenant checks as `summary`
- [x] `apps/owner` -- Export CSV / PDF on each report tab, Exports tab, print header and print styles, download with session refresh
- [x] `packages/ui` -- strings
- [x] NFR10: 30 synthetic business days at a Hub, summed rollups equal the Z-reports to the minor unit
- [x] Tests for each matrix row, axe on the Exports tab in light/dark/HC

**Acceptance Criteria:**
- Given the Story 8.17 criteria in `epics.md`, when the owner exports against a stubbed `/v1` and the cloud, then each is met and the suites pass.

## Spec Change Log

## Review Triage Log

## Design Notes

See the Story 8.17 entry in `epic-8-context.md`.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-cloud --all-targets -- -D warnings`; `cargo test -p arena-proto -p arena-store -p arena-core`; `cargo test -p arena-cloud --test owner_read --test branches`; `pnpm --filter @arena/owner check`; `pnpm --filter @arena/owner test`; `pnpm run check:strings check:layers check:protocol check:no-manifest format:check`

## Auto Run Result

Status: done.

- Summary: Export CSV/PDF for Revenue, Cash reconciliation and Stock, an Exports tab, and the 30-day proof that summed rollups equal the Hub's Z-reports.
- Verification: fmt and clippy clean; arena-proto (276), arena-store and arena-core (1281) tests pass; cloud `owner_read` (19) and `branches` pass; owner app `check` 0 errors and Playwright 346 passed; `check:strings`, `check:layers`, `check:protocol`, `check:no-manifest`, `check:hex`, `format:check` ok. `check:tokens-used` reports `--ad-reserved` in `apps/hub/.../floor/+page.svelte`, which this story does not touch.
- Residual risks: the NFR10 test builds the rollups at an in-process Hub and sums them with the same `OwnerRangeTotals::add_rollup` the cloud uses; it does not go through MinIO, so the CI job that uploads to MinIO is not added. The PDF is the browser's print-to-PDF, not a generated file, so it is not covered outside the print-media and `window.print` checks.

## Code review (2026-10-30)

Four layers (blind, edge-case, verification-gap, acceptance). Fixed: export forces `group_by=day`; revenue `missing_days` no longer overwrites real rows; empty `branches=` entries ignored; Cash CSV keeps rollup days that have no closed shift; PDF export no longer prints a failed report, guards re-entry, and waits for the refreshed "Generated" time; added unit tests for stock CSV alignment and multi-shift/rollup-only cash days. Deferred (non-blocking): server CSV text is English-only literals; the PDF "Generated" zone label; export-failed alert is not cleared on tab change; download() 401 retry has no dedicated test.
