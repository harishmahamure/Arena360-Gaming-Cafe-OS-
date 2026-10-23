---
title: 'Story 1.15: Audit log viewer on the Hub'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'e309532127080270896deed47a9e64719e7da63e'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-13-inline-manager-pin-elevation.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The change log records every write, but nobody at the branch can read it. "Who changed this?" can't be answered offline.

**Approach:** Add read-only loopback audit endpoints over `changes`: a filtered, keyset-paged list, plus a detail view that shows the before/after diff. They're gated by `audit.view`, or by a manager PIN read-elevation through the 1.13 ticket flow. They feed a System → Audit screen in the Hub UI with filters, "Load more" and a side-by-side diff with text markers.

## Boundaries & Constraints

**Always:**
- **Read gate:**
  - `/ui/audit*` needs a bearer session whose staff role allows `audit.view`, or a valid read-elevation.
  - Read-elevation extends 1.13: `POST /ui/elevate {permission: "audit.view", pin}` is accepted in place of an `intent_kind`. It is allowed only for read permissions (`audit.view`, `report.view`, `player.view`).
  - It issues a read ticket bound to the session and that permission, valid 10 min with unlimited reads. A session lock (1.14) or sign-out drops it.
  - Requests pass it as the header `X-Arena-Elevation`. Without a grant the endpoint answers `PERMISSION_DENIED` (403), and no audit data is ever returned without one.
- **List: `GET /ui/audit?actor=<kind:id>&module=<domain>&from=<ms>&to=<ms>&cursor=<opaque>&limit=<1..100, default 50>`**
  - Order is newest first by `(at DESC, epoch DESC, seq DESC)`. The cursor is that triple, base64url-encoded, used for keyset pagination (no OFFSET).
  - Each item: `{epoch, seq, at, business_day, actor: {kind, id, name}, elevated_by: {kind, id, name, mode} | null, module, intent_kind, intent_id, tbl, row_id, op}`.
  - `module` is the `domain` part of `intent_kind`.
  - Names come from `staff.display_name` for staff and `devices.label` for devices. `system` and `owner` show catalogue-style fixed labels, provided by the server as keys the UI translates. An unknown id shows the id itself.
  - Filters combine with AND. The module filter matches `intent_kind LIKE 'domain.%'`.
  - The response carries `next_cursor`, which is null at the end.
- **Indexes (migration):** `changes(at)`, `changes(actor_kind, actor_id, at)` and `changes(intent_kind, at)`.
- **Detail: `GET /ui/audit/item?epoch=&seq=`**
  - Returns `{item, after: map, before: map | null, changed: [field]}`. `before` is the row image of the previous change to the same `(tbl, row_id)` in log order; null means created. For a delete, `after` is the pre-delete image and `op` is shown.
  - The MessagePack images are decoded to JSON.
  - **Redaction:** `password_hash`, `pin_hash` and `token_hash`, plus any column named in `arena-core::audit::REDACTED`, are replaced with the marker `"[redacted]"` in both images. A changed redacted field shows only "changed".
  - **Personal data:** columns tagged as player PII have no tables yet. The rule is recorded in `REDACTED` / `ID_ONLY` consts for later epics, so the viewer shows player references as ids only.
- **Performance:**
  - A Rust test generates 100,000 change rows across 5 actors and 3 modules in one transaction. It asserts that a filtered first page (actor and date range) and the unfiltered first page each return in < 250 ms in debug on CI hardware. That is a proxy for the reference-hardware budget of < 1 s; the release benchmark is noted in the README.
  - The test sits behind `#[ignore]` only if it takes > 10 s in debug. Otherwise it runs normally.
- **Hub UI `/system/audit`** (the System route gains a sub-nav with Audit):
  - **Filters:** actor (a select of staff names plus System/Owner/Devices), module (a select of known domains), and a from/to date pair in the branch timezone. "Clear filters" resets them all in one action.
  - **List:** a table with time (`formatDateTime`, branch tz), actor name and kind, module, intent, and the record (`tbl` / `row_id`). Newest first. It has a "Load more" button and also loads the next page when an `IntersectionObserver` sees a sentinel.
  - **Empty state:** the catalogue line shown when nothing matches.
  - **Detail panel:** opens on row activation (Enter/click) in the same page. Before and after sit in two columns. A changed field carries the visible text marker "Changed" plus `aria-label`, not colour alone. A created record shows "Created" in the before column.
  - **Elevated changes:** show "Approved by {manager} ({mode})".
  - The screen is read-only: no edit or delete controls exist.
  - **Counter without `audit.view`:** the Audit entry stays visible. Opening it shows the `InlinePin` in place, and after approval it loads with the read ticket. If the PIN is cancelled, no data is fetched or shown.
- **Offline:** everything is local. Nothing calls the WAN.

