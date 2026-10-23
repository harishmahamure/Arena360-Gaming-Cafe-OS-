# Re-validation — Epics 10–14 and coverage (2026-10-23)

Source: `epics.md` after reassembly (417 stories), new ids per `epic-fix/renumber-e10-14.md` (and the e1-5 / e6-9 maps for older ids). epics.md was not edited.

**Totals:** 62 prior findings checked. 61 RESOLVED (including 8 advisory items accepted with no change needed), 1 OPEN. 7 new issues.

## A. validation-e10-14.md (28 findings, all RESOLVED)

### (a) Dependencies

| Prior finding | Status | Evidence (new ids) |
| --- | --- | --- |
| 10.2 payment-country mismatch needs webhooks | RESOLVED | The mismatch → `pending_review` AC now lives in 10.3. 10.2 has no mismatch AC |
| 11.6 CEC "tested device" rig | RESOLVED | 11.6 lists printers and drawers only. A `category` field is ready for TV rows, and "no TV, CEC or smart-plug row … at launch" |
| 11.12 attribution stop and KYC link | RESOLVED | 11.13: the KYC banner is text without a link (11.14 adds the link), and "Story 11.15 stops a suspended code attributing" |
| 11.13 payouts held on KYC re-verify | RESOLVED | 11.14 asserts only the KYC status change. The hold lives in 11.19 (`held_kyc`, resubmit → `held_kyc`) |
| 11.15 Payouts nav before payouts | RESOLVED | 11.16 nav: "Payouts joins it in Story 11.20"; 11.20 extends the nav |
| 12.3 CEC probe before adapter | RESOLVED | The CEC adapter moved to 12.3. The probe is in 12.4 |
| 12.4 CEC bus scan before adapter | RESOLVED | The scan is in 12.5, after the 12.3 adapter |
| 12.3 owner-command revoke (Epic 8) | RESOLVED | Epic 12 now depends on Epics 1–3, 6–9 and 11 |
| 12.6 standby (Epic 9) | RESOLVED | Epic 9 is declared |
| 12.14 9.16/9.19 rules | RESOLVED | Epic 9 is declared |
| 12.1 / 12.15 help centre (Epic 11) | RESOLVED | Epic 11 is declared |
| 13.17 lease release needs the sign-out report | RESOLVED | Swapped: 13.17 is the helper and emits `vault.signed_out` / `signout_failed`. 13.18 (lease) consumes them |
| 13.11 / 13.16 / 13.29 use Epic 9 and Epic 2 | RESOLVED | Epic 13 now depends on Epics 1–3, 5–6 and 8–9 |
| 14.x use Epics 10 and 12 | RESOLVED | Epic 14 now depends on Epics 1–13 |
| 14.24 revoke-on-removal before member sign-in | RESOLVED | 14.26 marks a member removed and frees the seat. Session revocation of removed members is in 14.28, after sign-up in 14.27 |

### (b) Tables and entities

| Prior finding | Status | Evidence |
| --- | --- | --- |
| Two "rollup schema v2" definitions | RESOLVED | 13.29 = v2 plus the per-tier pin and `ops rollup-pin`, accepted {2,1}. 14.15–14.17 = v3 on top of 13.29's v2, accepted {3,2}, reusing the 13.29 pin. *See new issue N1.* |
| 10.10 `grace_until` missing | RESOLVED | 10.11 has a migration AC that adds `grace_until` |
| 10.3 `billing_events` / `subscription_history` migration | RESOLVED | 10.3 has a migration AC with columns |
| 11.29 no `diagnostics_bundles` record | RESOLVED | 11.28 adds `diagnostics_bundles` (columns, 30-day expiry) and writes it after upload. 11.30 reads it |
| 11.30 `launch_metrics` / ad fill storage | RESOLVED | 11.31 has a migration AC (`first_settled_session_at`, `launch_metrics`). Ad fill reads `gz_control.ad_impression_daily` / `ad_request_daily`, created in Story 4.14 |
| 14.27 Hub `refund_requests` name clash | RESOLVED | 14.29 uses `refund_approvals` and `refund.approval_*`, "distinct from Epic 10's cloud `refund_requests`" |
| 14.51 incidents with no store | RESOLVED | 14.53 adds `status_incidents` and `status_incident_updates`, audited `/v1/admin/status/*`, and an admin Status nav section |

