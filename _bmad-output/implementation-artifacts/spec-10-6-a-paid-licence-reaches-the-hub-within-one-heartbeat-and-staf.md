---
title: 'Story 10.6: A paid licence reaches the Hub within one heartbeat and staff see the plan'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '3f614bad93eb1820589714abe09b7da5536a1111'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-5-upgrade-links-and-add-branch-reuse-the-same-checkout.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The tier table still held provisional paid values (Basic 10 users, no standby, Premium 50 users), the licence did not say how a branch is billed, and nothing told staff at the Hub which plan they were on or that it had just changed.

**Approach:** The tier table in `arena-proto` takes the Story 10.6 values; a paid-subscription licence carries the billing `cycle`; the Hub reads cycle and end from the verified licence (never a setting, so a heartbeat writes nothing), System shows "Plan: Basic · Monthly" and the licence end in branch time, and a move onto a different verified paid plan raises one silent "Plan updated to <tier>" alert in the same transaction as the tier row.

## Boundaries & Constraints

**Always:** the licence is the only carrier of paid claims; the swap needs no restart and every gate reads the new rows for the next intent; the old and new tier are the change record of `licence.tier` with `actor{system, hub-licence}`; offline paid claims hold to `exp` on trusted time (existing 2.12 path); Windows pairing is refused only for Free.

**Never:** no change to the 10.3 webhook transitions or the heartbeat/pairing wire shapes beyond the optional `cycle` claim; no new intent kind; no lapse/downgrade handling (10.12, 10.13); no invoices (10.7).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Free to Basic | heartbeat returns a paid licence | `licence.*` rows swap, tier row change (system), one `plan_updated` alert, System "Plan: Basic · Monthly" |
| Basic to Premium, monthly to yearly | next heartbeat | plan follows; an open notice is not doubled |
| First licence at pairing | no `licence.tier` row yet | no notice |
| Free, expired, or unverified licence | standing not verified or tier Free | no notice; coming back from unverified raises none |
| Basic + Tunnel add-on | `tunnel` add-on | flags gain `tunnel` and `live_view`; devices 60 + 20n |
| Comp licence | source comp | no `cycle` claim; System shows the tier without a cycle |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{tier,licence,entitlement,snapshot,alert}.rs` -- tier table (`UNLIMITED`), `cycle` claim, `Entitlement.{cycle,exp}`, snapshot `plan_cycle`/`licence_exp`, `AlertKind::PlanUpdated`.
- `crates/arena-cloud/src/{licence,ops}.rs`, `src/api/{heartbeat,pair,wizard_install}.rs` -- Tunnel add-on adds `live_view`; `claims_with_plan` adds the cycle for heartbeat, pairing and the standby licence.
- `crates/arena-core/src/apply/{licence,mutate}.rs`, `status.rs`; `crates/arena-tasks/src/heartbeat.rs`; `crates/arena-api/src/health.rs` -- the notice, status fields, snapshot fields.
- `apps/hub/src/{lib/plan.ts,lib/alerts.svelte.ts,routes/system/+layout.svelte}`, `packages/ui/src/messages/en.json` -- System plan line and the alert text.
- `apps/onboarding/src/lib/TierStep.svelte` -- "Unlimited staff logins" instead of the sentinel number.
- Tests: proto `tier`/`entitlement`, cloud `licence|billing_events|pair|presign`, core `apply::tests::licence`, tasks `heartbeat`/`upload`, hub `plan.spec.ts`, onboarding `tier.spec.ts`.

## Tasks & Acceptance

- [x] Tier table per spec (Basic 60 devices, users and branches unlimited, 900 s, 365 d, `owner_app kiosk_orders kitchen standby`; Premium 200, 300 s, 1095 d, plus `tunnel live_view`); Tunnel add-on on Basic adds `tunnel` and `live_view`
- [x] `cycle` on a paid-subscription licence; cloud signs it for heartbeat, pairing and the standby licence
- [x] Hub reads cycle and end from the verified licence into status and snapshot
- [x] One silent "Plan updated" alert in the licence-apply transaction; none on first licence, Free, unverified, repeat
- [x] System shows "Plan: <tier> · <cycle>" and the licence end in branch time
- [x] Wizard tier card says "Unlimited staff logins" for an uncapped tier
- [x] Tests (see Code Map); README section

## Verification

- `cargo fmt --all --check`; `cargo clippy -p arena-{proto,core,tasks,cloud,api,hub} --all-targets -- -D warnings` -- clean
- `cargo test -p arena-{proto,core,tasks,cloud,api,hub,store}` -- pass apart from the failures listed below
- Playwright: hub `plan|alerts|system-backup|tunnel|audit` and onboarding `tier|upgrade` pass; `pnpm run check:strings check:tier-lock` ok

## Auto Run Result

Status: review. Baseline-failing tests (they fail on Windows or are stale before this story): cloud `backup::a_hung_run_times_out_with_its_own_code`, `binary::unreachable_at_start…`, `price_book::a_draft_is_copied_edited…` (ordering), tasks `heartbeat::trusted_time_is_cloud_time_plus_monotonic_elapsed` (`Instant` underflow on a recently booted Windows PC) and `printer::lan::a_printer_that_stops_reading_times_out`, replay `replaying_the_seeded_log…` (no library rows seeded), api `ui_auth::lock_and_unlock…` (log capture race; passes alone), `arena-e2e` and kiosk-svc `discovery` tests (Unix-only); `check:generated` is stale only until the regenerated types are committed.

Behaviour changes to know: Basic now carries `standby` and `kiosk_orders` (a standby code is `TIER_LOCKED` only on Free), and Basic uploads every 900 s instead of 300 s (presign hourly limit 16). Residual risks: `licence.user_cap` for an uncapped tier is `u32::MAX`; a cycle change alone does not refresh the System line until the next snapshot load; the "within 70 s" claim is covered by the 60 s live interval and a test of the swap at the next heartbeat, not a wall-clock measurement on Android hardware.

## Code Review Result

Reviewed 2026-10-31 against the intent contract: no blocking findings. fmt and clippy clean; proto/core/cloud/tasks tests pass except the documented baseline failure (`trusted_time_is_cloud_time_plus_monotonic_elapsed`). The hub `playwright.kitchen.config.ts` `process` type error predates this story. Non-blocking: a cycle change alone does not refresh the System line until the next snapshot load.
