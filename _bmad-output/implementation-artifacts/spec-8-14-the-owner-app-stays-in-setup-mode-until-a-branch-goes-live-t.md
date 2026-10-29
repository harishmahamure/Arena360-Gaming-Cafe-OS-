---
title: 'Story 8.14: The Owner app stays in setup mode until a branch goes live, then opens its dashboard'
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '1ac325d2d3589a090dd261b4d83448293f6ed128'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-13-today-shows-every-branch-with-its-figures-cash-variance-and.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Today draws a card for every paid branch, live or not, so an owner still setting a branch up sees empty figures. `GET /v1/branches` gives no setup progress, and the Story 2.31 done screen lands on Account → Branches instead of the dashboard.

**Approach:** `GET /v1/branches` carries `live_at` and a setup block (steps done of 10, next missing step). Today shows a setup card for each branch with `live_at` null and a normal card for live ones; branch-scoped screens list only live branches and say "Available after <branch> goes live" for the rest. The app re-reads branches every 60 s while Today is visible, and "Open dashboard" opens Today.

## Boundaries & Constraints

**Always:**
- No `live` or `summary` call for a branch with `live_at` null. Setup mode wins over the tier lock until the branch is live; a live Free branch then follows Story 8.12.
- One shared live-branch list in the shell (`live_at` set); a branch-scoped screen takes its branches from it with no filter of its own.
- Standby is advisory: Go live is never blocked by it. Strings only from the catalogue.

**Never:**
- A branch picker with no consumer, new wizard steps, a change to Go live, Reports/Alerts/Manage content (Stories 8.15+).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| All in setup | every `live_at` null | one setup card per branch: "Setting up X — 4 of 10 steps done", next step named, "Continue setup"; no `live` call; no Needs attention |
| Mixed | one live paid, one in setup | normal card for the live one, setup card for the other, in one list |
| Goes live | `live_at` set at the next poll | setup card becomes a dashboard card ("Waiting for the first update…"), no sign-out |
| Free goes live | free branch, `live_at` set | tier-lock "Available on Basic" |
| Branch screen | Reports/Manage/Alerts, branch in setup | "Available after X goes live" and "Continue setup" |
| Standby missing | comp branch, no standby | Go live succeeds |
| Done screen | "Open dashboard" | opens the Owner app root (Today) |

</intent-contract>

## Code Map

- `crates/arena-proto/src/wizard.rs` -- `OwnerBranch` gains `live_at`, `setup: BranchSetupProgress`; `WizardStep::ALL`, `StepState`, `current_step` are reused.
- `crates/arena-cloud/src/api/branches.rs` -- `row()` fills them from `onboarding_progress`; test `crates/arena-cloud/tests/branches.rs`.
- `apps/owner/src/lib/{shell.svelte.ts,today.svelte.ts,TodayBoard.svelte,Section.svelte,api.ts}` -- `liveBranches`, setup list, 60 s poll, new-live fetch; new `SetupCard.svelte`, `SetupGate.svelte`.
- `apps/onboarding/src/lib/GoLiveStep.svelte`, `src/routes/setup/+page.svelte` -- dashboard target becomes the Owner root; `apps/onboarding/tests/go-live.spec.ts`.
- `packages/ui/src/messages/en.json` (`owner.setup.*`), generated TS in `packages/ui/src/generated`.
- Tests: `apps/owner/tests/{setup.spec.ts,stub.ts,today.spec.ts,shell.spec.ts}`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/wizard.rs`, `crates/arena-cloud/src/api/branches.rs` -- add `live_at` and the setup block; regenerate TS -- the app reads progress from `GET /v1/branches`
- [x] `apps/owner/src/lib/*` -- live-branch list, setup cards, gated branch screens, 60 s branch poll, fetch for newly live branches -- the Today and shell behaviour
- [x] `apps/onboarding/*` -- "Open dashboard" targets the Owner root -- matches the new landing
- [x] `packages/ui/src/messages/en.json` -- `owner.setup.*` strings
- [x] Tests for each matrix row (fake clock for the poll), Rust list test with progress, a11y on setup mode in all themes

**Acceptance Criteria:**
- Given the Story 8.14 criteria in `epics.md`, when Today and the branch screens run against a stubbed `/v1`, then each criterion is met and the suites pass.

## Spec Change Log

## Review Triage Log

Code review (2026-10-30): no blocking findings. Noted, not fixed: `liveBranches()` has no consumer until Story 8.15; a pre-upgrade offline cache lacking `live_at` would read as live until the next online fetch.

## Design Notes

- Steps done = steps whose state is not pending (done or skipped); next missing = `current_step`. A non-live branch can show at most 9 of 10 (Go live is the tenth).
- `live` stays on `OwnerBranch` (Account → Branches uses it) beside `live_at`.
- The branch picker needs a consumer; Story 8.15 (first branch-scoped screen with real content) adds it on top of `liveBranches`.

## Verification

**Commands:**
- `cargo fmt --check`; `cargo clippy -p arena-proto -p arena-cloud --all-targets -- -D warnings`; `cargo test -p arena-cloud --test branches`; `pnpm run gen:types` then `check:generated`; `pnpm --filter @arena/owner check`; `pnpm --filter @arena/owner test`; `pnpm --filter @arena/onboarding check`; onboarding go-live spec; `pnpm run check:strings check:layers check:protocol format:check`

## Auto Run Result

Status: in-review (implemented and verified).

- Summary: `GET /v1/branches` carries `live_at` and the setup progress; Today shows a setup card per branch in setup (Free included) and normal cards for live ones; Reports, Alerts and Manage say "Available after <branch> goes live" with a Continue setup link; Today re-reads the branches every 60 s (and on Refresh) and switches a branch to its dashboard card when it goes live; "Open dashboard" opens the Owner root.
- Decisions: (1) setup mode wins over the tier lock until `live_at` is set, so a Free branch in setup sees a setup card and the Free limits after; (2) the branch picker is not built, it has no consumer until Story 8.15 and would be dead code, only the shared `liveBranches()` list is; (3) `live` stays on `OwnerBranch` for Account → Branches; (4) with no live paid branch Today hides Refresh and Needs attention; (5) the Standby item was already advisory in the Go live view, so no cloud change was needed there.
- Verification: fmt and clippy clean; `arena-proto` and `arena-cloud` tests pass except `tests/binary.rs` `unreachable_at_start_retries_then_exits_1_without_logging_the_token` and `tests/backup.rs` `a_hung_run_times_out_with_its_own_code`, which fail on a clean checkout (noted in Stories 8.9 and 8.13); owner app `check` 0 errors and Playwright 280 passed (14 new in `setup.spec.ts`, axe on all routes in light/dark/HC for live-and-setup and all-in-setup tenants); onboarding `check` 0 errors and `go-live.spec.ts` 10 passed; `check:strings`, `check:layers`, `check:protocol`, `check:no-manifest`, `format:check` ok; `check:generated` reports the new generated files until they are committed.
- Residual risks: the poll runs only while a branch is in setup, so a branch made live on another device after all others are live shows on the next open or Refresh; the full Onboarding Playwright suite and the Owner app against a real cloud were not run.
