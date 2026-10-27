---
title: 'Story 3.17: Convert a Guest to a Profile, Including at Settle, and List Sessions'
type: 'feature'
created: '2026-10-27'
status: 'review'
baseline_revision: '015786f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-16-wallet-top-up-and-paying-from-the-wallet.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-6-create-a-player-profile-with-protected-personal-data-and-con.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A walk-in who becomes a regular keeps no history: a guest session and its bill cannot be attached to a player, and there is no screen that lists the day's sessions.

**Approach:** Add `session.assign_player {session_id, player_id, create?}` (needs `base_rev` = the session rev). With `create` it also writes the Story 3.6 profile (player, consent, key, zero wallet) in the same transaction, which makes "Convert to profile" one intent; without it, it links an existing player. It sets `sessions.player_id` and, when the session has an open bill, `orders.player_id`. The guest number stays on the session row, so it is never reused and the change log keeps it. The Hub gets "Convert to profile" and "Link to existing player" in the tile sheet (running guest session) and inside the `settle-bar` (unpaid guest bill), and a Sessions screen listing today's sessions.

## Boundaries & Constraints

**Always:**
- `session.assign_player` needs `session.start`. When `create` is present it also needs `player.manage`, held by the actor or by the elevating manager (checked in the permission step). `create.player_id` must equal `player_id`; the profile goes through the Story 3.6 validation (`USERNAME_TAKEN`, `PHONE_TAKEN`, sealed fields).
- Only a guest session (`player_id` null) whose bill is not settled can be assigned; otherwise `VALIDATION_FAILED` on `session_id`. An unknown session is `NOT_FOUND`, a stale or missing `base_rev` is `STALE` / `VALIDATION_FAILED`, an unknown player to link is `NOT_FOUND`. A refusal writes nothing.
- `/ui/intent` refuses a payload with `create` (`UNSUPPORTED`); the profile is sealed server-side by `POST /ui/sessions/convert {session_id, base_rev, form, idempotency_key, elevation_ticket?}`, which needs `player.manage` or a manager's PIN like `/ui/players`.
- `SnapshotBill` carries `session_rev` and `guest` so an unpaid bill can be converted after the session left the live list. After assignment the bill's `wallet_minor` is present, so `payment-split` offers the wallet method.
- `GET /ui/sessions` returns today's business-day sessions (device, player or guest number, mode, start, end, bill, state `running` / `unpaid` / `settled`, `order_id` while unpaid) for any signed-in staff.
- The new intent goes into the Story 1.8 proptest generator and `arena-replay`. Money is integer minor units. No code comments (user rule).

**Never:**
- Undo of a conversion, credit options on the converted bill (Story 3.18), group bills, reports (Story 5.30), a push event for other open windows (they refresh with the next snapshot).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Convert running guest | session rev 3, valid form | player + consent + wallet created, session carries the player, guest number kept | No error |
| Convert unpaid guest | ended session, open bill | session and bill both carry the player; wallet method offered | No error |
| Link existing | existing player | session and bill attached; no player row written | No error |
| Already has a player | converted twice | nothing written | `VALIDATION_FAILED` `session_id` |
| Settled bill | paid guest session | nothing written | `VALIDATION_FAILED` `session_id` |
| Stale / missing rev | wrong `base_rev` | nothing written | `STALE` / `VALIDATION_FAILED` |
| Phone in use | `create` with a taken phone | nothing written | `PHONE_TAKEN` |
| No `player.manage` | role with `session.start` only, `create` set | nothing written | `PERMISSION_DENIED` |
| Sessions screen | rows of today | filter by state; unpaid row opens its tile sheet with the settle bar | No error |

</intent-contract>

## Tasks & Acceptance

**Execution:**
- [x] Protocol (`SessionAssignPlayer`, `UiSessionConvertRequest`, `SessionListRow/State/View`, `SnapshotBill.session_rev/guest`, `IntentKind::SessionAssignPlayer`), store helpers (`assign_session_player`, `assign_order_player`, `list_sessions_for_day`)
- [x] Apply logic (load, base_rev, validate, mutate, permission) and `players::load_create`; business-day reader (`current_business_day`, `SnapshotReader::sessions_today`)
- [x] API: `POST /ui/sessions/convert`, `GET /ui/sessions`, `/ui/intent` refuses `create`
- [x] Rust tests for every matrix row, proptest op `SessionAssign`, replay entry, API tests
- [x] Hub: `ConvertGuest` (Convert to profile / Link to existing player) in the tile sheet and `settle-bar`, `CreateProfile` takes a custom save, Sessions screen with state filter, floor opens `?device=`, strings, audit label
- [x] Playwright spec for conversion, linking, no offer on a profiled bill, the Sessions screen and axe

**Acceptance Criteria:**
- Given a running, ended or unpaid guest session, when staff choose Convert to profile in the sheet or in the settle bar, then the Story 3.6 form opens in place and on save the session (and its bill) belong to the new player in one intent; the tile and bill show the player's name.
- Given an existing player, when staff choose Link to existing player and pick them in `player-lookup`, then the guest session and bill are attached to that player.
- Given the Sessions screen, then it lists today's sessions with state filters; selecting an unpaid row opens its settle bar; converted sessions show the player's name and the guest number stays in the row and the audit.
- Given a guest bill after conversion, then the wallet option becomes available for the linked player.

## Spec Change Log

## Review Triage Log

Re-run 2026-10-27: no open findings; fmt, clippy, hub check, strings, protocol, prettier and the proto/store/core/replay/api tests pass. `arena-api` `ui_events::a_component_change_also_reports_the_bundles_that_hold_it` failed once under load and passes alone and on rerun (50 ms sleep; not touched by this story).

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test` in `apps/hub`.

## Auto Run Result

Status: review
Verified: `cargo fmt --check`; clippy `-D warnings` clean for every crate except `arena-kiosk-svc` (its `tests/discovery.rs` has unused imports on this Windows machine, unchanged by this story); `cargo test` for proto, store, core (incl. the new conversion tests and the proptest with `SessionAssign`), replay and api (incl. convert and Sessions-list route tests); `pnpm run gen:types`; `pnpm --filter @arena/hub check`, check:strings, check:tokens-used, check:contrast, check:protocol, check:layers, format:check; the full Hub Playwright suite (827 passed, 11 skipped), including the new `convert.spec.ts`.
Not run: `arena-e2e` (Unix-only). These fail the same way on the untouched baseline: `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` and `binary::unreachable_at_start_retries_then_exits_1_without_logging_the_token`, `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`.
Left for later: other open Hub windows see a conversion with their next snapshot (no dedicated push event); the Sessions screen has no search and no date picker (today's business day only); credit options on a converted bill arrive with Story 3.18.
