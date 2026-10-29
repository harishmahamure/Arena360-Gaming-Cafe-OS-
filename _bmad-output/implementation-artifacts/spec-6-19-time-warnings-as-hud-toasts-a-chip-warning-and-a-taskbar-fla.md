---
title: 'Story 6.19: Time warnings as HUD toasts, a chip warning, and a taskbar flash in fullscreen games'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: '4f7942c8e2eaae1925a1d3b7abc107124b70b40b'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-14-the-overlay-hud-shows-time-and-balance-without-stealing-focus.md'
warnings: []
deferred:
  - 'Postpaid warnings counting to branch closing time: the Hub timer (arena-core session_timer) warns prepaid sessions only and sends no closing time, so a postpaid toast has the title but no end-time line until the Hub does.'
  - 'Extend as a button on the toast and the Ctrl+Shift+E key opening extend-options: Story 6.21 (the key caps show now; the key still opens the HUD panel).'
  - 'Configurable marks: Story 6.20. The kiosk marks stay 10, 5, 1 (SESSION_WARNING_MARKS_MIN) until then.'
---

<intent-contract>

## Intent

**Problem:** The Hub's `session.warning` reaches the kiosk service and the shell's chip, but the player gets no toast, no sound and, in a fullscreen game, no cue at all; with the Hub down no warning is raised.

**Approach:** The HUD page raises one persistent warning toast per new mark in `PcScreen.warning_min`; the kiosk service also passes the marks on its monotonic clock (the same `warn` the Hub's frame calls, so the two never double up); the Rust shell flashes the taskbar and beeps at the 5 and 1 marks while a fullscreen-exclusive game hides the HUD. The accepted deaf/hard-of-hearing limitation is recorded in `docs/kiosk-accessibility.md`.

## Boundaries & Constraints

**Always:** a warning toast is solid, never takes focus, stays until dismissed or time-up (session end, lock, or an extension that clears the mark), and is replaced by a newer warning; the chime is the existing two-note warning chime; a HUD that starts mid-session records the current mark without raising it.

**Never:** a new frame, intent or permission; an overlay drawn over a fullscreen-exclusive game; focus taken by the toast.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Mark reached | `warning_min` 10/5/1, prepaid | toast "N min left. Extend?", "Time's up at <time>. You get a G-min grace after that.", Dismiss, Ctrl+Shift+E caps |
| No grace | `grace_ms` 0 | "Time's up at <time>." |
| Postpaid | no `remaining_ms` | title and caps only |
| Newer mark | 10 then 5 | one toast, the 5 |
| Dismissed | same mark on next tick | not raised again |
| Cleared | mark null, lock, session end | toast removed |
| Hub down | mark passes on the kiosk clock | service `warn` raises it locally |
| Fullscreen-exclusive | new 5 or 1 mark | taskbar flash + system beep; nothing drawn |

</intent-contract>

## Code Map

- `apps/kiosk/src/lib/hud.ts` -- `raiseWarning`, `dropWarning`, `graceMinutes`, `WARNING_KEY`, `EXTEND_KEYS`.
- `apps/kiosk/src/lib/Hud.svelte` -- `followWarning` on each `pc-screen`; toast detail line and key caps.
- `packages/ui/src/messages/en.json` -- `player.warning.*` (replaces the shadowed reserved `player.hud.time_left` copy).
- `crates/arena-kiosk-svc/src/session.rs` -- `next_mark` and the mark timer in `run`.
- `crates/arena-kiosk-core/src/hud.rs` -- `warning_flash`; `apps/kiosk/src-tauri/src/{hud,main}.rs` -- `note_warning`, `flash_taskbar`; workspace `windows` gains `Win32_System_Diagnostics_Debug` (`MessageBeep`).
- The chip's ⚠ at 10 min or less is the existing Story 6.13 `isLow`.
- `docs/kiosk-accessibility.md` -- the accepted fullscreen-exclusive limitation.

## Tasks & Acceptance

- [x] Warning toast on the HUD with end time, grace line, Dismiss, key caps; replace, persist, clear
- [x] Local marks on the kiosk monotonic clock in the service (Hub unreachable)
- [x] Taskbar flash and sound at 5 and 1 over a fullscreen-exclusive game
- [x] Chip warning at 10 min or less (existing Story 6.13 behaviour, verified)
- [x] Accessibility limitation recorded in the repo docs
- [x] Tests: Playwright (`hud.spec.ts`), Rust (`session.rs`, `cleanup/tests.rs`, `hud.rs` core and shell)

## Spec Change Log

- The Help Centre source does not exist yet (Epic 11); the note lives in `docs/kiosk-accessibility.md` for it to draw from.

## Verification

- `pnpm check:strings && pnpm check:hex && pnpm check:no-blur && pnpm format:check && pnpm check:pins` -- pass
- `pnpm --filter @arena/kiosk check` -- 0 errors; `pnpm --filter @arena/kiosk test:ui` -- all pass but the 4 existing `theme.spec.ts` (`C:\C:\` path bug)
- `cargo test -p arena-kiosk-core -p arena-kiosk-ui`, `cargo test -p arena-kiosk-svc --lib` -- new tests pass; 14 existing lockdown/pipe/DPAPI/config tests fail on this machine (Windows privileges), untouched by this story
- `cargo clippy ... --lib --bins` clean; `tests/discovery.rs` has existing unused-import warnings

Not run: the real Tauri shell, a real fullscreen-exclusive game (taskbar flash and beep are Windows calls with no test double), a real Hub round trip.
