---
title: 'Story 3.28: Extend a Session from the Hub, Charged to Wallet, Pack or Credit'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'd14754a'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-24-start-a-prepaid-session-on-a-pack-or-the-wallet.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-26-time-up-on-pcs-grace-then-clean-up-or-staff-handles.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-27-pass-and-block-sessions-with-fixed-windows.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A pack or wallet session ends at the `ends_at` it was started with. A player who wants to keep playing has to lose the game, or staff have to end the session and start another.

**Approach:** `session.extend{session_id, minutes, source, pack_id?, credit_override?}` adds 30, 60 or 120 minutes to a running pack or wallet session of a player, paid from the wallet, from another owned pack, or on credit. The extension is charged as its own settled order (tied to the session by `orders.extends_session_id`, `session_id` null, one line from the old to the new end at the group's live rate) so the session's own close-out bill is untouched; a wallet session is billed only up to its original end. `ends_at` moves, `sessions.extended_min` grows, and a session in its grace period returns to `active`. The Hub tile sheet offers Extend on those sessions with live prices and the available sources.

## Boundaries & Constraints

**Always:**
- `minutes` is one of `EXTEND_OPTIONS_MIN` (30, 60, 120), else `VALIDATION_FAILED` on `minutes`. `session.extend` takes no `base_rev`.
- The session must be `active`, `paused` or `grace` (`STALE` otherwise, a held `expired` session included), of mode `pack` or `wallet` with a player and an end time. A postpaid session is open and a pass or block plays a fixed window: both are `VALIDATION_FAILED` on `session_id`.
- Price = half-up `rate * minutes / 60` at the group's active rate (`NO_RATE` for wallet and credit when the group has none). The order line runs from the old `ends_at` to the new one.
- Wallet source: the price must fit the balance less the player's running reservations (`INSUFFICIENT_BALANCE` on `source`, "Balance ₹x. Top up at counter."). Credit source: the Story 3.18 rules through `validate_payments` (`CREDIT_LIMIT`, `CREDIT_PAUSED`, `OVERRIDE_USED_TODAY`, override needs `credit.manage` or the manager's PIN), errors on `source`. Both write the payment, the `bill` or `credit` wallet transaction and the wallet or credit row.
- Pack source: an owned `pack` (not a pass) of the player, allowed for the session's group, unexpired, with at least `minutes` left, not on any live session (so never the pack the session runs on). Its minutes are taken at once; the order is a settled zero bill. Refusals: `NOT_FOUND`, `PLAN_NOT_FOR_GROUP`, `PACK_EXPIRED`, `INSUFFICIENT_BALANCE` (fewer minutes left), `VALIDATION_FAILED`, all on `pack_id`.
- A new end past the next branch closing time (opening hours, Story 2.26; none kept means no limit) is `PAST_CLOSING` on `minutes`.
- A wallet session's close-out bills only up to `ends_at - extended_min`; a pack session's close-out is capped at its own pack's minutes left, so neither pays for extended time twice. Ending early refunds nothing, like a block.
- An extension order cannot be Undone through `session.settle.reverted` (it has no session). New code `PAST_CLOSING` is registered wherever `PASS_OUTSIDE_WINDOW` is. Migration `0038_session_extend.sql` adds `sessions.extended_min` and `orders.extends_session_id`. `SnapshotSession` gains `mode` and `pack_id`. No code comments beyond doc comments in the existing style (user rule).

**Never:**
- Extending a postpaid, pass or block session, pausing a prepaid session, a rate resolver (3.29–3.31), kiosk-originated extend (Epic 6), an Undo of an extend, receipts or invoice numbers (Epic 5), changing the timer engine.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Wallet extend | pack session, balance 10000, rate 6000/h, +60 | `ends_at` +60 min, settled order of 6000, wallet 4000 | No error |
| Grace extend | session in `grace`, +30 | state `active`, `ends_at` old end + 30 min | No error |
| Pack extend | other pack with 100 min, +60 | that pack 40 min, zero-bill order, nothing charged | No error |
| Credit extend | limit 10000, +60 at 6000/h | outstanding 6000 | `CREDIT_LIMIT` over the limit; override once a day |
| Past closing | close 23:00, end 22:30, +120 | nothing written | `PAST_CLOSING` |
| Wallet short / pack short | balance 2900 / pack 20 min | nothing written | `INSUFFICIENT_BALANCE` |
| Pack unusable | other group / expired / empty / foreign / running / none | nothing written | `PLAN_NOT_FOR_GROUP` / `PACK_EXPIRED` / `NOT_FOUND` / `VALIDATION_FAILED` |
| Postpaid, pass, block, ended, held | | nothing written | `VALIDATION_FAILED` / `STALE` |
| End a wallet session past its original end | extended 30, ended at 110 min of 90 | bill for 90 min only | No error |

</intent-contract>

## Code Map

