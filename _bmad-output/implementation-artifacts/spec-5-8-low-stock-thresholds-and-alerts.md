---
title: 'Story 5.8: Low-stock thresholds and alerts'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '4e433b47c838727037a7015f1e3653a39a2db188'
baseline_revision: '4e433b47c838727037a7015f1e3653a39a2db188'
route: 'dispatch'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-6-record-waste-with-a-reason.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-7-expiry-alerts-and-expired-stock-held-from-sale-until-written.md'
---

<intent-contract>

## Intent

**Problem:** Nobody is told a tracked product is running low, so the café finds out when it is already out of stock.

**Approach:**
- New column `products.low_stock_qty` (whole units, optional, `0..=MAX_STOCK_QTY`), set with the new intent `stock.threshold {product_id, low_stock_qty}` (`null` clears it; permission `catalog.edit`, so a Manager; a Counter uses the existing manager-PIN elevation). Tracked, non-bundle products only.
- New alert kind `stock_low` ("Low stock: <product> (<qty> left)"), one open per product. `alerts` gains `product_id`, `qty`, `threshold` (the recorded event the owner alert of Epic 8 is sent from); the branch-level unique index becomes per `(kind, product)`.
- A stock decrement (waste, `decrement()`, write-off, stock-in revert) that takes the quantity from above the threshold to at or below it raises the alert, once per crossing. While it stays open and the product is still low, further decrements update its `qty`. A stock-in, a waste revert or a threshold change that leaves the quantity above the threshold (or clears the threshold) acknowledges it by the actor. Setting a threshold at or above the quantity on hand raises it.
- Hub: Inventory → Low stock (products at or under their threshold with quantity and threshold, plus a threshold field per tracked product to edit); the header alert list sounds, shows the product and quantity, and when picked acknowledges the alert and opens Low stock.

## Boundaries & Constraints

**Always:**
- `stock.threshold` validation (nothing written on failure): unknown, deleted, untracked or bundle product → `VALIDATION_FAILED product_id`; value outside `0..=MAX_STOCK_QTY` → `VALIDATION_FAILED low_stock_qty`. Setting the value a product already has is `STALE` (nothing to do).
- A product without a threshold, or an untracked one, never raises. A decrement that stays above the threshold, or starts at or below it, raises nothing.
- The alert text uses the product's name and the quantity at the crossing, refreshed by later decrements.
- The new intent is in the Story 1.8 proptest generator and the replay fixture.

**Never:**
- Owner alert delivery, reorder suggestions or purchase orders (Epic 8, 14.x), per-lot thresholds, thresholds on bundles, a low-stock alert from the sale path (Story 5.11 calls `decrement()`, which already raises), undo of a threshold change.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Crossing | threshold 5, qty 8, waste 3 | qty 5, one `stock_low` alert (qty 5, threshold 5) | none |
| Already low | qty 5, waste 1 | alert qty becomes 4, no second alert | none |
| After ack | alert acked, qty 4, waste 1 | no new alert (not a crossing) | none |
| Stock-in lifts | qty 4 → 9, threshold 5 | alert acknowledged, leaves list | none |
| Revert lifts | waste reverted, qty 5 → 8 | alert acknowledged | none |
| No threshold / untracked | decrement | nothing raised | none |
| Set at or above qty | qty 3, threshold 5 | alert raised | none |
| Raise above qty / clear | threshold 2, or `null`, qty 4 | open alert acknowledged | none |
| Bad target | bundle, untracked, unknown | nothing written | `VALIDATION_FAILED product_id` |
| Bad value | -1 or above max | nothing written | `VALIDATION_FAILED low_stock_qty` |
| Counter without PIN | `stock.threshold` | rejected | `PERMISSION_DENIED` |

</intent-contract>

## Code Map

