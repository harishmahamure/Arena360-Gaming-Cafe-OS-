# Arena360 — High-Level Design

Oct 22, 2026 · @Harish

## Purpose and scope

This is the high-level design for Arena360: what the system is made of, where each part runs, how data moves, and the qualities it must have. It is the document an engineering lead reads before the low-level design and the one a new engineer reads first. Schemas, APIs and protocols live in "Gamezone SaaS — Low-Level Architecture"; features live in "Gamezone SaaS — Product Requirements"; the eight flows it must serve live in "Arena360 — User Journeys".

**Decisions this design is built on**

| Decision | Choice | Why |
| --- | --- | --- |
| Where the truth lives | In the branch, in one Hub's SQLite | A cafe must run with the internet down; one writer removes merge logic for money |
| What the cloud does | Mirror, mailbox, licence | Owner visibility and commands without the branch depending on it |
| Sync mechanism | Append-only change log shipped in batches | One serializer feeds backups, standby and the cloud mirror |
| Multi-writer | None inside a branch; primary/standby with an epoch | CRDTs cannot protect wallet and stock invariants |
| Cloud platform | Cloudflare edge + one Rust VPS + Turso + R2 | Zero egress, per-active-database billing, under ₹1,000 fixed |
| Free tier | Android Hub with AdMob; Windows Hub for paid | Ads need a mobile ad SDK; Free costs the company nothing |
| Connectivity beyond LAN | Cloudflare Tunnel first; WebRTC only Hub-to-Hub | Tunnel is free and needs no NAT code; TURN reserved for off-site standby |
| Billing | Razorpay only, autopay mandatory | One integration; recurring revenue by default |
| Kiosk platform | Windows 10+, Tauri 2 (Rust core + WebView2) | One Rust toolchain shared with the Hub; Windows 7 dropped |

**Out of scope here**: UI design (see the Arena360 design system), accounting exports, player mobile app, cloud gaming.

## System context

Arena360 sits between the people in a cafe and four outside services. Everything a player or staff member does stays inside the branch; only the owner, billing, messaging and ads cross the internet.

&#91;embedded content: system context · 4 user types, 4 external services\]

| Relationship | Direction | Channel | Fails how |
| --- | --- | --- | --- |
| Owner ↔ Arena360 | Both | Owner app over HTTPS to the cloud; premium live view through a Tunnel to the Hub | Owner sees stale data with an age stamp; branch unaffected |
| Staff ↔ Hub | Both | Hub UI on LAN | Standby promote; nothing leaves the LAN |
| Player ↔ Kiosk, Pi | Both | LAN WebSocket to the Hub | Kiosk locks and waits; game keeps running |
| Partner → Arena360 | In | Referral code at tenant sign-up | None at runtime |
| Arena360 ↔ Razorpay | Both | Subscriptions and webhooks from the cloud; dynamic UPI QR from the Hub when online | Hub falls back to manual confirmation; subscriptions retry |
| Arena360 → Gupshup | Out | Cloud sends WhatsApp and SMS on the Hub's behalf (queued through the change log) | Messages queue and send later |
| AdMob → Android Hub | In | AdMob SDK in the Free-tier Hub only | No ad shown; no effect on operations |
| Kiosk ↔ Launchers | Both | Local process control on each PC | Launch fails visibly; logout still enforced |

## Component view

Twelve components in three places. Only the Hub primary writes branch data; the cloud writes only its own control tables and the mirrors it derives from Hub uploads.

