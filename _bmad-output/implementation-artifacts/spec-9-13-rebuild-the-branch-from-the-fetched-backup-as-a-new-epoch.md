---
title: 'Story 9.13: Rebuild the branch from the fetched backup as a new epoch'
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '3c90ade8d152e80c74d59a8a3085c7429ad862b1'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-7-restore-a-branch-from-an-export-file-as-a-new-epoch.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-12-fetch-a-paid-branch-s-cloud-backup-onto-a-replacement-hub.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A replacement Hub in "Restore this branch" now holds the released branch key (9.10) and a verified staging folder of the cloud backup (9.12), but nothing turns the staged snapshot, batches and key backup back into a working branch, and nothing opens the new epoch.

**Approach:** "Rebuild this branch" replays the staging folder into a new database file off to the side (`arena-replay::rebuild`): the snapshot is unpacked, batches are applied in log order honouring fork points, the player keys come back from the key backup, and the result is checked. That file is then staged exactly like a checked `.a360` restore (`stage_rebuilt` -> `write_export` into `restore-pending/`), the Hub restarts, and the Story 9.7 start-up swap makes it the branch: new epoch forked at the last change replayed, `ops.restore` recorded with the cloud facts.

## Boundaries & Constraints

**Always:**
- Start is refused unless the Hub waits in `restore_pending` on the Windows Hub (`UNSUPPORTED` on Android), is not revoked, is not Free (`FEATURE_NOT_ENTITLED`), holds the released key (`KEY_NOT_RELEASED`) and the 9.12 download is `ready` (else `NOT_FOUND`). Loopback-only routes, no staff session, like 9.12.
- Replay: forks are collected from every batch header first; batches are then applied one per transaction with `replay_history` (resume = the snapshot's last change; a forked epoch starts at `parent_seq + 1`; parent rows above a fork go to `orphaned_changes` and are counted, never applied); every applied change is also written to `changes`. A `seq` gap, a bad row or a delete of a missing row fails the rebuild (`INTERNAL`); a batch that does not decode is `RESTORE_CORRUPT`.
- Keys: the snapshot carries no `player_keys` (Story 8.8). The table is rebuilt from the key backup opened with the released branch key; a key for an erased player, or for a player the database does not hold, is dropped and counted. A Hub with no key backup rebuilds with none.
- Checking: branch rows must exist, `PRAGMA integrity_check` must be `ok`, `state_checksum` is returned, the upload cursor is set to the last replayed change (it is in the cloud already), WAL is folded away.
- The paired devices and kitchen displays of the old Hub are listed in the side table `restore_pair_again`; the export strip blanks their `token_hash`, and at the swap they get `PAIR_AGAIN_TOKEN` (not empty, so not "added by hand"; not a hash, so nothing authenticates) and the table is dropped. The snapshot carries `pair_again` and Settings -> Devices & display says "Pair again" under such a device.
- Staging reuses 9.7: `stage_rebuilt` exports the rebuilt file (licence cache and token hashes stripped), checks it with `inspect_export`, writes the marker last (`cloud` facts in it). `inspect_export` accepts a Hub with no `branch` rows only when its `hub_identity` waits for that branch (`restore_pending = 1`, same `branch_id`). At start `apply_pending` swaps (the waiting shell goes to `orphaned/`), re-applies erasures, writes a provisional epoch above everything seen forked at `{last epoch, last seq replayed}`, keeps this Hub's pairing, licence and tokens, and clears `restore_pending` (the new identity row defaults it to 0). The downloaded staging folder is removed once the swap is applied.
- `ops.restore` carries `cloud: {snapshot_date, first, last, batches, applied, orphaned, keys_restored}` (new optional field, strict, `is_consistent` checked); the actor falls back to the Hub (`restore`) because the replacement Hub has no Owner.
- Screen: the ready panel offers "Rebuild this branch" with an in-place confirmation; then "Replaying 2 of 9" / "Checking 3 of 4" with percent and Cancel; then "restarting". Cancel before the restart removes the rebuilt file and leaves the Hub in "Restore this branch" with the downloaded backup. Failure shows the code and keeps the download. Strings in `packages/ui/src/messages/en.json`.
- Logs carry counts, seq ranges and error codes, never keys, player ids or paths of secrets.

**Never:**
- No change to what 9.10 releases, to the 9.12 download or its staging layout, to the uploader, or to `POST /v1/restore/plan`. No Owner-PIN check (a replacement Hub has none; the key release was the authorisation, as recorded in 9.12). The old Hub's database is never touched.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Drill | snapshot + 4 batches, a fork, 2 orphan rows, an erased player, 4 keys | checksum equals the source's at the last uploaded seq; orphans absent; 2 keys back, 2 dropped | none |
| No snapshot | batches only | replay from seq 1 | none |
| Hole inside an epoch | batch missing | rebuild stops, no database left | `INTERNAL` "seq gap" |
| Damaged batch | bad sha | stops | `RESTORE_CORRUPT` |
| Cancel | any time before staging | work file removed, view cleared, download kept | none |
| No key in keystore | start | job fails | `KEY_NOT_RELEASED`, download kept |
| Not ready / Free / not released / Android / revoked | start | refused | `NOT_FOUND` / `FEATURE_NOT_ENTITLED` / `KEY_NOT_RELEASED` / `UNSUPPORTED` / `TOKEN_REVOKED` |
| Restart | staged | swap, new provisional epoch, `ops.restore`, staging removed | failed apply keeps the live database |
| Paired kiosk | token hash in the backup | `PAIR_AGAIN_TOKEN`, shown "Pair again" | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{restore.rs,restore_plan.rs,snapshot.rs,lib.rs}` -- `CloudRestoreFacts`, `OpsRestore.cloud`, `PAIR_AGAIN_TOKEN`, `RestoreRebuildPhase/View`, `SnapshotDevice.pair_again`; regenerated TS.
- `crates/arena-replay/src/{rebuild.rs,rebuild_tests.rs,audit.rs}` -- `rebuild`, `RebuildSource/Stage/Stop`, `Rebuilt`; the restore drill.
- `crates/arena-store/src/{restore.rs,lib.rs}` -- `live_branch`, `stage_rebuilt`, `PAIR_AGAIN_TABLE`, `pair_again` at the swap, cloud facts in the marker and outcome.
- `crates/arena-core/src/{restore_fetch.rs,lib.rs,snapshot.rs}` -- `RestoreRebuildControl`; `pair_again` in the device snapshot.
- `crates/arena-tasks/src/{restore_fetch.rs,lib.rs}` -- `StagedManifest::read`, exports.
- `crates/arena-hub/src/{restore_rebuild.rs,restore_rebuild_tests.rs,restore.rs,lib.rs}` -- `Rebuilder`, wiring, staging cleanup after the swap.
- `crates/arena-api/src/ui_restore_fetch.rs` -- `/ui/restore/cloud/rebuild` routes.
- `apps/hub/src/lib/{RestoreBranch.svelte,restore-cloud.ts}`, `apps/hub/src/routes/settings/devices-display/+page.svelte`, `packages/ui/src/messages/en.json` -- the screen.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- facts, token, rebuild view, `pair_again`; regenerate TS
- [x] `arena-replay` -- `rebuild` with forks, orphans, keys, checks, progress and cancel; restore drill tests
- [x] `arena-store` -- replacement-Hub branch lookup, `stage_rebuilt`, pair-again at the swap, cloud facts; tests
- [x] `arena-core` / `arena-tasks` -- control trait, manifest reader, device snapshot flag
- [x] `arena-hub` / `arena-api` -- `Rebuilder`, loopback routes, staging cleanup; tests
- [x] Hub screen, strings, Settings -> Devices "Pair again"; Playwright specs

**Acceptance Criteria:**
- [x] Given a complete staging folder, the Hub replays from the snapshot, applies batches in order honouring fork points (parent rows above a fork are skipped and counted as orphaned), unwraps the key backup with the released key and drops any key not in it.
- [x] Given the replay finishes, erased players stay erased (their key is not restored), the new epoch is opened forked at the last epoch and seq replayed, and `ops.restore` is recorded.
- [x] Given paired devices of the old Hub, they are listed under Settings -> Devices & display as "Pair again" with their records, groups and history kept.
- [x] Given replay, the Hub shows "Replaying" and "Checking" with percent, Cancel before the restart leaves the "Restore this branch" state, and the audit record carries the snapshot date, batch range, orphaned count and the new epoch.
- [x] Given the drill (snapshot + batches + a fork + an erased player), the rebuilt branch rows' checksum equals the source's at the last uploaded `seq`, the erased player stays erased and orphaned rows are absent.

## Spec Change Log

## Review Triage Log

## Design Notes

A hot swap would have to rebuild the writer, lease task and every reader that holds the database open, and Windows cannot rename an open file; Story 9.7 solved that by staging and swapping at start-up. The rebuild reuses it whole: the rebuilt file is exported into `restore-pending/` so the checks, the provisional epoch and fork, identity/licence/token keeping, the orphaning of the old file and the `ops.restore` record are the same code, tested once. Rebuilding on a blocking task beside the live (empty) database means Cancel is only a flag and a delete.

The epoch is the provisional-then-confirm path of 9.7 (the first heartbeat settles it through the fence, Story 9.3), not a separate `POST /v1/fence/claim` call: the replacement Hub may be offline at this moment, and the fence already refuses a Hub that lost the race.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-replay -p arena-core -p arena-tasks -p arena-api -p arena-hub`, `cargo clippy` on the touched crates `--all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run gen:types`, `check:strings`, `check:layers`, `check:protocol`, `format:check`, `pnpm --filter @arena/hub check`, Playwright `restore-pending` and `devices-display`

## Auto Run Result

Status: review

Implemented per spec. Verification results are recorded at the foot of this file after the final run.

Judgement calls and risks:
- The new epoch is the provisional epoch of Story 9.7 (confirmed by the first heartbeat), not a direct `fence/claim` call; the acceptance line "claims a new epoch (Story 9.3)" is met through the heartbeat's fence move.
- Re-pairing a restored kiosk goes through the existing 6-digit code, which registers a new device record; the old record stays listed as "Pair again" until staff revoke it. Adopting the old record on re-pair needs the kiosk to name its old device id and is not done here.
- Like 9.12, the rebuild has no Owner PIN (a replacement Hub has no Owner PIN yet); the confirmation is in place on the loopback-only screen and the key release (step-up MFA, Story 9.10) is the authorisation.
- A hole between two epochs' batches is not detected (a forked epoch starts at `parent_seq + 1` by definition); holes inside one epoch are.
- The rebuilt file is exported with `write_export`, so it is compressed and hashed once more than strictly needed; it keeps the 9.7 staging path unchanged.
- `epoch.changed` is not announced for a restore applied at start (UIs reconnect after the restart), as in 9.7.

## Verification Results

- `cargo fmt --check`, `cargo clippy` (arena-proto/store/replay/core/tasks/api/hub, `--all-targets -D warnings`): clean.
- `cargo test` on the same crates: all pass. One `proptest_apply` run failed once on the notify-outbox invariant (`owner_alert` vs `transactional`, a random case in code this story does not touch) and passed on rerun.
- `gen:types`, `check:strings`, `check:layers`, `check:protocol`, `format:check`: ok.
- `pnpm --filter @arena/hub check`: only 2 pre-existing errors in `playwright.kitchen.config.ts` (`process` types), none in touched files.
- Playwright `restore-pending` and `devices-display`: 51 passed, 1 skipped.
