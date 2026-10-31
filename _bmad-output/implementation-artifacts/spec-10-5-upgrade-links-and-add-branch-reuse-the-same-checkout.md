---
title: 'Story 10.5: Upgrade links and Add branch reuse the same checkout'
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: 'ae412467f0feb1bee3bf643b0f3636e6a49e8cbe'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-4-wizard-step-2-takes-payment-through-razorpay-checkout.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Every `tier-lock` "Upgrade" link was hidden or read "Available soon", and the Story 10.4 picker and Checkout were reachable only from wizard step 2. An owner who meets a limit on a live Free branch had no way to go paid.

**Approach:** Billing is switched on (`billingEnabled = true`, `ARENA_BILLING_ENABLED=true` accepted). Each lock links to one `/upgrade` page of the sign-up app that runs the 10.4 picker and Checkout (`TierStep` in upgrade mode) for one branch, pre-set to the tier the lock named, and returns the owner to where they came from once the entitlement is written. A branch that already has a subscription goes to Account -> Subscription instead.

## Boundaries & Constraints

**Always:** the branch is owner-checked by the existing API (`FORBIDDEN`, no checkout); no second subscription (`SUBSCRIPTION_EXISTS` -> Account -> Subscription); Free is never offered on the upgrade page; the return link is followed only on the Owner app or the sign-up app's own origin; closing or failing Checkout changes nothing and shows the 10.4 text; a CI check fails if a `tier-lock` can render "Available soon".

**Never:** no licence delivery or plan display on the Hub (10.6); no invoices (10.7); no tier change or proration for a branch that already has a subscription (10.9); no change to the 10.3 webhook transitions or the 10.4 quote and create endpoints.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior |
|----------|--------------|---------------------------|
| Lock on a Free branch | `/upgrade?tier=premium&branch=b1&return=<owner page>` | picker with Premium preselected, Basic and Premium only |
| Paid | Checkout paid, webhook writes the entitlement | "Confirming your payment…", then back to `return` |
| Closed / declined | Checkout dismissed or failed | summary stays, nothing changed, "Go back without paying" leaves |
| Live subscription | `GET subscription` is active, grace, lapsed… | redirect to Owner app `/account/subscription?branch=` |
| Unpaid `pending` subscription | state `pending` | picker; Pay again reuses the same Checkout |
| Not the owner's branch | foreign branch id | `FORBIDDEN` text, no Checkout |
| No branch named | Hub link | the only Free branch, or a chooser of Free branches |
| Hostile `return` | other origin | Owner app home |
| Add branch | tenant with a paid branch | new branch opens the wizard at step 2 (existing), same picker and Checkout |

</intent-contract>

## Code Map

- `packages/ui/src/{billing.js,upgrade.js,components/TierLock.svelte,index.js,messages/en.json}` -- flag on; `provideUpgrade`/`upgradeUrl`; the lock always links; the `staff.tier.soon` key is gone.
- `apps/onboarding/src/routes/upgrade/+page.svelte`, `src/lib/{upgrade.ts,TierStep.svelte,api.ts,razorpay.ts}` -- the entry page, return-target guard, upgrade mode of the picker, `getSubscriptionStatus`/`listBranches`.
- `apps/owner/src/{routes/+layout.svelte,lib/*,routes/account/{branches,subscription}}` -- locks pass their branch; Account -> Subscription opens on `?branch=`.
- `apps/hub/src/routes/settings/branch/ads/+page.svelte` -- the Ads "Upgrade" link.
- `crates/arena-cloud/src/config.rs` -- `ARENA_BILLING_ENABLED=true` accepted.
- `scripts/check-tier-lock.mjs` (+ test), `package.json`, `.github/workflows/ci.yml` -- the CI check.
- Tests: `apps/onboarding/tests/{upgrade.spec,upgrade.unit.spec}.ts` and `stub.ts`, owner `shell|live|branches` specs, hub `staff|operating` specs, gallery `a11y`, cloud `tests/subscription.rs`.

## Tasks & Acceptance

- [x] Billing flag on, `TierLock` links to the picker, `check:tier-lock` in CI
- [x] `/upgrade` page and `TierStep` upgrade mode (preselect, no Free, poll for the entitlement, return, Back)
- [x] Existing subscription -> Account -> Subscription; foreign branch -> `FORBIDDEN`; branch chooser; safe return
- [x] Owner locks pass their branch; Account -> Subscription opens on `?branch=`
- [x] Cloud accepts `ARENA_BILLING_ENABLED=true`; test that a Free branch upgrades through the same checkout
- [x] Tests: onboarding upgrade specs, updated owner/hub/gallery/onboarding specs

## Verification

- `cargo fmt --all --check`, `cargo clippy -p arena-cloud --all-targets -- -D warnings`, `cargo test -p arena-cloud --lib --test subscription --test tier --test billing_events` -- pass
- `pnpm --filter @arena/{onboarding,owner,gallery} check` -- clean; `pnpm run check:tier-lock check:strings check:generated check:no-remote` -- ok
- Playwright: onboarding 485 passed; owner full suite passed after updating the six old "no Upgrade link" assertions; hub 2027 passed; gallery 46 passed

## Auto Run Result

Status: review.

Known unrelated failures (they fail on the untouched baseline too): onboarding `account.spec.ts` "copies and downloads" backup codes (clipboard line endings on Windows); hub `reports.spec.ts` Daily sales export, `ad-slot.spec.ts` settle-region, flaky `interstitial.spec.ts`; `pnpm --filter @arena/hub check` (`playwright.kitchen.config.ts` has no `process` type); `check:layers` and the deploy script tests; prettier on `scripts/restore-drill-report*`.

Residual risks: the Hub's Upgrade link points at a fixed `https://start.arena360.cloud` and returns to the Owner app home (the Hub is a LAN app with no cloud return address); signing in again after an expired session sends the owner to the sign-up app's home, not back to `/upgrade`; the 5 s plan poll has no server push for the upgrade page.
