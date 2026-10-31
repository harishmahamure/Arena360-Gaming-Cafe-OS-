---
title: "Story 10.18: The Windows Hub imports the snapshot as the next epoch"
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: 'eea81774'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-17-the-android-hub-hands-over-a-final-snapshot-and-steps-down.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-7-restore-a-branch-from-an-export-file-as-a-new-epoch.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** A Windows PC that holds the tablet's final snapshot (Story 10.17) cannot use it: nothing imports the file, the branch fence still belongs to the tablet, the PC has no branch key, and the cloud still treats the tablet as the branch's Hub.

**Approach:** The PC asks the owner for the branch key as a replacement Hub does (the owner's existing "Release branch key to <PC name>" card, step-up MFA). With the key held, "Import the branch" claims the next epoch from the cloud (`fork = {snapshot epoch, last seq}`), stages the snapshot through the Story 9.7 restore machinery with that epoch, and restarts. The restarted Hub serves under the claimed, confirmed epoch and tells the cloud, which makes the PC the main Hub and revokes the tablet in one transaction.

## Boundaries & Constraints

**Always:** the import needs the branch key on this PC and a snapshot that checked out (hash, branch, header `purpose: migrate`); the epoch is issued only by the fence compare-and-set, only to the Windows Hub of a move whose tablet reported the handover, and only while the fence is still the snapshot's epoch; asking again for the epoch the PC already holds answers it again; the tablet's token is revoked only when the PC reports it serves; nothing is set aside; the PC's own pairing, licence and device tokens survive the swap.

**Never:** no `hub.successor` or kiosk re-pin (Story 10.19); no change to the `.a360` format; no new Hub schema; no heartbeat by the PC before the cloud made it the main Hub.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Key not released | Import tapped | `STEP_NOT_READY`, nothing asked of the cloud, nothing staged |
| Claim before the tablet handed over | | `STEP_NOT_READY` |
| Claim, fence still the snapshot's epoch | | fence `<n+1>:<PC instance>`, fork seq stored, same answer if repeated |
| Claim, fence moved on | | `FENCED`, nothing staged |
| Staged and restarted | | database = snapshot, lease = claimed epoch (confirmed), `migration_report` set, no orphan |
| PC reports | fence is its epoch | tablet revoked, PC `main`, move `done`; repeated report answers again |
| Report with another epoch / from the tablet | | `FENCED` |
| Snapshot from a file | `purpose: migrate`, this branch | kept and shown as received |
| Ordinary export given as the snapshot | | `RESTORE_CORRUPT` |
| Key request before the handover | | `STEP_NOT_READY` |

</intent-contract>

## Code Map

- `crates/arena-proto/src/handover.rs` (claim / imported / file / importing types, paths, `MIGRATION_REPORT_KEY`), generated TS.
- `crates/arena-store/src/restore.rs` (`stage_migration`, `Marker.migrate`, claimed epoch in `build_restored`, `live_branch` for a waiting PC), `lib.rs` (`migration_report`), `control/hub_migration.rs` (`claim_migration_fence`, `finish_hub_migration`, `live_migration_to_hub`), `control/hubs.rs` (`branch_fence_holder`), `control/key_requests.rs` (a move's Windows Hub counts as a live Hub).
- `crates/arena-cloud/src/api/migration_import.rs` (`/v1/migration/claim`, `/v1/migration/imported`), `heartbeat.rs` (`authenticate_key_holder`), `kek.rs` (request and fetch for the move's Hub).
- `crates/arena-core/src/status.rs` (`wants_branch_key`, `migration_report`), `handover.rs` (`MigrationImportControl`).
- `crates/arena-tasks/src/migration_import.rs` (`MigrationImporter`, `ImportReporter`), `restore_wait.rs` (asks when `wants_branch_key`), `heartbeat.rs` (no beat while a report is owed), `cloud.rs`.
- `crates/arena-api/src/ui_migration.rs` (`/ui/migration/file`, `/ui/migration/import`), `health.rs`; `crates/arena-hub/src/lib.rs` (wiring).
- `apps/hub/src/lib/{AwaitAndroid.svelte,handover.ts}`, `routes/+layout.svelte`, `packages/ui/src/messages/en.json`.
- Tests: `arena-store` `restore::tests`, `arena-cloud/tests/pair.rs` (Story 10.18 section), `arena-api/tests/handover.rs` (importer), `arena-tasks` `restore_wait::tests`, `arena-core` `status::tests`, `apps/hub/tests/migration-wait.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: claim / imported / file / importing types and paths; regenerate TS
- [x] Store: `stage_migration` and the migrate path of the restore swap (claimed confirmed epoch, report marker, no orphan); control-plane claim and finish transactions; key requests for the move's Hub
- [x] Cloud: `/v1/migration/claim`, `/v1/migration/imported`; key request and fetch for the move's Windows Hub after the handover
- [x] Hub: `MigrationImporter` (key gate, claim, stage, restart), `ImportReporter`, no beat while a report is owed, key request when a snapshot is held, file route
- [x] Hub app: import button with the key state, file form, refusal text
- [x] Tests for every I/O row; README

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api -p arena-hub -p arena-kiosk-core --all-targets --features arena-store/control -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api -p arena-kiosk-core -p arena-hub --features arena-store/control`
- `pnpm gen:types`; `pnpm --filter @arena/hub check` (only the existing `playwright.kitchen.config.ts` `process` errors); `playwright test tests/migration-wait.spec.ts tests/handover.spec.ts` (hub)
- `pnpm run check:strings`, `check:hex`, `check:tier-lock`, `check:secrets`, `check:no-blur`; `check:generated` passes once the regenerated files are committed

## Auto Run Result

Status: review. Deviations and risks to know: (1) the PC reports the import with a dedicated `POST /v1/migration/imported` call, not a heartbeat field, because the cloud refuses a standby token at `/v1/heartbeat`; the effect (tablet revoked, PC main, move `done`) is the same transaction the story describes. (2) The cloud moves the fence at the claim, before the PC has restarted; the tablet's heartbeat is fenced from then on (it is already stopped), and a failed import is repeated safely because the claim is idempotent for the epoch the PC holds. (3) The row-count / checksum comparison of the two databases and the "first batch after upgrade" check are not built; the import relies on the snapshot's SHA-256 and the restore's schema migration. (4) The restart after the import (`relaunch` or a supervisor) is the Story 9.7 mechanism and is not covered by an end-to-end test; the swap itself is covered in `arena-store`, and the claim / stage / restart request in `arena-api/tests/handover.rs`. (5) The owner app still shows the move as "awaiting handover" until it is `done`; its later phases belong to Story 10.20. (6) No end-to-end Hub binary test of tablet to PC to cloud was run.

Verify gate re-run (2026-10-31): fmt, clippy (`-D warnings`), the string / hex / generated / tier-lock / secrets / blur checks and the hub `check` (only the known `playwright.kitchen.config.ts` errors) pass. In the Rust test run, `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token` fail on this machine and `price_book::a_draft_is_copied_edited_and_published_with_a_plan_per_row` fails intermittently. None of those files or the code they exercise was touched by this story; all else passes.

## Code Review Result

Status: done (2026-10-31). Reviewed the cloud claim / finish transactions, key-holder auth, the restore migrate path, the importer and the reporter against the I/O matrix: no blocking findings, no code changes. Fmt, clippy -D warnings, the repo checks and the hub playwright specs pass. Rust test failures on this machine are environmental and untouched by the story: arena-cloud binary unreachable_at_start..., arena-tasks trusted_time_is_cloud_time_plus_monotonic_elapsed (needs 8 days of uptime for Instant subtraction) and printer::lan a_printer_that_stops_reading_times_out (Windows socket buffer).
