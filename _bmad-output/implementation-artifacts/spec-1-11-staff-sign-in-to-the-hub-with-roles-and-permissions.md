---
title: 'Story 1.11: Staff sign in to the Hub with roles and permissions'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: '7ded0451283d9e9f9b7cd98c2f19f9d4ef23befb'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-10-hub-app-shell-with-left-rail-header-and-floor-placeholder-on.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Every Hub action today runs as the system actor through stub permission checks. Nobody signs in, nothing is attributable to a person, and Counter staff could change things meant for managers.

**Approach:**
- **Permissions:** a fixed permission catalogue in `arena-proto`, with every intent mapped to exactly one requirement.
- **Data:** `staff`, `roles` and `role_perms` tables, with built-in roles seeded at bootstrap and Argon2id password and PIN hashes.
- **Checks:** a real RBAC step inside `apply()`, plus custom roles through `role.upsert`.
- **Sign-in:** loopback sign-in, sign-out and intent endpoints with a lockout, a sign-in screen, and the signed-in staff member shown in the header.

## Boundaries & Constraints

**Always:**
- **Catalogue (`arena-proto`):** `Permission` is a unit enum serialized as dotted strings and grouped by module through `Permission::module()`. Its variants:
  - **player:** `player.manage`, `player.view`
  - **wallet/credit:** `wallet.topup`, `credit.manage`, `credit.collect`
  - **session:** `session.start`, `session.pause`, `session.extend`, `session.transfer`, `session.end`, `session.force_end`
  - **booking:** `booking.manage`
  - **pos:** `pos.sell`, `pos.void`, `refund.issue`, `discount.manual`
  - **kitchen:** `kitchen.use`
  - **stock:** `stock.in`, `stock.adjust`, `stock.override`
  - **catalog:** `catalog.edit`
  - **shift and cash:** `shift.open`, `shift.close`, `cash.manage`
  - **device:** `device.manage`, `device.remote`, `device.pair`
  - **users:** `user.manage`
  - **settings:** `settings.edit`
  - **ops:** `backup.export`, `ops.promote`, `ops.restore`
  - **views:** `report.view`, `audit.view`
- **Intent requirements:**
  - `IntentKind::requirement() -> Requirement`, where `Requirement` is one of `Perm(Permission)`, `System`, `Device` or `Phone`. This is a total match.
  - A unit test enumerates every `IntentKind` and asserts exactly one requirement. A second test asserts every non-implicit permission is required by at least one intent, or is in an explicit "view/ops, used by later stories" allow-list.
  - The current mappings:
    - `branch.bootstrap` → System
    - `settings.set` → `settings.edit`
    - `device.register` → `device.manage`
    - `device.revoke` → `device.remote`
    - `device.version_reported` → System
    - `staff.upsert` and `role.upsert` → `user.manage`
    - `console.plan_requested` → Phone
- **Migration (next number):**
  - `roles(role_id PK, branch_id, name, builtin CHECK(0,1), rev, created_at, updated_at, UNIQUE(branch_id, name))`
  - `role_perms(role_id, permission, effect CHECK(effect IN ('allow','deny')), PRIMARY KEY(role_id, permission))`
  - `staff(staff_id PK, branch_id, login UNIQUE COLLATE NOCASE, display_name, role_id REFERENCES roles, password_hash, pin_hash, pin_verifier_version INTEGER NOT NULL DEFAULT 1, active CHECK(0,1), rev, created_at, updated_at)`
- **Built-in roles,** inserted by `branch.bootstrap`'s mutate with `builtin=1`, each `role_perms` row an allow, as change rows:
  - **Owner:** everything.
  - **Manager:** everything except `ops.restore`.
  - **Counter:** `session.start`, `session.pause`, `session.extend`, `session.transfer`, `session.end`, `pos.sell`, `wallet.topup`, `credit.collect`, `shift.open`, `shift.close`, `stock.in`, `player.manage`, `player.view`.
  - **Kitchen:** `kitchen.use`.
  - **Kiosk-device:** no catalogue rows (Device is implicit).
- **Permission step:**
  - `Perm(p)`: the actor must be `staff` with an active staff row whose role allows `p` and doesn't deny it; otherwise `PERMISSION_DENIED`.
  - An `owner` actor holds every permission. An `admin` actor holds none in this story.
  - `System` requires `actor.kind=system` and `Device` requires `actor.kind=device`; otherwise `PERMISSION_DENIED`.
  - The check runs before any write; it is step 1 in the pipeline.
  - The 1.7, 1.8 and 1.9 stubs are removed. Their tests are updated to use seeded staff, owner and system actors.
