---
title: 'Story 8.10: The Owner app shell — navigation, themes, install and Free limits'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'dd6d04b14adce64c76170f7d7eb75db6aba1ecea'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-6-owner-signs-in-on-a-new-device-with-totp-backup-code-or-sms.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `apps/owner` is the Story 1.2 placeholder. An owner has no navigation, no theme choice, no install and no behaviour on Free or offline.

**Approach:**
- A client-only SvelteKit PWA shell (`adapter-cloudflare`, `ssr = false`) with `owner-nav` (phone bottom tab bar, desktop sidebar), the five sections and the Account sub-pages.
- Theme from the shared `ThemeToggle` (System · Light · Dark), set before first paint.
- Web manifest, a service worker that caches the app shell, an IndexedDB cache of every loaded payload with its `as_of`, and an offline badge.
- On Free, the four data sections show a `tier-lock` with no Upgrade link.

## Boundaries & Constraints

**Always:**
- Reuse `@arena/ui` (`ThemeToggle`, `StatusBadge`, `Card`, tokens, `t()`). Add one shared `TierLock` component. All strings in `en.json`.
- **Navigation:** Today (`/`, home), Reports, Alerts, Manage, Account. Below 768 px: 64 px bottom bar, one column, 16 px gutter. From 768 px: 256 px sidebar, content up to 1200 px, 40 px gutter. Active item has `aria-current="page"`. Landmarks `header`, `nav`, `main`; one `h1` per route; targets at least 44 px.
- **Account:** links to MFA (the onboarding security screen, by `PUBLIC_ONBOARDING_URL`), Subscription, Branches, Support, and the Theme toggle; a Sign out button.
  - **Subscription:** read-only; each branch's tier and flags; "Plan changes are available soon"; no purchase control.
  - **Branches:** the branch list (Story 2.30 replaces it with pairing).
  - **Support:** the support email as `mailto:` and "Help centre — available soon"; no API call.
- **Data:** the shell reads `GET /v1/session` (401 refreshes once, then goes to sign-in) and `GET /v1/branches` (`[{id, name, tier, flags}]`, owned by Stories 2.12 and 2.30). Each successful read is stored in IndexedDB as `{payload, as_of}`; a failed read renders the cached payload with its age. Sign out and a refused session clear the cache.
- **Free:** every branch's tier is `free` means Today, Reports, Alerts and Manage render only `TierLock` ("Available on Basic"), no "Upgrade" link, and call no read API.
- **Offline:** a "You're offline" `StatusBadge` in a polite live region; cached screens show "Updated <age>"; never a blank page.
- **Install:** `manifest.webmanifest` (name, `display: standalone`, theme colours, SVG icon) and a service worker that precaches the build and static files and answers navigations from the cached shell when offline.

**Never:**
- Purchase controls, an Upgrade link, command sending, or any owner read API call from Free.
- Caching across a sign-out.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Phone | 360 px | bottom bar, Today current | N/A |
| Desktop | 1280 px | 256 px sidebar | N/A |
| Theme | OS dark/light flips | follows live on System; a chosen theme is kept | N/A |
| Free | all branches free | four sections show the tier-lock, Account works | N/A |
| Paid | any branch paid | sections show their placeholder | N/A |
| Offline, cached | API unreachable | cached branches, age, offline badge | N/A |
| Offline, no cache | API unreachable | empty-state line with Retry, offline badge | N/A |
| Signed out | 401 after refresh | cache cleared; go to sign-in | N/A |
| Sign out | button | `POST /v1/signout`, cache cleared, go to sign-in | cache cleared even if the call fails |

</frozen-after-approval>

## Code Map

`apps/owner` (layout, routes, `lib/api.ts`, `lib/cache.ts`, `lib/shell.svelte.ts`, `src/service-worker.ts`, `static/`), `apps/onboarding` (api client, theme script, Playwright config and a11y test as the pattern), `packages/ui` (`TierLock`, `en.json`).

## Tasks & Acceptance

**Execution:**
- [ ] `packages/ui`: `TierLock`, messages.
- [ ] `apps/owner`: shell, routes, cache, service worker, manifest, theme script, Playwright config.
- [ ] Tests: navigation at both widths, theme, Account pages, Free, offline and cache, sign out, install files, axe in light and dark.

**Acceptance Criteria:**
- Given the Story 8.10 criteria in `epics.md`, when the Owner app runs against a stubbed `/v1`, then each is met and the suite passes.

## Verification

**Commands:**
- `pnpm --filter @arena/owner check`; `ARENA_PW_PORT=5413 pnpm --filter @arena/owner test`; every `pnpm run check:*`; `pnpm run test:scripts`; gallery `test:a11y`; builds excluding marketing.

## Review Triage Log

Pass 1 (Verification Gap and Blind Hunter; on Sonnet). P = patch, D = defer, R = reject.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | VG: offline with no cache has no Retry | high | P | `Section` and the Account pages show the unavailable line with a Retry button; test added |
| 2 | VG/BH: two parallel session refreshes on a 401 | high | P | one shared in-flight rotation; a test counts exactly one refresh |
| 3 | VG: a network failure during refresh signed the owner out and wiped the cache | medium | P | sign-out only on a refresh answer below 500 |
| 4 | BH/VG: a late in-flight write re-caches after sign-out | high | P | an epoch counter drops writes started before `dropCache` |
| 5 | BH: navigations always served from the cached shell | high | P | navigations go network-first, cached shell as fallback |
| 6 | BH/VG: Free gating failed open when branches were unknown | medium | P | unknown branches show Retry, never the unlocked placeholder; the Free test asserts only session and branches are called |
| 7 | BH: unguarded `res.json()` left the page loading | medium | P | parse failure falls back to the cache |
| 8 | BH: the offline badge never turned on after load | medium | P | an `offline` listener |
| 9 | BH: `/reportsfoo` matched Reports | low | P | exact or `href/` prefix |
| 10 | VG: shown age used the branches payload only | low | P | the oldest `as_of` of the two payloads |
| 11 | VG: refused-session and sign-out tests could pass on a fresh DB or race the service worker | medium | P | the DB is never created by the check; sign-out with the API call failing is tested |
| 12 | BH: cache not keyed by owner | medium | D | needs the owner id from the session; sign-out already clears |
| 13 | BH: 403 treated as a cached read | low | D | role model arrives with the command stories |
| 14 | VG: PNG and maskable icons, service worker update path, `addAll` all-or-nothing | low | D | |
| 15 | VG: age does not tick; no 768 px test | low | D | |
| 16 | BH/VG: `.main a` forced inline-flex breaks prose links | low | R | `inline-flex` stays inline-level; links keep their flow |
| 17 | BH: `PUBLIC_ONBOARDING_URL` not validated | low | R | operator-controlled build variable |
