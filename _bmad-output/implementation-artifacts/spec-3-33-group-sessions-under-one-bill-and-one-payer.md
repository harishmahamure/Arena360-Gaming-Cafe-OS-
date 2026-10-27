---
title: 'Story 3.33: Group Sessions Under One Bill and One Payer'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: '08d6ad037ff6887f240469e294326f129ae086d6'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-32-transfer-a-session-to-another-device.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Friends on several PCs get several bills, so staff juggle separate settles and one person cannot pay for all.

**Approach:** A `group_bills` row (payer, state `open`/`closed`) links running postpaid sessions through `sessions.group_bill_id`. Members end and free their PCs individually; when the last running member ends, one order is created with every member's segments as lines, and it settles once like any bill (so Undo of settle returns the whole group bill to unpaid). Two new staff intents, `session.group` and `group.payer`, plus an optional `group_with` on `session.start` ("Start in group").

## Boundaries & Constraints

**Always:**
- `session.group {session_id, member_ids}` (permission `session.start`, `base_rev` = the host session's rev) joins the host and each member to the host's group, creating one paid by the host's player (or its guest number). Host and members must be `postpaid` and `active`/`paused`; otherwise `VALIDATION_FAILED` with "Only postpaid sessions can share a bill" (mode) or a running-state message. A member in another group is refused; a missing session is `NOT_FOUND`; 1 to 32 distinct members.
- `session.start.group_with` joins the new session (postpaid only) to the group of a running postpaid session, creating it if needed; a prepaid mode or a non-joinable host is refused on field `group_with`.
- `group.payer {group_bill_id, player_id | guest_no}` (permission `pos.sell`, `base_rev` = the group rev) names a profile (a member's or a looked-up player) or a guest member by guest number. Once the bill order exists it is re-pointed to the new payer (so wallet and credit follow); after it settles the change is `STALE`.
- Ending a member that is not the last running one closes its segments, ends it and frees its PC without an order. Ending the last creates the single order (`session_id` = the last session, `group_bill_id` set, player/guest = the group payer), with lines for all members in start order, each member billed with its own rounding and minimum, and sets the group `closed`.
- Undo of the last member's end deletes the order and its lines and reopens the group; Undo of an earlier member's end is `UNDO_NOT_ALLOWED`.
- The snapshot carries `sessions[].group_bill_id`, `bills[].group_bill_id` and `group_bills[]` (payer, running member ids, and the ended members' lines).
- No code comments beyond doc comments. Every new string is in `packages/ui/src/messages/en.json`.

**Never:**
- Prepaid, console or forced-end changes (3.34, 3.35), a new permission, or kiosk-side changes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| Add to group | two running postpaid sessions | one group, both linked, payer = host | none |
| Prepaid member | pack/wallet/pass/block session | refused, nothing written | "Only postpaid sessions can share a bill" |
| Member ends early | others still running | PC frees, no order, group open | none |
| Last member ends | all others ended | one order, one line per segment, group closed | none |
| Payer change | member, guest member or looked-up player | payer saved; open order re-pointed | `STALE` once settled; `NOT_FOUND` unknown player |
| Settle then Undo | group order settled | whole bill back to unpaid | `UNDO_NOT_ALLOWED` outside the window |

</intent-contract>

## Code Map

- `crates/arena-store/migrations/0040_group_bills.sql`, `src/group_bills.rs`, `branch.rs` (`Table::GroupBills`/`RowKey::GroupBills`), `sessions.rs` (`group_bill_id` on `LiveSession`, `OrderRow`, `OpenOrder`).
- `crates/arena-core/src/apply/group_bill.rs` -- `session.group` and `group.payer` load/base_rev/validate/mutate, the end-time `GroupEnd`; `session.rs` -- `mutate_end` builds the group order, `mutate_end_reverted` reopens the group, `session.start` `group_with`.
- `crates/arena-core/src/snapshot.rs` -- group bills and ended-member lines.
- `crates/arena-proto` -- `SessionGroup`, `GroupPayer`, `SessionStart.group_with`, `SnapshotGroupBill`, snapshot fields; `pnpm run gen:types`.
- `apps/hub/src/lib/GroupPanel.svelte`, `TileSheet.svelte`, `tile-actions.svelte.ts`, `routes/floor/+page.svelte`; `packages/ui` `BillBreakdown` (subtotal per device, group total), `FloorTile` (group target outline).

## Tasks & Acceptance

**Execution:**
- [x] `arena-store` -- `group_bills` table, order `group_bill_id`, change-log registration, queries
- [x] `arena-proto` -- intents, `group_with`, snapshot types, regenerated TS types
- [x] `arena-core` -- join, payer, group order at the last end, Undo of the last end, `group_with` on start, snapshot, proptest op, replay seed
- [x] `arena-core` tests -- `apply/tests/group_bill.rs` (join, guest payer, prepaid refusal, other group, stale/base_rev, early end, last end bill and snapshot, payer change and wallet settle, payer after bill, Undo settle, Undo last end, start in group)
- [x] Hub UI -- Add to group mode (add running postpaid, start in group on a free PC), group marker naming the payer, group breakdown with a subtotal per device and one total, payer panel with member select and player lookup, no guest convert on a group bill
- [x] Playwright `group-bill.spec.ts`

**Acceptance Criteria:**
- Given a postpaid session, when staff add others or start new ones in the group, then a `group_bills` row links them and each tile names the payer.
- Given a member's sheet, then the breakdown shows each device's lines, a subtotal per device and one total.
- Given the payer changes, then the settle options (wallet, credit) follow the new payer.
- Given members end at different times, then each device frees on its own and the one bill appears with the last end.
- Given a prepaid session, then adding it is refused with "Only postpaid sessions can share a bill".
- Given Undo of a group settle in the window, then the whole group bill is unpaid again.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test group-bill transfer billing session-start session-end pause convert extend prepaid-start floor tile-sheet` in `apps/hub`.

## Auto Run Result

Status: review
Verified: fmt, clippy; arena-core (656 lib tests incl. 14 `group_bill`, proptest with a `Group` op), arena-store, arena-proto, arena-api and arena-replay tests; hub and kiosk check; `check:strings`, `check:tokens-used`, `check:contrast`, `check:hex`, `check:protocol`, `check:layers`, `format:check`; Playwright (244 across group-bill, transfer, billing, session-start/end, pause, convert, extend, prepaid-start, floor, tile-sheet). `check:generated` reports the new generated files until they are committed. Known unrelated failures as before: arena-cloud (2) and arena-tasks (2). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: an earlier member's end is not undoable (only the last, which owns the bill); "Start in group" is a guest session (`group_with`), filed under the host sheet's status line; after the group bill exists the payer panel offers player lookup only; guest conversion is hidden on a group bill.

## Code Review Result

Status: done. Four parallel reviewers (blind, edge case, verification gap, acceptance). Fixed: `session.assign_player` is refused on a grouped session (it would desync the group payer); per-device subtotals only show on a group bill (they leaked onto transfer bills); "Add to group" no longer highlights sessions already in another group; the group marker is hidden when the group is not in the snapshot; undo of an earlier member's end gives a specific message; tests added for convert-in-group and the group staying in the snapshot until its bill settles.
Deferred: the group order takes its business day/fiscal year from the last member to end; a payer pick of an already-ended member (lookup only); no leave-group intent; "Start in group" guest-only; permission gating in the UI buttons.
Verified: fmt, clippy, workspace tests (only the known arena-cloud and arena-tasks failures), hub check, format:check, check:strings, Playwright (146).