- **Intents:**
  - `staff.upsert{staff_id?, login, display_name, role_id, password_hash?, pin_hash?, active}`. Hashes are computed with argon2id outside the transaction. Rule: a Manager may only assign the Counter or Kitchen roles, otherwise `PERMISSION_DENIED`.
  - `role.upsert{role_id?, name, perms: [{permission, effect}]}`:
    - unknown permission string → `VALIDATION_FAILED` with `field="perms[i]"`;
    - targets a built-in role → `PERMISSION_DENIED` "built-in roles cannot be changed";
    - new roles get `builtin=0`.
  - Regenerate the TS types.
- **Dev seed:** registers staff `owner`, `manager`, `counter` and `kitchen`. Their passwords are `dev-<login>-pass`, and their PINs are 1111, 2222, 3333 and 4444. Credentials are printed once and only in the `dev-fixtures` build.
- **Loopback auth (arena-api, on the 127.0.0.1:7409 server, same CORS as 1.10):**
  - `POST /ui/auth/sign-in {login, password}`:
    - success → `{token, staff: {staff_id, display_name, role_name}}`;
    - failure → `AUTH_FAILED` with one fixed message that never says which field failed. Unknown login, wrong password and inactive staff all take equal argon2 work, using a dummy hash for unknown logins.
  - **Lockout:** 5 consecutive failures for a login (case-insensitive) pause it for 60 s. Any attempt in that window gets `AUTH_LOCKED` with `retry_after_s`, and a success resets the counter. This is in-memory and keyed by login.
  - **Sessions:** the token is 32 random bytes in base64url, held in an in-memory session map for that process.
  - `POST /ui/auth/sign-out` with a bearer token removes the session.
  - `GET /ui/me` returns the session staff, or 401.
  - `POST /ui/intent` with a bearer token and `{intent}` overwrites the actor with `{staff, staff_id}` from the session and drops any client `elevated_by`. No token or an unknown token gives `AUTH_FAILED` with a 401.
- **Hub UI:**
  - `/sign-in` route: login and password `TextField`s, a submit `Button`, the fixed error message, and a live countdown while locked, all from the catalogue.
  - The layout gate redirects to `/sign-in` when there is no session, and to `/floor` after sign-in.
  - The header shows the display name and role name, plus a Sign out button.
  - The Settings placeholder gains one demo control: a "Receipt footer" `TextField` and Save that submits `settings.set` with `base_rev`. On `PERMISSION_DENIED` it shows the catalogue refusal inline next to the control, with no modal, and the message never names anyone.
  - After sign-out, the next intent call gets 401 and the UI goes to `/sign-in`.
- **Tests:**
  - Rust: the catalogue totality tests; the RBAC matrix (each built-in role against `settings.set` and `role.upsert`); built-in role immutability; the unknown-permission field error; lockout timing with a ManualClock (make the lockout clock injectable); equal error for an unknown login and a wrong password; actor overwrite.
  - Playwright (hub, using `page.route` stubs): sign-in success shows the name and role in the header; a failure shows the generic message; the lockout countdown; Counter Save shows the inline refusal; sign-out returns to sign-in; axe on `/sign-in`.

