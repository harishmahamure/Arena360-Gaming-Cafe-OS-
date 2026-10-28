# Epic 6 Context: Player experience at the kiosk

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Players log in or sign up at the kiosk, see an overlay HUD with time warnings, extend through the grace period, launch games from the branch library, order food and drink from the seat, and keep playing when the Hub drops. It also delivers the global game catalogue pipeline (control plane to Hub cache to kiosk) that gives the game library its art. This is the player-facing half of the product: the kiosk only asks, the Hub decides.

## Stories

- Story 6.1: Control plane publishes the global game catalogue to Hubs
- Story 6.2: Hub caches catalogue and media, serves them on the LAN
- Story 6.3: Branch game and app library at the Hub, applied to every kiosk
- Story 6.4: Kiosks scan for installed games and launchers
- Story 6.5: Hub matches scanned games to the catalogue; bulk approval
- Story 6.6: Game media falls back from video to cover to solid
- Story 6.7: Attract screen with branding, rates and active rule, no ads
- Story 6.8: Player login with username and password
- Story 6.9: Plan picker starts a prepaid session
- Story 6.10: Sign-up with one consent box
- Story 6.11: Session-end cleanup of programs, clipboard, temp files, volume
- Story 6.12: Session-end sign-out of launchers, Discord, browsers
- Story 6.13: Player shell with top bar, pill nav, time chip
- Story 6.14: Overlay HUD without stealing focus
- Story 6.15: Volume control and call staff
- Story 6.16: Staff messages and the messages bell
- Story 6.17: Account menu with Lock and Log out
- Story 6.18: "I'm done" ends a postpaid session
- Story 6.19: Time warnings as toasts, chip warning, taskbar flash
- Story 6.20: Per-branch warning marks and low-balance warning
- Story 6.21: Extend from the kiosk with live prices
- Story 6.22: Grace lock with Extend, then branch time-up mode
- Story 6.23: Games tab and one-click launch
- Story 6.24: Filter and search the Games tab
- Story 6.25: Game booster and launch time for staff
- Story 6.26: Launcher cards open the player's own account
- Story 6.27: Play hours per game and "Most played here"
- Story 6.28: Apps tab
- Story 6.29: Home with featured game hero video and featured row
- Story 6.30: Shop with live stock and popular items
- Story 6.31: Cart and add-to-session-bill from the seat
- Story 6.32: Pay now from wallet, Pay at counter, unpaid lines at prepaid end
- Story 6.33: Order status on the HUD
- Story 6.34: Account page (details, wallet and packs, orders, change password)
- Story 6.35: PC settings saved to player profile
- Story 6.36: Kiosk keeps the session running while Hub is unreachable
- Story 6.37: Screens reflow at every width, keyboard and high contrast
- Story 6.38: Player journey proven on reference hardware

## Requirements & Constraints

- Login is under 3 s on LAN; errors never reveal which field failed; 5 failures lock login for 60 s with a visible countdown and a Hub alert (supersedes any "3 failures" wording).
- Sign-up (username, password, phone, name; no OTP) is blocked until the single unticked consent box is ticked; duplicate username/phone give field-level errors.
- Attract screen renders from cached branding and rates with no ads; ads never appear on the kiosk.
- Shell tabs: Home, Games, Apps, Shop (Shop hidden on Free; ordering needs the kiosk-orders licence flag, Basic and above). Volume resets to the branch default at logout.
- HUD never steals focus, hides in fullscreen-exclusive games (then taskbar flash and sound at 5 and 1 min). Warnings at 10/5/1 min and low balance persist until dismissed or time-up; the chip gains a warning border at 10 min or less. Info toasts auto-dismiss after 8 s and stay in the messages bell. Global Extend hotkey is Ctrl+Shift+E.
- Extend is priced live by the Hub, charged to wallet, pack or credit, and reaches the floor within 2 s. At zero a grace countdown (default 2 min) runs over the visible game, then the branch time-up mode applies (clean up and return to attract, or lock held for staff).
- Session end: end non-allow-listed processes, clear clipboard and temp files, reset volume, sign out Steam, Epic, Riot, Discord and browser profiles. Failures report to the Hub and badge the tile. The next player must never see prior accounts.
- Background video is muted, never autoplays audio, and stops entirely while a game runs. No backdrop-filter or live blur anywhere; Sora bundled, no runtime font or CDN fetch.
- Orders: live stock check, out-of-stock greyed, bundles refused if short a component, order attaches to the running session or is paid from the wallet; with no session it is "Pay at counter". Orders reach Hub POS and KOT within 3 s.
- Hub unreachable: the session continues on last known time, time-up is enforced locally, the player is never locked out, "Reconnecting…" shows, and state reconciles on reconnect. Every Hub-dependent action is disabled with "Counter is reconnecting — try again shortly."
- Hours per game per branch feed a top-10 "Most played here" carousel (last 7 days, branch-level); top-3 shop items by units sold get a popular badge.
- Kiosk hardening target: no desktop escape in 30 minutes of attempts on Windows 10 and 11.

