---
title: "Story 8.30: Low-balance, session-ending and console 5-minute messages"
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: '79d0598'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
deferred: []
---

<intent-contract>

## Intent

**Problem:** The drain (8.29) and the cloud (8.28) can send a message, but nothing queues the three session messages a consenting player asked for: wallet running low, PC session about to end, console time nearly up.

**Approach:** `NotifyTriggerTask` (primary Hub, every 10 s) asks `notify_trigger::triggers` which messages a running session is due to send and applies each as a system intent: `notify.requested` queues one `notify_outbox` row (due at the end of the Undo window), `notify.rearm` closes a low-balance crossing. The 8.29 drain sends the rows; a new rule skips a session message whose session is no longer near its end.

## Boundaries & Constraints

**Always:**
- `low_balance`: a profiled player's wallet session whose remaining balance (free wallet plus the unused share, as the kiosk's `wallet_left_minor`) is at or below `billing.low_balance_minor` (off when 0), once per crossing; a later pass that sees the balance above the level sets `rearmed_at` on the latest row so the next fall is a new crossing.
- `session_ending` (PC) and `console_5min` (device kind `console`, names the TV): the session is active or paused with an end time 5 minutes or less away; at most one row of each per session ever (unique index), so an Extend never repeats it. Postpaid sessions (no end time) and grace sessions get none.
- `notify.requested` is refused unless the tier is paid, the message switch is on (`notify.low_balance`; `notify.session_ending` covers both session messages), the player's latest transactional consent is granted, the session is running and the player's own, and `params` match the kind. No row otherwise (guest, no consent, Free, switched off).
- New drain rule `session_near_end`: a `session_ending`/`console_5min` row is skipped (`session_changed`) unless its session is active or paused and 5 minutes or less from its end at send time.
- `notify_outbox.order_id` may be empty; such a row's `notify.sent` registers no irreversible effect.

**Never:**
- Never put the phone number in a row or intent; the drain still opens it at post time.
- No new setting or UI: the console message follows "Session ending".

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0079_session_notices.sql` -- table rebuild: kinds, nullable `order_id`, `session_id`, `params`, `rearmed_at`, unique `(session_id, kind)` for the two session kinds.
- `crates/arena-store/src/notify.rs` -- `NewNotify`/`NotifyRow` gain the columns; `session_notice_exists`, `latest_low_balance`, `rearm_notify`.
- `crates/arena-proto/src/notify.rs`, `intent.rs` -- kinds, `SESSION_NEAR_END_MS`, `NotifyRequested`, `NotifyRearm`, intents `notify.requested|rearm` (system).
- `crates/arena-core/src/notify_trigger.rs` -- `triggers` (pure read), `switch_key`, `param_names`, `SnapshotReader::notify_triggers`.
- `crates/arena-core/src/apply/notify_request.rs` -- gates, dedupe, row insert, rearm; wired through `Target::Notify`.
- `crates/arena-core/src/session_screen.rs` -- `wallet_left` extracted from `session_screens`, shared with the trigger.
- `crates/arena-core/src/notify_drain.rs` -- `session_near_end` rule, stored params, `balance`/`cafe` parameters.
- `crates/arena-tasks/src/notify_trigger.rs`, `crates/arena-hub/src/lib.rs` -- the task and its wiring.
- UI: audit labels and strings for the two intents; regenerated TS types.

## Tasks & Acceptance

**Execution:**
- [x] Migration 79, store queries, proto kinds/intents/payloads, regenerated TS types, audit labels.
- [x] `notify.requested` / `notify.rearm` apply with gates and once-only rules.
- [x] `notify_trigger::triggers` for the three messages and the shared `wallet_left`.
- [x] Drain: `session_near_end` rule, order-less rows, `balance` parameter.
- [x] `NotifyTriggerTask` and Hub wiring.
- [x] Tests: store dedupe and rearm, trigger + apply per message, gates, Extend, crossing/rearm, drain skip.

**Acceptance Criteria:**
- [x] Given a paid branch with "Low balance" on and a consenting profiled wallet player, when the balance first falls to the level, then one `low_balance` row is written; it fires again only after the balance rose above the level.
- [x] Given "Session ending" on and a PC session with an end time, when 5 minutes remain, then one `session_ending` row is written for that session and none after an Extend.
- [x] Given a console session, when 5 minutes remain, then one `console_5min` row naming the TV is written.
- [x] Given a guest, no consent, a Free branch or the message off, then no row is written.
- [x] Given the session ended or was extended past 5 minutes before the drain runs, then the row is skipped.

## Spec Change Log

- 2026-10-30: created and implemented.

## Design Notes

- The kiosk's low-balance toast is computed on the kiosk; the Hub recomputes the same figure with the shared `wallet_left`. A top-up seen by no running session leaves the crossing open until the next session scan.
- Detection runs every 10 s, so a message can be up to 10 s after the exact mark.
- Epic 9's erasure rule and 8.31's credit reminder (which has no session) append to `RULES`/`SESSION_NOTIFY_KINDS` neighbours.

## Verification

- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-hub --all-targets -- -D warnings`: clean.
- `cargo test` over those crates plus `arena-api`: all pass except two failures that also fail on the base commit (`arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed`, `printer::lan::tests::a_printer_that_stops_reading_times_out`).
- `pnpm run check:protocol`, `check:strings`, prettier on touched files: pass. `check:generated` reports only the uncommitted regenerated files.
