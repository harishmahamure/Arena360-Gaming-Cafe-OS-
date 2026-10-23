---
review: currency
target: ../ARCHITECTURE-SPINE.md (+ .memlog.md version entries)
date: 2026-10-23
method: live registry queries (crates.io API, npm registry, GitHub releases API) + vendor docs/pricing pages + web search
verdict: CHANGES REQUIRED — 3 stale stack entries, 2 decisions that do not fit current vendor reality, 1 vendor-ownership event
---

# Currency Review — Arena 360 Architecture Spine

**Lens:** was every committed decision checked against today's (2026-10-23) web, registry, or vendor docs rather than asserted from memory?

**Verdict: CHANGES REQUIRED.** The Rust stack and Tauri are current. They are only unpinned, which the spine already acknowledges. But the web-tier entries in the Stack table and memlog `(version)` lines are stale: SvelteKit 3 is GA, not RC, and Astro 6 is a superseded major. The Turso open question can now be closed, but with different numbers than the memlog has, and the vendor was acquired on 2026-10-02. Two adopted decisions do not fit current vendor limits: AD-23 per-tenant edge rate limits, and AD-20 UPI Autopay for yearly/large plans. The interstitial-ad rule (Open Q7) is a probable Play policy violation as written.

## Findings (ordered by severity)

### F1 — HIGH — AD-23 per-tenant rate limits at Cloudflare are not available on any self-serve plan
- **Spine says:** "Cloudflare enforces … per-tenant limits on the Owner API and presigned-upload issuance."
- **Reality:** Counting by header, cookie, JSON body, or custom characteristics is only available with **Enterprise + Advanced Rate Limiting**. Free plans get 1 rule, IP only, with a 10 s period; Pro gets 2 rules; Business gets 5 rules, IP/NAT only. No plan counts by JWT claim.
- **Recommend:** Keep per-IP limits at the edge. Move per-tenant limits into `arena-cloud`, using the existing `rate_limits` table and keying on the authenticated `tenant_id`, consistent with AD-9. Amend AD-23. Record the Cloudflare plan tier in the Operational Envelope.
- Source: https://developers.cloudflare.com/waf/rate-limiting-rules/

### F2 — HIGH — Ads: "interstitial after every 2nd completed settle" conflicts with current Play disruptive-ads policy (Open Q7 should block, not trail)
- **Policy text, current:** The policy disallows "Apps where an interstitial ad is placed after a user action" and apps that "display interstitial ads repeatedly to distract users" from in-app tasks. It also disallows interstitials that "show unexpectedly." Ads must be closeable within 15 s; the spine's 5 s skip complies with that.
- **Why this placement is a problem:** Settle is the core POS task for staff, and the next customer is often waiting. An interstitial on every 2nd settle is a recurring post-action interruption.
- **Recommend:** Before epics, replace the trigger with a natural break such as shift close, Z-report, or idle return after N minutes. Also apply an AdMob frequency cap and do a policy pre-check. Keep the banner. Move Open Q7 into Inherited Constraints as a PRD change request.
- Source: https://support.google.com/googleplay/android-developer/answer/9857753

### F3 — HIGH — AD-20 Razorpay UPI Autopay cannot silently renew yearly Premium (or multi-branch monthly)
- **Current limits:** A UPI Autopay mandate can be up to ₹1,00,000, but **frictionless debits are capped at ₹15,000**; the ₹1 L cap applies only to BFSI. eMandate (eNACH) goes up to ₹1 crore. Cards are also supported.
- **Impact:** Premium yearly is ₹19,990 + 18% GST = **₹23,588**, which exceeds ₹15,000. A tenant with 7 or more Premium branches on one monthly subscription also exceeds it. Above the cap, each debit needs customer authentication, so "autopay mandatory" renewals would fail or stall. That breaks the AD-10 grace/lapse timing assumptions.
- **Also unverified:** The PRD's "other countries in local currency where Razorpay supports it". The Subscriptions docs list availability as India, Malaysia, Singapore, and US only.
- **Recommend:** Steer yearly plans and any plan above ₹15k to card or eMandate. Decide one subscription per branch versus per tenant with the ₹15k ceiling in mind. Add a "renewal needs approval" state before `grace`. Confirm international Subscriptions support with Razorpay before the PRD's "all countries at launch".
- Source: https://razorpay.com/docs/payments/subscriptions/supported-payment-methods.md ; https://razorpay.com/docs/payments/payment-gateway/s2s-integration/recurring-payments/upi.md

