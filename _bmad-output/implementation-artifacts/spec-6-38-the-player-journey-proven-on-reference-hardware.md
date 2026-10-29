---
title: 'Story 6.38: The player journey proven on reference hardware'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '6522606'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-39-pos-and-money-proven-under-load-offline-and-on-real-hardware.md'
warnings: []
deferred:
  - 'Every number on the reference i3, the 30-minute outage, the 20 real-account launcher sessions on Windows 10 and 11, and the shell-phase memory run need the lab rig and its repository variables (docs/runbooks/journey-bench.md); none ran here. Locally the Hub legs (3-12 ms), the UI legs (4-100 ms), a 5-second outage and the report merge were run and pass.'
  - 'The Hub and UI legs are timed separately and summed by the report: the UI leg runs on the stubbed shell, so a real Tauri/WebView2 round trip to the service is inside neither number. A rig driver that clicks through the installed kiosk (WebView2 remote debugging) would close it.'
  - 'The shell-phase memory run needs the lab to start a session through JOURNEY_START_SESSION_CMD; the Hub has no dev command for it. Without the variable the shell result is missing and the report fails.'
---

## Intent

**Problem:** Nothing measures the whole player experience on a cheap PC: login time, how fast the shell opens, how fast Extend and a kiosk order reach the floor, the kiosk's memory, that no launcher login survives a session, and that a 30-minute Hub outage never interrupts play.

**Approach:** Proof only, no product change. A Hub-side timed run of Journey 3 over the real LAN protocol, a UI-side timed run of the same journey in the kiosk's Playwright suite, a real-clock 30-minute Hub-outage test, a launcher-residue checker with a 20-session driver, a shell-phase kiosk memory check, one release report that merges every timing against the story's budgets, and release-bench jobs that run all of it on the reference rig.

## Boundaries & Constraints

**Always:** budgets are env-overridable and default to the story's numbers (login 3000 ms, shell open 2000 ms, Extend 2000 ms, order on the tile 3000 ms, kiosk 80 MB); a missing timing fails the report.

**Never:** a change to `apply()`, a schema, an intent or a player-facing string.

## Tasks

- [x] `crates/arena-api/tests/lan.rs`: Journey 3 over the LAN (login, start, Extend, order on the tile) timed and written to `ARENA_JOURNEY_OUT`
- [x] `apps/kiosk/tests/journey.spec.ts`: attract → login → plan picker → Home → launch → warning → Extend → grace → clean-up → attract, each leg timed
- [x] `crates/arena-kiosk-svc/src/cleanup/tests.rs`: real-clock Hub outage (`ARENA_HUB_DOWN_SECS`, default 1800)
- [x] `scripts/launcher-residue.mjs` (+ test): residue scan and the N-session driver
- [x] `scripts/bench/kiosk-idle.ps1`: `-Phase` and `-OutJson`
- [x] `scripts/journey-report.mjs` (+ test): merge the timings, check the budgets, write the release report
- [x] `.github/workflows/release-bench.yml` and `docs/runbooks/journey-bench.md`

## Acceptance Criteria

- Given the reference PC and a Hub on the LAN, when Journey 3 runs, then login < 3 s, the shell opens < 2 s after approval, Extend reaches the floor ≤ 2 s and the PC returns to attract after clean-up, each timing in the release report.
- Given an order placed from the kiosk, then it is on the Hub tile ≤ 3 s.
- Given the kiosk service and app idle on attract and in the shell, then together they use < 80 MB, else the job fails the release.
- Given 20 consecutive sessions on Windows 10 and 11 with real accounts, then no later session sees a previous login or saved credential; any residual fails the release.
- Given the Hub stopped for 30 minutes mid-session, then play is not interrupted, the lock lands on time and everything reconciles on restart.

## Verification

- `cargo test -p arena-api --test lan` (89 pass, incl. `the_hub_side_of_journey_3_meets_its_timings`); `cargo test -p arena-kiosk-svc --lib a_real_thirty -- --ignored` with `ARENA_HUB_DOWN_SECS=5`; `pnpm exec playwright test journey.spec.ts` in `apps/kiosk`; `node --test scripts/journey-report.test.mjs scripts/launcher-residue.test.mjs` (21 pass); `pnpm --filter @arena/kiosk check`; `cargo fmt --check`; `cargo clippy -p arena-api --tests -- -D warnings`; `pnpm run format:check`, `check:strings`, `check:pins`, `check:secrets`, `check:no-blur`, `check:hex`.
- Pre-existing and unchanged by this story: 14 `arena-kiosk-svc` lib tests and 12 `scripts/*.test.mjs` tests fail on this Windows machine with or without it; `cargo clippy -p arena-kiosk-svc --all-targets` reports unused imports in `tests/discovery.rs` and `launchers/tests.rs` (Unix-only files).
