## Epic 12: Console TVs with Pi and QR self-start

Players start and see console time on the TV through a Pi agent and a QR code.

> Notes: this epic builds on the Epic 1 scaffold (Story 1.1: the `arena-pi` crate already exists as a placeholder, shared UI package, tokens and message catalogue), the LAN envelope and class scope table (Story 1.9: `paired`, `kds`, `phone`), `apply()` and idempotent intents (Stories 1.7–1.8), inline PIN (Story 1.13), the audit viewer (Story 1.15) and per-device HC (Story 1.21); on Epic 2 mDNS discovery and typed-IP fallback (Story 2.17), 6-digit device pairing with pinned fingerprint (Story 2.18), the device cap that already counts Pis and the pairing limiter (Story 2.19), console registration with a TV number (Story 2.21), group rates (Story 2.25) and opening hours (Story 2.26); on Epic 3 the tile sheet and "Failed — retry" pattern (Story 3.5), session start in every mode (Stories 3.9, 3.24, 3.27), session ticks with `lock_after_ms` and `time_up_mode` (Story 3.10), pause (Story 3.15), the timer engine and warning marks (Story 3.23), the alert service with sound and badge (Story 3.25), Hub Extend (Story 3.28), console sessions on the tile (Story 3.35) and console time-up plus controller check (Story 3.36); on Epic 6 the Hub media cache served on `:7401` to paired devices (Story 6.2), the `game-media` fallback component (Story 6.6), the branch logo setting (Story 6.7) and the kiosk Hub-unreachable rules (Story 6.36); on the Epic 7 pattern of a plain-HTTP browser client served by path on `:7401` (Story 7.6); on Epic 8 owner device revocation ("Manage → Devices and groups, and revoking a device"); on Epic 9 standby fingerprints, promotion and epoch rules ("The standby bootstraps from the main Hub and follows it live", "Promote the standby to main with a manager PIN"); and on the Epic 11 Help centre ("Help centre with setup guides, troubleshooting and compatibility lists"), where this epic publishes the Pi flashing steps and adds console-TV rows to the hardware compatibility data file. **Decided (architecture):** the Pi agent is Rust on aarch64, shipped as a Raspberry Pi OS Lite image with a read-only root and overlay, run by systemd; it pairs like a kiosk (paired-device class, device token, TLS pinned to the Hub certificate); TV screens are a Svelte page served by the agent on localhost and shown by Chromium in kiosk mode, using the shared tokens and components; the phone page is served by the Hub on `:7401` over plain HTTP at `/console`, as the anonymous `phone` class limited to `console.plan_requested` and reading its own timer, rate-limited per TV; staff confirm and start at the Hub with their own actor and the player pays at the counter; there is no player login and no console grace; CEC runs after commit from the side-effect outbox and records a result intent (AD-28); while the Hub is unreachable the Pi counts down on its monotonic clock, runs warnings and the lock, and writes nothing (AD-12); the phone page is unavailable then. **QR link (decided):** the QR uses the friendly mDNS name `http://arena360.local:7401/console/<tv>?t=<token>`, advertised by the primary Hub. **Known risk (accepted):** some Android phones cannot resolve `.local` names, so the phone page shows nothing for them; the fallback is staff starting the session from the tile, which works with or without the phone page. **Smart plugs (FR145 clause DEFERRED, decided):** FR145's "CEC standby or smart plug per branch" is delivered for CEC standby only. The smart-plug option is explicitly deferred: this epic ships the `PowerSwitch` adapter interface and a test fake only, no vendor adapter, and the vendor (Tuya, Tapo or other local-API plug) is TBD. The branch option is shown disabled until an adapter ships in a later release; NFR18's "smart plug as the guarantee" wording needs the same deferral note. **Out of this epic:** Pi agent auto-updates from the Hub's signed cache (FR176, Epic 14; this epic ships the signed image only), the console 5-minute WhatsApp/SMS (Story 8.30), the controller check itself (Story 3.36), and remote Pis through the Tunnel (Story 8.41 path, unchanged). Every story adds its new intents to the Story 1.8 `proptest` generator.

