---
type: reconcile
step: finalize-2
spine: ARCHITECTURE-SPINE.md
inputs:
  - 'docs/Arena 360 - Product.md (PRD)'
  - '_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/EXPERIENCE.md (EXP)'
  - '_bmad-output/planning-artifacts/ux-designs/ux-arena-360-final-2026-10-22/DESIGN.md (DES)'
created: '2026-10-23'
---

# Reconcile: PRD + UX against the Architecture Spine

Citations are `file Lnn` (line numbers at time of check). "Action" says whether to add an AD, amend one, add a convention, or defer.

## A. Contradictions (spine conflicts with an input)

| # | Item | Source | Spine | Action |
| --- | --- | --- | --- | --- |
| C1 | Hub idle < 50 MB RAM. A Tauri/WebView2 Hub (UI process alone usually exceeds this) cannot meet it as one figure. | PRD L432 | AD-15/AD-16 (Tauri UI on Hub) | **New AD: performance budgets** that scope 50 MB to the headless Rust Hub service, with a separate UI budget; or send the figure back to the PRD owner. |
| C2 | Sequential GST invoice numbers per branch and financial year, never reused. A force promote that loses unsynced data can reissue numbers already printed. | PRD L153, L425; EXP L589 | AD-11 (force promote may lose data) | **Amend AD-11 or new AD: invoice series fenced by epoch** (series prefix or epoch block, so gaps are allowed and reuse is impossible). |
| C3 | Branch settings hold "WhatsApp provider keys, SMS provider". | PRD L224 | AD-6 (no branch-held provider secrets); Gupshup called from the cloud (seed) | **New AD: notifications** (see M3). Flag PRD L224 for the PRD update. |
| C4 | Tunnel is a Basic **add-on** and included in Premium. Live owner view is Premium only. | PRD L462, L480, L375 | AD-6 says "Premium remote access via Tunnel" | **Amend AD-6**: Tunnel gated by an entitlement flag (Premium or add-on). Live view is a separate flag. |
| C5 | Cloud backup retention is 1 year (Basic) or 3 years (Premium). | PRD L457 | AD-24: money records kept 8 years; DR restores from snapshot + batches | **Amend AD-24/DR**: tier retention applies to batch history only. Snapshots, mirrors and the Hub DB keep money rows for 8 years. |
| C6 | Owner MFA: OTP on sign-up and new-device login (SMS), TOTP plus backup codes plus SMS fallback, fresh MFA for sensitive commands. | PRD L37, L372; EXP L402, L511 | AD-10: "owner password + TOTP" only | **Amend AD-10**: OTP via Gupshup, step-up MFA, and a list of sensitive commands owned by the command catalogue. |

## B. Missing (input has architectural consequences the spine does not cover)

