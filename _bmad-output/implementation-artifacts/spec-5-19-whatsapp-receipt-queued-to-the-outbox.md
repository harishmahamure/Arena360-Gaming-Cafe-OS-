---
title: 'Story 5.19: WhatsApp receipt queued to the outbox'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '496ab38'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-17-choose-print-or-none-at-settle-and-print-through-the-side-ef.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-15-receipt-and-notification-settings.md'
---

## Intent

**Problem:** A player who agreed to service messages can only get a paper receipt. Counter staff have no way to send the copy to the player's WhatsApp, and nothing records that a message is owed.

**Approach:**

- The receipt choice gains `whatsapp` (`ReceiptChoice::Whatsapp`) beside `print` and `none`. `session.settle`, `pos.sale` and `plan.sell` take it. The settle bar adds WhatsApp when the Hub says it is offered: the sale is for a profiled player whose latest `transactional` consent is granted, the branch is on a paid tier (`licence.tier` basic or premium), and `notify.whatsapp_receipt` is on.
- A settle with the choice writes one row on the new change-log table `notify_outbox` (purpose `transactional`, kind `receipt`, the order, the player, the consent policy version, `not_before` = commit + the 8 s Undo window, state `queued`) inside the settle transaction. The row names the player and never the phone number; Epic 8's drain decrypts the recipient when it sends. No invoice number is issued and no irreversible effect is recorded, so Undo stays open.
- An Undo of the settle (`session.settle.reverted`, `pos.sale.reverted`, `plan.sell.reverted`) marks the order's queued row `skipped`, so it is never sent.
- Rows stay `queued` until the Epic 8 drain. Recent sales on POS shows "WhatsApp receipt queued" for such a sale.

## Boundaries & Constraints

**Always:**

- Refusals are decided in order: a Free branch is `FEATURE_NOT_ENTITLED` (field `receipt`); then a guest, a player with no consent row, or a player whose latest consent is not granted is the new `CONSENT_REQUIRED`; then the switch off is `VALIDATION_FAILED`. A refused settle rolls back whole (no sale, no stock move, no row).
- The consent that decides is the latest `consents` row for the player and purpose `transactional` (the same rule the player screens use); its `policy_version` is copied to the row.
- A bill with no total takes no invoice number, so there is no receipt to send: the choice is accepted and queues nothing.
- `GET /ui/printer/ready?player_id=` answers `whatsapp: true` only when the same rule passes; the field is absent otherwise. The bill sent by a counter sale and a plan sale carries `player_id` for it (`SnapshotBill.player_id`, new, absent for a guest).

**Never:**

- Sending a message, the cloud send path, `notify.requested` as a standalone intent, `notify.sent` / `notify.failed`, or a drain (Epic 8).
- A phone number, name or message text in `notify_outbox` or the change log.
- Making WhatsApp a branch default (`receipt.default` stays `print` or `none`).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Consenting player, paid, switch on | settle with `whatsapp` | one `queued` row, `not_before` = now + 8 s, no number, no effect | none |
| Undo in the window | revert of that settle | row `skipped` (stays as the record) | none |
| Guest | `whatsapp`, no player | refused, nothing written | `CONSENT_REQUIRED` |
| No consent / withdrawn | latest consent not granted | refused | `CONSENT_REQUIRED` |
| Free branch | `licence.tier` free or absent | refused | `FEATURE_NOT_ENTITLED` |
| Switch off | `notify.whatsapp_receipt` false | refused | `VALIDATION_FAILED` |
| Zero-total bill | `whatsapp` | settles, no row | none |
| Settle bar | `ready?player_id=` | `whatsapp: true` only when allowed | none |

## Code Map

