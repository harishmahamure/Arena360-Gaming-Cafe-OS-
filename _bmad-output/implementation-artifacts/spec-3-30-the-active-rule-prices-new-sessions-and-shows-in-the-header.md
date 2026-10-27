---
title: 'Story 3.30: The Active Rule Prices New Sessions and Shows in the Header'
type: 'feature'
created: '2026-10-27'
status: 'done'
baseline_revision: 'f0ab3f7'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-29-dynamic-pricing-rules-by-weekday-hours-and-by-date.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-28-extend-a-session-from-the-hub-charged-to-wallet-pack-or-cred.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Pricing rules (3.29) are stored and resolved but nothing uses them: sessions and extensions still bill at the group's base rate, and staff cannot see which rule is running.

**Approach:** `session.start` and `session.extend` price through `pricing::resolve` (`pricing::priced` reprices the group's `RateRow` at `now`). A postpaid segment records the resolved rate and `rule_id`; a wallet start sizes `ends_at` and words its refusal on the resolved rate; an extension is priced at it. The Hub snapshot gains, per group, `active_rule` (name, adjustment, resolved rate, `until`) and `next_change_at`. The Hub header shows one `chip-rule` per group with a rule in effect; free tiles, the tile sheet's start and Extend areas use the resolved rate and show the chip; the layout re-reads the snapshot at the next boundary on the Hub's own clock.

## Boundaries & Constraints

**Always:**
- Only an active rule of the session's own group applies; no rule means the base rate and a null `rule_id`.
- `Resolved.until` is the first instant within 24 hours at which the winning rule changes (boundaries that do not change the winner are skipped); `next_change_at` is unchanged from 3.29.
- `SnapshotGroup.rate_minor` stays the base rate (Settings → Layout edits it); `active_rule.rate_minor` is the live one.
- `pricing_rules` writes push `groups.changed` so open views reload.
- No code comments beyond doc comments (user rule).

**Never:**
- Mid-session rate segments or re-pricing a running session (3.31), kiosk-side extend (6.21), plan-scoped rules.

</intent-contract>

## Code Map

- `crates/arena-core/src/pricing.rs`: `Resolved.until`, `winner_at`, `priced`; `apply/session.rs` (`SessionTarget.rule_id`, segment `rule_id`), `apply/extend.rs`, `snapshot.rs` (`active_rule`, `next_change_at`).
- `crates/arena-store/src/sessions.rs`: `insert_segment(.., rule_id)`, `SegmentRow.rule_id` (column exists since migration 0025).
- `crates/arena-proto/src/snapshot.rs`: `SnapshotActiveRule`, `SnapshotGroup.active_rule` / `next_change_at`; `arena-api/src/ui_events.rs`.
- Hub: `lib/rules.ts` (`liveRate`, `ruleChips`, `nextRuleChange`), `lib/RuleChips.svelte`, `routes/+layout.svelte`, `routes/floor/+page.svelte`, `lib/TileSheet.svelte`, `packages/ui/src/messages/en.json`, generated types, `tests/rule-chip.spec.ts`.
- Tests: `apply/tests/rule_pricing.rs`, `pricing/tests.rs` (`until`).

## Tasks & Acceptance

**Execution:**
- [x] Resolver `until` and `priced`, golden tests
- [x] Session start and extend priced through the resolver; segment records `rule_id`; wallet sizing/refusal on the resolved rate
- [x] Snapshot `active_rule` / `next_change_at`; `groups.changed` on rule writes
- [x] Header `chip-rule`, live rate on free tiles and sheet (start and Extend), boundary re-read from the local clock
- [x] Rust tests and Playwright spec on desktop and phone

**Acceptance Criteria:**
- Given a rule active for a group, then `chip-rule` shows its name, adjustment and end time, names each group when several are affected, and is absent when none apply.
- Given a rule starts, ends or is edited, then the chip, free-tile rates and sheet rates update (boundary timer on the Hub clock; `groups.changed` on edits).
- Given a session start or extend, then the rate comes from `pricing::resolve`, the sheet shows it with the chip, and the new segment records `rule_id`.
- Given a wallet start during a rule, then `ends_at` and the refusal text use the resolved rate.
- Given the WAN is down, then boundaries still change the chip and rates from the local clock (no network use).

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --exclude arena-e2e --exclude arena-kiosk-svc --all-targets -- -D warnings`, `cargo test --workspace --exclude arena-e2e --exclude arena-kiosk-svc --no-fail-fast`; `pnpm run gen:types`.
- `pnpm --filter @arena/hub check`, `pnpm --filter @arena/kiosk check`, `check:strings`, `check:tokens-used`, `check:contrast`, `check:protocol`, `check:layers`, `format:check`.
- `pnpm exec playwright test rule-chip rules extend settings tile-sheet floor` in `apps/hub`.

## Auto Run Result

Status: done
Verified: fmt, clippy, hub and kiosk check, string/token/contrast/protocol/layer/hex/secrets checks and prettier pass; arena-core (587 incl. 5 new `rule_pricing` apply tests, 3 new `until` golden tests, snapshot test) and the other workspace tests pass; `rule-chip`, `rules`, `extend`, `settings`, `tile-sheet`, `floor` Playwright specs pass on desktop and phone (132). Known unrelated failures as before: arena-cloud (`a_hung_run_times_out_with_its_own_code`, `unreachable_at_start_retries_then_exits_1_without_logging_the_token`) and arena-tasks (`trusted_time_is_cloud_time_plus_monotonic_elapsed`, `a_printer_that_stops_reading_times_out`). `arena-e2e` and `arena-kiosk-svc` tests not run.
Decisions: no migration (`session_segments.rule_id` already exists); the extension order line does not record a rule (no segment is opened); the snapshot keeps the base `rate_minor` and adds `active_rule` so Settings → Layout still edits the base; the chip names the group only when more than one group has a rule in effect; the layout re-reads the snapshot at `min(next_change_at)` (at least 1 s, 0.5 s after the boundary) rather than polling.
Left for later: mid-session rate segments and bill lines per rule (3.31); kiosk/attract screens showing the active rule (6.7).