| Component | Runs | Responsibility | Owns data | Provides | Consumes |
| --- | --- | --- | --- | --- | --- |
| Hub primary | Branch counter PC (Windows) or tablet (Android, Free) | Sessions, players, wallets, POS, inventory, kitchen, shifts, RBAC, floor view; the single writer; change log; lease | Branch SQLite | LAN WebSocket API `:7400`, HTTP UI `:7401`, `/replicate` | Control plane HTTP, R2 presigned PUT, command queue |
| Hub standby | Second branch PC | Replays the change log; promotable | Replica SQLite | Promote UI | `/replicate` from primary |
| Kiosk | Each gaming PC | Lockdown, login, HUD, orders, game library, launcher logout, booster | Device token only | — | Hub LAN API |
| Pi controller | Raspberry Pi per console TV | QR idle screen, CEC input switching, smart-plug control, lock screen | Device token only | — | Hub LAN API |
| Hub UI | Browser on LAN | Staff interface served by the Hub | — | — | Hub HTTP and WebSocket |
| Kitchen display | Tablet browser on LAN | KOT queue and status | — | — | Hub WebSocket |
| Control plane | Cloud VPS (Rust) | Tenants, owners, subscriptions, entitlements, pairing, devices, signaling, referral codes | `gz_control` (Turso) | Hub API, Owner API, webhooks | Razorpay, Gupshup, Cloudflare API |
| Mirror ingester | Same VPS | Applies change-log batches from R2 into per-branch Turso DBs; gap detection; epoch quarantine | Per-branch mirrors (derived) | Mirror read API | R2, upload manifest |
| Command API | Same VPS | Owner commands, fan-out per branch, status, expiry | `commands` per branch | Owner API, Hub poll endpoint | — |
| Edge | Cloudflare | TLS, DDoS, Tunnel hostnames with Access, TURN, static hosting | — | Hostnames, TURN credentials | Hub tunnels |
| Owner app | Phone (Flutter) and web | Dashboards, alerts, commands, billing, MFA, live view | — | — | Owner API, Hub via Tunnel (premium) |
| Marketing and onboarding | Cloudflare Pages + Workers | Site, pricing with PPP, sign-up, onboarding wizard, partner pages | — | — | Control plane |

**Interfaces that matter**

| Interface | Style | Direction | Notes |
| --- | --- | --- | --- |
| Hub LAN protocol | WebSocket, JSON frames | Devices → Hub | Every intent is a request; the Hub pushes session events |
| Hub → cloud | HTTPS, bearer token | Hub initiates all | Heartbeat, presign, manifest, commands, results; idempotent |
| Hub → R2 | HTTPS presigned PUT | Hub → R2 | Batches keyed by epoch and sequence range |
| Replication | WebSocket | Primary → standby | Same batch format as the cloud upload |
| Owner API | HTTPS, session + MFA | Owner app → cloud | Reads from mirrors, writes commands |
| Tunnel | Cloudflare Tunnel + Access | Remote device → Hub | Premium; same LAN protocol inside |
| WebRTC | Data channel over STUN/TURN | Hub ↔ Hub | Off-site standby only; cloud is signaling |

## Deployment view

The whole fleet is served by one small VPS behind Cloudflare, with Turso and R2 as managed stores; every branch is a self-contained island of PCs on its own LAN that only ever makes outbound connections.

&#91;embedded content: deployment view · branch, Cloudflare edge, one VPS\]

| Node | Hosting | Sizing at launch | Scales by |
| --- | --- | --- | --- |
| Cloudflare Pages | Managed | Free | — |
| Cloudflare edge (DNS, TLS, Tunnel, Access, TURN) | Managed | Free plus TURN usage | Usage |
| VPS | Hetzner or equivalent, EU or Mumbai | 2 vCPU, 4 GB, one Rust binary with three roles | Second VPS for the ingester at 50% sustained CPU; stateless, so N behind Cloudflare load balancing |
| Turso | Managed | Free tier, then $4.99 Developer | Per monthly-active database; self-host libSQL if the fleet outgrows plans |
| R2 | Managed | Free to 10 GB | Usage |
| Hub primary | Cafe's counter PC (Windows 10+) or Android tablet (Free) | 50 MB RAM idle | One per branch |
| Hub standby | Cafe's second PC | Same binary | Optional, paid |
| Gaming PCs | Cafe's PCs, Windows 10 1803+ | Kiosk app, Tauri | Up to the tier's device cap |
| Pi per TV | Pi Zero 2 W or Pi 4 | Pi agent | One per console TV |
| Kitchen tablet | Any browser on LAN | — | — |

