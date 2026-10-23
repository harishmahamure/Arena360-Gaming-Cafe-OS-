---
title: 'Story 1.14: Hub idle lock with PIN unlock in place'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '066c3867c79bf89a66239eed33e1d642a9320f92'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-13-inline-manager-pin-elevation.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A signed-in Hub left unattended can be used by anyone at the counter. Signing out to protect it loses the staff member's place.

**Approach:** The Hub UI tracks input idleness against the branch `idle_lock_minutes` setting. When it expires, an overlay lock screen covers the UI without unmounting it, so the screen, scroll position and unsaved fields survive. The overlay unlocks with a PIN through the shared `InlinePin`. The Hub verifies the PIN, which re-binds the session to whichever staff member's PIN matched. Lock and unlock are logged without any PIN material.

## Boundaries & Constraints

**Always:**
- **Setting:** `idle_lock_minutes` (seeded with 5 in 1.7, validated 1..=30 in 1.8). The Settings page shows its allowed range inline ("1–30 minutes") and edits it through `settings.set`, with the 1.13 manager elevation for a Counter.
  - An out-of-range value fails with `VALIDATION_FAILED value` and shows inline.
  - All open Hub UIs pick up a new value through `settings.changed` (1.12 SSE). It applies from the next idle period, meaning the current countdown is not shortened.
  - `HubSnapshot.locale`'s sibling `ui: {idle_lock_minutes}` carries the value.
- **Idle tracking (Hub UI):**
  - `keydown`, `pointerdown`, `pointermove`, `wheel` and `touchstart` on `window` reset a timer, throttled to once per second.
  - When `now - last_input >= idle_lock_minutes`, the UI locks. It uses a single `setTimeout`, rescheduled on input.
  - Only signed-in sessions lock; the sign-in screen never locks.
- **Lock overlay:**
  - A full-viewport `role="dialog"` with `aria-modal="true"` and a catalogue `h1` ("Locked"). It shows the signed-in staff display name and an `InlinePin`.
  - Everything behind it is `inert`, set via the `inert` attribute on the app root, and `aria-hidden`.
  - The underlying route component stays mounted, so its state, scroll position and unsaved input persist.
  - Focus moves to the PIN input on lock.
  - It has a Sign out button.
- **Unlock API (loopback, bearer):**
  - `POST /ui/lock` marks the session locked. Locked sessions reject `/ui/intent` and `/ui/elevate` with a new `SESSION_LOCKED` code.
  - `POST /ui/unlock {pin}` first checks the PIN against the session's current staff. If that fails, it checks the other active staff, at most 50.
    - **Same staff matches:** unlock; the session is unchanged.
    - **Another staff matches:** unlock, and the session's staff becomes them; the response carries their staff block.
    - **No match:** `PIN_REJECTED`, with the generic message.
  - 5 consecutive failures pause unlock for 60 s with `AUTH_LOCKED` and `retry_after_s`. Sign-out stays available.
  - This reuses the 1.13 argon2 PIN helpers and the injectable-clock throttle.
- **After unlock:**
  - The overlay closes, `inert` is removed, and focus returns to the element focused before the lock. The scroll position is unchanged.
  - If the staff member changed, the header updates to them, and every later intent carries the new actor, because the session actor is server-side.
  - The UI then refetches the snapshot so any state that changed while locked is shown.
  - The service never stops processing while the UI is locked.
- **Logging:** the Hub logs `ui.locked {staff_id, at}` and `ui.unlocked {staff_id, previous_staff_id?, at}` as tracing info lines. They never include the PIN or its length.
- **Keyboard:** the lock and unlock flow works with the keyboard only. Digits, Enter and Backspace go to the PIN; Tab reaches Sign out; Esc clears the PIN but does not unlock.