### Story 12.1: A signed, read-only Pi image is built in CI

As a café owner adding a TV,
I want one ready-made image I can flash onto a Pi,
So that setting up a console TV needs no Linux knowledge and the Pi can't be broken by being unplugged.

**Covers:** FR142 (hardware), AD-32 (signed agent), NFR17, NFR18

**Acceptance Criteria:**

**Given** the CI image job
**When** it runs on a release tag
**Then** it builds one Raspberry Pi OS Lite (64-bit) image for Pi Zero 2 W and Pi 4 containing the `arena-pi` aarch64 binary, Chromium, and a systemd unit that starts the agent and then Chromium in kiosk mode pointed at the agent's localhost page

**Given** the agent binary in the image
**When** it is built
**Then** it is ed25519-signed, and the agent's launcher verifies the signature before start and refuses to run an unsigned or tampered binary, logging why

**Given** the built image
**When** it boots
**Then** the root filesystem is mounted read-only with a tmpfs overlay, and a small writable `/data` partition holds only the device token, the pinned fingerprint, the last cached branding and rates, and size-capped logs

**Given** the image
**When** it is inspected in CI
**Then** SSH, the desktop environment, auto-login to a shell and any open inbound port other than the agent's localhost page are absent, and the job fails if any is present

**Given** a release
**When** the image is published
**Then** it is published with its SHA-256 and signature next to the help-centre flashing steps

### Story 12.2: The Pi boots into a TV screen, finds the Hub and recovers on its own

As a café owner adding a TV,
I want the Pi to show a clean screen on the TV and look for my Hub by itself,
So that the TV is ready to pair within a minute of plugging it in and keeps working after crashes and power cuts.

**Covers:** FR142 (discovery), AD-8 (mDNS name from `arena-proto`), AD-33, NFR17, NFR18, UX-DR6, UX-DR9, UX-DR94

**Acceptance Criteria:**

**Given** the Pi boots the Story 12.1 image
**When** the agent starts
**Then** within 60 s of power-on the TV shows the agent's Svelte page in the Kiosk 1920 layout and type ramp, dark-only, with `logo-slot` showing the Arena 360 mark and "Looking for the Hub…"; no desktop, cursor, console text or Chromium UI is ever visible

**Given** a Hub advertising the `arena-proto` mDNS service on the LAN
**When** the unpaired Pi searches
**Then** it lists discovered Hubs by branch and Hub name within 10 s; with none found it shows a "Hub IP address" field and "Search again", as on the kiosk (Story 2.17)

**Given** setup input
**When** a USB keyboard is plugged in, or the TV remote sends digit and arrow keys over CEC
**Then** the page can be operated with it (choose Hub, type IP or code); with neither, the page stays on the discovery screen and nothing else is reachable

**Given** the agent or Chromium crashes
**When** systemd or the agent's watchdog notices
**Then** it is restarted within 5 s and the TV returns to the agent page, and the restart is logged to `/data`

**Given** an automated power-pull test on both boards
**When** power is cut 100 times at random points during boot and while idle
**Then** every boot reaches the agent page and `/data` mounts clean each time

### Story 12.3: TV control adapters — HDMI-CEC, and a power-switch interface for smart plugs

As a developer on Arena 360,
I want the Pi to drive the TV through one adapter interface with a CEC implementation and a smart-plug slot,
So that every TV feature uses the same commands and plug vendors can be added later without touching them.

**Covers:** FR145 (CEC standby; smart-plug clause deferred — interface only), FR147 (power, input), AD-28, NFR18 (CEC best-effort; plug guarantee deferred)

**Acceptance Criteria:**

**Given** `arena-pi`
**When** the `TvControl` trait is defined
**Then** it exposes `power_on`, `standby`, `show_pi_input`, `show_console_input(port)` and `status`, each returning a typed result (`Ok`, `Unsupported`, `NoAck`, `Timeout`) within 3 s; the CEC implementation uses libcec, and a recorded fake implementation is used in tests

