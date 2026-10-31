---
title: 'Story 9.23: The new main Hub lists what was set aside after a promote or restore'
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '6db281c7'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-22-force-promote-after-a-data-loss-warning.md'
warnings: []
deferred:
  - 'The Hub UI: System -> Backup -> "Set aside after promote or restore" (the forks, the databases and the orphaned invoices from `GET /ui/backup/set-aside`, with the "truncated" note when `invoices_truncated`) and the "Export list" button calling `POST /ui/backup/set-aside/export` and showing the file name. Needs the System screen, which does not exist yet; both routes are built and tested.'
  - 'Running-Hub wiring for a standby: the sweep runs only on a primary Hub. A standby that was never promoted keeps its `orphaned/` files until it is (it has none until 9.25 moves a database there).'
  - 'A promote leaves the old-epoch rows on the old main PC, not on the new main, so a promote fork lists the lag at the time (`lag_rows`) and no database. The old main lists its own rows when 9.24/9.25 move its database into `orphaned/`; the file name will then match no restore record and its changes are counted as unknown (`set_aside_changes: null`) until 9.25 records the fork on that move.'
---

<intent-contract>

## Intent

**Problem:** A promote or a restore opens a new epoch and leaves old rows behind, but nothing on the new main Hub says which, when, by whom or what they were worth. The owner's accountant cannot account for the invoice numbers that only the set-aside database holds, and the 90-day clean-up of `orphaned/` deleted a database without leaving any trace of what was in it.

**Approach:** The fork declaration (lease fork columns, first batch header, `branch_fence.fork_seq`) and the replay handling of orphaned rows (standby, cloud restore, audit builder) were built in Stories 8.x, 9.3, 9.7, 9.13 and 8.36; this story adds what the new main Hub shows. `GET /ui/backup/set-aside` lists each fork (read from the `lease.promote` and `ops.restore` records in the change log) and, from every database in `orphaned/`, the changes set aside and the invoice numbers the live database lacks, with amount and time. `POST /ui/backup/set-aside/export` writes the same as CSV to the backup location. Daily housekeeping lists a database older than 90 days into the audit as the system intent `ops.orphan_sweep` (invoice numbers and amounts only) and deletes the file only after that record is committed.

## Boundaries & Constraints

**Always:**
- A fork is every `lease.promote` and `ops.restore` change row whose record parses: time, kind, old and new epoch, fork seq, who, forced or not, and the changes set aside (a promote's lag, a restore's rows above the file). Newest first.
- A database in `orphaned/` is any `*.db` there. Its orphaned invoices are the `orders.invoice_no` values the live database does not hold, with `total_minor` and `settled_at` (else `created_at`). A file that cannot be opened is listed as `unreadable` and never fails the list.
- Its set-aside changes are counted only when a restore record names the file (`orphaned_file`): the file's changes of that restore's old epoch above the restore's `last_seq`; otherwise unknown (`null`).
- The screen list holds at most 2 000 invoices (`invoices_truncated`, `invoice_total`); the CSV holds all of them, as three tables (forks, databases, invoices) with a byte-order mark, CRLF, money in major units with the currency, and text defused against spreadsheet formulas.
- `GET .../set-aside` needs `report.view`; `POST .../set-aside/export` needs `backup.export` and writes to `system.backup_dir` after the same folder probe as the other exports (`VALIDATION_FAILED` on `folder`: `not_set`, `unsupported`, `not_found`, `not_dir`, `not_writable`). Both read only; the export records nothing.
- The sweep runs from the daily housekeeping on a primary Hub (at start, daily, on becoming primary). A database older than 90 days (the stamp in its name) is listed in one `ops.orphan_sweep` record (system actor `hub-orphan-sweep`; file, set-aside changes, invoice number and amount per invoice; no personal data), which sets `ops.last_orphan_sweep`; the files are deleted only after that commit. A refused or failed record keeps them for the next run. The start-up sweep in `arena-hub` no longer deletes anything.
- Invoice numbers are never reused: the new epoch writes a new invoice segment (Story 5.14), so the orphaned numbers stay unique to the set-aside database.

