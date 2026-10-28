---
title: 'Story 4.16: The Android Hub ships through Google Play with a staged rollout that never interrupts a session'
type: 'feature'
created: '2026-10-28'
status: 'in-review'
baseline_revision: 'b0472f068bb2e11a99c4e9fc82f65ba667916576'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-15-upgrading-removes-ads-within-one-heartbeat-and-a-lapse-bring.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Android Hub had a release AAB in CI but no way to reach Free cafés: no Play upload, no recorded store declarations, no rollout plan, and no update path that waits for open sessions.

**Approach:** A tag-triggered workflow (after the cloud release is live) uploads the signed AAB to Play internal testing; the listing declarations and rollout plan are data checked against the build; promotion and halt are scripted with the recorded thresholds; the Hub downloads Play updates in the background and installs only with no open session.

## Boundaries & Constraints

**Always:**
- Cloud first: the Hub upload runs only after `deploy-cloud` succeeded for the tag.
- Credentials come from SOPS files; a Play build is never unsigned.
- Install waits while any session is active, paused or in grace; a Play-forced install restarts the service.

**Never:**
- No automatic promotion to production; no thresholds chosen at rollout time; no plaintext secret in the repo.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Tag, cloud deploy failed | workflow_run conclusion != success | no build, no upload |
| Ad SDK version bumped | listing `ad_sdk.version` stale | `check:play` fails |
| Promote 10% after 5 h at 1% | hold is 24 h | refused |
| Pre-launch report has a crash | any | promote refused |
| Update ready, one paused session | open > 0 | "Install and restart" disabled with the reason |
| Play installs during sessions | package replaced | `BootReceiver` restarts the service |

</intent-contract>

## Code Map

- `.github/workflows/release-android.yml` -- gate (cloud live, listing check) then signed AAB and internal upload.
- `scripts/play-release.mjs` (+ test) -- listing/rollout checks, promotion and halt rules, Play Developer API client.
- `deploy/play/{listing,rollout}.json` -- the store declarations and the staged plan with halt thresholds.
- `docs/runbooks/android-play-rollout.md`, `README.md` -- runbook and Android Hub section.
- `UpdateBridge.kt`, `MainActivity.kt`, `HubBridge.kt`, `build.gradle.kts` -- Play in-app updates (flexible).
- `apps/hub/src/lib/update.ts`, `routes/system/service/+page.svelte`, `service.svelte.ts`, `en.json` -- "Update ready" in System.

## Tasks & Acceptance

**Execution:**
- [x] Release workflow -- cloud gate, signed AAB, Play internal upload with SOPS credentials
- [x] Listing and rollout data plus `check:play` in `pnpm check`
- [x] Promotion, halt and API client with tests
- [x] Kotlin `UpdateBridge` and bridge methods
- [x] System update panel and tests
- [x] Runbook and README

**Acceptance Criteria:** those of Story 4.16 in `epics.md`. The pre-launch report criterion is a recorded gate (`promote --prelaunch`), since Play exposes the report only in the Console.

## Verification

**Commands:**
- `node --test scripts/play-release.test.mjs`, `pnpm run check:play`, `check:strings`, `check:pins`, `check:secrets`, `check:ads`, `check:generated`, `check:layers`, prettier, `pnpm --filter @arena/hub check`: pass.
- Hub Playwright `service`, `update.unit`: pass (desktop and phone). `pnpm run test:scripts`: all pass except the 12 `check-deploy` tests (no `kubectl` here).
- Not run: the Gradle/Kotlin build, the workflow, the Play API against a real account.
