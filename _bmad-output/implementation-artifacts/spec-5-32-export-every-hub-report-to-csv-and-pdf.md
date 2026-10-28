---
title: 'Story 5.32: Export every Hub report to CSV and PDF'
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
baseline_commit: 'd554543'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-29-gstr-1-export.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-31-stock-reports-with-waste-by-reason.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A manager can read every Hub report on screen but cannot hand one to an accountant or keep it offline.

**Approach:** One endpoint, `POST /ui/reports/export {report, format, folder, from?, to?, month?, shift_id?, product?, reason?}` (`report.view`), reads the report through the same `SnapshotReader` call its screen uses, lays it out once as an `ExportDoc` of typed cells (`arena-core::report_export`) and writes that document as CSV or as an A4 PDF (`arena-core::pdf`, the bundled Sora, no network). Every screen gets one shared `ExportPanel` (folder, Export CSV, Export PDF).

## Boundaries & Constraints

**Always:**

- Reports: `daily-sales`, `sessions`, `shifts`, `z-report` (`shift_id`), `players`, `staff`, `stock-on-hand`, `stock-moves` (+ `product`, `reason`), `stock-waste`, `stock-expiring`, `gst` (`month`; the Story 5.29 ZIP keeps its own endpoint). The panel sends the range, month or shift the screen loaded (`view.from/to`), so the file is the figures on screen.
- CSV: UTF-8 with a byte-order mark, CRLF, one header row per table, money in major units at the currency's decimals (no grouping) with a `Currency` column on every table that has money, times as `YYYY-MM-DD HH:MM` on the branch clock, percentages in tenths. A report of several tables writes each under its title, separated by a blank line. A text beginning `= + - @` is prefixed with `'` so a spreadsheet cannot read it as a formula.
- PDF: A4, portrait (landscape only when a table is too wide for portrait), branch name, report title, range, generated-at, "Page n of m"; the same figures grouped the branch's way (`1,23,456.78`), tables headed "(amounts in INR)"; long tables continue on the next page with their header; Sora embedded whole (`assets/Sora-Regular.ttf`, OFL), digits set to the widest digit's width (tabular).
- The file is written through a temporary name into an absolute writable folder; `VALIDATION_FAILED` on `folder` (`not_found`, `not_dir`, `not_writable`, `unsupported`), `report`, `from`/`to`, `month`, `shift_id`. The screen shows the problem inline and asks for another folder.

**Never:**

- Writing to the database, a network call, or recomputing a figure: the export reads the same stored rows and rounds nothing.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| CSV | stock on hand, Cola 20 at 15.00 | `Cola,20,15.00,300.00,INR` and a `Total` row | none |
| PDF | the same | A4 PDF showing `300.00` in Sora | none |
| Unwritable folder | `/bad`, a file, a missing path, an Android tree URI | nothing written | 422 `folder` with the reason; screen names it and offers another |
| Bad request | unknown report, no range, reversed range, no month or shift | nothing written | 422 on that field |
| No `report.view` | counter role | 403 `PERMISSION_DENIED` | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/report_export.rs` -- `ReportExportRequest`, `ReportExported`, `ExportFormat`, `EXPORT_REPORTS` (+ generated TS).
- `crates/arena-core/src/report_export.rs` -- `ExportDoc`/`Cell`, a builder per report, `to_csv`, `to_pdf`, `SnapshotReader::export_doc/export_report`; `pdf.rs` -- TrueType reader and PDF writer; `assets/Sora-Regular.ttf` + `Sora-OFL.txt`.
- `crates/arena-api/src/ui_report.rs` + `ui_auth.rs` -- `export` handler and route (reuses `write_export`).
- Hub: `lib/ExportPanel.svelte`, `lib/reports.ts` (`exportReport`), the Daily sales, Sessions, Shifts, Z-report, Players, Staff, Stock (each tab) and GST screens, `en.json`.
- Tests: core (CSV, PDF, font), API (`a_report_exports_to_csv_and_pdf_with_the_figures_of_its_screen`), Playwright `reports.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- request, answer, format, generated TS
- [x] `arena-core` -- `ExportDoc` builders for every report, CSV writer, PDF writer with the embedded Sora
- [x] `arena-api` -- `POST /ui/reports/export`
- [x] Hub -- `ExportPanel` on every report screen and Stock tab, strings
- [x] Tests -- core, API, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given any report on screen (including a Z-report and the stock and GST views), when staff choose Export → CSV, then the Hub writes a UTF-8 CSV with a header row, amounts in major units with the currency code column, dates in branch local time, to a chosen folder or USB.
- Given Export → PDF, when chosen, then the Hub renders an A4 PDF with the branch name, report title, range, generated-at time and the same figures, using the bundled Sora font with tabular numerals and no network fetch.
- Given a report figure on screen, when it is compared with its CSV and PDF, then all three match exactly.
- Given an unwritable destination, when export runs, then an inline error names the problem and offers another location.

## Spec Change Log

## Review Triage Log

Unattended build: fmt, clippy (-D warnings), cargo tests for proto/store/core/api, pnpm checks, Hub svelte-check and Playwright `reports.spec.ts` green.

## Design Notes

The figures cannot drift because there is no second computation: the screens, the CSV and the PDF all read the same typed report and the export only formats its stored minor units (`plain_money` for CSV, the branch's grouping for PDF). Labels in the files are English (a file is shared outside the Hub, so it does not follow the Hub's UI language). Sessions' per-hour utilisation chart and the GST ZIP's per-document sheets are not in these files: the GST view exports the section table the screen shows, and the ZIP keeps its own button. The font is the default (Regular) instance of the Sora variable font already bundled for the UI, decoded from WOFF2 to TrueType and embedded whole (66 KB, uncompressed). Digits get the widest digit's advance, which makes figures tabular without parsing GSUB.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`; `pnpm gen:types`, `check:generated`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `reports.spec.ts` in `apps/hub`.

## Auto Run Result

Status: in-review. Implemented and verified.
