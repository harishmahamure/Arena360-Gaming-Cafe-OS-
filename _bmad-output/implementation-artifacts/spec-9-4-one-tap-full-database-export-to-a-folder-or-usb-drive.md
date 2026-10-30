---
title: 'Story 9.4: One-tap full database export to a folder or USB drive'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: 'f280cafae16b4ea8ddf7903515ca6e0340eec22b'
baseline_revision: 'f280cafae16b4ea8ddf7903515ca6e0340eec22b'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-3-the-cloud-issues-epoch-counters-by-compare-and-set-on-the-br.md'
warnings: []
deferred:
  - 'Android tree-URI write: the folder bridge has no write call yet, so an export to a picked Android folder answers UNSUPPORTED (same limit as the Story 5.29 GST export).'
---

<intent-contract>

## Intent

**Problem:** An owner has no way to hold their own copy of the branch database. The backup location (Story 2.26) is only a probe target, and nothing writes a whole-database file.

**Approach:** `POST /ui/backup/export` builds a `.a360` file (versioned `arena-proto` header + zstd `VACUUM INTO` copy) in the backup location, then records it with the new `backup.export` intent (permission `backup.export`, manager-PIN elevation for a Counter), which stores `backup.last_export` and so lands in the audit log with both actors. Settings → System gets a Backup panel with "Export now".

## Boundaries & Constraints

**Always:**
- File name `<branch_code>-<yyyy-mm-dd-hhmm>.a360`, time in the branch timezone. Layout: magic `A360EXP1`, `u32` big-endian header length, header JSON (`ExportHeader`: `format_version`, `branch_id`, `branch_code`, `epoch`, `last_seq`, `schema_version`, `exported_at`, `sha256`), then the zstd body. `sha256` is the hash of the zstd body. `epoch`/`last_seq` are read from the copy, so they name the point in time.
- The body is a `VACUUM INTO` copy made on its own read connection (the writer is never blocked). `player_keys` stay as stored (wrapped by the KEK). The copy drops `licence_cache` rows, clears `devices.token_hash` and `kitchen_displays.token_hash`; the reference cache and keystore secrets are not in the database.
- The file is written as `.<name>.part` and renamed on success; any failure deletes the part file and the scratch copy.
- `backup.export` needs `Permission::BackupExport`; a Counter without it sends a manager's PIN ticket for `backup.export`. The route refuses (`PERMISSION_DENIED`) without the permission or a ticket before any file work, and the intent redeems the ticket after the file exists; if the intent is refused the file is deleted.
- A follower Hub (`writer` absent) answers `NOT_PRIMARY`.

**Never:**
- No restore, no daily export, no cloud upload (Stories 9.5, 9.7). No change to the snapshot format (`build_snapshot_file` stays).
- No Android tree-URI write; no tier lock.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Export | Manager, folder set | file written, `{file_name,bytes,epoch,last_seq,exported_at}`; `backup.last_export` set | none |
| No location | `system.backup_dir` empty | `VALIDATION_FAILED` field `folder`, message `not_set` | nothing written |
| Not writable / removed | folder missing or write fails | `VALIDATION_FAILED` field `folder`, message `not_found`/`not_dir`/`not_writable` | part file and scratch deleted |
| Counter, no ticket | no permission, no ticket | `PERMISSION_DENIED`, nothing written | none |
| Counter with PIN ticket | ticket for `backup.export` | exported; audit change shows staff + `elevated_by` | none |
| Tree URI | `content://…` location | `VALIDATION_FAILED` `unsupported` | nothing written |
| Trading during export | intents applied while it runs | never blocked; header `last_seq` is the copy's point | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/backup_export.rs` (new), `lib.rs`, `intent.rs`, `settings` keys -- `ExportHeader` + encode/decode, `export_file_name`, `BackupExport` payload, `KEY_BACKUP_LAST_EXPORT`, `IntentKind::BackupExport`, `Requirement::Perm(BackupExport)`; `USED_BY_LATER_STORIES` in `intent.rs` loses `BackupExport`.
- `crates/arena-store/src/export.rs` (new), `lib.rs` -- `write_export`: `VACUUM INTO` scratch, strip, zstd stream, hash, header, `.part` rename; model on `snapshot.rs::build_snapshot_file`.
- `crates/arena-core/src/snapshot.rs` -- `SnapshotReader::export_to_folder`.
- `crates/arena-core/src/settings_defaults.rs`, `src/apply/{mod,mutate,validate,guards}.rs`, new `apply/backup_export.rs` -- `backup.last_export` system-managed setting and the record-of-fact intent (template: `apply/printer_status.rs`).
- `crates/arena-api/src/ui_backup.rs` (new), `ui_auth.rs` (route, `run_intent` reuse, direct `/ui/intent` refusal), `lib.rs`.
- `apps/hub/src/lib/backup.ts`, `BackupPanel.svelte` (new), `routes/settings/system/+page.svelte`, `packages/ui/src/messages/en.json`, audit label in `apps/hub/src/lib/audit/labels.ts`.
- Generated types: `pnpm gen:types`. Proptest generator of Story 1.8 gets the new intent.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- header, name, payload, setting key, intent kind; tests
- [x] `arena-store` -- `write_export`; tests incl. header/body round trip, exclusions, cleanup, concurrent writes; ignored 1 GB benchmark
- [x] `arena-core` -- apply handler, setting rule, `export_to_folder`; tests
- [x] `arena-api` -- `POST /ui/backup/export`; tests (permission, ticket, no location, audit actors)
- [x] Hub UI -- Backup panel with Export now, inline PIN, inline cause with "Choose location"/"Try again"; strings; Playwright spec

