---
title: 'Story 9.21: Promote the standby to main with a manager PIN'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '7201346b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-20-the-standby-keeps-its-own-catalogue-media-and-licence-and-sh.md'
warnings: []
deferred:
  - 'Running-Hub wiring (same cause as Stories 9.19 and 9.20): building a `Promoter` in a Hub binary that runs as a standby, `UiAuth::set_promote` with a hook that opens the writer on the promoted database, starts the lease, heartbeat, mDNS and upload tasks, calls `Promotion::publish` and submits `lease.promote` as the Manager, and the restart of the follower after a failed promote (`PromoteError::follower_stopped`). Needs the standby runtime role and the 9.18 LAN link, which do not exist yet. Every piece below is built and tested without them.'
  - 'The Hub UI: "Promote to main" and the PIN in place on the standby screen, the `LEASE_FRESH` message, Settings -> Kitchen "Hub changed — re-pair" rows (`KitchenDisplayView.pair_again` is set by the promote), "Pair again" binding a fresh 6-digit code to the existing display, the pairing screen title, the board loading from the new main, the kitchen board banner "Hub changed? Re-pair at the counter" after 60 s, and the System tile "Former main — offline".'
  - 'Kiosk wiring: `EpochFloor` (arena-kiosk-core) and `HubClient::admit_epoch` refuse a lower epoch and are tested; the kiosk service must keep the floor on disk, check it after every `hello.ok`, and re-resolve by mDNS (`TXT_EPOCH`, `txt_epoch`) with the 2-30 s backoff. `DiscoveredHub` does not carry the epoch yet.'
  - 'The old main Hub on the System screen and the audit read of `lease.promote` (Story 9.23 lists what was set aside; 9.24 locks a returning old main).'
---

<intent-contract>

## Intent

**Problem:** A standby follows the main Hub (Stories 9.19-9.20) but nothing lets a Manager make it the main Hub when the main PC dies, so the floor stops until the PC is replaced.

**Approach:** `lease.promote` runs on the standby after a Manager's PIN: it checks the main Hub's lease has run out and the schema is not behind, stops the follower, claims a new epoch from the cloud with the fork `{old epoch, last replicated seq}` (provisional after 3 s of silence), re-applies every erasure, takes the lease and marks the kitchen displays to be paired again. The cloud swaps the roles in the same compare-and-set. Devices learn the new epoch from `hello.ok` and the mDNS TXT record and refuse any lower one.

## Boundaries & Constraints