- `crates/arena-proto`: `session.rs` (`SessionExtend`, `ExtendSource`, `EXTEND_OPTIONS_MIN`, `SnapshotSession.mode`/`pack_id`), `intent.rs` (`session.extend`, permission `session.extend`), `error.rs` (`PastClosing`), registered in `arena-api/src/ui_auth.rs`, `arena-cloud/src/api/error.rs`, `arena-kiosk-core/src/queue.rs`.
- `crates/arena-store`: `migrations/0038_session_extend.sql`, `sessions.rs` (`extend_session`, `insert_extension_order`, `LiveSession.extended_min`).
- `crates/arena-core`: new `apply/extend.rs` (load, validate, mutate), `apply/prepaid.rs` (`base_end` cap in the wallet close-out), `apply/session.rs` (`validate_settle_reverted`), `apply/guards.rs` (credit override), `session_screen.rs` (`closes_in`), `snapshot.rs`.
- `crates/arena-api/src/ui_auth.rs`: the kiosk is pushed the fresh session screen after an extend, like a resume.
- Tests: `apply/tests/extend.rs`, proptest `Op::SessionExtend`.
- Hub: `lib/players/ExtendOptions.svelte`, `lib/TileSheet.svelte`, `lib/tile-actions.svelte.ts`, `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated types, `apps/hub/tests/extend.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol intent, payload, source enum, error code and snapshot fields, registered everywhere, generated types
- [x] Migration `0038`, store `extend_session` and `insert_extension_order`, `LiveSession.extended_min`
- [x] `apply/extend.rs`: validation of state, mode, minutes, sources, closing; the extension order, ledger rows and pack minutes; grace returns to active
- [x] Close-out caps: wallet bill stops at the original end; the extension order is not a session bill for Undo
- [x] Apply tests for every matrix row; proptest `SessionExtend`
- [x] Kiosk push after an extend; Hub Extend options (times with live prices, wallet, packs, credit), refusals inline, Playwright specs on desktop and phone

**Acceptance Criteria:**
- Given an active, paused or grace pack or wallet session, when staff press Extend, then +30 min, +1 h and +2 h are listed with live prices at the group's rate, and the wallet (balance), each owned pack allowed for the group (time left) and credit (headroom) are offered.
- Given an option and source, when `session.extend` applies, then `ends_at` moves, the ledger is written (wallet debit, pack minutes taken, or credit spend with the 3.18 limit check and override), a grace session returns to `active`, and the kiosk is sent the new time at once.
- Given the new end would pass closing time, then it is refused `PAST_CLOSING` and the options stay open with the reason.
- Given insufficient wallet, pack minutes or credit headroom (or paused credit), then it is refused with the typed reason and the other sources stay available.
- Given a postpaid session, then Extend is not offered.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test extend prepaid-start session-start session-end session-timer pause tile-sheet` in `apps/hub`.

## Auto Run Result

Status: review
Verified: fmt, clippy, hub and kiosk check, string/token/contrast/protocol/layer/hex/secrets checks and prettier pass; arena-core (19 new `extend` tests, proptest with `SessionExtend`) and the other workspace tests pass; `extend`, `prepaid-start`, `session-start`, `session-end`, `session-timer`, `pause`, `tile-sheet`, `convert`, `settle` and `audit` Playwright specs pass on desktop and phone (194). Known unrelated failures as before: arena-cloud (`a_hung_run_times_out_with_its_own_code`, `unreachable_at_start_retries_then_exits_1_without_logging_the_token`). `owner_staff_and_device_edits_win_over_a_stale_base_rev` failed once under full-suite load and passes alone. `check:generated` reports the generated files as stale only until they are committed. `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: the extension is its own settled order (kind `session`, `session_id` null, `extends_session_id` set) because an order per session is unique and the order kind CHECK cannot change without rebuilding `orders`; pass and block sessions are not extendable (fixed windows) and prepaid sessions cannot be paused so a paused extend is only defensive; the pack minutes are taken when the extend applies and an early End refunds nothing; the pack a session runs on is not a source; the closing check uses the next closing after now; `session.extend` has no Undo; the credit override reuses the settle rule through `guards::overriding_credit`.
Left for later: kiosk-originated extend (6.21) and the rate resolver (3.30 switches the live price); extending pass and block sessions if the product wants it; replay coverage of extends; the LLD `session.extended` event; an owned-pack ledger row instead of a direct minutes deduction.

## Review Triage Log (code review)

- Fixed: extending a paused session failed with INTERNAL (no open segment) and would have unlocked the kiosk; paused is now refused `VALIDATION_FAILED` on `session_id` ("resume first") and Extend is no longer offered on paused tiles. Test added.
- Fixed: `credit_override` on a wallet or pack extend no longer triggers the `credit.manage` guard.
- Deferred (non-blocking): picker greys-out of unusable sources, load-failure retry state, extra Playwright/override/snapshot/kiosk-push tests, migration index.