### F4 — HIGH — Turso: Open Q1 resolvable now, memlog numbers stale, and the vendor was acquired
- **Memlog says:** "free tier ~500 DBs; paid $29 vs $299". **Current pricing page:**

  | Plan | Price | DBs | Storage |
  | --- | --- | --- | --- |
  | Free | $0 | **100** | 5 GB, 10M writes |
  | Developer | $4.99 | **unlimited** | 9 GB |
  | Scaler | $24.92/mo annual | unlimited | 24 GB |
  | Pro | $416.58/mo annual | unlimited | — |

  There are no per-database limits. Idle DBs cost only storage.
- **Ownership:** **Supabase announced it is acquiring Turso on 2026-10-02.** Turso says the platform continues, but no SLA, plan, or close-date commitments were published.
- **Engine direction:** The team's new feature work goes to the Rust rewrite, "Turso Database" (`turso` crate 0.8.3, with sync in beta). libSQL is maintained (`libsql` crate **0.10.0 GA, 2026-10-13**). Turso docs now recommend **Turso Sync** over Embedded Replicas for new projects. The spine does not use embedded replicas, so this is fine for now.
- **Why it matters:** AD-24 makes the per-branch Turso mirror the **8-year cloud store of record for GST records**, so vendor continuity is now a compliance risk.
- **Recommend:**
  - Close Q1 at Developer, then move to Scaler as the fleet grows.
  - Pin `libsql = 0.10.0`.
  - Promote the Deferred "self-hosted libSQL" item to a documented exit plan, with "acquisition terms change" as a trigger.
  - Add an independent yearly GST archive, for example per-FY export to R2 with 8-year retention, so record-of-record does not rest on one acquired vendor.
- Sources: https://turso.tech/pricing ; https://pulse2.com/supabase-raises-150-million-and-acquires-turso/ ; https://www.beri.net/article/supabase-turso-acquisition-database-per-agent-sqlite-postgres-pricing-continuity ; https://docs.turso.tech/features/embedded-replicas/introduction ; https://crates.io/crates/libsql ; https://crates.io/crates/turso

### F5 — HIGH — Astro 6 is a superseded major; the Svelte integration no longer supports it
- **Registry:** `astro` latest is **7.4.2** (2026-10-21). Astro 7.0.0 shipped 2026-06-22. The 6.x line ended at **6.4.8 (2026-06-17)**.
- **Integration:** `@astrojs/svelte` latest is **9.1.1**, with peer `astro ^7.0.0`. AD-15 ("Astro rendering the shared Svelte components") therefore needs Astro 7.
- **Node:** Astro 7.4.2 `engines.node >=22.23.4`, not "Node 22+".
- **Recommend:** Stack entry becomes Astro **7.4.x** + `@astrojs/svelte` 9.1.x, with Node pinned as in F6.
- Sources: https://www.npmjs.com/package/astro ; https://www.npmjs.com/package/@astrojs/svelte

### F6 — MEDIUM — SvelteKit 3 is GA, not RC (memlog `(version)` line and Stack entry are stale)
- **Registry:** `@sveltejs/kit` 3.0.0 was released **2026-10-01**; latest is **3.1.0** (2026-10-14).
- **Peer and engine requirements:** Vite ^8, svelte ^5.57.1, TypeScript ^6, vite-plugin-svelte ^7, Node >=22.17.
- **Adapters:** `adapter-static` 4.0.0 and `adapter-cloudflare` 8.0.1.
- **Breaking changes:**
  - Configuration moves into the Vite config.
  - `$lib` becomes `#lib`.
  - Remote functions are not in 3.0.
- **Svelte:** 5.57 is current (5.57.2).
- **Recommend:** Pin SvelteKit **3.1.x** (drop "pin exact RC"), Svelte 5.57.x, Vite 8, TS 6, adapter-static 4, adapter-cloudflare 8. Use **one Node pin for all web builds: Node 22 LTS ≥ 22.23.4** (or 24 LTS), so that Kit 3 and Astro 7 both satisfy their engine requirements.
- Sources: https://www.npmjs.com/package/@sveltejs/kit ; https://www.netlify.com/changelog/2026-10-01-sveltekit-3/ ; https://svelte.dev/blog/sveltekit-3-release-candidate

