---
review: accessibility (adversarial)
target: DESIGN.md + EXPERIENCE.md (Arena 360, draft 2026-10-22)
standard: WCAG 2.2 AA + practical floor for a commercial gaming-café SaaS
inputs: DESIGN.md, EXPERIENCE.md, .memlog.md, .working/color-themes-1.html (Cinema Electric + High Contrast)
date: 2026-10-23
---

# Accessibility Review: Arena 360 UX Spines

## Verdict

**FAIL. Not ready to hand to build.** The spines congratulate themselves ("all load-bearing pairs AA-verified", "state never by colour alone", "counter flow fully keyboard-operable") and then leave the mechanisms behind those claims undefined or wrong. The palette study checked text on flat swatches. It did not check text over media, non-text boundaries, focus, or the accent as link text. When I computed those, the hero copy block fails AA over a bright frame, every input boundary sits at 1.1–1.5:1, and there is no focus indicator anywhere in the token set. Autoplaying looping video has no pause control, and its only escape hatch is a high-contrast theme whose toggle "is not decided". Third-party trailers autoplay to anyone walking past with no flash screening. The HUD can't take focus yet holds the only Extend button. The Hub and Kitchen rely on sound in rooms that are loud by design.

Counts: **4 critical · 11 high · 10 medium · 6 low**.

### Computed ratios used below (WCAG 2.x relative luminance)

| Pair | Ratio | Needed | Result |
|---|---|---|---|
| accent #FF4FA8 / bg #09071A | 6.56 | 4.5 | pass (as text on flat bg) |
| accent / surface #16122C | 5.99 | 4.5 | pass |
| accent / surface-2 #201A3D | 5.43 | 4.5 | pass |
| accent / text #F5F2FF (link vs body text) | **2.74** | 3.0 (1.4.1 w/o underline) | **fail** |
| text-muted #ACA5CC / surface-2 | 7.06 | 4.5 | pass |
| line #362D5C / surface | **1.46** | 3.0 (1.4.11) | **fail** |
| line / bg | **1.60** | 3.0 | **fail** |
| surface-2 / surface (inline-pin cell, secondary button fill) | **1.10** | 3.0 | **fail** |
| hub-line #2A2350 / hub-tile #141030 | **1.27** | 3.0 | **fail** |
| hub-tile / hub-bg #0B0920 (tile edge) | **1.07** | 3.0 | **fail** |
| text #F5F2FF on ok / warning / info / danger / maintenance fills | **1.43 / 1.50 / 1.96 / 2.74 / 3.17** | 4.5 | **fail** (if badge is filled with light ink) |
| hub-tile ink on state fills (ok / maint / danger) | 11.61 / 5.23 / 6.05 | 4.5 | pass |
| Hero, white text over pure-white frame after scrim, h=35% x=30% | 4.98 | 3.0 large | marginal pass |
| same, h=35% x=51% (right edge of the ≈980px copy block) | **2.94** | 3.0 large | **fail** |
| text-muted meta line, h=15% x=30% / x=51% | 4.87 / **3.62** | 4.5 | **fail** at right |
| accent eyebrow (mock), h=45% x=3% / x=15% | **3.20 / 2.11** | 4.5 | **fail** |
| accent featured outline over bright art at h≥55% | **1.05–2.66** | 3.0 | **fail** |

Scrim model: bottom gradient 0.92→0.15 alpha over 0–55% height (holding 0.15 above), and left gradient 0.70→0 over 0–55% width, composited over #FFFFFF / #F7D84A / #FFE0C0 test frames.

---

## Findings

### Critical