**Network rules.** The Hub opens no inbound ports. Outbound from the branch is HTTPS 443 only: heartbeat, presigned PUTs to R2, command polling and, on premium, the `cloudflared` tunnel. Remote devices reach the Hub through the tunnel hostname behind Cloudflare Access. Hub-to-Hub replication across sites uses a WebRTC data channel negotiated through the control plane, STUN first and TURN as fallback.

**Regions.** The VPS lives in one region; branches anywhere in the world talk to it through Cloudflare's anycast edge, which keeps latency acceptable because every branch call is a background task, never on a player's critical path.

## Data architecture

Every row of branch data has exactly one owner, the Hub primary; everything in the cloud is either control data the cloud owns or a derived copy it can rebuild from R2. The loop closes through the same `apply()` the kiosk uses, so an owner command is just another intent.

&#91;embedded content: data flow · change log up, commands down\]

The standby replays the same batches over LAN that the uploader ships to R2; one serializer, one replayer.

**Stores and ownership**

| Store | Where | Owner | Contains | Rebuildable from |
| --- | --- | --- | --- | --- |
| Branch SQLite | Hub primary | Hub | Domain tables, `changes`, `lease`, device registry, entitlement cache | R2 snapshot + batches |
| Replica SQLite | Hub standby | Standby (read-only until promoted) | Same schema | Primary |
| R2 bucket | Cloudflare | Cloud | Change-log batches by tenant/branch/epoch/seq, monthly snapshots, kiosk and Hub installers | Source of truth for history |
| `gz_control` | Turso | Control plane | Tenants, owners, subscriptions, entitlements, hubs, devices, pairing codes, manifests, cursors, referrals | Turso PITR |
| Branch mirrors `gz_b_*` | Turso | Ingester | Domain tables + `changes` + `commands` per branch | R2 batches |
| Owner app cache | Device | Owner app | Last dashboards, pending commands | Owner API |

**Rules**

1. Money is integer paise; time is unix milliseconds in UTC; ids are ULIDs minted by the Hub, so rows stay unique across branches when mirrored.
2. The change log is append-only and gap-free per epoch; a reader detects loss by sequence number, never by timestamp.
3. The mirror is eventually consistent and says so: every owner-facing number carries the time of the last ingested batch.
4. Commands are idempotent on the Hub (applied command ids recorded in the same transaction) and expire after 7 days.
5. PII is minimal: player phone and name on the branch and in its mirror; no card data anywhere (Razorpay holds it).
6. Retention: branch `changes` pruned after 90 days once uploaded; R2 keeps 1 year (Basic) or 3 years (Premium); mirrors keep what R2 keeps.

**Backup and restore.** A branch is restored from the latest monthly snapshot plus every batch since, replayed by the same replayer. Restore is tested by the onboarding wizard's "restore drill" on paid tiers once a quarter.

## Integrations

Seven outside integrations, each behind one adapter module so a provider can be swapped without touching domain code. None of them sits on a player's critical path.

