---
title: 'Story 1.13: Inline manager PIN elevation'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'df4d25db3aad411c1f0a2b9aaeaa41b05d4e7b18'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-12-branch-locale-settings-currency-number-format-and-timezone.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** When a Counter's role lacks a permission, they hit a dead end. A manager has to take over the terminal, and the audit cannot show who approved the action.

**Approach:**
- **Shared component:** an `InlinePin` component that opens in place.
- **Ticket flow:** a loopback flow where the Hub verifies a manager PIN with Argon2id and issues a short-lived, single-use elevation ticket. The ticket is then attached to the intent.
- **`apply()`:** checks the permission against the elevator, checks `verifier_version`, and records `elevated_by`.
- **Reasons:** a generic per-intent reason mechanism, with its picker in the same panel.

## Boundaries & Constraints

**Always:**
- **arena-proto:**
  - `ElevatedBy { kind: ElevationKind (pin|mfa), id: staff_id, mode: hub|cached_verifier, verifier_version: i64 }`. This replaces the 1.7 shape. The `changes.elevated_by_kind` CHECK becomes `pin|mfa` (new migration).
  - `Intent` gains `reason: Option<ReasonCode>`. `IntentKind::reasons() -> Option<&'static [ReasonCode]>`: `device.revoke` requires one of `lost|stolen|replaced|other`, and every other kind requires none.
  - New `ErrorCode`s: `PIN_REQUIRED`, `PIN_REJECTED`, `REASON_REQUIRED`, `AUTH_LOCKED` (reused if it already exists).
  - Regenerate the TS types.