### (c) Size, ACs and Covers

| Prior finding | Status | Evidence |
| --- | --- | --- |
| 11.9 too large | RESOLVED | Split into 11.9 (Access, JWT, `admin_staff`, audit, CI check; 7 ACs) and 11.10 (shell, Audit viewer, Staff; 4 ACs) |
| 13.19 spike plus adapter | RESOLVED | Spike is 13.19. 13.20 (Steam adapter) is gated on the spike's go result |
| 14.13 too large | RESOLVED | Split into 14.13 (health frames, `device_health`) and 14.14 (Hub UI, summary `devices` block, `device_outages`) |
| 10.4 large | RESOLVED | Upgrade links and Add branch moved to the new 10.5. 10.4 now has 7 ACs |
| 12.15 physical lab is not dev work | RESOLVED | A Notes block names a human hardware owner as a prerequisite. The ACs cover CI wiring and the data-file write |
| 14.50 compare page ignores the Epic 13 importers | RESOLVED | 14.52 "Moving to Arena 360" names the Epic 5 CSV imports and the Epic 13 ggLeap and Smartlaunch importers |

## B. validation-coverage.md (34 findings: 33 RESOLVED or accepted, 1 OPEN)

### Check 1: FR adequacy (24 items, covering the 19 previously partial FRs and 5 traceability-only items)

| FR | Status | Evidence |
| --- | --- | --- |
| FR11 (dashboard unlock) | RESOLVED | New 8.14: the Owner app stays in setup mode until `live_at`, then shows the dashboard. Covers FR11. *See new issue N3.* |
| FR11 (standby item) | RESOLVED | 2.31: the standby item is advisory on every tier, never blocks Go live, and is "filled in by Epic 9". 8.14 repeats this |
| FR3 (backup restore) | RESOLVED | 9.10 Covers "FR3 (sensitive: backup restore)". The 8.27 note reserves `backup.restore` alongside `hub.revoke` |
| FR28 (orders and tax) | RESOLVED | 5.2 "FR28 (tax on the session bill)" and 5.12 "FR28 (orders on the session bill)" |
| FR30 (reported as Guest) | RESOLVED | 5.30 Covers FR30 |
| FR7 (plans commit and timing) | RESOLVED | 5.34 has a plans commit AC (`import.plans` chunks of 500 with idempotency keys) and a 1,000-row plans < 30 s AC |
| **FR17 (exactly one group)** | **OPEN** | The FR text was amended: "Unassigned … cannot start sessions". No AC enforces it. 2.20 still says "at most one group". The 3.9 refusal list covers `PC_BUSY`, `PC_UNAVAILABLE` and `NO_RATE` ("in a group with no active rate"), so an ungrouped device is not explicitly refused. Fix: add "Unassigned" to the 3.9 refusal (for example `NO_GROUP` or `NO_RATE`) and align the 2.20 wording |
| FR71 (references on Z-report) | RESOLVED | 5.28 adds a "UPI and card references" section listing each reference. Covers FR71 |
| FR97 (order id on KOT) | RESOLVED | 7.5 prints the short order id, and the golden files assert it. 7.3 Covers order id |
| FR85 / FR91 (owner alerts) | RESOLVED | 8.34 Covers FR85 (owner alert) and FR91 (owner half) |
| FR114 (logo, templates) | RESOLVED (FR amended) | The FR now says "text-only header lines with no logo" and that the templates are Arena 360-approved and only toggled. 5.15 matches: on/off plus preview, no keys |
| FR118 (remote unlock from tile sheet) | RESOLVED | 3.5 has "Admin unlock (maintenance)" behind `inline-pin`, applies `device.admin_unlock`, 15 minutes, audited with mode = remote |
| FR137 (owner dashboards) | RESOLVED | New 14.21 "Reports → Games" plus 14.15 and 14.17 data path. Covers FR137. *See new issue N2.* |
| FR145 (smart plug) | RESOLVED (deferred) | FR145 and NFR18 amended (smart plug deferred, vendor TBD). 12.3 and 12.12 state the deferral, and 12.12 shows the plug option disabled |
| FR150 (credit export) | RESOLVED | 8.32 has an Export CSV/PDF AC for Credit (player id and username only) |
| FR141 (5-min message) | RESOLVED | 8.30 Covers "FR141 (optional 5-min WhatsApp/SMS with consent)" |
| FR159 (email) | RESOLVED (FR amended) | The FR now reads "there is no email channel". 14.24 matches |
| FR185 (skip after 5 s) | RESOLVED (FR amended) | The FR now uses Google-controlled close timing. 4.13 matches |
| FR186 (owner blocks more) | RESOLVED (FR amended) | The FR now specifies a per-branch rating G/PG/T. 4.12 ACs match. *See new issue N4 (stale Covers wording).* |
| FR202 (Owner HC) | RESOLVED | New 8.11 adds a High contrast theme to the Owner app, per device, overriding light/dark, with contrast and axe checks |
| FR189 (domain) | RESOLVED (FR amended) | The FR now names `partners.arena360.cloud`. No `arena360.in` remains in epics.md |
| FR178 (owner-login add-on) | RESOLVED (FR amended) | The add-on is tagged [v1.1] in the FR. Epic 10 Notes record the phase split. 14.25 sells it |
| FR175 (last summary in header) | RESOLVED | 9.16: the header tooltip and accessible name include the last summary time. Covers the full FR175 list |
| FR153 (upload-based offline) | RESOLVED | 8.35: heartbeat alive but no upload or summary for 30 minutes in opening hours raises a "branch not uploading" alert |

