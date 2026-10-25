---
title: 'Story 2.19: Device cap and pairing rate limits at the Hub'
type: 'feature'
created: '2026-10-26'
status: 'review'
baseline_commit: 'cc055acfffdfcb2ff46cb1cb934691150dc2a364'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-18-kiosk-pairs-with-a-6-digit-code-from-the-hub-or-a-manager-pi.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 2.18 pairs kiosks with no device cap (only manual registers are capped), allows 10 wrong codes before replacing the code, and has no per-IP hourly limit on pairing attempts.

**Approach:** Count paired devices against `licence.device_cap` in `apply()`; invalidate the code after 5 wrong tries; limit each LAN IP to 20 pairing attempts (code or PIN) per hour with `RATE_LIMITED`; show the cap refusal on the kiosk and as the Hub's Devices `tier-lock`.

## Boundaries & Constraints

**Always:**
- The cap is `limits.devices` as already stored in `licence.device_cap` (Free 20, Basic 60, Premium 200). The count is non-revoked `devices_meta` rows (kiosks, Pis, manual devices); standby Hubs and kitchen displays never have rows there, so they are excluded.
- `DEVICE_LIMIT` is raised in `apply()`'s permission step so nothing is written. A refused pairing leaves the pairing code valid (it is restored, as for other failures).
- 5 wrong codes (`PAIR_FAILURES_PER_CODE`) invalidate the live code; the open panel gets a fresh one over SSE.
- 20 attempts per hour per LAN IP (`PAIR_ATTEMPTS_PER_IP_PER_HOUR`) count every `POST /pair` that reaches the limiter, code or PIN, success or not. IPv6 peers share a budget per /64 (`peer_key`). Over the limit: 429 `RATE_LIMITED` with `Retry-After` until the oldest attempt ages out; the Hub log records it. The check runs before any code or PIN is verified.
- Revoking a device frees its slot at once (already true; keep a test).
- Hub Settings → Devices shows `TierLock` ("Available on Basic", or Premium on Basic) with the "Upgrade" link shown as "Available soon" until Epic 10, whenever the branch is at its cap.
- New strings live in `en.json`.

**Never:**
- No cloud-side change, no new migration, no new pairing UI, no per-code attempt state beyond the existing `wrong` counter.
- No rate limit on the staff-session (`/ui/*`) routes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pair under cap | count < cap, valid code | 200 as before | No error expected |
| Pair at cap | count = cap, valid code | 403 `DEVICE_LIMIT`, no rows written, code stays valid | kiosk: "This branch has reached its device limit — ask the owner." |
| Manual register at cap | count = cap | `DEVICE_LIMIT` (unchanged) | N/A |
| Revoke then pair | at cap, one revoked | pairing succeeds | N/A |
| 5th wrong code | 5 wrong | code replaced, panel gets a new one | 401 `PAIR_CODE_INVALID` |
| 21st attempt | same IP within an hour | 429 `RATE_LIMITED`, `Retry-After`, warn log | kiosk: plain "Too many tries" line |
| Hour rolls | oldest attempt > 1 h old | attempt admitted | N/A |

</intent-contract>

## Code Map

- `crates/arena-core/src/apply/devices.rs` (`load_register`) -- the `token_hash.is_empty()` guard limits the cap to manual devices; drop it. Update the module doc.
- `crates/arena-core/src/apply/tests.rs` (~6637) -- cap test; add a paired-register case.
- `crates/arena-api/src/pairing.rs` -- `MAX_WRONG_CODES` (10 -> `PAIR_FAILURES_PER_CODE`), `Pairing` state; add the per-IP hourly attempt limiter beside the PIN state.
- `crates/arena-api/src/lan/pair.rs` (`pair`) -- call the limiter first; map to 429 with `Retry-After` (reuse `Answer.retry_after_s`).
- `crates/arena-proto/src/pairing.rs` -- `PAIR_FAILURES_PER_CODE`, `PAIR_ATTEMPTS_PER_IP_PER_HOUR` constants already exist.
- `crates/arena-proto/src/snapshot.rs`, `crates/arena-core/src/snapshot.rs` -- add `device_cap` to `HubSnapshot`; regenerate TS (`pnpm run gen:types`).
- `apps/kiosk/src/lib/bridge.ts`, `Discovery.svelte` -- `DEVICE_LIMIT` and `RATE_LIMITED` results and messages.
- `apps/hub/src/routes/settings/devices/+page.svelte` -- show the existing `TierLock` block when `devices.length >= device_cap`.
- `packages/ui/src/messages/en.json` -- strings.
- `crates/arena-hub/tests/pairing.rs`, `apps/kiosk/tests/pairing.spec.ts`, `apps/hub/tests/devices.spec.ts` -- tests.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-core/src/apply/devices.rs` + tests -- cap applies to paired registers; revoke frees the slot
- [x] `crates/arena-api/src/pairing.rs`, `lan/pair.rs` -- 5 wrong codes; per-IP hourly limiter, `RATE_LIMITED`, log; unit tests with `ManualClock`
- [x] `crates/arena-proto`, `crates/arena-core/src/snapshot.rs` -- `device_cap` in the snapshot; regenerate types
- [x] `apps/kiosk/**`, `apps/hub/**`, `en.json` -- kiosk messages; Hub tier-lock at cap; Playwright cases
- [x] `crates/arena-api/tests/pair.rs` -- real pairing at the cap refused, then succeeds after a revoke; 21st attempt `RATE_LIMITED`

**Acceptance Criteria:**
- Given a branch at its device cap, when a kiosk pairs with a valid code, then it gets `DEVICE_LIMIT`, nothing is written, and the Hub's Settings → Devices shows the tier-lock.
- Given 5 wrong codes, when the 5th arrives, then the code is replaced; given 21 attempts from one IP within an hour, then the 21st gets `RATE_LIMITED`.
- Given a revoked device at the cap, when a new kiosk pairs, then it succeeds.

## Spec Change Log

## Review Triage Log

## Design Notes

The Hub tier-lock is derived from the snapshot (`devices.length >= device_cap`) rather than a stored "last refusal" flag: it is stateless, survives reload and also covers the manual-register refusal already handled in the page.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --check` -- pass
- `pnpm run gen:types && pnpm run check:generated`; `pnpm --filter @arena/kiosk check && pnpm --filter @arena/kiosk test:ui`; `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell`; `pnpm run check:strings` -- pass
