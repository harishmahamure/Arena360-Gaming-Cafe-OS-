---
title: 'Story 8.38: Audit search in Reports with "Preparing audit…"'
type: 'feature'
created: '2026-10-30'
status: 'review'
baseline_commit: '7d346cc1'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
deferred: []
---

<intent-contract>

## Intent

**Problem:** The cloud can answer audit questions about a built month (Story 8.37), but the owner has no screen to ask them, and the first ask of a month waits on a build the owner cannot see.

**Approach:** Reports gains an Audit tab: branch, month and one catalogue search; a never-built month is started, shown as "Preparing audit…" with its progress and polled until the first page arrives; later searches return at once.

## Boundaries & Constraints

**Always:**
- One search names one catalogue entry and only its parameters: Everything (`recent`), Who (`actor`: kind, optional id), Part of the app (`module`), What was done (`intent`, typed text), A player (`entity` on `players` with the player id). A search that cannot be valid (intent not `[a-z0-9_.]`, empty player id) is stopped before it is sent.
- `NOT_FOUND` starts the build (`POST …/audit/build`); `AUDIT_BUILDING` keeps polling every 2 s; `AUDIT_BUILD_FAILED` shows "Couldn't prepare the audit." with Retry, which starts a new attempt. Preparing is muted text on a surface panel with a progress bar, no overlay or spinner.
- Rows show time (branch zone), actor (staff name, "Owner", "System", device label) with its kind, intent, module, the record (`op: table / id`) and, when elevated, "PIN by Priya" or "Owner MFA". Players appear as ids. `orphaned` rows (first page) are listed apart under "Not applied (orphaned)".
- Audit answers are never cached in the app's IndexedDB or the service worker.

**Never:** row images or changed values (the cloud sends none), combining two filters in one search, the admin app UI.

## Spec Change Log

- The epic asks for module filters "sessions, money, POS, stock, staff, settings, commands". The catalogue takes one intent domain per search, so the module list is domains (sessions, wallet, credit, refunds, cash, POS orders, stock, products, rates, plans, coupons, staff, settings, devices).
- "Changed fields" cannot be shown: the audit page is change metadata only (Story 8.37), so a row names the operation, table and row id.
- "Player id" as an actor is a search over the `players` table by row id, since a player is not an actor kind in the catalogue.
- The first-results benchmark (< 10 s on a busy staging month) needs the staging environment and is not measured here.

## Verification

**Commands:**
- `pnpm -C apps/owner check` -- expected: 0 errors
- `pnpm -C apps/owner exec playwright test tests/audit.spec.ts tests/reports.spec.ts tests/a11y.spec.ts` -- expected: pass
- `pnpm check:strings && pnpm check:layers && pnpm format:check` -- expected: ok

## Auto Run Result

Status: review
Implemented: `AuditPanel.svelte` and `audit.ts` (catalogue params, build/poll flow, labels) in the owner app; Audit tab in `ReportsBoard` (shared range controls and exports hidden for it); `api.probe` (status-keeping, uncached read); `owner.audit.*` strings. Tests: `apps/owner/tests/audit.spec.ts` (9 cases on desktop and phone: first search with preparing then page, ready month without build, failed build with Retry, row content with PIN/MFA and orphaned section, Load more cursor, each filter's parameters, pre-send validation, empty and refused answers, axe and overflow). Known unrelated failures: `pnpm check:tokens-used` (`--ad-reserved` in the Hub floor page) and `test:scripts` `check-deploy` (no kubectl).
