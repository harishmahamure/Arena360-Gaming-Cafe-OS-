---
title: 'Story 5.10: Open a shift with a confirmed float, and cash needs an open shift'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: 'c72b44e'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
---

<intent-contract>

## Intent

**Problem:** Cash has no owner. Any staff member can take cash with nothing recording whose drawer it went into, so no shift can later be counted against what it took.

**Approach:**
- New table `shifts` and append-only `cash_events` (migration `0046_shifts.sql`), both change-log tables.
- New intent `shift.open {float_minor}` (permission `shift.open`: Owner, Manager and Counter hold it). It writes a `shifts` row owned by the signed-in staff and a `cash_events` row of kind `float_in`. The shift row keeps `carried_minor` (the last shift's opening float, 0 for the first) beside `float_minor` (what staff confirmed), so the audit records both.
- One open shift per branch (a partial unique index and a `SHIFT_ALREADY_OPEN` refusal naming whose it is).
- Cash rule: any intent with a cash payment row (`session.settle`, `wallet.topup`, `plan.sell`, `credit.collect`) is refused `SHIFT_NOT_OPEN` without an open shift. UPI, card, wallet and credit never need one. Each cash row commits a `sale` cash event against the open shift with the settling staff member's id; the Undo of each of those intents writes a compensating `sale_reverted` row (negative, `reverses` the sale, same shift).
- The snapshot carries `shift: {open, carried_minor}` (opener, time, float, event count; never an expected drawer amount).
- Hub: the Shift screen (Open shift → float prefilled → confirm; open view with who, when, float, running event count and "blind until close"); the settle bar drops Cash and shows "Open a shift to take cash." with a link to Shift while no shift is open.

## Boundaries & Constraints

**Always:**
- `float_minor` is 0 to `MAX_FLOAT_MINOR` (`ValidationFailed float_minor`), the branch must exist (`ValidationFailed branch`), and `shift.open` takes no `base_rev`.
- A cash event is `float_in` (shift open), `sale` (cash kept = the cash payment row) or `sale_reverted` (the exact negative of the sale it cancels). Rows are never edited or deleted (database triggers).
- A refused cash intent writes nothing. The cash check precedes the payment-row and total checks.
- The settle bar treats a snapshot with no `shift` block (an older Hub) as "open": the Hub itself still refuses with `SHIFT_NOT_OPEN`.

**Never:**
- Closing a shift, the expected drawer amount, variance, deposits and expenses (5.25, 5.26), refunds as cash (5.24), the POS sale and its cash (5.11), drawer kicks (5.18).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected |
|----------|--------------|----------|
| First shift | no shift, float 0 | shift + `float_in`, carried 0 |
| Different float | carried 5000, entered 7500 | float 7500, carried 5000, both in the change row |
| Second open | a shift is open | `SHIFT_ALREADY_OPEN` "asha has a shift open." |
| Cash, no shift | settle, top-up, plan sale, collection | `SHIFT_NOT_OPEN`, nothing written |
| UPI/card/wallet/credit, no shift | any | settles as before |
| Cash with shift | settle 5000 cash + 2500 UPI | one `sale` of 5000 against the open shift, staff = settler |
| Undo | within 8 s | `sale_reverted` −5000, `reverses` the sale |
| Kitchen staff | `shift.open` | `PERMISSION_DENIED` |

</intent-contract>

## Code Map

- `crates/arena-store`: `migrations/0046_shifts.sql`, `shifts.rs`, `branch.rs` (`Table::Shifts`, `Table::CashEvents`), `lib.rs` (`shift_state`).
- `crates/arena-proto`: `shift.rs` (`ShiftOpen`, `ShiftState`, `SnapshotShift`, cash kinds), `intent.rs`, `error.rs` (`SHIFT_ALREADY_OPEN`, `SHIFT_NOT_OPEN`), `snapshot.rs` (`HubSnapshot.shift`).
- `crates/arena-core`: `apply/shift.rs` (open, gate, `record_sales`, `reverse_sales`), `apply/{session,wallet,plan_sales,credit_collect}.rs` (gate, sale and compensating events), `snapshot.rs`.
- `crates/arena-api`, `arena-cloud`, `arena-kiosk-core`: status and retry maps for the two codes.
- `apps/hub`: `src/routes/shift/+page.svelte`, `src/lib/shift.ts`, `src/lib/shift.svelte.ts`, `SettleBar.svelte`, `routes/+layout.svelte`, `tests/shift.spec.ts`, `settle.spec.ts`.

## Tasks & Acceptance

- [x] Store migration, tables and helpers.
- [x] Proto: payload, intent kind, error codes, snapshot block; regenerated `packages/ui/src/generated`.
- [x] Apply: `shift.open`; cash gate; `sale` events; compensating `sale_reverted` on every Undo.
- [x] Apply tests (`apply/tests/shift.rs`), proptest op and cash-ledger invariant, replay fixture, existing fixtures open a shift.
- [x] Hub Shift screen, settle-bar gate, messages, Playwright.
- [x] Verification gate.

**Acceptance Criteria:** the Given/When/Then of Story 5.10 in `epics.md`.

## Design Notes

- The "cash tendered minus change" of the AC is the cash payment row itself: the settle bar already takes change off the cash row before it sends (Story 3.13), so the Hub stores only the cash kept.
- The compensating row is written against the shift the sale went to, so an Undo after a shift change still cancels the right shift's cash. Closing a shift (5.26) must keep this in mind.
- The Undo's compensating row is a new insert, so the audit's `reverts` link (same-row, latest forward change) is empty on it; the payments and order rows of the same intent still link.
- Test fixtures: `primary()` opens a shift straight in the database (`DEFAULT_SHIFT`); `primary_closed()` and `bootstrapped_closed()` do not.

## Verification

**Commands:**
- `cargo test` on `arena-proto`, `arena-store`, `arena-core` (lib 728, proptest at 400 cases, floor_load, offline_flows), `arena-api`, `arena-replay` -- pass
- `cargo clippy` on the touched crates with `-D warnings`, `cargo fmt --check` -- clean
- `pnpm --filter hub check` -- 0 errors, 0 warnings
- `pnpm exec playwright test` in `apps/hub` -- 1358 passed, 61 skipped, 1 failed (`players.spec.ts:462` phone, Ctrl+K from another screen) that passes alone (a load flake)
- `check:strings`, `check:layers`, `check:protocol`, `check:hex` -- pass; `check:generated` passes once the regenerated types are committed

## Spec Change Log

## Review Triage Log

## Auto Run Result

Status: review

- **Pre-existing failures on this Windows host** (unrelated to shifts): `arena-tasks` `trusted_time_is_cloud_time_plus_monotonic_elapsed` (needs 8 days of uptime for `Instant::checked_sub`) and `a_printer_that_stops_reading_times_out`; `arena-kiosk-svc` privileged-registry and pipe tests; `arena-cloud` `backup` hung-run and `binary` unreachable-at-start tests; `arena-e2e` is unix-only.
- **Residual risks:** shift close and the expected-cash maths are 5.26; a `sale_reverted` row has no audit `reverts` link; the Shift screen refreshes its event count every 5 s through the snapshot.
