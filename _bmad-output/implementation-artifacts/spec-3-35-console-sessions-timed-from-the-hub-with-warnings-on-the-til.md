---
title: 'Story 3.35: Console Sessions Timed from the Hub, with Warnings on the Tile'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_revision: '14583c0'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-34-forced-end-with-manager-pin-and-reason.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A console (PS5, Xbox) is a manual device with a TV number and no kiosk, but `session.start` and `session.transfer` refuse anything that is not a PC, so staff cannot sell console time and the console tile cannot warn.

**Approach:** Let `session.start` and `session.transfer` take a `console` device as well as a `pc`. Every mode (postpaid, pack, pass, wallet, block) then runs as for a PC: the timer runs on the Hub, no kiosk is paired so nothing is unlocked, and plan/group checks (`PLAN_NOT_FOR_GROUP`, `GROUP_KIND_MISMATCH`) already keep PC and console plans apart. The Hub UI draws the console tile with a controller glyph and "TV n", shows "10 min left", "5 min left" or "1 min left" beside the warning border, and the console sheet starts sessions but offers no remote PC actions.

## Boundaries & Constraints

**Always:**
- A VR rig or simulator still takes no session (`VALIDATION_FAILED` on `device_id`).
- A console must be in a group with a rate for postpaid and wallet (`NO_RATE`), like a PC.
- A PC pack or pass never starts on a console and a console pack never on a PC (`PLAN_NOT_FOR_GROUP`).
- The tile warning text is shown for console tiles only, from 10 minutes left: 10 min, then 5 min, then 1 min.
- No code comments beyond doc comments. Every new string is in `packages/ui/src/messages/en.json`.

**Never:**
- A new intent, a new permission, grace or the TIME UP alert for consoles (3.36), the controller check (3.36), the Pi/QR start (Epic 12).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| Console postpaid start | console in a console group with a rate | session row, device `in_session`, no unlock sent | none |
| Console pack / wallet | console pack, funded wallet | timed session ending at the pack or wallet horizon | none |
| PC pack on a console | pack sold for a PC group | refused | `PLAN_NOT_FOR_GROUP` |
| VR rig | `session.start` on a VR device | refused | `VALIDATION_FAILED` on `device_id` |
| Console sheet | free console | start section shown, no Unlock, Message, Lock, Restart, Shut down or Admin unlock | none |
| Console warning | 9:30, 4:30, 0:40 left | "10 min left", "5 min left", "1 min left" with ⚠ and the warning border | none |

</intent-contract>

## Code Map

- `crates/arena-core/src/apply/session.rs`, `transfer.rs` -- the device kind check now allows `console`; `tests/console.rs` covers it.
- `crates/arena-api/src/ui_auth.rs` -- test: a manual console starts a session through `/ui/intent`.
- `packages/ui/src/components/FloorTile.svelte` -- `console` and `warningText` props, controller glyph, `data-variant="console"`.
- `apps/hub/src/routes/floor/+page.svelte` -- warning text for console tiles; `apps/hub/src/lib/TileSheet.svelte` -- console sheet starts sessions, remote PC actions only for a PC.

## Tasks & Acceptance

**Execution:**
- [x] `arena-core` -- console accepted by `session.start` and `session.transfer`
- [x] `arena-core` tests -- postpaid, pack, wallet, plan/group separation, busy console, VR still refused
- [x] `arena-api` test -- manual console session through the Hub API
- [x] `FloorTile` -- controller glyph and warning text; message `hub.floor.min_left`
- [x] Hub floor page and tile sheet -- console tile text, start section, no remote PC actions
- [x] Playwright `console.spec.ts`; `tile-sheet.spec.ts` now shows a VR rig for the details-only case

**Acceptance Criteria:**
- Given a console in a console group, when its tile renders, then it shows a controller glyph, "TV n" and the same states as a PC tile.
- Given the console sheet, when staff start a session, then postpaid, pack (console packs only), wallet, pass and block start as for PCs, the timer runs on the Hub and no kiosk unlock is sent.
- Given a running console session, when 10, 5 and 1 minutes remain, then the tile shows the warning border, the ⚠ glyph and "10 min left", "5 min left", "1 min left".
- Given a PC plan or pack offered for a console, then it is refused `PLAN_NOT_FOR_GROUP`.
- Given the console sheet, then lock, message, restart, shut down and admin unlock are absent.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `check:strings`, `check:tokens-used`, `check:contrast`, `check:hex`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test force-end undo pause group-bill transfer session-end session-start extend prepaid-start tile-sheet floor audit console session-timer alerts` in `apps/hub`.

## Auto Run Result

Status: review
Verified: fmt, clippy; workspace tests (arena-core 658 lib tests incl. 5 new `console` tests, arena-api 221 incl. the console start); hub and kiosk check; `check:strings`, `check:tokens-used`, `check:contrast`, `check:hex`, `check:protocol`, `check:layers`, `format:check`; Playwright 276 (console, tile-sheet, floor, session-timer, alerts, transfer, group-bill, force-end, undo, pause, session-start/end, extend, prepaid-start, audit). Known unrelated failures as before: arena-cloud (2) and arena-tasks (2). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: the server change is only the device-kind check, because grouping, rate and plan-group checks already key on the group kind; no kiosk push is skipped explicitly, as a manual console has no connection to push to; warning text is limited to console tiles so PC tiles are unchanged; transfer between consoles is allowed for the same reason.
Deferred: "Start" for group sessions (`group_with`) is still offered on PCs only; console time-up and controller check are Story 3.36.