| Integration | Used for | Called from | Protocol | Offline behaviour | Secrets live |
| --- | --- | --- | --- | --- | --- |
| Razorpay Subscriptions | Tenant billing, autopay mandates, PPP currencies | Control plane | REST + webhooks (signature verified) | N/A (cloud) | VPS env, rotated quarterly |
| Razorpay Payments | Dynamic UPI QR at the counter | Hub when online | REST from Hub with a branch-scoped key; webhook to cloud, relayed to Hub as a command | Hub shows a static UPI QR and staff confirm manually; flagged for reconciliation | Branch key issued by control plane, cached encrypted on Hub |
| Gupshup | WhatsApp receipts, credit reminders, session alerts; SMS fallback; owner OTP | Control plane on the Hub's behalf (requests ride the change log) | REST, templates pre-approved | Messages queue in the change log and send on next upload | VPS env |
| AdMob | Banner and rewarded ads on the Free Android Hub | Android Hub | AdMob SDK | No ad shown; app continues | App id in build config |
| Cloudflare Tunnel + Access | Remote devices and owner live view (premium) | Hub spawns `cloudflared` | Outbound tunnel; Access service tokens | Remote devices wait; LAN unaffected | Tunnel token delivered per heartbeat |
| Cloudflare TURN | Off-site standby replication fallback | Hub | ICE credentials from control plane | Replication pauses; resumes from last seq | Short-lived credentials |
| ESC/POS printers, cash drawer, barcode scanner, HDMI-CEC, Tuya/Tapo plugs | Receipts, KOT, drawer kick, scanning, console control | Hub and Pi | USB, LAN 9100, Bluetooth SPP; CEC via `cec-client`; local plug API | Printer offline shows a badge; kitchen display still works | None |

**Integration principles**

- The Hub never holds a tenant-wide secret; every key it has is scoped to its branch and revocable.
- Webhooks land in the cloud only; anything a branch needs to know arrives as a command, so the Hub has a single inbound channel to reason about.
- Each adapter has a recorded fake for tests and a "manual" mode for the counter, so a provider outage degrades to a human step, never a blocked sale.

## Technology stack

Two languages for the whole system: Rust for everything that holds data, and Dart for the owner app. The Kiosk is Tauri, so its core is the same Rust.

| Component | Language, framework | Key libraries | Why |
| --- | --- | --- | --- |
| Hub (Windows) | Rust, Axum, Tokio | rusqlite (WAL), tokio-tungstenite, zstd, rmp-serde, mdns-sd, ed25519-dalek, reqwest | One static binary, 50 MB idle, same codebase as the cloud |
| Hub (Android, Free) | Rust core via JNI + Kotlin shell | Same Rust crate; AdMob SDK; Android foreground service | Rust logic shared; Kotlin only for UI shell and ads |
| Hub UI | Svelte, TypeScript | Tabler icons, Manrope | Small bundle for old counter PCs; served from the binary |
| Kiosk | Rust, Tauri 2, Svelte front end in WebView2 | tokio-tungstenite, windows crate for hooks and process control, windows-service, Tauri updater | Shares arena-proto and cleaners with the Hub; Windows 10 1803+ only |
| Pi agent | Rust (aarch64) | cec-client bindings, framebuffer or minimal Wayland for QR screen | Same crate family; low RAM |
| Kitchen display | Web page served by Hub | Svelte | Any tablet, no install |
| Control plane, ingester, command API | Rust, Axum, Tokio | libsql client (Turso), aws-sdk-s3 for R2, jsonwebtoken (EdDSA), argon2 | One binary, three roles by flag |
| Owner app | Dart, Flutter (iOS, Android, web) | riverpod, dio, local\_auth, flutter\_secure\_storage | One codebase for phone and web dashboards |
| Marketing and onboarding | Astro on Cloudflare Pages, small Workers | — | Static, free, fast in every region |
| Data | SQLite (branch), Turso/libSQL (cloud), R2 | — | Same SQL dialect end to end; migrations shared |
| Observability | OpenTelemetry traces from VPS; Hub diagnostics bundle; Sentry for Kiosk and Owner app | — | Cheap, standard |
| Build and release | GitHub Actions; cargo, MSBuild, Flutter; code signing for Windows and Android | — | Staged rollout from control plane |
|  |  |  |  |

## Non-functional requirements

The branch is held to real-time standards; the cloud is held to "eventually, and honest about it".

