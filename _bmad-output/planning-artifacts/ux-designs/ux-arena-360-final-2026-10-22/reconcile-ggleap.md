# Reconcile — ggLeap kiosk imports vs spines

Inputs: `imports/ggleap-01..07*.png`, `.memlog.md`, `DESIGN.md`, `EXPERIENCE.md`. Spines not edited. Where a screenshot conflicts with the spines, the spines win.

## 1. Kept patterns: are they in the spines?

| Kept pattern | Status | Where |
|---|---|---|
| Art-backed login panel | Captured (solid panel over video, no blur) | DESIGN Components `login-panel`; EXPERIENCE Component Patterns `login-panel`; Inspiration & Anti-patterns |
| Hero Home + featured row + Play CTA | Captured | DESIGN `hero`, `game-tile-featured`; EXPERIENCE IA (Kiosk row), `hero`, `game-tile` |
| Top pill nav | Captured (Home · Games · Apps · Shop) | DESIGN `pill-nav`; EXPERIENCE `pill-nav`, `pill-nav-item-active` |
| Always-visible time chip | Captured | DESIGN `time-chip`; EXPERIENCE `time-chip` |
| Library/shop grid + left filter rail + search | Captured | DESIGN `filter-rail`, `shop-item-card`; EXPERIENCE `filter-rail` (Games, Apps, Shop) |
| Account sub-nav (ggleap-07) | **Partly missing.** EXPERIENCE IA line 48 cites ggleap-07 as a structure reference, but the Kiosk IA row, account-menu and component lists define no kiosk Account page or sub-nav. | Needs a decision |

## 2. Ideas dropped without being rejected (recommendation: raise each with the user)

- **Forgot password link (01):** username + password login has no recovery path. Raise with the user; "Ask at counter" could be enough.
- **Show-password eye toggle (01):** not specified. Raise with the user.
- **"Create account" link (01):** the sign-up link is in DESIGN, and its fields are already Open Question 5. No action.
- **PC name + version footer (01, 03):** in DESIGN `login-panel` but missing from EXPERIENCE. Raise with the user; consider adding the same footer to the account menu.
- **Help "?" button (02–07):** not specified. Raise with the user: should it be a "call staff" button?
- **Volume control in the top bar (02–07):** not specified. Raise with the user. Kiosk players need a system-volume control.
- **Notification bell with unread dot (02–07):** the spines put "Messages" inside the account menu, but nothing signals an unread message. Raise with the user.
- **Username in the top-right chip (02):** the time chip shows time only. Raise with the user.
- **"All games" end tile in the featured row (02):** not specified. Raise with the user.
- **Secondary hero CTA (02):** DESIGN mentions one, but nothing defines what it does. Raise with the user.
- **"Access your own games" launcher cards (04):** Steam/Epic shortcuts are not specified. Raise with the user.
- **"Most played here" Top-N carousel (04):** this is not gamification, so it was not covered by that rejection. Raise with the user.
- **"Discover N games" count heading (04):** not specified. Raise with the user.
- **License toggles (04):** free-to-play / borrow filters are not specified. Raise with the user.
- **"View more" on the tag list (04):** collapse behaviour is not specified. Raise with the user.
- **"Most popular" badge on shop cards (06):** not specified. Raise with the user.
- **Shop balance card (06):** not shown on Shop. Raise with the user.
- **"Add money" CTA (06, 07):** the spines say "Top up at counter", which conflicts. The spines win, but raise it with the user.
- **Cart (06):** an "Add to cart" button exists, but there is no cart or checkout surface. Raise with the user.
- **Account page (07):** personal details, orders history and change password/security are not specified. Raise with the user.
- **Membership block (07):** this is not a PRD feature. Raise with the user as low priority.
- **PC Settings in the account menu (03):** not specified. Raise with the user.
- **Pause/minimise control at the top right (01–07):** not specified. Raise with the user.

## 3. Conflicts (spines win)

- **Social login and language picker (01):** rejected (EXPERIENCE Inspiration & Anti-patterns; DESIGN `login-panel`).
- **Email-or-username login (01):** the spines say username + password.
- **Frosted translucent panels (01–03):** banned; the spines use solid panels and no live blur.
- **Arcade and Prize Vault tabs, coins, quests, challenges, social feed (02, 03):** banned as gamification.
- **Shop categories GamePass / merch / event tickets (06):** the kiosk Shop is food & drink only; time packs live in the account menu and HUD.
- **Buy/Redeem GamePass in the account menu (03):** the spines use "Extend". Redeem code is unaddressed, so treat it as a dropped idea.
- **Blue accent on primary actions (01–07):** the spines use one magenta accent.
- **"Lock PC" in the account menu (03):** captured as "Lock". No conflict.