**Always:**
- Promotion is manual. The main Hub's lease is stale after 15 s without a frame (the main Hub pings every 10 s); fresher is `LEASE_FRESH` with "The main Hub is still running. Check it before promoting." and nothing changes, the follower included.
- A standby whose schema is below the main Hub's last stated schema is `SCHEMA_BEHIND`, checked before anything stops. The main Hub's schema comes from its `ready` frame; a refusal as behind records a lower bound (own schema + 1).
- The claim is `POST /v1/fence/claim` with `reason: promote` and the standby's own Hub token: `expected` is the old main's epoch (the standby's newest change row), once more with the fence the cloud names if it answers `FENCED`. `Unreachable`, a 5xx and no answer within 3 s promote on a provisional epoch `max seen + 1`; any other refusal is reported and the follower is left stopped for the caller to restart.
- In the cloud one transaction moves the fence, re-queues commands past the fork, revokes the old main Hub's row and makes the standby the branch's main Hub. A standby token may claim the fence for `promote` only.
- One database transaction on the standby: erasures re-applied (player rows and `player.erased` change rows, keys deleted), lease taken (provisional unless the cloud issued the epoch), fork written for the first upload batch, highest counter raised, every live kitchen display marked `!pair-again`. Kiosk tokens are untouched: the standby already holds them and its fingerprint was pinned.
- The PIN is always required (`ops.promote`; Manager and Owner hold it): the signed-in member's own, or another holder's when a Counter is signed in. A wrong PIN counts against the same lockout as a restore.
- `lease.promote` is recorded once by the Manager, sets `ops.last_promote`, and its change row is the audit entry: old and new epoch, fork, lag, erasures, displays, `force: false`.

**Never:**
- No auto-promote, no force promote (Story 9.22), no old-main lock-out or demotion (9.24, 9.25), no list of what was set aside (9.23).

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Main dead 20 s | no frame for 20 s, cloud answers | claim `4:STBY` won, lease non-provisional |
| Main alive | frame 5 s ago | `LEASE_FRESH`, follower keeps running |
| Cloud down | no answer in 3 s | provisional `max seen + 1` |
| Fence moved | `FENCED` naming `5:X` | claim again from `5:X`; second `FENCED` fails `FENCED` |
| Schema behind | main stated 91, standby 90 | `SCHEMA_BEHIND`, nothing stops |
| Counter signed in | Manager's PIN | promote runs, audit names the Manager |
| Counter, own PIN or none | | `PERMISSION_DENIED` |
| Kitchen displays | 2 live, 1 revoked | 2 marked `pair_again`, revoked one untouched |
| Lower epoch Hub | device floor `4:STBY`, Hub `3:MAIN` | `FENCED`, floor unchanged |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{promote.rs,error.rs,intent.rs,replicate.rs,frame.rs,discovery.rs,lib.rs}` -- `LeasePromote`, `PromoteRequest`, `Promoted`, `main_lease_fresh` and the 15 s / 3 s / 10 s constants, `ErrorCode::LeaseFresh`, intent `lease.promote` (`ops.promote`), `Ready.main_schema`, `HelloOk.epoch`, `TXT_EPOCH`, `txt_epoch`.
- `crates/arena-core/src/apply/lease_promote.rs`, `settings_defaults.rs` -- the record and `ops.last_promote`.
- `crates/arena-store/src/{promote.rs,control/hubs.rs}` -- `promote_basis`, `promote_to_main`, `record_main_schema`; `promote_standby_fence`.
- `crates/arena-cloud/src/api/fence.rs` -- standby claims for `promote`.
- `crates/arena-tasks/src/{promote.rs,follow.rs,standby_progress.rs,mdns.rs}` -- `Promoter`, `Promotion`, `CloudClaimer`; the follower records the main Hub's schema; `silent_ms`; the epoch in the TXT record.
- `crates/arena-api/src/{ui_promote.rs,ui_auth.rs,lan/session.rs,lan/replicate.rs}` -- `POST /ui/standby/promote`, `pin_holder`, `PromoteHook`; `hello.ok` carries the epoch; `ready` carries the schema.
- `crates/arena-kiosk-core/src/{epoch.rs,client.rs}` -- `EpochFloor`, `HubClient::admit_epoch`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- promote types and constants, `LEASE_FRESH`, `lease.promote`, `ready.main_schema`, `hello.ok.epoch`, `TXT_EPOCH`; unit tests
- [x] `arena-core` -- apply `lease.promote` (audit record, `ops.last_promote`), proptest generator; tests
- [x] `arena-store` -- promote transaction (erasures, lease, fork, displays), main schema tracking, cloud role swap; tests
- [x] `arena-cloud` -- fence claim by a standby for `promote`; tests in `tests/pair.rs`
- [x] `arena-tasks` -- `Promoter` (lease check, schema check, stop, claim with 3 s wait and one `FENCED` retry, transaction), follower records the main schema, mDNS epoch; tests
- [x] `arena-api` -- `POST /ui/standby/promote` with PIN, `hello.ok` epoch; tests
- [x] `arena-kiosk-core` -- `EpochFloor`, `admit_epoch`; tests
- [ ] Standby runtime wiring, Hub and kitchen UI, kiosk service wiring (deferred, see front matter)

**Acceptance Criteria:**
- Given a standby whose main Hub has been silent 15 s, when a Manager confirms with a PIN, then it claims a new epoch with the fork, re-applies erasures, takes the lease and the cloud makes it the main Hub; the epoch reaches devices in `hello.ok` and the mDNS TXT record.
- Given a main Hub heard within 15 s, then `LEASE_FRESH` and nothing changes.
- Given paired kitchen displays, then each is marked "pair again" and none keeps a usable token.
- Given a device that has seen a higher epoch, then a Hub with a lower one is refused `FENCED`.
- Given a promote, then `lease.promote` records the Manager, both epochs, the fork and the lag.

## Spec Change Log

## Review Triage Log

- Dev verification (2026-10-31): `cargo clippy --workspace --all-targets -- -D warnings` clean (minus `arena-e2e`, `arena-kiosk-svc`); `cargo fmt --check`; tests for arena-proto, arena-store, arena-core (incl. the `proptest` generator with `lease.promote`), arena-tasks, arena-api, arena-kiosk-core, arena-cloud, arena-replay and arena-hub: 3,618 pass. `pnpm run gen:types` (3 new types), `check:strings`, `check:protocol` and prettier on the generated folder are clean.
- Failures that are not from this story: `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings` and `arena-cloud` `binary::unreachable_at_start_retries_then_exits_1...` (both on the baseline, see Story 9.19); `arena-tasks` `printer::lan::tests::a_printer_that_stops_reading_times_out` and `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` (socket-buffer and monotonic-clock assumptions on this Windows host; the files are untouched); `arena-cloud` `backup::a_hung_run_times_out_with_its_own_code` failed once under load and passes alone. `check:layers` reports `arena-api -> arena-store` and `arena-tasks -> arena-replay`, edges that predate this story (no `Cargo.toml` changed). `check:generated` needs the new generated files committed.

## Design Notes

The standby cannot read the main Hub's lease row, but it holds a stream the main Hub pings every 10 s while it runs, so silence is the lease. A frozen main Hub is silent too, which is what the Manager wants to promote past; a main Hub that is alive but cut off from the standby by the network looks the same, and the cloud's compare-and-set is what keeps two Hubs from both winning the fence.

The cloud revokes the old main Hub's row rather than keeping it as a second role: the branch keeps one live Hub per role (a unique index), a revoked token cannot upload, and Stories 9.24-9.25 own what a returning old main shows and how it becomes the standby.

The kitchen displays are marked directly in the database, as a restore does, not through `kds.*` intents, so the mark is local to the promoted Hub's database (`KitchenDisplayView.pair_again` reads it) and a later replay of its change log does not carry it. The new main's kiosks keep their tokens: the standby replicated the `devices` rows.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-tasks -p arena-api -p arena-kiosk-core -p arena-cloud`, `cargo clippy --workspace --all-targets -- -D warnings` (minus `arena-e2e`, `arena-kiosk-svc`), `cargo fmt --check`
- `pnpm run gen:types`, `pnpm run check:strings`

### Review Findings

Code review (2026-10-31): 4 layers (Blind Hunter, Edge Case Hunter, Verification Gap, Acceptance Auditor).

- [x] [Review][Patch] Fork read before the follower stopped; the claim could carry a different fork than the database records [crates/arena-tasks/src/promote.rs] — the basis is read again after the stop (it waits on the store lock a batch in flight holds)
- [x] [Review][Patch] The 3 s wait applied per claim attempt, so a `FENCED` retry could take 6 s before the provisional fallback [crates/arena-tasks/src/promote.rs] — one deadline for both tries
- [x] [Review][Patch] Erasure re-apply order was undefined for a player in both sources, so the "row's own time" claim did not hold [crates/arena-store/src/promote.rs] — `UNION ALL` with a source rank, ordered by it
- [x] [Review][Patch] No test that `hello.ok` carries the epoch of a real session [crates/arena-api/tests/lan.rs] — asserted in `paired_hello_records_the_version_and_pushes_applies`
- [x] [Review][Patch] `PromoteRequest.pin` and `MAIN_LEASE_STALE_MS` doc comments contradicted the route and the 10 s ping [crates/arena-proto/src/promote.rs] (generated TS refreshed)
- [x] [Review][Defer] A standby that replicated nothing promotes locally without a claim, so the cloud never swaps roles [crates/arena-tasks/src/promote.rs:253] — deferred: spec is silent on the empty case; a refusal or confirmation belongs with Story 9.22 (force promote)
- [x] [Review][Defer] A provisional promote is not settled in the cloud: heartbeat refuses a standby-role token and nothing re-sends the claim [crates/arena-tasks/src/promote.rs] — deferred: needs the running-Hub wiring (heartbeat after promote), listed in the spec's deferred work
- [x] [Review][Defer] Two concurrent or repeated `POST /ui/standby/promote` calls have no once-only guard [crates/arena-api/src/ui_promote.rs] — deferred: belongs to the promote hook the Hub wiring builds (`UiAuth::set_promote`)
- [x] [Review][Defer] Cloud claim succeeded but `promote_to_main` then fails, leaving a stopped follower with the cloud roles already swapped; also the cloud role-swap failure surfaces as `FENCED` naming an unchanged fence [crates/arena-tasks/src/promote.rs, crates/arena-store/src/control/hubs.rs] — deferred: rare partial failures, pin with tests when the wiring lands
- [x] [Review][Defer] Follower schema-ahead notes on the mid-stream and snapshot paths, and `EpochFloor::admit(None)`, have no integration coverage / let a Hub with no stated epoch through [crates/arena-tasks/src/follow.rs, crates/arena-kiosk-core/src/epoch.rs] — deferred: kiosk and runtime wiring are not built yet; add scripted-link tests then
- [x] [Review][Defer] Cloud does not check the main Hub's staleness itself and a standby claim shares the branch's rate key [crates/arena-cloud/src/api/fence.rs] — deferred: manual promote by design (the compare-and-set prevents two winners); revisit with Story 9.22

#### Rejected

- A 429 from the cloud aborts the promote — false: the spec says "any other refusal is reported"; only 5xx, unreachable and no answer are provisional.
- `lease.promote` audit record can be forged by any `ops.promote` holder — low: the holder is a Manager/Owner who can already write settings; a guard would add state checks for no named harm.
- Log wording changes in `pin_holder`, `set_promote` ignoring a second call, duplicated schema check in store and promoter, `debug_assert!` on `repaired`, `ReplicationLink::main_schema` default — low or cosmetic, not worth added complexity.
- `PROMOTE_BUDGET_MS` unused — false: it is a documented budget constant, not a runtime limit the spec asks to enforce.

Verification after fixes: clippy clean (arena-proto, arena-store, arena-tasks, arena-api, all targets, `-D warnings`), `cargo fmt --check`, promote tests in arena-proto/store/tasks/api and the lan hello tests pass, `check:strings` ok, `gen:types` refreshed.