### Check 1b: NFR proof

| Item | Status | Evidence |
| --- | --- | --- |
| NFR12 external pen test | RESOLVED | New 11.33: an independent firm tests, critical and high findings are closed and retested, and a GA promotion gate checks the record, with retest after lockdown changes |

### Check 2: Starter

| Item | Status | Evidence |
| --- | --- | --- |
| 1.1 front-loads later app skeletons | RESOLVED (accepted advisory) | The finding was optional. No change is needed |

### Check 3: Epic structure

| Item | Status | Evidence |
| --- | --- | --- |
| Epic 1 upfront substrate | RESOLVED (accepted) | Accepted as greenfield substrate |
| settle-bar churn | RESOLVED (no action) | Judged incidental |
| tile-sheet churn | RESOLVED (no action) | Judged incidental |
| `apply()` churn | RESOLVED (no action) | Expected |
| Intra-epic placeholders (2, 7, 11) | RESOLVED (no action) | Explicit placeholders |
| "> Draft notes:" labels | RESOLVED | 0 occurrences remain. Intros use "> Notes:" |

### Check 4: Coverage map

| Item | Status | Evidence |
| --- | --- | --- |
| Map vs actual | RESOLVED | Re-run: every FR's mapped epics each contain at least one citing story. Epic List "FRs covered" matches the map for all 14 epics. FR1–FR202 and NFR1–27 are each cited at least once |
| FR3 / 28 / 30 / 85 / 91 / 141 traceability | RESOLVED | Covers lines added (9.10, 5.2, 5.12, 5.30, 8.34, 8.30). The map lists secondary epics (FR3 → 8, 9; FR28 / FR30 → 3, 5; and so on) |

## C. Fresh scan: forward dependencies and Depends-on lines (Epics 1–14)

Method: every "Epic N" and "Story N.M" reference in each epic's intro and stories was compared with that epic's Depends-on line.

