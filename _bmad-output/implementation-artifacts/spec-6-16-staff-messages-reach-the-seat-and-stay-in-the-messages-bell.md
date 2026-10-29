---
title: 'Story 6.16: Staff messages reach the seat and stay in the messages bell'
type: 'feature'
created: '2026-10-29'
status: 'in-review'
baseline_commit: '08417799cd74a01deba8913658f66ce5e5136090'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-5-non-modal-tile-sheet-with-remote-pc-actions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-15-volume-control-and-calling-staff-from-the-top-bar.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Staff can message one PC (Story 3.5), but the kiosk shows it as a banner that is gone once dismissed, there is no way to message every PC, and the player has no place to read earlier messages.

**Approach:** A new intent `device.message{scope: seat|all}` replaces the message action of `device.screen`. The Hub pushes the existing `PcCommand{message}` to the target kiosk(s) and counts the PCs reached. The kiosk service keeps the session's messages (newest first, read flag), raises each as an info HUD toast with its text, and the top bar's `messages-bell` (in the slot 6.15 left, left of Call staff) lists them. Session end clears them.

## Boundaries & Constraints

**Always:**
- Text is 1..=140 chars, trimmed, no control characters, checked in the Hub form and in `apply` validation. `device.message` needs `device.screen` (Owner, Manager, Counter). Scope `seat` names an online PC (`device_id`); scope `all` names none and reaches every online PC that has a live session. The owner command of Story 14.32 will reuse the intent with an owner actor, so the scope and text rules live in `arena-proto`.
- `cmd_id` is the idempotency key, so Retry resends the same key and the kiosk acts once. For scope `all` every kiosk gets the same `cmd_id`; the ack carries its own `device_id`.
- The kiosk shows a message as an info `hud-toast` (8 s, solid, top-right, with the HUD's sound) within 2 s, and keeps it in the bell. The Story 3.5 `pc-message` banner is removed.
- The bell sits left of Call staff, lists this session's messages newest first with the time, shows an `info` dot and count while any is unread, the count is part of its accessible name, and opening marks all read. It is reachable by keyboard (Enter/Space opens, Esc closes and returns focus).
- Session end (the `ends` lock, session cleanup) clears the list, and a new session starts empty, so the next player never sees earlier messages. A message arriving with no session on screen is not kept.
- A seat message not acked in 5 s shows "Failed — retry" in the sheet and Retry resends with the same key (Story 3.5 behaviour, kept).

**Never:**
- Owner-side command plumbing (Epic 8/14), WhatsApp delivery, per-message dismiss, persisting messages across a kiosk service restart, links made clickable, backdrop blur, code comments beyond the codebase's doc-comment style.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Seat message | Counter, online in-session PC, "Closing at 11" | `PcCommand{message}` pushed; HUD info toast with the text; bell count 1; sheet shows the ack | PC offline: refused "the device is offline", nothing pushed |
| All PCs | Counter, 3 PCs in session, 1 free, 1 offline | pushed to the 3; result `delivered: 3`; Floor shows "Sent to 3 PCs" | none in session: `delivered: 0`, "No PCs in session" |
| Bad text | empty, 141 chars, control char | refused `VALIDATION_FAILED` on `text`, nothing pushed | form shows `hub.sheet.message_invalid` |
| Open bell | 2 unread | newest first with times, count cleared, dot gone | N/A |
| Session ends | list holds 3 | list empty, next login sees none | N/A |
| Replay | same key resent | kiosk shows it once (cmd_id dedupe) | N/A |

</intent-contract>

## Code Map

- `crates/arena-proto/src/pc_command.rs` -- `DeviceScreen::validate` (Message now refused there), `PC_MESSAGE_MAX`; add `MessageScope`, `DeviceMessage` + `validate`. `intent.rs` (`IntentKind` ALL/as_str/category/requirement/is_handled, `IntentPayload`, pinned-requirement tests), `permission.rs`, `lib.rs` re-exports, `ui_*`/result type for `delivered`.
- `crates/arena-core/src/apply/pc_command.rs` (`parts`, `load`, `validate`, `mutate` write a `pc.message` device event), `apply/mod.rs:297`, `mutate.rs:349`, `validate.rs:167`, `apply/tests.rs:2843-2930`, `tests/proptest_apply.rs` (add the intent to the generator).
- `crates/arena-api/src/ui_auth.rs` -- `run_intent` target/offline check (~2060), `command` push (~2126, ~2290), `UiIntentResult`; `lan/registry.rs` `push_pc_command` (~309) and a new `push_message_all`; `ui_auth.rs` tests ~3915.
- `crates/arena-kiosk-svc/src/pc.rs` (`apply` Message arm, `Screen.message`, `dismiss`), `app.rs` (`screen_message` ~326, requests, toast forward ~275), `runtime.rs` (`toasts` channel ~86, ~211), `hub_link.rs` (~288); `crates/arena-kiosk-core/src/ipc.rs` (`PcScreen`, new `StaffMessage`).
- `apps/kiosk/src-tauri/src/bridge.rs` + `main.rs` (event names, `ipc-names.json`, new `messages_read` command), `apps/kiosk/src/lib/bridge.ts` (`PcScreen`, `onStaffMessage`), `Hud.svelte` + `hud.ts` (toast with text), `Shell.svelte` (bell slot ~126), `KioskStage.svelte` (banner removal ~240), new `MessagesBell.svelte` + `messages.ts`.
- `apps/hub/src/lib/TileSheet.svelte` (`sendMessage` ~306, `screenJob`), `apps/hub/src/routes/floor/+page.svelte` ("Message all PCs"), `apps/hub/src/lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated types (`pnpm gen:types`).
- Tests: `apps/kiosk/tests/pc-screen.spec.ts`, `toast.spec.ts`, `shell.spec.ts`, new `messages-bell.spec.ts`; `apps/hub/tests/tile-sheet.spec.ts`, `floor.spec.ts`.

## Tasks & Acceptance

**Execution:**
- `crates/arena-proto` -- add `device.message` intent, `MessageScope`, `DeviceMessage` validation, `delivered` on the intent result; remove Message from `DeviceScreen`; `pnpm gen:types`
- `crates/arena-core` -- validate and audit `device.message` (seat device is a PC; no base_rev), add to the proptest generator, update pinned tests
- `crates/arena-api` -- offline refusal for seat, push on fresh and replayed keys, `push_message_all` to online PCs with a live session, `delivered` in the result; tests
- `crates/arena-kiosk-svc`, `arena-kiosk-core` -- keep the session's messages with a read flag, forward each as IPC `staff_message`, `messages_read` request, clear on session end/start, drop the banner field; tests
- `apps/kiosk` (+ `src-tauri`) -- text toast on the HUD, `MessagesBell.svelte` in the bell slot, strings `player.messages.*`, remove the banner; Playwright
- `apps/hub` -- sheet Message uses `device.message`; Floor "Message all PCs" form and "Sent to n PCs" result; strings, audit label; Playwright

**Acceptance Criteria:**
- Given an in-session PC, when staff send ≤140 chars from the tile sheet, then the kiosk shows an info HUD toast with the text and the bell counts it; no banner remains.
- Given Floor, when staff choose "Message all PCs" and send, then every online PC with a live session gets it and the Hub shows how many received it.
- Given the top bar, then the bell sits left of Call staff, unread messages show an info dot and a count in its accessible name, and opening lists them newest first with times and marks them read.
- Given the session ends, then the bell list is empty for the next player.
- Given a seat message with no ack in 5 s, then the sheet shows "Failed — retry" and Retry resends with the same key.
- Given text of 0 or 141 chars, then `apply` refuses it and nothing is pushed.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-core -p arena-kiosk-core -p arena-api` -- expected: pass (kiosk-svc `--lib` has known SYSTEM-ownership failures on this host)
- `cargo fmt --check`, `cargo clippy` -- expected: clean apart from known warnings
- `pnpm gen:types && pnpm check:strings && pnpm check:hex && pnpm check:no-blur && pnpm check:protocol && pnpm format:check` -- expected: pass
- `pnpm --filter @arena/kiosk check`, `pnpm --filter @arena/hub check`, Playwright kiosk and hub specs named above -- expected: pass