**[critical]** The scrim does not protect the hero copy block it was designed for, and the spine checks contrast against the wrong worst case. (DESIGN §Colors "Scrim"; §Layout & Spacing "Kiosk"; §Components `hero`; EXPERIENCE §Accessibility Floor "Text over media")
DESIGN says text must meet AA "against the darkest-plausible frame after scrim". For light text the worst case is the **brightest** frame, so the rule as written verifies nothing. The hero copy block is ≈980px wide, which is 51% of 1920. The left gradient has fallen to almost zero by that point, and the bottom gradient is down to ~0.4 alpha at title height. Over a bright frame (snow map, menu screen, white muzzle flash), the right end of the 144px title drops to 2.94:1. The muted meta line drops to 3.62:1. The magenta eyebrow drawn in the palette mock reaches only 2.1–3.2:1. A weight-200 title makes this worse, because thin strokes perform below their nominal ratio. The palette study never tested any of these pairs. It tested text on flat swatches.
*Fix:* Change the rule to "brightest-plausible frame (#FFFFFF)". Bound the hero copy block to the scrim's effective zone, either by narrowing it to ≤40% width or by extending the left gradient to 75% width with ≥0.55 alpha at the copy's right edge. Add a local solid or radial backing under the meta line and CTAs. Ban `accent` and `text-muted` as text over media; eyebrow and meta go in `text`. Add a build-time check that renders tokens over a white frame and asserts ≥4.5:1 for body text and ≥3:1 for display text.

**[critical]** Looping autoplay video has no pause, stop or hide control (WCAG 2.2.2, Level A). (EXPERIENCE §Interaction Primitives "Video lifecycle"; §Component Patterns `hero`; §State Patterns "Media"; DESIGN §Layout "Web surfaces" — marketing "full-bleed media sections")
Video loops indefinitely on kiosk attract, login, Home hero and Pi TV, and runs alongside other content on the marketing site. Nothing lets a user pause it. The only route to stillness is the HC theme, and EXPERIENCE §Accessibility Floor admits its toggle location "is not decided". On the public marketing site this is a straight Level-A failure with legal exposure. On the kiosk, a player with vestibular or attention issues has to ask staff, or has no way out at all.
*Fix:* Add a visible, keyboard-reachable Pause/Play control to every media-backed surface (kiosk top bar or hero corner, marketing sections). Honour `prefers-reduced-motion: reduce` by falling to the still cover image, not to HC. Add a per-player "Still backgrounds" preference, persisted to the account and reachable from the login panel and the account menu. On marketing, never autoplay video longer than 5 s without controls.

**[critical]** Unvetted trailers autoplay with no photosensitive-seizure screening (WCAG 2.3.1, Level A; safety). (EXPERIENCE §Game Media Pipeline; §Interaction Primitives "Video lifecycle"; DESIGN §Brand & Style "motion is subtle (no flash)")
"No flash" is a rule for UI motion. The media pipeline pulls arbitrary game trailers from a global library and lets owners upload their own clips. Esports and shooter trailers routinely contain strobes and red flashes. These play unattended on a 1920×1080 screen, and on Pi TVs, to every passer-by, including the 14–17-year-old players the product targets. No layer anywhere screens for flashing.
*Fix:* Add a flash-analysis gate (PEAT or Harding-style, general-flash plus red-flash thresholds) to the control-plane library ingest, and reject or trim failing clips. Owner uploads either pass through the same check locally or are restricted to still images. Document in the pipeline that a clip which fails or has not been checked falls to the still cover.

**[critical]** Focus indicators are undefined, collide with existing outlines, and can be hidden by the spine's own layouts (WCAG 2.4.7, 2.4.11; 1.4.11). (DESIGN front-matter tokens, which have no focus token; §Components `game-tile-featured`; EXPERIENCE §Component Patterns `game-tile` "Select = focus outline"; §Accessibility Floor "visible focus rings"; DESIGN §Colors HC "2px white outlines on every interactive surface")
The Accessibility Floor promises "visible focus rings", but:
1. No token defines colour, width, offset or style for focus.
2. The kiosk featured tile already carries a 3px magenta outline with a 3px offset. If focus is "an outline", a focused tile and the featured tile look the same.
3. In HC, every interactive surface already has a 2px white outline, so a focus ring has nothing left to differ by.
4. A magenta ring over bright art measures 1.05–2.66:1.
5. The non-modal `tile-sheet`, the bottom settle bar and the Free-tier `ad-banner` can each cover the focused tile, a 2.4.11 failure.