**Never:**
- Locking the Hub service, or pausing LAN processing.
- Storing the PIN.
- An OS-level screen lock.
- A different timeout per window.
- Shortening the countdown already in progress when the setting changes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Idle | 5 min with no input (fake clock in tests) | the overlay shows the staff name; the background is `inert` | N/A |
| Activity | input at 4:59 | the timer resets; no lock | N/A |
| Same PIN | the counter's own PIN | unlocks; same route, scroll and unsaved text | N/A |
| Other PIN | the manager's PIN while the Counter is locked | unlocks; the header shows Manager; the next intent's `actor_id` is the manager's | N/A |
| Wrong ×5 | 5 wrong PINs | `AUTH_LOCKED`; a 60 s countdown; Sign out works | N/A |
| Locked intent | an intent POST while locked | `SESSION_LOCKED` | N/A |
| Setting change | a manager sets 10 while another window counts down 5 | that window locks at its current period, then uses 10 for the next period | N/A |
| Out of range | 0 or 31 | `VALIDATION_FAILED value`, shown inline | N/A |
| Events while locked | a device is revoked while locked | after unlock, the floor no longer lists the device | N/A |
| Log | lock and unlock | log lines carry the staff ids and contain no PIN | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-api/src/ui_auth.rs` (1.11, 1.13) -- sessions, lockout and the elevation helpers. Add lock/unlock, `SESSION_LOCKED` and the actor re-bind.
- `crates/arena-proto/src/{error.rs,snapshot.rs}` -- the new code and the `ui` block. Then `pnpm gen:types`.
- `packages/ui/src/components/InlinePin.svelte` (1.13) -- reuse it.
- `apps/hub/src/routes/+layout.svelte` (1.10–1.12) -- the gate, the header, the SSE store. Add the idle tracker, the overlay and `inert`.
- `apps/hub/src/routes/settings/**` -- add the idle timeout field with its inline range.

## Tasks & Acceptance

**Execution:**
- [x] Rust: lock/unlock endpoints, the throttle, the re-bind, `SESSION_LOCKED`, the log lines and the snapshot `ui` block, with a test for every API matrix row.
- [x] `apps/hub/src/lib/idle.js` (+ unit test with a fake clock), `apps/hub/src/lib/LockOverlay.svelte`, layout wiring, and the settings field.
- [x] `apps/hub/tests/lock.spec.ts`: Playwright with `page.clock` for idle, state preservation (scroll and typed text), PIN switch, throttle countdown, keyboard-only, axe on the overlay.
- [x] Catalogue keys, `README.md`.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked`, `pnpm check` and the hub Playwright suite, then all pass with every matrix row covered.

## Implementation Notes

- **Wire.** `SESSION_LOCKED` is 423. `GET /ui/me` answers `UiMe` (the staff block flattened plus `locked`), so a reload of a locked session shows the lock again. `POST /ui/unlock` answers `UnlockOk {staff, switched}`. `HubSnapshot.ui` and `SettingsChangedEvent.ui` are `SnapshotUi {idle_lock_minutes}` (default 5 when the setting is missing or unreadable); the UI tolerates both being absent.
- **Lock.** `/ui/lock` sets the session's `locked` flag and drops its tickets; `ui.locked` is logged only on the transition. Only `/ui/intent` and `/ui/elevate` are refused while locked (`/ui/me`, `/ui/settings`, `/ui/events`, `/ui/elevate/drop`, sign-out keep working).
- **Unlock.** One SQL query lists active staff with a PIN, the session's member first (`ORDER BY staff_id <> ?1, staff_id LIMIT 50`); the PIN is checked one by one with argon2id under the shared 2-at-once semaphore. The PIN is checked even when the session is not locked. The throttle is a separate exact-key `Lockout` keyed by the locked session's staff id; a check that could not run releases its reservation. A matched member deactivated before the re-bind counts as a failure. A switch drops the session's tickets and keeps its token and `created_at`; the 5-sessions-per-member cap is not re-applied on a re-bind.
- **UI.** `idle.js` (one throttled `setTimeout`, the length read at period start), `ui-settings.svelte.ts` (the store), `LockOverlay.svelte`; the layout puts `inert` + `aria-hidden` on `.shell`, disables the Ctrl+ shortcuts and closes the More sheet while locked, reloads the snapshot (and session) before uncovering, and restores focus with `preventScroll`. `InlinePin` gained `label`, `submitLabel` and `cancellable` props (defaults unchanged). The idle lock field sits on Settings → Branch (number input, range note, server `VALIDATION_FAILED` shown inline).
- **Tests.** The idle unit tests live in `apps/hub/tests/idle.unit.spec.ts` (Playwright runner, no page; the repo has no other JS unit runner and no `@types/node`). The `lock.spec.ts` SSE stub only reconnects in the cross-window test, because a reconnect triggers a snapshot reload whose fetch a `page.clock` jump would abort.

- **Review round 1.** `/ui/unlock` refuses an unlocked session (409 `VALIDATION_FAILED` on `session`, before any PIN check) and uses its own rejection text ("that PIN can't unlock this screen"). The unlock throttle (`UnlockThrottle`) counts per staff member and per token, and pauses 60 s doubling to 15 min until a success, logging a warning per pause. Staff and `locked` are read under one lock; a staff refresh never reverts a re-bound session; a ticket is issued only if the session is still unlocked and acting as the requester (checked under the tickets lock), and redeemed only by that requester. `session.locked` / `session.unlocked` go to the streams of the same token; a failed UI-settings read sends the defaults. `/ui/me` adds `has_pin`. UI: the lock call is retried with back-off and awaited by the unlock (a `not_locked` answer re-locks once and retries); a sessionStorage flag survives a reload; `SESSION_LOCKED` from any call locks; the overlay also covers the unreachable state and tracking stops while gated; an unlock reloads snapshot and session and announces a switch; a member without a PIN is told to sign out. `idle.js` checks the wall clock on `visibilitychange`/`focus`/`pageshow` and restarts on a backwards clock. The idle field moved to its own **Screen lock** section (`/settings/screen-lock`) with client-side 1–30 validation; a bad `ui` block falls back to 5.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | `/ui/unlock` works on an unlocked session, so it acts as a user switch that bypasses audited elevation (blind, edge) | high | No `s.locked` check. | patch |
| 2 | Unlock guesses are checked against 50 staff with a flat 60 s pause | medium | About 2.5% per minute to take over some session. | patch (escalating pause) |
| 3 | UI never handles `SESSION_LOCKED` (blind, edge) | medium | A 423 shows a generic failure with no way to recover. | patch |
| 4 | Fire-and-forget `lockSession` can arrive after the unlock, or fail silently (blind, edge) | medium | Hub and UI disagree about the lock. | patch |
| 5 | Idle timer ignores sleep/suspend; a backward clock jump stops it (blind, edge) | medium | Single `setTimeout`. | patch |
| 6 | Lock and session read in two critical sections; `/ui/me` race (blind, edge) | low | One intent can slip through after a lock. | patch |
| 7 | Unlock timing hints at which staff PIN matched | low | Bounded by #1 and #2. | reject |
| 8 | Unlock ignores the returned staff; header can stay stale; the switch isn't announced | low | No session invalidate. | patch |
| 9 | Locked plus gated (Hub unreachable) hides the overlay and exposes controls (blind, edge) | medium | Overlay is conditional on `!gated`. | patch |
| 10 | Missing tests: unlock pause across sessions, switched tickets refused, deactivated mid-match, drop/settings while locked, stub keyed per staff, live `ui` via `settings.changed`, own-member-first ordering (blind, verify-gap) | low | Pre-verified gaps. | patch |
| 11 | No client validation for idle minutes; a bad `ui` block rejects the whole snapshot or event | low | Should fall back to the default. | patch |
| 12 | Doc and copy drift (elevation wording on the lock screen, the idle field under Branch copy, duplicated key constants) | low | Direct fix. | patch |
| 13 | A concurrent `session_for` write-back reverts a switched session to the old member | medium | The actor attribution would be wrong. | patch |
| 14 | An elevation ticket can be inserted after the lock drops tickets | low | Re-check under the lock. | patch |
| 15 | No cross-window lock propagation, despite the comment saying there is | low | Push a `session.locked` event. | patch |
| 16 | Own member with no PIN can never unlock | low | Show a sign-out hint. | patch |
| 17 | A failed `ui()` read suppresses `settings.changed` entirely | low | Fall back to the default. | patch |
| 18 | Staff beyond 50 PIN holders can't unlock | low | Documented cap, same as 1.13. | reject |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub build && pnpm --filter @arena/hub exec playwright test` -- expected: pass
