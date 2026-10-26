---
title: 'Story 3.5: Non-modal tile sheet with remote PC actions'
type: 'feature'
created: '2026-10-26'
status: 'done'
baseline_commit: '53512afb97855e47710f11919415407d9e1e1d64'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-4-collapsible-zones-with-per-state-counts.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-20-kiosk-admin-exit-with-manager-pin-offline-verifier-and-hub-r.md'
warnings:
  - oversized
deferred: []
---

<intent-contract>

## Intent

**Problem:** Staff can only act on a PC through one per-tile Remote unlock button. They cannot lock, unlock, message, restart or shut down a PC, nor handle a maintenance or unassigned device, from the live floor.

**Approach:** Tapping a tile (or Enter) opens a non-modal docked `tile-sheet` listing only the actions valid for that device's state. Two new Hub intents, `device.screen` (lock/unlock/message, held by Counter) and `device.power` (restart/shutdown, `device.remote` or manager PIN in place), are pushed to the paired kiosk as one new `HubEvent`; the kiosk executes it and answers one new ack that the Hub relays to the UI on `/ui/events`.

## Boundaries & Constraints

**Always:**
- Sheet: `role="complementary"`, no backdrop, floor stays live and operable; at ≥1600px the floor reflows to 4 columns while open; focus moves into the sheet on open; Esc closes and returns focus to the opening tile; Enter or tap on a tile opens it, another tile swaps the sheet. Works by keyboard alone. Use the existing `tile-sheet` design tokens.
- Sheet shows device name, group, zone, group rate, status, and only valid actions: PC free/in-session -> Lock, Unlock, Message, Restart, Shut down, Admin unlock (maintenance); maintenance -> Return to service (`device.maintenance` on:false) and Admin unlock; unassigned -> Assign to group (`device.assign`, group picker limited by `fits`); offline -> last seen only. Non-PC kinds show details only. Every action is one tap from the open sheet; Lock and Restart/Shut down are secondary buttons placed >=24px from primary actions.
- New permission `device.screen` held by Owner, Manager and Counter; `device.screen` intent requires it. `device.power` requires `device.remote`; a member lacking it gets `inline-pin` inside the sheet and the existing `elevate()` ticket flow (as Story 1.20); the audit records both actors. Return to service and Assign to group need `device.manage`; Counter uses the same inline-pin elevation. Wrong PIN or PIN of a member without the permission applies nothing and clears the cells.
- Admin unlock keeps its Story 1.20 behaviour (`device.admin_unlock`, 15-minute exit, audit mode remote) but is launched from the sheet; the tile-level Remote unlock button is removed and Unassigned/placeholder entries open the sheet.
- Offline devices are refused at once for screen/power/admin unlock ("the device is offline"), nothing pushed. Message text is 1..=140 chars, validated in Hub UI and in `apply` validation.
- One `HubEvent::PcCommand{cmd_id, action, text?}` and one paired-kiosk frame `kiosk.pc_ack{cmd_id}` (validated against the paired device and a pending command, not a DB row), relayed as `device.pc_ack{cmd_id, device_id}`. `cmd_id` derives from the intent idempotency key. The kiosk is idempotent per `cmd_id`: repeated lock/unlock harmless, repeated message not re-shown, repeated power not re-run. Retry resends the same intent with the same key; the Hub must push again on a dedupe replay.
- Result within 5 s: sheet shows it inline; otherwise the tile's secondary line shows "Failed — retry" (`hub.floor.remote_unlock_failed`) and the sheet's Retry resends with the same key.
- Kiosk: lock covers the screen with "Locked by staff"; unlock removes that cover; message shows a banner with the text until dismissed; restart/shutdown acked first, then Windows restart/shutdown via `cfg(windows)` code in `arena-kiosk-svc`; non-Windows builds refuse without crashing. Lock state is service-owned and survives UI reload.
- All new strings in `packages/ui/src/messages/en.json`; regenerate types with `pnpm gen:types`; keep existing `data-testid`s that still apply (`unlock-status`, `unlock-needs-approval` move into the sheet).

**Never:**
- Session-aware refusal (`PC_BUSY`) -- Story 3.9. Alert sounds/badges (3.25). Changes to billing or session tables.
- Polling for results; modal dialogs or backdrops for the sheet.
- Code comments (user rule).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open | Enter/tap tile | sheet docks right, floor 4 cols at 1920px, focus in sheet | N/A |
| Close | Esc in sheet | sheet closes, focus on tile | N/A |
| Lock | Counter, online PC | `device.screen` lock pushed, kiosk shows "Locked by staff", ack shown inline | no ack in 5 s -> tile "Failed — retry" |
| Unlock | Counter, locked PC | cover removed, ack inline | same |
| Message | 140 chars | kiosk banner until dismissed | 141 chars or empty rejected in UI and `apply` |
| Restart, Counter | no `device.remote` | inline-pin; correct manager PIN -> kiosk restarts Windows; audit both actors | wrong PIN: nothing applied, cells cleared |
| Retry | after Failed | same idempotency key resent and pushed again; kiosk acts once | N/A |
| Offline PC | status offline | sheet shows last seen, no actions | direct intent refused |
| Maintenance | status maintenance | Return to service, Admin unlock only | PIN flow as above |
| Unassigned | no group | Assign to group picker | wrong-kind group refused |

</intent-contract>

## Code Map