| # | Item | Source | Action |
| --- | --- | --- | --- |
| M1 | **Performance budgets**: UI actions < 100 ms on LAN; floor state ≤ 2 s; 60 devices on a 2015 PC; KOT ≤ 3 s; console input switch ≤ 3 s; promote < 10 s; 1 GB export < 10 s; pairing visible ≤ 60 s; owner Today < 2 s from mirror. | PRD L196, L173, L244, L246, L259, L276, L278, L280, L315, L332, L432; EXP L258, L283 | **New AD** (push frames over WS, no polling on LAN; per-release benchmark on reference hardware in CI). |
| M2 | **Where entitlement is enforced**: device cap 20/60/200 (+20 add-on), Free 3 users, Basic+ kiosk orders, Basic+ KOT/kitchen display, paid-only standby, Windows Hub paid-only (Free = Android), owner-login add-on, ads removed "within one heartbeat" after upgrade, ads only return after expiry on lapse. | PRD L34, L449–468, L478, L480 | **New AD**: Hub `apply()` checks the entitlement flag in the permission step (Hub-local gates). The cloud API checks owner-side gates. JWT refreshed on heartbeat. Lapse = grace until `exp`. Also a pairing-time check for Windows Hub on Free. |
| M3 | **Notifications**: WhatsApp/SMS via Gupshup (receipts, credit reminders 3/7/14 days, low balance, session ending, console 5-min alert); owner alerts by web push + WhatsApp; templates versioned; consent required; reminders off on Free. | PRD L36, L90, L305, L343–352, L428; EXP L526, L594, L597 | **New AD**: Hub writes `notify.requested` intents into an outbox. The cloud sends through Gupshup after the batch ingests (paid tiers). The consent check runs at the Hub. Web-push transport stays Open Q7. |
| M4 | **RBAC + PIN elevation**: fixed ~40-permission catalogue, custom roles, PIN elevation records both users, PIN never leaves screen, owner creates staff (with PIN) by command, 5-min idle lock. | PRD L202–212; EXP L263, L342 | **New AD**: catalogue in `arena-proto`. Intent carries `actor` + optional `elevated_by`. The Hub verifies PINs (Argon2id). Owner-set PIN/password is hashed client-side or sealed to the Hub key, never plaintext in a command. |
| M5 | **Printing and hardware side effects**: ESC/POS over USB/LAN/Bluetooth, 58/80 mm, cash-drawer kick via printer, KOT, Z-report, no-printer path never blocks. AD-13 depends on knowing "receipt printed". | PRD L153, L173, L183, L222, L426; EXP L312, L337, L585 | **New AD**: side effects (print, drawer kick, notify, CEC) run after commit from an outbox and record a `printed`/`kicked` intent. Undo checks that record. Bluetooth printing is an Android Hub plugin. |
| M6 | **Kiosk lockdown/process control**: shell replacement, allow/block list, USB autorun off, game booster, blocked-launch log, admin exit by manager PIN or Hub unlock, FSE detection (HUD hidden, taskbar flash + sound; forced visible warning dropped). | PRD L260–264, L274; EXP L84, L237, L396–398 | **New AD**: privilege split (LocalSystem service enforces; unprivileged Tauri UI). Offline admin exit uses a cached manager-PIN verifier pushed by the Hub. Kiosk logs are queued intents (AD-14). |
| M7 | **Updates**: Hub daily check, install on restart, rollback; kiosk pulls from Hub cache; never during a session. The Android Hub updates via Play Store, so a control-plane percentage flag cannot stage it. | PRD L249, L263, L436, L541 | **Amend Rollout**: Play staged rollout for Android; installers cached on the Hub for kiosks; signed-update verification. |
| M8 | **Backup cadence per tier**: Free = manual export only; Basic = 5-min batches; Premium = 1-min batches. Hub-header backup status. Does Free upload at all? | PRD L245, L457; EXP L257 | **Amend AD-4**: uploader cadence and enablement come from the entitlement flag. State whether Free ships batches. |
| M9 | **Data export / restore**: full DB export at any time (incl. Free), to file/USB, restore from file with confirmation. Restore must not fork the log. | PRD L244, L434 | **New AD or amend AD-11**: restore = new epoch, same as promote. Export format documented. |
| M10 | **Consent storage**: per-player sign-up consent with policy/template version and timestamp, checked before every send; ad consent on Free. | PRD L225, L272, L428; EXP L159, L216 | **Amend AD-24** (consent row + version, kept with the tombstone). |
| M11 | **Partner referral attribution**: `referral_code` captured from marketing → sign-up → tenant; payout by the 10th; TDS 2%; GST invoice upload. | PRD L40, L540; EXP L111, L554 | **Amend AD-19**: attribution captured at tenant creation (URL/cookie → onboarding), immutable after first payment. |
| M12 | **Strings externalised; locale number/date formats; English only.** | PRD L226, L430 | **Convention**: one message catalogue in the shared UI package; `AppError` code → catalogue key; lint for hard-coded strings. |
| M13 | **Branch time zone and financial year**: opening hours, business day, Z-report, FY invoice series. | PRD L223, L374, L425 | **Convention**: store UTC ms; derive business day/FY from the branch tz on the branch row. |
| M14 | **Theme scoping**: light theme only on Owner app + web; HC is a per-device setting (Hub-stored); Sora bundled, no runtime font/CDN fetch on branch surfaces. | EXP L201, L381–382; DES L85, L125 | **Convention** (extend the Design tokens row). |
| M15 | **SQLCipher opt-in** for the Hub DB. | PRD L433 | **Deferred**, or an AD if kept for MVP (affects `arena-store`, backups and the keystore). |
| M16 | **Smart plugs (Tuya/Tapo)**: Tuya is usually cloud-only, which conflicts with offline. CEC best effort. | PRD L312, L322, L426 | **Deferred** (v1.1 Pi). Revisit: require a local-API plug. |
| M17 | **Device health/heartbeat → owner "branch offline" alert** (30 min in opening hours). | PRD L129, L347 | Covered by observability in part. **Amend Operational Envelope**: tenant-facing alert job in the cloud. |

Already covered (no action): shared tokens (AD-15), media fallback chain (AD-17 + EXP L315), barcode wedge (UI only), standby tiers + off-site standby (Deferred), CEC via libcec (seed), web push (Open Q7).
