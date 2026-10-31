---
title: "Story 10.17: The Android Hub hands over a final snapshot and steps down"
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: 'd0765fec'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-16-owner-starts-moving-a-free-android-hub-to-a-windows-hub-afte.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** A paid branch whose Windows PC has paired for a move has no way to take the branch over: the tablet cannot be told to stop, nothing it wrote can reach the PC, and a failed transfer has no way out.

**Approach:** A manager confirms the move on the tablet with a PIN and reads a 6-digit code. The PC types the tablet's address and the code; a good claim stops the tablet (every intent `HUB_HANDING_OVER`), drains its writer queue and writes a final `.a360` snapshot (`purpose: migrate`). The PC pulls it over TLS, checks it and acknowledges; the tablet then shows only "This branch has moved to <PC>" and reports `handed_over` in its next heartbeat. Without a LAN path the manager writes the snapshot to the backup folder; "Cancel move" resumes the tablet as main.

## Boundaries & Constraints

**Always:** the PIN (an Owner's or Manager's, `ops.promote`) confirms start, file and cancel; the code never crosses the wire (each side proves it for the TLS certificate it saw); after a good claim no intent is applied and the snapshot holds the last change the tablet writes; the epoch and change log are never touched, so a cancel leaves the Hub as it was; a stopped Hub stays stopped across a restart until the PC acknowledges or the manager cancels.

**Never:** no import, epoch claim or branch-key release on the PC (Story 10.18) and no `hub.successor` for kiosks (Story 10.19); no Hub schema change (the state is `hub_meta`); no change to licence or fence wire shapes beyond the optional `handed_over` heartbeat field.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Confirm | PIN of Owner/Manager, a move asked for | 6-digit code, 15 min; asking again shows the same code |
| Confirm, no move asked for / wrong PIN | | `VALIDATION_FAILED` / `PIN_REJECTED`, nothing starts |
| Claim, right proof | correct code and certificate | Hub stops, drains, snapshot written, transfer token answered |
| Claim, wrong code or another certificate | | `PAIR_CODE_INVALID`, counted per code and per address; Hub still serves |
| Sixth wrong attempt | | code invalidated, new one shown (`code_replaced`) |
| Claim after expiry / while handing over | | `PAIR_CODE_EXPIRED` / `HUB_HANDING_OVER` |
| Pull and ack | token, matching hash/branch/seq | tablet `Moved`; next heartbeat reports `handed_over`; cloud move `handed_over` |
| Pull fails the checks | size, hash, branch, seq, epoch, purpose | PC deletes the file and does not acknowledge; tablet stays stopped |
| Use a file | PIN, backup folder set | snapshot in the backup folder, tablet stays stopped; no folder: `VALIDATION_FAILED` on `folder` |
| Cancel move | PIN, before `Moved` | tablet serves again, same epoch; snapshot and written file deleted |
| Owner cancels in the cloud | code showing / already stopped | handover ends at the next heartbeat / waits for the manager's Cancel |
| Restart mid-handover | | still stopped |

</intent-contract>

## Code Map