Counter staff are required to work without a mouse, so this blocks a stated requirement.
*Fix:* Add `focus-ring` tokens: a double ring, 3px `text` (#F5F2FF) outside a 2px `bg` inner gap, so it holds ≥3:1 on any backing, media included. In HC, use a 4px `accent-hc` yellow ring, distinct from the always-on white outlines. Give "featured" a non-outline marker (badge or size) so it cannot be confused with focus. Require `scroll-padding` and `scroll-margin` equal to the heights of the sheet, settle bar and ad-banner so focused content is never covered.

### High

**[high]** Time warnings and grace may not reach the player at all in fullscreen-exclusive games, and sound is then the only channel. (EXPERIENCE §Component Patterns `hud-pill` "hidden in fullscreen-exclusive games (toasts still show)", `hud-toast`, `lock-screen`; Journey 3 steps 6–8)
An always-on-top Tauri window cannot reliably draw over DirectX fullscreen-exclusive swapchains. Saying "toasts still show" is a hope, not a design. If the toast fails to draw, the 10/5/1-minute warnings arrive as sound only. A deaf or hard-of-hearing player, or anyone in a headset with game audio up, gets no warning, and the grace countdown "over the still-visible game" may never render. Billing time limits are arguably essential under 2.2.1, but they still need a perceivable warning.
*Fix:* Spec a fallback chain:
- Try the overlay.
- If the overlay is not composited, force the game out of exclusive fullscreen (minimise or borderless) at the 1-minute mark and at time-up.
- Make the grace countdown pause until the lock screen has actually been displayed.

Add a visual-only cue that survives exclusive mode, such as a keyboard RGB or LED flash where supported, as optional extra. Write the expected behaviour into Journey 3's failure line.

**[high]** The HUD's actionable toast is unreachable by keyboard and vanishes on a timer (WCAG 2.1.1, 2.2.1). (EXPERIENCE §Component Patterns `hud-pill` "never takes focus", `hud-toast` "never steals focus… auto-dismisses after 8 s"; §Interaction Primitives "HUD never steals focus"; Open Question 8)
Extend and Dismiss live in a window that is forbidden to take focus. A keyboard-only player, or one whose mouse is captured by the game, cannot activate them. Eight seconds is too short to read a title, a body line and a price and then decide. Staff messages are delivered the same way and disappear as well.
*Fix:* Define a global hotkey (for example Ctrl+Shift+E) that brings up the expanded HUD with Extend focused, and a second hotkey to return focus to the game. Print the hotkey inside the toast. Keep warning toasts until dismissed, or for ≥20 s. Log every toast and staff message to account-menu → Messages. Make the dismiss timer configurable per branch with a floor of 20 s.

**[high]** Hub and Kitchen alerts rely on sound in rooms that are loud by design. (EXPERIENCE §Interaction Primitives "Sound"; §Component Patterns `kitchen-ticket` "chime on arrival"; Journey 4 step 4; State "Printer offline")
"Hub plays a sound on alerts" never defines the alerts or what each one looks like. The kitchen ticket's only arrival cue is the chime plus position. Nothing stays visible until a person has seen it: no unread state, no acknowledge, no escalation. Android tablets and phones have vibration, and it isn't used. There is no volume, mute or test control.
*Fix:* List every alert. For each, specify a persistent visual state (header badge with count, tile pulse limited to ≤3 flashes/s, then steady) that holds until acknowledged. Kitchen: a new ticket gets a "NEW" tag and a thick `warning` edge until it is tapped or moved, plus a full-width header bar showing "2 new". Add vibration on Android, a per-device volume setting and a "test sound" button. Sound stays a secondary channel.

**[high]** Non-text contrast fails for inputs, PIN cells, secondary buttons and tile boundaries (WCAG 1.4.11). (DESIGN §Components `login-panel`, `button-secondary`, `inline-pin`, `floor-tile`, `card`; §Colors "line", "Hub tones")
`line` against `surface` is 1.46:1. `surface-2` PIN cells against a `surface` sheet are 1.10:1. `hub-line` against `hub-tile` is 1.27:1, and the tile against `hub-bg` is 1.07:1. The kiosk username and password fields and the Hub's single phone-lookup field have no boundary token at all, and would inherit `line`. A form field you cannot see is a 1.4.11 failure. The spine's claim that "every load-bearing pair is AA-verified" holds only because non-text pairs were never checked.
*Fix:* Add `field-border` at ≥3:1 against its container (for example #7A70AD on `surface`; verify). Give PIN cells a ≥3:1 outline and a filled/empty indicator. Keep hairlines decorative, but every input, checkbox, toggle and focusable boundary must use the ≥3:1 token. Add the non-text pairs to the palette study table.

**[high]** The magenta accent is used where its contrast fails: links next to body text and outlines over art (WCAG 1.4.1, 1.4.11). (DESIGN §Colors "Hot Magenta"; §Components `login-panel` "sign-up link", `game-tile-featured`; EXPERIENCE `filter-rail` "clear-filters link")
Accent as text on flat surfaces passes at 5.4–6.6:1. Against body `text` it is only 2.74:1, so an accent-coloured link inside text cannot be told apart by colour alone. The featured-tile outline over bright cover or video drops to 1.05–2.66:1. DESIGN says magenta is for "primary action only" but never forbids it as link colour, and the palette mock already puts it on the eyebrow.
*Fix:* Links are always underlined (or `text` + underline). Accent is never used as text over media. The featured outline gets an inner 2px `bg` keyline so it reads ≥3:1 on any art.

**[high]** The keyboard model exists only as a promise. Key maps, grid navigation, sheet focus management and the shell exit are all undefined (WCAG 2.1.1, 2.1.2, 2.1.4, 2.4.3). (EXPERIENCE §Interaction Primitives "Keyboard" (two [NOTE FOR UX]s); §Accessibility Floor; §Component Patterns `pill-nav`, `tile-sheet`, `inline-pin`, `floor-tile` "Updates ≤2 s")
"Counter flow is completable without a mouse" is an acceptance claim with no supporting design. Open questions:
- How do staff move through 24+ tiles: Tab, or arrow keys inside a grid?
- Where does focus go when the non-modal sheet opens and closes?
- What happens to a focused tile when the live floor re-renders every ≤2 s?
- Is "Guest" reachable from the lookup field?
- Are kiosk shortcut keys single characters (a 2.1.4 failure while typing in search)?
- How does a keyboard player get back to the shell from a launched game, given the kiosk replaces the Windows shell?
*Fix:* Ship a key map table in EXPERIENCE now:
- Floor grid uses roving tabindex with arrow keys, and Enter opens the sheet.
- The sheet takes focus on open and returns it to the originating tile on close.
- Esc closes.
- Live updates never move focus or re-order a focused tile.
- Shortcuts use modifiers (or are disabled while a text field has focus) and are listed in a "?" overlay.
- Kiosk and HUD get a documented return-to-shell hotkey.

**[high]** Money actions are one tap with no confirm or undo (WCAG 3.3.4, plus practical error prevention). (EXPERIENCE §Interaction Primitives "Hub speed rules"; `tile-sheet` "each one tap"; Journey 2 step 7, Journey 7 step 8 "one-tap collect"; `kitchen-ticket` "tap advances one stage")
End, Lock, Transfer and Settle are one tap each, under a "<10 s" mandate, on a phone grid where 44px targets sit next to each other. A mis-tap on End triggers billing and cleanup on someone else's PC. Settle is a financial transaction, and 3.3.4 requires it to be reversible, checked or confirmed. Kitchen stage advances have no undo.
*Fix:* Use a review line before settle (amount + method, confirm on a second tap in the same place, which keeps the 2-tap rule). Add a 5–10 s Undo snackbar for End, Lock, Transfer, the Kitchen advance and Collect. Put destructive actions (End, Lock) apart from Extend and Order, with ≥8px spacing and a distinct style.

**[high]** HC theme coverage and discoverability are undecided, and Windows forced-colors is ignored. (EXPERIENCE §Accessibility Floor [NOTE FOR UX]; DESIGN §Colors "High-contrast theme"; §Brand & Style "dark is the only theme")
HC is the only route to no-video, no-motion and higher contrast, yet nobody can say where a player turns it on. If the toggle sits in branch settings, a low-vision player has to ask staff in a busy café. HC is listed for Kiosk, Hub, Kitchen and Owner only, so Onboarding, the Partner portal and Marketing get nothing. There is no `prefers-contrast` or `forced-colors` handling. WebView2 honours Windows High Contrast, which will strip background-drawn state bars and status dots.
*Fix:* Put an HC toggle on the kiosk login panel and in the account menu, saved per player account and also settable as a per-device default. On staff surfaces, make it per user. Extend HC to every web surface. Map HC automatically from `prefers-contrast: more`. Add a `forced-colors: active` spec: state bars become borders, dots become text or icons, and system colours are used for focus.

**[high]** Web surfaces have no screen-reader or semantics spec. (EXPERIENCE §Foundation table (Owner web, Onboarding, Partner portal, Marketing); §State Patterns "Data age / pending command"; Journey 1 step 3 (pairing countdown); Journey 6; Journey 9)
None of the following is specified:
- landmarks
- heading structure
- skip link
- labels
- `aria-live` behaviour for "Waiting for branch" → Applied / Rejected / Expired (a 4.1.3 status message)
- data-age announcements
- the self-advancing wizard step (Journey 1 step 5 changes the page under the user)
- Reports chart text alternatives
- table semantics for payouts
- pairing-code countdown announcements (a per-second live region would flood the screen reader)

The Owner app is the paying customer's surface, and owners can be blind or low-vision too.
*Fix:* Add a "Web semantics" subsection:
- One `h1` per view, `nav` and `main` landmarks, and a skip link.
- Every control has a programmatic label.
- Status-badge changes go through a polite live region.
- Auto-advance moves focus to the new step's heading and announces it.
- Countdowns announce at milestones (5 min, 1 min), not every tick.
- Charts get a data-table alternative.
- Owner app native vs PWA (Open Question 6) is resolved with a stated screen-reader target (TalkBack / VoiceOver / NVDA).

**[high]** The kiosk password login is a cognitive-function test with lockout and no alternative (WCAG 3.3.8, Accessible Authentication (Minimum), AA). (EXPERIENCE §Component Patterns `login-panel`; Journey 3 step 2 (override to username + password); Open Question 5 (QR and staff-started login may not survive))
In a shell-replacement kiosk there is no password manager, and paste is probably blocked. Recalling and transcribing a password is exactly the test 3.3.8 regulates. Add a 3-strike, 60 s lockout, and a 14-year-old with dyslexia gets locked out in front of friends. Manager PIN entry has the same problem on the staff side, though that is more defensible.
*Fix:* Keep at least one non-memory path as a requirement, not an open question: a QR from the player's phone, or staff-started login from the Hub. Add a show-password toggle and allow paste. Error copy names which step failed without revealing which field ("Username or password didn't match").

**[high]** Running the same grid on a phone is unworkable as specified: the rail eats the width, a nested badge target, and an ad SDK beside the settle bar (WCAG 2.5.8, practical floor). (EXPERIENCE §Responsive "Hub — Android phone" [NOTE FOR UX]; DESIGN §Layout "Hub… same grid"; `floor-tile` minHeight 128px; `ad-banner`; §Ads "optional rewarded ads")
On a 360px phone, an 88px rail plus 2×24px gutters leaves ≈224px, which is one column. Twenty-four PCs then need about 3,300px of scrolling, so the floor cannot be read "at a glance" during a rush. The ready-order badge "sits on the tile". If it can be tapped, it is a target nested inside a target. Rewarded and banner ad SDKs bring their own close buttons (often 24–32px) next to a 44px settle bar, so mis-taps will open ads in the middle of a sale. The rail collapse and POS layout are explicitly undecided.
*Fix:* On phone, collapse the rail to a bottom bar (5 items + More) and drop gutters to 12px, giving 2 columns of ~160px. Add a compact tile variant (≥88px tall, still 44px+ hit area) and a "needs attention" filter at the top. The ready badge is not a separate target; it opens the order section inside the sheet. The ad banner sits behind ≥16px spacing and never shares an edge with settle or cart. Rewarded ads only on an explicit action outside any sale flow.

### Medium

**[medium]** The status-badge ink colour is undefined, and an 11px label is the only non-colour state cue. (DESIGN §Components `status-badge` "small uppercase tag in a state or semantic colour", `hub-micro` 11px; EXPERIENCE §State Patterns "Floor tile states")
If badges are filled in the state colour, light ink fails: 1.43–3.17:1. If they are coloured text, they pass, but then an 11px uppercase word is all a colour-blind staff member has from arm's length. In-session blue #7FB2FF, reserved lavender #C8A2FF and maintenance grey #8A85A8 run together under protanopia and on a dim, cheap tablet.
*Fix:* State the badge ink explicitly: `hub-tile` ink on a state fill (≥5.2:1). Add a distinct glyph per state (● free, ▶ in session, ◷ reserved, 🍽 order, 🔧 maintenance, ⚠ offline; icons, not emoji, in implementation). Raise state tags to 12–13px on phone.

**[medium]** "Order pending" is modelled both as a state and as an overlay, and green carries two meanings. (EXPERIENCE §State Patterns ("Order pending" row says "Order badge over session info"); DESIGN `kitchen-ticket` ready-accent = `state-free`)
A PC can be in session and have a pending order at once. Which colour does the 3px bar show, and which label? The rule "one state = one colour + label" breaks. Kitchen "Ready" reuses state-free green, so green means "PC free" on the Hub and "food ready" in the kitchen. That is a cognitive trap for staff who work both screens.
*Fix:* Make order-pending an overlay badge with its own label ("Order · Ready"), separate from session state. Give kitchen-ready its own token, or use `ok` plus a "READY" text tag, and say so explicitly.

**[medium]** The time-chip warning is a colour-only change on a 9px dot. (DESIGN `time-chip` indicator; EXPERIENCE `time-chip` "dot turns warning at 10 min"; Journey 3 step 6 "chip turns amber")
Green #5FE6A0 and amber #FFBE3D are hard to tell apart for deuteranopes. The digits show the time, but the *warning* (≤10 min) is signalled by colour alone.
*Fix:* At ≤10 min, also change shape and text: the dot becomes a ⚠ glyph, the chip gains a `warning` 2px border, and a "low" suffix or an icon appears.

**[medium]** Thin display weights are used over moving media and on countdowns. (DESIGN `kiosk-display` 144/200, `kiosk-headline` 48/300, `web-display` 40/300; `lock-screen` countdown; §Typography "Light weights only at display size")
The grace countdown is a weight-200 numeral over a still-visible game behind a scrim, and it is the one number a player must read under stress. Contrast ratios assume solid strokes; hairline Sora at 200 aliases on 1080p panels.
*Fix:* Countdown and any text over live game ≥ weight 400. Display titles over video ≥300 with the corrected scrim. Use 200 only on solid `bg`.

**[medium]** Hub idle lock and other timeouts lack warning or adjustment (WCAG 2.2.1). (EXPERIENCE §State Patterns "Idle lock: after 5 min idle"; "Login lock"; Journey 1 step 3 (15-min pairing code))
A 5-minute lock with no warning and no setting can land in the middle of a slow settle, or while a manager reads a variance breakdown. It also isn't defined what happens to a half-typed inline PIN or reason. The login lockout is fine in principle but has no copy. The pairing code is fine because of "Generate a new code".
*Fix:* Make idle lock configurable per branch (2–30 min). Show a 20 s "Locking in 20 s — tap to stay" banner. Preserve in-progress input across the lock. Never lock while a sheet has unsaved payment input.

**[medium]** Error identification is asserted but not designed (WCAG 3.3.1, 3.3.3, 4.1.3). (EXPERIENCE §State Patterns "Error (save/apply)"; `login-panel`; Journey 1 failure line; Onboarding CSV dry-run)
"Inline, next to the control, plain cause + retry" is good policy, but it comes with no programmatic association, no icon, no copy for kiosk login failure, sign-up validation, CSV dry-run row errors or OTP failure, and no focus behaviour on submit.
*Fix:* On submit failure, move focus to the first invalid field (or to an error summary on web). Link messages via `aria-describedby`. Use a ⚠ icon + `danger` text + an outline on the field, so the error is not marked by colour alone. Write the exact copy for the top ten errors in the Voice section.

**[medium]** Text resize and zoom are ignored on the kiosk and Hub (WCAG 1.4.4, 1.4.10, 1.4.12). (EXPERIENCE §Responsive "Kiosk fixed 1920×1080" + [NOTE FOR UX] other resolutions; DESIGN §Typography (px-only ramps); `floor-tile` minHeight 128px)
The kiosk is pixel-fixed with no text-size option. WebView2 zoom is presumably disabled in shell mode, and 1366×768 or 1600×900 café monitors are undecided. The Hub must survive Windows 125–150% scaling and Android font scale 1.3×. Px-only tokens and fixed tile heights will clip. Web surfaces have no 320 CSS px reflow statement.
*Fix:* Express type ramps in rem and honour OS scaling. Add a kiosk "Larger text" preference (×1.25), saved per account. Make tile minHeight a minimum, not a fixed height. Add a reflow-at-320px and text-spacing acceptance line for all web surfaces.

**[medium]** Reduced motion is tied to HC instead of standing alone. (EXPERIENCE §Interaction Primitives "Motion… reduced (cut to instant) in high-contrast"; DESIGN Do's "none in high-contrast")
A user who wants no motion is forced into a black-and-yellow theme, and a user who wants HC is forced into no motion. `prefers-reduced-motion` is never mentioned. HUD toast slide-ins and tile-state transitions on a live floor are the worst cases.
*Fix:* Treat motion and contrast as separate preferences. Honour `prefers-reduced-motion` everywhere (video → still, transitions → instant/opacity only).

**[medium]** Cognitive load for young, high-turnover counter staff is unaddressed. (EXPERIENCE §Information Architecture Hub rail (9 items, 11px labels); §Interaction Primitives keyboard [NOTE]; Journey 8 (promote under stress, failure undefined); §Voice "staff")
Undiscoverable shortcuts, a live floor that keeps changing behind a non-modal sheet, terms like "Transfer", "Promote to main" and "Variance", and a highest-stakes recovery flow (Journey 8) whose failure states are undefined. This is a lot to ask of a 19-year-old on day two during a Saturday rush.
*Fix:* Add a first-run tour plus a "?" help overlay listing shortcuts. Give each sheet action a one-line description on long-press or hover. Turn Journey 8 into a three-step guided wizard with explicit failure copy. Write a staff glossary and hold copy to it.

**[medium]** Game-tile names over cover art have no scrim spec. (DESIGN `game-tile` "name bottom-left in kiosk-tile-name"; §Typography "kiosk-tile-name 20/800 over cover art (with scrim)"; `scrim` defined only at hero scale)
"With scrim" is stated in Typography, but the `game-tile` component has no overlay token, and a 20px label over arbitrary cover art (often white-heavy key art) is unprotected.
*Fix:* Add a `tile-scrim` token (bottom 45% of the tile, #0A0816 at 0.85→0) and verify white text over a white cover.

### Low

**[low]** The out-of-stock "greyed" treatment is undefined. (DESIGN `shop-item-card`; EXPERIENCE `shop-item-card`)
Disabled controls are exempt from contrast rules, but the "out" tag carries the information and must stay readable.
*Fix:* Dim only the photo. Keep the name and the "Out of stock" tag at ≥4.5:1.

**[low]** The OTP and pairing-code entry details are missing. (EXPERIENCE Journey 1 steps 1, 3, 6)
*Fix:* Use `autocomplete="one-time-code"` and allow paste. Pairing alphabets exclude ambiguous characters (0/O, 1/I/L). Show codes in groups ("ABCD-EFGH") and give them an accessible name.

**[low]** Marketing media has no captions, transcript or decorative-video semantics. (DESIGN §Layout "Web surfaces"; EXPERIENCE §Foundation Marketing)
*Fix:* Any marketing video with speech gets captions and a transcript. Muted ambient video gets `aria-hidden` / no controls in the accessibility tree, besides the pause button.

**[low]** Kitchen elapsed time has no overdue signal. (EXPERIENCE `kitchen-ticket`; Journey 4)
*Fix:* Past a branch threshold, show an "Overdue" text tag plus a `danger` edge, not just a different colour.

**[low]** In HC, the accent, warning and pricing-chip yellows overlap. (DESIGN §Colors HC: accent #FFE14D, warning #FFB000, chip-rule bg #FFE14D)
Primary CTA, Extend and the pricing chip are all yellow in HC, which contradicts "one accent, never a state". DESIGN also specifies a 3px state bar, while the HC mock uses 8px.
*Fix:* In HC, warning buttons become black with a 3px #FFB000 border and amber text. The state bar is 8px in HC; write that into DESIGN.

**[low]** The palette study overstates its coverage. (DESIGN §Colors "Every text/background pair below is verified WCAG AA"; EXPERIENCE §Accessibility Floor "all load-bearing pairs AA-verified")
The table covers text on `bg` and `hub-tile` only: nothing on `surface-2`, nothing non-text, nothing on media, no link-vs-text check. The claim will be copied into stories as "done".
*Fix:* Reword it as "text-on-flat pairs verified". Add the computed table from this review and the missing non-text and media rows to the palette study, and make them a CI token test.
