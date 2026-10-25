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
