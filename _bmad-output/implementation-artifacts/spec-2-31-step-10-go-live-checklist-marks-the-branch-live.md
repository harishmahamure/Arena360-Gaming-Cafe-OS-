---
title: 'Story 2.31: Step 10 — go-live checklist marks the branch live'
type: 'feature'
created: '2026-10-26'
status: 'review'
baseline_commit: 'be4a8b94d7a79fefe96f973e59fc4525c4b3227e'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-30-owner-manages-branches-create-rename-timezone-new-hub-pairin.md'
warnings: []
deferred: []
---

## Intent

**Problem:** Wizard step 10 is a placeholder: an owner cannot confirm the branch is ready, `branches.live_at` has no writer, the Hub never learns it is live, and the `branch.go_live` command is unsupported.

**Approach:**
- Cloud: `GET /v1/wizard/go-live` returns the checklist (opening hours, backup location, standby, ad consent on Free) from the latest setup status, plus the first blocker; `POST /v1/wizard/go-live` refuses while a blocker remains, else queues `branch.go_live`, sets `live_at` and marks step 10 done.
- Hub: `branch.go_live` becomes a `settings.set` of the new `setup.live` setting; `SetupStatus.live` feeds `HubStatus.live`, which already switches the heartbeat to its normal interval and stops the setup block. The Settings nav drops Setup when live.
- Wizard: `GoLiveStep` lists items, disables "Go live" with the first missing item named, and ends on a done screen linking to the Owner app's Account → Branches; `/setup` on a live branch redirects there.

## Boundaries & Constraints

- Required: steps 1–5 and 9 done, steps 6–8 done or skipped, opening hours, backup location, and ad consent on Free. Standby is advisory on every tier and never blocks.
- Go live is idempotent: a live branch queues no second command.
- No comments in code.

## Tasks & Acceptance

- [x] `arena-proto` — `KEY_SETUP_LIVE`, `SetupStatus.live`, `GoLiveView` types; regenerate `packages/ui/src/generated`.
- [x] `arena-core` — `setup.live` default and rule, `branch.go_live` command, `live` in `setup_status`, `HubStatus.live` from the setup refresh — unit tests.
- [x] `arena-store` + `arena-cloud` — `go_live`, `wizard_golive.rs` — `tests/pair.rs`.
- [x] `apps/onboarding` — `GoLiveStep`, api, `PUBLIC_OWNER_URL`, live redirect, strings, `go-live.spec.ts`.
- [x] `apps/hub` — Setup nav hidden when live, test.

## Verification

- `cargo fmt --check`; `cargo clippy --locked --workspace --all-targets -- -D warnings`; `cargo test --locked --workspace --no-fail-fast` — pass (one pre-existing flake, `owner_staff_and_device_edits_win_over_a_stale_base_rev`, fails on a clean tree too).
- `pnpm check:strings`, `check:protocol`, `format:check`; onboarding and hub `check` and Playwright — pass.

## Known limits

- Checklist rows name the Hub screen in text; the wizard cannot link to a LAN-only Hub.
- `setup.live` is an ordinary setting, so a Hub user with settings access could set it; the Hub never sets it back.
- `PUBLIC_OWNER_URL` defaults to `https://app.arena360.cloud`; set it per deploy. Story 8.14 re-targets the action to the Owner dashboard.
- The Hub's Setup route stays reachable by URL; only the nav entry is hidden.
