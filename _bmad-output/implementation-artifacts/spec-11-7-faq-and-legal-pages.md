---
title: 'Story 11.7: FAQ and Legal pages'
type: 'feature'
created: '2026-10-31'
status: 'review'
review_loop_iteration: 0
baseline_revision: '566b3332'
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
deferred:
  - 'The text of the four Legal pages is first-draft launch copy written from the product as specified (what the Hub, cloud, Kiosk sign-up and ads do). It has not had legal review; counsel should read each page against the shipped product before launch. When a page changes, add a line at the top of its `history` in the front matter and raise `version` and `effective` to match.'
  - 'Raising the Privacy policy version means raising `PLAYER_POLICY_VERSION` in `crates/arena-proto/src/player_signup.rs` in the same change: the site build fails when the two differ. Kiosk sign-up consent rows already record that constant.'
  - 'The FAQ has two answers per group at most, in the groups the story names. The Billing and Ads answers are new catalogue text (`site.faq.*`); the Offline, cancellation and refund answers are the Pricing FAQ keys.'
---

<intent-contract>

## Intent

**Problem:** `/faq` and the four Legal pages are linked from the nav and footer but do not exist, so an owner cannot read the refund rule, who owns the data, or the terms they are agreeing to.

**Approach:** A FAQ page of five disclosure groups driven by a catalogue-key map (so the Pricing FAQ's offline, cancellation and refund text is one key shared by both pages) with `FAQPage` JSON-LD; and a `legal` content collection of four Markdown pages with version, effective date and change history, whose Privacy version is checked at build against the `arena-proto` constant.

## Boundaries & Constraints

**Always:** the refund, cancellation and offline answers use the `site.pricing.faq.*` keys; the Data ownership answers say export of the full branch database and every report at any time on every tier, branch data on the café's own Hub, cloud data hosted in India; the Ad policy says ads only on the Free Android Hub at the counter, never on player screens, adult/gambling/alcohol excluded, removed on upgrade; each Legal page shows version, effective date and change history, and its newest history line is its version and date; the Privacy policy version equals `PLAYER_POLICY_VERSION` and the build fails when it does not; words in the catalogue (`site.faq.*`, `site.legal.*`); tokens only.

**Never:** a second copy of the refund text; a Legal page that asks for personal data; a hard-coded policy version in the page templates.

## I/O & Edge-Case Matrix

| Scenario | State | Expected Output / Behavior |
|----------|-------|---------------------------|
| Open FAQ | any | Billing, Data ownership, Offline, Ads, Refunds as closed disclosures, `FAQPage` JSON-LD for every question |
| Refund answer | FAQ and Pricing | the same string, one catalogue key |
| Open Legal | any | Terms, Privacy, Data processing, Ad policy with version and effective date |
| Open a policy | any | its text, version, effective date and change history (newest first) |
| Version drift | Privacy `version` differs from `PLAYER_POLICY_VERSION` | the build fails naming both |
| History drift | `version` or `effective` is not the newest history line | the build fails naming the page |
| 390 px / pseudo-locale | any | nothing scrolls sideways |

</intent-contract>

## Code Map

- `apps/marketing/src/lib/faq.ts` -- the five groups as catalogue keys, `faqJsonLd`. `src/pages/faq.astro`.
- `apps/marketing/src/lib/legal.ts` -- pure rules: `LEGAL_SLUGS`, `policyVersionFrom`, `assertLegal`. `src/lib/legal-content.ts` -- reads the collection and the Rust constant at build time and runs the check.
- `apps/marketing/src/content.config.ts` (`legal` collection), `src/content/legal/{terms,privacy,data-processing,ad-policy}.md`.
- `src/pages/legal/index.astro`, `src/pages/legal/[slug].astro`, `src/styles/legal.css`, `src/pages/sitemap.xml.ts` (lists the four policies).
- `packages/ui/src/messages/en.json` -- `site.faq.*`, `site.legal.*` (heading, version, effective, history).
- Tests: `tests/legal.spec.ts` (new), `a11y.spec.ts` and `pseudo.spec.ts` (now cover `/faq`, `/legal`, `/legal/privacy`), `scripts/marketing-site.test.mjs` (constant parsing, version and history checks, shipped pages, FAQ keys and JSON-LD).

## Tasks & Acceptance

**Execution:**
- [x] FAQ page grouped under Billing, Data ownership, Offline, Ads, Refunds as a disclosure list; refund answer is the Pricing FAQ key; `FAQPage` JSON-LD
- [x] Data ownership answers: export on every tier, branch data on the café's Hub, cloud hosted in India
- [x] Legal index and one page per policy, each with version, effective date and change history
- [x] Privacy version read against the `arena-proto` policy-version constant; build fails on a difference
- [x] Ad policy states the placement, exclusions and removal on upgrade
- [x] Pages in the sitemap; footer and nav links resolve; a11y and pseudo-locale coverage

**Acceptance Criteria:**
- Given FAQ opens, then five groups appear as a disclosure list and the refund answer equals the Pricing FAQ's, with `FAQPage` data for every question (legal.spec.ts, marketing-site.test.mjs).
- Given the Data ownership answers, then they state export on every tier, data on the café's Hub and cloud data in India (legal.spec.ts).
- Given Legal, then four policies are listed, each on its own page with version, effective date and change history (legal.spec.ts).
- Given the Privacy policy builds, then its version is checked against `PLAYER_POLICY_VERSION` and a difference fails the build (marketing-site.test.mjs, build).
- Given the Ad policy, then it states the placement, exclusions and removal on upgrade (legal.spec.ts).

## Spec Change Log

## Review Triage Log

## Verification

- `pnpm --filter @arena/marketing test` (165 passed, incl. axe in light, dark and hc at 390 and 1440 px for `/faq`, `/legal`, `/legal/privacy`, the pseudo-locale overflow check, and `tests/legal.spec.ts`; the known theme-toggle tab contrast flake (`#s1-s1-tab-system`) showed intermittently on one page per run and passed on rerun), `node --test scripts/marketing-site.test.mjs scripts/check-strings.test.mjs` (35 passed), `pnpm run check:strings`, `check:tokens-fresh`, `check:contrast`, `check:hex`, `check:no-blur`, `check:pins`, `check:secrets`, `check:no-remote`, `prettier --check` on the changed files, `tsc --noEmit` in `apps/marketing` (clean). `check:tokens-used` (`--ad-reserved` in the Hub floor page) and `prettier` on `scripts/restore-drill-report*.mjs` fail on the baseline commit too and are not touched here.
