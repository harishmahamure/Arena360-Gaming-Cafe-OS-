---
title: 'Story 2.15: Owner changes reach the Hub through the command mailbox within one poll'
type: 'feature'
created: '2026-10-26'
status: 'done'
baseline_commit: '11461a4'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-11-hub-checks-in-by-heartbeat-and-the-wizard-advances-by-itself.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-13-branch-bootstrap-applies-the-onboarding-profile-at-first-pai.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Changes the owner makes online during setup have no reliable way to reach the Hub: nothing queues them while the Hub is offline, and nothing makes the Hub apply each one exactly once.

**Approach:**
- A `commands` table in `gz_control` queues setup commands per branch with a relay-assigned monotonic id and a unique `(branch_id, idempotency_key)`; a `command` role of `arena-cloud` serves `GET /v1/commands` and `POST /v1/commands/{id}/result` to the Hub.
- Only the seven setup kinds are accepted on every tier; any other kind is `UNSUPPORTED`.
- The Hub polls every 60 s, or at once when a heartbeat answer carries `pending_commands > 0`, applies each command through `apply()` with an `owner` actor, dedupes in `applied_commands`, and posts each result until the cloud acknowledges it.

## Boundaries & Constraints

**Always:**
- **Cloud migration** `0011_commands.sql`: `commands(id AUTOINCREMENT, branch_id, idempotency_key, kind, schema_version, payload, issued_by, status pending|fetched|applied|rejected, error, applied_seq, created_at, fetched_at, resolved_at)`, unique `(branch_id, idempotency_key)`. A duplicate submit returns the existing row.
- **Submit** (`api::commands::submit_command`, called by later wizard stories): kind in `SETUP_COMMAND_KINDS` else `UNSUPPORTED`; key a ULID; payload a JSON object of at most 16 KiB; `settings.set` only for `branch.name` and `branch.tz`.
- **Fetch:** the Hub's bearer token picks the branch; rows with `id > after` that are pending or fetched come back in id order (50 per page) and pending rows become fetched. Resolved rows never return. `epoch` is required but not fenced (fencing is Epic 8).
- **Result:** `{epoch, applied_seq?, error?}` resolves a pending or fetched row of the Hub's own branch; any repeat, even a different answer, changes nothing and returns the stored status. Another branch's id is `NOT_FOUND`.
- **Heartbeat:** the answer carries `pending_commands` (count of pending rows) only when it is above zero.
- **Hub migration** `0022_applied_commands.sql`: `applied_commands(command_id, idempotency_key, kind, status, applied_seq, error, applied_at, reported_at)`. The cursor is the largest recorded id, so nothing extra is persisted.
- **Apply:** `WriterHandle::apply_command` runs the intent and writes the `applied_commands` row in one transaction; a deterministic rejection is recorded alone; `NOT_PRIMARY` and `INTERNAL` record nothing so the next poll retries. A command already recorded by id or key returns its stored row.
- **Owner wins:** the intent's actor is `owner`; before `apply()` runs, `owner_wins` sets `base_rev` to the row's current `rev` for a setting, device or staff edit, so a stale or missing one never refuses it (groups, zones, products and maintenance already accept an owner's). `apply()` itself is unchanged, so other owner-actor callers still get `STALE`. The audit keeps both changes; the existing `groups.changed` toast covers owner group, zone and device-place writes.
- **Kinds the Hub runs:** `group.upsert`, `device.assign`, `device.register`, `staff.upsert`, `settings.set`. `setup.test_session` and `branch.go_live` are accepted by the cloud but the Hub records them as rejected `UNSUPPORTED` until Stories 2.29 and 2.31.
- **Deploy:** the command role is its own Deployment and Service (`arena-cloud-command`), and the Ingress sends `/v1/commands` to it. `--role command` serves `/healthz` and the two mailbox routes only; `control` and `all` serve them too.

**Never:** step-up MFA, fan-out to several branches, the sweeper, epoch fencing, owner command UI (all Epic 8); a new error code.

## Code Map

- `crates/arena-proto/src/commands.rs` (new), `pairing.rs` (`HeartbeatResponse.pending_commands`), `lib.rs`; generated TS.
- `crates/arena-store`: `control-migrations/0011_commands.sql`, `control/commands.rs`, `migrations/0022_applied_commands.sql`, `src/commands.rs`.
- `crates/arena-cloud`: `api/commands.rs` (new), `api/heartbeat.rs`, `api/mod.rs`, `config.rs` (`Role::Command`), `lib.rs`.
- `crates/arena-core`: `commands.rs` (command to intent), `writer.rs` (`apply_command`, `command_cursor`, `unreported_commands`, `mark_command_reported`); `commands.rs` also holds `owner_wins`.
- `crates/arena-tasks`: `cloud.rs` (`commands`, `command_result`), `commands.rs` (`CommandTask`), `heartbeat.rs` (poke); `crates/arena-hub/src/lib.rs` wires both.
- `deploy/k8s/base` (command Deployment and Service, Ingress path), both overlays, `.github/workflows/deploy-cloud.yml`, `deploy/README.md`, `scripts/check-deploy.test.mjs`.

## Tasks & Acceptance

- [x] Proto types, cloud and Hub migrations, store modules.
- [x] Cloud command role: submit, fetch, result, heartbeat poke, `--role command`, Kubernetes workload.
- [x] Hub: command-to-intent mapping, `apply_command`, `owner_wins`, `CommandTask`, heartbeat poke, wiring.
- [x] Tests: cloud (`tests/pair.rs`), store, core (`apply/tests/commands.rs`), tasks, and an end-to-end hub plus cloud test (`arena-hub/tests/pairing.rs`).

**Acceptance Criteria:**
- Given a submitted setup command, when the same key is submitted again, then the same row comes back; any other kind returns `UNSUPPORTED`.
- Given a paired Hub, when a command is pending, then the next heartbeat carries `pending_commands` and the Hub fetches, applies once as `owner` and posts the result, after which the row is `applied` or `rejected` with the error.
- Given the Hub was offline or a result post was lost, when it reconnects, then it applies the backlog in `id` order, re-posts unreported results and never applies a command twice.
- Given an owner command with a stale `base_rev`, when it applies, then it overwrites and the change log keeps both writes.

## Verification

- `cargo fmt --check`; clippy for the workspace and for `arena-hub`/`arena-api` with `--features dev-fixtures`; `cargo test --workspace --locked`; `pnpm run check:*` (generated types pass once committed); `test:scripts`; prettier; Hub `check`.

## Design Notes

Decisions (recommended option taken): the Hub's dedupe table holds both the id and the cloud key (each unique) so a rebuilt mailbox cannot replay a command, and the cursor is derived from it rather than stored; results are reported from `applied_commands.reported_at`, so a lost post is simply retried on the next poll; the intent reuses the cloud's idempotency key so `applied_intents` is a second line of defence; the command role mounts only the mailbox routes while `control` and `all` keep serving them for dev and tests; the Kubernetes command Deployment copies the control Deployment's environment because the binary loads one auth configuration for every role.

Known limits: the Hub has no `branch.name` setting yet, so a `settings.set` for it is rejected `VALIDATION_FAILED key` until Story 2.30; the "updated by owner" toast exists only for groups, zones and device placement (the `groups.changed` event) and fires on every owner write there, not only an overwrite; the `commands` and `applied_commands` rows are never pruned (the sweeper is Epic 8).

## Review Triage Log

### Review Findings

- [x] [Review][Patch] Heartbeat failed outright when the pending-commands count failed; it now logs and answers without the hint [crates/arena-cloud/src/api/heartbeat.rs:143]
- [x] [Review][Patch] Comments added by the story removed (global no-comments rule) [crates/arena-core, arena-cloud, arena-store, deploy]
- [x] [Review][Patch] No test applied `device.register`, `staff.upsert` or `device.assign` with a stale `base_rev`; added `owner_staff_and_device_edits_win_over_a_stale_base_rev` [crates/arena-core/src/apply/tests/commands.rs]
- [x] [Review][Defer] A cloud restore that rewinds `commands.id` can collide with `applied_commands` ids or keys and the cursor; the by-key hit returns the old row so the new id is refetched every poll — deferred: needs a decision on cursor survival after a cloud restore
- [x] [Review][Defer] A command that always fails `INTERNAL` blocks every later command (no retry cap or quarantine) — deferred: design choice, belongs with the Epic 8 sweeper
- [x] [Review][Defer] A non-`NotFound` 4xx on a result post stalls the mailbox; the Hub only ever sends well-formed results — deferred
- [x] [Review][Defer] `check-deploy` does not check the command Deployment, the `/v1/commands` Ingress path or its rollout step; the command pod carries secrets it does not use; no smoke or rate limit on the mailbox routes — deferred: deploy tooling
- [x] [Review][Defer] Missing tests: 50-row page boundary, `INTERNAL` raised inside the write transaction, a restart check that proves the dedupe — deferred
- [x] [Review][Defer] `submit_command` has no caller yet, so `issued_by` and unknown-branch handling are validated by the later wizard stories; a repeated key with a different payload returns the first row — deferred

Rejected: result body with neither or both of `applied_seq` and `error` (the Hub may legitimately send no seq for a no-op; it never sends both); ULID case normalisation (the Hub dedupes by canonical key); unrelated formatting churn (cargo fmt output); the spec's "through `apply()`" wording (`run()` is what `apply()` wraps).
