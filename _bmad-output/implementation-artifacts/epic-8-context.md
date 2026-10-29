# Epic 8 Context: Owner sees and controls branches remotely

## Goal

The café owner sees every branch and changes it from a phone or laptop, without being on the LAN. The Hub uploads change-log batches, a branch summary every cadence and a daily rollup; the cloud never holds a queryable copy of branch rows. The Owner PWA reads those files through the owner read API, sends commands through the Epic 2 mailbox (fan-out, step-up MFA, epoch fencing), gets alerts, and can open a Premium live view through a tunnel.

## Rules that shape every story

- Free uploads nothing and its Owner app shows only Subscription, pairing (Branches) and Support; Today, Reports, Alerts and Manage show a `tier-lock` "Available on Basic".
- Until Epic 10 the Subscription screen is read-only ("Plan changes are available soon") and no `tier-lock` shows an "Upgrade" link; until Epic 11 Support shows the support email and "Help centre — available soon".
- Cadence: `upload_interval_s` = `summary_interval_s`: Basic 900 s, Premium 300 s, floor 30 s.
- The Owner app shows only two command rejection reasons: "Device/Hub revoked", and "Couldn't apply at branch" with Retry.
- Paid tiers before Epic 10 come from the Story 2.8 operator-issued comp entitlement.

## Owner app (UX)

- Web surface: light theme "Cool paper" and dark Cinema Electric, System default following `prefers-color-scheme` live, `theme-toggle` in Account.
- `owner-nav`: Today, Reports, Alerts, Manage, Account; phone 64 px bottom tab bar (icon + `hub-micro`, `border-strong` top edge), desktop 256 px left sidebar (icon + `hub-label`, active filled). Gutter 40 (phone 16), content up to about 1200 px.
- Every screen caches its last payload with `as_of` in IndexedDB and shows the age; offline shows cached screens and a "You're offline" status badge.
- Web screen-reader baseline: `header`, `nav`, `main`, one `h1`, labelled controls, polite live regions.

## Story 8.10 neighbours

- 8.11 adds the high-contrast theme; 8.12 hardens the Free limits; 8.13 fills Today; 8.14 handles setup mode; 8.22 queues commands offline.

## Story 8.14: The Owner app stays in setup mode until a branch goes live, then opens its dashboard

- **Branch list.** `GET /v1/branches` now carries `live_at` (`live` stays) and `setup {steps_done, steps_total, next_step}`; done counts every step that is not pending (done or skipped) and `next_step` is the wizard's current step, so a branch that is not live shows at most 9 of 10.
- **Setup beats the tier lock.** A branch with `live_at` null gets a setup card on Today (Free included) and "Available after <branch> goes live" on branch-scoped screens; the Free limits apply only once it is live.
- **Shared list.** `liveBranches()` and `setupBranches()` in the shell are the single source; `showable` (Today) is live and paid. The branch picker is not built here: it would have no consumer until Story 8.15, which adds it on `liveBranches()`.
- **Polling.** Today re-reads the branches every 60 s while visible and while some branch is in setup, and on Refresh/pull; a branch that turns live is fetched at once. With no live paid branch the Refresh bar and Needs attention are hidden.
- **Open dashboard.** The onboarding done screen and the wizard URL of a live branch now open the Owner app root (Today).
- **Standby.** Unchanged: the Standby item is already advisory in the Go live view.

## Story 8.15: Reports — revenue and cash reconciliation

- **Shifts in the range.** `OwnerRange` gains `shifts` (`business_day` + the rollup's `RollupShift`, oldest closing first) from the accepted days; the rollup already carries the figures the Cash list needs, so the cloud only passes them through.
- **Over threshold.** `RollupShift` gains optional `over_threshold`, set by the Hub's rollup fold from the branch threshold at build time (`RollupRows.variance_threshold`). It is additive, so no schema bump: older files lack it and the app never flags them.
- **Per-branch calls.** The app asks `summary` and `live` for each live paid branch and sums per currency in the browser, rather than using `all`, so setup branches never enter totals or "partial" and the shared `liveBranches()` list stays the one source. Free live branches get a tier lock beside the picker.
- **So far today.** The branch's live `business_day` row ("So far today", net of refunds, with its data age) is shown in the by-day list when its rollup does not exist yet, and that day is not a missing day. It is not added to the rollup totals, which are gross and split by type and method.
- **Totals.** Labelled "Revenue, days with data"; "Partial" shows when any other day has no rollup. Refunds, discounts and voids are negative lines.
- **Dates.** Today/week/month use the date in the first picked branch's time zone; weeks start on Monday; Custom is capped at 400 days and refuses an inverted range before any call.
- **Badge colours.** `StatusBadge` takes its background from the dark Hub tile, which fails contrast on the paper theme, so Reports sets `--status-badge-background` to the surface. The same flaw exists on Today's badges (not changed here).
