---
title: 'Story 10.2: A branch subscription is created at a PPP band that is locked to it'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '0512df91f64d3fa6b94661b4c21e01cefad35bda'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-1-the-price-book-holds-tiers-add-ons-and-ppp-bands-synced-to-r.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A paid branch cannot be subscribed: nothing records which PPP band and price book an owner signed up at, so a later price-book publish could reprice them, and no Razorpay subscription is created.

**Approach:** A `subscriptions` table (one open row per branch, band and price book copied at creation), a subscription call on the billing adapter (fake, Razorpay, manual), `POST /v1/branches/:id/subscription` that quotes from the live book, creates the Razorpay subscription and returns checkout parameters, a quote function that always prices from the row's own book, and a jobs-role sweep that abandons unpaid `pending` rows after 24 h.

## Boundaries & Constraints

**Always:** amounts are minor units; the band comes only from `billing_country` (IN, MY, SG, US; anything else or a band not on sale is `BAND_NOT_AVAILABLE`); the live `price_book_id` is copied onto the row and every later quote for that row uses that book; GST (18%) is added on band IN only; a renewal debit above the band's limit (IN: 1,500,000 paise = Rs 15,000) allows card and eNACH only, otherwise UPI Autopay, card and eNACH (non-IN bands: card only); validation, ownership and existing-subscription checks all run before any provider call; the endpoint is gated by `billing_enabled` like the wizard tier.

**Never:** no webhooks, activation, entitlement writes or `first_paid_at` (Story 10.3); no checkout UI (10.4); no change-tier or proration (10.9); no owner-login purchase (quantity must be 0); no secret or provider body in an error.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Basic yearly, IN | owned branch, no open row | row `pending`, band IN, book `pb_v1`; checkout: amount 200000, tax 36000, total 236000, methods upi/card/emandate | none |
| Premium yearly, IN | same | total 2,358,820; methods card, emandate only; notice "Renewals over Rs 15,000 need a card or bank mandate" | none |
| Other country | `billing_country` = `GB`, or `MY` while MY is off | no row, provider not called | `BAND_NOT_AVAILABLE` |
| Open row exists | `pending`/`active`/`grace`/... for the branch | provider not called | `SUBSCRIPTION_EXISTS` |
| Other owner's branch | branch of another tenant | provider not called | `FORBIDDEN` |
| Free tier / bad add-ons | `tier` free; owner login > 0; tunnel > 1 | provider not called | `VALIDATION` |
| Provider fails | fake fails the call | row `abandoned`, branch free to retry | 503 |
| Book v2 published | v1 subscription, v2 higher | `quote_for(row)` still yields v1 amounts | none |
| Stale pending | `pending` older than 24 h, unpaid | cancelled at provider, `abandoned` | provider failure leaves it for the next run |

</intent-contract>

## Code Map

- `crates/arena-store/control-migrations/0035_subscriptions.sql` -- new; `subscriptions` and its indexes. Register in `crates/arena-store/src/control.rs` (migration list near line 249 and `LATEST_CONTROL_VERSION`; follow how 0034 is registered).
- `crates/arena-store/src/control/subscription.rs` -- new; `Subscription`, `SubscriptionState`, `MandateMethod`, `NewSubscription`; `reserve_subscription`, `set_razorpay_subscription`, `abandon_subscription`, `subscription_of_branch`, `stale_pending_subscriptions`. Model on `control/price_book.rs` (`Stmt`, `transact`, `.expect(1)`).
- `crates/arena-proto/src/subscription.rs` -- new; `AddOns`, `CreateSubscription`, `SubscriptionCheckout`, `MandateMethod`, `SubscriptionState`; export in `lib.rs`. Pattern: `price_book.rs` (ts-rs derive, `#[cfg_attr(feature = "export-ts", ts(export))]`, `#[ts(type = "number")]` on i64).
- `crates/arena-proto/src/error.rs` -- add `ErrorCode::SubscriptionExists` ("SUBSCRIPTION_EXISTS"); add to the name table, round-trip list, and status maps in `crates/arena-cloud/src/api/error.rs` (409, lines ~97 and ~372), `crates/arena-api/src/ui_auth.rs` (~1057) and `crates/arena-kiosk-core/src/queue.rs` (~64). Regenerate `packages/ui/src/generated/ErrorCode.ts` and new types (see 10.1 commit for the generation command).
- `crates/arena-cloud/src/billing.rs` -- extend: `SubscriptionSpec`, `SubscriptionResult`, `CancelResult`; `create_subscription` and `cancel_subscription` on `FakeRazorpay` (idempotent per our subscription id, `fail_subscriptions`, `subscriptions()`, `cancelled()`, fake key id `rzp_test_fake`), `RazorpayClient` (a plan priced at the locked total, then the subscription on it; cancel at once) and `Manual` (`Deferred`); `BillingProvider::key_id()`.
- `crates/arena-cloud/src/subscription.rs` -- new; `validate`, `band_for_country`, `quote` (from a given book id), `quote_for(db, &Subscription)`, `renewal_limit_minor`, `allowed_methods`, `create` (reserve, provider, store id, checkout), `sweep_pending`; ops task kind `razorpay_cancel_subscription` for manual mode.
- `crates/arena-cloud/src/api/subscription.rs` -- new; `POST /branches/{id}/subscription` using `OwnedBranch` (`api/owned.rs`); merge in `api/mod.rs`. Errors via `ApiError` (`forbidden`, `validation`, `unavailable`) and `AppError::with_field`.
- `crates/arena-cloud/src/auth.rs` and `lib.rs` (`auth_state`, ~line 225-250) -- add `billing: OnceLock<BillingProvider>` set like `tunnels` (fake when `SmsProviderConfig::Fake`, else `provider_from_env(false, ..)`, warn and leave unset on error).
- `crates/arena-cloud/src/config.rs`, `backup.rs` (`run_job`, ~line 428) -- new `Task::SubscriptionSweep` ("subscription-sweep"), parse, help text, `code::SUBSCRIPTION_SWEEP_FAILED/_TIMEOUT`; job runner in `subscription.rs` modelled on `command_sweep.rs::run_job`. Update `crates/arena-cloud/tests/config.rs`.
- `deploy/README.md` -- one paragraph: schedule `--role jobs --task subscription-sweep` every 15 minutes, needs the billing keys in its environment.
- Read-only: `crates/arena-cloud/src/price_book.rs` (`pricing`, `new_id`), `api/wizard_tier.rs` (billing gate and `billing_disabled`), tests `tests/tier.rs` (owner/branch harness to copy) and `tests/price_book.rs`.

