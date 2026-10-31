---
title: 'Story 9.22: Force promote after a data-loss warning'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '051c3775'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-21-promote-the-standby-to-main-with-a-manager-pin.md'
warnings: []
deferred:
  - 'Running-Hub wiring (same cause as Stories 9.19-9.21): the `PromoteHook` the Hub sets with `UiAuth::set_promote` must now take `(manager, force)` and call `Promoter::promote` or `Promoter::promote_forced`, turn a `PromoteError` into `PromoteFailure` with `PromoteError::failure`, open the writer, and submit `lease.promote` from `Promotion::record` (`force` and `force_reason` are already set). Needs the standby runtime role, which does not exist yet.'
  - 'Feeding `SeenEpochs`: `LeaseTask::seen_epochs()` is the input the old main Hub reads at every tick. Nothing calls `note`/`note_text` yet: the LAN listener (`epoch.changed` from the new main, a device presenting a higher epoch in its hello) and the mDNS browser (`TXT_EPOCH`) must. A restart of the old main still renews its own lease row; the self-lock on return belongs to Story 9.24.'
  - 'The Hub UI: the standby screen showing `ForceOffer` (message and `lag_text`) after a `LEASE_FRESH` or refused-claim answer, the "Force promote" action opening `inline-pin` with the acknowledgement, no action on `SCHEMA_BEHIND` or `FENCED`.'
  - 'Cloud side: nothing new beyond the kind. `standby_forced` is accepted through `POST /v1/notify` and raised through the Story 8.33 path like the other Hub kinds; the Owner app lists it as any alert (deep link `/#today-<branch>`).'
---

<intent-contract>

## Intent

**Problem:** A main Hub that is frozen but not dead keeps a fresh lease for as long as it answers, so "Promote to main" is refused `LEASE_FRESH` and the floor cannot move to the standby. The same happens when the cloud will not confirm the claim. The Manager has no way past it, and the owner is never told when somebody takes over by force.

**Approach:** The refusal carries a `ForceOffer` ("Data since the last sync may be lost. Promote anyway?" and the lag in rows). A Manager who types their PIN and ticks the data-loss acknowledgement sends the request again with `force`. `Promoter::promote_forced` runs the Story 9.21 steps without the freshness check, and on a provisional epoch when the cloud refuses. The record `lease.promote` says `force: true` and what it went past, and in the same transaction queues the owner alert `standby_forced`. A Hub that is told of a higher epoch stops writing at its next lease tick.

## Boundaries & Constraints

**Always:**
- Force is offered on `LEASE_FRESH` and on a refused cloud claim (any refusal but `FENCED`). It is never offered, and never accepted, on `SCHEMA_BEHIND` (checked first, before the lease) or on `FENCED` (a newer Hub already holds the fence).
- Force needs the manager's PIN (`ops.promote`, same holders and lockout as 9.21) and `data_loss_acknowledged: true`; without the acknowledgement the route answers `VALIDATION_FAILED` (`data_loss_acknowledged`) before anything starts.
- A forced promote runs every other 9.21 step: follower stop, fork, claim, erasures re-applied, lease taken, kitchen displays marked. A claim that times out or fails in the cloud promotes provisionally, as in 9.21; one the cloud refuses is provisional only when forced.
- `LeasePromote.force_reason` is set exactly when `force` is: `LEASE_FRESH`, the code of the cloud's refusal (comma-separated when both), or `FORCED` when nothing stood in the way.
- `lease.promote` with `force: true` queues one `notify_outbox` row of purpose `owner_alert`, kind `standby_forced`, param `manager`, `source_ref` the new epoch, due after the Undo window, through the Story 8.34 path (paid tiers only; once per new epoch). The cloud accepts the kind from a Hub and raises it like the others.
- The lease task compares the highest epoch it was told of (`SeenEpochs`) with its own at every tick: higher means the writer closes at once, the counter is kept in `max_seen_counter`, and the Hub publishes as a follower of that epoch. That is at most one renewal tick (3 s).

