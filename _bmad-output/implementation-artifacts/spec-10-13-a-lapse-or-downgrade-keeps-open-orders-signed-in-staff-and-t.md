---
title: "Story 10.13: A lapse or downgrade keeps open orders, signed-in staff and the main Hub working"
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: 'b9461798'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-12-a-lapse-or-downgrade-never-ends-a-running-session-and-over-c.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** After a lapse or downgrade the Hub must keep every open order, kitchen ticket and signed-in staff member, refuse only what is new, and let a standby that lost its flag stop following without touching the main Hub.

**Approach:** `licence.apply` already writes only settings and device standing, so open orders and tickets are untouched; this story pins that with tests and a proptest, refuses kiosk orders with `FEATURE_NOT_ENTITLED`, ranks new staff sign-ins against `licence.user_cap`, and gates the standby follower and promote on a `standby_needs_plan` status flag set from the standby's own licence.

## Boundaries & Constraints

**Always:** nobody signed in is signed out; a user-cap refusal is said only after the password matched; the standby gate is in the Hub status so a returning plan resumes without a restart.

**Never:** no new intent kind; no change to the licence or heartbeat wire shapes; no change to the main Hub when a standby loses its flag.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| `kitchen` drops with an open ticket | licence without `kitchen` | the ticket still moves to served; new placements open no ticket; flag back, tickets open again |
| Device order without `kiosk_orders` | `order.place` by a device | `FEATURE_NOT_ENTITLED`; counter orders not gated |
| More active staff than `users` | sign-in with right password | Owner and the most recently active up to the cap sign in; others `USER_LIMIT` "This branch's plan allows <n> users — ask the owner." |
| Wrong password beyond the cap | sign-in | `AUTH_FAILED` (nothing about the cap leaks) |
| Standby licence lacks `standby` | trusted licence | follower `NeedsPlan`, promote and force promote `FEATURE_NOT_ENTITLED` "Standby needs a paid plan"; plan back resumes |
| Untrusted standby licence | unverified licence | decides nothing |

</intent-contract>

## Tasks & Acceptance

- [x] Kiosk order refusal is `FEATURE_NOT_ENTITLED` (`apply/guards.rs`); tests updated
- [x] `arena_store::within_user_cap` / `Store::user_cap_standing`; `StaffDirectory::sign_in` refuses `USER_LIMIT` beyond the cap
- [x] `HubStatus.standby_needs_plan` set by `StandbyLicenceTask`; `Follower::with_plan_gate` (`FollowStatus::NeedsPlan`), `Promoter::with_plan_gate`, `StandbyScreen.needs_plan`
- [x] Open ticket/order tests (`apply::tests::lapse_orders`), sign-in cap test, follower, promoter and licence-task tests
- [x] `proptest_apply.rs`: `UserCapLicence` op, and orders/tickets/staff/sessions asserted unchanged across licence ops
- [x] README section

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-cloud -p arena-kiosk-core -p arena-tasks --all-targets -- -D warnings` -- clean
- `cargo test --no-fail-fast -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-tasks -p arena-kiosk-core -p arena-replay` -- all pass except pre-existing failures: `arena-replay::replaying_the_seeded_log_reproduces_branch_and_settings` (fails identically on the baseline), `trusted_time_is_cloud_time_plus_monotonic_elapsed` (documented baseline) and the Windows-timing `printer::lan::a_printer_that_stops_reading_times_out`
- `pnpm run check:generated` passes; no app code changed

## Auto Run Result

Status: done (code review: no blocking findings; comment tidy-ups only). Risks to know: "most recently active" is the newest change row a member authored, so a member who only reads is ranked last; the follower, promoter and licence task are not yet wired into the Hub binary, so the gate receiver is passed where that runtime lands; no standby screen app renders `needs_plan` yet. The disk was full during verification, so `target/debug/incremental` (regenerable cache) was deleted.