### F7 — MEDIUM — Android Hub: Play submission requirements not captured
- **Target API:** Since 2026-08-31, new apps and updates **must target API 36**. Extensions run to 2026-11-01.
- **Native libraries:** Tauri's mobile plugin docs say **NDK 28+** is needed for 16 KB-aligned native libraries, which Play requires.
- **Ads SDK:** Google Mobile Ads SDK **25.5.0** (2026-09-17) raised minSdk to **24** and requires Kotlin ≥ 2.1. This is compatible with the spine's Android 9+ (API 28) floor.
- **Plugin model:** The Tauri Android plugin model (Kotlin `Plugin` + `@TauriPlugin`/`@Command`, invoked via `run_mobile_plugin`) still matches AD-16. Note that commands run on the main thread, so ad loads need coroutines.
- **Recommend:** Add `targetSdk 36`, `minSdk 28`, NDK ≥ 28, GMA SDK 25.5.x, and Kotlin ≥ 2.1 to the Stack table.
- Sources: https://developer.android.com/google/play/requirements/target-sdk ; https://v2.tauri.app/develop/plugins/develop-mobile/ ; https://developers.google.com/admob/android/rel-notes

### F8 — MEDIUM — Tauri: current, but a v3 line exists
- **Versions:** `tauri` **2.12.5** and CLI 2.12.4 (2026-10-21); `tauri-plugin-updater` 2.14.0.
- **Tauri 3:** **tauri 3.0.0-alpha.5** was published 2026-10-14. AD-32 relies on the updater and AD-16 on the plugin API, and both could change in v3.
- **Recommend:** Pin `tauri 2.12.x` / CLI 2.12.x / updater 2.14.x. Add a Deferred row "Tauri 3 migration — revisit at 3.0 GA".
- Sources: https://github.com/tauri-apps/tauri/releases ; https://crates.io/crates/tauri

### F9 — MEDIUM — Cloudflare Pages: supported, but Workers Static Assets is the current target
- **Status:** No official Pages deprecation was found. Cloudflare publishes a Pages→Workers migration guide and states that static asset requests on Workers are free. Third-party reports say new investment goes to Workers.
- **Adapter:** `adapter-cloudflare` 8 targets Workers.
- **Recommend:** Deploy Owner PWA, partner, admin, onboarding, and marketing as **Workers + Static Assets** instead of Pages. This avoids a later migration. Update AD-15 wording ("Cloudflare Pages" → "Cloudflare Workers static assets").
- Source: https://developers.cloudflare.com/workers/static-assets/migrate-from-pages/

### F10 — LOW — Other Cloudflare products: confirmed with notes
- **TURN:** Now under **Cloudflare Realtime**. It costs $0.05/GB egress, with **1,000 GB/month free shared with the SFU**. STUN is free.
- **Access:** The free plan suits fewer than 50 users. Paid is $7/user/mo. This is fine for the AD-21 staff SSO.
- **Tunnel:** Pricing and limits were not re-verified in this pass. Confirm before relying on it for the per-branch Tunnel add-on.
- Sources: https://developers.cloudflare.com/realtime/turn/faq/ ; https://developers.cloudflare.com/realtime/sfu/platform/pricing/ ; https://www.cloudflare.com/en-au/products/cloudflare-access

### F11 — LOW — Gupshup: vendor health risk
- **Health:** Fidelity marked Gupshup's implied valuation down from $1.4 B to $280–300 M (April 2026). The product line is still active.
- **Pricing:** Meta moved WhatsApp to per-message pricing. India utility messages cost about ₹0.115 and marketing messages about ₹0.86 (third-party sources). Gupshup's own rate card was not found.
- **Recommend:** AD-34's adapter plus manual mode already covers outage. Make the BSP swappable at config level, record the cost-per-message assumption against the "< ₹20 infra per paid branch" target, and confirm the Gupshup contract and rate card.
- Sources: https://en.wikipedia.org/wiki/Gupshup ; https://baat.ai/blog/whatsapp-api-pricing-india ; https://www.zoko.io/post/whatsapp-api-gupshup-pricing-features

