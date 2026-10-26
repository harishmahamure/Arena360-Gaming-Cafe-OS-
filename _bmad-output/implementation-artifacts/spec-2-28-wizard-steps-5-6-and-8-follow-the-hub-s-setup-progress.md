---
title: 'Story 2.28: Wizard steps 5, 6 and 8 follow the Hub''s setup progress'
type: 'feature'
created: '2026-10-26'
status: 'done'
baseline_commit: 'ba8973ec9d641a134e8b2856b788076195958a61'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-22-step-4-owner-groups-devices-in-the-web-wizard-as-they-arrive.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-27-printer-detection-and-test-print-at-the-hub.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The online wizard's steps 5 (Rate plans), 6 (Products) and 8 (Printer & payments) are placeholders. The heartbeat `setup` block carries only devices and groups, so the cloud can't tell whether rates, a manager or the printer are done, and the Hub has no Setup checklist.

**Approach:**
- Widen the heartbeat `setup` block with a per-group `has_rate` flag and the Hub's `SetupStatus` plus `manager_exists` and `test_session`. The cloud completes step 5 and step 8 by itself from it.
- Add owner-driven "Skip for now" for steps 6 and 8, the three onboarding step screens, and a Hub Settings → Setup checklist.

## Boundaries & Constraints

**Always:**
- **Protocol** (`crates/arena-proto`): `SetupGroup` gains `has_rate: bool` (`#[serde(default)]`). `BranchSetup` gains `status: Option<SetupStatus>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`). `SetupStatus` gains `manager_exists: bool` and `test_session: TestSessionState` (`unset|passed|failed`, default `unset`; story 2.29 writes it), both `#[serde(default)]`. Regenerate `packages/ui/src/generated/*`; keep the existing `SetupStatus` JSON tests and health fixtures in step.
- **Hub** (`crates/arena-core`, `crates/arena-tasks/src/heartbeat.rs`): `branch_setup` fills `has_rate` from the snapshot group's `rate_minor` and `status` from the snapshot's `setup`. `manager_exists` is true when an active staff row holds the Manager or Owner role. The block is still sent only before go-live.
- **Cloud** (`crates/arena-cloud`, `crates/arena-store/src/control`): when a valid `setup` block arrives, `record_setup` also, in the same transaction:
  - marks `rates` done if there is at least one group and every group has `has_rate`;
  - marks `printer_payments` done if `status.printer` is `ok` or `skipped`.
  - Neither ever reverts a `done` or `skipped` step. A step already `skipped` is not overwritten with `done` for printer; rates can't be skipped.
