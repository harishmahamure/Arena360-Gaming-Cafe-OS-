# Kitchen flow bench (Story 7.11)

The per-release proof that the order-to-kitchen-to-seat flow holds on a busy night. It is part of
`.github/workflows/release-bench.yml` (manual run and release tags). The last job, `kitchen-report`,
merges every result with `scripts/kitchen-report.mjs` and fails the release when a budget is
missed **or a result is missing**: a rig that is down is a failed release, not a skipped one.

## What is measured

| Measure                                 | Budget                | Where it comes from                                                                                   |
| --------------------------------------- | --------------------- | ----------------------------------------------------------------------------------------------------- |
| KOT prints after commit                 | ≤ 3 s                 | `the_hub_side_of_journey_4_meets_its_timings`: `shop.place` answered to the slip at the printer         |
| Ticket appears on the board             | ≤ 2 s                 | the same run: `shop.place` to the pushed `kitchen.board` holding the ticket as New                      |
| HUD ready copy after the Ready tap      | ≤ 5 s                 | the same run: the Ready intent to the PC's `session.tick` carrying the ready copy                       |
| Tile shows READY after the Ready tap    | ≤ 2 s                 | the same run: the Ready intent to the staff client's `applied` event and the snapshot's READY ticket    |
| Printer unplugged                       | alert and board       | `an_unplugged_printer_raises_the_alert_and_the_board_still_gets_the_ticket`                              |
| Burst, 5 orders/s for 1 min, 20 kiosks, 2 displays | every KOT once, boards match, p95 ≤ 3 s | `five_orders_a_second_for_a_minute_across_twenty_kiosks` (`#[ignore]`d, run by the job) |
| 500 orders, WAN up and WAN down         | nothing lost, nothing differs | `five_hundred_orders_flow_through_the_kitchen_with_nothing_lost`, once with a route out and once in a network namespace with only loopback |
| Kitchen display, Chrome on Android and Safari on iPad | whole suite passes | `pnpm --filter @arena/hub test:kitchen` with `KITCHEN_DEVICES=1`, projects `android-chrome` and `ipad-safari` |

The Journey 4 legs, a short burst (15 orders) and a short soak (40 orders) also run in the ordinary
`arena-api` suite; the release jobs run the full sizes on the reference rig, where the numbers mean
something. Budgets can be overridden for a trial with `KITCHEN_KOT_MS`, `KITCHEN_BOARD_MS`,
`KITCHEN_HUD_MS`, `KITCHEN_TILE_MS` and `KITCHEN_P95_MS`; `KITCHEN_BURST_SECS` and
`KITCHEN_SOAK_ORDERS` change the burst length and the soak size (the report accepts the lower figure
only when it is given the same variable).

A KOT's latency in the burst is the time from the ticket first appearing in the branch database to
its slip reaching the (fake) printer, polled every 20 ms. The Hub's print task drains every 500 ms, so
a figure near 500 ms is the floor, not a fault.

## The rig

- **Reference i3** (`self-hosted, windows, reference-i3`): runs the Hub legs and the burst. The
  printer here is the in-process fake the `arena-tasks` suite uses, so the figures are the Hub's own.
  The real printers are measured by the monthly hardware job (`docs/runbooks/hardware-bench.md`,
  `docs/compat/hardware.md`): its fourth job sends a KOT through the real adapter to each USB and
  LAN ESC/POS printer and fails the printer when the slip takes over 3 s; whoever loads the paper
  checks the slip by eye.
- **Linux runner** (`self-hosted, linux, x64`, passwordless sudo, `unshare` and `ip`): the no-route
  soak. `unshare --net` gives the test a namespace where only loopback is up. With the `soak` input
  of the workflow, the 500 orders are spread over 8 hours (`KITCHEN_SOAK_SPREAD_SECS=28800`).
- **Browsers**: `kitchen-browsers` installs Chromium and WebKit and runs the Kitchen display suite on
  a Galaxy Tab S4 profile (Chrome on Android) and an iPad (gen 7) profile (Safari's engine).
  Playwright emulates the device; a run on a real tablet is by hand (below).

## The compatibility list

`kitchen-report` writes `docs/compat/kitchen-display.md`: one row per browser with the tests passed
and failed and the date of the run. A browser with any failed test fails the release. The job uploads
the file as the `kitchen-report-and-compat` artifact; commit it with the release notes.

## By hand

```
cargo test --release -p arena-api --test lan the_hub_side_of_journey_4 -- --nocapture
cargo test --release -p arena-api --test lan -- --ignored --nocapture five_orders_a_second
KITCHEN_SOAK_LABEL=wan-up cargo test --release -p arena-api --test lan -- --ignored --nocapture five_hundred_orders
sudo unshare --net -- bash -c 'ip link set lo up && KITCHEN_SOAK_LABEL=wan-down cargo test --release -p arena-api --test lan -- --ignored --nocapture five_hundred_orders'
KITCHEN_DEVICES=1 pnpm --filter @arena/hub exec playwright test -c playwright.kitchen.config.ts --project=ipad-safari
node scripts/kitchen-report.mjs --dir target/kitchen
```

`ARENA_KITCHEN_OUT`, `ARENA_KITCHEN_UNPLUGGED_OUT`, `ARENA_KITCHEN_BURST_OUT` and
`ARENA_KITCHEN_SOAK_OUT` (absolute paths) make the tests write the files the report reads;
`KITCHEN_JSON` does the same for a Playwright run.

On a real tablet: pair it (Story 7.6), open the board, place a kitchen order from a kiosk and check by
eye that the New chime sounds, the mute toggle holds, Undo takes a move back within 8 seconds and the
badge reads RECONNECTING with the Wi-Fi off and CONNECTED with it back.
