# Validation — Epics 10–14 (step-04 §3, §5)

Format: story | check | problem | suggested fix
Checks: (a) forward / undeclared dependency · (b) tables/entities created before use · (c) size, Given/When/Then ACs, Covers line

## (a) Dependencies

- 10.2 | (a) | The "first successful payment → payment-country mismatch → `pending_review`" AC needs webhook/charge handling that only arrives in 10.3 | Move that AC into 10.3, or after 10.3, where `subscription.charged` is applied
- 11.6 | (a) | An MVP story's HDMI-CEC TV list shows a "Tested device" badge "verified by the Arena 360 test rig". That rig is Story 12.15, which is v1.1 | Say printers only at MVP, with CEC rows added when Epic 12 ships, or drop the rig wording
- 11.12 | (a) | Suspension "stops the referral link attributing new sign-ups", but attribution is built later in 11.14. The approval banner also links to KYC, which is 11.13 | Move the attribution-stop AC to 11.14 (it already covers suspended codes). Show the KYC banner without a link until 11.13
- 11.13 | (a) | "Payouts are held until it is verified again" depends on statements and holds from 11.17–11.18 | Move the hold behaviour to 11.18 (which already has the resubmit → `held_kyc` AC). In 11.13 assert only the KYC status change
- 11.15 | (a) | Portal navigation lists Payouts, which is built in 11.19 | Add the Payouts nav item in 11.19, or say the link is hidden until then
- 12.3 | (a) | `capabilities` includes a CEC probe result (libcec), but the CEC adapter and `TvControl` are defined in 12.5 | Move 12.5 before 12.3, or defer the CEC probe field to 12.5
- 12.4 | (a) | Linking "scans the CEC bus and offers devices with HDMI port", which needs the 12.5 CEC implementation | Reorder so 12.5 comes before 12.4, or keep only manual HDMI 1–4 entry in 12.4 and add the scan in 12.5
- 12.3 | (a) | "Revoked … by owner command" uses Epic 8, which is not in Epic 12's declared dependencies (1–3, 6, 7) | Add Epic 8 to Epic 12's Depends on, or drop "by owner command"
- 12.6 | (a) | "A standby does not advertise until promoted" uses Epic 9 standby, which is undeclared | Add Epic 9 to Epic 12's Depends on
- 12.14 | (a) | Uses Story 9.16 standby fingerprint and Story 9.19 epoch rules (Epic 9), which are undeclared | Add Epic 9 to Epic 12's Depends on
- 12.1 / 12.15 | (a) | Publishes to the "help-centre flashing steps" and "help-centre hardware list" (Epic 11, Story 11.6), which are undeclared | Add Epic 11 to Epic 12's Depends on
- 13.17 | (a) | The lease moves to `released` only on a kiosk `vault.signed_out` report, which is built in 13.18. Without 13.18, every lease quarantines after 5 minutes | Swap 13.17 and 13.18, or have 13.17 deliver its own minimal kiosk sign-out report and let 13.18 reuse it
- 13.11 / 13.16 / 13.29 | (a) | Use Epic 9 (9.1 erasure, 9.2 key backup, 9.4 export, 9.17 replication, 9.19 failover), which is not in Epic 13's declared dependencies (3, 5, 6, 8) | Add Epic 9 (and Epic 2) to Epic 13's Depends on
- 14.13 / 14.23 / 14.33 / 14.44 / 14.50 | (a) | Use Epic 10 (10.1, 10.4–10.6, 10.8) and Epic 12 (Pi health, Pi image), which are not in Epic 14's declared dependencies (5, 7–9, 11) | Add Epics 10 and 12 (and 2–4, 6) to Epic 14's Depends on
- 14.24 | (a) | "Their sessions are revoked on removal" and the member role changes assume members can sign in, which arrives in 14.25 | Move the revoke-on-removal AC to 14.25 or 14.26, or reorder so 14.25 comes before the member-management ACs

## (b) Tables and entities

