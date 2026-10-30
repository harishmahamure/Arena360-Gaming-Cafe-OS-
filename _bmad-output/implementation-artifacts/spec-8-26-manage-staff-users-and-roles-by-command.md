---
title: "Story 8.26: Manage → Staff users and roles by command"
type: 'feature'
created: '2026-10-30'
status: 'done'
baseline_commit: '818ac4b41501f4d8968b098aa9a83f85d7ee4234'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-24-manage-products-and-bundles.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-25-step-up-mfa-for-sensitive-commands.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The owner cannot manage the team remotely: staff are added only in the wizard (one branch, once) or at the Hub, and a lost phone or a leaver means a trip to the café. The first real sensitive commands (8.25 built the gate, nothing uses it) are staff and roles.

**Approach:** Manage gets a third area, Staff & roles (`?area=staff`). It lists each live paid branch's staff (name, number, role, active) grouped by number, and the roles with their permissions. `staff.upsert`, `staff.deactivate` and `role.upsert` become owner command kinds, handled by the Hub and **sensitive**, so every send passes step-up MFA (8.25). The cloud, never the app, makes a new member's setup code: it adds only the hash and a 72 h expiry to the payload and returns the code once in the answer to the post.

## Boundaries & Constraints

**Always:**
- Owner `staff.upsert` payload is `OwnerStaffUpsert` (`deny_unknown_fields`): `staff_id?`, `login` (a phone for a new member), `display_name`, `role_id` (a role id of the branch or a built-in role name), `active`, `setup_code?`. A password, PIN or hash in it is refused `VALIDATION_FAILED` before the step-up check.
- Without `staff_id` the cloud adds a member: it mints the id (derived from the post key and branch, so a repeat names the same member), a setup code (`new_pairing_code`), `setup_code_hash` (argon2id) and `setup_expires_at = now + SETUP_CODE_HUB_TTL_MS`. With `staff_id` and `setup_code: true` it issues a new code for that member. Otherwise no code.
- The post answer carries `setup_codes` (`{branch_id, staff_id, code}`, `Debug` redacted) and `Cache-Control: no-store`. A post sent again with the same key finds the first row; its hash does not match a new code, so no code is returned. The owner list shows a staff payload without the hash and with `setup_code: true` (Retry then asks for a fresh code).
- `role.upsert` is `RoleUpsert`; every `perms[i].permission` must be in the `arena-proto` catalogue or the cloud refuses it (`perms[i]`) before anything is queued. `staff.deactivate` is `{staff_id}`.
- The three kinds are in `OWNER_COMMAND_KINDS` (`sensitive`, `handled`), in the Hub's `HANDLED_COMMAND_KINDS` (16), and the Hub treats the owner (cloud) as an `Owner` actor: `staff.deactivate` accepts it as it does a Hub Owner. The Hub still refuses deactivating or demoting the last active Owner, editing a built-in role, a taken login and a built-in role name; the Owner app shows each as "Couldn't apply at branch".
- Staff and role rows join `owner_wins` (`edited_row`): `base_rev` becomes the row's current `rev` and an override is recorded. The owner-update toasts gain "Staff member updated by owner" (also for a deactivation) and "Role updated by owner" (`OWNER_UPDATES`, `OwnerUpdate::covers`).
- Summary catalogue (additive, no schema bump): `CatalogueStaff.login` and `role_id`; role rows carry `fields.permissions` (`[{permission, effect}]`, built-in roles too).
- Owner app: ids are per branch, so the app builds one variant per branch. A person is told apart by login, a role by name. Adding skips a branch that has the number or lacks the role, with the reason; an edit or deactivation goes only where the person is on the team; deactivation skips a branch where they are already inactive. The shown setup codes live only in `cmd.codes` until Done; they are never written to IndexedDB, and a queued (offline) add shows its code when it finally posts.

