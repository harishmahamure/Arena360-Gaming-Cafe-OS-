---
title: 'Story 1.12: Branch locale settings — currency, number format and timezone'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_commit: 'b24506be362f4782b0ee0463a736b204252d485d'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-1-11-staff-sign-in-to-the-hub-with-roles-and-permissions.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The branch's currency, number format and timezone are fixed at bootstrap. Nothing formats money consistently, and nothing defines how a timezone or day-start change affects business days.

**Approach:**
- **Locale settings keys:** stored in `settings`, edited through `settings.set` from Settings → Branch, and pushed live to every open Hub view.
- **Shared formatter:** one money formatter in `@arena/ui`, driven by a currency table generated from `arena-proto`.
- **business_day/fy:** `apply()` stamps every change row, and tz/day-start changes take effect from the next business day.

## Boundaries & Constraints

**Always:**
- **Settings keys** (authoritative for locale; the `branch` row's columns are only the bootstrap values):
  - `branch.currency`: ISO 4217 code from the table.
  - `branch.symbol_position`: `before|after`.
  - `branch.number_format`: `indian|international`.
  - `branch.tz`: IANA, validated with chrono-tz.
  - `branch.day_start_min`: 0–1439.
  - `branch.fy_start_month`: 1–12.

  `branch.bootstrap` seeds them from its payload (`symbol_position=before`, `number_format=indian` when the currency is INR, otherwise `international`). Add them to the 1.8 `SETTING_RULES`. The permission is `settings.edit` (1.11).
- **Currency table:**
  - `arena-proto::currency::CURRENCIES`: `&[(code, exponent, symbol)]`, covering at least INR 2 ₹, USD 2 $, EUR 2 €, GBP 2 £, AED 2 د.إ, SGD 2 S$, JPY 0 ¥, KWD 3 KD and BHD 3 BD. It replaces the 1.7 const list, so validation uses it.
  - `pnpm gen:types` also writes `packages/ui/src/generated/currencies.json` from it, through the `export-ts` test path. `check:generated` covers it.
- **Deferred tz/day-start (migration):**
  - Add `prior_value TEXT NULL` and `effective_from TEXT NULL` (a business-day date) to `settings`, and `business_day TEXT NULL` and `fy TEXT NULL` to `changes`.
  - A `settings.set` on `branch.tz` or `branch.day_start_min` during business day D sets `prior_value` to the old value and `effective_from` to D+1, under the old rules. It only does so if no pending change is already in flight; if one is, it keeps the original `prior_value` and replaces the new value.
  - Other keys take effect immediately (`effective_from` NULL).
- **Business day logic** (`arena-core::locale`):
  - `effective_locale(settings_rows, business_day)` resolves prior versus current.
  - `business_day_of(ts_ms, tz, day_start_min) -> NaiveDate`: the local date of `ts - day_start`.
  - `fy_of(date, fy_start_month) -> "FY2026-27"`-style label (start year to the next year's two digits).
  - The stamp step sets `business_day` and `fy` on every change row: it computes the business day under the effective locale and then resolves.
- **Snapshot and events:**
  - `HubSnapshot` (1.10) gains `locale: {currency, exponent, symbol, symbol_position, number_format, tz, day_start_min, fy_start_month, pending: {tz?, day_start_min?, effective_from}?}`.
  - New loopback SSE endpoint `GET /ui/events?token=<session token>`. A bad token gets 401; a revoked or signed-out session closes the stream.
  - On each `Applied` that touches `settings`, it sends `event: settings.changed` with the new locale block and the changed keys. Other applies send `event: applied` with the intent_id only.
- **Formatter (`@arena/ui/format`):**
  - `formatMoney(minor: number | bigint, locale) -> string`:
    - scales by the exponent with exact integer arithmetic (no float maths beyond the final digits);
    - groups Indian-style (last 3 digits, then pairs) or internationally;
    - puts the symbol before or after (after means a space before the symbol);
    - writes negatives with a leading `−` (U+2212);
    - zero exponent means no decimals.
  - `formatDateTime(ts, tz)` uses `Intl` with the branch tz.
  - Pair it with the `.tabular-nums` class.
  - Unit tests run under `node --test` in `scripts/format.test.mjs`, or alongside the package if a test runner exists.
- **Hub UI, `/settings/branch`** (the rail Settings route gains a sub-nav, with Branch as the first entry):
  - **Fields:** currency (a select of the generated table, showing the code and symbol), symbol position and number format (radio groups), timezone (a select from `Intl.supportedValuesOf('timeZone')`, with a text fallback), day start (a time input as HH:MM converted to minutes) and FY start month.
  - **Preview:** a live sample amount (123456.78 in minor units) formatted with the current choices.
  - **Saving:** Save submits one `settings.set` per changed field, in sequence, each with its row's `base_rev`.
  - **Errors:**
    - `VALIDATION_FAILED` puts the catalogue field error inline.
    - `STALE` reloads the snapshot, shows the current value, and shows the catalogue "This changed elsewhere. Check the current value and try again."
  - **Pending changes:** show the catalogue "Takes effect from {date}" next to tz and day start, with the date formatted in the branch tz.
  - **Counter:** fields are read-only (`aria-readonly` / disabled inputs) and Save is a disabled `Button` with the catalogue reason "Only a manager can change branch settings."
  - **Live updates:** every open view, including other windows, subscribes to `/ui/events`. On `settings.changed` it updates the locale store, so formatted values and the settings form refresh without a reload, and a form with no unsaved edits refreshes its values.
- **Header clock:** uses the branch tz.

**Never:**
- Multi-currency per branch.
- Retroactively restamping existing rows.
- The floor or POS money UIs (later epics); the formatter is shared, and only the preview uses it now.
- Polling for settings.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Indian format | 12345678 minor, INR, before, indian | `₹1,23,456.78` | N/A |
| International | 12345678, USD, before, international | `$123,456.78` | N/A |
| Symbol after | 12345678, EUR, after, international | `123,456.78 €` | N/A |
| Zero exponent | 1234567, JPY | `¥1,234,567` | N/A |
| Three decimals | 1234567, KWD | `KD1,234.567` | N/A |
| Negative | -5000, INR | `−₹50.00` | N/A |
| Bad currency | save `XYZ` | inline field error; no change row | VALIDATION_FAILED currency |
| Bad tz | save `Mars/Base` | inline field error | VALIDATION_FAILED tz |
| tz change during D | tz Asia/Kolkata → Asia/Dubai at 14:00 on D | changes for the rest of D stamp D under Kolkata rules; from D+1 under Dubai rules; the UI shows "Takes effect from D+1" | N/A |
| Day start boundary | day_start 06:00, ts 05:59 local | business_day = the previous date | N/A |
| FY | FY start April, date 2027-03-31 / 2027-04-01 | `FY2026-27` / `FY2027-28` | N/A |
| Stale | window B saves with an old `base_rev` | STALE; the current value and the retry message are shown | N/A |
| Live | window A saves a currency | window B's preview updates via SSE with no reload | N/A |
| Counter | a Counter opens the page | read-only fields; the disabled Save shows its reason | N/A |

</frozen-after-approval>

## Code Map

- `crates/arena-core/src/apply/{settings_set.rs,stamp or changes}` and `settings_defaults.rs` / `SETTING_RULES` (1.8) -- extend the rules and add business_day/fy stamping.
- `crates/arena-core/src/apply/validate.rs` -- the 1.7 currency const list and the chrono-tz check. Switch to `arena-proto::currency`.
- `crates/arena-api` -- the `/ui/snapshot`, auth and session code (1.10, 1.11). Add SSE (axum `Sse`).
- `scripts/gen-types.mjs` -- export pipeline. Add currencies.json and keep the barrel format.
- `apps/hub/src/routes/settings/**` (the 1.11 demo control) -- grow into the sub-nav plus the Branch page; keep the receipt-footer control under a "Receipt" sub-entry or fold it in.
- `packages/ui/src/{index.js,package.json}` -- export `./format`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-proto/src/{currency.rs,snapshot.rs}` + `gen-types` output (currencies.json).
- [x] `crates/arena-store` -- the migration.
- [x] `crates/arena-core/src/{locale.rs,apply/*}` -- rules, deferral and stamping, with tests for every matrix row (the formatter rows are on the TS side).
- [x] `crates/arena-api/src/{ui_events.rs,...}` -- SSE plus snapshot locale, with tests.
- [x] `packages/ui/src/format.js` + tests.
- [x] `apps/hub/src/routes/settings/**`, the header clock, the locale store, and `apps/hub/tests/settings.spec.ts`.
- [x] Catalogue keys, `README.md`.

**Acceptance Criteria:**
- Given `cargo test --workspace --locked`, `pnpm check` and the hub Playwright suite, then all pass with every matrix row covered.

## Implementation Notes

- **Business day resolution.** `locale::resolve` works the day out under the prior rules first; once that day reaches a deferred change's `effective_from`, the new rules apply and the day is recomputed under them, never earlier than `effective_from`. So Kolkata→Dubai at 06:00 IST on D+1 (04:30 in Dubai) is already D+1, not D again. Setting a waiting tz/day start back to its `prior_value` cancels the wait (both columns NULL); a save equal to the value in effect writes no deferral.
- **FY label.** `FY{start}-{next two digits}` for every start month except January, which is a calendar year and is labelled `FY2027`.
- **Field errors.** `settings.set` keeps the 1.8 contract: a rejected value is `VALIDATION_FAILED field="value"`. The UI knows which field it submitted (one intent per field) and puts the catalogue error under that field.
- **Read-only for Counter.** `SessionStaff` (sign-in and `/ui/me`) gained `permissions` (the role's allow rows, catalogue order), and the Branch page is editable only with `settings.edit`; the Hub still checks every intent.
- **Snapshot.** `locale` is `null` before bootstrap; missing `branch.*` rows (a branch bootstrapped before this story) fall back to the `branch` columns. `ui_router` and `SnapshotReader::snapshot` now take the clock / `now_ms` to resolve the locale.
- **Events.** `/ui/events` lives in `ui_events.rs` behind the 1.11 auth router's origin guard. Besides `settings.changed` and `applied`, a stream that lags the writer's broadcast gets `event: resync` (the UI reloads the snapshot). The session is re-checked on every event, on every session end (sign-out, expiry, deactivation, sixth-session eviction via a watch channel) and every 30 s.
- **Currency table.** `CURRENCIES` keeps every 1.7 code (adds symbols/exponents for AUD, CAD, CHF, LKR, NPR, BDT, MYR, THB, SAR, QAR, OMR). `gen-types` now prettier-formats `.json` files too; regeneration is byte-stable.
- **UI routes.** `/settings` redirects to `/settings/branch`; the 1.11 receipt-footer control moved to `/settings/receipt` (auth.spec updated). The Settings layout owns the single `h1` and a second `<nav>` (section list), so shell.spec now asserts exactly one *Hub* nav and excludes Settings from the placeholder list.
- **Deps.** `chrono =0.4.45` (already in the lock via chrono-tz) is a direct `arena-core` dependency, with a README pin row.

## Spec Change Log

## Review Triage Log

| # | Finding | Verdict | Evidence | Route |
|---|---------|---------|----------|-------|
| 1 | Branches bootstrapped before 0006 have no `branch.*` rows, so Settings → Branch can never load or save (all three layers) | medium | The migration adds columns only. | patch (idempotent system seed intent at Hub start, also seeding missing built-in roles) |
| 2 | A deferred tz/day-start coming due is never pushed (blind, edge) | medium | No intent runs at the rollover. | patch |
| 3 | No resync after a stream reconnect (blind, edge) | medium | Events in the gap are lost. | patch |
| 4 | Every stream rebuilds the payload and re-reads staff | low | O(streams) work per Applied. | patch |
| 5a | `fy_start_month` applies mid-day, splitting one business day across two FY labels | medium | Test shows 2027-04-01 stamped twice. | patch (defer like tz) |
| 5b | Currency change with a different exponent re-means stored amounts | medium (unverified impact) | No money tables exist yet; the guard belongs with the first money story. | defer |
| 6 | Preview sample ignores the exponent | low | JPY shows ¥12,345,678. | patch |
| 7 | STALE on the Branch form discards the user's choice | low | Unlike Receipt. | patch |
| 8 | Partial saves not reported | low | Stops at the first refusal. | patch |
| 9 | Unsaved edits ignore the whole `settings.changed` event | low | Untouched fields stay stale. | patch |
| 10 | tz list comes from browser ICU, not the Hub's chrono-tz | medium | Aliases differ (`Asia/Calcutta`, UTC missing). | patch (generated list) |
| 11 | One `effective_from` for both pending keys (blind, edge) | low | Shows the max. | patch |
| 12 | Bad stored locale values fall back silently, stamping wrong days | low | No logging. | patch (warn) |
| 13 | Missing tests: lag resync, TTL close, pending note, reconnect, 401 redirect, live clock (blind, verify-gap) | low | Pre-verified gaps. | patch |
| 14 | `prior_value` kept after the deferral takes effect corrupts a later deferral | medium | The edge reviewer traced the off-by-one. | patch |
| 15 | A stream opened on a follower never delivers after promotion | low | No resubscribe. | patch |
| 16 | Edits made during a save are overwritten | low | Controls stay enabled. | patch |
| 17 | Same-staff re-sign-in leaves the stream on the dead token | low | Effect keyed on `staffId`. | patch |
| 18 | `arena_core::KNOWN_CURRENCIES` export removed | low | No external callers; pre-release. | reject |

## Verification

**Commands:**
- `cargo test --workspace --locked && cargo test --locked -p arena-hub --features dev-fixtures` -- expected: pass
- `pnpm gen:types && pnpm check` -- expected: pass after commit
- `pnpm --filter @arena/hub build && pnpm --filter @arena/hub exec playwright test` -- expected: pass