- 13.28 vs 14.14–14.16 | (b) | Two different "rollup schema v2" definitions. 13.28 defines v2 = v1 + tournaments, deposits and loyalty, and the cloud accepts {2,1}. 14.14 redefines v2 = v1 + utilisation, staff and kitchen, and 14.16 again sets accepted {2,1}. 13.28 also raises the schema without the per-tier `rollup_schema` pin, which only appears in 14.16 | Make 14.14–14.16 schema v3 = 13.28's v2 + new blocks, with the cloud accepting {3,2}. Move the pin mechanism (`ops rollup-pin`) into 13.28, or into a shared earlier story
- 10.10 | (b) | Uses `subscriptions.grace_until`, but the 10.2 migration has no such column and no story adds it | Add `grace_until` to the 10.2 `subscriptions` columns, or add a migration AC in 10.10
- 10.3 | (b) | `billing_events` and `subscription_history` are used with columns but have no migration AC, unlike the other tables in Epic 10 | Add an explicit "Given gz_control / When the migration runs / Then it adds…" AC
- 11.29 | (b) | Support lists bundles with reference, size, sent at, sent-by role and case link, but 11.27 only uploads a zip to R2. No `diagnostics_bundles` record or table is defined | In 11.27, add a cloud record when the upload completes (a `diagnostics_bundles` table in gz_control with reference, branch, case_id, size, sent_at, role, r2_key)
- 11.30 | (b) | `launch_metrics` and `branches.first_settled_session_at` are created without a migration AC. Ad fill rate reads `ad_impressions`, which reaches the cloud only as heartbeat counts, so its gz_control storage is not confirmed | Add a migration AC. Name the cloud store for ad impression counts (from the Epic 4 heartbeat)
- 14.27 | (b) | Creates a Hub `refund_requests` table with the same name as Epic 10's cloud `refund_requests` (a different store and meaning). This invites confusion in shared types and docs | Rename the Hub table to `refund_approvals` (and the intents to `refund.approval_*`)
- 14.51 | (b) | Incidents are "posted from the admin app", but no incidents table, admin endpoint or Admin nav section exists (the 11.9 nav has no Status) | Add an AC in 14.51 for a `status_incidents` table, an audited admin endpoint and a Status section in the admin app

## (c) Size, ACs and Covers

- 11.9 | (c) | Too large for one dev agent: Access deploy, JWT verification, the `admin_staff` table, audit extension with a trigger, the CI route-audit check, nav shell, audit viewer with CSV, and staff management | Split into (1) Access + JWT + `admin_staff` + audit write and CI check, and (2) the admin shell, Audit viewer and Staff management
- 13.19 | (c) | A 3-day spike and the Steam adapter build sit in one story | Split the spike into its own story (as 14.46 does) and gate 13.19 on its result
- 14.13 | (c) | Too large: kiosk WMI health, Pi health, the Hub `device_health` table, Settings → Devices UI, the tile health line, the summary `devices` block and `device_outages` | Split into (1) health frames + `device_health` + Hub UI, and (2) `device_outages` + the summary `devices` block
- 10.4 | (c) | Large: wizard tier picker, add-ons, summary, Checkout, the SSE/poll wait, failure paths, Upgrade entry points from the Hub, Owner app and Add branch, and step 3 | Split out "Upgrade and Add branch entry points reuse the checkout" as a follow-on story
- 12.15 | (c) | Building a physical lab (buying Pis and TVs, HDMI capture, a self-hosted runner) can't be done by a dev agent alone, and the help-centre BOM AC is content work | Mark it as an ops/hardware task with a human owner. Keep only the CI job wiring as the dev story
- 14.50 | (c) | "Moving to Arena 360" says only Story 5.33 CSV imports exist and that the page promises nothing unshipped. That contradicts Epic 13's ggLeap and Smartlaunch importers (13.23–13.26), which ship earlier in v1.1 | Mention the 13.23–13.26 importers when Epic 13 has shipped, or make the section read from a feature flag

Covers lines: present on every story in Epics 10–14. ACs are in Given/When/Then throughout.
