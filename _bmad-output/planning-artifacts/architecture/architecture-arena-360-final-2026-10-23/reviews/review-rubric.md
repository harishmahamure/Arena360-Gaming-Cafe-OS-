---
type: review
lens: rubric-walker (good-spine checklist)
spine: ../ARCHITECTURE-SPINE.md
date: 2026-10-23
---

# Rubric review: Arena 360 architecture spine

**Verdict:** Strong, mostly convergent initiative spine (paradigm, write path, sync, commands, entitlement and ops envelope are well fixed). It is not yet safe to hand to epics: six high-severity gaps remain where Hub, Kiosk, KDS, Owner PWA, cloud and Pi epics could still build incompatible things. No critical findings.

## Checklist walk

| Checklist item | Result | Notes |
| --- | --- | --- |
| Fixes the real divergence points for the level below, misses none | Partial | Write path, sync, commands, LAN envelope, entitlement, undo, outbox, signing and RBAC are covered. Gaps: non-INR money and tax, the Rust→TS contract, LAN clients without a device token (QR phone page, KDS browser), owner live-view auth, the cloud API contract, ingest ordering. See H1–H5, M3, M4. |
| Every AD Rule enforceable and actually prevents its divergence | Mostly | AD-24's erasure rule conflicts with AD-2 (append-only log) and with immutable R2 batches and snapshots (H6). AD-7 and AD-26 assume INR and GST only (H1). AD-25 requires a device token that the KDS and the player phone page don't have (H4). The other ADs are testable. |
| Nothing under Deferred could let units diverge | Partial | The mDNS service name, export file format and Android→Windows snapshot format are cross-unit contracts deferred to "LLD update" with no single owner (M7). Open Q5 (Free uploads?) is in effect a deferred blocker (H2). |
| Named tech verified-current | Partial | Svelte 5.57, SvelteKit 3 RC, Astro 6 and Tauri 2.x are verified in the memlog. Turso pricing, Grafana Cloud, Sentry, SOPS/age and Cloudflare limits are still unverified (Open Q3). SvelteKit 3 RC is bound across every surface (M8). |
| Ratifies, does not contradict, brownfield code | N/A | Greenfield; there is no code in the repo. |
| Covers driving spec (PRD/UX) capabilities | Partial | Multi-country launch with a currency setting and PPP bands (PRD L219, L403, L534, L542) is not carried (H1). Start by QR (PRD L315) and live view (PRD L375) have no architecture (H4, H5). |
| Parent spine not weakened | N/A | Initiative altitude; no parent spine. |
| Every dimension decided / deferred / open (incl. ops envelope) | Mostly | Environments, CI/CD, rollout, secrets, observability, DR and retention are decided. Missing: the VPS provider and region (data residency), ingest scaling and partitioning, mirror schema migration, and the support-access model (M4, M5, M10, M12). |
| Bloat | Some | The Inherited Constraints table carries product rules (ad cadence, credit override, pricing, add-on prices) and AD-19 carries payout percentages. These belong in the PRD (M9, L2). |

## Critical

None.

## High

**H1. Money and tax are INR/GST-only, but launch is multi-country.** AD-7 fixes "integer paise" and AD-26 fixes GST invoice series and GST rounding. The PRD sells to all countries at launch, with a per-tenant currency and timezone, PPP bands, and Razorpay in local currency or USD (PRD L219, L403, L534, L542). The LLD already hedges with "paise (or minor units of branch currency)". Hub, mirror and Owner PWA epics will diverge on column naming (`*_paise`), currency scale (JPY has 0 decimals, KWD has 3) and on what a non-GST tenant's invoice is.
*Fix:* amend AD-7 to "integer minor units in the branch currency (ISO 4217 code on the branch row; exponent from the code)". Amend AD-26 so that the GST series and rounding apply when the tenant is GST-registered, and a generic tax-rate/receipt series applies otherwise. Alternatively, record an explicit decision that launch is India-only and push that back to the PRD.