**Never:**
- Edit, delete or export controls in the viewer.
- The cloud audit builder (later epic).
- Showing hash values or player PII.
- OFFSET pagination.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Manager list | manager session, no filters | newest first, 50 items, `next_cursor` set | N/A |
| Page 2 | cursor from page 1 | the next 50 with no overlap and no gap | N/A |
| Filter | actor=staff:counter, module=settings, a date range | only matching rows | N/A |
| No match | a module with no changes | empty list; the UI shows the empty-state line | N/A |
| Detail update | a `settings.set` change | `before` = the previous image; `changed=["value","rev","updated_at"]` | N/A |
| Detail create | the bootstrap branch row | `before` null; "Created" shown | N/A |
| Redaction | the staff row change | `password_hash` and `pin_hash` are `[redacted]` | N/A |
| Elevated | a 1.13 elevated change | `elevated_by` shows the name and mode | N/A |
| Counter | counter session, no ticket | 403 PERMISSION_DENIED; the UI shows InlinePin; no data | N/A |
| Read elevation | the manager PIN then the list | 200 within 10 min; 403 after sign-out or lock | N/A |
| Perf | 100k rows | first page < 250 ms (debug) | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-store/src/branch.rs` / `read_changes` (1.7) -- the change row decoding and the msgpack row images. Add the audit queries and the migration with the indexes.
- `crates/arena-api/src/ui_auth.rs` (1.11, 1.13, 1.14) -- sessions, elevation tickets and the lock state. Add read-elevation and the audit routes.
- `crates/arena-core` -- the snapshot read path pattern (a read-only connection and `read_consistent`). Add `audit.rs`: name resolution, diff and redaction.
- `packages/ui/src/format.js` (1.12) -- `formatDateTime`. `InlinePin` comes from 1.13.
- `apps/hub/src/routes/system/**` -- the placeholder from 1.10. Add the sub-nav and the Audit page.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-store` migration with indexes plus audit list/detail queries.
- [x] `crates/arena-core/src/audit.rs` -- diff, redaction and names, with tests.
- [x] `crates/arena-api/src/ui_audit.rs` -- the endpoints and read-elevation, with tests for every API matrix row including perf.
- [x] `crates/arena-proto` -- the audit DTOs, plus generated TS.
- [x] `apps/hub/src/routes/system/audit/+page.svelte` and its components; `apps/hub/tests/audit.spec.ts`.
- [x] Catalogue keys and the README.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked`, `pnpm check` and the hub Playwright suite, when they run, then all pass with every matrix row covered.

## Implementation Notes

- **Read elevation extends the 1.13/1.14 items API** (no parallel path). `ElevationItem` is now `{intent_kind?, key?, permission?}`: an intent item as before, or a read item `{permission}` (one of `Permission::READ_ELEVATION` = `audit.view`, `report.view`, `player.view`), which must be the only item (else `VALIDATION_FAILED` on `items`). It reuses the candidate query, the staff-id throttle, the semaphore, the "already holds it" refusal and `issue_ticket` (so the 1.14 lock re-check under the tickets lock applies). Only differences: a follower may grant it (nothing is written), the ticket lives `READ_TICKET_TTL_MS` = 10 min, and it never matches an intent (`redeem` needs `intent_kind`), so using it on `/ui/intent` is `PIN_REQUIRED`. `drop_tickets` on lock, switch-unlock and sign-out drop it; it counts toward the 4-per-session cap (eviction takes the soonest-expiring, so it goes last).
- **Gate.** `ui_audit::gate` uses `acting_session` (so a locked session gets 423 `SESSION_LOCKED`, which the UI already turns into the lock screen), then `audit.view`, then `UiAuth::read_granted(ticket, token, staff_id, audit.view)`. Everything else is 403 `PERMISSION_DENIED` with one message, before any read. After sign-out the old token is 401; a new session or an unlocked session with the old ticket is 403 (tested).
- **Facets endpoint.** The UI's actor select needs staff names and the module select needs the known domains, so `GET /ui/audit/facets` (same gate) answers `{staff, modules}` (modules from `IntentKind::ALL`). Not in the frozen API list, but required by the UI bullets.
- **Indexes.** The list indexes end in `(epoch, seq)` — `changes(at, epoch, seq)`, `changes(actor_kind, actor_id, at, epoch, seq)`, `changes(intent_kind, at, epoch, seq)` — supersets of the listed ones, so the keyset order is read from the index without a temp B-tree (checked with `EXPLAIN QUERY PLAN`). A fourth index `changes(tbl, row_id)` serves the detail's previous-image lookup. The module filter is the index-friendly range `intent_kind >= 'm.' AND < 'm/'` (same set as `LIKE 'm.%'` for the lower-case domains the parser accepts).
- **Ordering.** The list orders by `(at DESC, epoch DESC, seq DESC)` as specified (epoch as text). "Previous change in log order" for `before` uses the replay order (numeric epoch counter, epoch text, seq), computed in Rust over the row's changes.
- **Names.** `actor.name` is the staff display name / device label, or the catalogue key `hub.audit.actor.{system,owner,admin}` for those kinds (the UI translates it); unknown ids show the id. `changed` is computed before redaction, so a changed secret is listed while both images read `"[redacted]"` (a null stays null). `changed` is empty for a created row.
- **Query parsing.** `from` inclusive, `to` exclusive; `actor=<kind>` (any id, used by the Devices option) or `<kind>:<id>`; empty values count as absent; malformed values are 422 `VALIDATION_FAILED` naming the parameter; `from > to` names `to`. Detail of an unknown change is 422 on `seq`.
- **Perf.** The 100k-row test inserts in one transaction (~5.5 s debug here, under the 10 s threshold, so not ignored) and measured ~3 ms per first page through the HTTP route.
- **UI.** Dates are converted with the new `zonedDayStart(day, tz)` / `nextDay(day)` in `@arena/ui/format` (unit-tested, DST-safe), `to` sent as the next day's start. The read ticket lives in page memory only. A `PERMISSION_DENIED` after approval clears every row and the panel and re-opens the PIN with "Enter the PIN again."; a staff switch on unlock resets the page. The "Changed" marker is a table cell with visible text and an `aria-label` (allowed on cells, unlike a bare span). `/system` redirects to `/system/audit`; the shell spec now treats System like Settings (own sections).
- **CORS.** The preflight now allows `x-arena-elevation`.
- **Review fixes.** Each read re-checks the approver (active, permission, `pin_verifier_version`; no cache) and drops the ticket otherwise. Grants are recorded on a primary as the system intent `audit.read_granted` (new `read_grants` table, Requirement System, category `audit`); a follower logs them at info. Migration 0008 (unshipped) now adds the virtual generated columns `epoch_counter` and `module` and indexes `(at, epoch, seq)`, `(actor_kind, actor_id, at, epoch, seq)`, `(actor_kind, at, epoch, seq)`, `(module, at, epoch, seq)`, `(tbl, row_id, epoch_counter, epoch, seq)` (the `intent_kind` index is gone: the module filter is equality on `module`, so dotless kinds match their whole kind); a plan test asserts no temp B-tree for every filter combination. `previous_row_image` is one indexed query, and a previous delete gives `before: null`. `ID_ONLY` values are reduced to their leading id. Unknown stored kinds/modes map to `unknown` (new `AuditActorKind`/`AuditElevationKind`/`AuditElevationMode`). `zonedDayStart` returns the first instant of a day whose midnight a DST change skips. The UI gives detail and load-more failures their own states, checks From ≤ To inline, shows a server-named filter error, drops the ticket on leave, offers Admin, re-asks for approval on any denial, resumes after an unlock, and keeps loading while the sentinel is in view.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Read ticket never re-checks the approver's active state, permission or verifier version (all three layers) | medium | A revoked manager's approval keeps granting audit reads for 10 min. | patch |
| 2 | Read grants leave no durable audit record | medium | Only a tracing line. | patch (system intent `audit.read_granted` when primary; log on follower) |
| 3 | `ID_ONLY` declared but unused | low | Docs promise enforcement. | patch |
| 4 | `previous_row_image` loads the row's whole history (blind, edge) | medium | O(history) per detail open. | patch (single LIMIT 1 query and index) |
| 5 | Module-only and kind-only filters sort with a temp B-tree; the plan and perf tests miss them (blind, verify-gap) | medium | Verified with EXPLAIN. | patch |
| 6 | A re-created row shows the deleted image as "before"; deletes are unclear (blind, edge) | low | Wrong "Created" marker. | patch |
| 7 | A failed detail load hides the list; a failed load-more drops loaded rows (blind, edge) | low | Shared `loadFailed`. | patch |
| 8 | A from > to range or other 422 shows a generic failure; Retry loops (blind, edge) | low | No field surfaced. | patch |
| 9 | Read ticket not dropped on navigate-away; evictable by intent tickets (blind, edge) | low | Slot pressure. | patch |
| 10 | No Admin option in the actor filter | low | Kind exists. | patch |
| 11 | A server-side permission loss leaves a blank screen; a lock during the first load leaves it blank (blind, edge) | low | Missing branches. | patch |
| 12 | Facets read outside one snapshot | low | Doc says one snapshot. | patch |
| 13 | README omits the 422 for a missing change | low | Doc fix. | patch |
| 14 | 100k perf test may flake on slow CI | low | Add a warm-up and a looser CI bound via env. | patch |
| 15 | Infinite scroll stalls after a short page (blind, edge) | low | Observer only fires on change. | patch |
| 16 | One bad stored enum fails every page containing it | low | Fall back to a placeholder item. | patch |
| 17 | `intent_kind` without a dot can't be matched by the module filter | low | Direct fix. | patch |
| 18 | Midnight DST gap in `zonedDayStart` | low | Direct fix. | patch |
| 19 | CORS `x-arena-elevation` preflight untested (verify-gap) | medium | Pre-verified; would silently break the Counter flow. | patch |
| 20 | Audit page reset on a switch-unlock untested (verify-gap) | medium | Pre-verified; risks a data leak. | patch |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm check && pnpm --filter @arena/hub build && pnpm --filter @arena/hub exec playwright test` -- expected: pass
