---
title: 'Story 6.35: PC settings saved to the player''s profile'
type: 'feature'
created: '2026-10-29'
status: 'in-review'
baseline_commit: 'd5601b9d031a5d2bdfb075dccb47c3be83e7ba8f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-34-the-kiosk-account-page-details-wallet-and-packs-order-histor.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A regular player sets their mouse speed, display mode, sound output and keyboard layout again on every PC, and the next player inherits whatever they left.

**Approach:** The account menu gains a "PC settings" row (above Lock, profiled players and guests) that opens a panel. The kiosk service applies each change at once through the player's session; a profiled player's choices are saved by the Hub to a new `player_pc_settings` row (`player.pc_settings_set` intent) and applied when their next session starts on any PC; the session-end cleanup puts the PC back. A Manager sets the allowed display modes in Settings → Branch → Kiosk display (`kiosk.display_modes`).

## Boundaries & Constraints

**Always:** The Hub decides what is saved; the kiosk applies and renders. Mouse speed is Windows 1–20. A display mode is `WxH@Hz`; the panel offers only modes that are in the branch's allowed list (empty list = this PC's current mode only) **and** that this PC's monitor supports. A display-mode change reverts after 15 s unless the player presses "Keep this"; only a kept mode is saved. At session start the saved settings are applied one by one; a setting this PC cannot honour (mode not allowed or not supported, device or layout missing) is skipped and the PC keeps its baseline. At session end (the cleanup pipeline) mouse, display, audio output and keyboard layout return to the baseline the service recorded before the first change (display: the PC's own mode). A guest's changes are applied but never saved. The save is a device-actor intent allowed only for the profiled player on the device's own live session (the repo's 6.34 pattern). Copy lives in `en.json` (`player.pcsettings.*`, `settings.kiosk_display.*`).

**Never:** Editing the settings from the Hub per player, multi-monitor layouts, per-game settings, colour/brightness, changing the Windows default device for other users, a new permission or actor kind.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Open panel | session running | slider, display list (allowed ∩ supported), audio list, layout list, current values |
| Display change | pick a mode | applied; "Keep this" with a 15 s countdown; no answer → previous mode restored, nothing saved |
| Mouse/audio/layout change | profiled | applied at once and saved |
| Guest | any change | applied, not saved; Hub answers `saved:false` |
| Session start | saved row | applied if honourable; each unhonourable value skipped |
| Session end | cleanup runs | `pc_settings` step restores baseline; failure follows the 6.11 failure path |
| Hub unreachable | save fails | change stays applied for the session; "Counter is reconnecting — try again shortly." |
| Bad value | mouse 0 or 21, malformed mode | `VALIDATION_FAILED`, nothing stored |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{pc_settings.rs (new),operating.rs,frame.rs,intent.rs,lib.rs,scope.rs}` -- `PcSettings`, `KEY_DISPLAY_MODES`, mode parser, `player.pc_settings{,_result}`, `player.pc_settings_set{,_result}` frames, `PlayerPcSettingsSet` intent.
- `crates/arena-store/{migrations/0067_player_pc_settings.sql,src/{branch.rs,migrations.rs,lib.rs,pc_settings.rs (new)}}` -- change-log table, read, upsert.
- `crates/arena-core/src/{settings_defaults.rs,rbac.rs,devices.rs,apply/{mod,validate,mutate}.rs,apply/pc_settings.rs (new)}`, `tests/proptest_apply.rs` -- setting rule, device rule, intent.
- `crates/arena-api/src/lan/{session.rs,player_account.rs}` -- both frame handlers.
- `crates/arena-kiosk-core/src/ipc.rs`, `crates/arena-kiosk-svc/src/{pc_settings.rs (new),pc_settings_windows.rs (new),cleanup.rs,cleanup_windows.rs,cli.rs,app.rs,hub_link.rs,player_session.rs,runtime.rs,host.rs,operating.rs}` -- state, host trait + fake, Windows helper (SystemParametersInfo, ChangeDisplaySettingsEx, IMMDeviceEnumerator/IPolicyConfig, GetKeyboardLayoutList), cleanup step.
- `apps/kiosk/src-tauri/{src/main.rs,ipc-names.json}`, `apps/kiosk/src/lib/{PcSettings.svelte (new),AccountMenu.svelte,Shell.svelte,pc-settings.ts (new),bridge.ts}`, `packages/ui/src/messages/en.json`, `apps/hub/src/lib/{operating.ts,audit/labels.ts}` + Settings → Branch card, `docs/kiosk-accessibility.md`, generated types.
- Tests: proto, store, core (`apply/tests/pc_settings.rs`, proptest op), api `lan.rs`, kiosk-svc `pc_settings/tests.rs`, kiosk Playwright `pc-settings.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Protocol: types, frames, intent, `kiosk.display_modes`; regenerate TS types
- [x] Store: table 0067, upsert and read
- [x] Core: setting rule, intent apply/validate, device rule, proptest op
- [x] Hub LAN: `player.pc_settings` and `player.pc_settings_set`
- [x] Kiosk service: host trait, state, revert timer, session-start apply, cleanup step, Windows helper
- [x] Kiosk IPC, Tauri and app: menu row, panel, strings, accessibility notes
- [x] Hub: Kiosk display card, audit label
- [x] Tests for each layer

**Acceptance Criteria:**
- Given Settings → Branch → Kiosk display, when a Manager saves allowed display modes, then paired PCs read `kiosk.display_modes`; empty means each PC's current mode only.
- Given the account menu, then a "PC settings" row sits above Lock for profiled players and guests.
- Given the panel, then it shows a 1–20 mouse slider, the display modes allowed and supported, this PC's audio outputs and installed keyboard layouts.
- Given a display change not confirmed in 15 s, then the previous mode returns and nothing is saved.
- Given a profiled player with saved settings, when a session starts, then honourable settings are applied and others skipped; guests and players without a row keep the baseline.
- Given the session ends, then mouse, display, audio output and keyboard layout return to the baseline.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-kiosk-core`; `cargo fmt --check`; `cargo clippy -D warnings` on touched crates
- `pnpm gen:types`, `pnpm --filter @arena/kiosk check`, Playwright kiosk `pc-settings account-menu`; `pnpm check:strings check:hex check:no-blur check:protocol format:check`
