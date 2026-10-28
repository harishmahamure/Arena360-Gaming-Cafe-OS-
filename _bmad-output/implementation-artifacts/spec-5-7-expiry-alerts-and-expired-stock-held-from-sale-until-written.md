---
title: 'Story 5.7: Expiry alerts, and expired stock held from sale until written off'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: '46c0ce0'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-5-stock-in-with-expiry-lots-and-weighted-average-cost.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-6-record-waste-with-a-reason.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Stock with an expiry date can be sold after it has expired, and nobody is told it is about to. Expired lots stay on the books with no way to clear them.

**Approach:**
- New branch setting `stock.expiry_warn_days` ("Warn before expiry", default 3, 0–30), edited with `settings.set` (permission `settings.edit`).
- New alert kind `stock_expiring` ("Expiring soon"), a branch-level alert (no device; the schema already allows one open per kind). It is raised (a) inside a `stock.in` whose expiry date falls inside the window and (b) by the system intent `stock.expiry_check`, which the Hub's hourly sweep submits only when it would change something: it raises the alert once per business day while any lot with stock left is expired or inside the window (an alert still open from an earlier day is acknowledged by the system and replaced), and clears it once no such lot is left. Local clock and business day only; no network call.
- A lot is *expired* when its expiry date is before today's business day (the date itself is still sellable, as in Story 5.5). Availability excludes expired units: a tracked product with no unexpired quantity reads `available = false, reason = expired` (bundles read their components the same way). The daily check touches the stock row of each product with an expired lot so `catalog.changed` carries the new availability when the day turns.
- New intent `stock.write_off {lot_id}` (permission `stock.adjust`; a Counter uses the existing manager-PIN elevation): empties one expired lot, lowers `stock.qty` (average cost unchanged), appends a `stock_moves` row `reason = expired` valued at the lot's own cost, and clears the alert when no other lot is expired or inside the window.
- Hub: Inventory → Expiring (lots in the window with product, quantity, date and cost, plus the warning-days field) and Inventory → Expired (each lot with a Write off button); the header alert list sounds and, when picked, acknowledges and opens Expiring.

## Boundaries & Constraints

**Always:**
- **Validation of `stock.write_off`** (nothing written on failure): unknown lot → `VALIDATION_FAILED lot_id`; lot with nothing left → `STALE lot_id`; lot without an expiry date or not yet past it → `VALIDATION_FAILED lot_id`.
- `stock.expiry_check` is system only (`PERMISSION_DENIED` for anyone else) and `STALE` when it has nothing to do.
- The move's `lots_used` records the lot; `value_minor = remaining × lot unit cost`.
- Both intents are in the Story 1.8 proptest generator and the replay fixture.

**Never:**
- Stock counts (14.x), expiry-based reports (5.31), per-lot alerts, undo of a write-off, a network call in the daily check, owner alerts for expiry (Epic 8).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected |
|----------|--------------|----------|
| Stock-in inside window | expiry = today + 3, warn 3, no open alert | lot added, one `stock_expiring` alert |
| Stock-in outside window / no expiry | today + 4 / none | no alert |
| Warn 0 | expiry today / tomorrow | alert / none |
| Check, new business day | lot in window, nothing raised today | alert raised (stale open one replaced) |
| Check, same day | already raised | `None` (nothing submitted) |
| Check, nothing left in window | open alert, lots wasted | alert acknowledged by `hub-stock-expiry` |
| Expiry date itself | lot expires today | still sellable, in the Expiring list |
| Day after | lot expired | held from sale, in the Expired list, product `expired` |
| Write off expired lot | counter without PIN | `PERMISSION_DENIED` |
| Write off | manager | lot 0, stock lowered, `expired` move at lot cost, alert cleared if last |

</frozen-after-approval>

## Code Map

- `crates/arena-proto`: `stock.rs` (`StockWriteOff`, `StockExpiryCheck`, `ExpiringLot`, `ExpiryView`, `add_days`, `lot_expired`, `lot_in_window`, setting key), `alert.rs` (`StockExpiring`), `intent.rs` (kinds, requirements), `catalog.rs` (`REASON_EXPIRED`).
- `crates/arena-store`: `stock.rs` (`list_expiring_lots`, `expired_qty`), `alerts.rs` (`latest_branch_alert_at`), `lib.rs` read methods.
- `crates/arena-core`: `apply/stock.rs` (load, write-off, check, stock-in alert), `settings_defaults.rs`, `catalog.rs` (expired availability, `expiry_view`), `snapshot.rs`/`devices.rs` (`now_ms` on `products`/`catalog`, `expiry`), `writer.rs` (`stock_expiry_check`).
- `crates/arena-tasks` `housekeeping.rs` (hourly sweep), `crates/arena-api` (`GET /ui/stock/expiry`, `now_ms` on catalog reads).
- `crates/arena-replay`, `crates/arena-core/tests/proptest_apply.rs`.
- `apps/hub`: `routes/inventory/{expiring,expired}/+page.svelte`, links on `inventory/+page.svelte`, `lib/expiry.ts`, `lib/alerts.svelte.ts` (kind, sound, href), `routes/+layout.svelte` (`pickAlert`), `tests/expiry.spec.ts`; `packages/ui/src/messages/en.json`; regenerated `packages/ui/src/generated`.

## Tasks & Acceptance

- [x] Proto: payloads, alert kind, intent kinds, setting key, date helpers, `expired` reason.
- [x] Store: expiring-lot and expired-quantity queries, latest branch alert.
- [x] Apply: `stock.write_off`, `stock.expiry_check`, alert on `stock.in`, `warn_days` setting default and rule.
- [x] Availability: expired units excluded from catalog and bundles; `GET /ui/stock/expiry`.
- [x] Hub sweep calls the check hourly while primary.
- [x] Apply tests, proptest ops, replay fixture, API test.
- [x] Hub Expiring and Expired pages, alert sound and pick, messages, Playwright.
- [x] Verification: `cargo fmt --check`, clippy on touched crates, tests on touched crates, `pnpm check` (hub), `check:strings`, `check:protocol`, `format:check`, `check:layers`.

**Acceptance Criteria:** the Given/When/Then of Story 5.7 in `epics.md`, as refined above.

## Verification Notes

- Pre-existing failures, unrelated to this story: `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`, `arena-cloud` `tests/binary.rs::unreachable_at_start_retries_then_exits_1_without_logging_the_token` and `tests/backup.rs::a_hung_run_times_out_with_its_own_code`; `check:tokens-used` reports `--ad-reserved` in `floor/+page.svelte`; `check:generated` lists the new generated files as untracked until they are committed.
- The daily check runs from the existing hourly sweep, so the alert lands within an hour of the business day turning over (and at Hub start), not at the exact minute.
- Because the alert is one per branch, "the alert for that lot clears" is implemented as: it clears when the last lot that is expired or inside the window is written off (or the next check finds none).

## Spec Change Log

## Review Triage Log