**Acceptance Criteria:**
- Given a Manager and a writable location, when they tap Export now, then `<branch_code>-<yyyy-mm-dd-hhmm>.a360` exists there and the panel shows its name, size and time.
- Given the file, then its header parses (format version, branch id, code, epoch, last seq, schema version, exported_at, sha256 of the body) and the body decompresses to a SQLite database whose `player_keys` rows equal the live ones, with no licence, device token hash or kitchen token hash.
- Given intents arriving during the export, then none is blocked and the header `last_seq` equals the copy's last change.
- Given a 1 GB database, then the ignored benchmark records tap-to-file-closed time (target < 10 s on the reference PC).
- Given no location, an unwritable one, or a removed drive, then the panel shows the cause with "Choose location" or "Try again", no partial file remains, and the Hub is unaffected.
- Given a Counter, when they tap Export now, then `inline-pin` opens, and the audit entry names both actors.

## Spec Change Log

## Review Triage Log

## Design Notes

- `backup.last_export` is a record of fact (like `printer.status`), written only by the `backup.export` intent, so the audit viewer shows the export with `actor` and `elevated_by` without a new table. The panel reads it through the settings list.
- The scratch copy lives beside the database (same disk as `VACUUM`'s own work), not on the possibly slow USB drive; only the compressed body is written to the target.
- Compression level 1 keeps the 1 GB budget on the reference HDD PC.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api` -- expected: pass except failures that also fail on the baseline
- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --all-targets -- -D warnings` -- expected: clean
- `pnpm gen:types`, `pnpm run check:layers check:pins check:protocol check:secrets check:strings` -- expected: clean
- `pnpm --filter @arena/hub check` and the hub unit tests -- expected: clean

## Auto Run Result

Status: done

Implemented per spec. Verified: clippy clean on proto/store/core/api/hub; `cargo test` for proto, store, core and api pass (one `ui_events` test failed once under load and passes alone); check scripts clean; `pnpm --filter @arena/hub check` shows only the two baseline `process` type errors in `playwright.kitchen.config.ts`. The ignored 1 GB benchmark took 8.8 s on the dev PC (release, incompressible data); it has not been run on the reference HDD PC.

Judgement calls: the file name has minute resolution, so two exports in one minute share a name (the route runs one export at a time and checks the PIN ticket before writing). Scratch copies sit beside the database and there is no free-space check. Exporting to an Android tree URI answers `UNSUPPORTED` (deferred in the frontmatter).

## Code Review (2026-10-30)

Four layers (blind, edge-case, verification-gap, acceptance). Fixed:
- A refused export deleted its file by name even when it had replaced an earlier export of the same minute, leaving `backup.last_export` naming a missing file. The route now notes the folder's file names first and keeps a file that replaced one. New test `a_refused_export_keeps_an_earlier_file_it_replaced`.
- The ticket pre-check ran before the export lock, so a queued export could spend the ticket in between. It now runs under the lock.

Deferred (not blocking): no free-space check or write-error cause mapping; dropped-request orphan file; no read-back/verify path (Stories 9.5/9.7); Android tree URI (already deferred); "Choose location" is a no-op for a Counter without `settings.edit`; extra Playwright/route tests for `PIN_REQUIRED`, `NOT_PRIMARY`, `HUB_SLOW`, concurrent exports; the 1 GB benchmark prints without asserting.
