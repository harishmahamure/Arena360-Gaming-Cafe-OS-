---
title: "Story 8.29: The Hub drains its notification outbox after the undo window"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '1bc1d45'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub queues a `notify_outbox` row for a WhatsApp receipt (Story 5.19) and the cloud can send it (Story 8.28), but nothing carries the row across. A message must go out only once the sale can no longer be undone, only while the player still agrees, and never twice.

**Approach:** `NotifyDrainTask` (paid, primary, paired Hub; every 10 s) asks the snapshot reader for a plan: each due row is sent, left waiting, skipped, or expired by a list of per-row rules. It posts `Send` rows to `/v1/notify` keyed by the row ULID, in `not_before` order, and applies the result as a system intent: `notify.sent` (channel), `notify.failed` (code), `notify.skip` (reason) or `notify.expire`. `notify.sent` registers the irreversible effect `notification_sent` against the row's order, so the sale can no longer be undone.

## Boundaries & Constraints

**Always:**
- A row is due when `state = queued` and `not_before <= now`. Rows go out in `not_before` order, at most 20 per pass.
- Per-row eligibility is `notify_drain::RULES`, run in order, first non-pass decides: `player_exists` (else skip `player_gone`), `consent_current` (latest consent of the row's purpose granted, else skip `consent_withdrawn`), `receipt_ready` (a receipt needs a settled order, else skip `order_reverted`; no invoice number yet is `Wait`). Later stories add rules (erasure in Epic 9, "session no longer near its end" in 8.30) by appending to the list.
- A row more than 7 days past `not_before` is `expired` and is never posted.
- The recipient is decrypted from the player's `phone_enc` with the player KEK only when posting; a phone that is not E.164 skips the row (`phone_invalid`). Nothing but the row id and the outcome is logged.
- A cloud answer of `sent` applies `notify.sent`; `failed` applies `notify.failed` with its `error_code` (`send_failed` when none); `queued` (outcome in doubt) records nothing and the row is asked about again after 60 s. The cloud's send ledger answers a repeat of the same row with the original result, so a lost reply never sends twice.
- Unreachable cloud, 429, 5xx and `TOKEN_REVOKED` stop the pass and leave every row queued. `VALIDATION_FAILED` and `IDEMPOTENCY_CONFLICT` fail the row.
- Each result applies to a `queued` row once; a repeat is `STALE` and changes nothing. A Free or follower Hub drains nothing.

**Never:**
- Never post a row whose action was undone, whose consent was withdrawn, or whose invoice number does not exist yet.
- Never send the plain phone number into the change log or the audit.

</intent-contract>

## I/O & Edge-Case Matrix

| Scenario | Behaviour |
|---|---|
| Row inside the Undo window | Not due; nothing happens |
| Receipt due, no invoice number | `Wait`; stays queued |
| Receipt due and numbered | Posted with `name`, `receipt_no`, `amount`, `cafe`; `notify.sent` + `notification_sent` effect |
| Consent withdrawn after queueing | `notify.skip` (`consent_withdrawn`), nothing posted |
| Settle undone | Already `skipped` by 5.19; any leftover is skipped `order_reverted` |
| WAN down | Pass posts nothing; rows stay queued and go out in `not_before` order on return |
| Row older than 7 days | `notify.expire`, never posted |
| Cloud lost-reply retry | Ledger returns the stored result; no second message |
| Cloud answers `queued` | Left; asked again after 60 s |

## Code Map

- `crates/arena-store/migrations/0078_notify_drain.sql`: `notify_outbox.state` gains `expired`, columns `channel`, `error_code`, `reason`; `irreversible_effects.kind` gains `notification_sent`.
- `crates/arena-store/src/notify.rs`: `list_due_notify`, `finish_notify`; `Store::due_notify`, `Store::invoice_order`.
- `crates/arena-proto`: intents `notify.sent|failed|skip|expire` (system, category Money), payloads `NotifySent|NotifyFailed|NotifySkipped|NotifyExpired`, `EffectKind::NotificationSent`, `NOTIFY_EXPIRY_MS`, `NOTIFY_DRAIN_INTERVAL_S`.
- `crates/arena-core/src/apply/notify_drain.rs`: load/validate/mutate of the four results.
- `crates/arena-core/src/notify_drain.rs`: `RULES`, `plan`, `SnapshotReader::notify_plan` (adds `amount` and `cafe`).
- `crates/arena-tasks/src/notify.rs`: `NotifyDrainTask`; `CloudClient::notify`; wired in `crates/arena-hub/src/lib.rs`.
- UI: audit labels for the four intents; regenerated TS types.

## Tasks & Acceptance

**Tasks:**
- [x] Migration 78, store queries, proto intents/payloads/effect kind, regenerated TS types and audit labels.
- [x] Apply module for `notify.sent|failed|skip|expire` including the irreversible effect.
- [x] Per-row rules, `plan`, `SnapshotReader::notify_plan` with receipt parameters.
- [x] `NotifyDrainTask`, `CloudClient::notify`, Hub wiring and shutdown.
- [x] Tests: apply results and effect, Undo refused after a send, plan decisions (wait, send, consent, expiry), store ordering, cloud-reply mapping.

**Acceptance Criteria:**
- [x] Given queued rows whose `not_before` has passed, when the drain runs (every 10 s, paid Hub), then each is eligibility-checked (extendable rules), reverted ones are skipped, the recipient is decrypted and posted keyed by the row ULID.
- [x] Given a cloud reply, then `notify.sent` (channel) or `notify.failed` (code) is applied and "notification sent" is registered so the forward action can no longer be undone.
- [x] Given a receipt row whose order has no invoice number, then the drain waits for it.
- [x] Given the WAN is down, then rows stay queued and go out in `not_before` order later; rows older than 7 days are `expired`.
- [x] Given consent withdrawn after queueing, then the row is `skipped` and nothing is posted.
- [x] Given a lost reply, then the cloud's send ledger returns the original result and no second message is sent.

## Spec Change Log

- 2026-10-30: created and implemented.

## Design Notes

- Decisions live in `arena-core` and are pure reads, so they are testable without a cloud; the task only transports and records.
- The expiry reference is `not_before` (the row's `created_at` is not set at insert).
- The task test of the full post path needs a fake cloud server; the cloud side is covered by Story 8.28's tests and the reply mapping by unit tests here.

## Verification

- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-hub --all-targets -- -D warnings`: clean.
- `cargo test` over the same crates plus `arena-api` and `arena-cloud`: all pass except three failures that also fail on the base commit (`arena-tasks` `trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-cloud` `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`). `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` failed once and passed on rerun.
- `pnpm run check:protocol`, `check:strings`, prettier on touched files: pass. `check:generated` reports only the uncommitted regenerated files.
- `arena-kiosk-svc` and `arena-e2e` do not compile on Windows on the base commit; not touched.
