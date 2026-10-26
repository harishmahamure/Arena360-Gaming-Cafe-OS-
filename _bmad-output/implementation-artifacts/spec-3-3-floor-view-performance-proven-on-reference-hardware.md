---
title: 'Story 3.3: Floor view performance proven on reference hardware'
type: 'feature'
created: '2026-10-26'
status: 'in-review'
baseline_commit: 'c7ab93e'
route: 'dispatch'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-2-floor-view-shows-every-device-as-a-live-tile-grouped-by-zone.md'
---

## Intent

**Problem:** Nothing proves the Floor stays fast with 60 devices on the reference i3 + HDD, and no release gate fails when it does not.

**Approach:** A Playwright benchmark drives the real Floor page with 60 devices in 4 zones, pushes `devices.changed` through the real event handler, measures in the page, and fails with the metric and value. A `floor-perf` job in the release benchmark runs it on the reference runner.

## Tasks & Acceptance

- [x] `apps/hub/tests/floor-fixture.ts` -- shared snapshot helpers split out of `floor.spec.ts`
- [x] `apps/hub/tests/bench/floor-perf.spec.ts`, `playwright.bench.config.ts`, `package.json` (`bench:floor`) -- cold open, churn (10 changes/s), frame time, input-to-paint (arrow focus + wheel), JS-heap soak
- [x] `.github/workflows/release-bench.yml` -- `floor-perf` job on `reference-i3`, 60 s churn and 30 min soak

**Acceptance Criteria:**
- Given 60 devices in 4 zones, when Floor cold-opens, then all 60 tiles paint within 1 s (`FLOOR_BENCH_COLD_MS`).
- Given 10 state changes per second, then each paints within 2 s and no frame exceeds 100 ms.
- Given arrow focus across all tiles and wheel scrolling, then no input-to-paint delay exceeds 100 ms.
- Given a missed budget, then the run fails naming the metric and its value.

## Implementation Notes

- Budgets are env-overridable (`FLOOR_BENCH_*`); defaults are the story's budgets. `FLOOR_BENCH_SECONDS` defaults to 60, `FLOOR_BENCH_SOAK_MINUTES` to 0 (the release job sets 30).
- Verified locally (not on reference hardware): cold 174 ms, paint 28 ms, frame 67 ms, input 18 ms; forcing budgets to 1 ms fails with metric and value.
- Not proven: the 30-minute check measures the page's JS heap (CDP), a proxy for the 250 MB Hub UI budget (WebView2 working set). The 50 MB Hub service budget with 60 devices is not measured: `arena-hub dev seed` currently exits `USER_LIMIT` (4 staff on the Free cap, fails on master before this story) and cannot seed 60 devices, so `hub-idle` still runs without them.

## Verification

- `pnpm --filter @arena/hub check`, `pnpm run format:check` -- pass
- `pnpm --filter @arena/hub test:shell` -- 611 passed, 11 skipped
- `FLOOR_BENCH_SECONDS=10 pnpm --filter @arena/hub bench:floor` -- pass
