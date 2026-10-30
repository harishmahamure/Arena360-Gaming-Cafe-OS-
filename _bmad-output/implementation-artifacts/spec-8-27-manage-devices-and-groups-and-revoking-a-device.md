---
title: "Story 8.27: Manage → Devices and groups, and revoking a device"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: 'b921b1e'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-25-step-up-mfa-for-sensitive-commands.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-26-manage-staff-users-and-roles-by-command.md'
deferred: []
---

<intent-contract>

## Intent

**Problem:** A lost or replaced kiosk, PC or Pi stays able to connect until someone is at the café, and the owner cannot see or regroup devices from a phone.

**Approach:** Manage gets a fourth area, Devices & groups (`?area=devices`). It lists each live paid branch's groups (name, swatch dot, kind) and devices (name, kind, group, revoked) from the summary catalogue. `group.upsert` and `device.assign` become plain owner kinds; `device.revoke` becomes a sensitive owner kind that carries a reason, so it passes step-up MFA (8.25). The Hub applies its existing intents of the same names with an `Owner` actor.

## Boundaries & Constraints

**Always:**
- Owner kinds: `group.upsert` (`GroupUpsert`), `device.assign` (`DeviceAssign`, `group_id: null` = Unassigned) and `device.revoke` (`OwnerDeviceRevoke {device_id, reason}`, `deny_unknown_fields`, reason one of lost|stolen|replaced|other). The cloud refuses a payload outside these before step-up. 17 kinds are handled by the Hub; `hub.revoke` and `backup.restore` stay reserved (Epic 9).
- The Hub turns the command's `reason` into the intent's `reason` (`REASON_REQUIRED` when absent or unlisted). The step-up shows as `elevated_by{kind: mfa}` on the change row (8.25), and the existing LAN fan-out closes the device's sockets with `TOKEN_REVOKED`.
- A revoke of a device that is already revoked is answered `applied` before any intent is built, changes nothing and writes no change row (`already_revoked` in the writer).
- Summary catalogue: group rows carry `fields {name, group_kind, colour, sort_order}`; a revoked device stays listed with `revoked: true` (its `devices_meta` row is soft-deleted by the revoke, so the query also takes rows whose device is revoked).
- The cloud's command sweeper rejects `pending`/`fetched` commands of a branch that has Hub rows but none with a live token, with `TOKEN_REVOKED` (`SweepReport.revoked`); the Owner app shows "Device/Hub revoked".
- Owner app: groups are matched by name over the branches in scope like Pricing; a device belongs to one branch, so a move or revoke is one command with that branch's own ids. A device is offered only groups of its kind (pc to pc, console to console, vr to vr, sim to other, the Hub's rule).

**Never:** revoking the Hub itself (`hub.revoke`, Epic 9), a revoke without a reason or step-up, moving or revoking an already revoked device from the app.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Edit a group | rename, colour lime | `group.upsert {id, name, group_kind, colour, sort_order}` per branch with `base_rev`; no step-up | empty name: copy inline, no preview |
| Add a group | name, kind, colour | `group.upsert` with the next sort order | name taken at a branch: left out with the reason |
| Move a device | pick a group or Unassigned | `device.assign {device_id, group_id}` to its branch, no step-up | Hub refuses a kind mismatch: "Couldn't apply at branch" |
| Revoke | reason, confirm, code | step-up dialog, then `device.revoke {device_id, reason}` | no reason: inline error, no preview |
| Revoke again | already revoked | `applied`, nothing changed | — |
| Hub token revoked | waiting commands | `rejected` `TOKEN_REVOKED` by the sweeper | shown "Device/Hub revoked" |

</intent-contract>

## Code Map