**Never:**
- No auto-promote. No old-main lock-out or demotion screen (9.24, 9.25), no list of what was set aside (9.23).

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Main alive | frame 5 s ago, no force | `LEASE_FRESH` with `force_offer {lag_rows, lag_text}`; follower keeps running |
| Force on a live main | same, `force` + acknowledgement + PIN | promotes; `force_reason: LEASE_FRESH`; owner alert queued |
| Claim refused (429) | stale lease | refusal reported with the offer; forced: provisional epoch, reason `RATE_LIMITED` |
| Force without acknowledgement | | `VALIDATION_FAILED` on `data_loss_acknowledged`, nothing starts |
| Force without PIN / Counter's own PIN | | `PERMISSION_DENIED` |
| Schema behind | main stated 91, standby 90, lease fresh | `SCHEMA_BEHIND`, no offer, force refused the same |
| Newer Hub holds the fence | `FENCED` twice | `FENCED`, no offer, force refused the same |
| Old main still running | new main's epoch told to it | closes the writer at its next tick, follower of that epoch |
| Free branch | forced promote | record written, no alert row |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{promote.rs,owner_alert.rs}` -- `PromoteRequest.force` / `data_loss_acknowledged`, `ForceOffer`, `PromoteRefused`, `PromoteFailure`, `lag_text`, `FORCE_PROMOTE_MESSAGE`, `LeasePromote.force_reason`; owner alert kind `standby_forced` (hub sourced, param `manager`).
- `crates/arena-core/src/apply/lease_promote.rs`, `mutate.rs` -- `force_reason` consistency; the owner alert queued in the record's transaction.
- `crates/arena-store/migrations/0091_standby_forced_alert.sql`, `migrations.rs`, `export-reference/meanings.txt` -- `notify_outbox.kind` gains `standby_forced` (schema 91).
- `crates/arena-tasks/src/{promote.rs,lease.rs}` -- `Promoter::promote_forced`, `PromoteError.force_offer`, `Promotion.force_reason`; `SeenEpochs` and the tick check.
- `crates/arena-api/src/{ui_promote.rs,ui_auth.rs}` -- the route reads `force` and the acknowledgement, answers `PromoteRefused`; `PromoteHook` takes `(manager, force)`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- force request fields, `ForceOffer`, `PromoteRefused`, `PromoteFailure`, `force_reason`, `standby_forced` kind; unit tests
- [x] `arena-store` -- migration 0091 and export reference; reference doc regenerated
- [x] `arena-core` -- `lease.promote` queues `standby_forced` when forced, validates `force_reason`; tests; proptest generator updated
- [x] `arena-tasks` -- `promote_forced`, offers on `LEASE_FRESH` and refused claims, schema check first; `SeenEpochs` in the lease task; tests
- [x] `arena-api` -- route force handling, acknowledgement, `PromoteRefused` body; tests
- [x] `arena-cloud` -- accepts and raises `standby_forced` from a Hub; tests in `tests/owner_alerts.rs`
- [x] Generated TS refreshed (`ForceOffer`, `PromoteRefused`, `PromoteRequest`, `LeasePromote`, `OwnerAlertKind`)
- [ ] Standby runtime wiring, Hub UI, feeding `SeenEpochs` from the LAN and mDNS (deferred, see front matter)

**Acceptance Criteria:**
- Given `LEASE_FRESH` or a refused claim, then the answer carries "Data since the last sync may be lost. Promote anyway?" and the lag ("12 changes not yet copied").
- Given a manager PIN and the acknowledgement, then `promote_forced` takes the lease regardless of freshness and runs the 9.21 steps; on `SCHEMA_BEHIND` force is not offered.
- Given a forced promote, then the record has `force: true`, the manager, the lag and the reason, and one `standby_forced` owner alert is queued and drained when the WAN is next up.
- Given an old main still running that is told of the higher epoch, then it stops writing within one lease tick.

## Spec Change Log

## Review Triage Log

- Dev verification (2026-10-31): `cargo clippy --workspace --all-targets -- -D warnings` clean (minus `arena-e2e`, `arena-kiosk-svc`); `cargo fmt --check`; tests for arena-proto, arena-store, arena-core, arena-tasks, arena-api, arena-kiosk-core, arena-cloud, arena-replay, arena-hub pass apart from the failures below. `pnpm run gen:types`, `check:strings`, `check:protocol` are clean.
- Failures that are not from this story (all on the baseline): `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings`; `arena-cloud` `binary::unreachable_at_start_retries_then_exits_1...` and `backup::a_hung_run_times_out_with_its_own_code`; `arena-tasks` `printer::lan::tests::a_printer_that_stops_reading_times_out` and `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` (Windows host timing assumptions).
- `arena-api/tests/replicate.rs` hard-coded schema 90 as the follower's schema; it now uses `LATEST_SCHEMA_VERSION` so the next migration does not break it.

## Design Notes

Force is the same promote with two checks relaxed, not a second code path: `run(force)` is shared, and `went_past` collects what was relaxed so the record can say so. The schema check moved ahead of the lease check so that a standby that is behind is told `SCHEMA_BEHIND` straight away and never shown a force offer that would then fail.

The offer is built where the lag is known (the follower's progress report) and travels as `PromoteFailure` from the hook to the route, which flattens the usual `UiError` and adds `force_offer`, so existing clients that read `code` and `message` are unaffected.

The alert is queued inside the `lease.promote` transaction, like the other Hub alerts, so it is exactly as durable as the record and is raised once per new epoch (`(kind, source_ref)` is unique). The alert text is "The standby Hub was forced to main by <manager>. ..."; the cloud puts the branch name in the title of every alert, which gives "Standby forced to main at <branch> by <manager>" as the owner sees it.

`SeenEpochs` is a plain shared maximum rather than a channel: the lease task only needs the latest answer at each tick, and a higher epoch never becomes lower. It does not write the lease row, so a restart of the old main is still 9.24's to lock.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-api -p arena-kiosk-core -p arena-cloud`, `cargo clippy --workspace --all-targets -- -D warnings` (minus `arena-e2e`, `arena-kiosk-svc`), `cargo fmt --check`
- `pnpm run gen:types`, `pnpm run check:strings`, `pnpm run check:protocol`
- `ARENA_UPDATE_DOCS=1 cargo test -p arena-store --test export_reference` after a migration

- Code review (2026-10-31): blind, edge-case and acceptance passes over `5b50e80d` found nothing blocking. Checked: the acknowledgement check runs after the session is authenticated and before the PIN; `SCHEMA_BEHIND` is checked before the lease, and `FENCED` never gets an offer or a forced claim; `force_reason` is set exactly when `force` is; the alert is queued in the `lease.promote` transaction; `SeenEpochs` demotes within one tick. `cargo fmt --check`, clippy `-D warnings` and tests for arena-proto, arena-store, arena-core, arena-tasks and arena-api pass.
