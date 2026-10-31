# Billing, tier changes and Hub move proofs (Story 10.20)

The per-release proof that billing never stalls a café and that no plan change or Hub move breaks a
running branch. It is the `billing-proof` job of `.github/workflows/release-bench.yml` (manual run and
release tags). Each test writes its figures to `$ARENA_PROOF_OUT/<name>.json` when the variable is
set (a file exists only if its test reached the end); `scripts/billing-report.mjs` merges them and
fails the release when a proof failed **or a result is missing**.

## What is proven

| Proof | Where | Result file |
| ----- | ----- | ----------- |
| Every band (IN, MY, SG, US) walks create → activate → upgrade with proration → cycle switch → renewal needs approval → grace → recovery, then grace → lapse, with states, entitlements, licences (read from the heartbeat) and invoice totals asserted at each step, and a duplicate and an out-of-order webhook at each stage | `crates/arena-cloud/tests/billing_events.rs` `each_band_walks_create_activate_upgrade_cycle_approval_grace_recovery_and_lapse` | `billing-walk.json` |
| Every band: cancel → period end → Free with no grace | `each_band_cancels_to_the_period_end_and_then_drops_to_free` | `billing-cancel.json` |
| Every band: 7-day refund → credit note equal to the invoice, plan Free, repeat event adds nothing | `each_band_refunds_within_seven_days_and_issues_a_credit_note_for_the_whole_invoice` | `billing-refund.json` |
| UPI is never offered at checkout for Premium yearly (asserted in the walk's cycle switch, every band) | same walk | `billing-walk.json` |
| A café of 24 PCs with 10 live sessions on the ten newest pairings and 2 open kitchen tickets goes Premium → Basic → Free: nothing running ends or changes, the four newest pairings are over cap only after their sessions end, open tickets still finish and no new one opens on Free, and the licence is evaluated on `trusted_now` so ads (Free tier on Android) are off a second before the paid licence's `exp` and on at `exp` | `crates/arena-core/src/apply/tests/plan_change_proof.rs` | `downgrade.json` |
| A Free Android Hub with 30 days of history, 20 kiosks and 5 live sessions moves to a Windows Hub over the LAN and again by file: every table of the branch is identical at the final `seq` (the same digest per table, and `state_checksum`), every session and segment is as it was, and all 20 kiosks connect to the new Hub with the token they already hold inside 30 s | `crates/arena-api/tests/handover.rs` `a_free_tablet_with_thirty_days_and_five_sessions_moves_to_a_pc_over_the_lan`, `the_same_cafe_moves_by_file_with_the_same_rows_sessions_and_tokens` | `move-lan.json`, `move-file.json` |

The cloud tests run on the Razorpay fake (`FakeRazorpay`) and a cloud clock the test moves
(`Harness::set_now`), so a year of renewals takes under a second. The 5-minute lifecycle sweep is
called directly (`lifecycle::sweep_once`), as the jobs role calls it.

### What the move proof stands in for

- The tablet and the PC are real: a TLS listener and the writer on each side, the puller, the
  importer, `stage_migration` and `apply_pending`. The cloud is a stub that answers the claim.
- The 20 kiosks are `arena-kiosk-core` clients pinned to the tablet. Following a successor is done
  by the test the way the kiosk service does it (`successor::verify`, then connect to the named
  address and fingerprint); the service's own runtime tests for it are `cfg(unix)`
  (`arena-kiosk-svc/tests/runtime.rs`).
- **By file no successor is pushed** (Story 10.19): the test points the kiosks at the new Hub by
  address, as a typed IP does, and checks the old token is accepted. The kiosk's certificate pin is
  re-trusted by the manager as in "Pair again"; no new device registration or token is needed.

## Not proven here

- **The nightly run on Razorpay test mode** (a Basic monthly UPI Autopay, a Premium yearly card and
  a Premium yearly eNACH subscription created, charged and refunded on staging) is not run: there is
  no staging environment or Razorpay test account in this repository. The report lists it as
  "not run" and `--strict` makes it required. A staging job writes `staging.json`:

  ```json
  {
    "subscriptions": [
      { "plan": "basic-monthly", "method": "upi", "charged": true, "refunded": true },
      { "plan": "premium-yearly", "method": "card", "charged": true, "refunded": true },
      { "plan": "premium-yearly", "method": "emandate", "charged": true, "refunded": true }
    ],
    "upi_offered_for_premium_yearly": false
  }
  ```

## Running it

```sh
rm -rf target/billing
ARENA_PROOF_OUT=$PWD/target/billing cargo test -p arena-cloud --test billing_events -- each_band
ARENA_PROOF_OUT=$PWD/target/billing cargo test -p arena-core --lib plan_change_proof
ARENA_PROOF_OUT=$PWD/target/billing cargo test -p arena-api --test handover -- a_free_tablet_with_thirty_days the_same_cafe
node scripts/billing-report.mjs --dir target/billing          # add --strict once staging runs
```

`BILLING_FOLLOW_MS` moves the 30 s kiosk budget for a trial; the report accepts a move only with at
least 20 kiosks, 5 sessions and 30 days of history.
