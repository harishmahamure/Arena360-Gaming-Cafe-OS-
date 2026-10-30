---
title: "Story 8.28: The cloud sends player and owner messages through a swappable messaging adapter, exactly once"
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: '9c805f4'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Hub has a `notify_outbox` (Story 5.19) but nothing sends it. Owners must not need a provider account, and a player must never get the same message twice.

**Approach:** `POST /v1/notify` (paid Hub device token) inserts a `send_ledger` row keyed by the Hub's `outbox_id`, picks the current approved template version of the message kind and sends it through the messaging adapter: WhatsApp first, the same message by SMS when WhatsApp is rejected. A repeat of an `outbox_id` returns the stored result and sends nothing. The adapter is Gupshup in production, the recorded fake in dev and CI, or `manual` (logs the message for an operator).

## Boundaries & Constraints

**Always:**
- Body `{outbox_id, kind, purpose, consent_policy_version, recipient, params}` (`NotifyRequest`, `deny_unknown_fields`). `kind` is one of `MESSAGE_KINDS` (credit_reminder, low_balance, session_ending, console_5min, receipt); `recipient` is E.164; `params` holds exactly the template's parameter names, plain text up to 200 characters. Anything else is `VALIDATION_FAILED` and writes nothing.
- A Free branch is refused `FEATURE_NOT_ENTITLED` before any write. No adapter configured is `UNAVAILABLE` (503).
- `send_ledger` (gz_control migration 22): one row per `outbox_id` (unique), `recipient_hash` = SHA-256("notify\0" ‖ pepper ‖ phone), never the number or the text; `template_version`; `channel`; `state` queued|sent|failed; `attempts`; `attempt_log` (one entry per provider attempt); `in_flight`.
- Exactly once: the row is inserted, then claimed (`in_flight = 1`, atomic) before a send; only the claimer sends. A row that is `sent`, `failed` or claimed answers its stored result. An outcome that proves nothing went out (connect failure, HTTP 429) settles the row `queued` and unclaimed, so the Hub's next post retries it. An unknown outcome (timeout, 5xx, lost reply) settles it `queued` and claimed: it is never sent again.
- A non-retryable WhatsApp rejection is followed by an SMS in the same call; the ledger logs both attempts and the final channel. A rejected SMS ends `failed` with the code (`notify.failed` in Story 8.29).
- `message_templates (kind, version, whatsapp_template_id, sms_dlt_template_id, sms_text, param_names, approved_at)`; version 1 of the five kinds is seeded. A change is a new version (`add_template_version`); the current one is the newest with `approved_at`. A row keeps the version it was created with, also on a retry.
- Logs and traces carry the outbox id and branch id only. `manual` mode is the one place a number and text are logged, because the operator sends them.
- A `outbox_id` of another branch is `IDEMPOTENCY_CONFLICT`.

