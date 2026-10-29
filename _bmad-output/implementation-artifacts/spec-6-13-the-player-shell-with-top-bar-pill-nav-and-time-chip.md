---
title: 'Story 6.13: The player shell with top bar, pill nav and time chip'
type: 'feature'
created: '2026-10-29'
status: 'review'
baseline_commit: 'cbbeae3'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-10-the-pc-unlocks-to-a-session-screen-when-a-session-starts.md'
---

## Intent

**Problem:** A started session shows the Story 3.10 minimal screen: a device name and an elapsed clock. It has no player, no navigation and no countdown for prepaid time.

**Approach:** Replace it with the player shell. The Hub's `SessionScreen` (pushed on `session.started` and every tick) gains the player's `username`, a guest's `guest_no` and a prepaid `remaining_ms`; the Kiosk service carries them to the app, which counts on its monotonic clock. The shell shows a top bar (32px from the top, 64px tall) with `logo-slot`, `pill-nav`, `time-chip` and the account chip. The existing grace, warning and "Ordered" lines move into the shell's Home body.

## Design

- Proto: `SessionScreen.{username?, guest_no?, remaining_ms?}` (all optional, so older Hubs/kiosks interoperate). `remaining_ms` is `ends_at - (paused_at or now)` for a session with an end time; postpaid has none and counts up.
- Kiosk service: `ScreenSession` keeps `username`, `guest_no` and `ends_at` (monotonic `Instant`); `screen_message` and the Tauri bridge re-base `remaining_ms` like `elapsed_ms`.
- The account chip names the player by **username** (the session row already holds it; the sealed real name is never opened on the push path), or "Guest <n>".
- `time-chip`: status dot and `H:MM:SS` in `hud-numeric` tabular figures; at 10 min or less (prepaid time left, or the branch's warning mark reached) a 2px warning border and a ⚠ glyph replace the dot.
- `pill-nav` (Home, Games, Apps): one active item (inverted); arrows move focus (wrapping), Enter or click selects; the selection lives for the session and resets to Home when it ends. Shop joins in Story 6.30.
- Every tab shows "No games here yet — ask at the counter." until its content stories land.
- Lock screen: the existing solid `bg` cover gains the `logo-slot` (branch logo, else the Arena 360 mark); the reason lines (including forced end) already existed.

## Tasks & Acceptance

- [x] `arena-proto` / `arena-core` / `arena-kiosk-core` / `arena-kiosk-svc` / Tauri bridge: carry `username`, `guest_no`, `remaining_ms`; generated types
- [x] Kiosk app: `Shell.svelte`, `shell.ts`, `+page.svelte` wiring, bridge parsing, lock-screen logo; strings `player.shell.*`
- [x] Tests: Rust assertions in `session_expire` / `session`; Playwright `shell.spec.ts` (shell, bar layout, chip up/down, low state, nav keys and reset, lock cover, already-running session)

**Acceptance (all covered by `shell.spec.ts` unless noted):**

- `session.started` opens the shell for the profiled player or "Guest <n>".
- Top bar: 32px down, 64px tall, logo left, nav centred, chip and account at the right.
- Chip counts down (prepaid) or up (postpaid) and warns at 10 min or less.
- Pill nav: one active item, arrows + Enter, Home at start, persists, resets at session end.
- A lock reason covers the shell with a solid screen, logo slot and its catalogue line, and lifts when cleared.

## Verification

- `cargo test -p arena-proto -p arena-core -p arena-kiosk-core -p arena-api -p arena-kiosk-ui` pass; `cargo test -p arena-kiosk-svc --lib` has the same 14 failures as before this change (SYSTEM ownership on this host); `cargo fmt --check` clean; `cargo clippy` clean except the known unix-only `arena-kiosk-svc/tests/discovery.rs` and `arena-e2e`.
- `pnpm gen:types`, `pnpm check:strings|hex|no-blur`, `pnpm format:check`, `pnpm --filter @arena/kiosk check` pass. `check:generated` reports `SessionScreen.ts` until it is committed. `check:tokens-used` fails on existing hub tokens only.
- Playwright kiosk suite: 122 pass; the 4 `theme.spec.ts` tests fail on a `C:\C:\` path bug on Windows, as before.
- Not run: the "interactive within 2 s of the tap" run on reference hardware.

## Deferred

- The shell tabs have no content yet (Games 6.23, Apps 6.28, Home 6.29); the account chip is not a menu until 6.17; the chip does not yet react to a Hub that is unreachable (6.36).