**H2. Open Q5 (does Free upload batches?) blocks three ADs.** AD-24 makes the per-branch Turso mirror the 8-year GST store of record "regardless of tier". AD-27 sends every notification from the cloud after batch ingest. AD-9 creates one Turso DB per branch. If Free uploads nothing (PRD: manual export only), Free branches have no GST cloud record, no receipts or alerts, and no mirror. If Free does upload, the DB count and cost (Open Q1) change. The uploader, ingest, notification and Owner PWA epics cannot proceed consistently.
*Fix:* resolve Q5 now as an AD. For example: "Free uploads at a coarse interval (no mirror UI, no history retention beyond GST)", or "Free uploads nothing; Free has no cloud notifications and no GST cloud record, and AD-24/AD-27 are scoped to paid tiers". Then amend AD-24, AD-27 and AD-29 to match.

**H3. No mechanism carries `arena-proto` to the TypeScript surfaces.** The layer table says the UI depends on "`arena-proto` frames only", and AD-7 and AD-30 put error codes and the permission catalogue there. Nothing says how nine Svelte surfaces get those types, codes and permissions. Each epic will hand-write its own TS copies, and they will drift.
*Fix:* add an AD: "TS types, error-code keys and the permission catalogue are generated from `arena-proto` (e.g. ts-rs or specta) into the shared UI package; hand-written protocol types are forbidden; CI fails on a stale generation." Do the same for cloud API DTOs (see M3).

**H4. LAN clients without a device token are unaddressed.** AD-25 requires `hello{device token,…}` and AD-25/AD-14 bind the kitchen display. But the KDS is "a browser page on any tablet, no install" (PRD L174), and the player's phone opens an unauthenticated Hub page from the TV QR code (PRD L315). There is no rule for how a KDS browser is paired or authorised, what the anonymous phone page may call (only a `console.request` intent, rate-limited, scoped to one TV), or which Hub endpoints are reachable from the cafe Wi-Fi. LAN TLS is deferred, so device tokens travel in plaintext on the same Wi-Fi the players use. Hub, Pi and Kitchen epics will each invent their own answer, which is a security hole.
*Fix:* add an AD on LAN client classes. (a) Paired devices use a device token. (b) A browser KDS pairs via a code and gets a scoped token stored in the browser. (c) Anonymous phone pages get a separate, read-mostly endpoint set limited to `console.request` and the live timer, rate-limited per TV. State whether the staff/device API listens on the guest Wi-Fi at all. Move "Optional LAN TLS" from Deferred into this AD, or tie its revisit trigger to this AD.

**H5. Owner live view through the Tunnel has no auth or protocol rule.** AD-6 gates it by flag, and AD-25 lists "owner live view" as a LAN-envelope client. But the owner authenticates to the cloud (AD-10), not to the Hub. Nothing says how the Hub verifies the owner (a cloud-issued short-lived token the Hub verifies with its JWT keys? Cloudflare Access only?), whether the live view is read-only, or whether writes must still go through AD-5 commands. The Owner PWA and Hub epics will diverge, and a write side door around AD-5 is possible.
*Fix:* add an AD: "Live view = read-only AD-25 subscription through Tunnel + Access. The owner presents a cloud-signed, branch-scoped, short-lived token that the Hub verifies with its entitlement keys. All owner writes still go through AD-5 commands."

**H6. DPDP erasure conflicts with the append-only log and immutable history.** AD-24 says erasure is a "tombstone intent anonymising name/phone in the change log". AD-2 makes the log append-only. R2 batches and snapshots kept 1–3 years (and restore by replaying them, AD-11) still hold the plaintext PII, and a restore would bring it back. Hub, ingest and DR epics will each interpret "anonymise" differently.
*Fix:* amend AD-24 to choose one approach. Option one: PII lives only in a side table that is never put in change rows (change rows reference `player_id`), and erasure overwrites that side table and its mirror. Option two: crypto-shredding, where PII in batches is encrypted with a per-player key that erasure deletes. Either way, also state that restore and replay must re-apply tombstones, and that snapshots are re-cut or shredded.