**Never:**
- No self-lock of a returning old main (9.24), no "Make this the standby" (9.25), no change to the fork declaration or replay.

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Nothing set aside | no records, no `orphaned/` | empty lists, `invoice_total: 0` |
| Restore with its file | record names `4-abc-...db`, 5 changes above the fork in it | fork row (restore, 5 set aside), database row `set_aside_changes: 5`, its invoices |
| Promote | forced, lag 7 | fork row `forced: true`, `set_aside_changes: 7`, no database |
| Unreadable file | not SQLite | database row `unreadable`, no invoices, list still answers |
| Invoice also held live | same number in both | not listed |
| 89 / 91 days old | stamp in the name | not swept / listed into the audit, then deleted |
| Record refused | no branch settings seeded | file kept, next run tries again |
| Follower Hub | not primary | nothing recorded or deleted |
| Counter | no `report.view` / `backup.export` | `PERMISSION_DENIED` |
| Backup location unset | no folder | export `VALIDATION_FAILED` (`not_set`) |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{set_aside.rs,intent.rs,lib.rs}` -- `SetAsideList` and parts, `SetAsideExported`, `set_aside_csv`, `set_aside_file_name`; `OrphanSweep` and `ops.orphan_sweep` (System requirement, `ops.last_orphan_sweep`).
- `crates/arena-store/src/{restore.rs,set_aside.rs}` -- `list_orphan_files`, `delete_orphans`, `OrphanFile::is_due`; `scan_orphan_databases`, `count_changes_above`.
- `crates/arena-core/src/{set_aside.rs,snapshot.rs,writer.rs,settings_defaults.rs}` and `apply/orphan_sweep.rs` -- the list, CSV and sweep read on the shared reader; `SnapshotReader::{set_aside,export_set_aside,orphans_due}`; `WriterHandle::record_orphan_sweep`; the setting and its rule.
- `crates/arena-tasks/src/housekeeping.rs`, `crates/arena-hub/src/{lib.rs,restore.rs}` -- `HousekeepingTask::{with_orphans,sweep_orphans}`; the start-up sweep removed.
- `crates/arena-api/src/{ui_backup.rs,ui_auth.rs}` -- the two routes.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- set-aside types, CSV and file name, `OrphanSweep`, `ops.orphan_sweep` kind; unit tests
- [x] `arena-store` -- orphan file listing and deletion, orphan database scan, change count above a fork; tests
- [x] `arena-core` -- list from the change log, CSV export, due sweep, `ops.orphan_sweep` apply and `ops.last_orphan_sweep`; proptest generator; tests
- [x] `arena-tasks` / `arena-hub` -- housekeeping sweep (list into the audit, then delete); tests
- [x] `arena-api` -- `GET /ui/backup/set-aside`, `POST /ui/backup/set-aside/export`; tests
- [x] Generated TS refreshed (`SetAside*`, `OrphanSweep*`, `IntentKind`, `IntentPayload`)
- [x] Fork declaration end to end -- already covered: `arena-tasks/src/upload.rs` (`the_first_batch_of_a_forked_epoch_carries_the_fork_and_starts_after_the_parent`), `arena-cloud` fence tests (`fork_seq == parent_seq`), `arena-replay`/`arena-cloud` audit builder tests (`orphaned_changes`, orphaned invoices in the GST archive, Story 9.14)
- [ ] Hub UI for the list and "Export list" (deferred, see front matter)

**Acceptance Criteria:**
- Given a Hub that opened a new epoch by promote or restore, then its first batch declares the fork from `parent_seq + 1` and `branch_fence.fork_seq == parent_seq` (built earlier, tests above).
- Given old-epoch rows above the fork, then standby replay, cloud restore and the audit builder never apply them and list them as orphaned (built earlier).
- Given the new main Hub, then `GET /ui/backup/set-aside` lists each fork (time, old and new epoch, who, forced or not) and, from every `orphaned/` database, the set-aside changes and each orphaned invoice number with amount and time; `POST .../export` writes it as CSV to the backup location.
- Given an `orphaned/` database older than 90 days, when daily housekeeping runs, then it is deleted after its list is written once into the audit as a system intent (invoice numbers and amounts only).
- Given orphaned invoice numbers, then later invoices never reuse them (Story 5.14 segment, unchanged).

## Spec Change Log

## Review Triage Log

- Dev verification (2026-10-31): see the Verification section.

## Design Notes

No migration: forks are read from the change log (the `lease.promote` and `ops.restore` rows already carry everything), so the list is as durable as the change log (uploaded rows are pruned after 90 days, money rows after 8 years; promote and restore records are not money rows, so a fork older than 90 days that was uploaded may no longer be listed on screen, though the orphaned invoices and the audit record of the sweep remain).

The sweep order is the point of AC4: `orphans_due` only lists, `record_orphan_sweep` commits the audit row through `apply()`, and only then `delete_orphans` removes the files, so a crash between the two leaves a file and an audit record, and the next run records it again (harmless) and deletes it. The recorded list is the whole payload, so it is capped at the GST archive's orphan cap (100 000 numbers); beyond that the record is refused and the file stays.

`sweep_orphaned` (restore.rs) is kept, now built on `list_orphan_files` and `delete_orphans`, but the Hub no longer calls it.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-api -p arena-hub`, `cargo clippy --workspace --all-targets -- -D warnings` (minus `arena-e2e`, `arena-kiosk-svc`), `cargo fmt --check`
- `pnpm run gen:types`, `pnpm run check:strings`, `pnpm run check:protocol`
