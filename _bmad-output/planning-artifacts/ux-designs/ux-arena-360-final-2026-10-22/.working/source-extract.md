# Source extract — Arena 360 (UX-oriented)

Sources: `docs/Arena 360 - Product.md` (titled "Gamezone SaaS — Product Requirements"), `docs/Arena360 — User Journeys.md`. Both Oct 22, 2026 · @Harish.

## 1. Product
"Gamezone is a local-first management system for gaming cafes and game zones": a Hub app at the counter, a Kiosk app on every gaming PC, a Pi controller for consoles, an Owner app for multi-branch oversight, and a marketing site with self-serve onboarding. Branch works with internet down; cloud mirrors data and relays owner commands. Customers: cafe owners (1–5 branches), primarily Indian, all countries at launch. Business model: per-branch monthly SaaS — Free (₹0, 1 branch, ad-funded, Android Hub only), Basic ₹200, Premium ₹1,999; PPP regional pricing; add-ons; reseller/installer partner program (10% recurring). Promise: "run your cafe offline, see it from anywhere, pay less than you do now."

## 2. Surfaces
- **Hub app** — counter; Windows (paid, Win10+) or Android tablet/phone (Free, with AdMob banner + rewarded ads). Floor view, sessions, players, plans, devices, dynamic pricing, POS (grid left / cart right / settle bottom), inventory, shifts/Z-report, users/RBAC, config, offline reports, backup/standby, audit.
- **Kiosk app** — Windows 7 SP1–11, .NET 4.8 WPF, shell replacement on each gaming PC. Attract screen, login/sign-up, plan picker, always-on-top **Overlay HUD**, warnings, extend, time-up/grace lock, orders, game library, launcher logout, messages.
- **Kitchen display** — browser page on any LAN tablet (new → preparing → ready → served); plus KOT thermal printer.
- **Pi controller** (v1.1 wave) — TV idle screen with branding/rates/QR, lock screen, input switching; **QR start page** on player's phone (Hub page via LAN/Tunnel).
- **Owner app** — phone app + web app, same account; dashboards, alerts, commands, MFA, subscription, branches, Premium live view. Free shows subscription/pairing/support only.
- **Marketing site** — static; Home, Product, Pricing, Demo (in-browser sandbox Hub), Compare, Get started, Downloads, Help centre, FAQ, Status/changelog, Partners, Legal.
- **Onboarding wizard** — web, resumable, 10 steps ending in a test session.
- Also: thermal receipts, WhatsApp/SMS messages (Gupshup), push notifications.

## 3. Personas
Owner; Branch manager; Counter staff ("often young, high turnover"); Kitchen staff; Player ("14–30, visits weekly, pays cash or UPI"). "The Hub is designed for counter staff first." No named protagonists — roles only. Default roles: Owner, Manager, Counter, Kitchen, Kiosk-device; custom roles from a ~40-permission catalogue.

