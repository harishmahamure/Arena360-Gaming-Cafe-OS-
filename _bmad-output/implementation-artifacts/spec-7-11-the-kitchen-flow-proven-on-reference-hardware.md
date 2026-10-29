---
title: 'Story 7.11: The kitchen flow proven on reference hardware'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '75fa607'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-38-the-player-journey-proven-on-reference-hardware.md'
warnings: []
deferred:
  - 'Every release-size number on the reference i3 (the full burst and soak in release mode), the no-route soak (it needs the Linux runner: `unshare --net`), the 8-hour spread of the soak and the KOT on the real USB and LAN printers need the lab rig; none ran here. Locally (debug build, fake printer, Windows) the Journey 4 legs, the unplugged printer, a 60 s burst of 300 orders across 20 kiosks and 2 displays (KOT p95 about 500 ms) and a 500-order soak with a route out passed; the report merged them and failed only on the missing WAN-down soak.'
  - 'The Kitchen display suite ran on Playwright emulations (Galaxy Tab S4 landscape on Chromium, iPad gen 7 landscape on WebKit), 62 tests each, all passing. Real Chrome-on-Android and Safari-on-iPad tablets are a by-hand run (docs/runbooks/kitchen-bench.md).'
  - 'The HUD and tile legs are timed from the Hub side (the PC session.tick, the staff client applied event and the snapshot); the kiosk UI paint and the Hub UI repaint are inside neither number.'
  - 'The burst measures KOT latency from the ticket first showing in the branch database to the slip reaching the fake printer, polled every 20 ms, so it is accurate to about that step. A real printer transfer time is measured by the hardware job.'
---

## Intent

**Problem:** Nothing measures the order-to-kitchen-to-seat flow end to end: how fast a KOT prints, how fast the board, the HUD and the tile follow, what a burst or a long outage does to it, or whether the display works on a phone-class Chrome and on Safari.

**Approach:** Proof, plus one small fix the proof found. A Hub-side timed run of Journey 4 over the real LAN protocol, an unplugged-printer run, a burst and a soak, a KOT job in the monthly hardware bench, the Kitchen display suite on two more browsers, one release report that merges them and publishes the browser list, and release-bench jobs that run all of it on the rig.

**Fix found by the proof:** the HUD ready copy rode the 5-second session tick, so the Ready tap reached the PC in 4.5 s in the first run, with no margin on the 5 s budget. The Hub fan-out now pushes the session screens as soon as an intent writes both a kitchen ticket and an order (the Ready move stamps the order), so the copy arrives in milliseconds.

## Boundaries & Constraints

**Always:** budgets are env-overridable and default to the story's numbers (KOT 3000 ms, board 2000 ms, HUD 5000 ms, tile 2000 ms, burst p95 3000 ms); a missing result fails the report.

**Never:** a change to `apply()`, a schema, an intent or a user-facing string.

## Code Map

- `crates/arena-api/tests/lan.rs` -- `kitchen_rig` (Basic branch, a kitchen item, `ReceiptPrintTask` on a fake printer), `the_hub_side_of_journey_4_meets_its_timings` (kiosk order, KOT, board New, Preparing, Ready, HUD copy, tile READY, Served; `ARENA_KITCHEN_OUT`), `an_unplugged_printer_raises_the_alert_and_the_board_still_gets_the_ticket` (`ARENA_KITCHEN_UNPLUGGED_OUT`), `burst` (a 3 s run in the ordinary suite, the `#[ignore]`d 5 orders/s for 60 s across 20 kiosks and 2 displays; `ARENA_KITCHEN_BURST_OUT`), `soak` (40 orders in the ordinary suite, the `#[ignore]`d 500 orders New to Served; `ARENA_KITCHEN_SOAK_OUT`, `KITCHEN_SOAK_LABEL`, `KITCHEN_SOAK_SPREAD_SECS`).
- `crates/arena-api/src/lan/registry.rs` -- the fan-out pushes the session screens after an intent that wrote `kitchen_tickets` and `orders`.
- `crates/arena-tasks/src/receipts.rs` -- the hardware bench gains a `kot` job per printer (slower than 3 s fails).
- `scripts/hardware-compat.mjs` (+ test), `docs/compat/hardware.md`, `docs/runbooks/hardware-bench.md`, `.github/workflows/hardware-bench.yml` -- the KOT column.
- `apps/hub/playwright.kitchen.config.ts` -- `KITCHEN_DEVICES=1` adds the `android-chrome` and `ipad-safari` projects; `KITCHEN_JSON` writes the run's results.
- `scripts/kitchen-report.mjs` (+ test), `docs/compat/kitchen-display.md` -- merges the results, checks the budgets, publishes the browser list.
- `.github/workflows/release-bench.yml` (`kitchen-hub`, `kitchen-soak-down`, `kitchen-browsers`, `kitchen-report`), `docs/runbooks/kitchen-bench.md`.

