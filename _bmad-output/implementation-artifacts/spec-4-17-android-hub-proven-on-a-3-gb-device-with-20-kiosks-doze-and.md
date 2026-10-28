---
title: 'Story 4.17: Android Hub proven on a 3 GB device with 20 kiosks, Doze and no WAN'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_revision: '58ed238'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-37-floor-invariants-throughput-and-offline-proof.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Nothing proves the Android Hub keeps a full floor on the cheapest supported device with ads on screen, in Doze, with the WAN down and after a kill.

**Approach:** Proof only, no product change. A fleet driver (20 pinned-TLS fake kiosks and a Hub UI stream) runs the Story 3.37 intent mix against a real `arena-hub`, writes a JSON report and verifies a kill and restart; a script holds the budgets, Doze control, frame and egress checks; a per-release workflow runs it on an Android 9 / 3 GB device and a latest-Android device.

## Boundaries & Constraints

**Always:** budgets are data in `deploy/android-farm/farm.json`, with the story's floors enforced by `check:farm`; a later measured operation is one new entry, no harness change.

**Never:** a new intent, permission, user-facing string or product behaviour change; the only product-tree edit is the dev-only seed's licence.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| apply p95 130 ms | any | `verify` fails |
| A committed intent never reaches a kiosk | `lost` > 0 | fails |
| Hub leaves Doze or stops answering | during the 30 min | fails |
| Time-up 9 s late | any probe | fails |
| Connection to a non-ad, non-probe address | egress check | fails |
| Hub killed, an acknowledged intent resent | after restart | same outcome or fails |
| Config asks for 10 kiosks | `check:farm` | fails |

</intent-contract>

## Code Map

- `crates/arena-hub/tests/android_fleet.rs` -- `load` and `verify` phases; local mode runs both against a spawned Hub on every push.
- `scripts/android-farm.mjs` (+ test) -- config floors, report verification, framestats, egress, Doze, kill.
- `scripts/android-farm/run.sh`, `.github/workflows/android-farm.yml`, `deploy/android-farm/farm.json`, `docs/runbooks/android-farm.md`.
- `crates/arena-hub/src/dev.rs` -- `dev seed` applies a roomy licence before the fixture staff.

## Tasks & Acceptance

- [x] Fleet driver: 20 kiosks, mixed load at 5/s, delivery, resend, timer probes, report
- [x] Kill and restart verify phase
- [x] Budgets, report verification, Doze, frames and egress checks with tests
- [x] Farm workflow, run script, runbook and README

**Acceptance Criteria:** those of Story 4.17 in `epics.md`. The device-farm runs themselves are operator-run (see deferred work).

## Verification

- `cargo test -p arena-hub --features dev-fixtures --test android_fleet` (6 s load and 8 s load with 1-minute probes, kill, restart, verify), clippy with dev-fixtures, fmt, `node --test scripts/android-farm.test.mjs`, `pnpm run check:farm`: pass.
- Not run: any device, the workflow, a 60-minute load.
