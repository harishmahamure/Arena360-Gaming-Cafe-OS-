---
title: 'Story 2.22: Step 4 — owner groups devices in the web wizard as they arrive'
type: 'feature'
created: '2026-10-26'
status: 'review'
baseline_commit: '9517349'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-15-owner-changes-reach-the-hub-through-the-command-mailbox-with.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-20-device-groups-with-six-swatches-held-at-the-hub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-21-register-consoles-vr-rigs-and-simulators-manually.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The owner cannot organise the floor from the browser: step 4 is a placeholder, the heartbeat carries no devices or groups, and the cloud shows no pending or refused device changes.

**Approach:**
- The Hub's heartbeat carries a `setup` block (devices and groups, no player data) before go-live; the cloud keeps the latest block per branch in `branch_setup_status`.
- Step 4 reads that block, overlays the owner's open `group.upsert`, `device.assign` and `device.register` commands as "Waiting for branch", and shows a Hub refusal inline for five minutes.
- Step 4 completes in the same transaction that stores a block with a grouped device.

## Boundaries & Constraints

**Always:**
- **Cloud migration** `0013_branch_setup_status.sql`: `branch_setup_status(branch_id PK, body TEXT, reported_at INTEGER)`.
- **Heartbeat:** `HeartbeatRequest.setup: Option<BranchSetup>` (devices: id, label, kind, group id, status; groups: id, name, kind, colour, sort). The Hub sends it while `!live`, with paired devices only and LAN presence applied. A block over 500 devices or groups is ignored; a malformed one is a 400.
- **Cloud API** (owner session, branch must have a seen Hub, else `STEP_NOT_READY`): `GET /v1/wizard/devices` returns the block, `reported_at`, `now`, `hub_online` (seen within 3 min) and the commands to overlay: open ones, applied ones resolved after `reported_at`, and rejected ones resolved in the last 5 min. `POST /v1/wizard/devices` queues one of `group.upsert`, `device.assign`, `device.register` through the existing mailbox; the Hub validates it.
- **Wizard events:** a changed block and every command result send `wizard.changed`.
- **Web:** Unassigned list, groups with their devices, drag onto a group or Move to group (native select), add group (name, kind, six swatches), add console, VR rig or simulator, `tier-lock` on `DEVICE_LIMIT`, "Hub offline" badge and "As of n min ago", polled every 5 s; moves to step 5 by itself.

**Never:** Player data in the block; a placeholder group as a drop target (its id is the Hub's to mint); hard deletes.

## Code Map

- `crates/arena-proto`: `setup_devices.rs`, `pairing.rs` (`HeartbeatRequest.setup`); generated TS.
- `crates/arena-store`: `control-migrations/0013_branch_setup_status.sql`, `control/setup_status.rs`.
- `crates/arena-core`: `snapshot.rs` (`branch_setup`), `writer.rs` (`snapshot`).
- `crates/arena-tasks/src/heartbeat.rs`, `crates/arena-hub/src/lib.rs` (presence).
- `crates/arena-cloud`: `api/heartbeat.rs`, `api/commands.rs`, `api/wizard_devices.rs`, `tests/pair.rs`, `tests/backup.rs`.
- `apps/onboarding`: `DevicesStep.svelte`, `devices.ts`, `api.ts`, `[step]/+page.svelte`, tests; `packages/ui` messages.

## Tasks & Acceptance

- [x] Proto types, `HeartbeatRequest.setup`, generated TS.
- [x] Migration, store module, heartbeat storing the block and finishing step 4, events.
- [x] `GET`/`POST /v1/wizard/devices`.
- [x] Hub heartbeat sends the block with presence.
- [x] Step 4 page, strings, stub and tests (Rust: core, cloud; Playwright and unit: step 4).

**Acceptance Criteria:**
- Given a paired Hub before go-live, when it heartbeats, then the block is stored as the branch's latest row.
- Given step 4 is open, when a kiosk pairs, then it appears Unassigned with its name and status within 5 s of the next block, without a reload.
- Given a new group, then `group.upsert` is queued and the group shows "Waiting for branch" until the Hub reports it.
- Given a device and a group, when the owner drags it (or uses Move to group), then `device.assign` is queued, the device moves with "Waiting for branch", and a rejection returns it with the plain reason inline.
- Given Add console, VR rig or simulator, then `device.register` is queued; `DEVICE_LIMIT` shows `tier-lock` "Available on Basic".
- Given a device in a group as reported by the Hub, then step 4 is done and focus moves to step 5.
- Given the Hub is offline, then the last devices show with "As of n min ago" and a "Hub offline" badge, and commands queue in the mailbox.
- Given groups were edited at the Hub, then the next block replaces what the wizard shows.

## Verification

- `cargo test --workspace --locked`; clippy; `pnpm run check`; onboarding `pnpm run check` and Playwright.

## Design Notes

Decisions: the cloud overlay is stateless (the page derives "waiting" and refusals from the command list); the presence feed is injected into `HeartbeatTask` so `arena-tasks` does not depend on `arena-api`; no rate limit on the new POST beyond the existing session and payload caps.

## Review Triage Log

No open review findings. Verify gate re-run: pnpm run check, cargo tests for cloud, core, proto, store and tasks, onboarding svelte-check and Playwright (172 passed, 2 skipped).
