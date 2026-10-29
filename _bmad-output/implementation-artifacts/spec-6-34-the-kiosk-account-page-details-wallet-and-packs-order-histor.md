---
title: 'Story 6.34: The kiosk Account page — details, wallet and packs, order history, change password'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '08f47ea'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-17-the-account-menu-with-lock-and-log-out.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A player can see a balance and pack hours in the account menu, but not who the Hub thinks they are, their expired packs, what they ordered, or change their password — they ask staff.

**Approach:** An "Account" row below Messages opens an Account page: `account-subnav` (Details · Wallet & packs · Order history · Change password) on the left, the section on the right. Two new paired-only frames carry it: `player.account_page` (name, username, masked phone, wallet, every pack and pass with groups and expiry, the newest 50 orders with invoice number) and `player.password_change{current, new}`. The Hub finds the player from the session on the PC; the kiosk names nobody.

## Boundaries & Constraints

**Always:** The Hub decides; the kiosk renders. The phone is masked at the Hub (all but the last 4 digits). The current password is checked under the lock screen's throttle (five wrong in a row pause that PC for 60 s, no hash checked meanwhile, one `login_lockout` alert), the new one is 6 to 128 characters and hashed Argon2id at the Hub, then stored through the `player.password_change` intent (device actor, `PlayerManage`, allowed only for the profiled player on the device's own live session). Passwords are `SecretPin`s in IPC, never printed, cleared once sent, and cleared from the form fields the moment they are sent. Guests have no Account row and the Hub answers `NOT_FOUND`. Copy lives in `en.json` (`player.account.*`).

**Never:** Staff-side password reset changes, a new permission or player actor kind (the repo's device-actor pattern of Stories 6.9–6.32 is kept; the epic's `player.self_update` is the `PlayerManage` device rule), PC settings (Story 6.35), editing name or phone, top-ups at the kiosk, paging past 50 orders.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Open Account | profiled player, Hub up | page with Details active, pill-nav tab unchanged |
| Guest | guest session | no Account row; Hub `NOT_FOUND` |
| Details | name, username, phone | phone `+••••••••3210` |
| Expired pack | pack lapsed with 45 min unused | muted, "Expired", "0h 45m unused"; fully used packs hidden |
| Order history | settled and open orders | newest first, items, total, "Paid" / "Not paid yet", invoice number or "No invoice yet" |
| Wrong current | bad current password | `wrong_current`, field error, focus returns; counts toward the pause |
| Short new | fewer than 6 characters | refused in place, nothing sent |
| Five wrong | fifth wrong current | `locked{60}`: fields disabled, button counts down |
| Hub unreachable | link down | "Counter is reconnecting — try again shortly.", nothing sent |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{player_session.rs,players.rs,intent.rs,frame.rs,lib.rs}` -- `PlayerAccountPage`, `AccountPackEntry`, `AccountOrder`, `PlayerChangePassword`, `PlayerPasswordChangeResult`, the two frames, `IntentKind::PlayerPasswordChange`.
- `crates/arena-store/src/{sessions.rs,lib.rs}` -- `list_player_orders`, `Store::player_orders`, `Store::group_names`.
- `crates/arena-core/src/{player_account.rs,devices.rs,rbac.rs}`, `apply/{players.rs,mod.rs,validate.rs,mutate.rs}` -- `account_page`, `mask_phone`, the device rule, the intent.
- `crates/arena-api/src/lan/{player_account.rs,session.rs}` -- both frame handlers, shared throttle.
- `crates/arena-kiosk-core/src/ipc.rs`, `crates/arena-kiosk-svc/src/{app.rs,player_session.rs}`, `apps/kiosk/src-tauri/{src/main.rs,src/bridge.rs,ipc-names.json}` -- `player_account_page` request, `PlayerPasswordChange` IPC message.
- `apps/kiosk/src/lib/{Account.svelte,AccountMenu.svelte,Shell.svelte,account.ts,account-text.ts,bridge.ts}`, `packages/ui/src/messages/en.json`, `apps/hub/src/lib/audit/labels.ts`, generated types, `docs/kiosk-accessibility.md`.
- Tests: `arena-core` `player_account_page`, `proptest_apply.rs` (`PasswordChange`), `arena-api` `lan.rs`, `arena-proto`, `arena-kiosk-core` ipc, kiosk `account.spec.ts` and `account-menu.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol: page and password frames, `player.password_change` intent; regenerate TS types
- [x] Store: newest orders of a player with product lines, group names
- [x] Core: `account_page`, phone mask, device password-change rule and apply wiring, proptest op
- [x] Hub LAN: `player.account_page` and `player.password_change` under the unlock throttle
- [x] Kiosk service, IPC and Tauri: request and secret-carrying message
- [x] Kiosk app: Account row, Account page, strings, accessibility notes
- [x] Hub audit label
- [x] Tests for each layer

**Acceptance Criteria:**
- Given the account menu for a profiled player, then it gains an "Account" row below Messages; guests do not see it.
- Given Account, then the page opens with the sub-nav on the left, the active item in `text` with a 3px left bar, arrow keys moving between items, and the pill-nav tab the player came from still active.
- Given Details, then name, username and the phone with all but the last 4 digits masked.
- Given Wallet & packs, then the balance with "Top-ups are accepted at the counter.", each owned pack with valid groups, expiry and hours left, passes with their window, expired packs muted with "Expired" and unused time.
- Given Order history, then this player's orders newest first with date, items, total, status and invoice number.
- Given Change password with the current and a new password (min 6), then the Hub stores a new Argon2id hash and the page shows "Password changed."; a wrong current password is a field error and five wrong ones lock the form for 60 s.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-kiosk-core -p arena-kiosk-ui`; `cargo fmt --check`; `cargo clippy -D warnings` on touched crates
- `pnpm gen:types`, `pnpm --filter @arena/kiosk check`, Playwright kiosk `account account-menu`; `pnpm check:strings check:hex check:no-blur check:protocol format:check`

## Auto Run Result

Status: review

- **Summary:** `player.account_page` and `player.password_change` frames (paired only) back a new kiosk Account page. The Hub opens the player's sealed name and phone (masking the phone), lists every pack and pass (expired ones with unused time) and the newest 50 product orders with invoice numbers. The password form is checked under the lock screen's throttle, hashed at the Hub and stored by a new `player.password_change` intent that a device may apply only for the profiled player on its own live session.
- **Verification:** see Verification; kiosk Playwright (all but the four pre-existing Windows-path `theme` failures), arena-core (incl. proptest with the new `PasswordChange` op), arena-api `lan` account/password tests pass; strings, hex, no-blur, protocol, layers, pins, prettier pass.
- **Residual risks:** `arena-kiosk-svc` has the same Windows-only test-target lint and test failures as before; the real Tauri shell and a real Hub were not exercised; the unlock throttle is shared with the lock screen, so wrong passwords on either count toward one pause; the proptest invariant for a settled counter sale now allows a coupon-zeroed sale with no payment (it flaked on a pre-existing case); no review pass has run.
