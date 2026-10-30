---
title: "Story 8.34: The Hub raises owner alerts for cash variance, low stock, overdue credit and large refunds or discounts"
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: '392fe2d332dc56cad93a6d6e4a5445c97cac24fc'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The cloud can keep, push and WhatsApp an owner alert (8.33) but nothing at the branch raises one. An owner learns of a cash gap, a product running out, credit gone 15 days overdue or a big refund or discount only by opening reports.

**Approach:** The Hub queues a `notify_outbox` row of purpose `owner_alert` in the transaction of the event (shift close, low-stock crossing, credit auto-pause, refund, discount), due after the Undo window. The existing drain posts it to `POST /v1/notify`, where the cloud raises the 8.33 alert under the outbox id. Per-tenant thresholds are one setting, `owner_alert.thresholds`, set by a `settings.set` owner command from a new Thresholds section in the Alerts tab.

## Boundaries & Constraints

**Always:**
- `arena-proto`: `NOTIFY_PURPOSE_OWNER_ALERT = "owner_alert"`; `OwnerAlertKind::HUB_SOURCED = [CashVariance, LowStock, CreditOverdue, LargeRefund, LargeDiscount]`; `CashVariance` gains optional params `reason`, appended to its line as ` (<reason>).` ; `NotifyChannel::App` (an alert reached the owner's app, not a phone); `KEY_OWNER_ALERT_THRESHOLDS` and `OwnerAlertThresholds {cash_variance_minor: Option<i64> (None = the branch's variance threshold), large_refund_minor, large_discount_minor, large_discount_percent}` with defaults 50000 / 20000 / 20, `parse` (strict JSON, minor 0..=MAX, percent 1..=100) and `SETTINGS_COMMAND_KEYS` gains the key.
- Hub migration 81 rebuilds `notify_outbox`: purpose also `owner_alert`; kind also the five alert kinds; `player_id` nullable (no player); `channel` also `app`; new `source_ref`; a row of purpose `owner_alert` needs `source_ref`; unique `(kind, source_ref)` for `owner_alert`. `NewNotify.player_id` empty means no player.
- The setting row `owner_alert.thresholds` is seeded with the defaults JSON (rule: valid `OwnerAlertThresholds`); a Hub that already has no row gets none (reads fall back to defaults).
- Every alert is queued only on a paid tier with a primary write: a Free branch queues nothing. `not_before` = commit + `UNDO_WINDOW_MS`. Text parameters (staff name, product name) are cut to 40 characters; money is stored raw as `amount_minor` and formatted at drain time into `amount` with the branch money format; the row names no player but `player_id` of a `credit_overdue`.
- Sources, once per event: `shift.close` with |variance| over `cash_variance_minor` (default: the branch variance threshold) → `cash_variance` (`source_ref` shift id; params `amount_minor` = |variance|, `shift` = closing staff name, `reason`); `raise_low` of a Hub alert → `low_stock` (`source_ref` alert id; `product`, `qty`); `credit.auto_pause` turning a pause on → `credit_overdue` (`source_ref` `<player>:<day>`; `amount_minor` owed, `days` = oldest age, `player_id`); `refund.create` with refunded total over `large_refund_minor` → `large_refund` (`source_ref` credit-note/refund id); a manual discount over `large_discount_minor` or over `large_discount_percent` of the bill before it → `large_discount` (`source_ref` order id, `order_id` set so the Undo of the settle skips it through `skip_on_revert`).
- Drain: owner-alert rows skip the player and consent rules; a `low_stock` row is skipped (`stock_recovered`) when its Hub alert is no longer open; the request is `{kind, purpose owner_alert, consent_policy_version "owner", recipient "", params}`. The cloud, for purpose `owner_alert` only, requires a Hub-sourced kind, validates params with `check_params`, skips the recipient check, and `raise()`s with `id = outbox_id` for the Hub's branch (idempotent by id), answering `{state sent, channel app}`; every other purpose behaves as before. `notify.sent` for channel `app` stores channel `app`.
- Owner app: Alerts gets a Thresholds section (cash variance with "Branch default" empty, large refund, large discount amount and percent, in the branch money unit) that sends one `settings.set` for key `owner_alert.thresholds` to all branches through the normal preview/confirm flow. Strings in `en.json`.

**Never:** Quiet hours, digests, new Owner-app alert channels, the offline detector (8.35), per-branch threshold overrides, any alert for a Free branch, personal data in an alert (no phone, name of a player, email).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Variance over | Close with |variance| > threshold (manager approved) | One `cash_variance` row | No error |
| Variance under | |variance| ≤ threshold | No row | No error |
| Low-stock crossing | Stock falls to ≤ level | One `low_stock` row per Hub alert | No error |
| Recovered | Stock back above level before the drain | Row skipped `stock_recovered` | No error |
| Overdue | Auto-pause turns a player's pause on | One `credit_overdue` row | No error |
| Large discount undone | Settle with discount over limit, then Undo | Row skipped | No error |
| Free tier | Any source | Nothing queued | No error |
| Drain post | Row due | Cloud raises one alert under the outbox id; repeat adds none | `VALIDATION_FAILED` for a bad kind or parameter |
| Thresholds | Owner saves | `settings.set` to all branches; Hub stores JSON | `VALIDATION_FAILED` for an out-of-range value |

</intent-contract>

## Code Map

- `crates/arena-proto/src/owner_alert.rs`, `notify.rs`, `commands.rs`, `lib.rs` -- kind helpers, thresholds type, channel, key.
- `crates/arena-store/migrations/0081_owner_alert_rows.sql`, `src/migrations.rs`, `src/notify.rs` -- table rebuild, `source_ref`, nullable player; migration-count tests move to 81.
- `crates/arena-core/src/apply/owner_alert.rs` (new) -- `queue()` used by the five sources; threshold read; hooks in `shift.rs` (`mutate_close`), `stock.rs` (`raise_low`), `credit_pause.rs` (`mutate`), `refund.rs` (`mutate`), `discount.rs` (`record`).
- `crates/arena-core/src/notify_drain.rs` -- rules and request for owner-alert rows; `settings_defaults.rs` -- seed and rule; `commands.rs` -- key allowed.
- `crates/arena-cloud/src/api/notify.rs`, `api/commands.rs` -- purpose `owner_alert` branch; settings key check.
- `apps/owner/src/lib/AlertsBoard.svelte`, new `ThresholdsForm.svelte`, `commands.svelte.ts` (`whatOf`), `packages/ui/src/messages/en.json`, `packages/ui/src/generated/*`.

## Tasks & Acceptance

**Execution:**
- `crates/arena-proto/...` -- add the constants, thresholds, channel, params -- one definition for Hub, cloud and app
- `crates/arena-store/...` -- migration 81 and `NewNotify`/`NotifyRow` changes -- rows without a player
- `crates/arena-core/src/apply/owner_alert.rs` and the five hooks -- queue rows in the event's transaction -- exactly once per event
- `crates/arena-core/src/notify_drain.rs`, `settings_defaults.rs`, `commands.rs` -- drain and setting -- skip rules, thresholds by command
- `crates/arena-cloud/src/api/notify.rs`, `commands.rs` -- accept the posted alert and the key -- cloud half
- Tests: Hub (`apply/tests/owner_alert.rs`), cloud (`tests/owner_alerts.rs` additions), owner app (`tests/alerts.spec.ts` additions) -- matrix rows

**Acceptance Criteria:**
- Given the thresholds, when a shift closes over the cash limit, a stock level crosses, a player is auto-paused, or a refund or discount is over its limit, then one owner-alert row waits for the end of the Undo window.
- Given a due row, when the drain runs, then the cloud holds one alert for that outbox id with no player personal data, and the alert shows in the Alerts tab.
- Given an Undone settle with a large discount, when the drain runs, then no alert is raised.
- Given Alerts → Thresholds, when the owner saves, then a `settings.set` reaches every branch and the Hub stores `owner_alert.thresholds`.
- Given typecheck, lint, Rust and Playwright suites and `pnpm check:generated`, `check:strings`, they pass.

## Spec Change Log

## Review Triage Log

## Design Notes

A credit alert rides the auto-pause flip because that is the Hub's single "reached 15 days overdue" moment (a flip back and a new crossing is a new event); with auto-pause switched off no alert is raised. The row `source_ref` unique index is the once-per-event guard, so an intent replay or a second hook call cannot queue twice.

## Verification

**Commands:**
- `cargo test -p arena-store -p arena-proto -p arena-core -p arena-tasks -p arena-cloud` -- expected: all pass
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` -- expected: clean
- `pnpm check:generated && pnpm check:strings && pnpm check:pins` -- expected: ok
- `pnpm --filter @arena/owner check && pnpm --filter @arena/owner test` -- expected: pass

## Auto Run Result

Status: in-review
Implemented by subagent; verified with cargo test (store, proto, core, tasks, cloud), clippy, fmt, check:strings, check:pins, owner svelte-check and the owner Playwright suite. Pre-existing unrelated failures: cloud backup `a_hung_run_times_out_with_its_own_code`, tasks `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `a_printer_that_stops_reading_times_out`.
Open notes for review: `settings.set` added to the handled owner kinds (17); thresholds key seeded in `DEFAULT_SETTINGS`; a re-settled order after an Undo queues no second discount alert; the limits form starts at defaults and does not read back stored limits.