- `apps/hub/src/routes/floor/+page.svelte` -- tile snippet, `activeId`, `unlock()`/`approve()` (Story 1.20) to move into the sheet, `order`, grid CSS (4 cols when sheet open).
- `apps/hub/src/lib/auth.ts` -- `submitIntent`, `elevate`, `hasPermission`, `newIdempotencyKey`; `lib/admin-unlock.ts` ack waiter to generalise to `cmd_id`; `lib/events.ts` + `routes/+layout.svelte:~243` event wiring.
- `apps/hub/src/lib/MoreSheet.svelte` -- modal sheet, NOT reusable; new `TileSheet.svelte` goes beside it. `packages/ui/src/components/InlinePin.svelte`, `FloorTile.svelte`.
- `apps/hub/src/routes/settings/devices/+page.svelte` (`run()` for `device.maintenance`), `settings/layout/+page.svelte:~204` (`device.assign`, `fits`) -- elevation and payload patterns to reuse.
- `crates/arena-proto/src/intent.rs` (`IntentKind` ALL/as_str/category/requirement/reasons/is_handled, payloads, pinned-requirement tests), `permission.rs`, `frame.rs` (`FrameBody`, `FRAME_KINDS`, `HubEvent`), `admin.rs` (ack types), `lib.rs`.
- `crates/arena-core/src/rbac.rs` (`COUNTER_PERMS`, count tests), `apply/mod.rs`/`validate.rs`/`mutate.rs`/`admin_exit.rs`, `apply/seed_defaults.rs` (backfills role permissions), `apply/tests.rs:117`, `tests/proptest_apply.rs:441`.
- `crates/arena-api/src/ui_auth.rs` (`/ui/intent` offline refusal ~1900, elevation ~1925, Counter-perms test ~2359), `lan/registry.rs` (`push_paired`, `push_admin_unlock`, ack broadcast), `lan/session.rs` (kiosk ack frames ~579/758), `ui_events.rs` (ack relay ~353).
- `crates/arena-kiosk-svc/src/hub_link.rs` (HubEvent match ~255), `admin_exit.rs::grant_remote` (ack pattern), `lockdown/windows.rs`; `crates/arena-kiosk-core/src/ipc.rs` (`IpcMsg`), `apps/kiosk/src-tauri/src/bridge.rs`, `apps/kiosk/src/lib/bridge.ts`, `routes/+page.svelte`, `+layout.svelte` (toast region reusable for banner).
- `apps/hub/tests/floor-fixture.ts`, `floor.spec.ts`, `remote-unlock.spec.ts` (retarget to the sheet), `apps/kiosk/tests/*.spec.ts`; `apps/hub/src/lib/audit/labels.ts` + `en.json` audit keys.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto`, `crates/arena-core`, `packages/ui/src/generated` -- add `device.screen` permission (Counter), `device.screen` and `device.power` intent kinds and payloads, `HubEvent::PcCommand`, `kiosk.pc_ack` frame, `AdminUnlockAck`-style UI event; fix pinned counts/tests; `pnpm gen:types`
- [x] `crates/arena-core/src/apply` -- validate (device exists, PC kind, text 1..=140, no `base_rev`) and audit both new kinds; permissions via seed backfill
- [x] `crates/arena-api` -- `/ui/intent` offline refusal and push on fresh and replayed intents; session handling of `kiosk.pc_ack`; relay `device.pc_ack`; unit and integration tests
- [x] `crates/arena-kiosk-svc`, `arena-kiosk-core`, `apps/kiosk` -- handle `PcCommand`: lock cover, unlock, message banner, restart/shutdown (Windows), per-`cmd_id` idempotence, ack; Playwright for cover and banner
- [x] `apps/hub` -- `TileSheet.svelte`, floor wiring (open/close/focus/4-column reflow), per-state actions, inline-pin, 5 s result/"Failed — retry"/Retry, remove tile-level button, `events.ts`, audit labels, `en.json`
- [x] `apps/hub/tests` -- retarget `remote-unlock.spec.ts`; new `tile-sheet.spec.ts` (open/Esc/focus, reflow, per-state actions, lock/message, PIN flow, retry, keyboard-only, axe); update `floor.spec.ts`

**Acceptance Criteria:**
- Given Floor, when staff tap a tile or press Enter, then a non-modal sheet docks right, floor reflows to 4 columns at 1920px and stays live, focus enters the sheet, and Esc closes it returning focus to the tile.
- Given a sheet, then it shows name, group, zone, group rate, status and only the state-valid actions.
- Given a Counter on an online PC, when they choose Lock, Unlock or Message (<=140 chars), then the kiosk covers "Locked by staff", removes the cover, or shows the banner until dismissed, and confirms to the Hub.
- Given Restart or Shut down by a role without `device.remote`, then inline-pin opens; with elevation the kiosk restarts/shuts Windows down and the audit records both actors.
- Given Admin unlock and a manager PIN, then `device.admin_unlock` applies as in Story 1.20; a wrong PIN applies nothing and clears the cells.
- Given any remote action, when the kiosk confirms within 5 s the sheet shows the result; otherwise the tile shows "Failed — retry" and Retry resends with the same idempotency key.
- Given the sheet layout, then each action is one tap, destructive actions are secondary and >=24px from primary ones, and keyboard alone works.

## Spec Change Log

## Review Triage Log

## Design Notes

`cmd_id` = idempotency key keeps Retry exact: the API layer pushes on every accepted submission of that key (apply returns the stored result on replay), the kiosk dedupes on `cmd_id`. One generic event/ack pair avoids four frame kinds.

## Verification

**Commands:**
- `pnpm run gen:types && pnpm run check:generated && pnpm run check:protocol && pnpm run check:strings` -- expected: pass
- `cargo fmt --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace` -- expected: pass
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell` -- expected: pass
- `pnpm --filter @arena/kiosk check && pnpm --filter @arena/kiosk test` -- expected: pass
