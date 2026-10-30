---
title: 'Story 9.14: The Hub emits a yearly GST archive kept for 8 years'
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '38cc1c7d544c06312de9a9f4d0eb7b3cb28ebc4c'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-13-rebuild-the-branch-from-the-fetched-backup-as-a-new-epoch.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Backups follow the plan's retention (1 or 3 years) but GST records must answer a tax notice for 8 years; nothing yet writes the financial year's invoices, credit notes and tax totals to a place that outlives the backup.

**Approach:** After the first business-day close following a financial year's end, a paid Hub builds `gst-archive/<fy>.zst` (zstd JSON: every numbered invoice with lines, every credit note, a per-rate tax summary, and invoice numbers orphaned by a fork), PUTs it through a new presign `kind: gst_archive` (same prefix and `FENCED` rule as snapshots), and records it with a system intent `gst_archive.publish` (fy, key, sha256, counts) so it shows in the audit and in System -> Backup. A late invoice or credit note stamped with an archived `fy` makes the next close rebuild and overwrite that file.

## Boundaries & Constraints

**Always:**
- Paid tiers on the Primary Hub only; Free (and a follower, revoked, restoring Hub) makes no request, the GST rows stay in the Hub database and in every export (no pruning is added here).
- Due = an `fy` that has numbered invoices or credit notes, is earlier than the financial year of the latest published rollup day (`fy_of` with the branch's FY start month), and has no log row, or whose fingerprint (invoice count, credit note count, newest `updated_at`) differs from the logged one. Checked once per newly published rollup day per process; a failed attempt is retried next cycle.
- Content is the stored rows as they are (no tax recomputed): invoices of `orders` (`status = settled`, sales kinds, `invoice_no` set, `orders.fy = fy`) with their lines and stored `tax_breakup`; `refunds.fy = fy` credit notes with their stored (negative) `tax_breakup`; a per-rate summary (rate, taxable, tax, cgst/sgst when stored) for invoices and for credit notes; the tax mode (`gst_in` or `simple`) and generic tax label; `orphaned_invoices`. Any tax mode produces the archive under the same key.
- Orphaned invoice numbers = numbers of that `fy` found in databases under `orphaned/` that the live database does not hold, unioned with the numbers already logged (so the 90-day sweep never loses one). Unreadable orphan files are skipped with a warning.
- Presign `kind: gst_archive` takes an `fy` (`FY2026-27` / `FY2027`, no path characters, else `FORBIDDEN`/`VALIDATION_FAILED`), is refused `FENCED` for a stale epoch and `FEATURE_NOT_ENTITLED` on Free, and returns the key `gst-archive/<fy>.zst` under the branch prefix.
- The record is written only after the PUT answered 2xx. `gst_archive.publish` is a system intent that writes only its `gst_archive_log` row (upsert by `fy`), carries `fy`, `key`, `sha256`, `bytes`, `invoices`, `credit_notes`, `orphaned`, `built_at`, and never touches a business row. It is added to the Story 1.8 proptest generator exclusions the same way as `rollup.publish`.
- System -> Backup lists "GST archive 26-27 saved" per archived year (read from `gst_archive_log`).
- Logs carry fy, counts and codes, never buyer names, GSTINs or amounts.

**Never:**
- No retention sweep or pruning (Story 9.15), no header badge (9.16), no change to the batch, summary, rollup, snapshot or key-backup paths, no archive on Free, no recomputation of tax.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First close after FY end | paid Hub, rollup published for 1 Apr, FY2025-26 has invoices | one PUT to `gst-archive/FY2025-26.zst`, log row, audit entry, "GST archive 25-26 saved" | none |
| Nothing due | FY not ended / already archived and unchanged | no presign | none |
| Late credit note | refund with `fy = FY2025-26` created after the archive | next close rebuilds and overwrites the same key, log row updated | none |
| Simple tax branch | `tax_mode = simple` | same archive, generic tax lines | none |
| Free / follower | tier Free | nothing uploaded | none |
| Orphans | `orphaned/x.db` holds invoice numbers the live DB lacks | listed in `orphaned_invoices`, kept on later rebuilds | unreadable file skipped |
| PUT fails | cloud 5xx | no log row; retried next cycle with backoff | `failed()` backoff |
| Fenced | stale epoch | presign `FENCED`, nothing recorded | handled as for snapshots |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{gst_archive.rs,presign.rs,intent.rs,lib.rs}` -- `PRESIGN_KIND_GST_ARCHIVE`, `GstArchiveV1`, `GstArchivePublish`, `valid_fy`, `gst_archive_object_key`; `PresignRequest.fy`; `IntentKind/IntentPayload::GstArchivePublish` (`gst_archive.publish`, system); regenerated TS.
- `crates/arena-store/migrations/0087_gst_archive_log.sql`, `src/{gst_archive.rs,branch.rs,lib.rs,migrations.rs}`, `export-reference/meanings.txt` -- `gst_archive_log` table, `Table/RowKey::GstArchiveLog`, source-row reads, fingerprint, closed-FY list, orphan scan.
- `crates/arena-core/src/{gst_archive.rs,apply/{gst_archive.rs,mod.rs,mutate.rs,validate.rs},snapshot.rs,writer.rs}` -- pure archive builder and zstd, apply handlers, `SnapshotReader::gst_archive_due/gst_archive_file`, `WriterHandle::publish_gst_archive`, list for the UI.
- `crates/arena-cloud/src/{presign.rs,api/presign.rs}` -- `PutKind::GstArchive`, `gst_archive_key`, `Want::GstArchive`.
- `crates/arena-tasks/src/upload.rs` -- `publish_gst_archive` after the snapshot step in `cycle`.
- `crates/arena-api/src/*` + `apps/hub/src/routes/system/backup`, `packages/ui/src/messages/en.json` -- list route and System -> Backup section.
- Patterns copied from: `publish_snapshot` (upload.rs), `rollup.publish` (apply/summary.rs, writer.rs:1062), `keys` presign (api/presign.rs).

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- archive types, intent, presign `fy`; regenerate TS
- [x] `arena-store` -- migration, table, queries, orphan scan; tests
- [x] `arena-core` -- builder, apply handlers, writer call, reader methods; tests
- [x] `arena-cloud` -- presign kind; tests
- [x] `arena-tasks` -- publisher; tests
- [x] API route, System -> Backup, strings; Playwright spec

**Acceptance Criteria:**
- Given a paid Hub and the first close after the FY ends, when the publisher runs, then it PUTs `gst-archive/<fy>.zst` holding every invoice, credit note and per-rate tax summary of that `fy` plus orphaned invoice numbers, via presign `kind: gst_archive` with the snapshot `FENCED` rule.
- Given the archive is uploaded, when it is recorded, then `gst_archive.publish` stores fy, key, sha256 and counts, appears in the audit, and System -> Backup shows "GST archive 26-27 saved".
- Given a later invoice or credit note with a past `fy`, when the next close runs, then the archive for that `fy` is rebuilt and overwritten.
- Given a branch with `tax_mode` other than `gst_in`, when the FY closes, then the same archive is produced with generic tax lines under the same key.
- Given a Free branch, when the FY closes, then nothing is uploaded and no GST row is removed.

## Spec Change Log

## Review Triage Log

## Design Notes

The log table is a dedicated one (not `rollup_log`) because it needs the hash, counts and the orphan list the staleness check compares against. The staleness fingerprint avoids rebuilding a year of invoices on every close. Orphan numbers are detected as "in a set-aside database, not in the live one": it works for restore now and for promote in Stories 9.21-9.23 with no change.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-tasks -p arena-api -p arena-hub`, `cargo clippy` on them `--all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run gen:types`, `check:strings`, `check:layers`, `check:protocol`, `format:check`, `pnpm --filter @arena/hub check`, Playwright spec for System -> Backup

## Auto Run Result

Status: review

Implemented per spec. Judgement calls and risks:
- The staleness check stores no separate fingerprint: the log row's counts and `built_at` are compared with the year's current invoice count, credit note count and newest `updated_at` (stale when a count differs or a row was written at or after `built_at`). A row written in the same millisecond as the build costs at most one extra rebuild.
- `gst_archive.publish` carries the orphaned invoice numbers themselves (`orphaned` is a list; the audit and `gst_archive_log.orphaned` hold them), so the 90-day sweep of `orphaned/` never loses one. The System -> Backup list shows only the count.
- An invoice or credit note with no stored `tax_breakup` counts at 0% with its total as taxable value in the per-rate summary, as the GSTR-1 export lists it.
- System -> Backup did not exist yet (Stories 9.16/9.17 add the status); this story adds the section with the GST archive list only, read through `GET /ui/backup/gst-archive` (`report.view`).
- `arena-core` gained a `zstd` dependency (already in the workspace).

## Verification Results

- `cargo fmt --check`, `cargo clippy` (proto, store, core, cloud, tasks, api, hub, replay; `--all-targets -D warnings`): clean.
- `cargo test`: proto, store, core (lib and proptest_apply), cloud (lib and presign), api, hub pass. Failing, none in touched code: `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings` (same "library_entries has no row" as at HEAD~1), `arena-tasks` `heartbeat::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::a_printer_that_stops_reading_times_out`, `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start_...` (Windows timing/process tests).
- `gen:types`, `check:strings`, `check:layers`, `check:protocol`, `format:check`: ok. `pnpm --filter @arena/hub check`: only the 2 pre-existing `playwright.kitchen.config.ts` errors. Playwright `system-backup`, `audit`, `service`, `operating`: pass.
