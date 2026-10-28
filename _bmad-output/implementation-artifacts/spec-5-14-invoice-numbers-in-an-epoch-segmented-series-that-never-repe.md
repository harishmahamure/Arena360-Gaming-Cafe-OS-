---
title: 'Story 5.14: Invoice numbers in an epoch-segmented series that never repeat'
type: 'feature'
created: '2026-10-28'
status: 'review'
baseline_commit: 'a595de26c0e8226d12c27bd252ce3bd158ac6c0b'
baseline_revision: 'a595de26c0e8226d12c27bd252ce3bd158ac6c0b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-13-take-stock-at-served-mark-items-served-and-cancel-unserved-l.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A settled sale has no legal invoice number, so a café cannot hand out a valid tax invoice and a Hub change or restore could repeat a number.

**Approach:**
- `orders.invoice_no` and a change-log table `invoice_sequences (fy, segment, next)` (migration 0050). A number is `<branch code>/<FY>/E<epoch counter><instance tag>/<seq>` with a 6-digit zero-padded seq, e.g. `DEV01/26-27/E1k/000142`. The FY is the order's stamped `fy` (`FY2026-27` prints `26-27`); the tag is one letter drawn from the writer's Hub instance id, so two Hubs that minted the same provisional counter still differ.
- System intent `invoice.issue {order_id}` takes the next number of the `(fy, segment)` of the writer's epoch inside `apply()`: the sequence advances by exactly one, `orders.invoice_no` is written, the branch code is locked at the first invoice (`lock_branch_code`) and the irreversible effect `invoice_number_assigned` is recorded (Story 3.14 guard), so the sale can never be undone.
- A Hub task (`InvoiceTask`) scans settled sales without a number, oldest settle first, and applies `invoice.issue` once the 8 s Undo window has closed. It starts with a scan, so a restart numbers the backlog in `settled_at` order first.
- Recent sales on the POS screen shows the number once issued ("Pending" before).

## Boundaries & Constraints

**Always:**
- Only a settled order of kind `session`, `plan_sale` or `pos_sale` with a non-zero total is numbered. A wallet top-up, a credit collection, a zero total (a pack session) and an order undone inside the window never get a number, so the series has no gaps.
- `invoice.issue` is system-only, takes no `base_rev`, and refuses (`STALE`) an order that already has a number, is not settled, or is still inside its Undo window (`now - settled_at <= UNDO_WINDOW_MS`).
- A new epoch or a new financial year opens a new `(fy, segment)` that starts at 000001; an old segment is never written again. `orders.invoice_no` is unique.
- `issue_number` (in `apply/invoice_issue.rs`) is the one place a number is minted; Story 5.17 calls it inside the settle transaction for a printed bill.