| Quality | Target | Where measured | How the design meets it |
| --- | --- | --- | --- |
| Availability (branch) | Sessions and sales work 100% of open hours with WAN down; Hub PC failure recovered in under 10 s with a standby | 8-hour offline soak, failover drill | Local-first, single writer, lease and epoch |
| Availability (cloud) | 99.5% monthly for Owner API; branch never depends on it | VPS uptime | Hub uploads direct to R2; stateless VPS behind Cloudflare |
| Latency (LAN) | Session start, sale, end under 100 ms server-side, under 2 s end to end including kiosk unlock | Hub timing logs | SQLite WAL, in-process, no network hops |
| Latency (owner) | Dashboard under 2 s; command visible as applied within 90 s online | Owner API traces | Mirror read API with 30 s cache; 60 s poll plus poke |
| Data loss | Zero committed transactions lost on Hub crash; at most one upload interval lost on disk failure without standby | Crash tests, restore drills | WAL, fsync on commit, batched uploads, monthly snapshots |
| Consistency | Wallet and stock never negative; one active session per device | Property tests on `apply` | Invariants inside one transaction |
| Throughput | 60 devices per Hub, 5 intents per second sustained, 50 per second burst | Load test on a 2015 PC | Thousands of SQLite tx/s available |
| Footprint | Hub idle under 50 MB RAM, Kiosk under 80 MB, no GPU use | Reference hardware benchmark | Rust binary, WPF without composition effects |
| Cost | Under ₹20 marginal per paid tenant per month; free tenant ≈ ₹0 | Cloud bills | Batched R2 writes, no per-tenant compute |
| Security | No inbound ports on Hub; every device token revocable within one heartbeat; signed entitlements | Pen test before GA | See Security |
| Compatibility | Kiosk on Windows 10 1803 to 11; Hub on Windows 10+ and Android 9+ (3 GB RAM); any ESC/POS printer | Compatibility matrix | Tauri; Rust; raw ESC/POS |
| Offline duration | Unlimited for Free; paid features resume from cursor after any outage; entitlement cache 7 days | Soak test | Change-log cursors, cached JWT |
| Upgrade safety | Hub and Kiosk updates never during an open session; rollback in one step | Release checklist | Staged rollout flags from control plane |
| Localisation | UI strings externalised; number, date and currency by locale; PPP pricing bands | Build check | i18n from day one |
| Observability | Every write traceable to an actor; diagnostics bundle under 5 MB; VPS traces with tenant id | Audit view, support tooling | Change log is the audit log |

## Security architecture

Trust is per device and per branch. Nothing in the system accepts an unauthenticated connection, and nothing a branch holds can reach another branch's data.

| Layer | Control | Detail |
| --- | --- | --- |
| Identity | Owner accounts with password + TOTP; staff accounts with password + PIN on the Hub; devices with 256-bit tokens | Owner MFA mandatory on new devices; staff PIN elevates inside the sheet |
| Device trust | Pairing codes (single use, short expiry) exchanged for tokens; argon2 hashes stored, plaintext only in the device keyring | Revocation from Owner app or Hub takes effect within one heartbeat or immediately on LAN |
| Authorisation | RBAC permission catalogue checked inside `apply`; every intent maps to one permission | A kiosk token cannot perform staff actions regardless of what it sends |
| Entitlement | EdDSA-signed JWT, 7-day expiry, public key embedded in Hub | Tampering produces a signature failure and free-tier behaviour |
| Network | Hub outbound-only on 443; Tunnel behind Cloudflare Access; TURN credentials one hour; LAN WebSocket plain by default with optional TLS | Threat on LAN is a staff member on the same switch; tokens and PINs address that |
| Data | Branch SQLite optional SQLCipher; R2 encrypted at rest; presigned URLs scoped to one key; PII limited to phone and name | No card data anywhere; Razorpay tokenises |
| Isolation | Per-branch Turso DB; per-branch R2 prefix; tenant scope enforced server-side on every Owner API call | A compromised Hub can read and write only its own prefix |
| Audit | `changes.actor` and `intent_id` on every row; `commands.issued_by`; owner-visible audit view | Audit is the change log, not a second system |
| Supply chain | Signed Windows and Android builds; Hub verifies Kiosk update signatures before serving them on LAN | Staged rollout with rollback |
| Abuse | Rate limits on pairing, login and OTP; kiosk login lockout; AdMob policy compliance on the Free Hub | Alerts to the owner on repeated failures |

