---
title: 'Story 6.17: The account menu with Lock and Log out'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '23ceede0f8a05f4ea2f33aceb874e375f0a28ba7'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-9-the-plan-picker-starts-a-prepaid-session-from-the-kiosk.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-16-staff-messages-reach-the-seat-and-stay-in-the-messages-bell.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The shell's account chip and time chip are plain text. A player cannot see their balance or packs, cannot lock the screen when stepping away, and cannot end their own session at the PC.

**Approach:** Both chips become buttons that open an `account-menu` panel (name, wallet, time left; rows Balance, Messages, Lock, Log out). Three new paired-only Hub frames carry what the Hub alone knows, none naming a player or session (the Hub reads the session running on the paired device): `player.account` (balance and packs), `player.end_session` (Log out) and `player.unlock` (the player's own password). Lock is a kiosk-local screen (`LockReason::LockedByPlayer`) that leaves the session and clock running. Log out builds `session.end` at the Hub with the device as actor, then pushes `PAY_AT_COUNTER` (a bill) or the new `LOGGED_OUT` (paid-for time: cleanup, then attract).

## Boundaries & Constraints

**Always:**
- The menu opens from the account chip or the time chip, takes focus on its first row, moves with the arrow keys, and Esc closes it and returns focus to the chip that opened it. A click outside closes it.
- Balance and Lock are offered to a profiled player only; a guest sees Messages and Log out.
- Balance shows the wallet (less what running wallet sessions set aside) and the unexpired packs and passes valid for the PC's group, with "Top-ups are accepted at the counter."
- Messages closes the menu and opens the messages bell's list.
- Lock covers the shell and any game with a solid screen; time keeps running and messages stay. It opens only with the signed-in player's own password (paste allowed; the password goes to the service only and is cleared when sent) or by staff Unlock from the tile sheet. Five wrong passwords in a row pause the field for 60 s with a countdown (the Hub's own throttle, per PC, with the `login_lockout` alert once).
- Log out on prepaid time left asks in place "You still have {time}. Log out anyway?" (focus on Stay); a postpaid session ends at once. Both go through `session.end` with the Hub's actor for the paired device and the session's own `rev`. Hub unreachable: nothing is sent and the row says "Counter is reconnecting — try again shortly."
- A device actor may end only the session on its own PC (`rbac::device_ends_own_session`); another PC's session, force end and every other session right stay denied.

**Never:**
- A new staff permission, an "I'm done" row (6.18), Extend, Account and PC settings rows (6.21, 6.34, 6.35), a player-lock state kept across a kiosk service restart, backdrop blur, code comments beyond the codebase's doc-comment style.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open | click either chip, profiled | menu with Balance, Messages, Lock, Log out | Esc closes, focus back on the chip |
| Balance | Hub connected | wallet + pack hours + counter line | Hub down: "Counter is reconnecting", nothing sent |
| Lock | profiled, unlocked | `locked_by_player` cover, clock runs | guest or already locked: refused, line in menu |
| Unlock | right password | Hub `ok`, service lifts the lock | wrong: field clears, line; 5th: 60 s pause |
| Unlock | staff Unlock | `PcAction::Unlock` lifts it | N/A |
| Log out | prepaid, time left | confirm with real time, then `session.end` + `LOGGED_OUT` | refused: line "Couldn't log out" |
| Log out | postpaid | `session.end` + `PAY_AT_COUNTER` lock, bill to the minute | Hub down: reconnecting line |

</intent-contract>

## Code Map

