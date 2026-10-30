---
title: "Story 8.32: Reports — credit, with a one-tap reminder"
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: '8d122df0765f5fd3c7299f401d41c412a2dc29d9'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The owner cannot see who owes what at each branch, by age, from the Owner app, and cannot chase a debtor without calling the café. The rollup already carries the credit list (8.5) and the Hub already queues reminders (8.31), but nothing joins them.

**Approach:** The cloud's range answer gains `credit` (from the newest accepted rollup, like stock); Reports gets a Credit tab with aged buckets, a player list and a "Send reminder" button; the new owner command `credit.remind{player_id}` is run by the Hub as a `notify.requested` (kind `credit_reminder`). Credit joins the CSV/PDF exports.

## Boundaries & Constraints

**Always:**
- Credit shows the newest accepted rollup's date, per-branch outstanding aged 0–7 / 8–14 / 15+ days and players by id and username with outstanding and oldest unpaid date. Never phone, name or email.
- `credit.remind{player_id}` is a handled, non-sensitive Basic kind (no MFA). The Hub applies it as an owner-actor `notify.requested` (kind `credit_reminder`, `params.amount_minor` replaced at queue time). A player without consent or credit, a Free/switched-off branch, or a reminder already queued today is `rejected`, shown as "Couldn't apply at branch".
- A player without consent is shown "No consent for messages" and no button. A reminder already queued or sent today (automatic or manual) shows "Reminder already sent today" and asks to confirm before sending; the Hub still keeps one row per player per business day (8.31), so a confirmed repeat the Hub refuses reads "Couldn't apply at branch".
- "Already today" comes from the Hub's summary (`reminded_today`: player ids with a credit reminder row for the current business day in state `queued` or `sent`), additive and defaulted for older Hubs.
- CSV: one row per player per branch (player id, username, outstanding, aging bucket, oldest unpaid date) built from the same rollups; PDF is the print-styled view; Credit is listed in Exports.

**Never:**
- No phone, name or email anywhere in the answer, CSV or command. No schema bump of the rollup. No change to the once-a-day rule.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Remind | consent + credit + not reminded today | one tap sends `credit.remind`, row shows its state, Hub writes one `credit_reminder` row | none |
| Repeat | player in `reminded_today` | inline "Reminder already sent today", Send anyway / Cancel | Hub refusal shows "Couldn't apply at branch" |
| No consent | `consent=false` | "No consent for messages", no button | n/a |
| Cleared tab | player owes nothing at the Hub | command `rejected` (Stale) | "Couldn't apply at branch" |
| No rollup | range has no accepted day | "No credit figures yet." | n/a |

</intent-contract>

## Code Map

- `crates/arena-proto/src/rollup.rs` -- `RollupCredit/RollupCreditPlayer` already hold the list (read-only).
- `crates/arena-proto/src/owner_read.rs` -- add `OwnerRangeCredit` (`as_of`, `outstanding`, `aged`, `players`) with `from_rollups`; `OwnerRange.credit`.
- `crates/arena-proto/src/owner_export.rs` -- `ExportReport::Credit`, `credit()` CSV.
- `crates/arena-proto/src/owner_commands.rs`, `commands.rs`, `lib.rs` -- `credit.remind` kind (16 kinds), `CreditRemind` payload, validation.
- `crates/arena-proto/src/summary.rs`, `crates/arena-core/src/summary.rs`, `crates/arena-store/src/notify.rs` -- `BranchSummary.reminded_today`, the store read.
- `crates/arena-core/src/commands.rs`, `rbac.rs` -- `HANDLED_COMMAND_KINDS` (18), `command_intent` builds `NotifyRequested`, owner may submit a `credit_reminder` request.
- `crates/arena-cloud/src/api/owner_read.rs` -- `range_of` fills `credit`; export accepts `report=credit`.
- `apps/owner/src/lib/CreditPanel.svelte` (new), `ReportsBoard.svelte`, `ExportsPanel.svelte`, `reports.ts`, `commands.svelte.ts` (`sendNow`, kind label) -- the tab, exports and one-tap send.
- `packages/ui/src/messages/en.json` -- strings. `apps/owner/tests/credit.spec.ts` (new), `reports-export.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Proto: `OwnerRangeCredit`, `OwnerRange.credit`, `ExportReport::Credit` + CSV, `credit.remind` kind/payload, `BranchSummary.reminded_today`; regenerate TS types.
- [x] Store/core: `reminded_today` read and summary build; `credit.remind` handled kind; rbac exception; `command_intent` mapping.
- [x] Cloud: `range_of` fills credit; export `report=credit`.
- [x] Owner app: `CreditPanel`, Credit tab, export entries, `sendNow`, strings.
- [x] Tests: proto (credit fold, CSV, kind validation), core (command → queued row, rejected cases, summary), cloud (range credit, export), Playwright (Credit tab, remind, repeat, no consent, export).

**Acceptance Criteria:**
- Given Reports → Credit, when it opens, then it shows each branch's outstanding aged 0–7/8–14/15+ from the latest rollup with its date and the player list (id, username, outstanding, oldest unpaid date) with no phone, name or email.
- Given a player with consent, when the owner taps "Send reminder", then a `credit.remind` command is sent without MFA and the row shows its `command-row` state.
- Given the Hub applies `credit.remind` for a consenting player who owes credit, then it writes a `credit_reminder` row and reports applied; otherwise it reports rejected ("Couldn't apply at branch").
- Given a player without consent, then the row says "No consent for messages" and offers no button.
- Given a reminder already sent today, when the owner taps again, then "Reminder already sent today" asks to confirm before sending.
- Given a range and branch, when the owner exports Credit as CSV or PDF, then it exports like the other reports and Credit is listed in Exports.

## Spec Change Log

## Review Triage Log

## Design Notes

- The credit list comes from the newest accepted rollup of the last 7 days (as stock levels do), so "Today" still shows the latest figures with their date.
- `credit.remind` carries a placeholder `amount_minor` ("0") only to satisfy the request's parameter check; `notify_request::mutate` overwrites it with what the player owes.
- Owner actors may submit only a `credit_reminder` `notify.requested` (the rest stay system-only).

## Verification

**Commands:**
- `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-tasks -p arena-hub -p arena-api --all-targets -- -D warnings` -- expected: clean
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-cloud` -- expected: pass
- `pnpm run gen:types && pnpm run check:protocol && pnpm run check:strings` -- expected: pass
- `pnpm --filter @arena/owner exec playwright test credit reports-export` -- expected: pass
