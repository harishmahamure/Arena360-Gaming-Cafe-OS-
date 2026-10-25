---
title: 'Story 8.12: On Free, the Owner app shows only Subscription, pairing and support'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'bcd50e0'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-8-10-the-owner-app-shell-navigation-themes-install-and-free-limit.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Story 8.10 locks the four data sections only when every branch is Free. A mixed tenant sees one undifferentiated placeholder, so a Free branch is neither named nor locked.

**Approach:**
- `Section` renders, for a mixed tenant, one entry per branch: a Free branch shows its name and `TierLock` "Available on Basic" in place of its figures and controls; a paid branch shows the section content.
- An all-Free tenant keeps the single `TierLock` (8.10). Subscription, Branches and Support stay usable on Free.

## Boundaries & Constraints

**Always:**
- Reuse `Section`, `TierLock`, `isFree`; no new strings or components.
- No "Upgrade" link (until Epic 10). Free makes no owner read call beyond `/v1/session` and `/v1/branches`.
- A Free branch that upgrades unlocks on the next app open (the shell reads fresh branches on every open), without signing out.
- Applies to Today, Reports, Alerts and Manage.

**Never:**
- Purchase controls, an Upgrade link, or a new read API call.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| All Free | every branch free | one tier-lock per section; Subscription, Branches, Support usable |
| Mixed | one free, one paid | Free branch: name and tier-lock; paid branch: section content |
| All paid | no free | section content, no lock |
| Upgrade | branch becomes paid, app reopened | section unlocks, no sign-out |

</frozen-after-approval>

## Code Map

`apps/owner/src/lib/Section.svelte`, `apps/owner/tests/shell.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `Section.svelte`: per-branch rendering for a mixed tenant.
- [x] Tests: mixed tenant per-branch lock, all-paid no lock, upgrade unlocks on reopen, Free pairing and Support usable.
- [x] Deferred: the cloud `FEATURE_NOT_ENTITLED` gate (AC 3) has no owner read or command endpoint to guard yet; it lands with each endpoint (8.13 and later).

**Acceptance Criteria:**
- Given the Story 8.12 UI criteria in `epics.md`, when the Owner app runs against a stubbed `/v1`, then each is met and the suite passes.

## Verification

**Commands:**
- `pnpm --filter @arena/owner check`; `ARENA_PW_PORT=5495 pnpm --filter @arena/owner test`; every `pnpm run check:*`; `pnpm run test:scripts`; gallery `test:a11y`; builds excluding marketing.

## Review Triage Log

Pass 1 (Blind Hunter and Verification Gap; on Sonnet). P = patch, D = defer, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | VG: AC 3 (`FEATURE_NOT_ENTITLED` in the cloud) not built or tested | high | D | no owner read or command endpoint exists yet; each endpoint adds the gate with its story |
| 2 | BH/VG: paid branches render the same `children` with no branch context | medium | D | the section content is a placeholder; 8.13 and later pass the branch in |
| 3 | BH/VG: empty branch list renders unlocked content | low | D | pre-existing from 8.10; a tenant always has a branch once it can sign in |
| 4 | VG: upgrade tested by a stub swap and reload; stale offline cache not covered | medium | R | online reads always replace the cache; offline shows cached tiers by design (8.10) |
| 5 | BH: test filters by substring name, loose `@` link, `unroute` glob, no explicit signed-in assert | low | R | fixture names are distinct; the unroute glob is the one `stubCloud` registers |
| 6 | BH: `list-style: none` drops list role in Safari; `type-hub-label` on an Owner heading | low | R | headings carry the branch name; the owner a11y suite (axe, all themes) passes on the mixed fixture |
