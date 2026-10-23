# HLD/LLD Extract (for architecture spine)

HLD = `docs/Arena360 — High-Level Design.md` · LLD = `docs/Arena360 — Low-Level Design.md` · PRD = `docs/Arena 360 - Product.md` · EXP/UPC = UX `EXPERIENCE.md` / `upstream-changes.md`.

## 1. System shape

- **Branch (LAN):** Hub primary (sole writer, SQLite WAL), optional standby replica, Kiosk per PC (Tauri app + LocalSystem service), Pi agent per TV, Hub UI and Kitchen display served by the Hub (HLD Component view; LLD Kiosk).
- **Cloud:** one Rust binary `arena-cloud` with control, ingest and command roles on one VPS. Stores: Turso `gz_control` plus one mirror DB per branch, and R2 for batches, snapshots and installers. Cloudflare edge provides Tunnel, Access, TURN and Pages (HLD Deployment; LLD Cloud internals).
- **Protocols:** LAN WebSocket JSON frames on `:7400`, HTTP UI on `:7401`. Hub to cloud: HTTPS bearer, outbound-only and idempotent. Hub to R2: presigned PUT. Primary to standby: WebSocket. Owner: HTTPS with MFA. Premium remote access: Tunnel + Access. Off-site standby: WebRTC (HLD Interfaces).
- **Sync:** an append-only change log, gap-free per epoch, stored as full-row msgpack and shipped as zstd batches. The upload sequence is presign, then PUT, then manifest (with gap reply). The ingester replays batches with the same `arena-replay` the standby uses (LLD Uploader, Ingester).
- **Owner writes:** commands are polled every 60 s or on a poke, applied via `command.apply`, deduplicated by `applied_commands`, and expire after 7 days (LLD Poller, Commands).
- **Mirror:** eventually consistent; every figure carries `as_of` (HLD Data rule 3).
- **Offline:** the Hub has no WAN dependency, and the uploader resumes from its cursor. The entitlement JWT is cached for 7 days. The Kiosk retries with 2–30 s backoff and shows "Hub slow"; the Pi shows "Hub unreachable" (HLD NFR; LLD Kiosk, Pi).

## 2. Candidate invariants already decided

- Local-first: truth lives in the Hub SQLite; the cloud is mirror + mailbox + licence (HLD Decisions).
- One writer per branch: no CRDT; primary/standby with a lease epoch (HLD Decisions; LLD Lease).
- One mutation path: intent, then `apply()` in one `BEGIN IMMEDIATE` (permission, epoch, validate, mutate, `changes`, commit, emit), serialised by a Writer actor (LLD Intent catalogue, Module map).
- Ownership: the Hub writes branch rows. The cloud writes control tables and derived mirrors. The owner app writes only commands (HLD Data architecture; LLD Owner app).
- Webhooks land in the cloud only; the branch learns of them through commands (HLD Integration principles).
- Data formats: ULIDs minted by the writer; `branch_id` on every row; money in integer paise; time in UTC ms (HLD Data rule 1; LLD Conventions).
- Versioning: `rev`/`updated_at` on rows; forward-only additive migrations; `SCHEMA_TOO_NEW`; frame `v`; `UNSUPPORTED` for unknown kinds; Kiosks accepted for N-2 releases (LLD Migrations).
- Errors: typed `AppError` with stable codes in `arena-proto` (LLD Conventions).
- Auth: owner password + TOTP; staff password + PIN; 256-bit device tokens (argon2id); single-use pairing codes; one permission per intent; EdDSA JWT valid 7 days (HLD Security; LLD Pairing, Entitlement).
- Tenancy: one Turso DB and one R2 prefix per branch; `tenant_id` comes from the session only (HLD Security; LLD Owner API).
- Network: the Hub is outbound-443 only (HLD Deployment).
- Audit is the change log (HLD Security).

## 3. Stack and versions

- **Rust:** Axum, Tokio, rusqlite, tokio-tungstenite, zstd, rmp-serde, mdns-sd, ed25519-dalek, reqwest, libsql, aws-sdk-s3, jsonwebtoken, argon2 (HLD Tech stack).
- **Kiosk:** Tauri 2 + Svelte on WebView2 Evergreen; Windows 10 1803+ (LLD Kiosk).
- **Hub UI and Kitchen display:** Svelte/TS, Tabler, Manrope.
- **Android Hub:** Kotlin + JNI + AdMob (HLD Tech stack; LLD Android Hub).
- **Pi:** Rust aarch64 on Raspberry Pi OS Lite with libcec (LLD Pi).
- **Owner app:** Flutter (riverpod, dio, drift), FCM/APNs (LLD Owner app).
- **Marketing:** Astro on Pages.
- **Infra:** Hetzner-class VPS (EU or Mumbai), Turso, R2, Cloudflare (HLD Deployment).
- **Services and tooling:** Razorpay, Gupshup, AdMob; OpenTelemetry, Sentry; GitHub Actions (HLD Integrations, Tech stack).
- **Pinned versions:** none anywhere.

## 4. Conflicts with current PRD/UX

