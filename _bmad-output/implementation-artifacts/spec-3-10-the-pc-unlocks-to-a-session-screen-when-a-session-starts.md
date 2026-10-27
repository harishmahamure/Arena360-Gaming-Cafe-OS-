---
title: 'Story 3.10: The PC unlocks to a session screen when a session starts'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_commit: 'f7fdf85'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-9-start-a-postpaid-session-for-a-player-or-a-guest.md'
deferred:
  - 'Prepaid remaining time (3.24) and pause/end lock pushes (3.11, 3.15, 3.26) are not sent by the Hub yet; the kiosk already renders every lock reason.'
  - 'Ticks come from a 5 s Hub interval over live sessions; Story 3.23 replaces it with the timer engine.'
  - 'The under-10 s start on reference hardware is not measured here.'
---

## Intent

**Problem:** A started session leaves the PC locked. The kiosk has no session screen, no lock reasons and no way to confirm an unlock.

**Approach:** The Hub pushes `session.started` (carrying the idempotency key as `cmd_id`) to the PC's kiosk after `session.start` commits, and `session.tick` every 5 s for each live session. Both carry the device name, elapsed time, `lock_after_ms` (until the next closing time from the branch opening hours and timezone, 24 h without hours) and `time_up_mode`. The kiosk service clears the cover, shows the session screen from the `pc_screen` channel, counts on its monotonic clock, locks with "Time's up" at `lock_after_ms` without writing anything, and acks with `kiosk.pc_ack`. The tile sheet waits for that ack for 5 s and otherwise shows "Failed — retry"; Retry resends the same key and the Hub pushes again.

## Tasks

- [x] `arena-proto`: `SessionScreen`, `LockReason`, `HubEvent::SessionStarted` / `SessionTick`, optional `reason` on `pc_command`; generated types
- [x] `arena-core`: `session_screens` (lock time from opening hours), readers on `DeviceDirectory` and `SnapshotReader`
- [x] `arena-api`: push on start in `run_intent`, 5 s tick in the LAN fan-out, staff lock carries `locked_by_staff`
- [x] `arena-kiosk-svc` / `arena-kiosk-core` / Tauri bridge: session state, lock-time task, reasons and session in `pc_screen`
- [x] Kiosk UI: session screen, reason lines in the cover; strings in `en.json`
- [x] Hub UI: `session.start` on a paired PC waits for the kiosk ack
- [x] Tests: Rust unit and integration, kiosk and hub Playwright

## Verification

- `cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace`
- `pnpm run check:protocol && pnpm run check:strings`; `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`
- `playwright test` in `apps/kiosk` and `tests/session-start.spec.ts tests/tile-sheet.spec.ts` in `apps/hub`
