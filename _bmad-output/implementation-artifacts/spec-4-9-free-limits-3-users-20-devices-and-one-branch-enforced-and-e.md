---
title: 'Story 4.9: Free limits — 3 users, 20 devices and one branch — enforced and explained at the Hub'
type: 'feature'
created: '2026-10-28'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-8-a-free-café-goes-from-sign-up-to-go-live-on-android-with-no.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** `USER_LIMIT` was already enforced in `staff.upsert`, but the Hub had no screen that says how many users are left, no way to free a slot (`staff.deactivate` did not exist) and no proof that the device, branch and missing-claim limits hold on the Android Hub.

**Approach:** Count the right people (active and invited, never Kiosk-device), add an Owner-only `staff.deactivate` intent confirmed with the Owner's own PIN, and a Settings → Staff screen with "N of M users" and `tier-lock` at the limit. The device, branch and missing-claim limits are proven against the existing paths.

## Boundaries & Constraints

**Always:**
- Limits come from the licence (`licence.user_cap`, `licence.device_cap`), falling back to the `arena-proto` Free table (users 3, devices 20, branches 1).
- A refused deactivation writes nothing; the row is kept (history keeps the name) with `active = 0`.
- `tier-lock` shows "Available on Basic" with Upgrade hidden and "Available soon" until Story 10.5 (`billingEnabled`).

**Never:**
- No ads, no billing, no new cloud endpoints. `staff.deactivate` is not accepted on the generic `/ui/intent`.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Fourth user on Free | 3 active or invited | `USER_LIMIT`, nothing written |
| Deactivate by Owner, right PIN | member active | member inactive, sign-in ends at once, slot freed, audit actor = Owner and row = member |
| Wrong PIN | any | `PIN_REJECTED`, nothing written; five in a row pause for 60 s |
| Self, last Owner, non-Owner, unknown id | any | `VALIDATION_FAILED` / `PERMISSION_DENIED`, nothing written |
| Missing or unverifiable `limits` claim | licence | Free defaults 3 / 20 / 1 |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{intent.rs,staff_list.rs}` -- `staff.deactivate`, `StaffList`, `StaffView`, `UiStaffDeactivateRequest`.
- `crates/arena-store/src/staff.rs` -- `count_active_staff` (excludes Kiosk-device), `deactivate_staff`, `list_staff`; `Store::staff_overview`.
- `crates/arena-core/src/apply/staff_deactivate.rs`, `snapshot.rs` (`staff_list`), `staff.rs` (`verify_own_pin`).
- `crates/arena-api/src/ui_auth.rs` -- `GET /ui/staff`, `POST /ui/staff/deactivate`.
- `apps/hub/src/routes/settings/staff/+page.svelte`, `apps/hub/src/lib/staff.ts`, strings in `en.json`, audit label.

## Tasks & Acceptance

**Execution:**
- [x] `staff.deactivate` intent, handler, proptest refusals and core tests (Owner only, self, last Owner, slot freed on Free).
- [x] User count: active and invited, never Kiosk-device.
- [x] `GET /ui/staff`, `POST /ui/staff/deactivate` (own PIN, elevation throttle) and API tests.
- [x] Settings → Staff screen with "N of M users", `tier-lock` at the limit, deactivate with PIN; Playwright spec.
- [x] `DEVICE_LIMIT` on the Android Hub and the staff list, added to the 4.8 host test.
- [x] Free defaults when the `limits` claim is missing; Cloud second Free branch and Add branch refusals confirmed by the existing `tier` and `branches` tests.
- [x] LLD row, `deferred-work.md`.

**Acceptance Criteria:** those of Story 4.9 in `epics.md`.

## Verification

**Commands:**
- `cargo test --locked -p arena-proto -p arena-store -p arena-core -p arena-api --features arena-api/dev-fixtures`
- `cargo test --locked -p arena-hub --features dev-fixtures --test free_android_golive`
- `cargo test --locked -p arena-cloud --test tier --test branches`
- `cargo clippy --locked -p arena-proto -p arena-store -p arena-core -p arena-api -p arena-hub --features arena-hub/dev-fixtures --all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm --filter @arena/hub check`, `npx playwright test` in `apps/hub`, `check:strings`, `check:protocol`, `check:layers`, `check:pins`, `check:secrets`, `check:hex`, prettier.

## Verification Log

Pass: all of the above. The full Hub Playwright run had one flake (`Ctrl+K` in `players.spec.ts`, phone) that passes alone. Not run: an Android device or emulator; see `deferred-work.md`.
