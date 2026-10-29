---
title: 'Story 6.9: The plan picker starts a prepaid session from the kiosk'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '9b11e017ec6a9e487beda343949084f03b876706'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-8-a-player-logs-in-at-the-kiosk-with-username-and-password.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** After the 6.8 login the kiosk only shows a holding step ("Ask at the counter to start your time."), so a prepaid player still needs staff to start the session. The kiosk cannot list the player's packs, passes, blocks and wallet, and cannot start a session: `session.start` is outside the paired-device scope, and the Hub keeps no record of who logged in.

**Approach:** The Hub remembers a successful `player.authenticate` per device (short-lived login record). Two new paired-only frames use it: `player.plans` returns the options valid for this PC's group; `player.start_session` re-checks the record, then applies the existing `session.start` (Stories 3.24 and 3.27) bound to that player and device, and pushes `session.started` to the PC. The Attract holding step is replaced by a `plan-picker` that selects an option, confirms, shows "Waiting for the counter…", and shows a typed refusal on the picker.

## Boundaries & Constraints

**Always:**

- **Login record.** `authenticate_player` on `ok` stores `device_id -> {player_id, expires_at}` in `LanHub` (120 s, refreshed by each `player.plans`/`player.start_session`). The kiosk never names a player in the new frames. A new paired-only `player.logout` frame (and a new `ok` login on the device) drops it. No record, expired or the session closed: `player.plans`/`player.start_session` answer an `error` frame with a new reason string, not a new `ErrorCode`; the app treats it as "log in again" and returns to attract.
- **Frames** (all `paired_device()`, others `FORBIDDEN`; not a primary Hub / no directory answers `error` as 6.8): `player.plans{}` -> `player.plans_result{wallet{balance_minor, rate_minor, rule?}, options[]}`; `player.start_session{choice}` with `choice` = `pack{pack_id}` | `pass{pack_id}` | `wallet` | `block{plan_id}` -> `player.start_result{outcome}`, `outcome` = `ok` | `refused{code, message}`; `player.logout{}` -> `ack`. Types in `arena-proto/src/player_session.rs`; no hand-written TS.
- **Options.** Only for this device's group: each owned pack (name, `minutes_left`, `expires_at`), each owned pass (window label), each active block plan whose group list has the group (name, `price_minor`, window end time today), and Wallet (available balance after reservations, hourly rate, active rule label as `chip-rule`). A pack/pass is `selectable=false` with tag `expired` (and unused minutes) when expired, or tag `outside_window` (with the window) when a pass/block window is closed now; a pack with no time left is `expired`. Postpaid is never offered. Ordering: packs, passes, blocks, then Wallet.
- **Start.** The Hub builds `SessionStart{device_id: this device, player_id: the logged-in player, mode, pack_id|plan_id}` and submits it through the writer as the paired device's actor; the new rule in `rbac::check` lets `ActorKind::Device` start a session only when `Requirement` is `SessionStart`, `actor.id == device_id` and the mode is `pack|pass|wallet|block` with `player_id` set. `PAIRED_SCOPE` is unchanged, so a kiosk cannot send `session.start` directly. The player stays on `session.player_id`; the change log shows the device actor.
- **Block pays from the wallet in the same intent.** `SessionStart` gains `#[serde(default)] pay_from_wallet: bool`, accepted only for a block with `player_id`. Then `apply` refuses `INSUFFICIENT_BALANCE` (field `player_id`) when `available_wallet < price` (the wallet never goes below zero), and otherwise, after `open_block_bill`, inserts a `wallet` payment of the block price, settles the order and debits the wallet with a `bill` wallet txn (reuse `insert_payments`, `settle_order`, `record_payer_txns`). The player frame always sets it for `block`. Staff-started blocks (flag false) are unchanged.
- **After a successful apply** the handler pushes `session.started` to the PC exactly as the staff path in `arena-api/src/ui_auth.rs` does (share the helper), then answers `ok`. The PC unlocks to the Story 3.10 session screen within 2 s; the app leaves the picker when `pc-screen` brings a session (player forgotten).
- **Refusals.** `refused{code, message}` carries `INSUFFICIENT_BALANCE`, `PACK_EXPIRED`, `PLAN_NOT_FOR_GROUP`, `PASS_OUTSIDE_WINDOW`, `PC_BUSY` as-is (other `AppError`s as their code). The picker maps them to catalogue copy (`INSUFFICIENT_BALANCE` -> `player.balance.insufficient` with the balance from the last options), keeps the player on the picker with the selection cleared, and refreshes the options.
- **Picker UI.** Cards in one grid; selected card 2px `text` border; one primary "Start" button, disabled until a selectable card is chosen; unselectable cards are muted with their tag and cannot be focused for selection; keyboard: arrow keys move, Enter/Space selects, Tab to Start; 60 s with no input on the picker forgets the player (calls `player.logout`) and returns to attract; "Not you? Log out" kept. While waiting: "Waiting for the counter…" with the selected plan name, all controls disabled; if no `pc-screen` session arrives within 10 s after `ok`, the picker returns with "Counter is reconnecting — try again shortly.".
- **Hub unreachable** (`hub-status` not `connected`, or no answer): Start is not sent and the picker shows "Counter is reconnecting — try again shortly." inline; options already shown stay.
- Copy from the catalogue (`player.picker.*`); money via the branch locale formatter; no `backdrop-filter`; no player context written by the kiosk to disk; no hex colours (tokens).

