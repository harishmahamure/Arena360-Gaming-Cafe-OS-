# Restore drill and privacy proof (Story 9.27)

The per-release proof that a branch restores, from a local export and from the cloud backup, with
the same rows and with erased players still erased, and that no personal data leaks into a backup
artefact. It is the `restore-drill` job of `.github/workflows/release-bench.yml` (manual run and
release tags). The job runs the tests below with `ARENA_RESTORE_DRILL_OUT=target/restore-drill`,
then `scripts/restore-drill-report.mjs` merges the result files and fails the release when a check
is missed **or a result is missing**. The report is `target/restore-drill/report.md` (uploaded as
the `restore-drill-report` artefact).

## What is checked

| Check | Where it comes from | Result file |
| --- | --- | --- |
| A seeded branch with 30 days of activity, a fork and two erased players restores from a local export on a new PC and from the cloud backup: a new epoch, the source's row checksum (`state_checksum`), both erased players without a key and without readable PII, the living players still readable | `crates/arena-replay/src/drill.rs`, `the_branch_restores_from_an_export_and_from_the_cloud_...` | `restore.json` |
| A player erased after the export is erased before the writer opens when the Hub is paid and the cloud key backup can be read; with no backup nothing extra is erased | `a_player_erased_after_the_export_is_erased_before_the_writer_opens_...` (the rule itself: `crates/arena-store/src/restore.rs`, `stage_export_checked`) | `late-erasure.json` |
| A promote after an erasure leaves the erased player without a key on the new main Hub, even when the standby kept a stale key | `an_erased_player_has_no_key_on_the_standby_once_it_is_promoted` | `promote.json` |
| No seeded name, phone or email appears in plaintext in any artefact the drill produced (batches and snapshot inflated, key backup, GST archive, audit cache, the export, the source and restored databases) or in the captured log, which carries player ids | `no_seeded_name_phone_or_email_is_in_plaintext_in_any_artefact_or_the_log` | `privacy.json` |
| The export of the 1 GB reference database completes in under 10 s | `export_one_gigabyte_benchmark` in `crates/arena-store/src/export.rs` (`--ignored`, release build, on the reference PC) | `export-bench.json` |

The scanner is itself tested: the privacy test plants a seeded phone into one batch and a seeded
email into the log and expects the scan to name exactly that file and that log.

## How the late erasure works

Restoring an export on a paid Hub, the Hub asks the cloud for the branch's player key backup
(`POST /v1/restore/plan`, the `keys_url`, one download, no retries), opens it with the branch key and
hands the ids to `stage_export_checked`. At the swap, before the writer opens, a player whom the
file holds a key for and the backup does not is erased with the same `erase_player` +
`delete_player_key` as a tombstone, and counted in `ops.restore`'s `erasures_reapplied`. A Free Hub,
no network, no backup or a backup the branch key does not open changes nothing (the Free/offline
warning of Story 9.7 stays). The plan call is only answered for a Hub that replaces another Hub of
the branch with the key released to it; on any other Hub the restore goes on as before.

## Running it by hand

```
ARENA_RESTORE_DRILL_OUT=target/restore-drill cargo test --release -p arena-replay --lib drill
ARENA_RESTORE_DRILL_OUT=target/restore-drill cargo test --release -p arena-store --lib export_one_gigabyte -- --ignored --nocapture
node scripts/restore-drill-report.mjs --dir target/restore-drill
```

`RESTORE_EXPORT_MS` and `RESTORE_DATABASE_MIB` move the export budget for a trial on other hardware.

## Not proven here

The drill's GST archive is the one the Hub builds for the seeded branch's year; the drill seeds no
GST invoices, so the scan covers its fields, not invoice rows. The cloud side of the key backup (what
the presigner lists and signs) is covered by the `arena-cloud` restore-plan tests, not re-run here.
