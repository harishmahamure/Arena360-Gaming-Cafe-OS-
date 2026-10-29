---
title: 'Story 6.14: The overlay HUD shows time and balance without stealing focus'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: 'a4b7e22'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-13-the-player-shell-with-top-bar-pill-nav-and-time-chip.md'
---

## Intent

**Problem:** Once a game is in front, the player's shell (Story 6.13) is behind it: they cannot see their time or balance without leaving the game. The blocked-app notice (Story 1.18) lived in the shell's own window, so it was hidden by the game too.

**Approach:** A second Tauri window, `hud`, always on top in the top-right of the primary monitor and non-focusable, draws a solid `hud-pill` (time left or elapsed, and the wallet balance for a profiled player) and the `hud-toast`s. Ctrl+Shift+E (caught by the existing low-level keyboard hook) expands the pill into a `hud-toast`-palette panel, moves focus into it and Esc gives the focus back to the window that had it. A Direct3D fullscreen-exclusive game hides everything.

## Design

- Proto: `SessionScreen.{balance_minor?, plan_name?}`. `session_screens` fills them for a profiled player: the wallet balance, and the name of the pack or pass the session runs on (`None` for wallet time and pay-as-you-go). Guests get neither. The Kiosk service carries them to the app unchanged (`ScreenSession`, `PcSession`).
- The HUD page (`/hud`, `Hud.svelte`) follows the same `pc-screen` / `attract` / `theme` / `toast` events as the shell. Money is formatted with the branch locale from the attract view; until it is known the balance is not shown.
- The page is the source of truth for the window's size: it reports the measured content box through `hud_layout(width, height)` (`0 × 0` for nothing) and the shell places, shows or hides the window ([`arena_kiosk_core::hud`]: `place`, `window_visible`). A session that is locked, or has ended, draws nothing.
- `hud.rs` (shell): builds the window from `tauri.conf.json` non-focusable and hidden; `expand` (hotkey) remembers the foreground window, makes the HUD focusable and focuses it; `collapse` (Esc or Close, `hud_collapse`) makes it non-focusable again and restores the remembered window. A 1 s watcher asks `SHQueryUserNotificationState`; only `QUNS_RUNNING_D3D_FULL_SCREEN` hides the overlay (a borderless-fullscreen window, including the kiosk's own shell, reports `QUNS_BUSY` and must not).
- The hotkey is decided in the keyboard hook (`is_expand_hotkey`: Ctrl+Shift+E down, no Alt or Windows key) and swallowed only while a session runs and is not locked, so outside a session the key reaches whatever has it.
- Toasts: an info toast goes after 8 s, a warning stays until dismissed (`hud.ts`: `raise`, `live`); the same text again replaces the one showing. Every toast chimes (two notes for a warning, one for info) with Web Audio, so no audio asset ships. The blocked-app toast (`player.app.blocked`) is a warning kind and moved from the kiosk stage to the HUD. The layout is split: `routes/+layout.svelte` now only chooses between the bare HUD page and `KioskStage.svelte` (the old layout).

## Tasks & Acceptance

- [x] `arena-proto` / `arena-core` / `arena-kiosk-core` / `arena-kiosk-svc` / Tauri bridge: carry `balance_minor`, `plan_name`; generated types
- [x] `arena-kiosk-core::hud`: hotkey, notification state, window visibility and placement
- [x] Shell: `hud.rs`, `hud_layout` / `hud_collapse` commands, `hud-expand` event, hook integration, `hud` window and capability
- [x] Kiosk app: `/hud` route, `Hud.svelte`, `hud.ts`, bridge parsing, strings `player.hud.*`, `KioskStage.svelte` split, blocked-app toast moved
- [x] Tests: Rust (`session_screens`, `hud`), Playwright `hud.spec.ts`, `toast.spec.ts` and `theme.spec.ts` on the HUD route

**Acceptance:**

- A running session shows a solid pill in a separate always-on-top window with time left or elapsed and, for a profiled player, the wallet balance in `hud-numeric`; guests see time only; a lock or the end of the session empties it (`hud.spec.ts`).
- The window is non-focusable until Ctrl+Shift+E; the panel shows time, balance and the plan, focus moves into it and Esc or Close collapses it and returns the focus (`hud.spec.ts` for the page; the focus hand-over itself is Windows-only and not run here).
- A fullscreen-exclusive game hides the pill and every toast (`arena_kiosk_core::hud` tests for the rule; the Windows query is not run here).
- Info toasts last 8 s, warnings persist, each with a sound; a blocked app shows "That app isn't allowed on this PC." as a persistent warning toast, 640px wide in the `hud-toast` palette (`hud.spec.ts`, `toast.spec.ts`).
- No blur or translucency (`pnpm check:no-blur`; computed styles asserted).

## Verification

- `cargo test -p arena-proto -p arena-core -p arena-kiosk-core -p arena-api -p arena-kiosk-ui` pass; `cargo test -p arena-kiosk-svc --lib` has the same failures as before this change (SYSTEM ownership on this host); `cargo fmt --check` clean; `cargo clippy` clean except the known unix-only `arena-kiosk-svc/tests/discovery.rs` and `arena-e2e`.
- `pnpm gen:types`, `pnpm check:strings|hex|no-blur`, `pnpm format:check`, `pnpm --filter @arena/kiosk check` pass. `check:generated` reports `SessionScreen.ts` until it is committed.
- Playwright kiosk suite: 139 pass; the 4 `theme.spec.ts` tests fail on the existing `C:\C:\` path bug on Windows. With that path fixed in a throwaway copy all 4 pass, including the HC toast test now run against `/hud`.
- Not run: the benchmark ("no frame-time spike above 1 ms") and any run on a real Windows desktop with a game in front (non-activation, the hotkey, the `SHQueryUserNotificationState` poll and the focus hand-back are compiled but only exercised on reference hardware, Story 6.38).

## Deferred

- Borderless-fullscreen games are not treated as exclusive; the pill stays over them, as it should. Time warnings as toasts, the taskbar flash and the Extend action are Stories 6.19 and 6.21; the panel's Extend options come with 6.21. Staff messages still show in the kiosk window (Story 6.16 moves them); order status on the HUD is Story 6.33.
- The "Counter is reconnecting" state is not shown on the pill until Story 6.36.