- `crates/arena-proto/src/player_session.rs`, `frame.rs`, `pc_command.rs`, `lib.rs` -- `AccountPack`, `PlayerAccountResult`, `PlayerEndResult`, `PlayerUnlock(+Result)`, six frame kinds (31), `LockReason::{LockedByPlayer, LoggedOut}`; generated types via `pnpm gen:types`.
- `crates/arena-core/src/player_account.rs`, `devices.rs`, `rbac.rs` -- `device_session`, `player_account`, `player_password_hash`; `device_ends_own_session`.
- `crates/arena-api/src/lan/player_account.rs`, `session.rs`, `mod.rs`, `player_login.rs` -- the three frames, a second throttle (`unlock_throttle`), reuse of `Reservation` and `raise_lockout`.
- `crates/arena-kiosk-svc/src/player_session.rs`, `app.rs`, `pc.rs`, `cleanup.rs`; `crates/arena-kiosk-core/src/ipc.rs` -- `account`, `end`, `lock`, `unlock`, IPC `player_unlock`, `LoggedOut` is an end-of-session reason that shows no lock once clean.
- `apps/kiosk/src-tauri` -- commands `player_account`, `player_end`, `player_lock`, `player_unlock`, `ipc-names.json`.
- `apps/kiosk/src/lib` -- `AccountMenu.svelte`, `UnlockForm.svelte`, `account.ts` (pure), `account-text.ts`, `Shell.svelte` (chips as buttons), `MessagesBell.svelte` (`show()`), `KioskStage.svelte` (unlock form on the cover), `bridge.ts`, `routes/+page.svelte`, `packages/ui/src/messages/en.json`.
- Tests: `crates/arena-api/tests/lan.rs`, `arena-core` `apply/tests/player_start.rs`, `arena-kiosk-svc` `cleanup/tests.rs`, `arena-proto` `player_session.rs`, `arena-kiosk-core` `ipc.rs`, `apps/kiosk/tests/account-menu.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto` -- account, end and unlock frames and results; `LockedByPlayer` and `LoggedOut`; `pnpm gen:types`
- [x] `crates/arena-core` -- session, account and password-hash reads; device may end its own PC's session
- [x] `crates/arena-api` -- `player.account`, `player.end_session`, `player.unlock` (own throttle and alert); lock push after the end
- [x] `crates/arena-kiosk-svc`, `arena-kiosk-core` -- account/end/lock/unlock handlers, `LoggedOut` cleanup and attract, IPC `player_unlock`
- [x] `apps/kiosk` (+ `src-tauri`) -- account menu, Balance view, Log out confirm, lock cover with the unlock form, strings `player.account.*`, `player.lock.by_player*`, `player.unlock.*`; Playwright
- [x] Story file and sprint status

**Acceptance Criteria:**
- Given the account chip or time chip, when activated, then the menu opens with the header and rows, and Esc returns focus to the chip.
- Given Balance, then wallet and pack hours show with the counter line. Given Messages, then the bell list opens.
- Given Lock on a profiled player's session, then a solid cover shows, time runs, and only the player's password or staff Unlock lifts it; guests are not offered Lock; five wrong passwords pause the field for 60 s.
- Given Log out with prepaid time left, then the menu asks with the real time, and on confirm the session ends, cleanup runs and the PC returns to attract. Given postpaid, then it ends and the PC locks to "Pay at counter".
- Given the Hub unreachable, then Log out sends nothing and says the counter is reconnecting.

## Spec Change Log

## Review Triage Log

Code review (unattended): no blocking findings. Hub-side throttle, device-scoped end right, lock/unlock gating and kiosk UI checked against the ACs; typecheck, tests and Playwright green.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-core -p arena-kiosk-core -p arena-api -p arena-kiosk-ui` -- expected: pass (kiosk-svc `--lib` has known SYSTEM-ownership failures on this host)
- `cargo fmt --check`, `cargo clippy` on the changed crates -- expected: clean
- `pnpm gen:types && pnpm check:strings && pnpm check:hex && pnpm check:no-blur && pnpm check:protocol && pnpm format:check` -- expected: pass
- `pnpm --filter @arena/kiosk check`, `pnpm --filter @arena/hub check`, Playwright kiosk suite -- expected: pass apart from the 4 `theme.spec.ts` tests

## Auto Run Result

Status: done

Verified: cargo tests for arena-proto, arena-core, arena-kiosk-core, arena-api and arena-kiosk-ui pass; the three new `arena-kiosk-svc` unit tests (player lock, password unlock gating, logged-out attract) pass; fmt and clippy clean on the changed crates; gen:types, check:strings|hex|no-blur|protocol|layers, format:check, kiosk and hub svelte-check pass; the kiosk Playwright suite passes (177) apart from the 4 `theme.spec.ts` tests (existing `C:\C:\` path bug).

Not run: `arena-kiosk-svc/tests/runtime.rs` (`cfg(unix)`), the real Tauri shell and a real LAN round trip, the lock cover over a fullscreen game. `check:generated` reports the new generated files as untracked until they are committed. `check:tokens-used` reports `--ad-reserved` in the Hub floor page (existing, not touched here).

Known flake (existing, not from this story): `proptest_apply` `apply_invariants_hold_over_random_intent_sequences` failed once in about ten runs with `DeviceUnassigned` on a generated `Session` start after `Scan` steps; re-runs pass.
