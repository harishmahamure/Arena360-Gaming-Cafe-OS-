---
title: "Story 10.16: Owner starts moving a Free Android Hub to a Windows Hub after upgrading"
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '4391fdc1'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-15-full-refund-within-7-days-of-the-first-payment-issued-throug.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** An owner who upgraded a Free Android café to a paid plan cannot start moving it to a Windows PC, so standby and Tunnel stay out of reach.

**Approach:** Account -> Branches offers "Move to a Windows Hub" on a paid branch whose main Hub is Android. Starting it writes a `hub_migrations` row and a `migrate` pairing code. A fresh Windows install pairs with the code beside the Android Hub (whose token and data are untouched) and waits; the Android Hub's next heartbeat carries the move and shows staff a persistent item. The owner can cancel before the handover.

## Boundaries & Constraints

**Always:** the move needs a paid plan and a live Android main Hub; a `migrate` code pairs only a Windows install and only through the migration path; the Android token, fence and data are never touched here; the PC's token cannot beat as the main Hub or promote; cancelling revokes the PC's token and kills the code; the Windows Hub serves nothing while it waits.

**Never:** no handover, snapshot, epoch claim, key release or kiosk follow here (Stories 10.17-10.19); no Hub schema change (the wait flag is `hub_meta`); no change to licence or fence wire shapes beyond the optional `migration` field on the heartbeat answer.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Start on Free / Windows Hub / no Hub | `POST /v1/branches/{id}/hub-migration` | `TIER_LOCKED` / `PLATFORM_UNSUPPORTED` / `STEP_NOT_READY`, nothing written |
| Start | paid, Android main Hub | 200 code; row `awaiting_windows`; card shows the code |
| Start again while waiting | | fresh code, old one invalid, still one row |
| Start after the PC paired | | `HUB_MIGRATION_ACTIVE` (409) |
| Pair, Windows, valid code | | token + licence, no bootstrap, `migration: true`; row `awaiting_handover` naming the PC; Android token valid |
| Pair, Android install | `migrate` code | `FEATURE_NOT_ENTITLED` |
| Pair, wrong / used / expired | | Story 2.10 errors |
| Android heartbeat | `awaiting_handover` | `migration {id, target_hub_name}`; item "Move this branch to <PC>" |
| PC beats as main / claims fence | | `FORBIDDEN` |
| Cancel | before or after the PC paired | 204; row `cancelled`; PC token revoked; code dead; Android item clears next heartbeat; a new move can start |
| Another owner | start / cancel | 403 |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0044_hub_migration.sql` (`hub_migrations`, `hubs.migration_id`, `pairing_codes.purpose`, live-code index per role and purpose), `src/control/hub_migration.rs` (start, pair, cancel, reads), `src/control/pairing.rs` (`CodePurpose`; Hub-code paths ignore `migrate`), `src/control/hubs.rs` (`migration_id`; standby reads and replacement ignore the PC).
- `crates/arena-cloud/src/api/hub_migration.rs` (start, cancel, `migration_of`), `pair.rs` (`pair_migration`, Android refusal), `heartbeat.rs` (`migration_notice`), `fence.rs` (PC cannot promote), `branches.rs` (`OwnerBranch.migration`).
- `crates/arena-proto` (`OwnerMigration`, `MigrationPhase`, `MigrationNotice`, `HubPairRequest.hub_name`, `HubPairResponse.migration`, `HeartbeatResponse.migration`, `HubSnapshot.migration_wait` / `move_notice`, `ErrorCode::HubMigrationActive`, generated TS).
- `crates/arena-core` (`HubStatus.migration_wait` / `move_notice`, `record_migration_pairing`, writer gating), `arena-store` (`MIGRATION_WAIT_KEY`, `Store::migration_wait`), `arena-tasks` (`CloudPairer::pair_for_migration`, `with_hub_name`, heartbeat: no beat while waiting, `move_notice`), `arena-api` (LAN gating, snapshot), `arena-hub` (initial status, host name).
- `apps/owner/src/lib/BranchCard.svelte`, `api.ts` (DELETE), `apps/hub/src/lib/AwaitAndroid.svelte`, `alerts.svelte.ts`, `routes/+layout.svelte`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-cloud/tests/pair.rs` (Story 10.16 section), `arena-core` apply tests, `arena-tasks` heartbeat tests, `apps/owner/tests/move.spec.ts`, `apps/hub/tests/{migration-wait,alerts}.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0044 and the store: moves, `migrate` codes, PC pairing, cancel, standby reads that ignore the PC
- [x] Cloud: start / re-issue / cancel, pairing with a `migrate` code, heartbeat `migration`, no main-Hub calls from the PC
- [x] Proto types, error code, generated TS
- [x] Owner app: Move block with steps, tier-lock, code card, waiting / paired / cancel states
- [x] Windows Hub: pairing for a move, persistent wait flag, serves nothing, "Waiting for your Android Hub", no heartbeat
- [x] Android Hub: persistent "Move this branch to <PC name>" alert item from the heartbeat answer
- [x] Tests for every I/O row; README

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api -p arena-hub -p arena-kiosk-core --all-targets --features arena-store/control -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-cloud -p arena-core -p arena-tasks -p arena-api -p arena-kiosk-core -p arena-hub --features arena-store/control` -- all pass except baseline `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`, `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` (needs 8 days of uptime) and `printer::lan::tests::a_printer_that_stops_reading_times_out`, which fail on the clean tree too
- `pnpm --filter @arena/owner check`; `playwright test tests/move.spec.ts tests/standby.spec.ts tests/branches.spec.ts` (owner) and `tests/migration-wait.spec.ts tests/alerts.spec.ts tests/restore-pending.spec.ts` (hub) pass
- `pnpm run check:strings`, `check:hex`, `check:tier-lock`, `check:secrets`; `check:generated` passes once the regenerated files are committed

## Auto Run Result

Status: in-review. Risks to know: the PC's `hubs` row uses the standby slot (role `standby`, `migration_id` set) because the role CHECK cannot be widened without rebuilding `hubs`; every standby read that must not see it filters `migration_id IS NULL`, so a new query on `role = 'standby'` must do the same. A Windows Hub that waits for a move refuses intents with `RESTORE_PENDING` (the existing wait gate) and does not heartbeat, so its licence is not refreshed until Story 10.18 (a standby-style licence refresh is available to its token but not wired). The Windows pairing and the Android item are covered by unit and browser tests; no end-to-end Hub binary test was run because `crates/arena-hub/tests/pairing.rs` (feature `dev-fixtures`) no longer compiles on the clean tree. The disk filled during verification; `target/debug/incremental` was deleted and tests ran with `CARGO_INCREMENTAL=0`.

## Code Review Result

Reviewed 2026-10-31. No blocking findings. fmt, clippy, Rust tests, owner/hub browser specs and string/hex/tier-lock/generated checks pass. Failing tests not tied to this change: `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`, `backup::a_hung_run_times_out_with_its_own_code` (zero-duration timeout racing a fast run), and `@arena/hub check` errors in `playwright.kitchen.config.ts` (`process` types, file untouched). Notes: a migration whose Android Hub is later replaced keeps a stale `from_hub_id` (the owner can still cancel); standby code issue is blocked while the migration PC holds the standby slot (intended).
