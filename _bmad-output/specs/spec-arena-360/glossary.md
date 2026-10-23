# Glossary — Arena 360

Terms used across SPEC.md and its companions. Architecture terms resolve in ARCHITECTURE-SPINE.md (AD-n); UX component names resolve in EXPERIENCE.md / DESIGN.md.

| Term | Meaning |
| --- | --- |
| Branch | One café location. Pricing, tier, subscription, invoice series and stock are per branch. A tenant (owner's business) has 1..n branches. |
| Hub | The branch's counter app and server: Windows (paid tiers) or Android (Free, also allowed on paid). Runs floor view, POS, money, reports; serves every LAN device. |
| Hub primary | The single Hub allowed to write branch data, holding the current epoch. |
| Standby | A second Hub on the LAN that follows the primary read-only and can be promoted manually (paid tiers). |
| Promote / force promote | Manager-PIN action turning the standby into the primary; force promote proceeds when normal promotion fails, warning that data since the last sync may be lost. |
| Epoch | Cloud-fenced identity `(counter, hub_instance_id)` of the current writer; a new epoch opens on promote or restore. A Hub that sees a higher epoch locks itself ("This PC is no longer the main Hub"). |
| Split-brain lock | The self-lock of a returning old primary; it never serves devices. |
| Kiosk | Windows app on each gaming PC; replaces the shell, enforces lockdown, holds no data. "The kiosk only asks; the Hub decides." |
| Overlay HUD | Always-on-top kiosk widget over games showing time and balance, warnings and Extend. |
| Attract screen | Kiosk idle screen between sessions (branding, video, rates, active rule; no ads). |
| Grace | Countdown (default 2 min) after prepaid time hits zero, over the still-visible game, with Extend available. Consoles have no grace. |
| Time-up mode | Per-branch action after grace: **clean up** (end non-allow-listed processes, sign out launchers, clear temp/clipboard, reset volume, back to attract) or **staff handles** (lock held over paused game). |
| Launcher logout | Signing out Steam, Epic, Riot, Discord and browser profiles and clearing saved credentials at session end. |
| Device group | Named set of devices (Basic, Pro, PS5, VR…) with colour; every rate, pack and rule is scoped to groups. |
| Zone | Floor-view area (PC hall, console room) grouping tiles; collapsible. |
| Floor view / tile / tile sheet | Hub home grid of device tiles; tapping a tile opens its non-modal action sheet. |
| Postpaid / prepaid | Postpaid: open session billed at end from the clock. Prepaid: session drawn from a pack, wallet, block or pass, counting down. |
| Pack / block / pass / membership | Pack: hours with validity. Block: fixed slot ending at fixed time. Pass: unlimited within a time window. Membership (v1.1): monthly recurring pass. |
| Dynamic pricing rule | Time-window (weekday + hours) or date rule overriding group rates; shown as the active rule chip. |
| Wallet | Player prepaid balance in minor units with full ledger. |
| Credit | Per-player postpaid limit with ageing buckets, reminders and auto-pause. |
| Inline PIN / PIN elevation | Manager PIN entered in place to authorise an action the signed-in user lacks; audit records both actors. |
| Setup code | One-time code an owner-created staff user enters at the Hub to set their own PIN and password. |
| Permission catalogue | Fixed list of ~40 permissions; each intent maps to exactly one. Owners compose roles; they do not invent permissions. |
| Intent / `apply()` | Every state change is an intent applied in one transaction and appended to the change log (AD-2). |
| Change log | Append-only per-epoch log of every mutation; it is the audit trail and the sync/backup stream. |
| Batch | Compressed slice of the change log uploaded to cloud storage by paid-tier Hubs. |
| Summary (`state/latest.json`) | Hub-computed current-state file for owner Today views, published per tier cadence. |
| Rollup | Hub-computed daily figures file published after close; owner reports sum rollups. Report fields are limited to rollup fields. |
| Data age (`as_of`) | Age of an owner-app figure; target Basic ≤ 15 min, Premium ≤ 5 min. |
| Command | Owner/admin write queued in the cloud mailbox, pulled and applied once by the Hub; states Queued on this device → Waiting for branch → Applied / Rejected / Expired (7 days). |
| Owner wins | An applied owner command overwrites a prior local Hub edit and announces it ("Price updated by owner"). |
| Audit builder | Cloud job that replays a branch's snapshot + batches on demand to answer owner audit searches ("Preparing audit…"). |
| Heartbeat | Periodic Hub → cloud call carrying upload state; returns licence, fence, cloud time and resend requests. |
| Licence / entitlement | Signed token carrying tier, flags and limits; the only source of paid features at the Hub; refreshed on heartbeat. |
| Tier | Free (₹0, 1 branch, Android Hub, ads), Basic (₹200), Premium (₹1,999), per branch per month + GST. |
| Tunnel | Cloudflare Tunnel + Access link from cloud to a Hub without inbound ports; gated by the Tunnel entitlement (add-on on any paid tier, included in Premium); carries live view and remote devices only. |
| Live view | Read-only, second-by-second floor view of a Hub through the Tunnel; Premium, or Basic + Tunnel add-on. |
| KOT | Kitchen order ticket printed on ESC/POS when a kitchen-flagged item is ordered. |
| Kitchen display (KDS) | Browser page on a LAN tablet: New → Preparing → Ready → Served. |
| Z-report | End-of-shift/day printable: sales by type, payments by method, expected vs counted cash, variance, waste, expenses, deposits, refunds, drawer opens. |
| Variance | Counted − expected cash (+ over, − short); over the branch threshold (default ₹100) needs manager PIN + reason. |
| Float | Opening cash in the drawer at shift start. |
| UPI last-4 | Required last 4 digits of the UPI transaction reference; UPI/card are recorded by reference, never via a payment integration. |
| Invoice series | Sequential per branch per financial year, never reused, segmented by epoch. |
| GST archive | Yearly archive of invoices, credit notes and tax summaries, kept 8 years regardless of tier. |
| Pi controller | Raspberry Pi per TV (v1.1) that shows idle/QR, switches TV input via HDMI-CEC (smart plug fallback) and enforces console time. |
| QR start / live timer | Player phone page on café Wi-Fi (no login) to request a console plan; then shows time left only. |
| HC theme | High-contrast theme, set per device by owner/manager; removes media, adds outlines, cuts motion. |
| Crypto-shredding | Player PII encrypted per player; erasure deletes the key so backups keep only unreadable ciphertext. |
| Partner | Reseller or installer referring cafés for 10% of subscription revenue; separate portal and login. |
| Admin app | Internal Arena 360 staff app (catalogue, partners, refunds, support), behind staff SSO. |
| PPP band | Regional price band (MY, SG, US) stored on the subscription so later table changes don't reprice. |
