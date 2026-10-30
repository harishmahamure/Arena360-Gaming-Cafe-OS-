---
title: "Story 8.31: Credit reminders go out on cycle days, and Free shows an overdue list"
type: 'feature'
created: '2026-10-30'
status: 'review'
baseline_commit: 'dbb9042'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
deferred: []
---

<intent-contract>

## Intent

**Problem:** Players who owe credit are only chased by staff. The drain (8.29) and the cloud (8.28) can send a `credit_reminder`, but nothing queues one, and the Hub has no list of who is overdue.

**Approach:** `CreditReminderTask` (primary Hub, every 5 min) asks `credit_reminder::due` who is due a reminder today (from 11:00 branch time) and applies each as a `notify.requested` system intent. The row is one per player per business day (unique index), so repeats and late starts are harmless. A new read, `GET /ui/players/overdue`, feeds Players → Overdue.

## Boundaries & Constraints

**Always:**
- A reminder is due when the player has credit outstanding, their oldest unpaid spend (FIFO aging, 3.20) is exactly as many days old as one of their `reminder_days` (default 3, 7, 14), the branch is paid, "Credit reminder" is on and their latest transactional consent is granted. Nothing before 11:00 on the branch clock.
- `notify.requested` (kind `credit_reminder`, no `session_id`, `params.amount_minor`) is refused unless the player owes credit, and writes the owed amount at queue time. At most one row per `(player_id, reminder_day)`, whoever asks (8.32's manual reminder shares the rule).
- A Hub that was off at 11:00 runs the check once for that business day when it starts; earlier days are never made up (only today's age is compared).
- The drain skips a reminder whose player owes nothing any more (`credit_cleared`) and sends the amount owed at send time.
- The Overdue list is read-only: players with credit outstanding, oldest unpaid first, amount, aging bucket of the oldest spend, consent, and on paid branches the business day of the last reminder sent. On Free it shows a tier-lock line "Reminders are available on Basic" and no reminder is ever written.

**Never:**
- No phone number in a row or intent. No new setting.

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0080_credit_reminders.sql` -- table rebuild: kind `credit_reminder`, `reminder_day`, unique `(player_id, reminder_day)` for credit reminders.
- `crates/arena-store/src/notify.rs`, `lib.rs` -- `reminder_day`, `credit_reminder_exists`, `last_credit_reminder_sent`, `Store::debtors`.
- `crates/arena-proto/src/notify.rs`, `report.rs` -- kind, `CREDIT_REMINDER_MINUTE|INTERVAL_S`, `NotifyRequested.session_id` optional, `OverdueRow`/`OverdueView`.
- `crates/arena-core/src/credit_reminder.rs` -- `due` (pure read), 11:00 rule.
- `crates/arena-core/src/apply/notify_request.rs` -- credit gates, today's business day, once-a-day.
- `crates/arena-core/src/notify_drain.rs` -- `credit_still_owed` rule, `amount` parameter.
- `crates/arena-core/src/overdue.rs`, `crates/arena-api/src/ui_auth.rs` -- the read and `GET /ui/players/overdue` (`player.view`).
- `crates/arena-tasks/src/credit_reminder.rs`, `crates/arena-hub/src/lib.rs` -- the task and its wiring.
- UI: `apps/hub/src/lib/players/OverdueList.svelte`, Players page view switch, `loadOverdue`, strings, regenerated TS types.

## Tasks & Acceptance

**Execution:**
- [x] Migration 80, store queries, proto kind/consts/payloads, regenerated TS types.
- [x] `notify.requested` for `credit_reminder` with gates and once-a-day rule.
- [x] `credit_reminder::due` and `CreditReminderTask`, Hub wiring.
- [x] Drain: `credit_still_owed` rule and `amount` parameter.
- [x] `overdue_view`, `GET /ui/players/overdue`, Players → Overdue with the Free tier lock.
- [x] Tests: store per-day rule, apply/due/drain/overdue (core), API permission, Playwright for the view.

**Acceptance Criteria:**
- [x] Given a paid branch with "Credit reminder" on and a consenting player on a cycle day, when the check runs from 11:00, then one `credit_reminder` row (outstanding amount) is written.
- [x] Given a player not on a cycle day, then nothing is written.
- [x] Given no consent, a guest balance (no profile) or the switch off, then no reminder is written and the player stays on the overdue list.
- [x] Given the Hub was off at 11:00, when it starts the same business day, then the check runs once for that day and missed days are not sent.
- [x] Given Players → Overdue, then it lists players oldest unpaid first with amount, bucket, consent and last reminder date; tapping a row opens the profile.
- [x] Given a Free branch, then the same list shows the "Reminders are available on Basic" lock and no reminder is written.

## Spec Change Log

- 2026-10-30: created and implemented.

## Design Notes

- The "once per day" rule lives in the outbox (unique index), not in a last-run marker: the task may run every 5 minutes and a restart cannot repeat or skip a day.
- 11:00 is the branch's local clock; the day is the business day (a late-night hour before the day start still belongs to the previous day, whose 11:00 has passed).
- A reminder names what is owed when it is queued and again when it is sent (`credit_still_owed` skips a cleared tab).
- `last_reminder_day` is the business day of the latest row in state `sent`; queued, skipped or failed rows do not count.
- 8.32's `credit.remind` reuses `notify.requested`; its "already sent today" warning reads the same `reminder_day`.

## Verification

- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-hub -p arena-api --all-targets -- -D warnings`: clean.
- `cargo test` over those crates: all pass. One run of `proptest_apply` failed once while the whole workspace was testing in parallel and passed in 8 later runs (including 3 of 200 cases); no regression file was written.
- `pnpm run check:protocol`, `check:strings`, prettier on touched files: pass. Playwright `players-overdue.spec.ts` and `credit-aging.spec.ts`: pass on desktop and phone. `svelte-check` reports only the two existing `playwright.kitchen.config.ts` errors.
