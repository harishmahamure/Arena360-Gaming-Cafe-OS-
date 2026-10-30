---
title: 'Story 9.3: The cloud issues epoch counters by compare-and-set on the branch fence'
type: 'feature'
created: '2026-10-30'
status: 'in-review'
baseline_commit: '23197aaf90dc003b2e85ea6dbcddcaac4d1f89f1'
baseline_revision: '23197aaf90dc003b2e85ea6dbcddcaac4d1f89f1'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-2-player-keys-are-backed-up-and-the-branch-key-is-escrowed.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The branch fence is only "the first epoch a Hub reported". A Hub that takes over (promote, restore, a lease taken offline) cannot get a counter from the cloud, a provisional epoch is never settled against a higher fence, and the re-queue of owner commands has no real fork point.

**Approach:** One compare-and-set on `branch_fence` is the only place an epoch counter is issued. `POST /v1/fence/claim` and the heartbeat both go through it; the heartbeat settles a provisional epoch (confirm, or answer with the higher fence so the Hub demotes). The Hub remembers the highest counter it has seen in `lease`, so an offline mint is `max seen + 1`.

## Boundaries & Constraints

**Always:**
- The epoch text stays `<counter>:<instance>`. A claim makes `(expected.counter + 1):<instance_id>`; the CAS matches the `expected` text exactly, else `FENCED` naming the current fence and nothing is written.
- `fork_seq` of the claim (`fork.parent_seq`) is stored on the fence row and is the fork point of the command re-queue (`move_branch_fence`, now `claim_branch_fence`, in the same transaction as the CAS).
- Heartbeat: fence equal to the Hub's epoch confirms; a Hub counter above the fence's moves the fence to the Hub's epoch by CAS (with the heartbeat's `fork_seq`) and confirms; same counter with another instance, or a Hub counter below the fence, is unconfirmed and carries the fence. The Hub demotes (closes the writer, `NOT_PRIMARY` for LAN intents) when unconfirmed and the fence is not below its own epoch.
- `branch_fence` is written only in `arena_store::control::hubs` (`pair_hub` reset, `heartbeat` first insert, `claim_branch_fence`) and read by other roles.
- The Hub raises `lease.max_seen_counter` (never lowers it) from heartbeat answers, claim replies and, when LAN epochs exist, the LAN. A lease taken from another instance's expired row uses `max(row counter, max seen) + 1`.

**Never:**
- No promote, restore, standby or self-lock UI (Stories 9.7, 9.21, 9.24); no LAN epoch is read yet, only the writer call that records one.
- No `branch_fence` write from the command, builder or jobs roles; no change to epoch text or batch keys.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Claim wins | `expected` = fence `3:A`, instance `B` | fence `4:B`, `fork_seq` stored, `{epoch:"4:B"}`, commands past the fork re-queued | none |
| Claim stale | `expected` older than the fence | `FENCED`, message names the fence, nothing written | Hub records the fence counter as seen |
| Two claims at once | same `expected` | exactly one wins, the other `FENCED` | none |
| Bad body | unparsable `expected`, instance or parent epoch, negative seq | `VALIDATION_FAILED` | none |
| Offline mint then beat | Hub `2:B`, fence `1:A` | fence moves to `2:B`, `confirmed:true`, Hub clears `provisional` | none |
| Same counter elsewhere | Hub `2:B`, fence `2:A` | `confirmed:false`, fence `2:A`; Hub demotes | none |
| Higher fence | Hub `1:A`, fence `3:C` | `confirmed:false`; Hub demotes, notes counter 3 | none |
| Offline lease take | expired row counter 2, max seen 5 | new provisional counter 6 | none |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0028_fence_fork.sql`, `src/control.rs`, `src/control/hubs.rs` -- `branch_fence.fork_seq`; `claim_branch_fence` (CAS + re-queue) replaces `move_branch_fence`; `FenceMove`.
- `crates/arena-proto/src/fence.rs` (new), `lib.rs` -- `FenceClaimRequest/Response/Reason/Fork`, `fenced_epoch` (reads the epoch out of a `FENCED` message).
- `crates/arena-cloud/src/api/fence.rs` (new), `api/mod.rs`, `api/heartbeat.rs` -- the route; heartbeat settlement. `src/presign.rs` `Epoch` is reused for ordering.
- `crates/arena-store/migrations/0084_lease_max_seen.sql`, `src/lib.rs` -- `lease.max_seen_counter`, `read_max_seen_counter`, `raise_max_seen_counter`.
- `crates/arena-core/src/writer.rs` -- `note_epoch_seen`, `max_seen_counter`.
- `crates/arena-tasks/src/lease.rs` -- `decide` mint rule; `src/fence.rs` (new) -- `claim_fence`; `src/cloud.rs` -- `fence_claim`; `src/heartbeat.rs` `accept` -- demote and note.
- Tests: `arena-cloud/tests/pair.rs` (`a_later_beat_with_another_epoch_is_not_confirmed` changes meaning), `tests/owner_commands.rs` `set_fence`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-store` -- control migration 0028, `claim_branch_fence`, Hub migration 0084 and max-seen functions, tests
- [x] `arena-proto` -- claim types, `fenced_epoch`; `pnpm gen:types`
- [x] `arena-cloud` -- `POST /v1/fence/claim`; heartbeat confirm/advance; integration tests incl. concurrent claims
- [x] `arena-core` / `arena-tasks` -- writer calls, lease mint rule, `claim_fence`, heartbeat demotion, tests
- [x] Update existing tests whose meaning changed (`pair.rs`, `owner_commands.rs`)