**Never:** postpaid start from the kiosk; sign-up (6.10); a new `IntentKind`, `ActorKind` or `ErrorCode`; the kiosk asserting a player id; adding `SessionStart` to `PAIRED_SCOPE`; changing the Story 3.24/3.27 rules for staff-started sessions; extend, shell, HUD (later stories).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Options | logged in, group A | owned packs/passes/blocks for A + Wallet; none for other groups | — |
| Expired pack / closed pass | expired or off-window | muted card, "Expired" + "0h 45m unused" or window; not selectable | — |
| Start a pack | selectable pack, Start | `ok`; waiting; session screen within 2 s | — |
| Start wallet | balance covers minimum | `ok`; session | — |
| Wallet too low | balance < minimum | `refused INSUFFICIENT_BALANCE`; "Balance ₹40. Top up at counter." | stays on picker |
| Block, wallet covers | price <= available | session ends at window end; order paid by wallet in one intent | — |
| Block, wallet short | price > available | `refused INSUFFICIENT_BALANCE`; no session, no txn | stays on picker |
| Group mismatch / pack expired between list and start | stale options | `refused PLAN_NOT_FOR_GROUP` / `PACK_EXPIRED` / `PASS_OUTSIDE_WINDOW` | refresh options |
| PC busy | session already on PC | `refused PC_BUSY` | — |
| No/expired login record | 120 s idle | `error` frame; app returns to attract | — |
| Non-paired caller | phone/kds | `FORBIDDEN` | — |
| Device actor, other mode/device | `postpaid`, or `device_id` of another PC | denied by `rbac::check` | writes nothing |
| Hub down at Start | link not connected | not sent; reconnecting line | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/player_session.rs` (new) -- option/choice/outcome types; `frame.rs` kinds `player.plans`, `player.plans_result`, `player.start_session`, `player.start_result`, `player.logout`; `session.rs:31` `SessionStart.pay_from_wallet`; `lib.rs` re-exports; generated TS.
- `crates/arena-core/src/rbac.rs:103` `check` -- device actor may `session.start` as above; unit tests; `apply/session.rs:42 load / 118 validate`, `apply/prepaid.rs:43 load_start / 113 check_start / 495 open_block_bill` -- wallet-paid block; reuse `session.rs:800 insert_payments`, `836 record_payer_txns`, `settle_order`, `available_wallet` (1644).
- `crates/arena-core/src/devices.rs:242` `DeviceDirectory::login_candidate` (pattern) -- new `player_options(player_id, device_id)` from one `read_consistent` snapshot: `Store::player_packs` (`arena-store/src/lib.rs:1227`), `list_plans`/`list_plan_groups` (`plans.rs:38-58`), `find_wallet`, `attract.rs:59` rate/rule, `windows::PlayWindow`.
- `crates/arena-api/src/lan/player_login.rs` (login record), new `lan/player_session.rs` (handlers), `lan/session.rs:561-611, 759, 961` (dispatch, `paired_device`, binding), `lan/registry.rs:326 push_session_started`, `ui_auth.rs:2134,2265-2294` (extract the push helper).
- `crates/arena-kiosk-core/src/ipc.rs:174` -- `IpcMsg::PlayerPlans|PlayerStart|PlayerLogout`; `arena-kiosk-svc/src/{player_login.rs (pattern),app.rs:411,lib.rs}` -- forwarders.
- `apps/kiosk/src-tauri/{ipc-names.json,src/main.rs,src/bridge.rs:85}` -- commands `player_plans`, `player_start`, `player_logout`.
- `apps/kiosk/src/lib/{bridge.ts:376 (pattern),plans.ts (new),PlanPicker.svelte (new),Attract.svelte:227-239,Login.svelte}`, `routes/+page.svelte` -- picker replaces `login-holding`.
- `packages/ui/src/messages/en.json` (`player.picker.*`; reuse `player.balance.insufficient`, `player.hub.reconnecting_retry`, `player.login.holding.logout`).
- Tests: Rust beside each; `apps/kiosk/tests/{plan-picker.spec.ts (new),login.spec.ts,attract.spec.ts}`; `crates/arena-core/tests/proptest_apply.rs` (device-actor `session.start` denial case).

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- picker types, five frame kinds, `pay_from_wallet`; regenerate types; tests
- [x] `arena-core` -- device actor rule in `rbac::check`, wallet-paid block in `apply`, `DeviceDirectory::player_options`; unit tests for every matrix row and a proptest denial case
- [x] `arena-api` -- login record, `player.plans`/`player.start_session`/`player.logout` handlers, shared `session.started` push; tests incl. non-paired `FORBIDDEN`, expired record, refusals
- [x] `arena-kiosk-core` / `arena-kiosk-svc` -- three `IpcMsg` variants and forwarders; tests
- [x] Tauri shell -- three commands and names list; tests
- [x] Kiosk app -- `PlanPicker.svelte`, `plans.ts`, Attract wiring, 60 s idle, waiting/refusal/reconnecting states; Playwright `plan-picker.spec.ts`; update `login.spec.ts` holding tests
- [x] Catalogue strings `player.picker.*`

**Acceptance Criteria:**

- Given a successful login, then the `plan-picker` replaces the holding step, keeps "Not you? Log out", and 60 s with no input forgets the player and returns to attract.
- Given the picker, then it shows only options for this PC's group (packs, passes, blocks, then Wallet with balance, hourly rate and `chip-rule`) and never postpaid.
- Given an expired pack or a closed pass, then it is muted with "Expired" and unused time (e.g. "0h 45m unused") or its window, and cannot be selected.
- Given a selected option and Start, then the card has a 2px `text` border, the request is sent, and "Waiting for the counter…" shows with the plan name.
- Given an accepted start, then the PC shows the session screen within 2 s and the floor tile shows the player in session; a pack/pass/wallet start follows Stories 3.24/3.27.
- Given a block chosen, then its price is paid from the wallet in the same intent (never below zero) and the session ends at the block window's end.
- Given a refusal (`INSUFFICIENT_BALANCE`, `PACK_EXPIRED`, `PLAN_NOT_FOR_GROUP`, `PASS_OUTSIDE_WINDOW`, `PC_BUSY`), then the picker stays open with the typed reason (e.g. "Balance ₹40. Top up at counter.") and the player can choose another option.
- Given the Hub is unreachable, then Start sends nothing and shows "Counter is reconnecting — try again shortly.".
- Given a kiosk that sends `session.start` directly or a non-paired class sends the new frames, then `FORBIDDEN`.

## Spec Change Log

## Review Triage Log

## Design Notes

The epic names an `actor{kind: player}` and a `session.self_start` permission. Adding an `ActorKind` means widening the `changes.actor_kind` CHECK in two migrations and every total match; 6.8 argued the kiosk must not assert a player. This story keeps the same stance: the Hub holds the login record and builds the intent itself, with the paired device as actor and the player on the session row (audit shows both through the session). A real player actor can replace the device actor later without changing the frames.

The login record is Hub state in `LanHub` only (not branch state, not in the change log). The kiosk sends no player id, so a compromised app cannot start a session for someone else; at worst it uses the player who just logged in on that PC.

## Verification

**Commands:**

- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-kiosk-core -p arena-kiosk-svc` -- expected: pass
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` -- expected: clean
- `pnpm gen:types`, `pnpm check:strings`, `pnpm check:hex`, `pnpm check:no-blur`, `pnpm format:check` -- expected: pass
- `pnpm --filter @arena/kiosk check`, `pnpm --filter hub check` -- expected: 0 errors
- Playwright `apps/kiosk/tests/plan-picker.spec.ts`, `login.spec.ts`, `attract.spec.ts` -- expected: pass

## Implementation Notes

- The login record (`LoginRecords` in `arena-api/src/lan/player_login.rs`) is keyed by device and also bound to the LAN connection that logged in (a per-connection number), so "the session closed" kills it: a reconnect means log in again. It is also dropped after a successful start (the player is on a session; a retry would be `PC_BUSY` anyway).
- "Log in again" is `AUTH_FAILED` with the message `player login required` (`PLAYER_LOGIN_REQUIRED`); the shell's `Answer` carries only the code, so the app treats `AUTH_FAILED` from the picker commands as that signal.
- The wallet option (and `wallet` in `player.plans_result`) is offered only when the PC's group has an active rate; with no group or a PC in maintenance there are no options.
- `rbac::check` lets a device actor start `pack|pass|wallet|block` with a player on its own PC; it does not itself require `pay_from_wallet` on a block (a kiosk cannot send `session.start` at all; only the Hub builds it, always with the flag for a block).
- A block paid from the wallet stamps the payment, wallet txn and settlement with the device id as the actor.
- With the Hub link not `connected` the Start button is disabled with "Counter is reconnecting — try again shortly." (nothing is sent); a start with no answer shows the same line inline.
- `player.attract.login.hint` ("Ask at the counter to start your time.") was removed with the holding step.
- Not run on this Windows host: the `cfg(unix)` `arena-kiosk-svc/tests/runtime.rs` tests added here (type-checked only). `arena-e2e`, `arena-kiosk-svc/tests/discovery.rs`, 14 `arena-kiosk-svc` lib tests (SYSTEM ownership) and `apps/kiosk/tests/theme.spec.ts` (doubled drive-letter path) fail before this story as well. `pnpm check:generated` reports the new generated types as untracked until committed.
