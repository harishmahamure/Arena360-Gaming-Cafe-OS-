---
title: 'Story 3.29: Dynamic Pricing Rules by Weekday Hours and by Date'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'bc03c22'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-21-time-packs-blocks-and-passes-per-device-group.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-28-extend-a-session-from-the-hub-charged-to-wallet-pack-or-cred.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A café has one hourly rate per group (2.25). Owners cannot run happy hours or special-date prices, and there is no single place that says which price applies at an instant.

**Approach:** `rule.upsert` (Manager, `catalog.edit`) stores a pricing rule in a new change-log table `pricing_rules` (migration `0039_pricing_rules.sql`). A rule belongs to one device group and is either weekly (`days_mask`, bit 0 = Monday, and an hour range `start_min`..`end_min` that may wrap midnight) or by date (`date_from`..`date_to`, inclusive local `YYYY-MM-DD`; a single date is a range of one day). It either sets the rate (`override_minor`, per hour) or moves it by a percentage (`percent_bp`, negative = discount). The resolver `pricing::resolve(rules, group, base, tz, at)` returns the one rule that applies (never stacking), the resulting rate and `next_change_at`. Overlaps save, and `GET /ui/rules` lists every rule with its conflicts and who wins. Settings → Pricing rules is the Hub screen; a Counter's save opens `inline-pin`. Using the resolver to price sessions and the header chip are Stories 3.30 and 3.31.

## Boundaries & Constraints

**Always:**
- Field errors name the field and write nothing: `name`, `group_id` (a live group), `rule_kind` (`weekly` or `date`), `days_mask` (1..=127), `start_min`/`end_min` (0..1439, differ), `date_from`/`date_to` (canonical dates, last not before first, at most 366 days), `adjust_kind`, `adjust_value` (`override_minor` 1..=`RATE_MAX_MINOR`; `percent_bp` -10000..=100000, so -100% is allowed and below is not), `priority` (0..=1000), `id`.
- Fields that do not belong to a kind must be absent: a weekly rule has no dates; a date rule has no days or hours.
- Windows that wrap midnight belong to the day they started on (a Friday 22:00–02:00 rule covers Saturday 01:00).
- Exactly one rule applies at any instant: highest `priority`, then a date rule over a weekly one, then the latest edit (`updated_at`), then the rule id. Percentages round half-up in minor units.
- `next_change_at` is the earliest start or end of any active rule of the group in the next 24 hours, read on the branch wall clock.
- Conflicts are computed for active rules of the same group only, are symmetric, and say whether the other rule wins.
- Edits carry `base_rev` (`STALE`); a deactivated rule stays listed. `rule.upsert` joins the Story 1.8 proptest generator and `arena-replay`. No code comments beyond doc comments (user rule).

**Never:**
- Pricing sessions or extends with the resolver, the header chip, recording `rule_id` on segments (3.30), mid-session rate segments (3.31), plan-scoped rules, a delete intent, coupons.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Weekly | Mon–Fri 16:00–18:00, -20%, base 10000 | 8000 inside, base outside | No error |
| Wrap | Fri+Sat 22:00–02:00 | Sat 01:00 applies (started Fri), Fri 01:00 does not | No error |
| Date | 10–12 Mar, override 15000 | applies on those local dates | No error |
| Overlap | two active rules, same group, same time | both save, each lists the other and who wins | No error |
| Bad input | no days, start = end, -100.01%, reversed dates | nothing written | `VALIDATION_FAILED` with field |
| Edit | `base_rev` = rev | rev + 1 | `STALE` when behind |
| Counter | no `catalog.edit` | PIN asked in place, then applied with ticket | `PERMISSION_DENIED` without |

</intent-contract>

## Code Map