Threats explicitly out of scope: a cafe owner defrauding their own staff (the system records, it does not police) and physical theft of the counter PC (SQLCipher is offered for that).

## Scalability and capacity

The design scales by adding tenants, not by making anything bigger: each branch brings its own compute, and the cloud's work per tenant is a few hundred requests and a few megabytes a day.

| Scale | Branches | Cloud load per day | VPS | Turso | R2 | Monthly cloud cost |
| --- | --- | --- | --- | --- | --- | --- |
| Launch | 1–100 | \~50 k requests, \~3 GB batches | 1 × 2 vCPU, idle | Free tier | Free tier | under ₹1,000 |
| 1,000 tenants (half paid) | 1,000 | \~0.5 M requests, \~30 GB batches | 1 × 2 vCPU, \~10% | Developer plan | \~0.5 TB | \~₹6,000 |
| 10,000 tenants | 10,000 | \~5 M requests, \~300 GB batches | 2 × 4 vCPU (API + ingester) | Scaler or self-hosted libSQL | \~5 TB | \~₹60,000 |
| 100,000 tenants | 100,000 | \~50 M requests, \~3 TB batches | 4–6 VPS behind Cloudflare LB; ingester sharded by tenant hash | Self-hosted libSQL cluster or enterprise | \~50 TB | \~₹10 lakh, 2.5% of revenue |

**What scales and how**

- **Hub**: fixed per branch; 60 devices and 5 intents/s are far below SQLite's ceiling. The only per-branch growth is the change log, pruned at 90 days.
- **VPS**: stateless; the ingester partitions by `hash(branch_id) mod N`, so adding an ingester never reorders a branch's batches. The command API and control plane are plain request handlers.
- **Turso**: one DB per branch keeps the hot set small and the blast radius one cafe. If managed plans run out, the same libSQL runs self-hosted on the VPS cluster with no application change.
- **R2**: object storage; growth is linear in paid tenants and bounded by retention.
- **Owner API**: cached reads; the heaviest query is one branch's month, served from its own small DB.
- **Ads (Free)**: served by AdMob, so impression volume never touches the VPS.

**Where the design does not scale and that is deliberate**: live owner views are served by each Hub through its tunnel, never by cloud compute, so there is no per-tenant always-on process anywhere.

## Environments, CI/CD and rollout

Three environments, one deploy pipeline, and a rollout that treats the branch fleet as the thing that must never break.

| Environment | Cloud | Branch software | Data | Purpose |
| --- | --- | --- | --- | --- |
| dev | Each engineer's local Rust binary with SQLite in place of Turso and MinIO in place of R2 | Hub and Kiosk against a local Hub on the same machine or VM | Seeded demo tenant | Daily work |
| staging | Separate VPS, Turso org and R2 bucket; `api.staging.arena360.app` | Internal test cafe with 10 PCs, a Pi and a printer; two friendly beta cafes | Anonymised copy of production control data; synthetic branch data | Release candidate soak, failover and restore drills |
| production | VPS behind Cloudflare; `api.arena360.app` | The fleet | Real | — |

**Pipeline** (GitHub Actions): lint and unit tests on every push; property tests for `apply` invariants; integration tests that run a Hub, a fake Kiosk and the cloud binary together; Windows and Android builds signed; Flutter builds for stores and web; release candidate to staging on tag.

**Rollout order on a release**: cloud first (backward compatible with every Hub version in the field), then Hub to 1% of branches by control-plane flag, 10%, 50%, 100% over a week, then Kiosk the same way served through each Hub. Hub updates install on restart only when no session is active; Kiosk updates between sessions. Rollback is the previous build re-flagged; schemas migrate forward only with additive changes until the fleet has caught up.

**Compatibility contract**: a Hub must accept any Kiosk from the previous two releases and any command the cloud emitted in the last 7 days; unknown message kinds return `UNSUPPORTED` rather than failing.