## 4. Journeys ("The eight journeys")
1. **Owner onboarding** — Owner; Site, Onboarding, Hub, Kiosk. Sign up (OTP) → Create branch/tier → Pairing code (8-char, 15-min, QR) → Install Hub → Hub paired → Kiosks pair (6-digit) → Groups and plans → Invite staff → Test session (2-min) → Go live. Fail: expired code → "generate a new code"; manual Hub IP.
2. **Walk-in postpaid session** — Counter staff, player; Hub, Kiosk. Arrive → Find player/Guest → Start postpaid from tile sheet → Kiosk unlocks (HUD elapsed+rate) → Plays/orders → "I'm done" or staff end → "Pay at counter" → Bill computed → Settle (split) → Rest to credit → Receipt → PC back to free (tile green). Start/settle <10 s.
3. **Prepaid player at the kiosk** — Player; Kiosk, Hub. Attract → Login (phone+PIN) → Pick plan → Hub approves → Plays → Warnings 10/5/1 (HUD amber at 10) → Extend? → Time up (2-min grace over visible game) → Lock screen → Cleanup. Refusal: balance + "Top up at counter".
4. **Kiosk order to kitchen** — Player, kitchen; Kiosk, Hub, Kitchen display. Open menu → Build cart → Place order → KOT prints → Kitchen display → Marked ready → HUD shows ready → Served → Settled with bill. Basic+ only.
5. **Shift close and variance** — Counter staff, manager; Hub. Shift opened → Sales → Deposits/spend → Close shift (blind count) → Variance shown → Within threshold / Over threshold (PIN+reason) → Owner alerted → Z-report → Next shift.
6. **Owner changes a price remotely** — Owner; Owner app, cloud, Hub. Edit price (preview branches) → Command queued → "Waiting for branch" → Hub polls → applies / Rejected → logged → result → badge applied or reason+retry; "expired" after 7 days.
7. **Credit player** — Player, staff, owner; Hub, WhatsApp, Owner app. Credit enabled → Plays on credit (headroom shown) → Limit checked ("Credit limit reached, see counter") → Ledger → Reminders → Overdue 15 d+ → Returns → Collect → Balance drops.
8. **Hub fails, standby promoted** — Counter staff; Hub standby, Kiosk. "Kiosks reconnect by themselves." No step table in source.

## 5. Explicit UX requirements
- Touch targets ≥44 px on Hub; high-contrast theme; POS keyboard shortcuts; "Counter flow possible without mouse".
- Every common action one tap from floor view; no action >2 taps; "no modal that blocks a sale"; manager PIN inline ("PIN entry never leaves the current screen"); idle lock 5 min.
- Floor view: device tiles coloured by group, zones, states (free, in session w/ time left, reserved, offline, maintenance, order pending); active pricing rule in header; up to 60 devices on a 2015 PC.
- Offline indicator: "nothing else changes"; owner app shows data age / "Waiting for branch".
- HUD must not steal focus; hides in fullscreen exclusive with toasts; toast+sound warnings.
- Kiosk ads only on attract screen, never during a paid session; no adult/gambling/alcohol.
- Shift sheet never shows expected cash before count; "a number, not an accusation".
- Languages: English, Hindi at MVP; Tamil, Telugu, Kannada, Bengali, Indonesian, Vietnamese later; locale number/date/currency formats.
- Performance: UI <100 ms; state updates ≤2 s.
- Hardware: ESC/POS printers (USB/LAN/BT), cash drawer, barcode scanners, Pi, HDMI-CEC TVs, smart plugs.
- Payments: cash, UPI dynamic QR (Razorpay/PhonePe), card reference, wallet, credit; GST invoices.
- Journey-diagram colour convention: green = key step, grey = context, dashed = failure branch. Branding: kiosks/Pi show "branch branding"; co-branded kiosk for partners.

## 6. Gaps / questions
- Product name inconsistent: "Gamezone" (PRD) vs "Arena360"/"Arena 360" — which is the brand?
- No brand assets, palette, typography, logo, or tone of voice specified.
- Journey 8 has no step detail.
- Hub form factors: Windows desktop vs Android phone vs tablet — one responsive layout or separate? Phone-size Hub for POS/floor view?
- Kiosk resolutions/ultrawide; WPF constrains design system reuse.
- Owner app: native vs PWA; parity between phone and web.
- Light/dark theme default; how high-contrast differs.
- Tenant/branch theming extent on kiosk/Pi screens.
- RTL/script support and string-length budgets for 8 languages.
- Ad-unit sizes/placements on Android Hub banner; rewarded-ad player flow UX.
- Kitchen display device size; sound alerts?
- QR start page and Pi screens (v1.1) visual spec.
- Marketing site illustration style and demo video ownership.
- Conflicts: payments "Razorpay/PhonePe" vs later "Razorpay only"; credit override "once" vs "once per day"; tier table gives Free "Kiosk app, lockdown, overlay, orders" but Journey 4 says on Free "orders are taken at the counter POS only"; Free Hub listed on "tablet or phone" while Hub UX assumes floor-view density.