## Medium (12)

- **M1.** The deployables diagram has "PWA reads Mirror" directly. This contradicts AD-9 (tenant from session via the cloud API). Route it as PWA → cloud Owner API → Mirror.
- **M2.** Where commands are stored is inconsistent. The diagram has the command role writing commands to `gz_control`, while the HLD keeps `commands` per branch in the mirror DB, which the ingester owns as derived data (AD-3). Pick one owner.
- **M3.** The cloud API contract for Owner PWA, partner portal, onboarding and admin is undecided: auth/session mechanism (cookie or bearer, refresh), API style, versioning and error envelope. AD-7/AD-8 cover only LAN frames and the Hub. Add a convention or AD.
- **M4.** Ingest ordering and scaling are silent. The HLD partitions ingesters by `hash(branch_id) mod N` so a branch's batches are never reordered. Add this to AD-4 as the rule that one branch is ingested serially.
- **M5.** The infra/provider envelope is incomplete. The VPS provider and region are not decided (HLD: "Hetzner or equivalent, EU or Mumbai"), and there is no data-residency stance for DPDP or a global launch. Decide them or add them to Deferred or Open Questions.
- **M6.** There is no Hub clock-trust rule. JWT `exp`, the grace period, the undo window, invoice dates and the business day all use the Hub wall clock, and rolling it back defeats entitlement expiry. Add: time is checked against the last cloud heartbeat time, and a backwards jump beyond a tolerance degrades to free-tier behaviour.
- **M7.** Deferred cross-unit contracts have no single owner. The mDNS service name and the export file format (shared by export, restore and the Android→Windows upgrade) are deferred to "LLD update". State that they are defined once in `arena-proto` and versioned.
- **M8.** Several technologies are still unverified (Turso plan and pricing, Grafana free tier, Sentry plan, SOPS/age, Cloudflare limits). SvelteKit 3 RC is bound for every surface, including production Pages apps. Either pin the 2.x stable line with an upgrade trigger, or record the RC risk acceptance.
- **M9.** Bloat: Inherited Constraints restates product rules (ad cadence and skip time, credit override frequency, add-on prices, the 10× yearly price, the undo window). Keep only rows that bind architecture (offline, no branch PSP, Tauri/Win10+, PWA push) and cite the PRD for the rest.
- **M10.** AD-8 covers Hub migrations, but there is no rule for migrating thousands of per-branch Turso mirror DBs: whether ingest migrates lazily on first batch, or a fleet job does it before a cloud deploy.
- **M11.** AD-15 shares tokens only, while the PRD says "one shared web UI" codebase. The shared component package and the repo/monorepo and package-manager layout are not seeded, so the Hub, Kiosk and PWA epics could fork components.
- **M12.** The support-access model is missing. The HLD says support uses the diagnostics bundle and the mirrored audit log, "never direct access to a branch". The AD-21 admin scope doesn't say whether staff may read tenant mirrors, or with what audit.

## Low (6)

- **L1.** AD-14 says a rejected replayed intent is dropped silently by the UI. AD-5 exposes a `rejected` state to owners. Scope "silently" to device queues.
- **L2.** AD-19 carries payout economics (10%, ₹500 rollover, TDS). Keep the ledger and manual-transfer rule and move the figures to the PRD.
- **L3.** AD-26 depends on the `<branch code>` being unique per tenant and immutable, but the spine doesn't state that.
- **L4.** The Pi TV idle and QR screen rendering stack is undecided (HLD: framebuffer or Wayland), while AD-15 says "all app surfaces Svelte". State whether the Pi screen is in scope of AD-15.
- **L5.** AD-10 "tampering ⇒ free-tier behaviour" is undefined for a Windows Hub, where Free is not allowed (AD-29). State what it means there, e.g. read-only or lock.
- **L6.** The "failover < 10 s" envelope target and manual-only promotion (AD-11) need a measurement definition, e.g. from the promote confirmation to the first accepted intent.
