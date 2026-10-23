# Final fixes: epics.md (2026-10-23)

Sources: revalidation-e1-9.md (17 new issues) and revalidation-e10-14-coverage.md (FR17 open, N1–N7; N3 is the same item as 2.31). All stories keep their numbers; no stories were added.

- 2.31: "Open dashboard" opens Account → Branches. 8.14 gets a new AC that re-targets it to Today.
- 8.14: the live-branch filter is now a general rule, a shared live-branch list and picker. Reports (8.15), Manage (8.23) and Alerts (8.33) consume it.
- 6.2: the standby/promoted/restored cache clause moved to 9.20. 6.2 keeps a testable "cache not in replication" AC plus a hand-off, and its AD-11 Covers wording was adjusted.
- 6.36: "(or a promoted standby takes over)" was removed and replaced by a hand-off note to Epic 9 (9.21).
- 8.20: the FENCED fetch now uses the Story 1.6 demotion (NOT_PRIMARY). Demotion on a higher fence is 9.3; the self-lock screen is 9.24.
- 3.28: Extend uses the group's active rate until 3.30 wires in the 3.29 resolver.
- 3.23: tests use fixture `ends_at` sessions until 3.24. 3.26 changes the action to grace.
- 2.18/2.20: the `devices_meta.group_id` foreign key is added by the 2.20 `device_groups` migration.
- 2.21: a migration adds `devices_meta.tv_number`, with a unique index over console rows.
- FR17: 2.20 now says "exactly one group or Unassigned; Unassigned cannot start". 3.9 adds the `DEVICE_UNASSIGNED` refusal and message and Covers FR17. The coverage map and the Epic 3 FR list were updated to match.
- 4.10: `ad.requested{slot}` was added to the plugin event list.
- 5.13/5.23: a new `stock_holds` table (with columns) is created in 5.13 and released in 5.13/5.23.
- 6.10: "credit balance" was removed from `player.create`.
- 7.4: 7.4 creates the POS open-tickets strip.
- 7.6/7.7: `kitchen_displays.high_contrast` was added. The 1.21 HC control now covers kitchen displays (7.6), and 7.7 cites it.
- 8.2: a migration adds nullable `lease.fork_parent_epoch` / `fork_parent_seq`.
- 9.9/9.10: a new gz_control `key_requests` table (with columns). 9.10 cites it.
- 9.8: the Manager-level clause was replaced by a fixture non-owner session. Team logins are handed off to Epic 14.
- 9.4/9.7/9.18/9.20: 9.4 creates "Settings → System → Backup" and 9.18 creates "Settings → System → Standby". 9.7 and 9.20 use the same names.
- N1: 14.17 adds a deploy pre-check that refuses the {3,2} set while any tier pin is below 2. 13.29 has a note on the default pin.
- N2: FR137 is split into kiosk play hours and carousel [MVP] and owner dashboards [v1.1] (revised 23 Oct 2026, epics). The coverage map and the Epic 14 list were updated.
- N4: the 4.12 Covers line now reads "per-branch content rating".
- N5: 11.15 has a migration AC for `tenants.referral_partner_id`, `referral_captured_at` and `referral_attributions`.
- N6: Epic 10 now depends on Epics 1–6 and 8–9.
- N7: Epic 7 now depends on Epics 1–6.

Verification: 14 epics and 417 stories, numbered in sequence. Every story has Covers and Given/When/Then. There are no `{{placeholders}}` and no references to stories that don't exist. FR1–202 and UX-DR1–116 are all cited. The frontmatter is unchanged: status draft, steps [1, 2, 3].
