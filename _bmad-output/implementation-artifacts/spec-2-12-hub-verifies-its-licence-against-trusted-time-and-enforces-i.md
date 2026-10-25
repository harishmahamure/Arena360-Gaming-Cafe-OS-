---
title: 'Story 2.12: Hub verifies its licence against trusted time and enforces its limits'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'b736a8b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-11-hub-checks-in-by-heartbeat-and-the-wizard-advances-by-itself.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Hub verifies a licence only when a heartbeat arrives and throws the result away. Nothing checks that the licence names this branch, nothing keeps time the PC clock cannot move, `licence.flags` (read by Story 7.1) and `licence.device_cap` (read by Story 2.21) are never written, and staff are never told when something is wrong.

**Approach:**
- One entitlement evaluation (`Entitlement::evaluate`) turns the cached licence, the two compiled-in verify keys, the paired branch and tenant, and `trusted_now` into a tier, flags and limits. Anything that fails is Free limits.
- The Hub writes that outcome to branch settings through one system intent, `licence.apply`, so `apply()` gates read it through one API (`entitlement::limit`, `entitlement::has_flag`) and every change is an audit row.
- The heartbeat task keeps a trusted clock (cloud time plus monotonic time), persists it, and flags a wall clock more than 5 minutes off.

## Boundaries & Constraints

**Always:**
- **Hub migration** `0015_trusted_clock.sql`: `trusted_clock(id = 1, cloud_ms, mono_ms, wall_delta_ms)`.
- **Evaluation** (`arena-proto` `Entitlement::evaluate(jwt, keys, tenant, branch, trusted_now_ms)`):
  - no licence, bad signature, unknown key, malformed, issued in the future, or `tenant_id`/`branch_id` different from `hub_identity` ⇒ Free limits, standing `unverified`;
  - signature good and `exp` passed on `trusted_now`: a Free licence stays `verified` Free; a paid one ⇒ Free limits, standing `expired`, no warning;
  - otherwise the licence's tier, flags and limits, standing `verified`.
- **Settings written by `licence.apply`** (System actor `hub-licence`, no `base_rev`, inserted when missing, only rows that differ, nothing submitted when nothing differs): `licence.flags`, `licence.device_cap`, `licence.user_cap`, `licence.status` (`verified|expired|unverified`), `licence.clock` (`ok|wrong`). Each change is a change row, so each is an audit row.
- **Entitlement API in `apply()`** (`arena-core::entitlement`): `limit(tx, "devices"|"users")` and `has_flag(tx, name)`; the kitchen tier gate and the device cap use it.
- **Heartbeat** (`HeartbeatTask`):
  - stores the received licence only if it verifies against `hub_identity` at `cloud_time`; otherwise it is ignored, the standing is `unverified` (Free limits, warning) until a later licence verifies;
  - after every beat (success or failure) and once at start it re-evaluates the cached licence against `trusted_now` and publishes `HubStatus.{tier, licence, clock_wrong}`;
  - persists `trusted_clock{cloud_ms (trusted now at the beat), mono_ms (process uptime), wall_delta_ms (wall minus trusted)}` with the licence in one transaction;
  - at start restores `trusted_now = max(last cloud_ms, wall_now − wall_delta_ms)` and then counts monotonic time from there.
- **Trusted time:** `trusted_now = cloud_ms + monotonic elapsed`. Licence `exp` uses it, never the wall clock. Business timestamps keep using the wall clock.
- **Clock warning:** `|wall − trusted_now| > 5 min`, only once a cloud time is known; it clears when the clocks agree again. Both transitions write `licence.clock` (an audit row).
- **Expiry of a paid licence** affects only new work: caps and flags are checked on new registrations and new gated settings; nothing running is ended.
- **Tier table** (`arena-proto`): Basic and Premium gain the `kitchen` flag, which Story 7.1's gate reads; a genuine licence whose flags are not lower-case names (`a-z0-9_`, 256 characters in all) is `unverified`.
- **Missing rows:** with no `licence.device_cap`/`licence.user_cap` row, `limit()` answers the Free limit (20, 3), never "no cap".
- **Hub UI:** `/ui/snapshot` carries `licence_unverified` and `clock_wrong` (absent when false). The shell `hub-header` has one `status-badge` slot (role `status`) with "Licence couldn't be verified" and "PC clock is wrong", new `warning` badge state. Strings in `en.json`.
- **Tests:** evaluation matrix; `licence.apply` permission, validation and diff-only writes; the cap and kitchen gate through the API; heartbeat settle (wrong branch, recovery, clock wrong and recovered, 5-minute edge, expiry on trusted time with the wall moved ±30 days, trusted clock persisted and restored); `/ui/snapshot` flags; Hub Playwright for the slot.

