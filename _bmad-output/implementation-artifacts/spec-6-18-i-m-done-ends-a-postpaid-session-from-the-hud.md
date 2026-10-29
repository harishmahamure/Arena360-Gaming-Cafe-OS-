---
title: 'Story 6.18: "I''m done" ends a postpaid session from the HUD'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: '8bb3b2abe4fc4402fe41e11990cef9e598567b31'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-17-the-account-menu-with-lock-and-log-out.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A postpaid player can only stop paying by finding Log out in the account menu, which carries no hint that the bill is next, and the expanded HUD offers no way to finish at all.

**Approach:** "I'm done" is a secondary action on postpaid sessions only, in the account menu (above Log out) and in the expanded HUD panel. It asks "End your session? You'll pay at the counter." in place and, on confirm, goes through the Story 6.17 `player.end_session` frame: the Hub ends the session on the paired PC (`session.end`, the session's own `rev`), bills it to the minute as in Story 3.11, and pushes the `PAY_AT_COUNTER` lock after cleanup. No new frame, intent or permission: the kiosk-side work is UI only.

## Boundaries & Constraints

**Always:**
- Offered to postpaid sessions (profiled or guest) only; prepaid sessions show neither the row nor the HUD button.
- The question takes focus on Cancel. Cancel (and Esc on the HUD question) changes nothing and the session continues; Esc in the account menu closes the menu and sends nothing.
- Hub unreachable: nothing is sent and the row or panel says "Counter is reconnecting — try again shortly." A Hub refusal says "Couldn't log out — try again." and keeps the session.
- The bill, the tile's "UNPAID <amount>" and staff Undo within 8 s are the existing Story 3.11 / 3.14 paths; the end is a `session.end` like any other.

**Never:**
- A new frame, intent kind, permission or `ActorKind::Player`; changes to the Log out row, prepaid flow or the lock covers.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Offer | postpaid, menu or expanded HUD | "I'm done" shown (menu: above Log out) | prepaid: not shown |
| Confirm | "End session" | `player_end` sent; Hub bills, cleanup, lock "Pay at counter" | refused: line, session continues |
| Cancel | Cancel / Esc | nothing sent, session continues | N/A |
| Hub down | "I'm done" | nothing sent, "Counter is reconnecting — try again shortly." | N/A |

</intent-contract>

## Code Map

- `apps/kiosk/src/lib/AccountMenu.svelte` -- `done` row and view (reuses `end()`), postpaid only; `rows[4]` is now Log out.
- `apps/kiosk/src/lib/Hud.svelte` -- "I'm done" button, in-place question, Hub status for the reconnecting line.
- `packages/ui/src/messages/en.json` -- `player.account.row.done`, `done_confirm`, `done_yes`, `done_cancel`.
- Tests: `apps/kiosk/tests/account-menu.spec.ts`, `apps/kiosk/tests/hud.spec.ts`; the Hub path is covered by `crates/arena-api/tests/lan.rs` (`logging_out_of_a_postpaid_session_bills_it_and_locks_to_pay_at_counter`).

## Tasks & Acceptance

**Execution:**
- [x] `apps/kiosk` -- "I'm done" in the account menu and the expanded HUD panel, strings, Playwright
- [x] Story file and sprint status

**Acceptance Criteria:**
- Given a postpaid session, then the account menu lists "I'm done" above Log out and the expanded HUD offers it; a prepaid session shows neither.
- Given "I'm done" confirmed, then `player_end` is sent once and the Hub ends and bills the session and locks the PC to "Pay at counter".
- Given Cancel or Esc, then nothing is sent and the session continues.
- Given the Hub unreachable, then nothing is sent and the counter-is-reconnecting line shows.

## Spec Change Log

- The epic says `session.end{actor: player}`. The Hub has no player actor kind; as in Story 6.17 the end is applied with the paired device as actor, and the audit row carries the session (and so its player). Adding a player actor kind is a separate, wider change.

## Review Triage Log

Code review (iteration 1): no blocking findings. HUD window is covered by the default capability; `player_end` and `hub_status` are service-bridged. Checks and the hud/account-menu Playwright suites pass (46 tests).

## Verification

**Commands:**
- `pnpm check:strings && pnpm check:hex && pnpm check:no-blur && pnpm format:check` -- expected: pass
- `pnpm --filter @arena/kiosk check` -- expected: pass
- `pnpm --filter @arena/kiosk test:ui` -- expected: pass apart from the 4 `theme.spec.ts` tests (existing `C:\C:\` path bug)

## Auto Run Result

Status: done

Verified: strings, hex, no-blur, format and kiosk svelte-check pass; the kiosk Playwright suite passes (including 10 new "I'm done" tests) apart from the 4 `theme.spec.ts` tests. No Rust changed.

Not run: the real Tauri shell and a real LAN round trip; the HUD window's focus behaviour in the shell.
