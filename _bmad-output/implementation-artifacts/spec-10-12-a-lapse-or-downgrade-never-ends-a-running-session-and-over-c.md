---
title: "Story 10.12: A lapse or downgrade never ends a running session, and over-cap devices go read-only"
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: 'c963f5c8'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-6-a-paid-licence-reaches-the-hub-within-one-heartbeat-and-staf.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** When a licence drops (lapse to Free, Premium to Basic, add-ons removed, or an expiry on trusted time) the Hub keeps every device startable, so a branch can run more devices than its plan allows.

**Approach:** The system intent that applies a licence (`licence.apply`) also recomputes device standing in the same transaction: live devices ordered by pairing, the oldest up to `licence.device_cap` stay active, the rest are marked `over_cap` (newest pairing first). A marked device keeps its tile and any running session; new starts and transfers onto it are refused `DEVICE_OVER_CAP`. A revoke or a higher licence recomputes and frees the oldest first.

## Boundaries & Constraints

**Always:** the recompute reads and writes no session or order; only a changed flag is written, as a system `devices_meta` change row (the audit record of which devices were marked); the same code runs for a Free licence from a lapse and for an expiry on `trusted_now` (the existing 2.12 path applies the Free limits through `licence.apply`); the tile stays on the floor.

**Never:** no new intent kind; no change to the licence or heartbeat wire shapes; no handling of open orders, kitchen tickets, staff sign-ins or the standby (10.13).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Cap drops below live devices | `licence.apply` with `device_cap` < live devices | newest pairings beyond the cap get `over_cap = 1`; one system change row per device; sessions untouched |
| Session on a marked device | active, paused or grace session | extend, pause, transfer away and end work as before; only when idle does the kiosk show "over the plan's device limit" |
| Start or transfer onto a marked device | `session.start` / `session.transfer` | `DEVICE_OVER_CAP` on `device_id`; nothing written; tile shows "Over plan limit" with Upgrade |
| Revoke or higher cap | `device.revoke` / `licence.apply` | recompute; the oldest marked devices become active first |
| Paid licence held past `exp` | `licence.apply` with Expired standing and Free limits | same recompute against Free limits |
| Cap not reached | any licence | no flag changes, no extra change rows |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0092_device_over_cap.sql`, `src/devices.rs` (`DeviceMetaRow.over_cap`, `live_devices_by_age`, `set_device_over_cap`), `migrations.rs`.
- `crates/arena-core/src/apply/standing.rs` (the recompute), called from `licence.rs` and `devices.rs::mutate_revoke`; refusals in `session.rs`, `transfer.rs`; undo guards in `session.rs`, `force_end.rs`; `player_plans.rs`; `attract.rs`; `snapshot.rs`.
- `crates/arena-proto/src/{error,attract,snapshot}.rs` (+ generated TS): `DEVICE_OVER_CAP`, `AttractState::OverCap`, `SnapshotDevice.over_cap`; `arena-api/ui_auth.rs`, `arena-cloud/api/error.rs`, `arena-kiosk-core/queue.rs` map the new code.
- `apps/hub/src/{routes/floor/+page.svelte,lib/TileSheet.svelte,lib/tile-actions.svelte.ts}`, `packages/ui/src/components/TierLock.svelte` (optional `label`), `packages/ui/src/messages/en.json`, `apps/kiosk/src/lib/{Attract.svelte,bridge.ts}`.
- Tests: `arena-core` `apply::tests::over_cap`, `apply::tests::attract`, `tests/proptest_apply.rs` (`DeviceCapLicence`); hub `maintenance.spec.ts`; kiosk `attract.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0092 and store reads/writes for the `over_cap` flag
- [x] Standing recompute inside `licence.apply` and `device.revoke`, oldest pairings stay active, one system change row per flag change
- [x] `DEVICE_OVER_CAP` on `session.start` and `session.transfer`; undo of end and force-end refuses an over-cap device; the kiosk plan picker offers none
- [x] Snapshot field, floor tile "Over plan limit", sheet `tier-lock` with Upgrade and no start form
- [x] Idle kiosk shows "This PC is over the plan's device limit — ask staff." and offers no login
- [x] Tests for every I/O row and a proptest op that interleaves licence-cap changes with session/order intents; README

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-cloud -p arena-kiosk-core --all-targets -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub -p arena-tasks -p arena-kiosk-core -p arena-replay`
- hub `npx playwright test tests/maintenance.spec.ts`; kiosk `npx playwright test tests/attract.spec.ts tests/locked.spec.ts`; `svelte-check` (kiosk clean; hub only the `playwright.kitchen.config.ts` `process` type error that predates this story)
- `pnpm run check:strings`, `check:hex`, `check:tier-lock`, `check:secrets`; `check:generated` passes once the regenerated files are committed

## Auto Run Result

Status: review. See Verification for what ran. Test results are recorded in the commit's run; baseline Windows failures listed in Story 10.6/10.11 are unchanged.

Risks to know: an existing Hub with more devices than its cap is marked at the next licence change, not at upgrade time; the order of pairing is `devices_meta.created_at` then row order (same-millisecond pairings fall back to insertion order); `arena-e2e` and kiosk-svc discovery tests are Unix-only and were not run.

## Code Review

Reviewed the implementation commit; no blocking findings in the story's code. Fixed one pre-existing test defect: the proptest notification invariant treated Story 8.34 `owner_alert` outbox rows as player messages and failed intermittently (`proptest_apply.rs`). Fmt, clippy, unit/proptest, hub and kiosk Playwright specs, and string/tier-lock/generated checks pass.
