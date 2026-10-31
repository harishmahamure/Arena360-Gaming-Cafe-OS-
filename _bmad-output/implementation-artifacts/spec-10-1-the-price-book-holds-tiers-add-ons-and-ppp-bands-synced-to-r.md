---
title: 'Story 10.1: The price book holds tiers, add-ons and PPP bands, synced to Razorpay plans'
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: 'dcc6a5134429e90e6ba60ce5d7b2680fe223684c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-8-step-2-free-tier-issues-a-signed-licence.md'
deferred:
  - 'Razorpay plan creation is not idempotent on the Razorpay side: a publish that fails after some plans were created leaves those plans unreferenced and the retry creates new ones. Razorpay offers no lookup by notes; unreferenced plans are harmless.'
  - 'No HTTP write path for prices: the "API refuses PRICE_BOOK_IMMUTABLE" clause is met by the store, the ops commands and a database trigger. The admin UI is Epic 11.'
  - 'The wizard and Owner app do not yet call GET /v1/pricing (they read the generated snapshot); the wizard shows the band message only if the API answers BAND_NOT_AVAILABLE. Story 10.2 adds the band to the branch.'
  - 'The recorded fake covers plans only; subscriptions, payments, refunds and webhooks are added by Stories 10.2 to 10.15.'
  - 'Failures not from this story: arena-cloud binary test unreachable_at_start_retries_then_exits_1 (Windows connect timeout) and backup a_hung_run_times_out_with_its_own_code, check:layers edges arena-api -> arena-store and arena-tasks -> arena-replay, clippy in arena-kiosk-svc, prettier on scripts/restore-drill-report*.mjs, arena-e2e does not build on Windows.'
---

<intent-contract>

## Intent

**Problem:** Prices are hard-coded in the UI and nothing mirrors them to Razorpay, so the wizard, the Owner app and an invoice could disagree, and a price change would touch existing subscribers.

**Approach:** A versioned price book in `gz_control` (tiers, add-ons, four PPP bands, monthly and yearly), a billing-provider adapter with a recorded fake and a manual mode, an audited `arena-cloud ops price-book` path that publishes a draft by creating one Razorpay plan per row, `GET /v1/pricing`, and a generated snapshot the web apps build from.

## Boundaries & Constraints

**Always:** amounts are minor units; yearly is exactly 10x monthly; GST applies to band IN only; Free is never billed; a published book is immutable; publishing is all or nothing; every ops change writes an `ops_audit` row.

**Never:** no subscriptions, checkout or webhooks (Stories 10.2 onward); no owner-login purchase (data only).

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0034_price_books.sql` -- `price_books`, `price_book_items` (immutability triggers), `billing_bands`, `ops_tasks`, the seed book `pb_v1`.
- `crates/arena-store/src/control/price_book.rs` -- queries, draft, set amount, publish transaction.
- `crates/arena-proto/src/price_book.rs` -- `Band`, `PriceItem`, `BillingCycle`, `PriceLine`, `Pricing`; error codes `PRICE_BOOK_IMMUTABLE`, `BAND_NOT_AVAILABLE`.
- `crates/arena-cloud/src/billing.rs` -- `BillingProvider` (`Fake`, `Razorpay`, `Manual`), env selection.
- `crates/arena-cloud/src/price_book.rs`, `ops.rs`, `config.rs`, `main.rs` -- the service and `ops price-book list|draft|set|publish|band`.
- `crates/arena-cloud/src/api/pricing.rs` -- `GET /v1/pricing?band=`.
- `packages/ui/src/pricing.snapshot.ts` (generated), `pricing.ts`, `apps/onboarding` -- snapshot-backed pricing table and the band message.
- `deploy/README.md` 6d -- operator steps.

## Tasks & Acceptance

**Execution:**
- [x] Migration 0034 with the seed live book -- AC1
- [x] Store module, shared types, error codes -- AC1, AC3
- [x] Billing adapter: recorded fake, Razorpay client, manual mode with an ops task -- AC4, AC5
- [x] `ops price-book` publish/draft/set/band/list, audited -- AC2, AC3
- [x] `GET /v1/pricing` and `BAND_NOT_AVAILABLE` -- AC6
- [x] Generated snapshot, `pricing.ts` from it, CI-checked by a Rust test -- AC4 (snapshot)
- [x] Tests: `crates/arena-cloud/tests/price_book.rs`, `pricing_snapshot.rs`, unit tests in `billing.rs` and `arena-proto`, `apps/onboarding/tests/pricing.unit.spec.ts`

**Acceptance Criteria:**
- Given `gz_control`, when the migration runs, then the tables exist and the seed live book holds the published prices for IN, MY, SG and US, yearly 10x monthly, GST on IN only, Free absent.
- Given a draft, when `ops price-book publish` runs, then one plan per (band, item, cycle) is created, plan ids stored, the book is live and the previous retired; a provider error leaves the draft unpublished and nothing written.
- Given a published book, when anyone tries to change an amount, then it is refused with `PRICE_BOOK_IMMUTABLE`.
- Given the UI package, when the web apps build, then the pricing table comes from the generated snapshot and a test fails if the snapshot differs from the seed.
- Given `billing.provider = manual`, then every Razorpay call is a no-op that writes an ops task.
- Given a band that is off sale, when pricing is requested, then the API answers `BAND_NOT_AVAILABLE` and the wizard shows the unavailable message with Free still selectable.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo test -p arena-cloud --test price_book --test pricing_snapshot`, `cargo test -p arena-store --features control --test control`, `cargo test -p arena-cloud --lib billing`, `cargo test -p arena-proto price_book` -- expected: pass
- `cd apps/onboarding && npx playwright test tests/pricing.unit.spec.ts` -- expected: pass
- `cargo fmt --all --check`, `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-api -p arena-kiosk-core --all-targets -- -D warnings`
