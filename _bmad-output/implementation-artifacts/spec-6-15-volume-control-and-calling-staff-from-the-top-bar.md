---
title: 'Story 6.15: Volume control and calling staff from the top bar'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '4839ef8'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-13-the-player-shell-with-top-bar-pill-nav-and-time-chip.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-25-hub-alerts-with-sound-and-a-persistent-badge.md'
---

## Intent

**Problem:** The player shell's top bar (Story 6.13) has no way to change the sound or ask for help; a player has to shout across the room.

**Approach:** Two controls join the right cluster, in the order volume, (room for the messages bell, Story 6.16), Call staff, time chip, account chip. The speaker opens a slider that sets Windows system volume through the Kiosk service. Call staff sends a new paired-only frame, `device.call_staff`; the Hub raises a `call_staff` alert for that PC with the Story 3.25 machinery, so the tile shows a "CALL" `alert-badge` and the Hub sounds.

## Design

- Proto: `AlertKind::CallStaff` (`call_staff`, tile tag `call`; the `AlertBadge` already had the CALL tag) and the unit frame `device.call_staff` (`FRAME_KINDS` is 25 long). No new `IntentKind`: the Hub raises the alert as a system `alert.raise`.
- Hub (`arena-api/src/lan/call_staff.rs`): `device.call_staff` is paired-only (`FORBIDDEN` otherwise). A follower answers `NOT_PRIMARY`. The Hub raises the alert as the LAN system actor; a call already open for the PC is `STALE` from `alert.raise`, treated as success, so there is never a second alert and the kiosk still shows "Staff notified.". Staff open the tile sheet or the list row and the Story 3.25 acknowledge clears the badge (`call_staff` is not in `STAYS_OPEN`).
- Hub UI: `KIND_LABELS` / `KIND_TAGS` gain `call_staff` ("Player is calling staff", tile tag CALL).
- Kiosk service: `volume.rs` keeps the cleanup host (attached in `Runtime::cleanup_with`) and the last level set. Request `volume` with no body reads the level (last set, else the branch default `kiosk.volume_default`, else 50); `volume{percent}` sets it through `CleanupHost::set_volume` (Windows: the helper running in the player's session) and refuses more than 100. A new session (`session.started` with no session already on screen) resets the volume to the branch default, whatever the previous player left; the session-end cleanup already did too (Story 6.11). Request `call_staff` forwards `device.call_staff`; no live Hub connection is `HUB_SLOW` and nothing is queued.
- Tauri shell: commands `volume_get`, `volume_set(percent)` and `call_staff`. `call_staff` emits the HUD toast itself: `player.call_staff.sent` ("Staff notified.") when the Hub took the call, `player.hub.reconnecting_retry` ("Counter is reconnecting — try again shortly.") otherwise. Both are info toasts on the HUD (8 s).
- Kiosk app: `VolumeControl.svelte` (speaker button, 44px; popover with a native range input, step 5 so the arrow keys move 5%; `border-strong` track, `text` fill, 44px thumb hit area; Esc or a click away closes; each change is sent at once, one request in flight with the newest value waiting), `volume.ts`, and in `Shell.svelte` a Call staff pill ("?" mark and label, `surface` with a `border-strong` edge, 44px high) whose label reads "Staff notified" for 8 s after the Hub took the call. `bridge.ts`: `volumeLevel`, `setVolume`, `callStaff`, two new toast keys.

## Tasks & Acceptance

- [x] `arena-proto`: `AlertKind::CallStaff`, `device.call_staff` frame; generated types
- [x] `arena-api`: `device.call_staff` handler (paired only, idempotent raise); tests
- [x] `arena-kiosk-svc`: `volume` and `call_staff` requests, volume reset at session start; tests
- [x] Tauri shell: `volume_get`, `volume_set`, `call_staff` commands, toast, `ipc-names.json`; test
- [x] Kiosk app: `VolumeControl.svelte`, Call staff button, bell slot, strings `player.volume.*` / `player.call_staff.*`; Playwright `volume-staff.spec.ts`, `toast.spec.ts`
- [x] Hub: label and tile tag for `call_staff`; Playwright `alerts.spec.ts`

**Acceptance:**

- The right cluster reads volume, Call staff, time chip, account chip, with the bell's place free left of Call staff (`volume-staff.spec.ts`).
- The slider sets system volume at once, arrow keys step 5%, the track/fill/thumb use the tokens with a 44px hit area (`volume-staff.spec.ts`; the service side in `cleanup/tests.rs`).
- A new session starts at the branch default volume (`a_new_session_starts_at_the_branch_default_whatever_the_last_player_set`).
- Call staff raises one CALL alert for the PC and answers "Staff notified." every time, with no second alert (`lan.rs`: `call_staff_raises_one_alert_for_the_device_and_answers_ack_each_time`); the tile shows the CALL tag and the Hub sounds (`alerts.spec.ts`); acknowledging by opening the sheet is the Story 3.25 path.
- An unreachable Hub sends nothing and the toast reads "Counter is reconnecting — try again shortly." (`call_staff_toast`, `toast.spec.ts`, `HUB_SLOW` from the service).
- A non-paired class sending `device.call_staff` is `FORBIDDEN`; a follower answers `NOT_PRIMARY` (`lan.rs`).

## Verification

- `cargo test -p arena-proto -p arena-core -p arena-kiosk-core -p arena-api -p arena-kiosk-ui` pass; `cargo test -p arena-kiosk-svc --lib` has the same 14 failures as before this change (SYSTEM ownership on this host), the new volume tests pass; `cargo fmt --check` clean; `cargo clippy` clean except the known `arena-kiosk-svc/tests/discovery.rs` warnings.
- `pnpm gen:types`, `pnpm check:strings|hex|no-blur|protocol`, `pnpm format:check`, `pnpm --filter @arena/kiosk check`, `pnpm --filter @arena/hub check` pass. `check:tokens-used` fails on the existing hub `--ad-reserved` tokens only; `check:generated` reports the changed generated types until committed.
- Playwright kiosk suite: 146 pass; the 4 `theme.spec.ts` tests fail on the existing `C:\C:\` path bug on Windows. Hub `alerts.spec.ts`: 22 pass.
- Not run: the real Windows mixer (the helper path is Story 6.11's, compiled but exercised on reference hardware in Story 6.38), the "CALL tile and sound within 2 s" timing on a real LAN, and `arena-kiosk-svc/tests/runtime.rs` (`cfg(unix)`).

## Deferred

- The slider shows the last level the service set, not a reading of the Windows mixer; a hardware volume key moves the mixer but not the slider until the next set or session. Reading the mixer needs a second helper call.
- The messages bell (Story 6.16) fills the slot left of Call staff; the "Reconnecting…" indicator (Story 6.36) will join the cluster at the left.
- The Call staff button is not disabled while the Hub is unreachable; the press answers with the reconnecting toast.