- `crates/arena-store`: `migrations/0045_low_stock.sql` + `migrations.rs`; `alerts.rs` (`AlertRow`/`NewAlert` product, qty, threshold; `find_open_alert_for_product`, `set_alert_qty`), `stock.rs` (`low_stock_qty`, `set_low_stock_qty`, `list_low_stock`), `lib.rs` exports and a read method.
- `crates/arena-proto`: `alert.rs` (`StockLow`, `SnapshotAlert` product fields), `stock.rs` (`StockThreshold`, `LowStockItem`, `LowStockView`), `intent.rs` (kind, category, requirement, payload), `lib.rs`.
- `crates/arena-core`: `apply/stock.rs` (target load, validate/mutate threshold, `after_decrement` hook in `consume`, write-off, revert; clears on stock-in and waste revert), `apply/{mod,mutate,validate}.rs` dispatch, `snapshot.rs` (alert fields, `low_stock()` read), `catalog.rs` (`low_stock_view`).
- `crates/arena-api` `ui_auth.rs` (`GET /ui/stock/low`), `crates/arena-replay`, `crates/arena-core/tests/proptest_apply.rs`.
- `apps/hub`: `routes/inventory/low-stock/+page.svelte`, link on `inventory/+page.svelte`, `lib/lowstock.ts`, `lib/alerts.svelte.ts` (kind, label params, sound, href), `routes/+layout.svelte`, alert list component, `tests/low-stock.spec.ts`; `packages/ui/src/messages/en.json`; regenerated `packages/ui/src/generated`.

## Tasks & Acceptance

**Execution:**
- Store: migration 0045 (`products.low_stock_qty`, alert columns, per-product open index), alert and stock queries.
- Proto: kind, payload, view types, intent wiring, generated types.
- Apply: `stock.threshold`; the low-stock hook on every decrement path and the clear on lifts; unit tests for the I/O matrix, proptest op, replay fixture.
- Read side: snapshot alert fields, `GET /ui/stock/low`, API test.
- Hub: Low stock page with threshold editing, alert label/sound/pick, messages, Playwright.
- Verification: `cargo fmt --check`, clippy and tests on touched crates, `pnpm check` (hub), `check:strings`, `check:protocol`, `format:check`, `check:layers`.

**Acceptance Criteria:**
- Given a Manager sets a threshold, when it commits, then `products.low_stock_qty` holds it and Inventory → Low stock shows it.
- Given a decrement takes stock from above the threshold to at or below it, when it commits, then one "Low stock: <product> (<qty> left)" alert is open with a sound and badge and the product is in the Low stock list; no second alert for the same crossing.
- Given a stock-in lifts the product above its threshold, when it commits, then the alert clears and the product leaves the list.
- Given no threshold or an untracked product, when stock changes, then no alert is raised.
- Given a low-stock alert, when it is written, then it records product id, quantity and threshold.

## Spec Change Log

## Review Triage Log

## Design Notes

The alert row is the recorded `stock.low` event: it is written in the same change-log batch as the decrement and carries `product_id`, `qty`, `threshold`. Epic 8 reads it from the change log. `qty` is rewritten while the alert stays open, so the header text never shows a stale count; the first change-log image keeps the crossing quantity.

## Verification

**Commands:**
- `cargo test -p arena-core -p arena-store -p arena-proto -p arena-api -p arena-replay` -- expected: pass (known unrelated failures listed in spec 5.7)
- `pnpm --filter hub check` and the Playwright spec -- expected: pass

