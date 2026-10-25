---
title: 'Story 2.11: Hub checks in by heartbeat and the wizard advances by itself'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'dd6d04b14adce64c76170f7d7eb75db6aba1ecea'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-10-hub-installer-pairs-with-the-code-and-retries-in-place.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** After pairing, nothing keeps the Hub and cloud in touch. The licence is never refreshed, the branch fence isn't confirmed, and the wizard waits on step 3 forever.

**Approach:**
- A Hub heartbeat task posts `POST /v1/heartbeat` every 10 s until the branch is live, then every 60 s. It never blocks an intent.
- The cloud updates `last_seen_at`, re-signs the licence, creates `branch_fence` on the first beat, and returns `{licence, fence, cloud_time}`.
- The first heartbeat marks step 3 done. The wizard advances within 5 s through SSE, with polling as a fallback.

## Boundaries & Constraints

**Always:**
- **Control migration** `0009_branch_fence.sql` (number reserved): `branch_fence` with branch_id PK, epoch (TEXT, the Hub's provisional epoch), hub_id, created_at.
- **`POST /v1/heartbeat`** (bearer `hub_<id>.<secret>`, verified against `hubs.token_hash` with Argon2id behind the 2.5 semaphore; cache a verified token for 5 minutes in memory, keyed by a SHA-256 of the full token):
  - **Body:** `{epoch, version, schema_version, last_seq_uploaded}`.
  - **Revoked Hub:** 401 `TOKEN_REVOKED`.
  - **Unknown or bad token:** 401 `TOKEN_INVALID`.
  - **Otherwise, in one transaction:**
    - update `hubs.last_seen_at`, `version` and `schema_version`;
    - if `branch_fence` is missing, insert it with the given epoch by compare-and-set (`INSERT … ON CONFLICT DO NOTHING`, then read back);
    - mark the `install_hub` progress step done the first time.
  - **Answer:** `{licence: <re-signed from the current entitlement, 2.8>, fence: {epoch, confirmed: epoch == fence.epoch}, cloud_time: <ms>}`.
  - Per-Hub rate limit: 1 per 5 s (`rate_reserve`), else 429.
- **Wizard events:**
  - `GET /v1/wizard/events` is an SSE stream for the owner session's branch. It sends `wizard.changed` when that branch's progress changes, through an in-process broadcast from the heartbeat transaction. It sends a keep-alive comment every 20 s.
  - The onboarding app subscribes on the setup layout. Without SSE (error or proxy) it polls `GET /v1/wizard` every 5 s while the current step is `install_hub`.
  - On advance it uses 2.7's focus and announce helper on the step 4 heading.
  - `GET /v1/wizard/install-hub` now reports a real `hub_paired` (the branch has a non-revoked `hubs` row with `last_seen_at`).
- **Hub task** (`crates/arena-tasks/src/heartbeat.rs`, `HeartbeatTask`):
  - The interval is 10 s while the branch isn't live, and 60 s otherwise. "Live" is the `go_live` step done; until 2.31, the Hub reads a `branch.live` setting, default false.
  - **Each run:** read the token from the keystore, then post. On 200:
    - replace `licence_cache` (verify the licence with the Hub's verify set first; an invalid licence is logged and ignored);
    - store `cloud_time` and the monotonic offset in `HubStatus` for 2.12's trusted time;
    - if the fence is confirmed, clear `lease.provisional` (Epic 1 lease).
  - **On `TOKEN_INVALID` or `TOKEN_REVOKED`:** set `HubStatus.pairing = revoked`, which the Hub UI shows as a staff-visible banner "This Hub is no longer paired". Local trading continues and the task backs off to 5 minutes.
  - **On network failure:** jittered exponential backoff from 2 s up to the normal interval. Epic 1's `chip-offline` covers the UI; nothing else changes.
  - It runs on its own task and never touches the Writer except through intents (the lease update goes through the existing lease API), and has a test proving an intent completes while a fake heartbeat hangs.
  - `last_seq_uploaded` is 0 on Free.
- **Hub UI:** a revoked banner in the shell header area (role `status`), with the copy above. Every string lives in `en.json`.
- **Tests:**
  - **Cloud:** each matrix row; a first beat creates the fence and marks step 3 done; the CAS race (two first beats with different epochs give one fence, the second gets `confirmed: false`); licence re-signed with the current tier (a comp grant shows up on the next beat); the token cache; the rate limit; SSE delivers `wizard.changed`.
  - **Hub:** the interval switch, backoff with jitter bounds (paused clock), licence verification, provisional cleared on confirm, the revoked state, the intent-not-blocked test.
  - **Playwright** (onboarding): step 3 advances within 5 s via SSE, and via polling when SSE fails, with focus on the step 4 heading. **Hub Playwright:** the revoked banner.

**Never:**
- Blocking an intent on the heartbeat.
- Stopping local trading when revoked.
- Trusting an unverified licence.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First beat | paired, no fence | fence created; step 3 done; `confirmed: true`; licence | N/A |
| Later beat | fence exists, same epoch | `confirmed: true`; `last_seen_at` updated | N/A |
| Other epoch | fence has a different epoch | `confirmed: false` (2.12+ handles fencing) | N/A |
| Revoked | the Hub's row revoked | 401 `TOKEN_REVOKED`; Hub banner; trading continues | backoff 5 min |
| Bad token | random | 401 `TOKEN_INVALID` | same as revoked |
| WAN down | network error | backoff 2 s → interval with jitter; chip-offline | N/A |
| Comp granted | grant-comp then beat | the new licence carries paid limits | N/A |
| Too fast | 2 beats within 5 s | 429 | N/A |
| Wizard SSE | first beat while step 3 open | `wizard.changed`; UI on step 4 ≤ 5 s | poll fallback |

</frozen-after-approval>

## Code Map

Investigate from 2.10 (`hubs`, Hub keystore, cloud client, `hub_identity`, `licence_cache`), 2.8 (signer, verify set, `tier_at`), 2.7 (progress, focus/announce helper, wizard store), Epic 1 lease (`crates/arena-tasks/src/lease.rs`, provisional epoch, `HubStatus`), 1.22 connectivity task pattern. Control migration **0009 reserved**.

## Tasks & Acceptance

**Execution:**
- [ ] cloud: migration 0009, store, `api/heartbeat.rs`, token cache, `api/wizard_events.rs` (SSE) + broadcast.
- [ ] Hub: `arena-tasks/src/heartbeat.rs`, `HubStatus` (cloud time, pairing state), lease provisional clear, revoked banner.
- [ ] onboarding: SSE subscription + poll fallback; `install_hub` advance.
- [ ] tests, README.

**Acceptance Criteria:**
- Given `--dev` cloud and a Hub paired through 2.10, when the Hub starts, then within 5 s the open wizard moves from step 3 to step 4 with focus on its heading, and `/healthz` on the Hub shows the licence's tier.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast` (`ARENA_REQUIRE_SQLD=1`); dev-fixtures runs; clippy incl. control + dev-fixtures; fmt; linkage script.
- onboarding and hub `check` + Playwright with unique `ARENA_PW_PORT` and `--workers=2`; every `pnpm run check:*`; `pnpm run test:scripts`.

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter, Verification Gap; on Sonnet). P = patch, D = defer, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | BH/ECH: duplicate `pairing_revoked` field in two `arena-core` test literals | high | P | would not compile; removed |
| 2 | BH/ECH: `branch_fence` never resets, so a re-paired Hub is `confirmed: false` forever | high | P | `pair_hub` deletes the branch's fence in its transaction; test `re_pairing_resets_the_fence_so_the_new_hub_confirms` |
| 3 | BH/ECH: `wizard.changed` lost if a later step fails, or if the stream (re)opens after the first beat; polling off while streaming | high | P | event sent right after the beat commits; the layout syncs on `onopen` |
| 4 | BH/ECH: progress upsert and licence checked against a skewed local clock | medium | P | upsert only writes when not already done; licence verified at `cloud_time` |
| 5 | ECH: `CloudTime.at` taken after the round trip | medium | P | stored at the midpoint of the request |
| 6 | BH/ECH: jitter from sub-second nanos has about 10 distinct values on Windows | low | P | `RandomState` hash |
| 7 | VG: `/healthz.tier`, `pairing_revoked` mapping, `TOKEN_INVALID`, 429/500/garbage, request contents, cache keyed by full token, revoked-race write, fence count, `schema_version` and `completed_at` had no failing test | high | P | tests added in `health.rs`, `heartbeat.rs`, `tests/pair.rs` |
| 8 | BH: verified-token cache not tied to the stored hash, unbounded until the next insert | medium | R | a hub id's hash never changes; entries exist only for verified tokens; revoked rows are rechecked each beat |
| 9 | BH: 2 s retry after a lost answer meets the 5 s gap and counts as a failure | low | R | self-heals on the next backoff step |
| 10 | BH: `tier` on `/healthz` leaks | low | R | `/healthz` binds loopback only |
| 11 | BH: SSE ignores `branch_id` | low | R | `GET /v1/wizard` uses the same default branch |
| 12 | ECH: `hub_paired` false until the first beat | low | R | by spec |
| 13 | BH: unauthenticated heartbeat requests cost an Argon2 verify with no per-IP limit | medium | D | shares the 2.5 semaphore with pairing; needs an IP limiter |
| 14 | BH: Hub accepts a validly signed licence for another branch or an older one | medium | D | compare with `hub_identity` and the cached licence |
| 15 | BH: `cloud_time` is unsigned | medium | D | 2.12 trusted time |
| 16 | BH/ECH: revoked 5-minute backoff not interruptible, no signal when the keystore token is missing | medium | D | belongs with the Hub re-pair and unpair flow |
| 17 | BH/ECH: SSE has no connection cap, no auth expiry, no `X-Accel-Buffering`, no silent-stream watchdog | medium | D | |
| 18 | ECH: a same-Hub restart bumps the epoch counter, so `confirmed` goes false | medium | D | spec matrix defers fencing to 2.12 |
| 19 | VG: no Hub end-to-end heartbeat test, validation and per-Hub limit edges, SSE scoping, Playwright poll suppression, 8 to 9 upgrade test, `record_heartbeat` guards, slow-answer intent test | medium | D | |
| 20 | VG: nothing sets `HubStatus.live`, so the 60 s interval is unreachable | medium | D | Story 2.31 wires the `go_live` step |