**Given** the `PowerSwitch` trait (`on`, `off`, `state`)
**When** the agent starts
**Then** it loads registered adapters by name from a registry that is empty at launch apart from a test fake, and no vendor adapter (Tuya, Tapo or other) is built in this epic — the smart-plug clause of FR145 is deferred with the vendor TBD; Settings → Consoles at the Hub shows "Smart plug" as a disabled option with "No supported smart plug yet"

**Given** a TV command frame (defined in `arena-proto`; sent by a test harness in this story and by the Hub once a Pi is paired)
**When** the agent runs it
**Then** it replies with the result kind and the time taken, and the Hub records it through the side-effect outbox as `tv.command.result` (never inside `apply()`); a CEC `NoAck` is retried once before it is reported

**Given** a TV whose CEC probe was negative
**When** any CEC command is requested
**Then** it returns `Unsupported` at once without touching the bus

**Given** the fake adapters
**When** the unit suite runs
**Then** every command, result kind, timeout and retry path is covered

### Story 12.4: Pair a Pi with a 6-digit code and see it on the floor within a minute

As a café owner setting up a console TV,
I want to type the Hub's 6-digit code on the Pi once,
So that the TV joins my branch securely and I can see it at the counter.

**Covers:** FR142 (pairing, floor ≤ 60 s), AD-10, AD-42 (paired-device class), AD-29, NFR11, NFR23, UX-DR2, UX-DR61

**Acceptance Criteria:**

**Given** Settings → Devices on the Hub
**When** a Manager or Owner presses "Pair a TV (Pi)"
**Then** the same 6-digit single-use code panel as kiosk pairing (Story 2.18) is shown with a 10-minute countdown and a fresh code after each use

**Given** a Pi that has selected a Hub
**When** the code is entered
**Then** over TLS to `:7401` it sends `pair{code, name, version, kind: pi}`; the Hub mints a 256-bit device token (Argon2id hash in `devices`), creates a `devices_meta` row with `kind = pi` via `device.register`, and returns the token and the Hub certificate fingerprint once; the agent stores both on `/data` (mode 0600) and pins the fingerprint for every later connection