### 2026-10-28 — Review pass
- verdicts: 25 findings — high 0, medium 2, low 4, false 3, maybe-false 1 (blind 10, edge-case 7, verification-gap 2 plus 1 repeat, intent-alignment 6 divergences, 5 of them restating blind/edge findings)
- findings:
  - `[medium]` `patch` Open `stock_low` alert survives deleting, untracking or bundling its product (blind, edge-case ×2, verification-gap ×2) — added `stock::ack_low_for`, called from `product.upsert` and `product.delete`; test `deleting_or_untracking_a_product_acknowledges_its_open_low_stock_alert`.
  - `[medium]` `patch` No test with two products holding open alerts at once (blind, verification-gap) — added `two_products_each_hold_their_own_open_low_stock_alert`.
  - `[low]` `patch` Saving another row while the manager-PIN prompt is open replaces the pending job (edge-case) — `save` returns while `approving`.
  - `[low]` `defer` Quantity compared is on hand, expired lots included (blind, edge-case, intent-alignment) — a write-off of the expired lot is a decrement and crosses; a lot passing its date writes nothing. Recorded in deferred-work.md.
  - `[false]` `reject` Bare `UPDATE`s skip `rev`/`updated_at` (blind) — `ack_alert` and `update_stock` do the same; rev/updated_at are set by the apply engine from `Written`.
  - `[low]` `reject` STALE on an unchanged threshold shows a refusal (blind) — spec requires `STALE`; message is clear.
  - `[low]` `reject` Threshold nudge after ack raises again (blind) — spec: a threshold set at or above the quantity raises; it is a new decision by a person.
  - `[low]` `reject` Acknowledged alert stays silent at 0 (blind) — spec: once per crossing, listed in Low stock.
  - `[low]` `reject` Announcement not repeated on qty change, blank product fallback, error not cleared on input, no `aria-describedby`, hard-coded max, thin Playwright/API/proptest/replay coverage (blind, edge-case, intent-alignment) — cosmetic or covered elsewhere; fixes add guards for states not shown reachable (the alert's product is a foreign key).
  - `[false]` `reject` Spec Code Map lists `+layout.svelte` untouched; epic-5-context rewrite; missing audit label (blind, verification-gap, intent-alignment) — fix would edit the spec, the context rewrite is the workflow's own recompile, and no stock intent has an audit label.
  - `[low]` `reject` Replay fixture comment claims a lift and re-raise (edge-case) — the fixture's later stock-in and write-off do exercise them.
  - `[low]` `reject` "Clears" is acknowledgement; no production caller of `decrement()` yet; extra tracked list on the endpoint (intent-alignment) — descriptive; the spec fixes all three.

## Auto Run Result

Status: done

- **Change:** `products.low_stock_qty` set by `stock.threshold` (`catalog.edit`, Counter via manager PIN); `stock_low` alert one open per product, carrying product id, quantity and threshold; raised on a crossing by waste, `decrement()`, write-off and stock-in revert, refreshed while open, acknowledged by stock-in, waste revert, threshold changes and product delete/untrack/bundle; `GET /ui/stock/low`; Hub Inventory → Low stock with threshold editing and a sounding header alert that opens it.
- **Files:** migration `0045_low_stock.sql`; `arena-store` (`alerts.rs`, `stock.rs`, `lib.rs`, `migrations.rs`); `arena-proto` (`alert.rs`, `stock.rs`, `intent.rs`, `lib.rs`); `arena-core` (`apply/stock.rs`, `apply/products.rs`, dispatch, `snapshot.rs`, `catalog.rs`, tests, proptest); `arena-api` (`ui_auth.rs`, `ui_pair.rs`); `arena-replay`; hub page, `lowstock.ts`, `alerts.svelte.ts`, `AlertList.svelte`, Playwright spec; messages and generated types.
- **Review:** 3 patches applied (2 medium, 1 low), 1 deferred, the rest rejected with the reasons above. Follow-up review not recommended (no high; 2 medium patched).
- **Verification:** `cargo fmt --check`, clippy `-D warnings` on arena-core/api/store/proto/replay, `cargo test` on arena-core (716), arena-store, arena-proto, arena-api, arena-replay all pass; `pnpm check` (hub) 0 errors; `format:check`, `check:strings`, `check:protocol`, `check:layers` pass; Playwright `low-stock` (12) passes after the patches; `expiry` and `alerts` passed before them. Known unrelated failures as in spec 5.7 (`arena-tasks` heartbeat/printer timing, `check:tokens-used`, `check:generated` until committed).
- **Residual risks:** quantity is on hand, not sellable; a system `alert.raise` of kind `stock_low` would make a product-less alert.

### 2026-10-28 — Code review (bmad-code-review)
- Layers: blind, edge-case, verification-gap, acceptance auditor. No acceptance-criteria violation.
- `[medium]` `patch` Untracking or bundling a product left its `low_stock_qty`, so re-tracking listed it as low with no alert — `mutate_upsert` now clears the threshold with the ack; extended `deleting_or_untracking_…` test.
- `[medium]` `patch` `alert.raise` accepted `stock_low` and stored a product-less alert — rejected with `VALIDATION_FAILED alert_kind`; test `a_low_stock_alert_cannot_be_raised_without_a_product`.
- Rejected/deferred: on-hand vs sellable quantity (already deferred), STALE on same threshold (spec), per-alert product lookup, label fallbacks, extra coverage (cosmetic/unreachable).
- Verification: fmt, clippy `-D warnings`, `cargo test` arena-core/store/proto/api/replay all green.