- `crates/arena-proto/src/owner_commands.rs` -- three kinds (15 in the catalogue), `OwnerDeviceRevoke`, `DEVICE_REVOKE_REASONS`, payload checks.
- `crates/arena-core/src/{commands.rs,writer.rs}` -- `device.revoke` handled (17), reason into the intent, `already_revoked`.
- `crates/arena-store/src/summary.rs` -- group `fields`, revoked devices listed. `crates/arena-store/src/control/commands.rs` + `crates/arena-cloud/src/command_sweep.rs` -- revoked-token rejection.
- `apps/owner/src/lib/devices.ts` (new) -- lines, group rules, `proposeGroup|Assign|Revoke`; `DevicesBoard`, `DevicesPanel`, `GroupForm`, `MoveForm`, `RevokeForm` (new); `ManageBoard` fourth area; `commands.svelte.ts` kind labels.
- `packages/ui/src/messages/en.json` -- `owner.devices.*`.
- Tests: `arena-proto` unit, `arena-core` (owner commands, summary), `arena-cloud` (`stepup.rs`, `owner_commands.rs`), `apps/owner/tests/devices.spec.ts` (7 × desktop/phone).

## Tasks & Acceptance

**Execution:**
- [x] Proto/core/store: kinds, revoke payload and reason, already-revoked no-op, summary fields; Rust tests.
- [x] Cloud: sweeper rejects commands of a branch with no live Hub token; step-up and payload tests.
- [x] Owner app: Devices & groups area, forms, strings; Playwright tests.

**Acceptance Criteria:**
- [x] Given the catalogue, then `group.upsert` and `device.assign` are non-sensitive, `device.revoke` is sensitive, each with a payload schema and a Hub handler that applies the existing intent as the owner.
- [x] Given Manage → Devices & groups, then groups (name, swatch dot) and devices (name, kind, group, revoked) are listed from the summary and group edits and moves send `group.upsert` / `device.assign`.
- [x] Given "Revoke device", when the owner confirms and passes step-up, then `device.revoke` is sent and the Hub marks the token revoked, closes the socket with `TOKEN_REVOKED`, frees the slot and audits `elevated_by{kind: mfa}`.
- [x] Given a command for a branch whose Hub token is revoked, then the cloud marks it `rejected` `TOKEN_REVOKED`, shown "Device/Hub revoked".
- [x] Given a revoke for an already revoked device, then it reports `applied` and changes nothing.

## Spec Change Log

- The epic says the cloud marks a pending command rejected "when its Hub token is revoked"; no code path revokes a Hub without pairing a replacement before Epic 9, so the sweeper (every minute) does it for branches whose Hub rows are all revoked rather than at revoke time.

## Review Triage Log

Code review run (blind, edge-case, acceptance). Fixed: the Hub now refuses a revoke reason outside lost|stolen|replaced|other (REASON_REQUIRED, with a test); the summary lists live devices before revoked ones so revoked rows cannot crowd out the cap; Move with the group unchanged closes instead of sending a no-op; stale HANDLED_COMMAND_KINDS doc. Deferred as non-blocking: sweep window between a Hub revoke and re-pair (accepted in the Spec Change Log), extra sweeper and Playwright edge tests. Verification: clippy for the touched crates, cargo tests (proto, store, core, cloud, api), owner `svelte-check`, the full owner Playwright suite (522), `check:strings`, `check:layers`, `check:protocol` and `format:check` are green. Pre-existing, untouched: the flaky Windows `arena-cloud` `binary` test `unreachable_at_start...`, the Windows-only `arena-e2e` build and a non-exhaustive `HubEvent` match in `arena-kiosk-svc`. `check:generated` compares with git, so it passes once the new type is committed.

## Design Notes

- Owner `group.upsert` base_rev is not enforced (an owner actor always wins at the Hub for groups); the app still sends the row's `rev`.
- The preview names a device through the draft's payload (`{name}`) while the variant holds what is sent, like staff.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-api`, `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-api --all-targets -- -D warnings`, `pnpm run gen:types`, `check:strings`, `check:layers`, `check:protocol`, `format:check`; in `apps/owner`: `pnpm run check` and `pnpm exec playwright test` -- expected: clean

## Auto Run Result

Status: in-review. Not run: staging proof of the end-to-end revoke on a real kiosk, the code-review pass.