**Never:** Print or receipt layouts (5.16/5.17), credit notes (5.24), B2B GSTIN invoices (5.20), orphaned old-epoch reconciliation (Epics 8-9).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|----------|--------------|----------|-------|
| Issue after window | settled `pos_sale`, window closed | `DEV01/26-27/E1x/000001`, sequence next = 2, branch code locked, effect recorded | none |
| Second and third sale | issued in turn | 000002, 000003 | none |
| Inside the window | `now - settled_at <= 8 s` | nothing written | `STALE order_id` |
| Already numbered | order has `invoice_no` | nothing written | `STALE order_id` |
| Not a sale | top-up / collection order, zero total | nothing written | `VALIDATION_FAILED order_id` |
| Unsettled or missing | open order / unknown id | nothing written | `STALE` / `NOT_FOUND` |
| Undone sale | reverted inside the window | never numbered, no gap | `NOT_FOUND` |
| New epoch | lease counter 2 | `…/E2x/000001`, old segment untouched | none |
| New FY | order `fy = FY2027-28` | `…/27-28/E2x/000001` | none |
| Restart | backlog of unnumbered sales | numbered in `settled_at` order, none twice | none |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0050_invoices.sql` -- NEW. `src/invoices.rs` -- NEW: `take_sequence`, `find_invoice_order`, `set_invoice_no`, `list_unnumbered`. `src/branch.rs` -- `Table::InvoiceSequences` (key `fy, segment`), `RowKey::InvoiceSequences`. `src/sessions.rs` -- `list_pos_sales` reads `invoice_no`.
- `crates/arena-proto/src/{intent.rs,pos.rs,lib.rs}` -- `IntentKind::InvoiceIssue`, payload `InvoiceIssue`, `Requirement::System`.
- `crates/arena-core/src/invoice/number.rs` -- NEW: `fy_label`, `segment`, `format`. `src/apply/invoice_issue.rs` -- NEW: load, validate, mutate, `issue_number`. `apply/{mod,guards,validate,mutate}.rs` -- wiring. `snapshot.rs` -- `unnumbered_orders`.
- `crates/arena-tasks/src/invoice.rs` -- NEW `InvoiceTask`; `crates/arena-hub/src/lib.rs` -- spawns it.
- `apps/hub/src/lib/audit/labels.ts`, `packages/ui/src/messages/en.json`; TS regenerated in `packages/ui/src/generated`.
- Tests: `crates/arena-core/src/apply/tests/invoice.rs` (NEW), `invoice/number.rs`, `arena-store` migration and sequence tests, `arena-tasks` `invoice.rs` tests, `proptest_apply.rs` (`Op::InvoiceIssue`, `assert_invoices_hold`), `arena-replay` seed.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store` -- migration 0050, `invoice_sequences` change-log table, readers and writers -- schema
- [x] `crates/arena-proto` -- `invoice.issue` kind and payload; regenerate TS -- wire contract
- [x] `crates/arena-core` -- number format, `invoice.issue` pipeline, `issue_number`, branch-code lock, irreversible effect -- domain
- [x] `crates/arena-tasks` + `arena-hub` -- invoice task: backlog scan at start, oldest settle first, after the Undo window -- Hub task
- [x] Tests -- matrix rows, proptest op and invariants, replay seed, task tests -- proof
- [x] Verification gate

**Acceptance Criteria:**
- Given a settled sale whose `undo_until` has passed, then `invoice.issue` writes `<branch code>/<FY>/E<counter><tag>/<seq>` to `orders.invoice_no` and advances the sequence by one in the same transaction.
- Given a sale reverted inside the window, then it never receives a number and the series has no gap.
- Given a restart with settled sales past `undo_until` and no number, then they are numbered in `settled_at` order and none twice.
- Given a new epoch or a new financial year, then the next number starts at 000001 in the new `(fy, segment)`.
- Given the first invoice at the branch, then `lock_branch_code()` sets `code_locked_at` in the same transaction.
- Given a zero total, a top-up or a collection, then no number is issued.
- Given concurrent issues, then numbers are consecutive and unique (the writer actor serialises them).

## Design Notes

- The tag is `a + (last byte of the instance id) % 26`. A ULID's last character is random, so two Hubs collide on a segment only one time in 26 *and* only if they also minted the same counter; the sequence per `(fy, segment)` and the unique `invoice_no` index still prevent a repeat inside one database.
- `invoice_sequences` stores the stamped FY (`FY2026-27`); only the printed number shortens it.
- Numbering a sale is a write to its `orders` row, so a hub client may see the row's `rev` move after the window; nothing depends on that rev afterwards because Undo is closed.

## Verification

**Commands:**
- `cargo test` on `arena-proto`, `arena-store`, `arena-core`, `arena-tasks`, `arena-api`, `arena-hub`, `arena-replay`; `cargo clippy -D warnings`; `cargo fmt --check`
- `pnpm --filter hub check`; Playwright `audit.spec.ts` and `pos.spec.ts`; `check:strings`, `check:layers`, `check:protocol`, `check:hex`, `check:generated`
- Pre-existing Windows-only failures, unrelated: `arena-tasks` `heartbeat::trusted_time_*` and `printer::lan::*stops_reading*`; `arena-e2e` and `arena-kiosk-svc` tests do not build on Windows.
