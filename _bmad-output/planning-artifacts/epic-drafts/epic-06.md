## Epic 6: Player experience at the kiosk

Players log in or sign up, see a HUD with warnings, extend through grace, launch games from the library, order from the seat, and keep their session when the Hub drops.

> Draft notes: this epic builds on the Epic 1 kiosk service, lockdown, allow-list (Story 1.17), admin exit and per-device HC (Story 1.19); the Epic 2 pairing, groups, group rates (Story 2.24) and branch operating settings (fulfilment, kiosk default volume, opening hours; Story 2.25); and the Epic 3 session engine: `session.start` / `session.extend` / `session.end` (Stories 3.9, 3.24, 3.27, 3.28), the timer engine and its 10/5/1-min warnings (Story 3.23), grace and time-up mode (Story 3.26), the minimal session screen and lock reasons (Story 3.10), the alert service (Story 3.25), `chip-rule` (Story 3.30), forced end (Story 3.34), player profiles, wallet, packs and credit (Stories 3.6, 3.16, 3.18, 3.22). From the Epic 5 draft it uses, by title: "Products with categories, barcodes, images and the kitchen flag" (the `catalog.changed` availability feed), "Bundles priced on their own", "Sell at the counter — a walk-up sale settles and takes stock", "Attach an order to a running session so it settles with the bill", "Take stock at served, mark items served, and cancel unserved lines into waste" and "Invoice numbers in an epoch-segmented series that never repeat". **Player actor:** kiosk-originated intents carry `actor{kind: player, id}` (or `{kind: device}` for guests and system events) plus the device id, travel through the kiosk service over the paired-device connection, and are checked against a fixed self-service permission set in `arena-proto` (`session.self_start`, `session.self_extend`, `session.self_end`, `order.self_place`, `player.self_update`); a player can only act on their own session and profile. **Login lockout** uses 5 failures → 60 s (decision), matching staff sign-in (FR120 updated; UX-DR28's "3 failures" is superseded); the catalogue line "Too many tries. Try again in 0:60." is unchanged. **Hub unreachable:** every story that adds a Hub-dependent player action (login, sign-up, plan request, Extend, Place order, I'm done, Log out, Call staff, change password, PC-settings save) disables it with "Counter is reconnecting — try again shortly."; Story 6.36 adds the shell-wide behaviour and the proof. **Out of this epic:** catalogue curation and flash-screening UI (Epic 11 admin app — this epic adds the `gz_control` catalogue, its read API and a seed import tool), KOT, the kitchen display and the READY tile badge (Epic 7 — this epic adds a Hub "Mark ready" action that the kitchen display will reuse), owner dashboards reading play hours and the low-balance WhatsApp message (Epic 8), the launcher-account vault and game requests (Epic 13), Pi/QR (Epic 12), and kiosk ads (none in Phase 1). Every story adds its new intents to the Story 1.6 `proptest` generator.

### Story 6.1: The control plane publishes the global game catalogue to Hubs

As a café owner,
I want my Hub to receive Arena 360's curated list of games with their covers and videos,
So that my PCs show proper game art without me uploading anything.

**Covers:** FR135 (source), AD-17, AD-9, AD-36 (shared catalogue media prefix), AD-40, AD-23, NFR14

**Acceptance Criteria:**

**Given** `gz_control`
**When** the migration runs
**Then** new tables `catalog_games` (id ULID, name, launcher, category, age rating, published flag, `rev`, `updated_at`, soft delete), `catalog_exe_signatures` (game id, executable file name, product name, publisher, optional SHA-256) and `catalog_media` (game id, kind cover|video, content hash, bytes, `flash_screened` flag) exist, and a `catalog_version` counter increments on every published change

**Given** a paired Hub with a valid device token and licence
**When** it calls `GET /v1/catalog/manifest?since=<version>`
**Then** it receives only published games changed since that version (including tombstones for unpublished or deleted ones), each with exe signatures and media refs, plus the current `catalog_version`, with an ETag; an unchanged version returns 304

**Given** a video ref
**When** the manifest is built
**Then** it is included only if `flash_screened = true`; unscreened videos never reach a Hub, while the game's cover still does

**Given** catalogue media
**When** it is stored
**Then** it lives under the shared R2 prefix `catalog/<sha256>.<ext>` behind the CDN, is immutable (content-addressed), and the manifest gives each item's CDN URL, hash and size

**Given** dev, staging and launch before the Epic 11 admin app exists
**When** an operator runs `arena-cloud catalog import <file>` with a JSON/CSV list and media files
**Then** games, signatures and media are upserted idempotently by name + launcher, media are uploaded once by hash, the version increments, and the import is recorded in the admin audit with `actor.kind = admin`

**Given** an unauthenticated caller, a revoked device token, or more requests than the per-tenant limit
**When** the manifest endpoint is called
**Then** it returns 401, 403 or `RATE_LIMITED` respectively and no catalogue data

### Story 6.2: The Hub caches the catalogue and media and serves them on the LAN

As a café owner,
I want the Hub to keep a local copy of game art and serve it to every PC,
So that kiosks never reach the internet for media and art still shows when the WAN is down.

**Covers:** FR135 (Hub cache), AD-17, AD-11 (standby keeps its own cache), AD-42, NFR1, NFR11, UX-DR99

**Acceptance Criteria:**

**Given** the Hub service with the WAN up
**When** it starts, and every 6 hours after
**Then** it pulls the manifest from its last `catalog_version` and stores games, signatures and media refs in a separate reference database outside branch state and the change log, never through `apply()`

**Given** manifest entries
**When** the cache fills
**Then** covers for every catalogue game are downloaded first and always kept; videos are downloaded best-effort afterwards; each file is verified against its SHA-256 and a mismatch is discarded and retried on the next pull

**Given** the media cap (Settings → System → "Game media cache", default 10 GB, minimum 1 GB)
**When** stored media would exceed it
**Then** videos are evicted least-recently-served first; covers are never evicted; the screen shows used and cap sizes

**Given** a paired kiosk
**When** it requests `GET /media/<sha256>` on `:7401` with its device token
**Then** the Hub serves the file with immutable cache headers; an unknown or not-yet-downloaded hash returns 404 quickly; an unauthenticated request is refused

**Given** the WAN is down or the manifest call fails
**When** kiosks request media
**Then** everything already cached is served as normal, no player-visible error appears, and the next successful pull resumes where it stopped

**Given** a standby Hub, or a Hub promoted or restored
**When** it runs
**Then** it keeps or refetches its own reference cache from the manifest; nothing in the cache is replicated through the change log or included in branch batches and snapshots

### Story 6.3: The branch game and app library at the Hub, applied to every kiosk

As a branch manager,
I want one list of the games and apps on our PCs that I edit once at the Hub,
So that every PC shows the same library and can launch it.

**Covers:** FR132, AD-17 (`game.matched` branch rows), AD-39, FR117 (allow-list follows the library), UX-DR18, UX-DR66, UX-DR88

**Acceptance Criteria:**

**Given** Settings → Games on the Hub
**When** a Manager adds an entry
**Then** `library.upsert` (permission `games.manage`) stores kind (game or app), name, launcher (Steam · Epic · Riot · Battle.net · None), executable path, optional launch URI or arguments, category, tags, age rating and enabled flag in new `library_entries` table, with optional link to a global catalogue id

**Given** an entry linked to a global catalogue game
**When** it is shown or edited
**Then** name, launcher and age rating come from the catalogue and are read-only; category and tags stay editable for the branch

**Given** a saved, enabled entry
**When** it commits
**Then** a `library.changed` event reaches every connected kiosk within 2 s, and the entry's executable (and its launcher's executables) are added to the branch allow-list from Story 1.17 by signed path, so a library launch is never killed as unknown

**Given** an entry disabled or soft-deleted
**When** it commits
**Then** it disappears from every kiosk within 2 s and its executable leaves the allow-list unless another entry still needs it

