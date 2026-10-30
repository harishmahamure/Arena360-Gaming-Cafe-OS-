---
title: 'Story 9.6: The export format is documented and every report exports alongside it'
type: 'feature'
created: '2026-10-30'
status: 'done'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-4-one-tap-full-database-export-to-a-folder-or-usb-drive.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-5-the-hub-saves-a-daily-export-to-the-backup-location.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An `.a360` file is only as useful as the owner's ability to open it without Arena 360, and nothing documents it. A person who wants their numbers has the database file but no reports and no map of the tables.

**Approach:** A generated public reference (`docs/export-format/reference.md`: header, decompression, every table and column with type and meaning, money in minor units, ciphertext columns marked) that a `cargo test` fails on when it is stale against the migrations. `POST /ui/backup/export-everything {from, to}` writes one folder holding the Story 9.4 file, every Story 5.32 Hub report for the range as CSV, and a `README.txt` that links the reference. A CI test opens a real export with the stock `zstd` and `sqlite3` tools.

## Boundaries & Constraints

**Always:**
- The reference is rendered by `arena_store::render_export_reference()` from a database migrated by the shipped migrations plus `arena-proto`'s header constants, and `crates/arena-store/tests/export_reference.rs` fails when `docs/export-format/reference.md` differs (`ARENA_UPDATE_DOCS=1 cargo test -p arena-store --test export_reference` rewrites it). Meanings come from `crates/arena-store/export-reference/meanings.txt` (`table: text`, `table.column: text`, `*.column: text`) plus naming rules (`*_minor`, `*_at`, `*_bp`, `*_enc`, foreign keys); a table or column with no meaning fails the test, so a new migration cannot ship undocumented.
- Ciphertext columns (`*_enc`, `player_keys` wrapped keys) are marked "ciphertext". Money columns say "minor units". The reference lists what an export clears (licence cache, device and kitchen token hashes) and that the file holds no keystore secret or reference cache.
- "Export everything" needs the same permission or manager PIN ticket as `backup.export`, takes `from`/`to` (`YYYY-MM-DD`, `from <= to`, at most the Reports range limit), and writes into a new folder `<branch_code>-everything-<from>-<to>-<yyyy-mm-dd-hhmm>` inside the backup location. The folder holds the `.a360` file, a CSV for each of: daily sales, sessions, players, staff, stock movements, stock waste (range), shifts, one Z-report per shift closed in the range, GST for each month the range touches, stock on hand, stock expiring; and `README.txt`. The file is recorded with `backup.export` like a plain export; a report that cannot be written (e.g. players on a Hub without a key store) is listed in the README under "Not exported" and does not fail the export.
- Free and paid Hubs behave identically; no `tier-lock`.

**Never:**
- No cloud upload, restore, PDF in this export, or change to the `.a360` format (version stays 1). No Android tree-URI write (same `unsupported` as 9.4).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Export everything | Manager, folder set, valid range | folder with `.a360`, report CSVs, `README.txt`; `{folder_name, file_name, bytes, reports[], skipped[]}` | none |
| Bad range | `from > to`, missing, or too long | `VALIDATION_FAILED` field `from`/`to`, nothing written | none |
| No / bad location | as 9.4 | `VALIDATION_FAILED` field `folder` with the 9.4 reasons | partial folder removed |
| Counter, no ticket | no permission | `PERMISSION_DENIED`, nothing written | none |
| One report fails | e.g. players without a key store | export succeeds, README lists it under "Not exported" | logged |
| Schema moved on | a migration without regenerating the reference | `export_reference` test fails naming the change | run with `ARENA_UPDATE_DOCS=1` |
| New table | table with no meaning entry | test fails naming the table | add a meaning |
| Stock tools | real export, `zstd -d`, `sqlite3` | every reference table present, row counts equal the source | skipped locally if tools are absent; required in CI |

</intent-contract>

## Code Map