**Acceptance Criteria:**
- Given a Hub with a valid device token, when it claims with the current fence, then the fence becomes `counter+1` with the claimer's instance and `fork_seq`, and a stale `expected` gets `FENCED` with the current fence and no write.
- Given two simultaneous claims, then exactly one succeeds.
- Given a provisional epoch and its first heartbeat, then it is confirmed when no higher fence exists, and otherwise the answer carries the higher fence and the Hub closes its writer and refuses LAN intents with `NOT_PRIMARY`.
- Given two Hubs that minted the same counter offline, then the first CAS wins and the second is `FENCED` and demotes.
- Given a committed claim, then commands applied past its `fork_seq` return to `pending` using the real fork point.
- Given any other role, then it only reads `branch_fence`.
- Given an epoch seen in a heartbeat or claim reply, then `lease.max_seen_counter` holds the highest, and an offline lease take mints `max seen + 1`.

## Spec Change Log

## Review Triage Log

## Design Notes

- A `confirm` claim follows the same arithmetic as the others; the heartbeat is the normal confirm path because it settles the Hub's own minted epoch exactly, not `counter+1`.
- Demotion on the Hub mirrors the commands task: close the writer, set the role to follower; the lease task then sees `NOT_PRIMARY` and publishes its own demotion.
- `max_seen_counter` is a lease column read and raised by separate functions so `LeaseRow` (47 literals) stays unchanged.

## Verification

**Commands:**
- `cargo test -p arena-store --features control -p arena-proto -p arena-core -p arena-tasks -p arena-cloud` -- expected: pass except failures that also fail on the baseline
- `cargo clippy -p arena-store -p arena-proto -p arena-core -p arena-tasks -p arena-cloud -p arena-hub --all-targets -- -D warnings` -- expected: clean
- `pnpm gen:types`, `pnpm run check:layers check:pins check:protocol check:secrets check:strings` -- expected: clean

## Auto Run Result

Status: in-review

Implemented per spec. Failures seen that are not from this story (they are on the 9.2 baseline list): `arena-tasks` `heartbeat::tests::trusted_time_is_cloud_time_plus_monotonic_elapsed` and `printer::lan::tests::a_printer_that_stops_reading_times_out`; `arena-cloud` `backup` `a_hung_run_times_out_with_its_own_code` and `binary` `unreachable_at_start_retries_then_exits_1_without_logging_the_token`.

Judgement calls: a Hub demotes when unconfirmed and the fence counter is at least its own (the matrix row `2:B` vs `2:A`), so two Hubs never serve one counter; a claim before any heartbeat gets `FENCED` with no epoch; a heartbeat advance without `fork_seq` re-queues only `fetched` commands. Claims are rate-limited to 60 an hour per branch. Nothing calls `claim_fence` yet (promote and restore are Stories 9.7 and 9.21), and no LAN epoch is read yet; `note_epoch_seen` is the call for it.
