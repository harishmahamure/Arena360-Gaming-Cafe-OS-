---
title: 'Story 3.37: Floor Invariants, Throughput and Offline Proof'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_revision: 'eb37c0c'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
warnings: []
deferred:
  - 'The real-Hub, no-route-out run of the Epic 3 flows (`scripts/wan-down/run.sh` is Linux netns only and was not run here).'
  - 'Kiosk unlock timing on reference hardware; the Playwright bench stubs the Hub.'
  - 'Floor load numbers on the reference i3 runner; verified locally only.'
---

<intent-contract>

## Intent

**Problem:** Nothing proves Epic 3's money and session rules hold under random sequences, a busy Saturday, a WAN outage or a killed Hub.

**Approach:** Proof only, no product change. Extend the Story 1.8 proptest with the global invariants, add a Writer load test with a kill-during-burst check, an offline equivalence test, a Playwright flow-timing bench, and release-benchmark jobs that fail the release on a miss.

## Boundaries & Constraints

**Always:** no change to `apply()` or any schema; budgets are env-overridable and default to the story's numbers.

**Never:** a new intent, permission or user-facing string.

## Code Map

- `crates/arena-core/tests/proptest_apply.rs` -- `assert_sessions_hold` (one live session per device) and `assert_orders_hold` (settled orders' payments equal totals, UPI 4-digit reference, card reference) run after every step, beside the wallet, credit, gap-free log and replay checks.
- `crates/arena-core/tests/floor_load.rs` -- 60 devices, start/pause/resume/extend/transfer/end/settle mix at 5/s with a 50/s burst; fails on apply p95 over 100 ms, a change taking over 2 s to reach a subscriber, a lost or duplicated intent (every 7th is resent with its key), or a log gap. A child process is killed during a burst and the reopened database must hold every acknowledged change, pass `integrity_check`, rebuild the timer heap and give a snapshot that covers the live sessions.
- `crates/arena-core/tests/offline_flows.rs` -- the same scripted flow under `online`, `internet_offline` and `cloud_unreachable` gives identical change logs and money; `arena-core` source contains no socket or HTTP use.
- `apps/hub/tests/bench/flow-timing.spec.ts`, `package.json` (`bench:flows`) -- session start and settle each under 10 s from the first tap.
- `.github/workflows/release-bench.yml` -- `floor-load` (600 s, 10 s bursts) and `flow-timing` (plus profile creation under 15 s) on `reference-i3`.

## Tasks & Acceptance

- [x] Proptest global invariants
- [x] Load test with delivery latency, resend and gap checks
- [x] Kill-during-burst restart check
- [x] Offline equivalence and no-network source guard
- [x] Playwright flow timings and release jobs

**Acceptance Criteria:**
- Given the proptest suite, then every step also checks one live session per device, settled payments equal totals, and UPI/card references; the CI test run covers it on every push.
- Given 60 devices at 5/s with 50/s bursts, then apply p95 is under 100 ms and updates arrive within 2 s with nothing lost or duplicated; the release job fails otherwise.
- Given start, settle and profile creation, then each is timed against 10 s, 10 s and 15 s.
- Given the WAN up or down, then the Epic 3 flow results are identical and `arena-core` makes no outbound call.
- Given the Hub killed during a burst, then every acknowledged row survives, the timer heap rebuilds and the snapshot kiosks re-sync to matches the Hub's record.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test -p arena-core` (663 unit tests plus the new suites), `PROPTEST_CASES=256` proptest, `pnpm --filter @arena/hub check`, `pnpm run format:check`, `check:strings`, `bench:flows` and the profile timing test -- pass.
- Local release-profile load run (20 s, 5 s burst): 325 intents, apply p95 7.3 ms, slowest update 329 ms. Not run on reference hardware.

</intent-contract>