**Never:** a second send for one `outbox_id`; sending for Free; storing or logging the recipient or text outside manual mode; Hub-side drain, consent re-check or `notify.sent` (Story 8.29).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First send | paid Hub, valid body | `sent`, channel whatsapp, `provider_message_id`, attempts 1 | — |
| Repeat | same `outbox_id` | stored result, nothing sent | — |
| WhatsApp rejected | adapter rejects | SMS sent, channel sms, attempts 2, both logged | SMS rejected too: `failed` + code |
| Transient | connect failure / 429 | `queued`, unclaimed; next post retries | — |
| Unknown outcome | timeout / 5xx | `queued`, claimed; never resent | — |
| Free branch | no paid entitlement | `FEATURE_NOT_ENTITLED`, no row | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/notify.rs` -- `NotifyRequest|Response|State|Channel`, `MESSAGE_KINDS`, `valid_e164` (types generated to `packages/ui/src/generated`).
- `crates/arena-store/control-migrations/0022_send_ledger.sql`, `src/control/send_ledger.rs` -- tables, seed templates, `current_template`, `template_version`, `add_template_version`, `ledger_insert|claim|settle|by_outbox`.
- `crates/arena-cloud/src/messaging/notify.rs` -- `Notifier` (Fake | Manual | Gupshup), `Channel`, `Outgoing`, `SendFailure`, `FakeNotifier` with per-channel `Behaviour`; `gupshup.rs` gains `send_with_template` (per-kind DLT id).
- `crates/arena-cloud/src/api/notify.rs` -- the route; `auth.rs` `AuthState.notifier` + `recipient_hash`; `lib.rs` `auth_state` builds the notifier; `api/dev.rs` `GET /v1/dev/notify`.
- Tests: `crates/arena-cloud/tests/notify.rs` (14), `messaging::notify` unit tests, `arena-proto` unit tests; migration table lists in `tests/backup.rs` and `arena-store/tests/control.rs`.

## Tasks & Acceptance

**Execution:**
- [x] Proto types, migration 22 with the five seeded templates, store ledger and template API.
- [x] Adapter: recorded fake, manual mode, Gupshup WhatsApp template send and SMS with a per-template DLT id; env config.
- [x] `POST /v1/notify`: entitlement gate, validation, ledger claim, WhatsApp-then-SMS fallback, stored-result replay.
- [x] Tests for the matrix, concurrency, template versions, hash-only storage and the no-adapter 503.

**Acceptance Criteria:**
- [x] Given `POST /v1/notify` from a paid Hub, then a `send_ledger` row is inserted with the salted recipient hash and the current template version and the message goes through the adapter.
- [x] Given the same `outbox_id` again, then nothing is sent and the original result is returned.
- [x] Given a non-retryable WhatsApp failure, then the message is sent by SMS and the ledger records both attempts and the final channel.
- [x] Given a Free branch, then `FEATURE_NOT_ENTITLED` and nothing is sent.
- [x] Given templates for the five kinds, then each has a version, the approved WhatsApp template id and an SMS text, and a change is a new version that leaves earlier ledger rows alone.
- [x] Given dev and CI, then the recorded fake captures sends and `manual` mode logs the rendered message.
- [x] Given logs and traces, then they carry the outbox id and branch id only (manual mode excepted by design).

## Spec Change Log

- The story lists `message_templates` columns without the DLT template id or parameter names. Both are added: an SMS under India's DLT rules needs a registered template id per message, and WhatsApp takes parameters by position.
- Seeded WhatsApp and DLT template ids (`arena360_*_v1`, `DLT-ARENA360-*-1`) are placeholders until the provider approves the real ones; the operator then adds a new version.
- Production config: `ARENA_NOTIFY_MODE` (gupshup default, or manual) and `ARENA_GUPSHUP_WA_API_KEY|SOURCE|APP` (+ optional `_ENDPOINT`). Missing WhatsApp variables do not stop the cloud: `/v1/notify` answers 503 and a warning is logged.

## Design Notes

- The ledger claim is a single `UPDATE ... WHERE state='queued' AND in_flight=0`; two simultaneous posts of one id send one message (test `two_posts_at_once_send_one_message`).
- An uncertain send is deliberately never retried: a duplicate message is worse than one the Hub reports as unconfirmed. Story 8.29 treats `queued` + replied-twice as in doubt.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store --features control -p arena-cloud`, `cargo clippy -p arena-proto -p arena-store -p arena-cloud --all-targets -- -D warnings`, `cargo fmt --check`, `pnpm run gen:types`, `check:layers`, `check:protocol`, `check:strings`, `check:secrets`, `format:check` -- expected: clean

## Auto Run Result

Status: in-review. Not run: a send through a live Gupshup account (no credentials here), the code-review pass. Known unrelated failures on Windows: `arena-cloud` `binary` test `unreachable_at_start...` and the timing test `a_hung_run_times_out_with_its_own_code` (also fails on a clean tree).
