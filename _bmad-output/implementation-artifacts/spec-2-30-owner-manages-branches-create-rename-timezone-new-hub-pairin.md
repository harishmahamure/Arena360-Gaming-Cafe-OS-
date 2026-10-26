---
title: 'Story 2.30: Owner manages branches — create, rename, timezone, new Hub pairing code'
type: 'feature'
created: '2026-10-26'
status: 'review'
baseline_commit: '0bd54f96c70677bd944eda09254087adbcca2fc6'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-15-owner-changes-reach-the-hub-through-the-command-mailbox-with.md'
warnings: []
deferred: []
---

## Intent

**Problem:** The Owner app's Account → Branches page is a bare list, `GET /v1/branches` does not exist in the cloud, an owner cannot rename a branch or change its timezone, and the Hub refuses `settings.set` for `branch.name`.

**Approach:**
- Cloud: `GET /v1/branches`, `PATCH /v1/branches/{id}` (name, timezone) and `POST /v1/branches` (add). A paired branch also gets one `settings.set` command per changed field; an unpaired one only changes `gz_control` and reaches the Hub in `branch.bootstrap`.
- Hub: `branch.name` becomes a real setting (seeded at bootstrap, backfilled by `branch.seed_defaults`, read by the branch summary), so the command applies.
- Wizard: `GET /v1/wizard` and `PUT /v1/wizard/cafe` take an optional `branch_id`; the onboarding app carries `?branch=` on every wizard call, so a new branch opens at step 1.
- Owner app: branch cards with name, code, plan, setup/live state, Hub status, "Waiting for branch", inline edit, "Get pairing code" (the shared `PairingCode` card), "Continue setup", "Add branch" and a locked standby row on Free.

## Boundaries & Constraints

- Free tenant: Add branch and Standby show `tier-lock` "Available on Basic"; the cloud refuses `POST /v1/branches` with `FEATURE_NOT_ENTITLED` unless some branch of the tenant has a non-Free tier (paid or comp).
- Branches get a nullable `live_at` (migration 14) so the page can show Live; Story 2.31 sets it.
- A paired branch is one with an active `hubs` row. Its cloud row is updated at once and the card shows "Waiting for branch" while a `settings.set` command is pending or fetched.
- The Hub applies a timezone change from the next business day (existing deferral); the name applies at once.
- No standby code here, no paid checkout, no `live_at` writer.
- No comments in code.

## Tasks & Acceptance

- [x] `arena-proto`, `arena-core`, `arena-store` (Hub) — `KEY_BRANCH_NAME`, `SettingRule::BranchName`, seeding, summary — a `branch.name` command applies and bad names are refused.
- [x] `arena-store` (control) + `arena-cloud` — migration 14, `add_branch`, `update_branch_profile`, `rename_branch`, `settings_pending`, the three routes, `branch_id` on the wizard state and café save — `tests/branches.rs`.
- [x] `apps/onboarding` — `target` branch, injected into wizard calls, events URL and step links.
- [x] `apps/owner` + `packages/ui` — `BranchCard`, page, `send`, messages, Playwright tests with axe.

## Verification

- `cargo fmt --check`; `cargo clippy --locked --workspace --all-targets -- -D warnings`; `cargo test --locked --workspace --no-fail-fast` — pass.
- `pnpm check:generated`, `check:protocol`, `check:strings`, `format:check`; owner and onboarding `check` and Playwright — pass.

## Known limits

- A `settings.set` the Hub rejects is not surfaced on the card (it only stops showing "Waiting").
- A new branch has no entitlement until step 2; with billing off, a comp grant for it comes from the operator CLI.
- Add branch copies city, country, currency, timezone and GST fields from the first branch; step 1 of the new wizard lets the owner change them.