**Never:**
- Locking a Windows Hub or ending a running session because of the licence.
- Trusting an unverified licence, or storing it as the cached one.
- Using the wall clock for licence expiry.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Genuine licence | signed, this branch, not expired | tier, flags, limits applied; `licence.status = verified` | N/A |
| Tampered / foreign key | bad signature or unknown `kid` | Free limits; badge "Licence couldn't be verified"; audit row | Hub keeps trading |
| Other branch | valid signature, other `branch_id` or `tenant_id` | same as tampered | same |
| Expired paid | `exp` passed on `trusted_now` | Free limits for new work, no badge, `licence.status = expired` | nothing ended |
| Expired Free | `exp` passed | Free, `verified`, no change | N/A |
| Wall back a week | wall moves, no heartbeat | expiry unchanged; "PC clock is wrong" + audit row | N/A |
| Wall off ≤ 5 min | skew 4 min | no badge | N/A |
| No cloud time yet | never heartbeated | no clock warning; expiry on wall clock | N/A |
| Restart | `trusted_clock` row | `trusted_now = max(cloud_ms, wall − delta)` | accepted limit below |
| Before bootstrap | no `branch` row | status published, settings not written, retried next beat | N/A |
| Non-primary Hub | follower | status published, nothing written | `NOT_PRIMARY` swallowed |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/licence.rs` (`verify_claims`, `check_time`), `entitlement.rs` (new: `Entitlement`, `LicenceStanding`, `LicenceApply`), `intent.rs` (`licence.apply`), `kitchen.rs` (keys), `snapshot.rs`.
- `crates/arena-store/migrations/0015_trusted_clock.sql`, `src/identity.rs`.
- `crates/arena-core/src/entitlement.rs` (new API), `apply/licence.rs` (new), `apply/{guards,devices}.rs` (use the API), `writer.rs` (`licence_state`, `apply_licence`, `record_heartbeat`), `status.rs`.
- `crates/arena-tasks/src/heartbeat.rs`, `crates/arena-hub/src/main.rs`, `crates/arena-api/src/health.rs`.
- `apps/hub/src/routes/+layout.svelte`, `packages/ui/src/components/StatusBadge.svelte`, `messages/en.json`.

## Tasks & Acceptance

- [x] proto: evaluation, intent, keys, snapshot flags, generated types.
- [x] store: migration 0015, trusted clock row.
- [x] core: entitlement API, `licence.apply`, writer methods.
- [x] tasks: settle, trusted clock persist/restore, genuine-only cache.
- [x] UI: header status-badge slot and strings.
- [x] tests.

**Acceptance Criteria:** the Given/When/Then of Story 2.12 in `epics.md`, as mapped by the matrix above.

## Verification

`cargo test --workspace --locked`; `cargo test -p arena-hub -p arena-api --features dev-fixtures`; clippy with dev-fixtures `-D warnings`; `cargo fmt --check`; every `pnpm run check:*`; `pnpm run test:scripts`; prettier; Hub `test:shell` (unique `ARENA_PW_PORT`); app builds except marketing; `pnpm check:generated` after the commit.

## Accepted limits

- `cloud_time` is plain JSON from the cloud over TLS; it is not signed (deferred from 2.11).
- A restart restores trusted time from the wall clock minus the saved delta; a wall change made while the Hub was off shifts it once (never earlier than the last cloud time).
- `Instant` pauses while a PC sleeps on some platforms, so trusted time can lag after a long suspend until the next heartbeat (a false "PC clock is wrong" is possible).
- `USER_LIMIT` is not enforced here: `licence.user_cap` is written; the staff story that counts staff rows enforces it.

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter with this spec, Verification Gap; on Sonnet). P = patch, D = defer, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | ECH: Basic and Premium licences never carry the `kitchen` flag, so the 7.1 gate stays closed for every real tier | high | P | `kitchen` added to both tier rows; tests `every_paid_tier_carries_the_kitchen_flag_and_free_does_not`, `an_applied_licence_opens_then_closes_the_kitchen_gate` |
| 2 | BH/ECH/VG: a genuine licence with a flag outside the grammar made `licence.apply` fail every beat and left the old paid rows in force | high | P | `evaluate` treats it as `unverified`; flag grammar tests |
| 3 | BH/ECH/VG: a missing `licence.device_cap` row meant no cap | medium | P | `limit()` falls back to the Free limits; `with_no_licence_rows_the_free_limits_apply` |
| 4 | ECH/VG: the "unverified" warning vanished on restart (`rejected` was in memory) | medium | P | `restore()` reads `licence.status`; `the_warning_survives_a_restart_until_a_licence_verifies` |
| 5 | ECH: an expired paid licence from the cloud was rejected and warned as unverified; a `licence_state()` error discarded a good licence | medium | P | only `Unverified` is rejected; an error skips the beat; `an_expired_paid_licence_from_the_cloud_is_kept_and_does_not_warn` |
| 6 | ECH: `rejected` set before `record_heartbeat` succeeded | low | P | set after the write |
| 7 | BH/ECH: `cloud_time` going backwards could un-expire a licence | medium | P | trusted time is `max(previous, new)`; `a_cloud_time_that_goes_backwards_never_moves_trusted_time_back` |
| 8 | BH: unchecked i64 arithmetic on server-supplied time | medium | P | saturating adds/subs and `unsigned_abs`; `the_skew_limit_is_exclusive_in_both_directions` |
| 9 | ECH: `apply_licence` warned on every beat before the branch exists | low | P | returns quietly with no branch; test `before_the_branch_exists_...` |
| 10 | VG: no test for a rejected licence being uncached, follower, no branch, monotonic term, no cloud time, start-up settle, failed-beat settle, 5-minute edge, unknown key, bad signature, expiry instant, kitchen gate through the intent, downgrade ending nothing, all non-system actors | high | P | tests added in `heartbeat.rs`, `entitlement.rs`, `apply/tests.rs` |
| 11 | BH: replayed older but valid licence accepted (no `iat` monotonicity); `cloud_time` unsigned; HTTP allowed to the cloud | high | D | shared with the 2.11 deferral; needs signed time or an `iat` floor |
| 12 | BH/ECH: restart restore trusts wall minus saved delta; a wall change while off shifts it once; `mono_ms` is stored but unused | medium | D | spec schema; accepted limit |
| 13 | BH: no clock warning before the first cloud time; offline rollback undetected | low | D | persist last-seen wall time |
| 14 | BH: status is published before the settings write; a skipped write leaves old rows | medium | R | skip happens only without the lease or branch, where nothing registers; the next beat retries |
| 15 | BH/VG: Expired shows no badge | low | R | the AC lists two badges only |
| 16 | BH/VG: update change rows carry no before-image | low | R | same as every other settings mutator |
| 17 | BH: trusted clock persisted even for a rejected licence | low | R | it is the cloud's time, not the licence's |
| 18 | ECH: follower Hub publishes no standing (no writer handle) | medium | D | with the standby story |
| 19 | ECH: pairing registrations are exempt from the device cap | low | D | Story 2.19 |
| 20 | ECH: pre-first-beat wall clock behind `iat` shows unverified briefly | low | D | pairing licence `iat` vs a slow PC clock |
| 21 | VG: `ui_printer.rs` reads `licence.flags` directly | low | D | outside `apply()`; switch to the API with Story 7.x |

Verification after fixes: see the commit message and the final report.

