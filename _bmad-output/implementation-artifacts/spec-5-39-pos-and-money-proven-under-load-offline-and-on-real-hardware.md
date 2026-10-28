---
title: 'Story 5.39: POS and Money Proven Under Load, Offline and on Real Hardware'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_revision: '50aeccd'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-37-floor-invariants-throughput-and-offline-proof.md'
warnings: []
deferred:
  - 'The real-Hub, no-route-out run of the Epic 5 flows (`scripts/wan-down/run.sh` is Linux netns only and was not run here). Reports, exports and imports over HTTP are covered by the Epic 5 flow at the intent level (the Z-report is built and stored in the shift close; `import.products` runs); the CSV/PDF export and the CSV parsing paths are not part of the offline-equivalence run.'
  - 'Load numbers on the reference i3 runner and the hardware bench on the test café printers; both were verified locally only (debug build, fake printer) and run by `release-bench.yml` and `hardware-bench.yml`.'
  - 'A print job sent in the instant before its result intent is recorded would be sent again after a kill (at-least-once). Closing it needs a `sending` outbox state, which is a schema change; the proof covers rows left queued, not that window.'
---

<intent-contract>

## Intent

**Problem:** Nothing proves Epic 5's stock, tax, invoice and drawer rules hold under random sequences, a busy night, a WAN outage, a killed Hub or real printers.

**Approach:** Proof only, no product change. Extend the Story 1.8 proptest with the Epic 5 global invariants, add a counter-sale mix and a kill-during-sale-burst check to the load test, an Epic 5 offline-equivalence flow, a printer-restart test, release-benchmark jobs, and a monthly hardware job that publishes to a compatibility list.

## Boundaries & Constraints

**Always:** no change to `apply()` or any schema; budgets are env-overridable and default to the story's numbers.

**Never:** a new intent, permission or user-facing string.

## Code Map

- `crates/arena-core/tests/proptest_apply.rs` -- after every step also: `assert_ledger_holds` (stock quantity equals the sum of its `stock_moves` deltas, beside the existing non-negative and lots check; every invoice sits on a settled order), `assert_tax_holds` (a counter sale's breakup has each rate once, its tax and taxable amounts are the order's, total is subtotal plus tax, lines less coupon and discount equal the total (tax inclusive) or the subtotal (exclusive)), `assert_drawer_holds` (`expected_cash` equals the shift's cash events, drawer-open rows are zero, the float row is the shift's float). `assert_orders_hold` now compares settled orders with positive payment rows, because a refund writes a negative row that its credit note accounts for.
- `crates/arena-core/tests/floor_load.rs` -- `Mix::Counter` (cash/UPI counter sales with Print and a manual discount, waste, cash refunds of numbered sales, `invoice.issue`) runs through the same `load` as the floor mix; `assert_books` checks stock against lots and moves, settled payments against totals, gap-free invoice and credit-note series, the cash ledger and each shift's drawer. `a_hub_killed_during_a_sale_burst_keeps_every_sale_stock_move_and_invoice` kills a child during a sale burst and checks every acknowledged change, `integrity_check`, the books, exactly one queued receipt per acknowledged Print sale (numbered in its own settle), then restarts a Writer and numbers each pending sale once (a second ask is refused) with no gap.
- `crates/arena-core/tests/offline_flows.rs` -- `boot()` shared by the Epic 3 and Epic 5 flows; the Epic 5 flow (products, stock-in, waste, coupon, discount, counter sales with a Print receipt, a session order, invoice numbering, a refund with its credit note, a deposit and an expense, a product import, a shift close with its Z-report) gives identical change logs, numbers, stock, money and Z-report under `online`, `internet_offline` and `cloud_unreachable`. The no-network source guard covers all of `arena-core`.
- `crates/arena-tasks/src/receipts.rs` -- `rows_left_queued_by_a_crash_print_once_across_restarts` (restarted tasks print each queued row once) and the `#[ignore]`d `hardware_bench_*` test that sends a receipt, the Z-report and a drawer kick through the real adapter to each printer in `ARENA_HW_PRINTERS`.
- `apps/hub/tests/bench/scanner.spec.ts`, `package.json` (`bench:scanner`) -- a real keyboard-wedge scan adds the product to the POS cart.
- `scripts/hardware-compat.mjs` (+ test), `docs/compat/hardware.md`, `docs/runbooks/hardware-bench.md` -- merges the hardware results into the compatibility list; exit 1 on any failed job or scan.
- `.github/workflows/release-bench.yml` (`counter-load`, `pos-timing`), `.github/workflows/hardware-bench.yml` -- release gates and the monthly job on the test-café runner.