## Tasks

- [x] Journey 4 over the LAN, timed, with the KOT, board, HUD and tile legs
- [x] Printer unplugged: the Printer offline alert and the board still gets the ticket
- [x] Burst: 5 orders/s for 1 minute across 20 kiosks and 2 displays; every KOT once, boards match the Hub, p95 within 3 s
- [x] Soak: 500 orders New to Served, no ticket, advance or KOT lost; the same run with no route out compared with a route in
- [x] Push the session screens when the Ready move stamps the order (HUD within 5 s with margin)
- [x] KOT job in the monthly hardware bench and its column in the compatibility list
- [x] Kitchen display suite on Chrome on Android and Safari on iPad, results in the compatibility list
- [x] Release report, release-bench jobs and the runbook

## Acceptance Criteria

- [x] Given Journey 4 on the Hub over the LAN, the KOT prints <= 3 s after commit, the ticket is on the board <= 2 s, the HUD ready copy shows <= 5 s after the Ready tap and the tile shows READY <= 2 s, each timing recorded in the release report.
- [x] Given the printer unplugged, an order raises the "Printer offline" alert and the board still receives the ticket.
- [x] Given 5 orders per second for 1 minute across 20 kiosks with 2 displays, every KOT prints once, both boards match the Hub and KOT latency stays <= 3 s at the 95th percentile.
- [x] Given the WAN-outage soak with 500 orders, no ticket, advance or KOT is lost and the outcome equals a WAN-up run's (the job and the report enforce it; the no-route run is the rig's).
- [x] Given the Kitchen display on Chrome on Android and Safari on iPad, the board, chime, mute, Undo and LAN-loss suite passes on both and the results are published to the compatibility list.

## Verification

- `cargo test -p arena-api` (299 lib, 105 `lan`, with the new Journey 4, unplugged, short burst and short soak tests); `cargo test -p arena-api --test lan -- --ignored five_orders_a_second five_hundred_orders` (60 s burst: 300 KOTs once each, p95 503 ms, boards equal; 500 orders, 1500 advances, none lost); `cargo test -p arena-tasks --lib` (the new `kot` job is in the `#[ignore]`d hardware bench).
- `node --test scripts/kitchen-report.test.mjs scripts/hardware-compat.test.mjs` (19 pass); `KITCHEN_DEVICES=1 playwright test -c playwright.kitchen.config.ts --project=android-chrome` and `--project=ipad-safari` (62 pass each).
- `cargo fmt --all --check`, `cargo clippy -p arena-api -p arena-tasks --tests -- -D warnings`, `pnpm --filter @arena/kitchen check`, `format:check`, `check:strings`, `check:pins`, `check:secrets`, `check:no-blur`, `check:hex`, `check:layers`, `check:protocol`, `check:generated`.
- Pre-existing and unchanged by this story: `arena-tasks` lib tests `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out` fail on this Windows machine with or without it.

## Notes

- A display (and any test standing in for one) must ping at least every 45 s or the Hub closes its session; the first 60 s burst run lost its boards at 216 tickets for that reason until the follower pinged as the tablet does.
- The local run of the report is 15 of 16: the WAN-down soak is the one result only the Linux runner can produce.