- `crates/arena-store`: `migrations/0039_pricing_rules.sql`, `pricing_rules.rs` (`PricingRuleRow`, queries), `branch.rs` (`Table::PricingRules`, `RowKey::PricingRules`), `lib.rs` (`Store::pricing_rules`).
- `crates/arena-proto`: `pricing.rs` (`RuleUpsert`, `RuleView`, `RuleConflict`, limits), `intent.rs` (`rule.upsert`, `CatalogEdit`).
- `crates/arena-core`: `pricing.rs` (`resolve`, `conflicts`, golden tests in `pricing/tests.rs`), `apply/rules.rs` (+ `mod`, `validate`, `guards`, `mutate`), `windows.rs` (`local_ms` shared), `snapshot.rs` (`SnapshotReader::rules`).
- `crates/arena-api/src/ui_auth.rs` — `GET /ui/rules`.
- Tests: `apply/tests/rules.rs`, proptest `Op::Rule`, `arena-replay` fixture, API test.
- Hub: `routes/settings/rules/+page.svelte`, `lib/rules.ts`, `routes/settings/+layout.svelte`, `lib/audit/labels.ts`, `packages/ui/src/messages/en.json`, generated types, `tests/rules.spec.ts`, `tests/settings.spec.ts`.

## Tasks & Acceptance

**Execution:**
- [x] Migration `0039`, change-log table registration and store queries
- [x] Protocol types, intent kind, generated types
- [x] `rule.upsert` apply logic and `GET /ui/rules` with conflicts
- [x] Resolver `pricing::resolve` with `next_change_at` and golden tests
- [x] Rust tests (matrix, permissions, API), proptest op, replay fixture
- [x] Settings → Pricing rules page, nav link, strings, inline PIN
- [x] Playwright spec on desktop and phone

**Acceptance Criteria:**
- Given Settings → Pricing rules, when a Manager creates a weekly rule, then name, group, weekdays, hour range (may wrap midnight) and an override rate or a percentage in basis points are stored in `pricing_rules`.
- Given a date rule, then it stores specific dates or an inclusive range in branch local time and outranks weekly rules of the same priority.
- Given a new or edited rule that overlaps another active rule of the group, then the save succeeds and the list shows "Conflicts with <rule>" with which rule wins.
- Given the resolver, then golden tests show exactly one rule at any instant, wrap-around windows applying to the day they started, and `next_change_at`.
- Given invalid input (no days, start = end, percentage below -100%), then a field error shows and nothing is written.
- Given a Counter, when they edit rules, then `inline-pin` opens for manager elevation.

## Spec Change Log

## Review Triage Log

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `pnpm run check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test rules plans settings` in `apps/hub`.

## Auto Run Result

Status: review
Verified: fmt, clippy, hub and kiosk check, string/token/contrast/protocol/layer checks and prettier pass; arena-core (18 resolver golden tests, 11 `rules` apply tests, proptest with `Op::Rule`), arena-api (`/ui/rules`), arena-replay and arena-store tests pass; `rules`, `plans` and `settings` Playwright specs pass on desktop and phone. Known unrelated failures as before: arena-cloud (`a_hung_run_times_out_with_its_own_code`, the `binary` test) and arena-tasks timing tests (`trusted_time_is_cloud_time_plus_monotonic_elapsed`, `a_printer_that_stops_reading_times_out`). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: scope is group only (the LLD allows `plan` scope; no story needs it yet); the permission is `catalog.edit` like plans; the field is `rule_kind` because a bare `kind` collides with the intent tag; conflicts are derived on read, not stored, and compared on active rules of one group; `other_wins` is true when the other rule outranks this one; a date rule has no hour range (whole days, per the LLD); -100% is allowed (free) while below it is refused; the resolver takes the rules and base rate as arguments so it stays pure, and `next_change_at` counts every boundary of the group's active rules, not only those that change the result.
Left for later: calling the resolver at session start and extend and the `chip-rule` header (3.30); rate segments and the `rule_id` on them (3.31); owner-side rule management (8.23); a delete intent.

## Code Review Result

Reviewed against the spec and ACs; no blocking findings in the story code. Fixed one pre-existing flaky test (`owner_staff_and_device_edits_win_over_a_stale_base_rev` picked an arbitrary role, sometimes the unassignable Kiosk-device role); it now uses the Counter role. fmt, clippy, arena-core/store/proto/replay/api tests, hub check, strings, prettier and the `rules`/`settings` Playwright specs pass.