**Never:**
- PIN elevation (1.13), idle lock (1.14), setup codes or real onboarding (Epic 2).
- Persisting sessions across Hub restarts.
- Revealing whether a login exists.
- Any `/ui/*` beyond loopback.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Sign in | counter / dev-counter-pass | token; header "Counter · Counter"; `/floor` | N/A |
| Wrong password | counter / bad | `AUTH_FAILED`, the same message as for an unknown login | N/A |
| Unknown login | nobody / x | `AUTH_FAILED`, identical body | N/A |
| Lockout | 5 failures, then a correct password within 60 s | `AUTH_LOCKED retry_after_s` ≤ 60; succeeds after 60 s | N/A |
| Denied | Counter `settings.set` | `PERMISSION_DENIED`; no rows or change rows | rollback |
| Allowed | Manager `settings.set` | applied; `changes.actor_kind=staff`, `actor_id=<manager id>` | N/A |
| Custom role | `role.upsert` with `["pos.sell": allow, "nope.x": allow]` | `VALIDATION_FAILED field="perms[1]"` | rollback |
| Built-in edit | `role.upsert` targeting Counter | `PERMISSION_DENIED` | rollback |
| Manager assigns Owner | `staff.upsert` with the Owner role by a Manager | `PERMISSION_DENIED` | rollback |
| Sign out | sign-out, then `POST /ui/intent` | 401 `AUTH_FAILED` | N/A |
| Spoofed actor | the client sends `actor{system}` | stored with the session's staff actor | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-proto/src/{permission.rs,intent.rs,scope.rs,error.rs}` -- `Permission` (`device.remote` and `device.pair` from 1.9), the intent kinds, the class scope. Add the codes `AUTH_FAILED`, `AUTH_LOCKED` and `PERMISSION_DENIED` if absent.
- `crates/arena-core/src/apply/{permission or guards}` -- the current stubs (bootstrap system-only, the settings.set stub, the device stubs). Replace them.
- `crates/arena-core/src/devices.rs` (1.9) -- the argon2 helpers. Reuse them for staff hashing and verification.
- `crates/arena-api/src/health.rs` + `/ui/snapshot` (1.10) -- the loopback router and CORS. Add the auth and intent routes.
- `crates/arena-hub/src/dev.rs` -- the seed. Add staff.
- `apps/hub/src/routes/**` (1.10 shell) -- the layout, header and settings placeholder. Add sign-in and the gate.
- `packages/ui/src/messages/en.json` -- add catalogue keys.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/*` + generated TS -- the catalogue, requirements, intents and codes, with the totality tests.
- [x] `crates/arena-store` -- the migration and the staff/role queries.
- [x] `crates/arena-core/src/{rbac.rs,apply/*,staff.rs}` -- the permission step, intents and hashing.
- [x] `crates/arena-api/src/ui_auth.rs` -- sign-in/out/me/intent, lockout and sessions, with tests.
- [x] `crates/arena-hub/src/dev.rs` -- seed staff.
- [x] `apps/hub/src/routes/{sign-in/+page.svelte,+layout.*,settings/+page.svelte}`, `apps/hub/tests/auth.spec.ts`.
- [x] `README.md` -- the dev credentials table (dev only) and the auth endpoints.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked` and the dev-fixtures step, then every matrix row has a passing test.
- Given `pnpm check` and the hub Playwright suite, then both pass.

## Implementation Notes

- **Permission step.** `rbac::check` (arena-core) is step 1 of `apply()`; it keeps the 1.9 guards (empty actor id, revoked/unknown device actor, unverified `elevated_by`) and every refusal there is now `PERMISSION_DENIED` (class-scope refusals on the LAN stay `FORBIDDEN`). Rules that need the target row run in a new **authorize** step right after load: built-in roles are immutable, and the Manager rule.
- **Manager rule.** Applied to every `staff` actor whose role is not the built-in Owner (a Manager, or a custom role holding `user.manage`): they may only assign the built-in Counter or Kitchen roles, and only edit staff who currently hold one of them (so a Manager cannot demote an Owner). An `owner` actor and Owner-role staff are unrestricted.
- **Edits.** `staff.upsert`/`role.upsert` with an id that names a row require `base_rev` (`STALE` on mismatch); creates take none. A new staff member needs `password_hash`; `None` keeps the stored hash on an edit. A new `pin_hash` bumps `pin_verifier_version`. `role.upsert` on an existing custom role replaces its rows (one delete change per old row, then inserts). Logins are unique without case (`VALIDATION_FAILED field="login"`), role names unique per branch.
- **`role_perms`** has no `rev`/`updated_at` (per the migration spec), so `Table::is_versioned()` skips it in the stamp step; it is versioned through its role's `rev`. Replay covers the three new tables.
- **Device intents.** `device.register` (`device.manage`) and `device.revoke` (`device.remote`) are no longer system-only: the dev seed, `POST /dev/revoke-device` and the tests now use an `owner` actor for them. `device.version_reported` stays System.
- **`GET /ui/settings`** (bearer, loopback, same guard) was added so the Settings demo can read `receipt.footer`'s `rev` for `base_rev`; it is not in the frozen list but is needed by the "Save with `base_rev`" requirement.
- **Errors.** `/ui/*` auth and intent errors use a new `UiError {code, message, field?, retry_after_s?}`; status codes: 401 `AUTH_FAILED`, 429 `AUTH_LOCKED`, 403 `PERMISSION_DENIED`, 400 `BAD_FRAME`, 422 validation, 409 `STALE`, 503 `NOT_PRIMARY`. Internal errors send the code only.
- **Clippy.** The bigger `IntentPayload` tripped `large_enum_variant` on `FrameBody`; it is allowed there rather than boxing the intent (which would change every construction site).
- **UI.** The token is kept in `sessionStorage` (the Hub's in-memory session map is still the authority; a Hub restart signs everyone out). The layout gate renders a header + main without the rail while signed out. `shell.spec.ts` stubs `/ui/me` and `/ui/settings` and seeds a token so the 1.10 shell tests run signed in.
- **Review fixes.** Restricted actors can't edit their own role or grant permissions their role lacks. The last active Owner-role staff member can't be deactivated or moved to another role. Role names are unique without case (`COLLATE NOCASE`), built-in names are reserved, and a name clash uses the never-recorded field `name_taken`. Kiosk-device can't be assigned to people. Logins are ASCII `[A-Za-z0-9._-]{2,32}`. The lockout reserves in-flight attempts and never evicts live counts. Sessions re-read the staff row on every request, last at most 12 h, are capped at 5 per member, and sign-out is idempotent. In the UI, an unknown session shows the retry state, sign-in returns to `redirectTo`, idempotency keys are minted by the caller, and the Settings demo reloads the server `rev` and keeps the user's text on STALE.
- **`check:generated`** compares the regenerated folder with git, so it passes only once the new generated files are committed; regeneration was verified to be byte-stable.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | `role.upsert` lets a Manager or custom `user.manage` holder grant any permission, including to their own role (all three layers) | high | `authorize_role` only blocks built-ins. | patch |
| 2 | Parallel sign-ins pass the lockout check together (all three layers) | medium | `check` and `record_failure` take separate locks around argon2. | patch |
| 3 | Spraying junk logins wipes a target login's failure count (all three layers) | medium | At 4096 entries, every unlocked count is dropped. | patch |
| 4 | Sessions never expire, never reflect deactivation or role changes, and grow without limit (all three layers) | medium | The `SessionStaff` cache is never refreshed. | patch |
| 5 | The last active Owner can be deactivated or demoted | medium | No invariant check. | patch |
| 6 | An unknown `/ui/me` state renders the full shell to nobody (all three layers) | medium | `gated` is false when the state is unknown. | patch |
| 7 | UI retries mint a new idempotency key; `rev += 1` is a guess | medium | Defeats 1.8 exactly-once. | patch |
| 8 | STALE discards typed text; a missing setting leaves Save disabled forever (blind, edge) | low | Direct UI fix. | patch |
| 9 | The role-name conflict is recorded but depends on state (blind, edge) | low | The `name` field is on the allow-list. | patch |
| 10 | Login folding is inconsistent (Unicode vs NOCASE); role names are case-sensitive and can shadow built-ins; Kiosk-device can be assigned to staff | low | Direct validation. | patch |
| 11 | Sign-in drops the deep link; double navigation | low | Add return-to. | patch |
| 12 | `status_of` is not exhaustive | low | New codes become 500. | patch |
| 13 | README omits the custom-role restriction and the Deny semantics | low | Doc fix. | patch |
| 14 | Proptest forces Owner, so the permission step is never exercised | low | Add staff actors. | patch |
| 15 | Auth UI tests: unknown session, STALE, 5xx `/ui/me`, malformed body | low | Pre-verified gaps. | patch |
| 16 | Branch bootstrapped before migration 0005 has no roles (edge, verify-gap) | low | Pre-release: only dev data dirs exist, and `dev seed` recreates them. | reject |
| 17 | More sheet stays over the sign-in screen | low | Close it when gated. | patch |
| 18 | A non-numeric `retry_after_s` causes NaN | low | Direct fix. | patch |
| 19 | A malformed 200 sign-in leaves an orphan session token | low | Sign out the token. | patch |
| 20 | rbac doc says "active" but doesn't check it | low | Direct fix. | patch |
| 21 | Sign-out of an unknown token returns 401; README says 204 | low | Make it idempotent 204. | patch |
| 22 | The `perms[` recordable prefix is untested (verify-gap) | low | Pre-verified gap. | patch |
| 23 | Custom `user.manage` restriction is untested (verify-gap) | low | Pre-verified gap. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub build && pnpm --filter @arena/hub test:shell` -- expected: pass (Node 22.23.4)