**Never:** a credential in a payload or log, a stored or cached code, a `Free` branch being offered these kinds (the cloud's tier check already refuses them), editing a built-in role, `hub.revoke` (8.27).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Add a member | name, `+91 98765 43210`, role Counter, Send, code | step-up dialog, then `staff.upsert {login, display_name, role_id, active}` per branch; "Setup code for Neha: K7Q4 9MZD…" once, Copy, Done | bad name/number/role: copy inline, no preview |
| Add to All branches | two branches | one post and one code per branch; each branch's own role id | a branch with the number or without the role is left out with the reason |
| Edit | role changed | `staff.upsert {staff_id, login, display_name, role_id, active}` with the row's `base_rev`, no code | branch where the person is absent or the role missing: left out |
| New code | "Issue a new setup code" | `setup_code: true`, one code | Hub refuses once they chose a password |
| Deactivate | Deactivate Ravi | `staff.deactivate {staff_id}` where active | last Owner: rejected, "Couldn't apply at branch" |
| Role edit | custom role | `role.upsert {role_id, name, perms[allow]}` with `base_rev` | built-in name refused inline; unknown permission refused by the cloud |
| Free branch | tier free | not in Manage (tier lock) | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/owner_commands.rs` -- three kinds (`sensitive()`), `OwnerStaffUpsert`, `StaffSetupCode`, `OwnerCommandPosted.setup_codes`, role permission check, `OWNER_UPDATES` (7), `OwnerUpdate::covers`; `summary.rs` -- `CatalogueStaff.login/role_id`.
- `crates/arena-store/src/summary.rs` -- staff login/role id, role permissions.
- `crates/arena-core/src/{commands.rs,apply/staff_deactivate.rs}` -- handled kinds (16), owner in `edited_row`, Owner actor may deactivate. `crates/arena-api/src/ui_events.rs` -- `covers`.
- `crates/arena-cloud/src/api/commands.rs` -- `checked_staff`, `staff_part` (code + hash), `shown_payload`, `no-store`.
- `apps/owner/src/lib/staff.ts` (new) -- people, roles, permission catalogue (`Record<Permission, true>`), validators, `proposeStaffAdd|Edit|Deactivate`, `proposeRole`; `StaffBoard`, `StaffPanel`, `StaffForm`, `RoleForm`, `SetupCodes` (new); `ManageBoard` third area; `commands.svelte.ts` kinds, `cmd.codes`, `takeCodes`, `currentBaseRev` for staff/role ids.
- `apps/hub/src/lib/owner-update.ts`, `packages/ui/src/messages/en.json` -- toast lines, `owner.staff.*`, `owner.roles.*`.
- Tests: `crates/arena-cloud/tests/stepup.rs` (4), `arena-core` apply + summary tests, `arena-proto` unit tests, `apps/owner/tests/staff.spec.ts` (10 × desktop/phone), Hub `groups.spec.ts` toast rows.

## Tasks & Acceptance

**Execution:**
- [x] Proto/store/core: kinds, payload types, summary fields, owner-wins, Owner actor deactivates, toast table; Rust tests.
- [x] Cloud: payload checks before step-up, setup code and hash per branch, no-store answer, owner-visible payload; Rust tests.
- [x] Owner app: Staff & roles area, forms, setup codes panel, strings; Playwright tests.

**Acceptance Criteria:**
- [x] Given the catalogue, then `staff.upsert`, `staff.deactivate` and `role.upsert` are sensitive with payload schemas and Hub handlers, and the Hub toasts "Staff member updated by owner" and "Role updated by owner".
- [x] Given Manage → Staff & roles, then staff per branch (name, phone, role, active) and the roles with their permissions are listed from the summary.
- [x] Given a new member for one branch or All, when sent with step-up, then the cloud makes one 8-character code per branch, the Hub gets only its hash and a 72 h expiry, and the app shows "Setup code for Neha: K7Q4 9MZD. Give it to Neha; it works once." with Copy.
- [x] Given a role change or deactivation, then it is sent with step-up and the Hub applies it; the last Owner cannot be deactivated (rejected, "Couldn't apply at branch").
- [x] Given a custom role edit, then permissions come only from the catalogue and the cloud refuses an unknown one before sending.
- [x] Given a Free branch, then it is not offered (tier lock "Available on Basic"), so it never reaches its 3-user limit from here.

## Spec Change Log

- The epic's copy says "Give it to her"; the app says "Give it to {name}" because the owner's staff have no recorded pronouns.
- The epic keys one command by "one branch or all"; staff ids differ per branch, so an edit or deactivation reaches the branches where the person's number is on the team (one post per branch, like 8.23), and an add uses the same per-branch posts so each branch gets its own role id and code.

## Review Triage Log

Code review (2026-10-30): no blocking findings. clippy, cargo tests (proto, store, core, cloud, api), owner svelte-check, staff Playwright spec (20), check:strings/layers/protocol and format:check all green. Unrelated flaky Windows tests (arena-cloud binary `unreachable_at_start...`) not touched by this story.

## Design Notes

- Why per-branch posts for an add: a custom role has a different id at each branch, and the cloud cannot map one name to many ids. The cloud still supports `all` for a built-in role name.
- The summary now carries staff phone numbers (`login`) as the epic asks; no password, PIN or hash is in it.
- Explicit `deny` permission rows are listed but the role form edits "allowed" only: saving a role drops its deny rows (an absent permission is denied).

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-api`, `cargo clippy -p arena-proto -p arena-store -p arena-core -p arena-cloud -p arena-api --all-targets -- -D warnings`, `pnpm run gen:types`, `check:strings`, `check:layers`, `check:protocol`, `format:check`; in `apps/owner`: `pnpm run check` and `pnpm exec playwright test` -- expected: clean

## Auto Run Result

Status: in-review. Not run: staging proof of the end-to-end owner flow, the code-review pass.