1. **Old Kiosk stack remnants.** HLD/LLD are on Tauri/Win10, but WPF/.NET traces remain: "WPF without composition effects" (HLD NFR), "MSBuild" (HLD Tech stack), "C# generated" (LLD Crates), C# contract tests and Win 7 lockdown VMs (LLD Test plan), and Sentry "(Win 8+)" (LLD Tracing).
2. **UPI.** HLD specifies a dynamic UPI QR, a branch Razorpay key and webhook relay (HLD Integrations). The PRD uses a manual last-4 reference instead (PRD POS; UPC).
3. **Ads.** HLD specifies banner + rewarded; LLD has `wallet.reward` and a rewarded entry point. The PRD specifies banner + interstitial after every 2nd settle (HLD Integrations; LLD Android Hub; PRD Tiers).
4. **Kiosk attract ads.** HLD/LLD have no explicit slot, but `ad_impressions.device_id`/`creative_id` imply one (LLD schema). The PRD says none (PRD Ad rules 1, 3).
5. **Android Hub.** LLD uses Kotlin + JNI; the PRD uses Tauri 2 mobile. AdMob support in Tauri 2 mobile is unconfirmed (LLD Android Hub; UPC).
6. **Owner app.** HLD/LLD specify native Flutter with FCM/APNs. The PRD specifies a responsive web app + PWA with web push (LLD Owner app; PRD Owner app).
7. **Pi QR start.** LLD has the player log in and pick a plan, optionally via Tunnel. The PRD has no login, payment at the counter, a staff start, and a live timer only (LLD Pi; PRD Start by QR).
8. **Pi warnings and time-up.** LLD uses a 4 s switch, a way to turn warnings off, and grace before standby. The PRD uses a 5 s switch, no console grace, and a controller check (LLD Pi; PRD Pi).
9. **Player login.** LLD uses phone + PIN with `pin_hash`. The PRD uses username + password (LLD Intents; PRD Kiosk Login).
10. **Credit override.** LLD says "unless an override is recorded" and defines no intent or frequency. The PRD allows one override per player per day with a PIN (LLD Invariants; PRD Credit).
11. **Extend in grace.** LLD only allows `session.extend` in active/paused, and grace follows `session.end`. The PRD allows Extend during grace (LLD Timer; PRD Time-up).
12. **Time-up mode.** The PRD's "clean up" vs "staff handles" setting is absent from LLD (LLD Launcher logout; PRD Configuration).
13. **Variance sign.** The sign of `variance_paise` is undefined in LLD. The PRD sets it as counted − expected, with expected hidden until counted (PRD Shift).
14. **Force promote.** LLD rejects promotion with `LEASE_FRESH`. The PRD allows a PIN force promote, and the old Hub locks itself (LLD Lease; PRD Standby).
15. **Kiosk offline.** LLD displays time only from `session.tick`. The PRD keeps the countdown running locally and disables login, orders and Extend (LLD HUD; PRD Hub unreachable).
16. **Game media.** LLD has per-branch `art_blob` fetched by the Hub. The PRD/UX require global, curated media with no owner upload (LLD Library scan; EXP Game Media).
17. **Onboarding keys.** HLD issues a branch Razorpay key, but PRD step 8 says no provider keys (HLD Integrations; PRD Onboarding).
18. **Billing.** LLD has one plan per tier/currency band and no yearly cycle. The PRD has yearly at 10×, proration and a 7-day refund (LLD Billing; PRD Tiers).
19. **Kiosk orders tier.** LLD's Device `order.create` has no gate. The PRD makes kiosk orders Basic+ (LLD Permissions; PRD Tiers).
20. **PRD internal.** The PPP table cites "Stripe or Xendit", but the PRD elsewhere says "Razorpay only" (PRD Regional pricing vs Resolved second pass).

## 5. Gaps

- **Partner portal** (separate login, KYC, TDS, payouts). LLD covers only referral CSVs (LLD Referrals).
- **Global game-media service:** curation, storage and cache delivery.
- **Undo-toast** on money actions vs append-only ledgers (EXP Shared).
- **"Owner commands win" conflict rule** (EXP Offline & Sync).
- **Kitchen display queueing** during LAN loss.
- **Kiosk lockout duration:** UX says 60 s; LLD has `AUTH_LOCKED` only.
- **Builds:** Tauri Windows Hub and Tauri 2 mobile build/signing/Play policy.
- **PWA:** service worker and web push infrastructure.
- **Secrets:** only "VPS env, rotated quarterly"; no vault.
- **Rate limits:** none on the Owner API or presign.
- **PII retention/erasure:** DPDP/GDPR not addressed.
- **DR:** for `gz_control` and the VPS region.
- **Operations:** no SLOs, on-call rotation or OTel vendor named.
- **Versions:** none pinned.
- **Covered already:** environments, CI/CD and staged rollout (HLD Environments), restore drills (HLD Data), test plan (LLD).

## 6. Document titles as written

- "Arena360 — High-Level Design"; "Arena360 — Low-Level Design"; "Arena 360 — Product Requirements"; "Arena360 — User Journeys"; "Arena 360 — Experience Spine".
- "Gamezone SaaS — Low-Level Architecture" is cited by the PRD, HLD and LLD as authoritative for infra tables, batch format, endpoints and credentials. **It is not in `docs/`.**
- "Gamezone SaaS — Product Requirements" is cited by HLD Purpose; it refers to the PRD above.
