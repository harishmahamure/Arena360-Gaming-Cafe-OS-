---
title: 'Story 5.25: Cash deposits and expenses'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '7112bf2cd988fc6f06f75584e3191ad0660b055d'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-24-refund-a-settled-order-with-a-credit-note.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Cash that leaves the drawer for the safe, or pays a small bill, has no record, so the expected drawer cash at close (Story 5.26) cannot be right.

**Approach:** `cash.deposit {amount_minor, reference}` and `expense.create {category, amount_minor, paid_from, receipt_photo?, note?}` (permission `cash.manage`; Counter elevates with the inline manager PIN) each write one `cash_events` row against the open shift. `amount_minor` on the row is the drawer's effect: negative for a deposit and for a drawer expense, 0 for an expense the owner paid (`spent_minor` keeps what it cost). The expected cash of a shift is the sum of its `cash_events` (`arena_store::expected_cash`), so reversed sale pairs net out and refunds, deposits and drawer expenses reduce it. The Shift screen gets a Deposit / Expense panel.

## Boundaries & Constraints

**Always:**

- Both intents take no `base_rev`, need an open shift (`SHIFT_NOT_OPEN`) and an amount of 1 to 100,000,000 minor units (`VALIDATION_FAILED`, field `amount_minor`).
- A deposit needs a slip reference of 1 to 40 plain characters (field `reference`). An expense category is one of supplies, utilities, repairs, food_stock, other; the note is optional plain text up to 140 characters.
- A deposit or a drawer expense above the shift's expected cash is `DRAWER_SHORT` (new, HTTP 409); its message never says how much the drawer holds. An owner-paid expense never touches the drawer, so it is never short.
- The receipt photo is a JPEG data URI of at most 200 KB; the Hub resizes a picked picture (longest side and quality step down) before it is sent, and `apply()` refuses anything bigger or not a JPEG (field `receipt_photo`).

**Never:**

- Showing the expected cash (the count stays blind until Story 5.26), or editing/removing a cash event (the table stays append-only; migration 0058 extends the trigger to the new columns).

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0058_cash_moves.sql` -- `cash_events` gains `reference`, `category`, `paid_from`, `spent_minor`, `note`, `receipt_photo`; `src/shifts.rs` -- `NewCashMove`, `insert_cash_move`, `expected_cash`.
- `crates/arena-proto/src/shift.rs` -- `CashDeposit`, `ExpenseCreate`, `ExpenseCategory`, `ExpensePaidFrom`, `receipt_photo_problem`, `CASH_DEPOSIT`, `CASH_EXPENSE`; `intent.rs` -- kinds `cash.deposit`, `expense.create` (`Permission::CashManage`, no reason); `error.rs` -- `DRAWER_SHORT`.
- `crates/arena-core/src/apply/shift.rs` -- `validate_deposit/expense`, `mutate_deposit/expense`; `ShiftTarget` carries the open shift's expected cash.
- Hub: `lib/CashMovePanel.svelte` (on `routes/shift/+page.svelte`), `lib/shift.ts` (`parseAmount`, `resizeReceipt`, categories), `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated TS.
- Tests: `apply/tests/cash_moves.rs` (6), store migration test, proptest `Op::CashMove`, replay seed (deposit + expense), Playwright `cash-moves.spec.ts`.

## Tasks & Acceptance

**Execution:**

- [x] Store: migration 0058, `insert_cash_move`, `expected_cash`
- [x] Protocol: `cash.deposit`, `expense.create`, `DRAWER_SHORT`, generated TS
- [x] Core: validate and mutate both intents; expected cash on the shift target
- [x] Hub UI: Deposit / Expense panel with inline manager PIN and photo resize
- [x] Tests: Rust matrix, migration, proptest op, replay, Playwright
- [x] Verification gate

**Acceptance Criteria:**

- Given an open shift, a deposit with amount and slip reference writes a `deposit` cash event; Counter elevates with the inline PIN.
- Given an expense with category, amount, paid-from and a photo, it writes an `expense` cash event; only Drawer expenses reduce expected cash; the photo is at most 200 KB.
- Given a deposit or drawer expense above the expected cash, it is rejected `DRAWER_SHORT` without the amount.
- Given no open shift, both are rejected `SHIFT_NOT_OPEN`.
- Given a shift, expected cash = float + cash sales - cash refunds - deposits - drawer expenses from `cash_events` only, reversed pairs netted.

## Spec Change Log

## Review Triage Log

Code review (2026-10-28): no blocking findings; fmt, clippy and Rust tests re-run green.

Verification (2026-10-28): fmt, clippy -D warnings, cargo test (proto, store, core, api, replay, kiosk-core), `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`, Hub svelte-check, Playwright `cash-moves`, `shift`, `pos`, `audit` pass. `check:generated` lists the new generated files until they are committed.

## Design Notes

Details live on `cash_events` as nullable columns rather than a second table so the append-only ledger, the change log and the replay need no new table, and the expected-cash sum stays one query. The expected cash is computed on the Hub in the same transaction (`ShiftTarget.expected_minor`), so two concurrent outflows cannot both pass the check.

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked -D warnings --all-targets` and `cargo test` for `arena-proto`, `arena-store`, `arena-core`, `arena-api`, `arena-replay`, `arena-kiosk-core`; `pnpm gen:types`, `check:strings`, `check:protocol`, `check:layers`, `check:hex`, `format:check`; Hub `pnpm check`; Playwright `cash-moves.spec.ts shift.spec.ts pos.spec.ts audit.spec.ts` in `apps/hub`.
