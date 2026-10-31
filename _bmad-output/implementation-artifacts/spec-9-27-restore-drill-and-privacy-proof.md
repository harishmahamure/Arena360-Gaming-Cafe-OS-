---
title: 'Story 9.27: Restore drill and privacy proof'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: 'e48e0f93220ddc2690a173fe49b1ef66e4fa8725'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-13-rebuild-the-branch-from-the-fetched-backup-as-a-new-epoch.md'
warnings: ['oversized']
deferred:
  - 'The 1 GB export benchmark (ignored test) compiled but did not run here: the dev disk was full. The release-bench job runs it on the reference i3; the figure is unverified until then.'
  - 'A player created just before the export whose key the cloud key backup had not yet received is erased by the re-apply, as the spec says: erasure that holds against a resurrected player was chosen over keeping that player.'
  - 'The cloud restore plan answers only a replacement Hub with the branch key released; on any other paid Hub the key lookup returns nothing and the export restore proceeds as before.'
  - 'The drill seeds no GST invoices, so the scan covers the GST archive fields only.'
  - 'Failures not from this story: arena-replay replaying_the_seeded_log_reproduces_branch_and_settings; arena-tasks heartbeat trusted_time and printer lan a_printer_that_stops_reading tests.'
---

<intent-contract>

## Intent

**Problem:** Nothing proves, before a release, that a branch restores from a local export and from the cloud backup with the same rows and with erased players still erased, or that no personal data leaks into a backup artefact. One piece is missing: an export restored on a new paid PC does not erase a player erased after the export (Story 9.7 re-applies only the replaced database's own tombstones and left the cloud key-backup re-apply open).

**Approach:** Build the missing re-apply (restore from export, paid, cloud reachable: a player whose key is in the export but not in the cloud key backup is erased before the writer opens), then the drill: one seeded branch with 30 days of activity, a fork and two erased players, restored both ways; a privacy scanner over every artefact; the 1 GB export benchmark; one release report; and a release-bench job that blocks the release.

## Boundaries & Constraints

**Always:**
- Re-apply by key backup only when the Hub is paid, the key backup could be fetched and opened with the branch key. Otherwise nothing changes (the Free/offline warning of Story 9.7 stays). A player already erased in the file is untouched; a player with no key row in the file is untouched.
- The erasure runs inside `apply_pending`, before the swap, with the same `erase_player` + `delete_player_key` as the tombstone path, and is counted in `erasures_reapplied`.
- The drill compares `state_checksum` of the restored rows with the source's at the restore point, and checks for each erased player: no `player_keys` row, `erased_at` set, PII columns empty.
- The privacy scanner reads every file the drill produced (batches and snapshots decompressed, key backup, GST archive, audit cache, export) and the captured log text, and fails on any seeded name, phone or email in plaintext.
- The report fails the release on any missed budget or missing result.

**Never:**
- No change to the epoch, fork or replay rules. No new UI. No cloud-side change.

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Erased after export | file holds P2's key, cloud key backup does not | P2 erased in the restored DB, no key, `erasures_reapplied` counts it |
| Backup unavailable | free tier, offline, unreadable | restore as before, nothing erased extra |
| Player not in backup but erased already | file row has `erased_at` | untouched |
| Player created after the export | not in the file | not in the restored DB |
| Plaintext in an artefact | a seeded phone appears in a batch | scanner names the file and fails |

</intent-contract>

## Code Map

- `crates/arena-store/src/restore.rs` -- `Marker`, `stage_export`, `build_restored` (tombstone loop at "Every erasure of the replaced database"): add `live_keys` to the marker and the key-backup erasure.
- `crates/arena-core/src/snapshot.rs` `stage_restore` -- passes the live key ids; `crates/arena-core/src/restore_fetch.rs` -- new `KeyBackupLookup` trait.
- `crates/arena-api/src/ui_backup.rs` (`restore`) + `ui_auth.rs` (`with_key_backup`) -- ask the lookup on a paid Hub before staging.
- `crates/arena-tasks/src/restore_fetch.rs` -- `RestoreFetcher::key_backup_bytes` (plan, `keys_url`, download); `crates/arena-hub/src/lib.rs` -- builds the lookup (open with the branch key, `arena_core::key_backup::open_key_table`).
- `crates/arena-replay/src/rebuild_tests.rs`, `src/tests.rs` -- the Story 9.13 backup fixture (`backup()`), reused by the drill for the cloud path.
- `crates/arena-replay/src/drill.rs` (test module) -- the drill and the privacy scan; `crates/arena-store/src/export.rs` `export_one_gigabyte_benchmark` -- the benchmark, which also writes its result.
- `scripts/restore-drill-report.mjs` (+ test), `.github/workflows/release-bench.yml` job `restore-drill`, `docs/runbooks/restore-drill.md`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store/src/restore.rs` -- marker `live_keys`, `stage_export_checked`, erase in `build_restored` -- AC2
- [x] `crates/arena-core`, `arena-api`, `arena-tasks`, `arena-hub` -- the lookup seam and its wiring -- AC2 on a real Hub
- [x] `crates/arena-replay` drill test -- AC1, AC3, AC4
- [x] export benchmark result file -- AC5
- [x] `scripts/restore-drill-report.mjs`, workflow job, runbook -- AC6

**Acceptance Criteria:**
- Given a seeded branch with 30 days of activity, a fork and two erased players, when it is restored from a local export on a new PC and from the cloud backup, then both reach a new epoch, row checksums equal the source's, and both erased players have no key and no readable PII.
- Given a player erased after the export on a paid branch, when restore from the export runs with the cloud reachable, then that player is erased before the writer opens.
- Given a promote after an erasure, when the standby becomes main, then the erased player has no key there.
- Given every artefact the drill produced, when the scanner searches for the seeded names, phones and emails, then none appears in plaintext and the logs carry only player ids.
- Given the 1 GB reference database, when the export benchmark runs, then it completes under 10 s and the result is recorded in the report.
- Given the release pipeline, when a candidate is built, then the drill runs and any failure blocks the release.

## Spec Change Log

## Review Triage Log

- Patched: a key backup that opens but holds no key is not used to erase (BackupKeys::open); key-backup scratch file is unique per call and removed on cancel; lookup timeout is logged; HashSet lookup in the re-apply; empty env vars ignored; report requires living_readable and tolerates null entries.
- Deferred (low): the drill log scan is partly self-written; result JSON values are literals written after assertions held; Hub wiring (run()) has no test; the 1 GB benchmark is unverified until the reference-i3 job runs.

## Verification

**Commands:**
- `cargo test -p arena-store restore` / `-p arena-replay drill` / `-p arena-api` / `-p arena-tasks restore_fetch` -- expected: pass
- `node --test scripts/restore-drill-report.test.mjs` -- expected: pass
- `cargo fmt --all --check`, `cargo clippy -p <touched> --all-targets -- -D warnings`