- `crates/arena-proto/src/receipt.rs` -- `ReceiptChoice::Whatsapp`, `NOTIFY_*` constants, `WhatsappRefusal` and `whatsapp_refusal()` (shared by `apply()` and the API); `error.rs` -- `ConsentRequired` (HTTP 422); `printer.rs` -- `PrinterReady.whatsapp`; `pos.rs` -- `PosSaleView.whatsapp_state`; `session.rs` -- `SnapshotBill.player_id`.
- `crates/arena-store/migrations/0052_notify_outbox.sql`, `src/notify.rs` (`insert_notify`, `queued_notify_ids`, `skip_notify`, `latest_receipt_state`, `order_player_id`), `src/branch.rs` (`Table::NotifyOutbox`, `RowKey::NotifyOutbox`, `Table::ALL` 39), `src/sessions.rs` (recent sales state).
- `crates/arena-core/src/apply/receipt_notify.rs` -- `queue_on_settle` (called from `receipt_print::queue_on_settle` for the `whatsapp` choice) and `skip_on_revert` (called by the three revert mutations); `src/snapshot.rs` -- `SnapshotBill.player_id`.
- `crates/arena-api/src/ui_printer.rs` -- `ready` takes `?player_id=`.
- `apps/hub/src/lib/SettleBar.svelte`, `printer.ts`, `routes/pos/+page.svelte`, `lib/players/SellPlan.svelte`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/receipt.rs` (5 new), `receipt.rs` proto (1), `notify.rs` store (2), `ui_printer.rs` (1), `arena-replay` seed (a WhatsApp sale), `proptest_apply.rs` (`PosSale.whatsapp`, `assert_notifications_hold`), Playwright `settle.spec.ts` (3) and `pos.spec.ts`.

## Tasks & Acceptance

- [x] Protocol: choice, error code, shared refusal rule, `PrinterReady.whatsapp`, `SnapshotBill.player_id`, `PosSaleView.whatsapp_state`.
- [x] Store: migration 0052, `notify_outbox` as a change-log table, row queries.
- [x] Core: queue the row at settle with consent, tier and switch checks; skip it on Undo.
- [x] API: `ready?player_id=`.
- [x] Hub UI: WhatsApp choice on the settle bar, "WhatsApp receipt queued" on Recent sales.
- [x] Tests: Rust matrix, replay seed, proptest op and invariant, Playwright.
- [x] Verification gate.

**Acceptance Criteria:**

- Given the settle bar for a profiled player with current consent on a paid tier, when the receipt choice renders, then WhatsApp is added beside Print and None.
- Given a settle with receipt choice WhatsApp for such a player, when it commits, then a `notify_outbox` row (transactional, consent policy version, receipt, order id, `not_before` = `undo_until`, queued) exists and no phone number is in the change log.
- Given a guest, a player without consent, or a Free branch, then WhatsApp is not offered and an API attempt is `CONSENT_REQUIRED` or `FEATURE_NOT_ENTITLED`.
- Given the settle is undone within the window, then the row is `skipped` and will never be sent.
- Given queued rows, then they stay queued for the Epic 8 drain, which sends after `not_before` once the invoice number exists; the order shows "WhatsApp receipt queued".

## Verification

- `cargo test` for `arena-proto`, `arena-store`, `arena-core` (lib and `proptest_apply`), `arena-api`, `arena-replay`, `arena-hub` pass; `cargo clippy -D warnings` (workspace except `arena-e2e`, which does not build on Windows, and the existing `arena-kiosk-svc` `discovery` test warnings) and `cargo fmt --check` are clean; `pnpm gen:types`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, prettier and Hub `svelte-check` pass; Playwright `settle`, `pos`, `receipts` pass.
- Failures that are not from this story (also fail on the baseline): `arena-tasks` `a_printer_that_stops_reading_times_out` and `trusted_time_is_cloud_time_plus_monotonic_elapsed`; `arena-cloud` `binary` `unreachable_at_start_retries_then_exits_1_without_logging_the_token` (and, intermittently, `backup` `a_hung_run_times_out_with_its_own_code`).
- `proptest_apply` at more than the default 64 cases finds older, unrelated generator failures (for example a rule edit with a stale `base_rev`); one in `PlanSell` (a price of 1 with `short`) was fixed here.

## Notes for later stories

- Epic 8's drain reads `notify_outbox` where `state = 'queued'` and `not_before` has passed, re-checks the player's current consent, skips erased players and rows whose settle was reverted, posts to `/v1/notify`, and applies `notify.sent` / `notify.failed`; it must not send before `orders.invoice_no` exists. It also registers "notification sent" with the irreversible-effect guard (this story registers none).
- The row is written by the settle itself; there is no standalone `notify.requested` intent yet. Epic 8's other kinds (`low_balance`, `session_ending`, owner alerts) add it, along with the `kind` and `purpose` values the table's checks allow today (`receipt`, `transactional` only).
- Only POS Recent sales shows the queued state; the Sessions screen and tile sheet do not yet.