## Technical Decisions

- Single mutation path: kiosk intents go through the kiosk service over the paired-device connection to the Hub's `apply()`. They carry `actor{kind: player, id}` (or `device` for guests and system events) plus device id, an idempotency key, TTL and `base_rev` for edits. A fixed self-service permission set in `arena-proto` bounds them (`session.self_start`, `session.self_extend`, `session.self_end`, `order.self_place`, `player.self_update`); a player acts only on their own session and profile. Add every new intent to the Story 1.8 `proptest` generator.
- Kiosk is a Tauri 2 / Svelte 5 UI running unprivileged. A LocalSystem Windows service enforces lockdown and process control. The shared UI package supplies tokens, components and generated ts-rs types; hand-written protocol types are forbidden.
- Time-up offline: each Hub tick carries `lock_after_ms` and `time_up_mode`, already resolved. While the Hub is unreachable the kiosk counts down on its monotonic clock (not the wall clock). Login, orders and Extend are disabled; the kiosk writes no state.
- Game catalogue: one global catalogue (games, exe signatures, content-addressed media) lives in `gz_control`, with media in R2 under `catalog/<sha256>.<ext>` behind the CDN. A Hub pulls a versioned manifest (ETag/304, 6-hourly) into a reference cache kept outside branch state and the change log, never through `apply()`. Covers are always kept; videos are best-effort and LRU-evicted under a cap (default 10 GB, minimum 1 GB). Only flash-screened videos are published. Kiosks fetch media only from the Hub (`GET /media/<sha256>` on `:7401`, device token), never the internet.
- Hub-side catalogue matching writes branch rows through a `game.matched` intent. Library entries (`library_entries`) are branch rows edited under `games.manage`. `library.changed` reaches kiosks within 2 s and the allow-list follows the library. Catalogue-linked entries have read-only name, launcher and age rating.
- Control-plane admin actions are written to the append-only `admin_audit` table (UPDATE and DELETE refused by trigger); this epic creates it and later epics reuse it. Catalogue curation UI is out of scope; an `arena-cloud catalog import` CLI seeds it.
- Optimistic concurrency: edits carry `base_rev` and fail as `STALE`. Errors are typed `AppError` codes mapped to catalogue strings; money is integer minor units. Elevated actions use `inline-pin` with both actors audited.
- Media fallback chain: video, then still cover (or launcher/exe icon for unmatched games), then solid background. High contrast skips straight to solid.
- Performance: Kiosk RAM under 80 MB; LAN action under 100 ms server and under 2 s end to end.

## UX & Interaction Patterns

- Kiosk is dark-only plus a per-device high-contrast theme, landscape, reflowing at 1366 / 1920 / 2560 / ultrawide. Keyboard: Tab/Shift+Tab, Enter, Esc, arrow keys in nav, and Ctrl+Shift+E for Extend.
- Plan picker shows only plans valid for the device's group, then "Waiting for the counter…" until the Hub approves or refuses with a typed reason. Refusals keep the player on the picker.
- Player copy is casual and gamer-friendly ("10 min left. Extend?"). Warning toasts use the warning button only for Extend. HUD toasts are solid, never translucent, top-right, with sound.
- Log out confirms only when prepaid time remains. Account sub-nav: Details, Wallet and packs, Order history, Change password.
- Reconnecting indicator shows only while the Hub is unreachable. Staff-call sends a "CALL" badge to the tile and the kiosk confirms "Staff notified."

## Cross-Story Dependencies

- Depends on Epic 1 (kiosk service, lockdown, allow-list 1.18, admin exit and high contrast 1.21, proptest generator 1.8), Epic 2 (pairing, groups and rates 2.25, branch settings 2.26, comp entitlement 2.8) and Epic 3 (session engine start/extend/end, timer warnings 3.23, grace and time-up 3.26, lock reasons 3.10, alert service 3.25, `chip-rule` 3.30, forced end 3.34, wallet, packs and credit).
- Depends on Epic 5 products, bundles, the `catalog.changed` availability feed, session-attached orders, and invoice numbering. Shop stories are tested via the comp entitlement until Epic 10 billing exists.
- Within the epic: 6.1 then 6.2 then 6.3 and 6.4, then 6.5 and 6.6 (catalogue then library then scan then match then media). The login and plan picker (6.8, 6.9) precede the shell and HUD (6.13 and later). The warnings (6.19, 6.20) feed Extend (6.21) and grace (6.22). The Shop (6.30 to 6.33) builds on the cart. 6.36 and 6.38 close out the epic over the others.
- Epic 7 reuses the Hub "Mark ready" action added here. Epic 8 owns owner dashboards and the low-balance WhatsApp message. Epic 9 (Story 9.20) handles standby and restored Hubs filling their own catalogue cache. Epic 11 adds catalogue curation UI and flash-screening, and writes to `admin_audit`. Epic 13 covers the launcher-account vault and game requests.
