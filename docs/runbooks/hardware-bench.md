# Hardware bench (Story 5.39)

The monthly proof that receipts, the Z-report, the cash drawer and a keyboard-wedge scanner work on
the test café's real hardware. The workflow is `.github/workflows/hardware-bench.yml`; it runs on
the first of each month at 03:00 UTC and on demand.

## The bench

- A Windows machine registered as a self-hosted runner with the labels `self-hosted`, `windows`
  and `test-cafe`, logged in to a desktop session (the scanner check runs a headed browser).
- Every ESC/POS printer on the compatibility list, on USB (Windows spooler name) or LAN (port
  9100), with the paper roll named in `PRINTERS` loaded. A cash drawer on at least one of them.
- A USB keyboard-wedge scanner and a sheet with the barcode `8901` printed on it.

## Repository variables

| Variable   | Value                                                                           |
| ---------- | ------------------------------------------------------------------------------- |
| `PRINTERS` | `[{"kind":"usb","id_or_addr":"EPSON TM-T20","name":"Epson TM-T20","paper":"80"}]` |
| `SCANNER`  | The scanner model, for its row in the list                                      |

`kind` is `usb` or `lan` (`id_or_addr` is `192.168.1.50:9100`); `paper` is `58` or `80`. List a
printer twice to test it on both widths, loading the other roll between runs.

## What a run does

1. `hardware_bench_prints_receipts_and_a_z_report_and_kicks_the_drawer` (arena-tasks) sends a sale
   receipt, the Z-report, a drawer kick and a kitchen ticket (KOT, Story 7.11) through the real
   adapter to each listed printer. A KOT slower than 3 s counts as failed. Each
   job must come back `done`.
2. `pnpm --filter @arena/hub bench:scanner` opens the POS and waits 60 s for a scan of `8901`; the
   product must reach the cart.
3. `scripts/hardware-compat.mjs` merges the results into `docs/compat/hardware.md` and the
   workflow opens a pull request with the new list. A failed job or scan fails the run.

Whoever loads the paper looks at the slips (the shop name, lines, tax and totals, a clean cut) and
that the drawer opened, and closes the pull request if they do not match the report.

## Running it by hand

```
$env:ARENA_HW_PRINTERS = '[{"kind":"lan","id_or_addr":"192.168.1.50:9100","name":"Counter","paper":"80"}]'
cargo test --release -p arena-tasks hardware_bench -- --ignored --nocapture
$env:ARENA_HW_SCANNER = "Honeywell 1250g"
pnpm --filter @arena/hub bench:scanner
node scripts/hardware-compat.mjs target/hardware-bench/printers.json
```