**Operations**: VPS metrics and traces to a hosted OpenTelemetry backend; alerts on ingest lag over 15 minutes per branch, command backlog, webhook failures, and heartbeat gaps across many branches at once (a cloud problem, not a cafe problem). Support uses the Hub's diagnostics bundle and the mirrored audit log, never direct access to a branch.

## Journey-to-component traceability

Every one of the eight user journeys is served by the components below; the column "must not fail" names the single component or link each journey depends on most.

| Journey | Hub | Kiosk | Pi | Owner app | Control plane | Ingester + mirror | Command API | Edge | Must not fail |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 Owner onboarding | ● | ● | ○ | ● | ● | — | — | Pages | Pairing: control plane → Hub |
| 2 Walk-in postpaid | ● | ● | — | — | — | — | — | — | Hub `apply` and LAN |
| 3 Prepaid at kiosk | ● | ● | — | — | — | — | — | — | Kiosk HUD ↔ Hub |
| 4 Order to kitchen | ● | ● | — | — | — | — | — | — | Hub → printer / display |
| 5 Shift close | ● | — | — | ○ | — | ○ | — | — | Expected-cash arithmetic in Hub |
| 6 Remote price change | ● | ○ | — | ● | ● | ○ | ● | — | Command idempotency on Hub |
| 7 Credit player | ● | ● | — | ○ | ● (Gupshup) | ○ | ○ | — | Limit check inside `apply` |
| 8 Hub failover | ● (standby) | ● | ○ | — | ○ | ○ | — | — | Lease epoch and mDNS re-resolve |

● primary participant · ○ secondary or later · — not involved

The table makes the priority order concrete: journeys 2, 3 and 4 depend only on the Hub, Kiosk and LAN, which is why the branch stack is built and hardened first.

## Risks, mitigations and open items

| Risk | Likelihood | Impact | Mitigation | Owner |
| --- | --- | --- | --- | --- |
| Kiosk lockdown bypassed | Medium | Players reach the desktop; revenue leak | Shell replacement, hook-based key blocking, process allow-list; pen test before GA; bug bounty for cafes | Engineering |
| Split-brain after a bad promote | Low | Duplicate writes during a partition | Epoch in every transaction; old-epoch batches quarantined; manual promote in v1 | Engineering |
| R2 write costs from a bad interval | Medium if misconfigured | 100× cost per tenant | Hard floor of 30 s on `upload_interval_s`; cost alert per tenant | Engineering |
| Turso plan ceiling at tens of thousands of active DBs | Medium at scale | Cost or forced migration | Self-hosted libSQL path tested in staging by 1,000 tenants | Engineering |
| Razorpay unavailable in a target country | High for some regions | Cannot sell there | Pricing page hides unsupported countries; revisit PSP per region after launch | Product |
| AdMob policy rejects the kiosk/counter use case | Medium | Free tier loses funding | Ads only in the Android Hub app (a normal app surface), never injected on PCs; review policy before Free launch | Growth |
| Gupshup template approval delays | Medium | Reminders late at launch | Submit templates at wave 1; SMS fallback | Product |
| Scope: everything in one launch | High | Date slips | Wave-1 beta with 20 cafes while waves 2 and 3 are built; cut list agreed before beta | Founder |
| Windows 7 gaming PCs cannot run the Kiosk | Medium | Onboarding friction | Stated minimums on the downloads page; partners offer OS upgrades as a service; Android Hub for old counter PCs | Product |
| Printer compatibility | Medium | Support load | Raw ESC/POS, tested-device list, "print test" in onboarding, WhatsApp receipts as a no-printer path | Engineering |

**Open items for the LLD**

- [x] Exact Access policy and service-token rotation for Tunnel hostnames
- [x] WebRTC signaling message schema for off-site standby
- [x] libSQL self-hosting runbook and the migration from managed Turso
- [x] AdMob placement review against policy for the Android Hub
- [x] Pi image build pipeline and CEC compatibility matrix
