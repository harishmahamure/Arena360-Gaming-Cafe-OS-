---
title: 'Story 5.2: Tax is computed once per rate on every bill, in minor units'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_commit: 'b736a8b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-1-tax-settings-gst-registration-rates-inclusive-or-exclusive-a.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Bills carry no per-rate tax. Settings (Story 5.1) say which rates apply, but nothing turns bill lines into a rounded, auditable breakup.

**Approach:**
- A pure function `arena_core::invoice::tax::compute(lines, inclusive, gst) -> Option<Bill>` and `TaxSettings::rates()` in `arena-proto`. No storage and no migration: `orders`, `order_lines`, `plans` and sessions do not exist yet, so the stored columns (`subtotal_minor`, `tax_minor`, `total_minor`, `tax_breakup`, `plans.tax_rate_bp`) are created by the stories that create those tables (3.x, 5.x), which call this function and write its result.

## Boundaries & Constraints

**Always:**
- Amounts are `i64` minor units of the branch currency; the function is exponent-agnostic, so 0 (JPY) and 3 (KWD) exponent currencies round at their own minor unit.
- Lines are summed per `rate_bp` first; tax is computed and rounded once per rate, half-up (half away from zero for negative amounts, so a refund mirrors its sale exactly), using `i128` intermediates.
- Inclusive: `tax = round(gross * rate / (10000 + rate))`, `taxable = gross - tax`, `subtotal = sum taxable`, `total = sum gross`. Exclusive: `tax = round(net * rate / 10000)`, `taxable = net`, `total = subtotal + tax`.
- GST mode (`gst = true`): each rate also carries `cgst = round_half_up(tax / 2)` and `sgst = tax - cgst`. Simple mode omits both (and they are absent from the serialized breakup).
- The breakup is ordered by ascending rate, includes zero-rate groups (nil-rated), and is empty for no lines. Overflow of `i64` returns `None`, never wraps.
- **Rate source (`TaxSettings::rates()` -> `(product_bp, play_bp)`):** `tax_mode = gst_in` uses `product_bp` and `play_bp`; every other branch, including an Indian branch without GST registration, uses `rate_bp` for both (default 0, so an unregistered Indian branch charges no tax). A plan line uses `plans.tax_rate_bp`, defaulting to the play rate (column added with plans).

**Never:**
- Per-line rounding, floats, a stored `tax_mode`, new tables, intents, error codes or migrations, or UI changes in this story.

## I/O & Edge-Case Matrix

| Scenario | Input | Expected |
|----------|-------|----------|
| Golden | 31000 inclusive 5% GST | tax 1476, CGST 738, SGST 738, subtotal 29524, total 31000 |
| Exclusive | 31000 at 5% | tax 1550, total 32550 |
| Mixed rates | 3 x 110 at 5%, 2 x 100 at 18% inclusive | tax 16 and 31 (per-line would give 15 and 30) |
| Exponents | 310 JPY, 310000 KWD fils at 5% inclusive | tax 15; 14762 |
| Half boundary | 5 at 10% exclusive | tax 1 |
| Odd tax | 11 total tax | CGST 6, SGST 5 |
| Refund | negated lines | every figure negated |
| Overflow | amounts beyond i64 | `None` |

</frozen-after-approval>

## Code Map

- `crates/arena-core/src/invoice/tax.rs` -- `Line`, `RateTax`, `Bill`, `compute`, unit, golden and proptest tests.
- `crates/arena-core/src/lib.rs` -- `pub mod invoice`.
- `crates/arena-proto/src/tax.rs` -- `TaxSettings::rates`.

## Tasks & Acceptance

- [x] Tests first: golden, boundary, exponent, overflow, properties (totals consistent, rounding within half a minor unit, order and line splitting invariant, negation symmetric, CGST + SGST = tax).
- [x] `compute` and `rates`.
- [x] Verification commands from the loop state.

**Acceptance Criteria:** the Given/When/Then of Story 5.2 in `epics.md` that this story can meet without the orders, plans and bill UI tables: rounding rules, CGST/SGST, golden cases, inclusive/exclusive totals, rates per line kind. Deferred to the stories that own those artifacts: the `orders` columns, `plans.tax_rate_bp` and its Settings → Plans editor, `bill-breakdown` and settle-bar lines, and immutability of stored totals (they are stored values, never recomputed).

## Spec Change Log

## Review Triage Log

Pass 1 (Blind Hunter, Edge Case Hunter with the spec as claims, Verification Gap, all on Sonnet; money). P = patch, R = reject, D = defer.

| # | Finding | Verdict | Route | Evidence |
|---|---------|---------|-------|----------|
| 1 | VG: negative CGST/SGST and per-rate breakup never compared in the negation test (mutation survived) | high | P | `negation_mirrors_every_figure`, unit test -11 gives -6/-5 |
| 2 | VG: proptest amounts never negative; CGST/SGST and rounding bound not sign-aware | high | P | signed generator, sign-aware assertions |
| 3 | VG: tie direction not pinned (bound accepts half-down); inclusive ties missing | medium | P | tie cases both signs, strict direction assertion |
| 4 | VG: overflow tests cover total only; no `i64::MIN` | medium | P | per-figure overflow and extremes test |
| 5 | VG: generator rates miss 2000, odd rates, above 10000; large amounts missing | medium | P | wider rates and amounts |
| 6 | VG: zero-rate group with others, mixed-sign netting, Indian unregistered `rates()` | low | P | tests added |
| 7 | BH/EC: `gst` flag passed separately from the rates can disagree | low | D | callers pass `settings.tax_mode() == GstIn` |
| 8 | EC: `None` when one rate group's net overflows though the grand total fits | low | R | safe side, unreachable with real amounts |
| 9 | EC: play rate not snapshotted at bill open; line kinds; labels such as "GST 5% (included)"; `Bill` not `Serialize` | medium | D | bill, orders and UI stories |
| 10 | BH: no `rate_bp` cap in `compute` | low | R | `TaxSettings::check` bounds rates |
| 11 | BH: first odd-tax case has even tax | low | R | second case covers it; negative case added |