**Given** the agent connects with `hello{class: paired}`
**When** it reports `capabilities`
**Then** they include the board model and a CEC probe result (`cec: true|false`, adapter, the Pi's physical address), and the Hub stores them on the device row

**Given** a just-paired Pi
**When** 60 s have passed since the code was entered
**Then** it appears in the floor's Unassigned section as a dashed entry with its name and a TV glyph, and the TV shows "Not linked yet — link this TV at the counter."

**Given** a wrong or expired code, 5 failures on one code, or more than 20 attempts per IP per hour
**When** they happen
**Then** the Pi clears the field with a plain retry line, the code is invalidated, or `RATE_LIMITED` is returned (Story 2.19 limiter)

**Given** the device cap is reached
**When** a Pi pairs
**Then** it is refused `DEVICE_LIMIT`, the Hub shows the Story 2.19 `tier-lock`, and the TV shows "This branch has reached its device limit — ask the owner."

**Given** a Pi is revoked at the Hub or by owner command
**When** the revocation applies
**Then** its socket closes with `TOKEN_REVOKED`, the agent deletes its token from `/data` and returns to the discovery screen, and its slot is freed

### Story 12.5: Link a Pi to a console so the TV shows its TV number

As counter staff,
I want to link each Pi to the console it sits next to,
So that the TV shows the right number and the console tile knows it has a TV it can control.

**Covers:** FR142 (TV number, floor), UX-DR62, UX-DR97 (Unassigned, Offline), UX-DR110

**Acceptance Criteria:**

**Given** an Unassigned Pi's sheet
**When** a Manager chooses "Link to console" and picks a registered console (Story 2.21) that has no Pi
**Then** `pi.link{pi_id, console_id}` applies, the Pi leaves Unassigned, and the TV shows the console's "TV n" in `kiosk-headline` within 2 s

**Given** a console that already has a Pi, or a Pi already linked
**When** another link is attempted
**Then** it is refused `PI_ALREADY_LINKED` with the inline reason naming the other device; "Unlink" on the console sheet frees both (Manager, audited)

**Given** a linked console tile
**When** it renders
**Then** the sheet shows a "TV" line with the Pi state as text and glyph: "TV connected", "TV offline (last seen 8:42 pm)" after 30 s without the Pi, or "No CEC — switch inputs by hand" when the probe found no CEC

**Given** the console's TV number is edited (Story 2.21)
**When** it is saved
**Then** the linked TV shows the new number within 2 s

**Given** the Pi's link
**When** the console's input is set at linking
**Then** the agent scans the CEC bus and offers the devices it found with their HDMI port; staff pick the console's port (or type HDMI 1–4 if the scan finds nothing), and it is stored on the link as the console input

**Given** HC is switched on for a Pi in the Story 1.21 list (Pis now listed by name)
**When** it is saved
**Then** the TV switches to `-hc` tokens within 2 s and applies it from cache before first paint on its next cold start

### Story 12.6: The idle TV shows branding, console rates and a "Scan to start" QR

As a player walking past a free console,
I want the TV to show what it costs and a code I can scan,
So that I know the price and can ask for a session from my phone.

**Covers:** FR143, AD-17, AD-42 (QR token), UX-DR53, UX-DR26, UX-DR15, UX-DR94, UX-DR99, UX-DR104 (no ads on Pi), UX-DR108

**Acceptance Criteria:**

**Given** a linked Pi whose console tile is Free
**When** the TV is on the Pi input
**Then** it shows `tv-idle` at 1920×1080: full-bleed `game-media` (Story 6.6) cycling curated videos of console games in the branch library from the Hub's `/media` with the device token, falling back to cover then solid `bg`, under `scrim`; `logo-slot` top-left; "TV n" in `kiosk-headline` and the console group's plans and rates in tabular figures bottom-left in the copy zone; and a QR of at least 360 px with a white quiet zone and `rounded.md` frame bottom-right labelled "Scan to start"; there is no ad slot anywhere

**Given** the Hub
**When** it issues the QR for a console
**Then** the QR encodes `http://arena360.local:7401/console/<tv>?t=<token>` where the token is a random per-console value that rotates every 5 minutes, the previous token stays valid for one more period, and the TV redraws the QR on each rotation

**Given** the primary Hub
**When** it is running
**Then** it advertises the host name `arena360.local` by mDNS pointing at its LAN IP (a standby does not advertise it until promoted), and Settings → Consoles shows the QR link it encodes

**Given** the group rate, a console plan, a pricing rule or the branch logo changes
**When** the Hub pushes the change
**Then** the TV's rates and logo update within 1 min without a reload

**Given** a cold start
**When** the page first paints
**Then** branding, TV number and rates come from the `/data` cache before any network call, and media streams in afterwards

**Given** media cannot play (decode error, or the Pi drops frames past the agent's threshold)
**When** it renders
**Then** it falls to the cover for the rest of that view with no visible error; HC skips straight to solid `bg` with no video

**Given** a console tile that is in maintenance, has "CHECK CONTROLLERS" open, or has no plan for its group
**When** `tv-idle` renders
**Then** the QR is replaced by "Ask at the counter" and no token is issued

### Story 12.7: The phone start page requests a console plan with no login

As a player with my phone,
I want to scan the TV's code, pick a plan and tap Request,
So that the counter knows which TV I want and what I want to pay for.

**Covers:** FR144 (QR page, request), AD-42 (anonymous `phone` class, plain HTTP), AD-23, NFR23, UX-DR56, UX-DR48, UX-DR94, UX-DR103, UX-DR109

**Acceptance Criteria:**

**Given** a phone on the café Wi-Fi scanning the QR
**When** it opens the URL
**Then** the Hub's `:7401` listener serves the phone page on the `/console` path over plain HTTP (TLS clients on the same port unaffected), and it renders `qr-start-page` in one portrait column with 16 px gutters: `logo-slot` and "TV n", the console plans valid for that console's group as `plan-picker` cards with live tabular prices, and one primary "Request" button; there is no login, sign-up or account field

**Given** a valid token
**When** the player picks a plan and taps Request
**Then** the page calls `console.plan_requested{tv, plan_id, token}` as the `phone` class; the Hub writes a `console_requests` row (new table: id, device_id, plan_id, status, request-token hash, created_at) and returns a 128-bit request token that the page keeps in browser storage; the page shows "Waiting for the counter — pay at the counter to start."

**Given** an expired or unknown token
**When** the page loads or requests
**Then** it shows "This code has expired — scan the TV again." and nothing is written

**Given** the console is in session, in maintenance, or already has an open request
**When** a request is made
**Then** it is refused, the page shows "Ask at the counter.", and the open request is not changed

**Given** a phone that cannot resolve `arena360.local` (some Android phones do not resolve `.local` names)
**When** it scans the QR
**Then** the phone page does not load and nothing reaches the Hub; the players ask at the counter and staff start the session from the console tile as in Story 3.35, with the TV following the session as normal

**Given** rate limits
**When** one TV receives more than 6 requests in 10 minutes, or one IP more than 20 in an hour
**Then** further requests return `RATE_LIMITED` and the page shows "Ask at the counter."

**Given** a `phone`-class call
**When** it targets anything other than `console.plan_requested` or its own timer, or presents a paired or `kds` token over plain HTTP
**Then** the Hub returns `FORBIDDEN` and logs it; a CI test asserts the `phone` scope contains no staff, money, player or device intent and that no player name or phone number is ever in a `/console` response

### Story 12.8: Staff confirm or refuse a QR request from the console tile

As counter staff,
I want a console request to alert me with the plan already chosen,
So that I can take payment and start the TV in a couple of taps, or turn it down.

**Covers:** FR144 (REQUEST badge, staff start, refusal), UX-DR62, UX-DR22, UX-DR107, UX-DR101, UX-DR104

**Acceptance Criteria:**

**Given** a new `console_requests` row
**When** it is written
**Then** the console tile raises `alert-badge` "REQUEST" with sound through the Story 3.25 alert service, and the header alert list shows "Request · TV n · <plan>"

**Given** staff open that tile's sheet
**When** it renders
**Then** the requested plan is pre-selected in the start area, staff pick a player by lookup or Guest, and any start mode valid for the group (postpaid, pack, wallet, pass, block) can still be chosen instead

**Given** staff tap Start
**When** `session.start{request_id}` applies
**Then** the request becomes `started` and is linked to the session, the badge clears, and the audit records the staff actor (the phone is never the actor of a start)

**Given** staff tap "Refuse"
**When** `console.request.refused` applies
**Then** the badge clears and the phone shows "Ask at the counter." within 2 s

**Given** a request unanswered for 10 minutes
**When** the timer engine reaches it
**Then** it becomes `expired`, the badge clears, and the phone shows "Ask at the counter."

**Given** the tile still shows "CHECK CONTROLLERS"
**When** a request arrives
**Then** it is accepted and badged, but Start stays disabled with "Mark controllers first" (Story 3.36)

### Story 12.9: The TV follows the console session — console input at start, idle at end

As a player,
I want the TV to switch to the console as soon as the counter starts my time,
So that I can pick up the controller and play straight away.

**Covers:** FR144 (TV switches ≤ 3 s), AD-12 (ticks to Pi), AD-28, NFR3, UX-DR53, UX-DR103

**Acceptance Criteria:**

**Given** a linked console
**When** a session starts on it
**Then** the Hub sends the Pi `session.started` and then `session.tick` frames with remaining or elapsed time, `lock_after_ms` (postpaid: until closing time) and `time_up_mode: lock`, and the Pi calls `show_console_input(port)`; the TV shows the console within 3 s of the Start tap

**Given** the input switch fails or the TV has no CEC
**When** the Pi reports the result
**Then** the tile's TV line shows "Failed — retry" (or "No CEC — switch inputs by hand"), Retry resends with the same idempotency key, and the session keeps running

**Given** staff pause the session (Story 3.15)
**When** `session.pause` applies
**Then** the Pi switches the TV to its input with "Session paused — see the counter", and on resume switches back to the console within 3 s

**Given** a session ends early by staff, or by forced end
**When** `session.end` applies
**Then** the Pi switches to its input and shows `tv-idle` within 3 s

**Given** Extend from the Hub (Story 3.28)
**When** it applies
**Then** the Pi receives the new `lock_after_ms` within 2 s

**Given** the Pi reconnects after a restart during a session
**When** it re-syncs
**Then** the Hub's record wins and the TV shows the console, the lock or idle to match it

### Story 12.10: The phone shows a live timer for its TV

As a player,
I want my phone to show how long is left on our TV,
So that we know when to wrap up without asking the counter.

**Covers:** FR144 (live timer only), AD-42 (own timer), AD-12 (phone page unavailable offline), UX-DR57, UX-DR98, UX-DR103

**Acceptance Criteria:**

**Given** a phone holding the request token of a started request
**When** the session starts
**Then** the page replaces the waiting line with `live-timer`: one `surface` card with "Time left on TV n", the time in tabular 96 px Sora 200 (elapsed for postpaid), and "Extend at the counter"; there are no buttons or links

**Given** the page is open
**When** the Hub pushes ticks on the timer read (scoped to that request token's session only)
**Then** the time updates every second and reflects an Extend within 2 s

**Given** the phone leaves the café Wi-Fi, or the Hub is unreachable
**When** updates stop for 5 s
**Then** the card keeps the last value and adds "Reconnect to café Wi-Fi to update"; it resumes on reconnect

**Given** the session ends
**When** the phone next updates
**Then** it shows "Time's up — see the counter." and the request token stops working 10 minutes later

**Given** a request token from another TV or an ended session past its window
**When** it is used to read a timer
**Then** the Hub returns `FORBIDDEN` and the page falls back to "This code has expired — scan the TV again."

### Story 12.11: TV warnings at 5 and 1 minute on the Pi input

As a player on a console,
I want the TV to tell me when 5 and 1 minutes are left,
So that I can finish the match or ask for more time.

**Covers:** FR145 (warnings), AD-12, UX-DR54, UX-DR103, UX-DR110

**Acceptance Criteria:**

**Given** a running console session with a linked Pi
**When** 5 min and then 1 min remain (to `ends_at`, or to `closing_at` for postpaid)
**Then** the Pi switches the TV to its input and shows `tv-warning` for 5 s — solid `bg`, 8 px `warning` frame, "5 min left" (or "1 min left") in `kiosk-display` and "ask at the counter to extend" in `kiosk-headline` — then switches back to the console input

**Given** the warning marks
**When** the Pi computes them
**Then** it uses the marks from the Hub's ticks against its own monotonic clock, so a dropped tick never skips a warning, and each mark fires at most once per session

**Given** Extend moves the end past a mark that has not fired
**When** the new tick arrives
**Then** the mark is rescheduled; a mark that already fired does not fire again for the same remaining time

**Given** the switch back to the console fails
**When** the Pi gets `NoAck` twice
**Then** the tile's TV line shows "Failed — retry" and the TV stays on the warning screen until Retry or the next session event

**Given** a TV with no CEC
**When** a mark is reached
**Then** the Pi draws `tv-warning` on its own input only, and the tile still shows the Story 3.35 warning states

### Story 12.12: Time-up locks the TV with no grace, then the branch power action

As a café owner,
I want a console TV to stop the moment time runs out,
So that nobody plays on unpaid while staff are busy.

**Covers:** FR145 (time-up lock, CEC standby per branch; smart plug deferred), AD-12, UX-DR55, UX-DR103

**Acceptance Criteria:**

**Given** a console session reaching zero
**When** the Pi's countdown or the Hub's time-up arrives (whichever is first)
**Then** the Pi immediately switches the TV to its input and shows `tv-lock` — solid `bg`, `logo-slot`, and "Time's up — see the counter." in `kiosk-headline`, nothing else — with no grace; the tile's "TIME UP" alert is Story 3.36's

**Given** Settings → Consoles → "At time-up" with Lock screen only · Lock, then TV standby · Lock, then smart plug off (shown disabled with "No supported smart plug yet": the smart-plug clause is deferred and no vendor adapter ships in this epic)
**When** "Lock, then TV standby" is set and 30 s pass after the lock
**Then** the Pi sends CEC standby; the result is recorded as `tv.command.result`

**Given** the TV is locked
**When** the CEC bus shows another source becoming active (a player switched input with the TV remote) or the TV is powered back on
**Then** the Pi switches back to its input and re-shows `tv-lock` within 3 s

**Given** staff end the session
**When** `session.end` applies
**Then** the Pi powers the TV on if it was put in standby, shows `tv-idle`, and the tile carries on to "CHECK CONTROLLERS"

**Given** a postpaid console session reaching branch closing time (`lock_after_ms` from opening hours)
**When** it reaches zero
**Then** the TV locks the same way with the same copy and power action

### Story 12.13: Remote TV commands from the tile sheet — power, input and message

As counter staff,
I want to turn a TV on or off, switch its input or put a message on it from the counter,
So that I can sort out a TV without walking over.

**Covers:** FR147 (remote commands ≤ 5 s), NFR3, AD-25, UX-DR65, UX-DR101 (Failed — retry), UX-DR104

**Acceptance Criteria:**

**Given** a linked console's sheet with the Pi connected
**When** it renders
**Then** a "TV" section offers Power on, Power off, Show console, Show TV screen and Message, each one tap from the open sheet; actions the Pi reported `Unsupported` are shown disabled with "No CEC on this TV"

**Given** a Counter chooses Show console, Show TV screen or Message (text up to 140 characters)
**When** `tv.command` applies (permission `device.screen`)
**Then** the Pi runs it; a message switches the TV to the Pi input, shows the text in `kiosk-headline` on solid `bg` for 10 s, then returns to the previous input

**Given** Power on or Power off
**When** chosen by a role without `device.remote`
**Then** `inline-pin` opens in the sheet, and the command is audited with both actors

**Given** Power off on a console with an active, paused or expired-unended session
**When** requested
**Then** it is refused `CONSOLE_BUSY` with "End or pause the session first"

**Given** any TV command
**When** the Pi's result arrives within 5 s
**Then** the sheet shows it inline; otherwise the tile's TV line shows "Failed — retry", and Retry resends with the same idempotency key

**Given** the Pi is offline
**When** the sheet renders
**Then** the TV actions are disabled with "TV offline (last seen …)"

### Story 12.14: The Pi enforces the session while the Hub is unreachable

As a café owner,
I want a console TV to keep warning and locking on time even if the counter PC drops,
So that a Hub problem never means free play.

**Covers:** FR147 (offline enforcement), AD-12, AD-14, NFR1, NFR8, UX-DR98

**Acceptance Criteria:**

**Given** the Pi loses the Hub (no reply within 5 s or socket closed) during a session
**When** it stays unreachable
**Then** the agent counts `lock_after_ms` down on its monotonic clock (a wall-clock change has no effect), shows the 5- and 1-minute warnings and the time-up lock at the right moments, and runs the branch time-up power action

**Given** the Pi is unreachable from the Hub
**When** anything happens on the Pi
**Then** it writes no session or request state and queues nothing that changes branch data; command results it could not send are dropped and the Hub shows them as "Failed — retry"

**Given** the TV is idle while the Hub is unreachable
**When** `tv-idle` renders
**Then** the QR is replaced by "Ask at the counter" (the phone page cannot work) until the Hub is back

**Given** the Pi reconnects with jittered backoff (2–30 s), or a promoted standby takes over (standby fingerprint pinned per Story 9.18, epoch rules per Story 9.21)
**When** it re-syncs
**Then** the Hub's record wins: a session the Hub ended shows `tv-idle`, an extended one returns to the console, and the TV line on the tile clears "TV offline"

**Given** an integration test with the Hub stopped for 30 minutes during a prepaid console session on the fake TV adapter
**When** it runs
**Then** warnings fire within 1 s of 5 and 1 min, the lock within 1 s of zero, the Pi writes nothing, and after restart Hub and Pi agree

### Story 12.15: CI drives a hardware lab rig of real Pis and TVs

As a developer on Arena 360,
I want CI to drive a permanent rig of real Pis and TVs,
So that every release is checked on the hardware cafés actually buy.

**Covers:** NFR18, FR142 (BOM < ₹4,000)

> Notes: the physical lab is a human ops/hardware task, not dev-agent work. A named hardware owner at Arena 360 buys and assembles the boards, TVs, HDMI capture devices and Hub PC, installs the self-hosted runner and writes the help-centre bill of materials; this story starts only once that owner has recorded the rig as ready. The dev story is the CI job wiring and the capture-based assertions below.

**Acceptance Criteria:**

**Given** the hardware owner has recorded the lab as ready (a human prerequisite)
**When** the CI job inventories it
**Then** it finds a Pi Zero 2 W and a Pi 4 on the Story 12.1 image, at least two TV brands with CEC and one without, a console or CEC test source on each TV, a Hub on reference hardware, and a phone-browser runner on the same LAN, all reachable from a self-hosted CI runner

**Given** the rig
**When** a test needs to know what the TV shows
**Then** an HDMI capture device on each TV path reports the active input and a frame, so input switches and screens are asserted on the real signal, not on the agent's own report

**Given** the release pipeline
**When** a release candidate for the Pi agent or Hub is built
**Then** the rig flashes the new image, pairs each Pi with a fresh code and links it to its console, and fails the job with a clear message if any step does

**Given** the bill of materials written by the hardware owner
**When** a rig run passes
**Then** the job writes each TV model's tested CEC result as `category: console_tv` rows in the help-centre hardware data file, and a CI check fails if the published Pi Zero 2 W kit (board, case, power supply, microSD, mini-HDMI cable) totals ₹4,000 or more; console-TV rows note that CEC is best-effort and the smart-plug option is not yet available

### Story 12.16: Console TVs proven on the rig against their targets

As a café owner,
I want the TV path measured on the boards and TVs I'm told to buy,
So that the cheap kit I set up works the way the counter expects.

**Covers:** FR144 (≤ 3 s), FR147 (≤ 5 s), NFR3, NFR15, NFR18, UX-DR53–UX-DR57

**Acceptance Criteria:**

**Given** the Story 12.15 rig
**When** the release suite runs
**Then** console input switch at start is ≤ 3 s at p95 over 50 starts, every remote command result is ≤ 5 s at p95, and both 5-s warnings return to the console every time on the CEC TVs

**Given** the Pi Zero 2 W
**When** `tv-idle` runs for 8 hours
**Then** the agent stays under 60 MB RAM, Chromium does not restart from memory pressure, and media falls back to cover rather than stuttering

**Given** Playwright against the TV page and the phone page in normal and HC themes
**When** contrast and axe checks run
**Then** load-bearing pairs meet 4.5:1 text and 3:1 non-text with zero serious or critical violations, and the phone page targets are ≥ 44 px

**Given** a full Journey 10 run (scan, request, refuse, request, start, warnings, time-up, end, controller check)
**When** it runs on the rig with the WAN blocked
**Then** every step completes with the Voice lines from the catalogue and no outbound connection is attempted

**Given** any target missed
**When** the suite finishes
**Then** the release is blocked and the report names the board, TV and step