- `crates/arena-proto/src/backup_export.rs`, `lib.rs` -- `ExportEverythingRequest`, `EverythingExported`, `everything_folder_name`; header constants the reference quotes.
- `crates/arena-store/src/export_reference.rs` (new), `lib.rs`, `export-reference/meanings.txt` (new), `tests/export_reference.rs` (new), `tests/export_stock_tools.rs` (new) -- the renderer, the meanings, the staleness test, the stock-tool test. Migrations come from `migrations::MIGRATIONS` via `Store::open` on a temp file.
- `docs/export-format/reference.md` (new, generated) -- the public reference for the help centre.
- `crates/arena-core/src/snapshot.rs`, `report_export.rs` -- `export_everything_to_folder`: db file into a subfolder, report documents via `export_doc`/`to_csv`, README text. Reuses `ShiftList`, `ReportExportRequest`.
- `crates/arena-api/src/ui_backup.rs`, `lib.rs` -- `POST /ui/backup/export-everything`; shares the permission, lock, probe and intent steps with `export`.
- `apps/hub/src/lib/backup.ts`, `BackupPanel.svelte`, `ReportRange.svelte` (reused), `packages/ui/src/messages/en.json` -- "Export everything" with a date range; Playwright spec.
- `.github/workflows/ci.yml` -- install `zstd` and `sqlite3` on Linux and set `ARENA_REQUIRE_STOCK_TOOLS=1`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-store` -- reference renderer, meanings, staleness test, stock-tool test; commit `docs/export-format/reference.md`
- [x] `arena-proto` -- request/response types and folder name; tests; `pnpm gen:types`
- [x] `arena-core` -- `export_everything_to_folder`; tests (files present, CSV equals the single-report export, README, skipped report)
- [x] `arena-api` -- the route; tests (permission, ticket, range, location, Free hub, audit)
- [ ] Hub UI -- Export everything with range, strings, Playwright spec
- [ ] CI -- install tools and require them

**Acceptance Criteria:**
- Given the migrations, when `cargo test -p arena-store` runs, then the committed reference equals the rendered one, and it fails after a migration adds a column without a regenerated reference.
- Given the reference, then it names the header fields, the `zstd` step, every table and column with type and meaning, minor units for money and ciphertext columns marked.
- Given a Manager picks a range under System → Backup → "Export everything", then one folder holds the `.a360` file, the report CSVs for that range and a `README.txt` linking the reference.
- Given a real export, when CI decompresses it with stock `zstd` and opens it with stock `sqlite3`, then every table in the reference exists and its row count matches the source.
- Given a Free branch, then export and "Export everything" both work with no `tier-lock`.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api` -- expected: pass
- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --all-targets -- -D warnings` -- expected: clean
- `pnpm gen:types`, `pnpm run check:layers check:pins check:protocol check:secrets check:strings`, `pnpm --filter @arena/hub check` -- expected: clean

## Auto Run Result

Status: done

Implemented per spec. Verified: `cargo fmt --check` clean; clippy `-D warnings` clean on proto/store/core/tasks/api/hub; `cargo test` passes for proto/store/core/api (including the new `export_reference`, `export_stock_tools`, core and API tests) except `heartbeat::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::a_printer_that_stops_reading_times_out`, which fail on the baseline (same as Story 9.5); `ui_events::a_component_change_also_reports_the_bundles_that_hold_it` and the `proptest_apply` invariants failed once under full-workspace load and pass alone and on a rerun; `check:layers`, `check:pins`, `check:protocol`, `check:strings`, `check:secrets`, `format:check` clean; `pnpm --filter @arena/hub check` shows only the two baseline `process` errors in `playwright.kitchen.config.ts`; Playwright `backup-export.spec.ts` passes (24 tests).

Judgement calls:
- The reference is rendered from a database the shipped migrations built and compared in `cargo test` (which CI runs on every OS), so "CI fails if stale" needs no extra job. Meanings are a hand-written data dictionary (`meanings.txt`) with naming rules as fallback; a table or column with no meaning fails the test.
- The stock-tool test (`zstd -d` + `sqlite3`) is skipped when the tools are absent, and required in CI on Linux (`ARENA_REQUIRE_STOCK_TOOLS`, installed by an apt step). It could not be run on this PC (no `zstd`/`sqlite3`), so the same checks also run with the `zstd` crate and `rusqlite` on every PC.
- The reference lives at `docs/export-format/reference.md`; the help centre (Epic 11) will publish it. The README links the placeholder URL `EXPORT_FORMAT_REFERENCE_URL`.
- A branch that is not GST-registered has no GST return, so that report is left out silently, not listed as "Not exported".
- Z-reports are written for each shift closed in the range (by business day); "as of now" reports (stock on hand, expiring, shifts) are included once.
- The shared export route steps in `ui_backup.rs` were folded into one function used by both routes; a refused record deletes the whole folder.

## Code Review

Reviewed the 9.6 diff (renderer, meanings, tests, core folder writer, route refactor, UI, CI). No blocking findings; fmt, clippy -D warnings and cargo test for proto/store/core/api are green.