- `crates/arena-proto/src/handover.rs` (paths, phases, `HandoverView`, claim/ack/refusal types, `HandedOver`), `backup_export.rs` (`ExportHeader.purpose`), `error.rs` (`HUB_HANDING_OVER`), `pairing.rs` (`HeartbeatRequest.handed_over`), `snapshot.rs` (`HubSnapshot.handover`), generated TS.
- `crates/arena-core/src/handover.rs` (`Handover`, `handover_proof`, `HandoverJoinControl`), `writer.rs` (`barrier`, `record_meta`, gating in `ctx_from`), `apply/mod.rs` (`ApplyCtx.handing_over`), `status.rs`, `snapshot.rs` (`hub_branch_id`).
- `crates/arena-store/src/export.rs` (`write_export_for`, handover state stripped from the copy), `control/hub_migration.rs` (`mark_migration_handed_over`).
- `crates/arena-cloud/src/api/heartbeat.rs` (records `handed_over`).
- `crates/arena-api/src/lan/handover.rs` + `listener.rs` (claim, snapshot, ack on `:7401` TLS), `lan/mod.rs` (`with_handover`, `HUB_HANDING_OVER` at `hello`), `ui_handover.rs` (start / file / cancel), `ui_migration.rs` (`POST /ui/migration/join`, loopback).
- `crates/arena-tasks/src/handover_pull.rs` (the PC's claim, pull, checks, ack), `heartbeat.rs` (reports `handed_over`, ends an unclaimed code), `crates/arena-hub/src/lib.rs` (wiring).
- `apps/hub/src/lib/{MoveBranch,HandoverStopped,AwaitAndroid}.svelte`, `handover.ts`, `AlertList.svelte`, `routes/+layout.svelte`, `packages/ui/src/messages/en.json`.
- Tests: `arena-core` `apply/tests/handover.rs`, `arena-api/tests/handover.rs` (tablet and PC over loopback TLS), `arena-cloud/tests/pair.rs` (Story 10.17 section), `apps/hub/tests/handover.spec.ts`, `alerts.spec.ts`, `migration-wait.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: handover types and paths, `ExportHeader.purpose`, `HUB_HANDING_OVER`, `handed_over` heartbeat field, `HubSnapshot.handover`, generated TS
- [x] Core: `Handover` state machine (begin, claim, snapshot_for, acknowledge, use_file, cancel, on_notice), proof, writer barrier and gating, persistence in `hub_meta`
- [x] Store: `write_export_for` with a purpose; handover state stripped from the copy
- [x] LAN: claim, snapshot and ack routes (TLS only), per-code and per-address limits, `hello` refused while stopped
- [x] Hub UI routes: start, file, cancel with the manager's PIN; loopback join on the Windows Hub
- [x] Windows Hub: puller (claim, pull, size/hash/branch/seq/epoch/purpose checks, ack), received state
- [x] Cloud: `handed_over` moves the move on once and only for its Android Hub
- [x] Hub app: move panel with code and address, stopped / moved screens, join form and received state
- [x] Tests for every I/O row; README

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api -p arena-hub -p arena-kiosk-core --all-targets --features arena-store/control -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api -p arena-kiosk-core -p arena-hub --features arena-store/control` -- all pass except the baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`, `backup::a_hung_run_times_out_with_its_own_code`, `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`, plus `ui_auth::tests::lock_and_unlock_log_the_staff_ids_and_never_the_pin`, which failed once in a full run and passes alone and on the clean tree (shared log capture)
- `pnpm --filter @arena/hub check` (only the existing `playwright.kitchen.config.ts` `process` errors); `playwright test tests/handover.spec.ts tests/migration-wait.spec.ts tests/alerts.spec.ts` (hub) pass
- `pnpm run check:strings`, `check:hex`, `check:tier-lock`, `check:secrets`, `check:no-blur`; `check:generated` passes once the regenerated files are committed

## Auto Run Result

Status: review. Risks to know: (1) the handover claim authenticates by the code bound to the TLS certificate, not by a pinned certificate, because the PC has never seen the tablet's; a six-digit code is guessable offline by someone who records a claim, so the 15-minute code lifetime, five tries per code and 30 wrong claims per address per 15 minutes are the real limits. (2) A tablet that has stopped refuses every intent, so a manager who signs out cannot sign back in with the intent path; Cancel move and Use a file need a session that already exists on the tablet (the screen disables them with "Sign in as an Owner or Manager first" when there is none). (3) The snapshot blanks device token hashes like every export, so kiosks pair again until Story 10.19. (4) After `handed_over` the owner app still reads the move as "awaiting handover" until Stories 10.18/10.20 add the later phases. (5) No end-to-end Hub binary test was run: `crates/arena-hub/tests/pairing.rs` (feature `dev-fixtures`) did not compile before this story; the exchange is covered by `arena-api/tests/handover.rs` over a real TLS listener. The disk filled during verification; stale test binaries under `target/debug/deps` were deleted and tests ran with `CARGO_INCREMENTAL=0`.