## Tasks & Acceptance

**Execution:**
- [x] `0035_subscriptions.sql` + control registration -- AC1
- [x] `arena-store` subscription module and `arena-proto` types, error code, TS regeneration -- AC1, AC5
- [x] `billing.rs` subscription and cancel on fake, Razorpay, manual (+ unit tests) -- AC2, AC5
- [x] `subscription.rs` (quote, methods, create, `quote_for`, sweep) and `POST /v1/branches/:id/subscription` -- AC2, AC3, AC4, AC5
- [x] `Task::SubscriptionSweep` job, config, tests, deploy note -- AC6
- [x] Tests: `crates/arena-cloud/tests/subscription.rs` (all ACs and the matrix), unit tests in `subscription.rs`, `billing.rs`, `arena-proto`; `tests/config.rs` task parse; migration check in `arena-store` control tests

**Acceptance Criteria:**
- Given `gz_control`, when the migration runs, then `subscriptions` exists with the story's columns and a unique index lets a branch hold only one row outside `abandoned`, `cancelled` and `lapsed`.
- Given an owner of an empty branch, when they POST `{tier, cycle, add_ons, billing_country}`, then the band comes from the country, the live book id is copied, a Razorpay subscription is created through the adapter, the row is `pending` with its id, and the response carries key id, subscription id, allowed methods, amount, tax and total.
- Given a renewal debit above Rs 15,000 incl. GST, then only card and eNACH are offered with the "Renewals over Rs 15,000 need a card or bank mandate" notice; Premium yearly IN totals Rs 23,588.20 (no UPI) and Basic yearly allows UPI.
- Given a subscription on v1 and a later published v2, when its quote is computed, then v1 amounts are used.
- Given an existing open row, a branch the owner lacks, tier free or invalid add-ons, then `SUBSCRIPTION_EXISTS`, `FORBIDDEN`, `VALIDATION` and the provider is never called; a country with no band or a band off sale answers `BAND_NOT_AVAILABLE`.
- Given a `pending` unpaid subscription older than 24 h, when the jobs sweep runs, then it is cancelled at the provider and becomes `abandoned`, and the branch can subscribe again.

## Spec Change Log

## Review Triage Log

## Design Notes

Razorpay bills one plan per subscription and its add-ons are one-time, so the real client creates a plan priced at the locked total (tier + add-ons, GST included for IN) and subscribes to it; the fake records the spec. The branch is reserved first (insert `pending` with no Razorpay id), then the provider is called, so two racing requests cannot both reach Razorpay; a crash in between leaves an unpaid `pending` row the sweep clears. `lapsed` counts as ended so a lapsed owner can subscribe again. Non-IN renewal limits are approximate conversions of Rs 15,000 (MYR 85,000, SGD 24,000, USD 18,000 minor) and only matter once those bands are enabled.

## Verification

**Commands:**
- `cargo test -p arena-cloud --test subscription --test config`, `cargo test -p arena-cloud --lib`, `cargo test -p arena-proto`, `cargo test -p arena-store --features control --test control` -- expected: pass
- `cargo fmt --all --check`, `cargo clippy -p arena-proto -p arena-store -p arena-cloud -p arena-api -p arena-kiosk-core --all-targets -- -D warnings` -- expected: clean

## Auto Run Result

Status: review (implemented and verified; the 4-layer code review is the next pass)

Implemented: `subscriptions` table (migration 0035), store module, proto types and `SUBSCRIPTION_EXISTS`, billing adapter subscription/cancel (fake, Razorpay, manual), `subscription.rs` (quote, methods, create, `quote_for`, sweep), `POST /v1/branches/{id}/subscription`, the `subscription-sweep` jobs task, deploy README note, regenerated TS types.

Verification: `cargo test -p arena-cloud --test subscription --test config` (15 + 24 pass), `--lib` (136), `arena-proto` (363), `arena-store --features control --test control` (24); `cargo fmt --check` and clippy `-D warnings` clean. Known unrelated failure: `binary.rs::unreachable_at_start_retries_then_exits_1_without_logging_the_token` (Windows connect timeout, listed in spec 10.1).

Residual risks: a crash after Razorpay accepts the subscription but before its id is stored leaves an orphan at Razorpay; a cancel answered with HTTP 400 is retried each sweep; a sweep with provider failures still exits success (logs a warning); no HTTP-stub test for the new Razorpay client calls; validation errors use the existing `VALIDATION_FAILED` code; `tunnel` add-on is refused on Premium (included).

## Code Review Result

Reviewed 2026-10-31: no blocking findings. fmt, clippy `-D warnings` and the listed test suites pass. Non-blocking, left as noted in residual risks: a Razorpay cancel answered with HTTP 400 is retried each sweep (oldest rows could crowd the 1000-row batch), and an orphaned Razorpay subscription is possible after a crash before its id is stored.