- **Skip:** `POST /v1/wizard/products/skip` and `POST /v1/wizard/printer-payments/skip` mirror `/v1/wizard/staff/skip` (owner session, `ready_branch`, only a `pending` step changes, returns `WizardState`). One shared store method takes the step. Skipping step 6 opens step 7 (`current_step` already does this).
- **Offline:** steps 5 and 8 read `GET /v1/wizard/devices` (already carries `setup`, `reported_at`, `now`, `hub_online`) and show the last reported state with its age ("as of 3 min ago", reuse `age()` from `apps/onboarding/src/lib/devices.ts`) and a `StatusBadge` "Hub offline" when `hub_online` is false. No state yet shows a plain "waiting for the Hub" line, never an error page. Poll every 5 s and re-fetch `GET /v1/wizard`; when `current` moves, use `advance()` (focus on the new heading).
- **Onboarding UI** (`apps/onboarding/src/lib/`): `RatesStep.svelte` (lists groups with no rate by name; says it completes by itself; "Moving from another system? Import at the Hub: Settings → Import."), `ProductsStep.svelte` (explains products can be added later at the Hub, the same import line, "Skip for now"), `PrinterStep.svelte` (shows the Hub's printer state ok/failed/skipped/unset with a pointer to Settings → Printer, plus "Skip for now"). Wire them in `routes/setup/[step]/+page.svelte`. Every string lives in the messages catalogue the other steps use (`ui.*` / `onboarding.*` keys); axe passes.
- **Hub UI**: new `apps/hub/src/routes/settings/setup/+page.svelte` listed in the Settings sub-navigation. A checklist (Pair kiosks, Groups, Rates, Printer, Test session) from the snapshot (`setup`, `groups`, `devices`), each row done / not done with a link to its screen (`/settings/devices`, `/settings/layout`, `/settings/printer`; test session has no link yet). Groups created in the web wizard show as done because the snapshot has them. Viewable by any staff session. Strings in `en.json`; axe passes.

**Never:**
- Making a Hub screen or intent depend on the cloud being reachable.
- A new heartbeat endpoint or a new cloud table (reuse `branch_setup_status` and `onboarding_progress`).
- Starting a test session (story 2.29), the go-live checklist (2.31), or completing step 6 from `products_count` (story 5.3).
- Marking step 8 done on `failed` or `unset`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Rates complete | 2 groups, both `has_rate` | step 5 `done`; wizard moves on | N/A |
| Rates pending | 2 groups, 1 without rate | step 5 stays pending; the group's name is listed | N/A |
| No groups | empty `groups` | step 5 stays pending | N/A |
| Printer ok / skipped | `status.printer` ok or skipped | step 8 `done` | N/A |
| Printer failed / unset | failed or unset | step 8 pending | N/A |
| Skip products | owner "Skip for now" | step 6 `skipped`; step 7 opens | N/A |
| Skip printer | owner "Skip for now" | step 8 `skipped` | N/A |
| Skip a done step | step already `done` | state unchanged | N/A |
| Old Hub block | `setup` without `status` / `has_rate` | parses; nothing new completes | N/A |
| Hub offline | `hub_online` false | last state + age + "Hub offline" badge | no error page |
| Hub Setup panel | groups made in the web wizard | Groups row shows done | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/setup_devices.rs` -- `BranchSetup`, `SetupGroup`; add `status`, `has_rate`.
- `crates/arena-proto/src/operating.rs` -- `SetupStatus` (+ JSON test), `TestSessionState`.
- `crates/arena-core/src/setup.rs`, `crates/arena-core/src/snapshot.rs` -- `setup_status`, `read_setup`, `branch_setup`; `crates/arena-store/src/lib.rs` for the manager check.
- `crates/arena-tasks/src/heartbeat.rs:170` -- builds the `setup` block.
- `crates/arena-store/src/control/setup_status.rs` -- `record_setup`; `staff_invites.rs:120` -- `skip_staff_step` to generalise.
- `crates/arena-cloud/src/api/heartbeat.rs:120`, `wizard_staff.rs:264` (skip route), `wizard_devices.rs` (view reused), `mod.rs` (router).
- `apps/onboarding/src/lib/{DevicesStep.svelte,StaffStep.svelte,devices.ts,api.ts,wizard.svelte.ts}`, `routes/setup/[step]/+page.svelte`.
- `apps/hub/src/routes/settings/{+layout.svelte,printer/}`, `apps/hub/src/lib/` snapshot store, `packages/ui` messages.

## Tasks & Acceptance

**Execution:**
- [x] proto + generated types; Hub `branch_setup` / `setup_status` / `manager_exists`; Rust unit tests for each.
- [x] cloud `record_setup` completion rules, shared skip store method, two skip routes; cloud integration tests for each matrix row.
- [x] onboarding `RatesStep`, `ProductsStep`, `PrinterStep`, api client calls, step wiring, messages; Playwright tests (rates auto-complete, skip products, skip and auto printer, offline badge with age, axe).
- [x] Hub `settings/setup` page, nav entry, messages; Playwright test (checklist, links, axe).

**Acceptance Criteria:**
- Given a paired Hub whose groups all have a rate, when it heartbeats, then the owner's wizard leaves step 5 on its own and `GET /v1/wizard` reports `rates` done.
- Given the owner is on step 6, when they press "Skip for now", then step 6 is `skipped` and step 7 is current.
- Given the Hub reports the printer `skipped` or `ok`, when it heartbeats, then step 8 completes and step 9 opens.
- Given the Hub is offline, when the owner opens steps 5–8, then the last reported state shows with its age and a "Hub offline" badge.

## Verification

**Commands:**
- `cargo test --locked --workspace --no-fail-fast`; clippy `--workspace --all-targets -- -D warnings`; `cargo fmt --check`.
- `pnpm --filter @arena/hub check && pnpm --filter @arena/hub test:shell`; `pnpm --filter @arena/onboarding check` and its Playwright tests; every `pnpm run check:*` -- expected: all pass.

## Repo Rules

- Do not add any comments to code (line, block or doc), in any language, even though existing code and specs are comment-heavy. Leave existing comments alone unless the code they describe changes.
- Short, compact code; reuse existing helpers and patterns; no extra abstractions.