## Tasks & Acceptance

- [x] Proptest global invariants for Epic 5 (stock = lots = moves, tax once per rate, payments = totals, invoice numbers, drawer = cash events)
- [x] Counter-sale load mix with apply p95, delivery, resend and books checks
- [x] Kill-during-sale-burst restart check (rows, stock moves, invoices, queued receipts, pending numbers issued once)
- [x] Epic 5 offline-equivalence flow
- [x] Restarted print task sends each queued row once
- [x] Hardware bench job, scanner check and compatibility list
- [x] Release jobs for counter load and the 10 s timings

**Acceptance Criteria:**
- Given the proptest suite, then after every step stock equals its lots and its moves and is never negative, a counter sale's tax adds up per rate, settled payments equal totals, invoice and credit-note numbers are unique and gap-free per series on settled orders, and the expected drawer cash equals the shift's cash events; the CI test run covers it on every push.
- Given the WAN up or down, then the Epic 5 flow results are identical and `arena-core` makes no outbound call.
- Given the Hub killed during a burst of sales, then every acknowledged sale, stock move and invoice is present, pending invoice numbers are issued once, and queued receipts are one per Print sale and print once after the restart.
- Given the test-café hardware, then the monthly job sends receipts at 58 and 80 mm, the Z-report and the drawer kick to the listed printers and a scan adds an item, and publishes the results to the compatibility list.
- Given the scanned sale and the within-threshold shift close, then each is timed against 10 s on the reference runner.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test -p arena-core` (with `PROPTEST_CASES=256`), `cargo test -p arena-tasks receipts`, `pnpm --filter @arena/hub check`, `pnpm run format:check`, `check:strings`, `check:pins`, `check:secrets` and `node --test scripts/hardware-compat.test.mjs` -- pass.
- Two `arena-tasks` tests fail on this Windows machine in files this story does not touch (`printer::lan::a_printer_that_stops_reading_times_out`, `heartbeat::trusted_time_is_cloud_time_plus_monotonic_elapsed`), and the `check:deploy` tests need `kubectl`.
- Local counter run (6 s, debug build): 120 intents, apply p95 7.8 ms, slowest update 10 ms. Not run on reference hardware.

</intent-contract>

### Review Findings

- [x] [Review][Patch] Failed scanner step left no `scanner.json`, so the publish step crashed and no printer result reached the list — the step now writes `ok:false`, and `hardware-compat.mjs` tolerates a missing scanner file and reports a missing printer file [hardware-bench.yml, hardware-compat.mjs]
- [x] [Review][Patch] PR step: same-day reruns collided on the branch name and native exit codes were unchecked — time-stamped branch, exit-code checks [hardware-bench.yml]
- [x] [Review][Patch] `main()` exit codes and the scanner row were not tested — test added [hardware-compat.test.mjs]
- [x] [Review][Patch] Crash drill passed vacuously on empty Print/pending lists, and the second-issue check accepted any error — non-empty asserts and `Stale` code check [floor_load.rs]
- [x] [Review][Patch] `assert_tax_holds` treated a missing `branch.tax` as exclusive, the product default is inclusive [proptest_apply.rs]
- [x] [Review][Patch] Bench `paper` was not validated — asserted to be "58" or "80" [receipts.rs]
- [x] [Review][Patch] `floor-load` release job re-ran the counter tests that `counter-load` runs — skipped there [release-bench.yml]
- [x] [Review][Defer] Offline flow compares states with each other only (no golden figures); kill drill and print-once halves are not joined end to end; at-least-once print window — disclosed in the spec's deferred list or low value.

#### Rejected

- Widened `expected_refusal`, negative-payment filter, `|` in printer names, scanner timing realism, no failure alerts for the self-hosted runner, SQL interpolation of a validated paper value: test-only or hardware-bench ergonomics with no demonstrated harm.
