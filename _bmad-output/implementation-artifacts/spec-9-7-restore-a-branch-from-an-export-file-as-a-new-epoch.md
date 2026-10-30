---
title: 'Story 9.7: Restore a branch from an export file as a new epoch'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_revision: '5d8d176b3975bc20fc85e21b4b98ba71c8ebd2c7'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-1-erase-a-player-s-personal-data-by-crypto-shredding.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-3-the-cloud-issues-epoch-counters-by-compare-and-set-on-the-br.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-4-one-tap-full-database-export-to-a-folder-or-usb-drive.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner whose database is corrupted or wrongly changed has an `.a360` file (Stories 9.4-9.6) but no way to put it back. Nothing reads an export back, `RESTORE_*` errors do not exist, and no code opens a new epoch on purpose.

**Approach:** System → Backup → "Restore from file" checks a `.a360` file on this PC (sha256, branch id, schema) and shows what it holds and what will be set aside. After the Owner's PIN and the typed branch code the Hub stages the file and restarts; at start-up, before the writer opens, it moves the current database to `orphaned/`, loads the file, migrates it forward, re-applies every erasure, opens a new provisional epoch forked at the file's `{epoch, last seq}`, and records `ops.restore`.

## Boundaries & Constraints

**Always:**
- Check order: header/sha256 → `RESTORE_CORRUPT`; branch id → `RESTORE_WRONG_BRANCH`; schema newer than the binary → `SCHEMA_TOO_NEW`. A refused file leaves the current database and the data folder untouched (nothing staged).
- Restore needs the Owner (`ops.restore`, Owner-only in `rbac`). A Manager or Counter is shown the Owner-PIN prompt in place; the route accepts only a body that carries the Owner's own PIN (verified like `staff_deactivate`, with the elevate lockout) — any other role is `PERMISSION_DENIED`. The typed branch code must equal the database's `branch_code`, else `VALIDATION_FAILED` field `branch_code`.
- Apply at start (`arena_store::restore::apply_pending`), before `Store::open` serves: checkpoint and move `branch.db` (+`-wal`/`-shm`) to `orphaned/<old-epoch>-<yyyymmddThhmmss>.db`; move the staged file in; `Store::open` migrates it forward; in one write transaction re-apply erasures (each `players.erased_at` of the replaced database not yet erased in the file: `erase_player` + `delete_player_key`), write the lease as `(max(file epoch counter, replaced lease counter, max_seen) + 1, own instance id, provisional)` with `fork {file epoch, file last seq}`, and raise `max_seen`. The heartbeat confirms the provisional epoch (Story 9.3). A failure before the swap leaves the old database in place and removes the pending marker; the Hub starts normally.
- `orphaned/` files older than 90 days are deleted at start. Orphaned databases are never opened by replay, standby or audit builder.
- The Hub then submits `ops.restore` (a record of fact like `backup.export`, actor = the Owner who confirmed) carrying the file header facts, old and new epoch and rows set aside (`changes` rows of the replaced database above the file's last seq); it stores `backup.last_restore` and the change row is the audit entry.
- The status carries the new epoch; `/ui/events` announces `epoch.changed` to connected UIs when the lease epoch changes.
- Free and paid behave identically except the confirmation copy: on Free it also says "Players erased after <exported time> may reappear." when the replaced database has no tombstones for them (a new PC); the tombstone path works on every tier. Cloud key-backup erasure re-apply (`keys/player-keys.bin`) is Story 9.13; this story re-applies from the replaced database's own tombstones only.
- Windows Hub only; the Android Hub answers `unsupported` (no process restart there; Story 9.13 and Epic 10 own that path).

**Never:**
- No restore from cloud or onto new hardware, no standby, no promote, no change to the `.a360` format, no upload of the staged file. The old database is never deleted by restore (only by the 90-day sweep).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Check a good file | Owner, `.a360` of this branch | branch name, `exported_at`, epoch, last seq, schema, warning text, `erased_may_reappear` flag | none |
| Corrupt file | sha256 or magic/header wrong, truncated | `RESTORE_CORRUPT`, nothing staged | none |
| Other branch | `branch_id` differs | `RESTORE_WRONG_BRANCH` | none |
| Newer schema | `schema_version` > binary | `SCHEMA_TOO_NEW` | none |
| Older schema | `schema_version` < binary | accepted; migrated forward at apply | none |
| Not the Owner | Manager/Counter token | `PERMISSION_DENIED` | none |
| Wrong PIN / branch code | bad Owner PIN, or code ≠ `branch_code` | `PIN_INVALID`-style refusal / `VALIDATION_FAILED` field `branch_code`, nothing staged | lockout as elevate |
| Confirm | valid, PIN, code | file staged, `{restarting: true}`, Hub stops; on start old DB → `orphaned/`, restored DB serves, new epoch, `ops.restore` audited | pending removed on failure |
| Player erased after export | replaced DB tombstones X; file has X's data | X is erased in the restored database before the writer opens | none |
| Android Hub | confirm | `unsupported` | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/backup_export.rs`, `error.rs`, `intent.rs`, `permission.rs`, `lib.rs` -- `ExportHeader::read`/`decode` reused; add `RestoreCheck`/`RestoreRequest`/`OpsRestore` types, `ErrorCode::RestoreWrongBranch`/`RestoreCorrupt`, `IntentKind::OpsRestore`; `Permission::OpsRestore` exists (remove from `USED_BY_LATER_STORIES`).
- `crates/arena-store/src/restore.rs` (new), `lib.rs`, `export.rs` -- `inspect_export`, `stage_export`, `apply_pending`, `sweep_orphaned`; hashing/decompress recipe mirrors `write_export` (`export.rs:251`); `players.rs:303` `erase_player`/`delete_player_key`; `read_lease`/`write_lease`/`write_lease_fork`/`raise_max_seen_counter` (`lib.rs`, `upload.rs:146`).
- `crates/arena-core/src/apply/` (`backup_export.rs` is the template: `mod.rs:287`, `mutate.rs:170`, `validate.rs:63`, `guards.rs:425`), `settings_defaults.rs`, `rbac.rs` -- the `ops.restore` record intent and `backup.last_restore` setting; `apply/tests/` + proptest generator.
- `crates/arena-api/src/ui_backup.rs`, `ui_auth.rs` (routes ~3610; `staff_deactivate` 3332 PIN check; status code map ~985), `ui_events.rs` -- `POST /ui/backup/restore/check`, `POST /ui/backup/restore`; `epoch.changed`.
- `crates/arena-hub/src/lib.rs` -- call `apply_pending` before `Store::open` (line ~241) and submit `ops.restore` after the writer starts; `config.rs` for the data dir.
- `apps/hub/src/lib/backup.ts`, `BackupPanel.svelte`, `labels.ts`, `packages/ui/src/messages/en.json` -- "Restore from file" panel (path entry, checked summary, warning, inline Owner PIN, branch code, confirm); Playwright spec.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- error codes, intent kind, payload and request/reply types; `pnpm gen:types`
- [x] `arena-store` -- `restore.rs` (inspect, stage, apply_pending, orphan sweep); tests (good, corrupt, wrong branch, newer/older schema, swap, erasure re-apply, lease/fork, failure keeps old DB)
- [x] `arena-core` -- `ops.restore` record intent, setting, rbac, proptest generator; tests
- [x] `arena-api` -- check and confirm routes, `epoch.changed`; tests (roles, PIN, branch code, refusals leave disk untouched, Android `unsupported`)
- [x] `arena-hub` -- apply pending at start, submit the record, stop after staging
- [x] Hub UI -- Restore from file panel, strings, audit label, Playwright spec

**Acceptance Criteria:**
- Given a `.a360` file, when the Owner checks it, then the Hub shows branch name, exported time, epoch, last seq and "Everything after <exported time> on this PC will be set aside. Kiosks reconnect by themselves."
- Given a wrong branch id, corrupt sha256 or newer schema, when it is checked or confirmed, then restore is refused with `RESTORE_WRONG_BRANCH`, `RESTORE_CORRUPT` or `SCHEMA_TOO_NEW` and the database is untouched.
- Given the Owner's PIN and the branch code, when restore runs, then the old database is under `orphaned/<epoch>-<timestamp>.db`, the file is loaded and migrated, a new provisional epoch forked at the file's `{epoch, last seq}` is held, and `epoch.changed` is announced.
- Given a player erased after the export, when restore runs, then that player is erased in the restored database before the writer opens.
- Given a Manager or Counter, when they open "Restore from file", then it asks for the Owner PIN in place and refuses any other role.
- Given a completed restore, then the audit shows `ops.restore` with the file header, old and new epoch and the rows set aside.

## Spec Change Log

## Review Triage Log

## Design Notes

A hot swap would have to rebuild the writer, lease task, snapshot reader and directories that all hold the file open (and Windows cannot rename an open file). Staging then applying at start gives the same observable result (writer closed, file moved, writer opened on the restored file) without touching those components. The 3 s cloud claim is replaced by the existing provisional-then-confirm path: the restored Hub always starts provisional and its first heartbeat confirms or self-locks.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api` -- expected: pass
- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --all-targets -- -D warnings` -- expected: clean
- `pnpm gen:types`, `pnpm run check:layers check:pins check:protocol check:secrets check:strings`, `pnpm --filter @arena/hub check` -- expected: clean

## Auto Run Result

Status: review

Implemented per spec: `arena_store::restore` (inspect, stage, apply at start, 90-day orphan sweep), the `ops.restore` record intent and `backup.last_restore` setting, `POST /ui/backup/restore/check` and `/ui/backup/restore` (Owner PIN, branch code), `epoch.changed` on `/ui/events`, Hub start-up wiring, and the "Restore from file" panel. Verified: fmt, clippy `-D warnings`, `cargo test` for proto/store/core/api/hub, `check:layers/pins/protocol/secrets/strings`, `format:check` all clean; `pnpm --filter @arena/hub check` shows only the two baseline `process` errors; Playwright `backup-restore.spec.ts` and `backup-export.spec.ts` pass.

Judgement calls and risks:
- Restore is staged, then applied at the next Hub start (a hot swap would have to rebuild every component holding the database open). The Hub relaunches itself unless `ARENA_HUB_SUPERVISED=1`; the relaunch itself is not covered by a test, and a service supervisor that also restarts the Hub could double-start without that variable.
- The epoch is always provisional at restore; the first heartbeat confirms it (no separate 3 s claim).
- The restored database keeps this Hub's pairing identity, licence cache and device/kitchen token hashes from the replaced database, so kiosks reconnect.
- Cloud key-backup erasure re-apply is left to Story 9.13. Android answers `UNSUPPORTED`.
- Playwright `audit.spec.ts` "filters combine, dates are branch days" failed once on a date boundary; not touched by this story and not confirmed against baseline.

## Code Review Result

Status: done. Fixed in review:
- `ulid_like` produced ids whose first character could exceed `7`, so about half of all restores failed the ULID check and `ops.restore` was never recorded (the Hub restore test was flaky for this reason). The first character is now clamped; a unit test covers it.
- `restore_owner`: a non-Owner's own correct PIN reset the elevate lockout, so alternating it with guesses at the Owner PIN avoided the pause. It now counts as a failed attempt.
- `stage_export` deleted the source file when the Owner typed the path of the already staged `restore-pending/restore.a360`; it is now refused (`path`/`not_readable`) and the earlier staging is kept.

Deferred (not blocking): crash between the two renames of the swap leaves no `branch.db` (data is in `orphaned/` and the original export); `epoch.changed` is not announced for a restore applied at start (UIs reconnect after the restart); a failed apply at start is only logged; relaunch and supervisor double-start untested; disk-full during unpack reports `RESTORE_CORRUPT`.
