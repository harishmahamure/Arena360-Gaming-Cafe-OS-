---
title: 'Story 6.21: Extend from the kiosk with live prices'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: 'd44187a'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-20-per-branch-warning-marks-and-the-low-balance-warning.md'
warnings: []
deferred:
  - 'The grace overlay with Extend and the branch time-up mode: Story 6.22. Extend during grace already works on the Hub (a `grace` session extends); the overlay is 6.22.'
  - 'A pass or block session (prepaid, but not extendable) opens the panel and is told "This session can''t be extended." The session screen carries no mode, so the kiosk cannot hide the row and key for it without a new field.'
---

<intent-contract>

## Intent

**Problem:** A prepaid player who is warned about running out of time still has to walk to the counter: the kiosk cannot quote an extend, cannot pay for one and `session.extend` is outside the paired-device scope.

**Approach:** Two paired-only frames, `player.extend_options{}` and `player.extend{minutes, source, pack_id?}`, in the 6.17 style: they name no player and no session, the Hub finds both from the session on the paired device. The Hub quotes +30 min, +1 h and +2 h at the live rate and lists the sources (wallet, packs valid for the group, credit headroom). The extend builds the existing `session.extend` with the device as actor, so Story 3.28 applies unchanged; a refusal returns typed. The HUD's expanded panel gains `extend-options`; Ctrl+Shift+E, Extend on a warning toast and Extend in the account menu all open it.

## Boundaries & Constraints

**Always:** the price quoted is the price charged (one `pricing::price_of` at the `live_rate`); the kiosk sends a step, a source and a pack id only; a refusal leaves the panel open and the other options usable; with the Hub unreachable every option is disabled and says "Counter is reconnecting — try again shortly."; focus lands on the first option; Extend shows only for a profiled player on prepaid time.

**Never:** a manager override on the kiosk (a device actor with `credit_override` is denied by `rbac`); a new `IntentKind`, `ActorKind` or `ErrorCode`; a kiosk naming a session or player; a postpaid or guest Extend.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Open options | pack/wallet session, profiled | three quotes at the live rate; wallet (net of reserved), group packs (not the session's own), credit headroom if enabled |
| Add time | step + source, Confirm | `session.extend` applies; "More time added. GG."; panel and warning toasts close; PC gets the new end |
| Past closing | new end after closing | refused `PAST_CLOSING`: "Can't extend past closing time (11:00 pm)." |
| Credit over limit | headroom short | `CREDIT_LIMIT`: "Credit limit reached, see counter."; override denied |
| Wallet short / pack short | balance below price | `INSUFFICIENT_BALANCE`: "Balance ₹40.00. Top up at counter." / pack hours left |
| Postpaid, guest, empty PC | no extendable session | no row, no Extend button; Hub answers `VALIDATION_FAILED` / `NOT_FOUND` |
| Pass or block | prepaid, fixed window | panel says "This session can't be extended." |
| Hub unreachable | link down | options disabled with the reconnecting line; nothing asked |

</intent-contract>

## Code Map

- `crates/arena-proto/src/player_session.rs`, `frame.rs`, `lib.rs` -- `ExtendQuote`, `ExtendPackSource`, `PlayerExtendOptions`, `PlayerExtend` (+`validate`), `PlayerExtendResult`; four frame kinds (`FRAME_KINDS` 35); generated TS.
- `crates/arena-core/src/player_extend.rs`, `devices.rs`, `pricing.rs` (`price_of`, shared with `apply/extend.rs`), `session_screen.rs` (`closing_in`), `rbac.rs` (`device_extends_own_session`).
- `crates/arena-api/src/lan/player_extend.rs`, `lan/session.rs` -- handlers, dispatch, `session.started` push after an extend.
- `crates/arena-kiosk-svc/src/{player_session,app}.rs`, `apps/kiosk/src-tauri/{src/main.rs,src/bridge.rs,ipc-names.json}` -- `player_extend_options`, `player_extend`, `hud_expand`.
- `apps/kiosk/src/lib/{extend.ts,extend-text.ts,ExtendOptions.svelte,Hud.svelte,AccountMenu.svelte,bridge.ts}`, `packages/ui/src/messages/en.json` -- panel, toast Extend (`button-warning` tokens), account row with key caps, copy.

## Tasks & Acceptance

- [x] Proto frames and types, regenerated TS
- [x] Hub quote read (live rate, wallet net of reserved, packs, credit headroom, closing label) and `rbac` rule for a device extending its own PC
- [x] LAN handlers: paired-only, typed refusals, session screen pushed after an extend
- [x] Kiosk service forwarders (validated before leaving the PC), Tauri commands incl. `hud_expand`
- [x] HUD `extend-options` panel, Extend on warning and low-balance toasts, account menu Extend row
- [x] Tests: Rust (proto, core, api, kiosk-svc), Playwright (`extend.spec.ts`, `account-menu.spec.ts`)

## Spec Change Log

- The epic names `session.extend{actor: player}` with `session.self_extend`. As in 6.9 and 6.17 the actor is the paired device (a kiosk cannot assert a player) and no permission is added; the change log shows the device, the order shows `settled_by` the device.
- Ctrl+Shift+E on prepaid time opens the panel with the options below the time, balance and plan, rather than a separate window; postpaid keeps the panel as it was.

## Verification

- `cargo test -p arena-proto -p arena-core -p arena-kiosk-core -p arena-api` -- pass; `cargo test -p arena-kiosk-svc --lib` -- new tests pass, the same 14 lockdown/pipe/DPAPI/config tests fail as before (Windows privileges)
- `cargo clippy -D warnings` clean on proto, core, api, kiosk-core, kiosk-svc (`--lib --bins`) and the Tauri shell; `cargo fmt --check` clean
- `pnpm --filter @arena/kiosk check` -- 0 errors; Playwright kiosk suite 226 pass, the 4 `theme.spec.ts` tests fail as before (doubled drive-letter path)
- `pnpm check:strings check:hex check:no-blur check:layers check:pins check:protocol format:check` -- pass; `check:tokens-used` reports `--ad-reserved` in the Hub floor page (before this story); `check:generated` reports the new generated types only because they are uncommitted

Not run: a real Hub-to-kiosk round trip, the Tauri shell, the global hotkey.