- **Forward references (all epics):** All are explicit hand-offs or placeholders, and none blocks: for example "real setup-code onboarding is Epic 2", "feature flag off until Epic 10", "handed off to Epic 9", "Pis when Epic 12 ships", "the service Epic 11's admin app will call". Epics 13 and 14 have no forward references. No story in Epics 10–14 needs an unbuilt later story.
- **Intra-epic order (Epics 10–14):** No story uses a later story in its own epic. The previous breaks were fixed by the moves recorded in the renumber map (10.3 / 10.5, 11.13–11.20, 12.3–12.5, 13.17 / 13.18, 13.19 / 13.20, 14.13 / 14.14, 14.26–14.28).
- **Depends-on consistency:** Epics 1–4, 6, 8, 9 and 11–14 match the epics they actually reference. Epic 8 declares Epic 7 but does not reference it by number; that is harmless. Mismatches are listed as new issues N6 and N7. Epic 5's mention of Epic 4 is an out-of-scope pointer (Bluetooth printing), not a dependency.

## D. Fresh scan: table and entity creation (Epics 10–14)

Every table or column used in Epics 10–14 is either created by a migration AC, introduced inline with its columns at first write ("new table: …"), or built in an earlier epic. The exception is new issue N5. Checked: `price_books`, `subscriptions`, `billing_events`, `subscription_history`, `grace_until`, `refund_requests`, `hub_migrations`, `admin_staff`, `admin_audit`, `partners`, `partner_kyc`, `partner_statements`, `support_cases`, `diagnostics_bundles`, `launch_metrics`, `ad_*_daily` (4.14), `console_requests`, `bookings`, `tournament_*`, `loyalty_txns`, `vault_accounts`, `vault_leases`, `game_requests`, `stock_counts`, `suppliers`, `purchase_orders`, `petty_cash_entries`, `attendance`, `prep_stations`, `device_health`, `device_outages`, `report_schedules`, `tenant_members`, `refund_approvals`, `signal_messages`, `status_incidents`.

## E. New issues

| # | Where | Problem | Suggested fix |
| --- | --- | --- | --- |
| N1 | 14.17 vs 13.29 | Deploying the {3,2} accepted set drops v1. 13.29 defaults every tier's pin to 1 and builds v2 "only in tests" until a pin is raised, and no AC requires every tier to be at pin 2 or higher first. Branches still pinned at 1 would report `ROLLUP_SCHEMA_OLD`, and their owner reports would show missing days | Add a 14.17 precondition: refuse the deploy, or keep {3,2,1}, while any tier pins 1. Or have 13.29 raise all tiers to 2 as part of its rollout |
| N2 | FR137 / 14.21 | FR137 is [MVP], but its "feed owner dashboards" clause is delivered only by v1.1 stories (14.15, 14.17, 14.21). The FR text is not tagged, unlike the FR178 phase split | Tag that FR137 clause [v1.1] (or move the Games view into Epic 8 on rollup v1 `play_hours`), and note the split in Epic 14 Notes |
| N3 | 2.31 | The Go-live AC says the done screen's "Open dashboard" opens "the Owner app dashboard with the branch shown as Live". The dashboard arrives in Epic 8 (8.13 / 8.14), and Epic 2 has only the Account → Branches route. There is no interim landing or hand-off | Say "opens the Owner app (Account → Branches until Epic 8; Story 8.14 then shows the dashboard)" |
| N4 | 4.12 Covers | The Covers line still reads "FR186 (… owner blocks more …)", but FR186 was amended to a content rating instead of blocking categories | Change it to "FR186 (excluded categories, per-branch content rating, consent view)" |
| N5 | 11.15 | Writes `tenants.referral_partner_id`, `referral_captured_at` and a `referral_attributions` history row with no migration AC and no column list. This is the same class of gap as the old 11.30 finding | Add a "Given gz_control / When the migration runs / Then it adds …" AC |
| N6 | Epic List: Epic 10 Depends on | Lists Epics 1–5 and 8–9, but 10.17 and 10.19 rely on the Epic 6 kiosk reconnecting and reconcile states. Epic 6 is a dependency only transitively, through Epic 8 | Change Depends on to "Epics 1–6, 8–9" |
| N7 | Epic List: Epic 7 Depends on | Lists Epics 5–6, but the intro and stories cite Epic 1–3 stories directly (for example 7.1 → 2.x, 7.4 → 3.x) and the Epic 4 Bluetooth plugin. Every other epic lists its direct dependencies explicitly | Change Depends on to "Epics 1–6" |