### F12 — LOW — Web Push (VAPID) on iOS: fits, with constraints the spine should state
- **Requirements:** Standard Web Push works on iOS/iPadOS 16.4+ **only for PWAs added to the Home Screen**, with permission requested from a user gesture.
- **Newer option:** Declarative Web Push (Safari 18.4+/iOS 26) is backward compatible with standard Web Push and needs no change to the VAPID sender.
- **Recommend:** Add "iOS owners must install to Home Screen to receive alerts" to AD-27 and the onboarding UX. WhatsApp stays primary, as stated.
- Sources: https://webkit.org/blog/16535/meet-declarative-web-push/ ; https://developer.apple.com/videos/play/wwdc2025/235/

### F13 — LOW — Observability vendors: Open Q3 can be closed
- **Grafana Cloud Free:** 10k active series, 50 GB each of logs, traces, and profiles, 14-day retention, 3 users. These come from third-party sources reportedly checked against Grafana's page in Aug 2026; confirm on grafana.com/pricing.
- **Sentry Developer:** Free, **1 user**, 5k errors/mo. Team is about $26/mo.
- **Recommend:** Budget Sentry Team from the first paid tenants. A 1-seat plan does not support the on-call needs the spine defers.
- **Crates:** `opentelemetry`/`opentelemetry-otlp` 0.33.1 and `sentry` 0.49.3 are current.
- Sources: https://last9.io/blog/grafana-cloud-pricing/ ; https://costbench.com/software/developer-tools/sentry/free-plan/ ; https://crates.io/crates/opentelemetry ; https://crates.io/crates/sentry

### F14 — LOW — Rust crates: all exist and are maintained; pin these (from crates.io, 2026-10-23)

| Crate | Latest | Note |
| --- | --- | --- |
| tokio | 1.54.1 | |
| axum | 0.8.9 | |
| rusqlite | 0.40.2 | |
| libsql | 0.10.0 | |
| tokio-tungstenite | 0.30.0 | |
| zstd | 0.14.3 | |
| rmp-serde | 1.3.1 | |
| mdns-sd | 0.21.6 | |
| ed25519-dalek | **3.0.0** | new major: check `jsonwebtoken` EdDSA key interop |
| reqwest | 0.13.5 | |
| aws-sdk-s3 | 1.155.0 | |
| jsonwebtoken | 11.1.0 | |
| argon2 | 0.6.0 | |
| windows-service | 0.8.1 | |

- **aws-sdk-s3 against R2:** Recent SDK versions send default CRC checksums. Set `request_checksum_calculation = WhenRequired` and region `auto`, and verify presigned PUT in a spike. This could not be re-confirmed because the Cloudflare doc URL returned 404.
- **rusqlite + libsql together:** Verify that `arena-replay`'s rusqlite/libsql feature flags never link both bundled SQLite builds into one binary, since the two can clash on symbols. This is unverified and worth a build spike.
- Source: https://crates.io (API queried per crate)

### F15 — LOW — SOPS + age: current
- **Versions:** SOPS **v3.13.3** (2026-07-23); age **v1.3.2** (2026-08-29). Pin both.
- Sources: https://github.com/getsops/sops/releases ; https://github.com/FiloSottile/age/releases

## Required spine edits (summary)

1. **Stack table:**
   - SvelteKit 3.1.x (GA)
   - Astro 7.4.x + @astrojs/svelte 9.1.x
   - Node 22 LTS ≥ 22.23.4
   - Vite 8 / TS 6
   - Tauri 2.12.x (CLI 2.12.x, updater 2.14.x)
   - libsql 0.10.0 and the F14 crate pins
   - GMA SDK 25.5.x
   - Android targetSdk 36 / NDK ≥ 28
   - SOPS 3.13.x / age 1.3.x
   - Turso Developer → Scaler
2. **AD-23:** per-tenant limits move into the app; the edge does per-IP only.
3. **AD-20:** card or eMandate route for debits above ₹15k; a renewal-approval state.
4. **AD-24 / Deferred:** independent 8-year GST archive; Turso exit plan with an acquisition trigger.
5. **AD-15:** Cloudflare Workers static assets instead of Pages.
6. **AD-27:** iOS Home Screen install requirement.
7. **Open Questions:**
   - Close Q1 and Q3 with the figures above.
   - Escalate Q7 (ads) to a PRD change before epics.
   - Add Q: Razorpay international Subscriptions coverage.
8. **Memlog:** supersede the stale `(version)` lines: "SvelteKit 3 RC", "Astro 6.x", "Turso ~500 DBs / $29 vs $299".