**Given** a duplicate (same kind, launcher and executable path), an empty name, or a missing executable for a non-URI entry
**When** saved
**Then** a field error shows and nothing is written; edits carry `base_rev` and conflict as `STALE`

**Given** a Counter
**When** they edit the library
**Then** `inline-pin` opens for manager elevation and the change is audited with both actors

**Given** a kiosk that cold-starts with the Hub unreachable
**When** the shell needs the library
**Then** it uses the last library it received

### Story 6.4: Kiosks scan for installed games and launchers and report them to the Hub

As a branch manager,
I want each PC to find its installed games and launchers by itself,
So that I don't have to type install paths for every game on every PC.

**Covers:** FR133 (kiosk auto-scan), AD-17, AD-14, AD-31

**Acceptance Criteria:**

**Given** a paired kiosk
**When** the kiosk service first connects after pairing, and daily at 05:00 branch time when no session is running (or at the next moment with no session)
**Then** it scans Steam library folders and app manifests, Epic manifests, the Riot client install list and common folders (Program Files, Program Files (x86), `C:\Games` and other fixed drives' `Games` folders)

**Given** a completed scan
**When** the service reports it
**Then** it sends `kiosk.scan_reported` with each found executable's path, file name, product name, publisher and launcher, plus which of Steam, Epic and Riot are installed and their paths

**Given** a scan report
**When** the Hub receives it
**Then** it stores it as that device's latest scan in a new `kiosk_scans` table (device, scanned at, entries, installed launchers), replacing the previous one, and Settings → Games shows each PC's last scan time

**Given** a scan running
**When** a session starts on the PC
**Then** the scan pauses and resumes after the session, so it never slows a player's game

**Given** the Hub is unreachable when a scan completes
**When** the report is sent
**Then** it is queued with its idempotency key and applied once on reconnect

**Given** a folder the service cannot read
**When** the scan reaches it
**Then** it is skipped, logged and the rest of the scan continues

### Story 6.5: The Hub matches scanned games to the catalogue and the manager approves them in bulk

As a branch manager,
I want found games matched to Arena 360's catalogue and approved in one go,
So that the library fills with proper names and art in minutes.

**Covers:** FR133 (matching, bulk approval), AD-17 (`game.matched`), FR132, UX-DR88

**Acceptance Criteria:**

**Given** a stored scan report
**When** the Hub processes it
**Then** it matches each executable against the cached catalogue signatures (file name + product name/publisher, or SHA-256 when given), records each match with a `game.matched` intent (global id, device, path), and keeps unmatched executables as proposals named by product name or file name

**Given** Settings → Games → "Found on PCs"
**When** a Manager opens it
**Then** it lists proposals not yet in the library with matched name and cover (or the file name for unmatched ones), launcher, category from the catalogue, and the number of PCs where each was found; already-approved or dismissed items are not listed again

**Given** several proposals selected
**When** the Manager presses Approve
**Then** one `library.upsert` per proposal creates library entries linked to their catalogue ids, and per-device install paths are stored in a new `library_installs` table (entry, device, path), so the same game at different paths on different PCs works

**Given** a library entry
**When** a kiosk shows its library
**Then** the entry appears only on PCs where an install path is known for it, or where its configured executable path exists

**Given** a later scan on a PC that finds an already-approved game at a new path
**When** it is processed
**Then** the PC's install path is updated without a new proposal

**Given** a Manager presses Dismiss on proposals
**When** later scans report the same executables
**Then** they are not proposed again unless the Manager clears the dismissed list

**Given** a Counter
**When** they approve or dismiss proposals
**Then** `inline-pin` opens for manager elevation

### Story 6.6: Game media on the kiosk falls back from video to cover to solid

As a player,
I want game art and trailers to load cleanly and never show a broken player,
So that the screens look polished and nothing flashes or blares at me.

**Covers:** FR135 (fallback chain), NFR26, UX-DR15, UX-DR27, UX-DR99, UX-DR108, UX-DR4 (HC skips media)

**Acceptance Criteria:**

**Given** the shared UI package
**When** a `game-media` component renders for a library entry
**Then** it plays the entry's curated, flash-screened video from the Hub if one exists, else shows the cached cover, else (for unmatched entries) the launcher or executable icon extracted locally by the kiosk, else solid `bg`; any load or decode error falls straight to the next step with no visible error

**Given** a video plays
**When** it renders
**Then** it is muted with no audio track ever played, loops, uses only `:7401` media URLs (never an internet URL), and only one video plays on the screen at a time

**Given** media behind copy
**When** it renders
**Then** `scrim` (or `tile-scrim` on tiles) is always drawn with copy inside its copy zone, and no `backdrop-filter` or live blur is used (a lint fails the build on `backdrop-filter` in any branch surface)

**Given** a playing video
**When** the `video-control` (44px round button, `bg` fill, `border-strong` ring, icon showing the next action) is activated by click, Enter or Space
**Then** the video pauses on its current frame for the rest of that view and the button switches to Play; it is always visible while video plays and reachable by Tab

**Given** the component receives the shell's "game running" signal
**When** the signal is on
**Then** the video is unloaded entirely (not just paused) and only the cover shows until the signal clears

**Given** the HC theme on the device
**When** media would render
**Then** it skips straight to solid `bg` with no scrim, video or `video-control`

**Given** media files previously fetched
**When** they are requested again
**Then** they come from the WebView cache by content hash, so the attract and Home screens repaint without refetching

### Story 6.7: The attract screen shows branding, rates and the active rule, with no ads

As a player walking up to a free PC,
I want to see what the café charges and what's on right now,
So that I know the price before I log in.

**Covers:** FR122, FR63 (kiosk attract), UX-DR26, UX-DR76 (`chip-rule`), UX-DR86 (flow), UX-DR99 (cold load), UX-DR104 (no kiosk ads)

**Acceptance Criteria:**

**Given** Settings → Branch → Logo on the Hub
**When** a Manager uploads a PNG or SVG
**Then** it is resized to at most 512 px and 200 KB, stored by `settings.set`, and pushed to kiosks; with no logo set, `logo-slot` shows the Arena 360 mark

**Given** a kiosk with no session
**When** it is idle
**Then** it shows the attract screen in place of the Epic 1 locked placeholder: `logo-slot` top-left, branch name, the hourly rate of this PC's group, the active `chip-rule` when a rule applies to the group, and full-bleed `game-media` cycling the curated videos of the branch library's games (each up to 30 s), with no ad slot anywhere

**Given** a rule starts, ends or changes, or the group rate changes
**When** the Hub pushes the change
**Then** the attract rates and chip update within 1 min without a reload

**Given** the kiosk cold-starts
**When** the attract screen first paints
**Then** branding, rates and rule come from the kiosk's last cached copy before any network call, and media streams in afterwards

**Given** the attract screen
**When** the player presses any key, clicks or taps
**Then** the login panel opens over the same media; Esc on the login panel, or 60 s without input, returns to attract

**Given** a PC in maintenance, unassigned or with no group rate
**When** attract renders
**Then** it shows "Ask at the counter" in place of rates and the login panel is not offered

### Story 6.8: A player logs in at the kiosk with username and password

As a player,
I want to log in with my username and password in a couple of seconds,
So that I can start playing without queueing at the counter.

**Covers:** FR120, NFR6 (login < 3 s), NFR23 (kiosk login lockout), UX-DR18, UX-DR28, UX-DR101 (login lock), UX-DR107 (lockout alert), UX-DR112, UX-DR103

**Acceptance Criteria:**

**Given** the login panel (solid `surface` panel, radius xl, centred over muted media under the scrim)
**When** it renders
**Then** it shows username and password fields (show/hide, paste allowed), a primary "Log in", underlined "Forgot password?" and "Create account" links, and a footer with PC name, zone and kiosk version; there is no language picker and no social or QR login

**Given** valid credentials
**When** the player presses Enter or Log in
**Then** the kiosk sends `player.authenticate` through the service, the Hub verifies the Argon2id hash, records a `kiosk.player_login` audit row with player and device, and returns the player context in under 3 s on LAN, after which the kiosk shows the plan step for that player

**Given** a wrong username or password
**When** submitted
**Then** both fields show the error treatment with "Username or password didn't match." (never naming which), focus returns to the username field, and the password field clears

**Given** 5 consecutive failures on this kiosk
**When** the fifth fails
**Then** the panel disables both fields and shows "Too many tries. Try again in 0:60." counting down in tabular figures, the Hub raises a login-lockout alert on the tile with sound and badge (Story 3.25), and the counter resets after the 60 s or a successful login

**Given** "Forgot password?"
**When** activated
**Then** the panel shows "Ask staff at the counter to reset it." in place; no reset flow exists on the kiosk

**Given** a player who already has an active, paused or grace session on another PC
**When** they log in
**Then** login is refused with "You're already playing on <PC name>. Ask at the counter."

**Given** the Hub is unreachable
**When** the login panel shows
**Then** Log in is disabled with "Counter is reconnecting — try again shortly." inline, and it re-enables by itself on reconnect

**Given** the plan step is open with no input for 60 s
**When** the timeout passes
**Then** the kiosk forgets the player context and returns to attract

### Story 6.9: The plan picker starts a prepaid session from the kiosk

As a prepaid player,
I want to pick my pack, pass or wallet after logging in and start playing,
So that I don't need staff to start my time.

**Covers:** FR31 (player self-start), FR63 (plan picker), FR32, FR33, NFR10, UX-DR48, UX-DR101 (refusal), UX-DR76

**Acceptance Criteria:**

**Given** a logged-in player on the plan step
**When** the `plan-picker` renders
**Then** it shows only options valid for this PC's group as cards: each owned unexpired pack (hours left, expiry), each owned pass (window), each active block plan (price, end time) payable from the wallet, and Wallet (balance, hourly rate and the active `chip-rule`); postpaid is not offered on the kiosk

**Given** an expired pack, or a pass outside its window
**When** it renders
**Then** it is muted with an "Expired" tag and unused time ("0h 45m unused"), or the window, and cannot be selected

**Given** the player selects an option
**When** they confirm with the primary button
**Then** the selected card gets a 2px `text` border, the kiosk sends `session.start` with the matching mode and `actor{kind: player}` (permission `session.self_start`), and shows "Waiting for the counter…" with the selected plan

**Given** `apply()` accepts the start (rules of Stories 3.24 and 3.27)
**When** `session.started` arrives
**Then** the kiosk unlocks to the Story 3.10 session screen within 2 s and the floor tile shows the player in session

**Given** `apply()` refuses (`INSUFFICIENT_BALANCE`, `PACK_EXPIRED`, `PLAN_NOT_FOR_GROUP`, `PASS_OUTSIDE_WINDOW`, `PC_BUSY`)
**When** the refusal returns
**Then** the picker stays open with the typed reason (e.g. "Balance ₹40. Top up at counter.") and the player can choose another option

**Given** a block chosen
**When** the start applies
**Then** the block price is paid from the wallet in the same intent (wallet never below zero) and the session ends at the block window's end

**Given** the Hub is unreachable
**When** the player confirms
**Then** the request is not sent and the picker shows "Counter is reconnecting — try again shortly."

### Story 6.10: A new player signs up at the kiosk with one consent box

As a first-time player,
I want to create my account at the PC with just a few fields,
So that I can play without waiting for staff to set me up.

**Covers:** FR121, FR201 (consent), AD-24, NFR13, UX-DR18, UX-DR29, UX-DR101 (duplicate), UX-DR103

**Acceptance Criteria:**

**Given** "Create account" on the login panel
**When** the `signup-form` opens in the same shell
**Then** it shows username, password (show/hide, paste allowed), phone (default +91) and name, plus one consent checkbox, unticked, with ≥ 44px hit area and the text "I agree to the Terms and Privacy Policy and to receive service messages on WhatsApp/SMS." with underlined Terms and Privacy links that open read-only pages in the kiosk; there is no OTP and no marketing consent

**Given** the box is unticked
**When** the form renders
**Then** the primary "Create account" is disabled with the reason inline ("Tick the box to continue")

**Given** valid input and the box ticked
**When** submitted
**Then** `player.create` with `source: kiosk` and `actor{kind: device}` creates the player, wallet, credit balance and a granted consent row with the current policy version, encrypting personal data exactly as Story 3.6, and the player continues to the plan picker logged in

**Given** a username or phone already used at the branch
**When** submitted
**Then** the field shows "That username's taken." or "That phone number already has an account — ask staff if it's yours.", focus moves to it, and nothing is written

**Given** a password under 6 characters or an invalid phone
**When** submitted or the field blurs
**Then** the field shows its inline error and nothing is written

**Given** the Hub is unreachable
**When** the form is submitted
**Then** nothing is sent and the form shows "Counter is reconnecting — try again shortly."

**Given** the Hub audit and log
**When** a kiosk sign-up applies
**Then** the audit shows the player id, the kiosk device and the consent version, and the log carries no name or phone

### Story 6.11: Session-end cleanup ends the player's programs and clears the clipboard, temp files and volume

As the next player at a PC,
I want the PC to be clean when I sit down,
So that nothing the last player left open or copied is still there.

**Covers:** FR130 (clean-up actions), FR136 (failures report to the Hub and badge the tile), NFR12, AD-31, AD-14, UX-DR21 (kiosk cleanup on End), UX-DR34 (volume reset)

**Acceptance Criteria:**

**Given** a session ends by any path that releases the PC (staff End, time-up in clean-up mode, forced end, a later player end)
**When** the kiosk service receives the end
**Then** it immediately runs a cleanup pipeline that ends every process in the player session that is not on the protected list (kiosk, its service, Windows essentials), clears the clipboard and clipboard history, deletes the player account's temp files, sets system volume to the branch default, and returns to attract; the pipeline has named steps so later steps can be added to it

**Given** the session ended into a "Pay at counter" lock
**When** cleanup runs
**Then** it runs before the lock shows, so nothing of the player's remains while the bill is open

**Given** "Staff handles" time-up mode
**When** grace ends
**Then** cleanup does not run until staff End the session

**Given** any cleanup step fails (a process will not exit, a file is locked)
**When** one retry also fails
**Then** the service sends `kiosk.cleanup_failed` naming the step, the Hub raises a "CHECK PC" alert on the tile (Story 3.25), and the kiosk holds a locked "Getting this PC ready — see the counter." screen instead of attract until staff choose "Retry cleanup" or "Checked" in the tile sheet

**Given** the Undo of an End (Story 3.14)
**When** it applies after cleanup ran
**Then** the session record is restored as before, and the player must log in again

**Given** the Hub is unreachable when the kiosk locks locally
**When** cleanup runs
**Then** it runs the same steps and queues its result for the Hub with an idempotency key

**Given** an automated test on Windows 10 and 11 images
**When** a session that left apps open, text on the clipboard, temp files and a changed volume ends
**Then** the next session starts with no player processes, an empty clipboard and history, no leftover temp files and the branch default volume

### Story 6.12: Session-end cleanup signs out launchers, Discord and browsers

As the next player at a PC,
I want every launcher and browser signed out when I sit down,
So that I never see someone else's Steam, Discord or browser account.

**Covers:** FR136, NFR12 (launcher logout), AD-31

**Acceptance Criteria:**

**Given** the cleanup pipeline from Story 6.11
**When** a session ends
**Then** it also signs out Steam, Epic, Riot and Discord by removing their saved login tokens and remembered-account files, resets Chrome, Edge and Firefox profiles to the clean template, and removes the player account's Windows Credential Manager entries, before the PC returns to attract

**Given** a launcher still running at cleanup
**When** its sign-out step starts
**Then** the launcher is shut down first, and its files are removed only after it has exited

**Given** a sign-out step fails after one retry
**When** it is reported
**Then** it uses the Story 6.11 failure path, naming the launcher or browser in `kiosk.cleanup_failed` and on the tile's "CHECK PC" alert

**Given** a launcher or browser that is not installed on the PC
**When** cleanup runs
**Then** its step is skipped without error

**Given** an automated test on Windows 10 and 11 images with real Steam, Epic, Riot, Discord, Chrome and Edge logins made during a session
**When** the session ends and a new player session starts
**Then** every launcher and browser opens signed out and no saved credential is offered

### Story 6.13: The player shell with top bar, pill nav and time chip

As a player,
I want a clear home screen with my time always visible,
So that I can find things fast and always know how long I have.

**Covers:** FR123 (shell, pill nav, time chip, account chip), FR27 (staff start lands in the shell), UX-DR26, UX-DR30, UX-DR31, UX-DR32, UX-DR52 (solid lock variants), UX-DR86 (flow), UX-DR100, UX-DR106 (arrow keys in pill nav), UX-DR115

**Acceptance Criteria:**

**Given** a session starts on the PC, from the kiosk plan picker or by staff from the Hub
**When** `session.started` arrives
**Then** the kiosk opens the player shell (replacing the Story 3.10 minimal screen) within 2 s, for the profiled player or "Guest <n>"

**Given** the shell
**When** it renders
**Then** the top bar sits 32px from the top, 64px tall, with `logo-slot` left, `pill-nav` centred and, at the right, the `time-chip` and the account chip (initial and name), matching the Home mock layout

**Given** the `time-chip`
**When** a session runs
**Then** it shows a status dot and the time in `hud-numeric` tabular figures, counting down for prepaid and up for postpaid, following the Hub ticks; at ≤ 10 min left a 2px warning border and a ⚠ glyph replace the dot

**Given** `pill-nav` with Home · Games · Apps
**When** the player uses it
**Then** exactly one item is active (inverted), arrow keys move between items and Enter selects, Home is active at session start, the selection persists through the session and resets at session end

**Given** a tab with no content yet
**When** it renders
**Then** it shows "No games here yet — ask at the counter."

**Given** the Hub sends a lock reason (paused, locked by staff, Pay at counter, forced end)
**When** the kiosk shows it
**Then** a solid `bg` `lock-screen` with `logo-slot` and the reason's catalogue line covers the shell (forced end: "Your session was ended by staff. Please see the counter."), and it lifts when the Hub clears it

**Given** a Playwright run on reference hardware
**When** staff start a session from a free tile
**Then** the shell is interactive within 2 s of the tap

### Story 6.14: The overlay HUD shows time and balance without stealing focus

As a player in a game,
I want a small always-on-top pill with my time and balance,
So that I can check my time without leaving the game.

**Covers:** FR127, UX-DR49, UX-DR50 (toast component), UX-DR106 (Ctrl+Shift+E), UX-DR108, FR117 (blocked-app toast as `hud-toast`), UX-DR115 (key-kiosk-hud-lock)

**Acceptance Criteria:**

**Given** a running session and a game or app in the foreground
**When** the HUD shows
**Then** a separate always-on-top, non-activating window shows `hud-pill` (solid `bg`, time left or elapsed and the wallet balance in `hud-numeric`; guests show time only), and it never takes keyboard focus or input from the game

**Given** the player presses Ctrl+Shift+E anywhere in the session
**When** the hotkey fires
**Then** the HUD expands into a panel in the `hud-toast` palette with time, balance and the session's plan, focus moves into the panel, and Esc collapses it and returns focus to the previous window

**Given** a fullscreen-exclusive game (detected via the Windows user-notification state)
**When** it is in the foreground
**Then** the pill is hidden and nothing is drawn over the game

**Given** the `hud-toast` component in the HUD window
**When** an info toast is raised
**Then** it shows top-right (~640px, solid `#1E1606`, amber border and title, `text` body, Dismiss) with its sound, never takes focus, and auto-dismisses after 8 s; a warning toast persists until dismissed or time-up

**Given** a blocked app is killed (Story 1.17)
**When** the kiosk tells the player
**Then** "That app isn't allowed on this PC." shows as a persistent warning `hud-toast`

**Given** the HUD on reference hardware
**When** a game runs at full load
**Then** the HUD adds no measurable frame-time spikes above 1 ms per frame in the benchmark and uses no blur or translucency

### Story 6.15: Volume control and calling staff from the top bar

As a player,
I want to set my volume and call staff with one tap,
So that I can get help without shouting across the room.

**Covers:** FR41, FR123 (volume, call staff), UX-DR34, UX-DR36, UX-DR31 (cluster order), UX-DR22, UX-DR107

**Acceptance Criteria:**

**Given** the top bar right cluster
**When** it renders
**Then** it shows, in order, `volume-control`, `call-staff-button` ("?" icon + "Call staff" label), `time-chip` and the account chip, leaving a slot for the messages bell left of Call staff

**Given** the speaker icon
**When** the player opens it
**Then** a horizontal slider (`border-strong` track, `text` fill, 44px thumb hit area, arrow keys step 5%) sets Windows system volume through the service immediately

**Given** a new session starts
**When** the shell opens
**Then** system volume is the branch default from Story 2.25, whatever the previous player set

**Given** the player presses Call staff
**When** `device.call_staff` applies
**Then** the tile shows a "CALL" `alert-badge` with the alert sound (Story 3.25) within 2 s, and the kiosk shows the info toast "Staff notified."

**Given** a call already open for this PC
**When** the player presses Call staff again
**Then** no second alert is raised and the kiosk shows "Staff notified." again

**Given** staff open the tile sheet
**When** the alert is acknowledged
**Then** the CALL badge clears

**Given** the Hub is unreachable
**When** the player presses Call staff
**Then** nothing is sent and the toast reads "Counter is reconnecting — try again shortly."

### Story 6.16: Staff messages reach the seat and stay in the messages bell

As counter staff,
I want to message one PC or every PC from the Hub,
So that I can tell players about closing time or a ready order without walking over.

**Covers:** FR40, FR39 (message moves to the bell), NFR3 (staff message ≤ 2 s), UX-DR35, UX-DR50 (info toast)

**Acceptance Criteria:**

**Given** the tile sheet's Message action (Story 3.5) on an in-session PC
**When** staff send text up to 140 characters
**Then** `device.message{scope: seat}` applies and the kiosk shows it as an info `hud-toast` within 2 s, replacing the Story 3.5 banner

**Given** Floor view
**When** staff choose "Message all PCs" and send text
**Then** `device.message{scope: all}` reaches every PC with an active session within 2 s, and the Hub shows how many PCs received it

**Given** the top bar
**When** it renders
**Then** `messages-bell` sits left of Call staff; unread messages add an `info` dot and count, and the count is part of the button's accessible name

**Given** the bell
**When** the player opens it
**Then** it lists this session's messages newest first with time, and opening marks them read

**Given** the session ends
**When** cleanup runs
**Then** the bell list is cleared, and the next player never sees earlier messages

**Given** a message sent to a PC whose kiosk does not confirm within 5 s
**When** the timeout passes
**Then** the sheet shows "Failed — retry" and Retry resends with the same idempotency key

### Story 6.17: The account menu with Lock and Log out

As a player,
I want one menu with my balance, messages, a lock and log out,
So that I can step away safely or finish when I'm done.

**Covers:** FR123 (account menu), UX-DR37, UX-DR32 (chip opens menu), UX-DR14 (menu shadow), UX-DR115 (key-kiosk-account)

**Acceptance Criteria:**

**Given** the account chip or the `time-chip`
**When** the player activates it
**Then** the `account-menu` opens as a `surface` panel with a header (name, wallet balance, time left) and rows Balance, Messages, Lock and Log out; Esc closes it and returns focus to the chip

**Given** Balance
**When** chosen
**Then** it shows wallet balance and owned pack hours, with the line "Top-ups are accepted at the counter."

**Given** Messages
**When** chosen
**Then** the messages bell list opens

**Given** Lock on a profiled player's session
**When** chosen
**Then** a solid lock screen covers the shell and any game, time keeps running, and it unlocks only with the player's own password (paste allowed; 5 failures lock the field for 60 s) or by staff Unlock from the tile sheet; Lock is not offered to guests

**Given** Log out with prepaid time remaining
**When** chosen
**Then** the menu asks in place "You still have 1h 42m. Log out anyway?" (showing the actual time); on confirm `session.end{actor: player}` (permission `session.self_end`) applies with the Story 3.24 deductions, cleanup runs and the PC returns to attract

**Given** Log out on a postpaid session
**When** chosen
**Then** `session.end{actor: player}` applies, the bill is computed to the minute as in Story 3.11, cleanup runs and the PC locks to "Pay at counter"

**Given** the Hub is unreachable
**When** the player chooses Log out
**Then** nothing is sent and the row shows "Counter is reconnecting — try again shortly."

### Story 6.18: "I'm done" ends a postpaid session from the HUD

As a postpaid player,
I want to end my session myself when I'm finished,
So that I stop paying the moment I stop playing.

**Covers:** FR29, AD-13, UX-DR52 (Pay at counter)

**Acceptance Criteria:**

**Given** a postpaid session
**When** the expanded HUD or the account menu shows
**Then** it offers "I'm done" as a secondary button; prepaid sessions do not show it

**Given** the player chooses "I'm done"
**When** the in-place prompt "End your session? You'll pay at the counter." is confirmed
**Then** `session.end{actor: player}` applies, the bill is computed to the minute as in Story 3.11, cleanup runs, the kiosk locks to "Pay at counter", and the tile shows "UNPAID <amount>" within 2 s

**Given** the confirm prompt
**When** the player presses Esc or Cancel
**Then** nothing changes and the session continues

**Given** an "I'm done" end
**When** the Hub shows it
**Then** the audit records the player as actor, and staff Undo within 8 s works as for a staff End (Story 3.14)

**Given** the Hub is unreachable
**When** the player chooses "I'm done"
**Then** nothing is sent and the panel shows "Counter is reconnecting — try again shortly."

### Story 6.19: Time warnings as HUD toasts, a chip warning, and a taskbar flash in fullscreen games

As a player,
I want clear warnings before my time runs out, even in fullscreen games,
So that I'm never cut off without a chance to extend.

**Covers:** FR128 (time warnings), FR127 (fullscreen-exclusive flash + sound), UX-DR50, UX-DR111 (incl. documented limitation), UX-DR32, UX-DR107, UX-DR103

**Acceptance Criteria:**

**Given** a Story 3.23 warning arrives at 10, 5 or 1 min
**When** the kiosk shows it
**Then** a warning `hud-toast` appears top-right with its sound, the title "10 min left. Extend?" (5 and 1 min likewise), the end time and grace line ("Time's up at 7:23 pm. You get a 2-min grace after that."), Dismiss, and the Ctrl+Shift+E key caps; it never takes focus

**Given** a warning toast
**When** it is not dismissed
**Then** it stays until dismissed or time-up; a newer warning replaces the older one

**Given** a postpaid session approaching branch closing time
**When** the marks pass
**Then** the same warnings show, counting to closing time

**Given** a fullscreen-exclusive game in the foreground at the 5- and 1-min marks
**When** the warning fires
**Then** no overlay is drawn; the kiosk flashes its taskbar button and plays the warning sound

**Given** the shell (not a game) is in the foreground
**When** time left is ≤ 10 min
**Then** the `time-chip` shows its warning border and ⚠ glyph, and warnings also show as toasts over the shell

**Given** the kiosk accessibility notes in the repo docs and help-centre source
**When** this story is done
**Then** they record the accepted limitation: in true fullscreen-exclusive games the 5- and 1-min warnings may be sound-only for deaf and hard-of-hearing players, while the 10-min chip warning, the grace lock and the Ctrl+Shift+E hotkey still apply

**Given** the Hub is unreachable
**When** a mark passes on the kiosk's monotonic countdown
**Then** the kiosk raises the same warning locally

### Story 6.20: Per-branch warning marks and the low-balance warning

As a café owner,
I want to choose when players are warned and when they hear their wallet is low,
So that warnings suit how my café plays.

**Covers:** FR128 (configurable per branch, low balance), UX-DR50, UX-DR101 (low balance), UX-DR103

**Acceptance Criteria:**

**Given** Settings → Branch → Billing (Story 3.1)
**When** a Manager opens it
**Then** it also shows "Warn players at" (up to three marks, each 1–30 min, default 10, 5, 1) and "Low balance warning at" (an amount, default ₹50, or off)

**Given** valid values
**When** saved
**Then** `settings.set` stores them, the Story 3.23 engine uses the new marks for sessions from the next tick, and kiosks receive them via `settings.changed`; duplicate or out-of-range marks show a field error

**Given** a profiled player's session paid from the wallet
**When** the remaining wallet balance (after what the session has used) first falls to or below the threshold
**Then** the kiosk shows a persistent warning `hud-toast` "Balance running low — ₹40 left." (actual amount) with "Top up at the counter" guidance and the Extend key caps, once per crossing

**Given** the balance rises above the threshold (a top-up at the counter)
**When** it later falls again
**Then** the warning can fire again

**Given** the `time-chip` warning
**When** the marks are changed
**Then** the chip still turns to its warning state at ≤ 10 min, independent of the configured marks

### Story 6.21: Extend from the kiosk with live prices

As a prepaid player,
I want to add time from the HUD, the menu or a warning without leaving my game,
So that I can keep playing without walking to the counter.

**Covers:** FR129, FR35 (player extend), FR50 (credit limit), NFR3 (floor ≤ 2 s), UX-DR51, UX-DR19 (`button-warning`), UX-DR101 (Extend refusals), UX-DR103

**Acceptance Criteria:**

**Given** a prepaid session
**When** the player presses Ctrl+Shift+E, Extend on a warning toast, or Extend in the account menu (shown with key caps)
**Then** `extend-options` opens in the expanded HUD with focus on the first option, listing +30 min, +1 h and +2 h with prices quoted live by the Hub at the resolved rate, and the payment sources the player has: wallet (balance), each owned pack valid for the group (hours left), and credit (headroom) if credit is enabled

**Given** an option and source chosen
**When** the player confirms
**Then** `session.extend{actor: player}` (permission `session.self_extend`) applies with the Story 3.28 rules, the kiosk shows "More time added. GG.", the chip and HUD update, any warning toast closes, and the floor tile shows the new time within 2 s

**Given** a refusal (`PAST_CLOSING`, `CREDIT_LIMIT`, `INSUFFICIENT_BALANCE`)
**When** it returns
**Then** the panel stays open with "Can't extend past closing time (11:00 pm).", "Credit limit reached, see counter." or "Balance ₹40. Top up at counter.", and the other options stay usable; there is no manager override on the kiosk

**Given** warning toasts from Story 6.19
**When** they render
**Then** they now show Extend as `button-warning` beside Dismiss

**Given** a postpaid session or a guest
**When** the player looks for Extend
**Then** it is not offered

**Given** the Hub is unreachable
**When** `extend-options` opens
**Then** every option is disabled with "Counter is reconnecting — try again shortly."

### Story 6.22: The grace lock with Extend, then the branch time-up mode at the kiosk

As a prepaid player,
I want a short grace period to finish my round with the chance to extend,
So that the PC never cuts me off mid-fight.

**Covers:** FR130, FR168 (local time-up), AD-12, UX-DR52, UX-DR111, UX-DR113 (milestone announcements)

**Acceptance Criteria:**

**Given** a session enters grace (Story 3.26)
**When** the kiosk shows it
**Then** a topmost overlay shows "Time's up", the grace countdown in `kiosk-display` tabular digits and "Wrap up your round. This PC locks when the timer hits 0:00." over the still-visible game behind the scrim (no blur), with Extend (`button-warning`) and the Ctrl+Shift+E key caps

**Given** the grace overlay
**When** the player keeps playing
**Then** the overlay does not take focus and passes input through to the game everywhere except the Extend button; Ctrl+Shift+E moves focus to `extend-options`

**Given** a successful Extend during grace
**When** it applies
**Then** the overlay disappears and the session continues

**Given** a fullscreen-exclusive game at grace start
**When** grace begins
**Then** no overlay is drawn; the taskbar flashes and the warning sound plays

**Given** grace ends in "Clean up" mode
**When** the session ends
**Then** Story 6.11 cleanup runs and the PC returns to attract

**Given** grace ends in "Staff handles" mode
**When** the session expires
**Then** a solid lock "Time's up — see the counter." holds over the game, the game's processes are suspended until staff act, and staff End runs cleanup while a staff Extend from the Hub resumes them and lifts the lock

**Given** the Hub is unreachable
**When** `lock_after_ms` reaches zero on the monotonic clock
**Then** the kiosk performs the mode's local action (clean up and attract, or the staff-handles lock), writes no state, and re-syncs to the Hub record on reconnect

### Story 6.23: The Games tab shows the library and launches a game in one click

As a player,
I want to see every game on this PC and start one with a click,
So that I'm in the game seconds after sitting down.

**Covers:** FR134 (launch, launch log), FR125, UX-DR40, UX-DR100, UX-DR108, UX-DR115 (key-kiosk-games)

**Acceptance Criteria:**

**Given** the Games tab
**When** it renders
**Then** it shows a grid of `game-tile`s (portrait cover 140×196, radius lg, name bottom-left on `tile-scrim`, `surface` fallback with centred name) for every enabled library game installed on this PC, sorted by name

**Given** a focused tile
**When** the player presses Enter or clicks it
**Then** the kiosk service starts the game from its install path or launch URI, checks it against the library and allow-list first, and the Hub records `game.launched` with player, device, game and time

**Given** a game is running
**When** the shell is visible
**Then** the "game running" signal is on and all background video is stopped entirely until the last launched process exits

**Given** the game is already running
**When** the player activates its tile again
**Then** the existing game window is brought to the front and no second copy starts

**Given** the executable is missing or fails to start
**When** the launch fails
**Then** the player sees "Couldn't start <game> — ask at the counter." as an info toast and the Hub log records the error

**Given** a library with no games on this PC
**When** the tab renders
**Then** it shows "No games here yet — ask at the counter."

### Story 6.24: Filter and search the Games tab

As a player,
I want to filter games by type and search by name,
So that I can find my game in a big library in seconds.

**Covers:** UX-DR44, UX-DR106, UX-DR100

**Acceptance Criteria:**

**Given** the Games tab
**When** it renders
**Then** a `filter-rail` on the left lists "All" plus every category and tag in the library (active item in `text`, others muted), and a search field sits right-aligned in the content title row

**Given** a category chosen
**When** it is selected
**Then** the grid filters instantly with no Apply button; only one category is active at a time

**Given** text typed in search
**When** characters are entered
**Then** the grid filters by name, case-insensitive, combined with the active category

**Given** no game matches
**When** the grid is empty
**Then** it shows an underlined "Clear filters" link that resets to All with empty search

**Given** keyboard use
**When** the player uses Tab and arrow keys
**Then** they move between the rail, search and grid in reading order, with the focus ring on every stop

### Story 6.25: The game booster speeds up launches and shows launch time to staff

As a café owner,
I want PCs to free up resources when a game starts and to see how long launches take,
So that games run smoothly and I can spot a slow PC.

**Covers:** FR119, FR134 (booster at launch), AD-31

**Acceptance Criteria:**

**Given** Settings → Games → Booster on the Hub
**When** a Manager opens it
**Then** it shows a list of background programs to close at launch (empty by default; executable names), game process priority (Normal · Above normal · High, default Above normal) and "Switch to High performance power plan during play" (off by default), stored by `settings.set`

**Given** a game launch (Story 6.23)
**When** the service starts it
**Then** it first closes the listed programs running in the player session, starts the game, sets its priority, and switches the power plan if enabled

**Given** the session ends
**When** cleanup runs
**Then** the original power plan is restored

**Given** a launch
**When** the game's first main window appears
**Then** the service measures time from the click to that window and sends it with the launch record; the tile sheet shows "Last launch: <game> · 14.2 s" and Settings → Games shows each game's median launch time over 7 days per PC

**Given** a game that shows no window within 3 minutes
**When** the timeout passes
**Then** the launch is recorded with no launch time and no error is shown to the player

**Given** a listed program that refuses to close
**When** the booster runs
**Then** the game still launches and the failure is logged

### Story 6.26: Launcher cards open the player's own Steam, Epic or Riot account

As a player,
I want to open Steam, Epic or Riot with my own account,
So that I can play games I own, knowing I'll be signed out when I leave.

**Covers:** UX-DR43, FR136 (logout enforced), FR117 (launchers allowed)

**Acceptance Criteria:**

**Given** the Games tab on a PC where Steam, Epic or Riot is installed (found by Story 6.4 or configured)
**When** it renders
**Then** a row of `launcher-card`s ("Play on your own account") shows each installed launcher's logo, name and "Use your own account"; launchers not installed are not shown

**Given** a launcher card
**When** the player activates it
**Then** the service opens that launcher signed out, on its login screen, and records `launcher.opened` with player and time

**Given** the player signs in to the launcher
**When** the session ends
**Then** Story 6.12 signs it out and removes its saved login

**Given** the launcher is already open
**When** its card is activated
**Then** its window is brought to the front

### Story 6.27: Play hours per game and the "Most played here" carousel

As a player,
I want to see what's popular at this café,
So that I can jump into what everyone else is playing.

**Covers:** FR137, UX-DR42, UX-DR106 (arrow keys)

**Acceptance Criteria:**

**Given** a game launched in a session
**When** its last process exits, or the session ends
**Then** a `game_plays` row (new table: player or guest, device, library entry, started, ended, minutes) is written by a `game.play_recorded` intent, so hours per game per branch are in the change log for later owner reporting

**Given** the Hub
**When** a kiosk asks for "most played"
**Then** it returns the branch's top 10 games by played hours over the last 7 × 24 hours, recomputed at most every 15 minutes, and each player's last-played time per game

**Given** the Games tab with plays in the window
**When** it renders
**Then** a `most-played-carousel` titled "Most played here" with "Top 10 by play hours · last 7 days · <branch>" sits above the grid, each tile ranked "#1"…"#10" (`bg` fill, `text` ink) top-left; activating a tile launches the game; arrow keys scroll

**Given** fewer than one game played in the window
**When** the tab renders
**Then** the carousel is hidden

**Given** a game in the top 10 that is not installed on this PC
**When** the carousel renders
**Then** it is skipped and the next one fills the place

**Given** the Hub is unreachable
**When** the session's plays end
**Then** the records are queued and applied once on reconnect

### Story 6.28: The Apps tab

As a player,
I want the café's apps (Discord, a browser, music) in one place,
So that I can open them as easily as games.

**Covers:** FR123 (Apps), UX-DR40, UX-DR44, UX-DR100

**Acceptance Criteria:**

**Given** library entries of kind app (Story 6.3)
**When** the Apps tab renders
**Then** it shows the same structure as Games — `filter-rail` with the apps' categories, search, and a tile grid — without launcher cards or a carousel

**Given** an app tile
**When** activated
**Then** the app opens through the service with the same allow-list check and is recorded as an app launch (not counted in game hours)

**Given** no apps on this PC
**When** the tab renders
**Then** it shows "No games here yet — ask at the counter."

**Given** an app that keeps saved logins (Discord, a browser)
**When** the session ends
**Then** Story 6.12 cleanup covers it

### Story 6.29: Home shows the featured game with a hero video and a featured row

As a player,
I want the home screen to show what's hot here with one button to play,
So that I can start the café's favourite game straight away.

**Covers:** UX-DR39, UX-DR40 (featured), UX-DR41, UX-DR27, UX-DR15, FR125, UX-DR115 (key-kiosk-home)

**Acceptance Criteria:**

**Given** Home
**When** it renders
**Then** the `hero` shows the branch's most-played game over the last 7 days (Story 6.27), falling back to the first library game with media, with `game-media` under the scrim, eyebrow "Most played at <branch> this week", the title in `kiosk-display`, a meta line (genre · launcher · "You last played 2 days ago", omitted if never played), primary Play now and secondary Game details, with copy inside the left-45% × bottom-50% zone and ≤ 864px wide (≤ 720px at 1366)

**Given** Play now
**When** activated
**Then** the game launches as in Story 6.23 and the hero video stops entirely

**Given** Game details
**When** activated
**Then** a card panel shows genre, age rating and launcher; Esc closes it and returns focus to Game details

**Given** the featured row
**When** it renders
**Then** it shows the featured game as `game-tile-featured` (190×260, 3px magenta outline offset 3px, distinct from the focus ring), then the next most-played installed games, ending with the `all-games-tile` ("All games", grid icon) that opens Games with no filter

**Given** `video-control` bottom-right of the hero
**When** the player pauses
**Then** the hero holds its frame for the rest of the session's Home view

**Given** a library with no games
**When** Home renders
**Then** the hero shows solid `bg` with "No games here yet — ask at the counter." and no Play now

### Story 6.30: The kiosk Shop shows the menu with live stock and popular items

As a player on a Basic or Premium café,
I want to browse food and drinks from my seat,
So that I can see what's available without getting up.

**Covers:** FR131 (menu, live stock, out of stock, bundles), FR69 (kiosk shop), AD-29, UX-DR24 (player surfaces hide gated features), UX-DR30 (Shop on Basic+), UX-DR44, UX-DR45, UX-DR46, UX-DR100, UX-DR115 (key-kiosk-shop)

**Acceptance Criteria:**

**Given** a branch whose licence has the kiosk-orders feature
**When** the shell renders
**Then** `pill-nav` shows Shop after Apps; on a Free branch Shop is absent and the route cannot be reached

**Given** the Shop tab
**When** it renders
**Then** it shows a `filter-rail` of product categories with "All", a "Search food & drinks" field, and `shop-item-card`s (photo from the Hub, name, tabular price, Add) for active products and bundles

**Given** a product the Hub publishes as unavailable on `catalog.changed` (out of stock, expired, inactive), or a bundle with a short component
**When** the card renders
**Then** the photo dims, the name and an "Out of stock" tag stay at full ink, and Add is disabled; the card updates within 2 s of the Hub change

**Given** a bundle
**When** its card renders
**Then** it lists its components

**Given** the Hub
**When** the kiosk asks for popular items
**Then** the top 3 products by units sold over the last 7 days carry a `popular-badge` ("POPULAR")

**Given** no available products
**When** the Shop renders
**Then** it shows "Kitchen's closed right now."

**Given** an order-capable intent from a kiosk on a branch without the feature
**When** it reaches `apply()`
**Then** it is rejected `FEATURE_NOT_IN_TIER`

### Story 6.31: Build a cart and add the order to the session bill from the seat

As a player,
I want to order food and drink onto my session bill,
So that I eat without leaving my game and pay once at the end.

**Covers:** FR131 (cart, attach to session, reaches Hub ≤ 3 s), FR67 (kiosk), FR68 (bundle refusal), NFR3, UX-DR47, UX-DR64 (ORDER badge), UX-DR107 (new kiosk order sound)

**Acceptance Criteria:**

**Given** Add on a card
**When** the player adds an item
**Then** the `cart` opens as a right-side panel ("Your order · To <PC> · <zone>") with ±1 steppers, bundle components indented, and the total in `hud-numeric` with "Inclusive of applicable taxes"; an empty cart shows "Cart's empty."

**Given** a stepper change
**When** the player changes a quantity
**Then** stock is checked live with the Hub and a step past availability is refused with the reason inline

**Given** a running session
**When** the cart shows its payment choice
**Then** it offers "Add to session bill" with the line "Payable at the counter when your session ends", and a primary Place order

**Given** Place order
**When** `order.place{actor: player}` (permission `order.self_place`) applies through the Epic 5 "Attach an order to a running session so it settles with the bill" path
**Then** the lines join the session's open bill, stock follows the branch's take-stock mode, and the cart clears with a confirmation

**Given** any kiosk order
**When** it commits
**Then** the Hub plays the new-order sound and raises an alert badge, and the tile shows the "ORDER" `tile-order-badge` within 3 s, until every line is served

**Given** a stock shortage at placement (`OUT_OF_STOCK`, `BUNDLE_SHORT`)
**When** it is refused
**Then** the cart stays, the short line is marked with the reason, and there is no override on the kiosk

**Given** the Hub is unreachable
**When** the cart renders
**Then** Place order is disabled with "Counter is reconnecting — try again shortly."

### Story 6.32: Pay now from the wallet, Pay at counter, and unpaid order lines at the end of a prepaid session

As a player,
I want to pay for my order from my wallet straight away, or at the counter if my session has ended,
So that every order is paid for one way or another without fuss.

**Covers:** FR131 (wallet, Pay at counter), NFR10, UX-DR47, UX-DR52 (Pay at counter)

**Acceptance Criteria:**

**Given** a profiled player's cart in a running session
**When** the payment choices show
**Then** "Pay now from wallet" with the wallet balance is offered beside "Add to session bill", disabled with the reason inline when the balance is below the total; guests see only "Add to session bill"

**Given** "Pay now from wallet" and Place order
**When** the order applies through the Epic 5 "Sell at the counter — a walk-up sale settles and takes stock" path with a single wallet payment
**Then** the order is settled in one transaction (wallet never below zero), its invoice number is issued as for any unprinted bill, and the order is tied to this PC for delivery

**Given** a concurrent wallet spend that leaves too little balance
**When** the wallet order reaches `apply()`
**Then** it is rejected `INSUFFICIENT_BALANCE`, the cart stays and shows "Balance ₹40. Top up at counter." with the actual balance

**Given** the session ended while the cart was open
**When** the cart shows payment choices
**Then** the only option is "Pay at counter", which places an unpaid order for this PC and player for staff to settle at the POS

**Given** a prepaid session whose bill holds unpaid order lines
**When** the session ends
**Then** the kiosk locks to "Pay at counter" and the tile shows "UNPAID <amount>" for the order lines, while the time is settled from the pack or wallet as before

### Story 6.33: Order status reaches the HUD with delivery or pick-up copy

As a player who ordered,
I want to know when my order is ready and whether it's coming to me,
So that I don't wait at the counter for nothing.

**Covers:** FR131 (status toasts), FR67 ("ordered" on the HUD), FR115 (fulfilment setting), UX-DR50 (info toasts), UX-DR35 (kept in bell), UX-DR103

**Acceptance Criteria:**

**Given** an order placed from the kiosk or added by staff to this session
**When** it commits
**Then** the kiosk shows the info toast "Ordered: <n> items" and the `hud-pill` shows an "Ordered" tag while any line is unserved

**Given** the tile sheet's order section on the Hub
**When** staff press "Mark ready" on an order
**Then** `order.ready` applies and the kiosk shows, within 5 s, "Order's ready — on its way to you." when fulfilment is deliver-to-seat or "Order's ready — grab it at the counter." when it is pick-up

**Given** all lines are served (Mark served, Epic 5)
**When** the last line is served
**Then** the "Ordered" tag clears from the HUD

**Given** an unserved line cancelled at the Hub
**When** the cancellation commits
**Then** the kiosk shows "<item> was cancelled — see the counter."

**Given** any order-status toast
**When** it shows
**Then** it auto-dismisses after 8 s, never takes focus, and is kept in `messages-bell`

**Given** the kiosk reconnects after the Hub was unreachable
**When** it re-syncs
**Then** it shows the latest status of each open order once, without replaying every missed toast

### Story 6.34: The kiosk Account page — details, wallet and packs, order history, change password

As a player,
I want to see my balance, packs and orders and change my password at the PC,
So that I know where I stand without asking staff.

**Covers:** FR126, FR48 (kiosk), FR123 (Account row), UX-DR38, UX-DR30 (source tab stays active), UX-DR18, UX-DR115 (key-kiosk-account)

**Acceptance Criteria:**

**Given** the account menu
**When** the player chooses Account
**Then** the Account page opens with `account-subnav` (Details · Wallet & packs · Order history · Change password) on the left and content on the right, the active item in `text` with a 3px left bar, arrow keys moving between items, and the `pill-nav` tab the player came from still active; guests do not see Account

**Given** Details
**When** shown
**Then** it shows name, username and the phone with all but the last 4 digits masked

**Given** Wallet & packs
**When** shown
**Then** it shows the wallet balance with "Top-ups are accepted at the counter.", each owned pack with valid groups, expiry and hours left, passes with their window, and expired packs muted with "Expired" and unused time ("0h 45m unused")

**Given** Order history
**When** shown
**Then** it lists this player's orders at this branch newest first with date, items, total, status and invoice number

**Given** Change password
**When** the player enters the current password and a new one (min 6 characters)
**Then** `player.password_change{actor: player}` (permission `player.self_update`) stores a new Argon2id hash and shows "Password changed."; a wrong current password shows a field error, and 5 wrong attempts lock the form for 60 s

**Given** the Hub is unreachable
**When** the Account page opens
**Then** it shows the last data received this session, and Change password is disabled with "Counter is reconnecting — try again shortly."

### Story 6.35: PC settings saved to the player's profile

As a regular player,
I want my mouse, display, audio and keyboard settings to follow me to any PC,
So that every seat feels like mine.

**Covers:** FR124, FR123 (PC settings row), UX-DR37 (PC settings panel)

**Acceptance Criteria:**

**Given** Settings → Branch → Kiosk display on the Hub
**When** a Manager edits it
**Then** they set the allowed display modes (resolution + refresh pairs, default: each PC's current mode only)

**Given** PC settings in the account menu
**When** the panel opens
**Then** it shows a mouse sensitivity slider (Windows 1–20), display mode from the allowed list that this PC's monitor supports, audio output device from this PC's devices, and keyboard layout from the layouts installed on this PC

**Given** a change
**When** the player makes it
**Then** the service applies it at once, a display-mode change reverts after 15 s unless the player confirms "Keep this", and for profiled players `player.pc_settings_set` saves it to a new `player_pc_settings` row

**Given** a profiled player with saved settings
**When** their session starts on any PC in the branch
**Then** the saved settings are applied; a setting this PC can't honour (mode not allowed or not supported, device missing) is skipped and the branch default is kept

**Given** guests and players without saved settings
**When** their session starts
**Then** the branch defaults apply, and a guest's changes last only for that session

**Given** the session ends
**When** cleanup runs
**Then** mouse, display, audio output and keyboard layout return to the branch defaults

### Story 6.36: The kiosk keeps the session running while the Hub is unreachable

As a player,
I want my game to keep going if the counter PC drops,
So that a problem at the counter never kicks me out of a match.

**Covers:** FR168, AD-12, AD-14, NFR1, NFR8 (kiosk reconnect), UX-DR33, UX-DR98, UX-DR103

**Acceptance Criteria:**

**Given** the kiosk loses the Hub (no reply within 5 s or socket closed)
**When** the shell or HUD is visible
**Then** the `reconnecting-indicator` ("Reconnecting…" quiet `surface` pill, static icon) shows left of the volume control and on the expanded HUD; nothing else locks or interrupts the game

**Given** a running session
**When** the Hub stays unreachable
**Then** the time chip and HUD keep counting from the last tick on the monotonic clock (`lock_after_ms`), a wall-clock change on the PC has no effect, warnings fire at the marks, and at zero the kiosk performs the branch's time-up action locally (Story 6.22)

**Given** the Hub is unreachable
**When** the player tries login, sign-up, a plan request, Extend, Place order, I'm done, Log out, Call staff, change password or saving PC settings
**Then** each is disabled with "Counter is reconnecting — try again shortly.", while launching games and apps, volume and Lock keep working

**Given** events made offline (launches, play records, cleanup results, scan reports)
**When** the Hub returns
**Then** they replay in creation order and apply once by idempotency key

**Given** the Hub returns (or a promoted standby takes over)
**When** the kiosk reconnects with jittered backoff (2–30 s) and re-syncs
**Then** the Hub's record wins: the chip and HUD snap to the Hub's time, a session the Hub ended meanwhile locks with its reason, and the indicator disappears only after this reconcile

**Given** an integration test with the Hub process stopped for 30 minutes during a prepaid session
**When** it runs
**Then** the player is never locked out before `lock_after_ms`, the kiosk locks within 1 s of zero, writes no session state, and after restart the Hub and kiosk agree on the session

### Story 6.37: Kiosk screens reflow at every supported width, by keyboard and in high contrast

As a player on any café monitor,
I want the kiosk to fit my screen and work with just a keyboard,
So that it looks right on every seat and I never need a mouse to get around.

**Covers:** UX-DR86, UX-DR106, UX-DR4, UX-DR17, UX-DR109, UX-DR110, NFR15, NFR16 (+30% strings), NFR26

**Acceptance Criteria:**

**Given** the shell at 1366 px wide
**When** Home and Games render
**Then** the featured row shows 4 tiles + All games, the grid has 5 columns and the hero copy is ≤ 720px; at 1920: 6 + All games, 7 columns, ≤ 864px; at 2560: 8 + All games, 9 columns; at 21:9 ultrawide: 9 columns with content capped at 2560px centred and the hero anchored to the left 45%; the layout reflows and is never scaled, with 56px margins

**Given** a Playwright visual suite at the four widths
**When** it runs on Home, Games, Apps, Shop with cart, Account, login, sign-up, plan picker, HUD, warning toast and grace lock
**Then** nothing clips, overlaps or scrolls horizontally, including with a +30% pseudo-locale

**Given** keyboard only
**When** a tester goes from login through plan picker, Home, Games, launch, Shop order, Account and Log out
**Then** every step works with Tab/Shift+Tab, Enter, Esc and arrow keys in pill nav, sub-nav and carousels, plus Ctrl+Shift+E; no other kiosk shortcut exists, and the focus ring is visible at every stop

**Given** HC is on for the kiosk (Story 1.19)
**When** every player screen renders
**Then** media, scrims, shadows and translucency are gone, every interactive surface has a 2px white outline, and the axe and contrast checks pass with zero serious or critical violations

**Given** any kiosk or HUD stylesheet
**When** CI lints it
**Then** `backdrop-filter`, `filter: blur` and autoplaying unmuted media fail the build

### Story 6.38: The player journey proven on reference hardware

As a café owner,
I want the whole player experience measured on a cheap PC before release,
So that I can trust it on my real floor.

**Covers:** NFR2 (Kiosk < 80 MB), NFR3, NFR6 (kiosk login < 3 s), NFR12 (launcher logout), FR168, FR131 (≤ 3 s), FR129 (≤ 2 s)

**Acceptance Criteria:**

**Given** the reference kiosk PC and a Hub on the LAN in CI's hardware lab
**When** the scripted Journey 3 runs (attract → login → plan picker → Home → launch → warning → Extend → grace → clean-up → attract)
**Then** login completes in < 3 s, the shell opens < 2 s after approval, Extend reaches the floor ≤ 2 s and the PC returns to attract after cleanup, with each timing recorded in the release report

**Given** the same rig
**When** an order is placed from the kiosk
**Then** it appears on the Hub tile ≤ 3 s

**Given** the kiosk service and app idle on attract and idle in the shell
**When** the per-release benchmark measures them
**Then** they use < 80 MB RAM together, excluding the running game, and the job fails the release otherwise

**Given** the launcher-logout suite on Windows 10 and 11 with real accounts
**When** 20 consecutive sessions sign in to Steam, Epic, Riot, Discord, Chrome and Edge and end by each end path
**Then** no later session sees a previous login or saved credential, and any residual fails the release

**Given** the Hub is stopped for 30 minutes mid-session on the rig
**When** the test runs
**Then** play is not interrupted, the lock lands on time, and everything reconciles on restart