- **apply():**
  - **Permission step:** if `elevated_by` is set, the permission is checked against the elevating staff member instead of the actor. That staff member must be active, must hold the permission, and must not be the actor. `mode` must be `hub` here (`cached_verifier` is Story 1.20's kiosk exit). `verifier_version` must equal that staff member's current `pin_verifier_version`, otherwise `PIN_REQUIRED` "enter the PIN again".
  - **Validate step:** a kind with reasons and `reason` = None fails `REASON_REQUIRED`; a reason not in the list fails `VALIDATION_FAILED` on `reason`.
  - **Change rows:** they record `actor` (the Counter) and `elevated_by_*`.
  - **Remove the 1.7 blanket rejection.** Client-supplied `elevated_by` is still always stripped at the edges (`/ui/intent`, LAN), so only the Hub's ticket path can set it.
- **PIN rotation:** `staff.upsert` with a new `pin_hash` bumps `pin_verifier_version` by 1.
- **Elevation API (loopback, bearer session):**
  - `POST /ui/elevate {intent_kind, pin}`. The Hub finds the requirement for that kind. If it is a `Perm(p)`, it verifies the PIN with argon2id against active staff (excluding the session's own staff) whose role holds `p`, one by one, at most 50.
  - **Match:** returns `{ticket}`. The ticket is 128 random bits, held in memory with `{staff_id, verifier_version, intent_kind, session}`, expires in 60 s and is single-use.
  - **No match, or a non-Perm requirement:** `PIN_REJECTED` with one fixed message that never names anyone. Matching a staff member whose role lacks the permission gives the same message.
  - **Throttle:** 5 consecutive failures per session lock elevation for 60 s, giving `AUTH_LOCKED` with `retry_after_s`. This uses the injectable clock (1.11 lockout pattern).
  - **Redeeming:** `POST /ui/intent {intent, elevation_ticket?}` redeems the ticket. It must belong to the same session and the same intent kind and be unexpired, and is consumed. The Hub builds `elevated_by` from it, and `apply()` re-checks the verifier version. A bad ticket gives `PIN_REQUIRED`.
  - The PIN is never logged or stored, never sent anywhere except this one loopback call, and never echoed.
- **`InlinePin` component (`@arena/ui`):**
  - Props: `length` (4–6, default 4), `onsubmit(pin) -> Promise<Result>`, `oncancel`, `message` (catalogue key), plus a `returnFocusTo` element.
  - Renders N cells (`surface-2` fill, 1px `border-strong`, ≥44px), with a visually hidden `<input inputmode="numeric" autocomplete="off">` that holds the digits, and a numeric keypad (0–9, ⌫, ✓) of ≥44px buttons.
  - Typing digits, Backspace and Enter work. Esc clears the PIN, calls `oncancel` and returns focus.
  - After submit, the PIN state is cleared immediately whatever the outcome (assign an empty string and drop the reference).
  - Rejection clears the cells and shows the message inline.
  - The live region announces cell count ("2 of 4"), never the digits.
  - It works in dark, HC and light, and appears in the gallery with an axe pass.
- **`ReasonPicker` component:** radio options from catalogue keys plus Confirm. It is shown in place of the PIN after acceptance, when the intent kind needs a reason.
- **Hub UI, Settings → Branch** (this supersedes 1.12's Counter read-only rule):
  - A Counter can edit the fields. Save shows the inline note "Needs manager approval".
  - Activating Save opens `InlinePin` in the panel without navigating away.
  - After `/ui/elevate` succeeds, the UI submits each `settings.set` with the ticket. One ticket covers one intent, so the UI requests one elevation and the Hub accepts it for up to N intents of the same kind within 60 s. The ticket carries `uses_left` = the number of changed fields, capped at 10.
  - On success it closes and focus returns to Save.
  - It works with the keyboard only.
- **Gallery:** a demo of a reason-requiring flow, so the reason-picker AC is UI-tested (a stubbed `device.revoke`).

**Never:**
- Kiosk cached-verifier elevation (1.20).
- Owner MFA (later epic).
- Sending the PIN to any other device or LAN client.
- Storing the PIN client-side beyond the component's local state.
- Naming the matched or failed staff member in any message.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Approve | Counter edits the currency, then the manager PIN 2222 | applied; change row `actor=counter`, `elevated_by={pin, manager, hub, v1}` | N/A |
| Wrong PIN | 9999 | `PIN_REJECTED` generic; cells clear; nothing applied | N/A |
| Lacking role | the Kitchen PIN 4444 | the same generic `PIN_REJECTED` | N/A |
| Own PIN | the Counter's PIN 3333 | `PIN_REJECTED` (cannot self-elevate) | N/A |
| Rotated | the manager PIN rotates between elevate and intent | `PIN_REQUIRED`; the UI asks for the PIN again | N/A |
| Ticket reuse | the same ticket used beyond `uses_left`, or after 60 s | `PIN_REQUIRED` | N/A |
| Throttle | 5 wrong PINs | `AUTH_LOCKED` with a countdown | N/A |
| Reason needed | `device.revoke` elevated without a reason | `REASON_REQUIRED`; the picker shows; applied after a choice with `reason` recorded | N/A |
| Esc | Esc mid-entry | cleared; focus back on Save | N/A |
| Spoof | the client sends `elevated_by` in its intent body | stripped; the Counter is denied `PERMISSION_DENIED` | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/{actor.rs or intent.rs,error.rs}` -- `ElevatedBy`, from 1.7. Change its `kind` to `ElevationKind`.
- `crates/arena-core/src/apply/{permission/rbac,validate}` -- the 1.11 RBAC step and the 1.7 rejection of `elevated_by`. Replace them.
- `crates/arena-api/src/ui_auth.rs` (1.11) -- sessions, lockout and `/ui/intent`. Add `/ui/elevate` and the tickets.
- `crates/arena-core` argon2 helpers -- `devices.rs` and `staff.rs`.
- `packages/ui/src/components/*`, `apps/gallery` -- the component patterns and the a11y suite.
- `apps/hub/src/routes/settings/branch` (1.12) -- the Save flow.

## Tasks & Acceptance

**Execution:**
- [x] proto, migration, `apply()` changes, `staff.upsert` version bump, with Rust tests for every non-UI matrix row.
- [x] `crates/arena-api` `/ui/elevate`, tickets, throttle, strip/redeem, with tests.
- [x] `packages/ui/src/components/{InlinePin,ReasonPicker}.svelte` + gallery entries + a11y tests.
- [x] `apps/hub` Settings → Branch flow + `apps/hub/tests/elevation.spec.ts` (keyboard-only path, Esc, rejection, rotation message via stub).
- [x] Catalogue keys, `README.md`.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked`, `pnpm check`, and the gallery and hub Playwright suites, then all pass and every matrix row is covered.

## Implementation Notes

- **Migration 0007** rebuilds `changes` (SQLite cannot alter a CHECK): `elevated_by_kind IN ('pin','mfa')`, the append-only triggers recreated, and a new `reason TEXT NULL` column so the chosen reason code is recorded on each change row. `ElevatedBy.verifier_version` is `i64` (TS `number`).
- **Reasons.** `ReasonCode` is an enum (`lost|stolen|replaced|other`), so an unknown string fails to decode (`BAD_FRAME`); `VALIDATION_FAILED` on `reason` covers a listed code the kind does not take, including any reason on a kind with none. `IntentKind::reasons()` is generated as `packages/ui/src/generated/reasons.json` (`reasonsFor()` / `reasonOptions()` in `@arena/ui`), so a screen asks for the reason with the PIN.
- **Fingerprint.** The dedupe fingerprint covers kind, payload, `base_rev`, actor and `reason` (only when set, so older fingerprints are unchanged), never `elevated_by`: a same-key retry after re-entering a PIN (or with another approver) is the same intent. `PIN_REQUIRED` and `REASON_REQUIRED` are never recorded.
- **apply() permission step.** With `elevated_by`: the requirement must be a `Perm`, kind `pin`, mode `hub`, the actor an active staff member other than the elevator, the elevator active and holding the permission (else `PERMISSION_DENIED`), then the verifier version (else `PIN_REQUIRED` "enter the PIN again").
- **/ui/elevate.** Takes `items: [{intent_kind, key?}]` (1–10; `settings.set` items name their key, others take none; else `VALIDATION_FAILED` on `items`, which is not a recordable field). Order: session, body, follower (`NOT_PRIMARY`), item shape, unhandled kind (`UNSUPPORTED`), the member already holding every permission (`VALIDATION_FAILED` on `items`), throttle, then the PIN. The PIN must be 4–6 digits (anything else is a `PIN_REJECTED` that still costs one argon2id check, as does having no candidate); candidates must hold every permission the items need (one SQL query: active, with a PIN, not the session's member, ordered by id, limit 50). At most 2 PIN checks run at once (a semaphore). The throttle is keyed exactly by the requesting staff id, so a new session does not reset it; a check that could not run releases its reservation without counting either way. The fifth failure already answers `AUTH_LOCKED`. Failures log at `warn` (requester and kinds, never the PIN); a grant logs the approver id. `ElevateRequest`'s `Debug` redacts the PIN.
- **Tickets.** Each approved item is redeemed once by an intent of its kind (and, for `settings.set`, its key); a bad ticket is `PIN_REQUIRED` and spends nothing. A refusal (any error but `HUB_SLOW`, as `apply()` is one transaction) gives the item back, unless the ticket has lapsed, been dropped or been evicted. A session holds at most 4 tickets (the oldest goes); `POST /ui/elevate/drop {ticket}` gives one up; signing out drops the session's tickets. The elevation throttle entry is deliberately kept on sign-out (dropping it would let sign-out/sign-in reset the count, which the staff-id key exists to prevent).
- **Status codes.** `PIN_REQUIRED`/`PIN_REJECTED` are 403, `REASON_REQUIRED` 422.
- **InlinePin.** `minLength`/`maxLength` (4/6): `maxLength` cells, Enter/✓ from `minLength`, the last cell submits on its own. The "visually hidden" input is a transparent `type="password"` input laid over the cells (a 44px target that takes taps and draws the focus ring; screen readers never echo digits). The PIN state is cleared before `onsubmit` is awaited; Esc and Cancel wait while it is checked. Locked keys use `aria-disabled` with the countdown as their description.
- **ReasonPicker.** Focuses the chosen option (else the first); `onconfirm` may answer a catalogue key shown inline, and a throw shows `ui.pin.unreachable`; Esc waits while confirming.
- **Hub Branch page.** Fields are editable by any signed-in member; without `settings.edit` Save shows "Needs manager approval" and opens the PIN in the form, listing the fields it approves. One `/ui/elevate` names exactly the changed fields (≤10); each `settings.set` carries the ticket; a lapsed approval (`expires_in_s`) or `PIN_REQUIRED` reopens the PIN with "Enter the PIN again." keeping unsaved edits, and a partial save is reported either way. Unused approvals are dropped. An approval that arrives after a cancel is dropped and saves nothing; an approved save with nothing left to save says so. An unexpected elevate answer shows "Couldn't check the PIN". The 1.12 Counter read-only test moved to `apps/hub/tests/elevation.spec.ts` in its new form.
- **Gallery.** The keyboard-focus test counts a radio group as one tab stop. The reason flow asks for the reason first (from the generated table), then the PIN.
- **LAN.** The actor/elevation binding of `intent.submit` is a small function with its own unit test: no LAN intent can apply today (every handled kind needs a permission, which a device never holds), so a change row cannot show the strip end to end.
- **Dev revoke.** `POST /dev/revoke-device` takes an optional `reason` (default `other`); an unknown one is `VALIDATION_FAILED` on `reason`.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Elevation throttle keyed by session token, so a re-sign-in resets it (blind, edge) | high | A 4-digit manager PIN can be brute-forced by cycling sign-ins. | patch (key by staff_id) |
| 2 | UI only accepts 4-digit PINs; the Hub allows 4–6 (blind, edge) | high | Managers with 5- or 6-digit PINs can never approve. | patch |
| 3 | Esc during "Checking…" still saves with the approval (blind, edge) | medium | `approve()` continues after cancel. | patch |
| 4 | Ticket not bound to what was approved; one approval covers any 10 settings keys (blind, edge) | medium | Binding is session + kind + uses only. | patch (bind to keys, show them) |
| 5 | Store or join errors reset the failure streak (all three layers) | medium | `settle(true)` on error. | patch |
| 6 | Partial-save message lost on a PIN re-prompt | low | `!pinAgain` guard. | patch |
| 7 | Ticket uses burned on errors where nothing was applied (blind, edge) | low | Only REASON_REQUIRED refunds. | patch |
| 8 | Fingerprint includes `elevated_by`, so a same-key retry after a new PIN conflicts (all three layers) | medium | Authorization context isn't part of the request body. | patch (exclude, plus not-recordable tests) |
| 9 | Non-Perm kinds reject without argon2 work; 50 unthrottled checks per call | low | Requirements are public; add a CPU cap. | patch (semaphore) |
| 10 | Failed elevations unlogged; approver not logged | low | Ops visibility. | patch |
| 11 | Tokens lower-cased by `lockout_key`; sign-out leaves tickets | low | Direct fix. | patch |
| 12 | Unknown non-OK responses shown as unreachable; `expires_in_s` ignored | low | Direct fix. | patch |
| 13 | LAN strip and non-staff/inactive-actor elevation untested (blind, verify-gap) | low | Pre-verified gap. | patch |
| 14 | `ElevatedBy.kind` change breaks older clients | low | Pre-release; no deployed clients. | reject |
| 15 | ReasonPicker: wrong focus selector, no inline refusal, unhandled throw, Esc while busy (blind, edge) | low | Direct fixes. | patch |
| 16 | `reasons()` not exported to TS; the PIN is asked first and then fails on REASON_REQUIRED | low | Export a generated reasons table so the UI knows up front. | patch |
| 17 | Elevation issued when not needed, on a follower, or for an unhandled kind (blind, edge) | low | Direct guards. | patch |
| 18 | Counter read-only test deleted; `manager_only` path unreachable | low | Clean up and test. | patch |
| 19 | Refund reinserts an evicted ticket past the cap | low | Direct fix. | patch |
| 20 | `uses` > 255 gives BAD_FRAME instead of clamping | low | Direct fix. | patch |
| 21 | Unknown dev reason silently becomes `other` | low | Direct fix. | patch |
| 22 | A save that exits early after the ticket gives no message | low | Direct fix. | patch |
| 23 | Leftover ticket uses stay live after the loop breaks | low | Drop the ticket. | patch |
| 24 | Ticket cap eviction untested (verify-gap, deferred by reviewer) | low | Cheap to add. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check` -- expected: pass after commit
- `pnpm --filter @arena/gallery test:a11y && pnpm --filter @arena/hub exec playwright test` -- expected: pass
