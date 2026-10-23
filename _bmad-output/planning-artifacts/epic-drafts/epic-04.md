## Epic 4: Free cafés run on an ad-supported Android Hub

A Free café runs its branch on an Android Hub (Tauri 2 mobile) with a non-intrusive banner, an interstitial only on idle return, and Free limits enforced.

> Draft notes: the Android Hub is the same Svelte Hub UI and the same Rust core (`arena-core`, `arena-store`, `arena-api`, `arena-tasks`) as the Windows Hub, hosted by Tauri 2 mobile. Epic 4 adds only what Android needs: the app build and its spec checks, a foreground service, mDNS through `NsdManager`, the Android Keystore, Bluetooth printing and a folder picker, phone layout behaviour, and the Free tier's ads. Everything from Epics 1–3 must run unchanged on it. Until this epic, a Free branch could not pair any Hub (Story 2.9 refuses Windows on Free). Story 4.4 closes that gap, and Story 4.8 proves a Free owner can go from sign-up to go-live on Android with no comp entitlement. **Ads** are reached only through an in-house Tauri Kotlin plugin (`tauri-plugin-arena-ads`). No Rust crate contains ad code. The Windows build ships without the plugin. Ads show when the platform is Android **and** the effective entitlement (Story 2.11) is Free. That includes the Free fallback after a failed licence check and after a lapse. A paid Android Hub shows none. **Banner:** the layout reserves a permanent fixed-height bottom slot while ads are enabled; sheets, dialogs and the lock screen stop above it, so the banner is always visible and never covered, and the slot keeps its height when AdMob has no fill. **Ad exclusion surfaces:** the interstitial checks one shared "ad-exclusion" registry in the UI package. In this epic the registry holds the `tile-sheet`, `settle-bar`, `undo-toast`, `inline-pin`, the More sheet and any dialog. The Hub POS cart and `phone-cart-bar` from Epic 5 add themselves to it, and Epic 4 does not wait for them. **Interstitial:** a standard AdMob interstitial is used (the policy-safest format), so Google controls its close button and timing. The UX spec's custom `ad-interstitial` chrome (solid `bg`, own "Ad" label, "Skip in 5 s" → "Skip ad" countdown, muted start) is **superseded**; DESIGN.md/EXPERIENCE.md `ad-interstitial` and UX-DR80 need updating to match. Its trigger rules stay: only on PIN unlock after an idle lock, never over an open cart or sheet, never after a settle, at most once per 15 minutes. **Owner ad controls** are a per-branch content rating (G · PG · T, default PG, never MA) applied through AdMob's maximum ad content rating request configuration on the Hub device (one Hub per branch). Free uploads **no** batches, summaries or rollups: ad impression counts travel inside the heartbeat. Phone POS (`pos-product-grid` full width, `phone-cart-bar`, full-height cart sheet) is built with the POS in Epic 5 on top of the phone rules from Story 4.5. The Android→Windows upgrade migration (FR183) is Epic 10. The rollup half of NFR5 is measured by the device-farm job from Story 4.17 once the rollup engine exists. Each story lists what it covers on its **Covers** line.

### Story 4.1: Android Hub app builds from the shared Hub code and checks the device spec on first run

As a Free café owner with an Android tablet or phone at the counter,
I want an Android Hub app built from the same code as the Windows Hub that tells me at once if my device is too small,
So that I get the same Hub on cheaper hardware and never set up on a device that will fail later.

**Covers:** AD-15, AD-16 (Tauri 2 mobile, shared UI and core), AD-8, AD-41, NFR17, NFR16; Additional Requirements "STARTER / SCAFFOLD" (Android Hub app), "CI/CD" (signed Android builds), "Signing and distribution" (build half); Stack pins (targetSdk 36, minSdk 28, NDK 28+, Kotlin 2.1+)

**Acceptance Criteria:**

**Given** the monorepo from Story 1.1
**When** CI runs the Android job
**Then** a Tauri 2 mobile Android Hub app builds as a signed release AAB and a debug APK from the same Hub SvelteKit (`adapter-static`) front end and the same `arena-hub` Rust crates as the Windows Hub
**And** the Gradle and Tauri config pin `targetSdk 36`, `minSdk 28`, NDK 28+ and Kotlin 2.1+, and a CI step fails if any pin changes without updating the pin table in the repo README

**Given** the release AAB
**When** CI inspects it
**Then** every native library is aligned for 16 KB memory pages, ABIs `arm64-v8a` and `x86_64` are included, and the upload key comes from SOPS-managed secrets, never from a plain file in the repo

**Given** the Rust core on Android
**When** the app starts
**Then** `arena-hub` runs in-process on its own Tokio runtime thread and opens its SQLite file in app-private storage with the Story 1.4 settings (WAL, `synchronous=FULL`, migrations, `hub_instance_id` minted once)
**And** `/healthz` on loopback reports the same fields as on Windows plus `platform: android`

**Given** a device running Android 8.1 or earlier
**When** someone tries to install the app
**Then** it cannot be installed (minSdk 28)

**Given** a device with less than 3 GB RAM, or less than 20 GB of free storage on the app's data volume
**When** the app opens for the first time, before the pairing screen
**Then** it shows one plain screen naming the requirement that failed and the value found (for example "This device has 14 GB free. The Hub needs 20 GB free."), with "Check again", and does not create the branch database
**And** a device that meets every requirement goes straight to the "Pair this Hub" screen

**Given** the app manifest
**When** CI checks it
**Then** Android auto-backup and device-to-device transfer exclude the database, the licence cache and every key file, so branch data and tokens are never copied to a cloud account or another phone

**Given** the Android System WebView on the device is older than the minimum version recorded in the README
**When** the Hub UI would load
**Then** a plain screen asks the user to update "Android System WebView" from Play, and the Rust core keeps running

**Given** the message-catalogue lint and the axe scan from Epic 1
**When** they run on the Android build's UI bundle
**Then** they pass with the same rules as on Windows, and every new string in this story is in the catalogue

### Story 4.2: The Android Hub keeps serving while the screen is off, in Doze and after a reboot

As counter staff on an Android Hub,
I want the Hub to keep running whatever the tablet does,
So that kiosks never lose their Hub because Android put the app to sleep.

**Covers:** AD-16, AD-1, AD-12, NFR1, NFR9, NFR17; Additional Requirements "Android Hub specifics" (foreground service, WAKE_LOCK + battery-optimisation exemption)

**Acceptance Criteria:**

**Given** the Hub app is started
**When** the Rust core comes up
**Then** it is hosted by an Android foreground service of type `connectedDevice`, not by the activity, with a persistent notification showing branch name and "Hub running · n devices online", updated when the count changes

**Given** the activity is destroyed (rotation, Back to home, swiping the app from recents)
**When** the foreground service is running
**Then** the core keeps serving kiosks without restarting, and reopening the app reattaches the UI to the running core within 2 s

**Given** first-run setup on Android
**When** the owner reaches the setup screen after the spec check
**Then** the Hub asks for notification permission (Android 13+) and for exemption from battery optimisation through the system dialog, explaining in one line why
**And** if either is refused, the Hub keeps working, a `status-badge` "Battery optimisation on" stays in System, and an `alert-badge` in the header opens a screen with a button to the system setting

**Given** the service is running
**When** the screen turns off or the device enters Doze (forced by `adb shell dumpsys deviceidle force-idle` in the test)
**Then** a partial wake lock and a high-performance Wi-Fi lock keep the core and the LAN up, and a fake kiosk sees no missed frame and no reconnect for 30 minutes

**Given** the Hub UI is in the foreground
**When** no one touches the screen
**Then** the screen stays on (keep-screen-on flag), and the Hub idle lock from Story 1.12 still applies after its timeout

**Given** the device reboots, or the app is updated
**When** Android finishes booting or replacing the package
**Then** the foreground service starts by itself without anyone opening the app, and kiosks reconnect by themselves

**Given** Android kills the process (low-memory killer, simulated with `am kill`)
**When** the system restarts the sticky service
**Then** every committed change is present, the database passes `PRAGMA integrity_check`, and the timer heap is rebuilt as on Windows

**Given** staff choose "Stop Hub" in System
**When** a Manager confirms with `inline-pin`
**Then** the service stops, the notification goes, the stop is audited with both actors, and kiosks continue their sessions offline under AD-12; opening the app starts the Hub again

### Story 4.3: Kiosks find and pair with an Android Hub on the LAN

As a café owner with an Android Hub,
I want my gaming PCs to find the tablet Hub by themselves exactly as they would find a Windows Hub,
So that pairing kiosks needs nothing extra on Free.

**Covers:** FR15 (Android Hub side), AD-6, AD-8 (shared mDNS service name), AD-11 (re-resolve), AD-25, AD-42, NFR11, NFR17; Additional Requirements "Android Hub specifics" (mDNS via `NsdManager`, no Tunnel/standby on Android)

**Acceptance Criteria:**

**Given** a paired Android Hub on Wi-Fi or Ethernet
**When** the core opens its listeners
**Then** WebSocket TLS on `:7400` and HTTP(S) on `:7401` are reachable from the LAN, and an in-house Tauri Kotlin plugin registers the `arena-proto` mDNS service through `NsdManager` with the same TXT records as Windows (branch name, Hub name, protocol version, certificate fingerprint)

**Given** an unpaired kiosk on the same LAN
**When** it runs discovery (Story 2.16)
**Then** it lists the Android Hub within 10 s, and pairing by the 6-digit code or manager PIN (Story 2.17) works unchanged, with the kiosk shown as Unassigned within 60 s

**Given** Settings → Devices on the Android Hub
**When** it opens
**Then** it shows the Hub's current LAN IP and the network name, for typed-IP fallback

**Given** the Android Hub's IP changes (DHCP renewal, Wi-Fi reconnect)
**When** the network callback fires
**Then** the plugin re-registers the service with the new address within 5 s, and paired kiosks re-resolve the Hub by mDNS before retrying their stored IP, reconnecting within 60 s with jittered backoff 2–30 s

**Given** the device loses Wi-Fi entirely
**When** the LAN is gone
**Then** the Hub shows an `alert-badge` "No LAN connection" in the header, keeps trading locally, and re-advertises when the network returns

**Given** the Android Hub
**When** staff look for Tunnel remote devices or standby pairing
**Then** neither is offered on Android; where those sections exist they read "Needs the Windows Hub"

**Given** the LAN client-class tests from Story 1.7
**When** they run against the Android Hub in CI on an emulator
**Then** they pass unchanged, including `FORBIDDEN` for out-of-scope browser classes and `TOKEN_REVOKED` closing a socket at once

### Story 4.4: The Android Hub pairs with the wizard code and keeps its secrets in the Android Keystore

As a Free café owner,
I want the setup wizard to send me to the Android Hub and pair it with the same 8-character code,
So that my Free branch gets its Hub without a Windows PC, and its keys stay locked to that device.

**Covers:** FR13 (Free download path), FR14 (Free pairs only Android), AD-10, AD-22 (Android Keystore), AD-29, NFR11, NFR17, UX-DR84, UX-DR85, UX-DR103

**Acceptance Criteria:**

**Given** wizard step 3 for a Free branch
**When** it opens
**Then** "1 · Download" offers the Android Hub (Google Play link from configuration) with the minimum spec "Android 9 or later · 3 GB RAM · 20 GB free", and the Windows download is not offered; the `pairing-code` card is the same as in Story 2.8
**And** for a paid or comp branch the step stays as in Story 2.8, with Windows first and the Android link second

**Given** the Android Hub's "Pair this Hub" screen (the Story 2.9 screen)
**When** a valid code for a Free branch is entered
**Then** `POST /v1/pair` is sent with `platform: android` and accepted, the device token and the branch KEK are stored wrapped by a non-exportable Android Keystore key, and the licence is written to `licence_cache`

**Given** a wrong, used or expired code on Android
**When** "Pair" is pressed
**Then** the field clears, focus stays in it, and the Story 2.9 line "That code didn't work. Get a new code from your browser." shows; the on-screen keyboard stays open

**Given** the Keystore key is lost (app data cleared, or a Keystore reset after a lock-screen change)
**When** the Hub starts
**Then** it does not crash or open a blank branch; it shows "This Hub needs to be paired again" with the pairing screen, and the database file is left untouched for restore

**Given** the first heartbeat from the Android Hub
**When** the cloud records it
**Then** the wizard advances to step 4 within 5 s (Story 2.10), `hubs.platform = android` is shown on Account → Branches, and `branch.bootstrap` and the owner's setup code (Stories 2.12–2.13) work on the tablet with the on-screen keyboard

**Given** the stored token and KEK
**When** a test reads the app's private files
**Then** neither appears in clear, and the Keystore key cannot be exported from the device

### Story 4.5: The Hub works on an Android phone and tablet by touch

As counter staff using a phone or tablet as the Hub,
I want every Hub screen to fit, scroll and respond to touch and the Back button without hiding what I'm working on,
So that a small screen never slows me down at the counter.

**Covers:** FR202 (≥ 44 px incl. Android phone Hub), FR24 (sheet on phone), NFR15, UX-DR17 (scroll padding under sticky bars), UX-DR59, UX-DR60 (phone header), UX-DR71 (settle bar on phone), UX-DR87 (phone layout), UX-DR104, UX-DR109

**Acceptance Criteria:**

**Given** the Hub on a window below 840px wide
**When** the header renders
**Then** it is 64px with branch name, `alert-badge` count and signed-in staff; backup status and clock are dropped, and `chip-rule`/`chip-offline` move to a row under the header (per the phone mock)

**Given** a phone below 840px
**When** staff open a tile
**Then** the `tile-sheet` opens as a full-height sheet over the floor, its `settle-bar` pinned at the bottom of that sheet, and Android Back or Esc closes it and returns focus to the tile
**And** at 840px and wider the sheet docks right as in Story 3.5

**Given** the More sheet, a tile sheet or the lock screen is open
**When** the Android Back gesture or button is used
**Then** it closes the topmost sheet only, never exits the app or navigates away from an unsaved field; on the floor with nothing open, Back does nothing

**Given** Android 15 edge-to-edge enforcement
**When** the Hub renders on a device with a display cutout, gesture bar or 3-button bar
**Then** no control, chip or bar sits under the status bar, cutout or navigation bar (system insets applied), checked on a phone and a tablet in portrait and landscape

**Given** a text field or `inline-pin` gets focus
**When** the on-screen keyboard opens
**Then** the focused field scrolls above the keyboard, `inline-pin` opens the numeric keypad, and sticky bars never cover the focused element (scroll padding equals their height)

**Given** the device rotates, or a tablet is split-screened across 840px
**When** the layout switches between rail and `hub-bottom-bar`
**Then** the current screen, scroll position, open sheet and unsaved field contents are kept

**Given** the system font size is raised
**When** the Hub renders
**Then** text scales up to 130% with no clipped or overlapping text, and larger system settings are capped at 130%

**Given** an automated target-size and axe check on the phone (412 × 915) and tablet (1280 × 800) layouts of Floor, tile sheet with settle bar, Players, Shift placeholder and More sheet
**When** it runs in CI
**Then** every target is at least 44 × 44 px and there are zero serious or critical violations

### Story 4.6: The Android Hub prints over Bluetooth or LAN

As a Free café owner on an Android Hub,
I want to connect my receipt printer by Bluetooth or the network and print a test,
So that I can finish the printer step on Android just as on Windows.

**Covers:** FR10 (Android), AD-28 (Bluetooth printing plugin), AD-34, NFR18, UX-DR95

**Acceptance Criteria:**

**Given** Settings → Printer on the Android Hub
**When** a Manager presses "Find printer"
**Then** the Hub lists Bluetooth printers already paired in Android settings (through an in-house Tauri Kotlin Bluetooth SPP plugin behind the printer adapter) and LAN printers on port 9100 (from the Rust `printer` task), plus the manual IP field; USB is shown as "Not supported on Android"

**Given** Bluetooth permission (Android 12+) has not been granted
**When** "Find printer" is pressed
**Then** the Hub asks for it with a one-line reason; if refused, the Bluetooth list shows "Bluetooth permission is off" with a link to settings and the LAN search still runs

**Given** a selected Bluetooth or LAN printer and paper width 58 or 80 mm
**When** "Test print" is pressed
**Then** the Story 2.26 test page prints, the result is recorded as setup status ok or failed, and a printer that is off or out of range fails within 10 s with a plain error and never blocks the Hub

**Given** the Bluetooth printer drops mid-print
**When** the plugin reports the failure
**Then** the outbox result records it as failed, the header shows the printer-offline `alert-badge`, and a later print retries the connection

**Given** the Bluetooth plugin
**When** it is tested in CI
**Then** it runs against the printer adapter's recorded fake, and "Skip printer for now" (Story 2.26) works on Android exactly as on Windows

### Story 4.7: The Android Hub saves backups to a folder the owner chooses

As a Free café owner on an Android Hub,
I want to choose where backups go, including a USB drive,
So that I can finish the go-live checklist on Android and keep my data off the tablet.

**Covers:** FR115 (backup location on Android), FR11 (go-live input on Android), AD-22, NFR17

**Acceptance Criteria:**

**Given** Settings → System → Backup location on Android
**When** a Manager picks a location
**Then** the system folder picker opens (Downloads, internal storage or a USB OTG drive), the Hub keeps persistent permission to that folder, writes and deletes a probe file to check it, and shows an inline error if the folder is not writable

**Given** a writable folder is chosen
**When** the Hub computes setup status
**Then** backup location reports set, and the change is audited with the staff id

**Given** the chosen USB OTG drive is unplugged or the permission is revoked
**When** the Hub next checks the location (at start and hourly)
**Then** setup status reports backup location not set, and System shows "Backup folder not available" with "Choose again"

**Given** a Counter without settings permission
**When** they try to change the location
**Then** `inline-pin` opens for manager elevation and the change is audited with both actors

### Story 4.8: A Free café goes from sign-up to go-live on Android with no help

As a Free café owner,
I want the whole setup, from signing up to going live, to work with only an Android Hub,
So that I can open my café on Free without a Windows PC, a comp grant or a support call.

**Covers:** FR6 (Free path end to end), FR11 (go live on Android), FR14 (Windows refused, Android accepted), FR4, NFR27 (onboarding telemetry on Android), NFR17

**Acceptance Criteria:**

**Given** an end-to-end test on an Android emulator or device-farm device with a Windows kiosk VM on the same virtual LAN
**When** it runs sign-up → Free → pair the Android Hub → owner first sign-in → pair a kiosk → group it in the wizard → set a rate → skip the printer → choose a backup folder → tick ad consent → 2-minute test session → go live
**Then** every step completes with no comp entitlement, the kiosk locks at zero, and the branch shows Live

**Given** the same Free branch
**When** the test first tries to pair a Windows Hub with the step 3 code
**Then** it is refused `FEATURE_NOT_ENTITLED`, the code is not consumed, and the same code then pairs the Android Hub

**Given** the go-live checklist on that branch
**When** step 10 opens
**Then** opening hours, backup location and ad consent show as done from the Android Hub's setup status, and standby PC shows "Paid tiers only" without blocking

**Given** the completed run
**When** `onboarding_progress` is read
**Then** it holds every step time from sign-up to go-live with no PII, the same as on Windows

**Given** the end-to-end job
**When** CI schedules it
**Then** it runs nightly and on every release candidate, and a failing step fails the release with the step name and a screenshot

### Story 4.9: Free limits — 3 users, 20 devices and one branch — enforced and explained at the Hub

As a Free café owner,
I want the Hub to show how many users and devices I have left and stop clearly at the Free limits,
So that I understand what Free includes and where to upgrade.

**Covers:** FR107, FR20 (Free device cap on Android), AD-29, AD-10 (limits from the licence), NFR11, UX-DR24, UX-DR88

**Acceptance Criteria:**

**Given** a Free licence with `limits.users = 3`
**When** `apply()` processes a `staff.upsert` that would add a user
**Then** it counts staff rows that are active and not deleted, including the Owner and invited users who have not yet used their setup code, and excluding the Kiosk-device role; a fourth is rejected `USER_LIMIT` in the permission step and nothing is written

**Given** Settings → Staff on the Hub (Owner or Manager)
**When** it opens
**Then** it lists users with name, role and status (active / invited), shows "2 of 3 users" in `hub-label`, and at 3 of 3 shows `tier-lock` "Available on Basic" with an underlined "Upgrade" link

**Given** an Owner on Settings → Staff
**When** they deactivate a staff member, confirming with their PIN
**Then** a `staff.deactivate` intent ends that person's Hub sign-in at once, frees a slot, and is audited with both actors; the Owner cannot deactivate themselves or the last Owner

**Given** a Free branch at 20 devices
**When** a kiosk tries to pair with the Android Hub
**Then** the Story 2.18 `DEVICE_LIMIT` refusal and `tier-lock` apply unchanged, verified on Android

**Given** a Free tenant
**When** the owner tries to add a second branch (Story 2.29) or pick Free for another branch (Story 2.7)
**Then** both are refused as before, and an API test confirms the cloud rejects a second Free branch even when called directly

**Given** a licence whose `limits` claim is missing or fails verification
**When** the limits are read
**Then** the Free defaults from the `arena-proto` tier table apply (users 3, devices 20, branches 1)

### Story 4.10: In-house AdMob plugin and the ads gate, with no ad code in Rust or on Windows

As the Arena 360 team,
I want ads to reach the Hub only through one small Kotlin plugin that switches on solely for Free on Android,
So that ads can never leak into Rust, the Windows Hub, a kiosk or a paid branch.

**Covers:** FR186 (Windows build has no ad SDK), AD-16, AD-29 (hub-android ads), NFR11, NFR17; Stack pin (Google Mobile Ads SDK 25.5.x)

**Acceptance Criteria:**

**Given** the Android Hub project
**When** the plugin `tauri-plugin-arena-ads` is built
**Then** it is a Kotlin module in the repo depending on Google Mobile Ads SDK 25.5.x (pinned), exposing only `init`, `banner.place(rect)`, `banner.hide`, `banner.destroy`, `interstitial.preload`, `interstitial.show`, and events `ad.loaded`, `ad.failed`, `ad.shown{slot}`, `ad.clicked`, `ad.dismissed`
**And** SDK initialisation and ad loads run off the main thread, verified by StrictMode in debug builds reporting no main-thread network or disk access from the plugin

**Given** the UI package
**When** the ads gate is evaluated
**Then** ads are enabled only when the platform is Android **and** the Hub's effective entitlement (Story 2.11) is Free, which includes the Free fallback for a failed licence check; the Hub emits `entitlement.changed` to the UI whenever the effective tier changes, and the gate re-evaluates on it without a restart

**Given** the gate is off (paid Android Hub, or any Windows Hub)
**When** the app runs
**Then** the plugin is never initialised, no ad request is made, and no ad slot is rendered

**Given** ad unit IDs
**When** CI builds debug and release
**Then** debug builds use Google's test ad unit IDs and the AdMob app ID comes from build config; CI fails if a release build contains a test unit ID or a debug build contains a production unit ID

**Given** the Rust workspace
**When** the CI ad-code lint runs
**Then** it fails if any crate depends on an ads/AdMob crate or contains ad symbols or ad endpoints, and the plugin is the only module that imports `com.google.android.gms.ads`

**Given** the Windows Hub and Kiosk builds
**When** CI inspects their artefacts and Tauri plugin lists
**Then** neither includes the ads plugin or any ad SDK or ad endpoint string, and the shared ad components are no-ops when the platform is not Android

**Given** a dev-only gallery route on a debug Android build
**When** a developer opens it
**Then** a test banner and a test interstitial can be loaded and shown, and each plugin event is listed as it fires

### Story 4.11: An always-visible banner in a permanent slot that nothing covers

As counter staff on a Free Android Hub,
I want the ad banner to sit in its own permanent strip that never covers anything and is never covered,
So that ads never slow a sale or cause a mis-tap.

**Covers:** FR184, UX-DR79, UX-DR17 (scroll padding), UX-DR87, UX-DR104, UX-DR109

**Acceptance Criteria:**

**Given** ads are enabled
**When** any Hub screen renders below 840px
**Then** the layout reserves a permanent fixed-height `ad-banner` slot (plain `surface` strip, 1px `line` edge, "Ad" label in `hub-micro` muted) between the main content and `hub-bottom-bar`, as in the phone mock, and the plugin places an anchored adaptive banner inside exactly that rect

**Given** ads are enabled at 840px and wider
**When** a screen renders
**Then** the permanent slot spans the bottom of the window below the main content and the `tile-sheet` column, never under the rail, with at least 24px between the slot and a docked `settle-bar` so they never share an edge

**Given** a `tile-sheet` with `settle-bar`, the More sheet, a dialog, `inline-pin` or the idle lock screen opens
**When** it lays out
**Then** it stops above the banner slot (a full-height phone sheet ends at the slot's top edge, with at least 24px between its `settle-bar` and the slot); no app surface ever draws over the slot and the banner never draws over app content, so the banner stays visible on every screen

**Given** the banner slot
**When** content scrolls
**Then** the floor grid, lists, sheets and forms scroll above the slot and never under it, and scroll padding equals the slot height so no focused element is ever hidden behind it

**Given** no ad is available (no fill, WAN down, load error)
**When** the slot renders
**Then** the strip keeps its height with only the "Ad" label, so nothing shifts under a finger; the slot never collapses or grows while ads are enabled

**Given** the app goes to the background or the screen turns off
**When** the activity pauses
**Then** the banner is paused, makes no requests, and resumes when the activity resumes

**Given** a banner tap
**When** the ad opens in the browser
**Then** the Hub core keeps serving kiosks, and returning shows the same screen with its state kept

**Given** an automated layout test at 412 × 915, 800 × 1280 and 1280 × 800 on Floor, a tile with the settle bar open, the More sheet, a dialog, the idle lock screen and Settings
**When** it compares the banner rect with every tile, sheet, dialog, the `settle-bar` and the bottom bar
**Then** no intersection exists in any case and the banner rect is fully visible, and the test fails otherwise

### Story 4.12: Safe ad categories, a per-branch content rating and the ads consent view

As a Free café owner,
I want ads on my counter screen to stay away from adult, gambling and alcohol content and to let me make them stricter,
So that nothing embarrassing shows at the counter in front of my customers.

**Covers:** FR186 (excluded categories, owner blocks more, consent view), FR115 (ads consent), UX-DR24, UX-DR88 (Settings → Ads)

**Acceptance Criteria:**

**Given** the AdMob account for the Hub app
**When** the release checklist runs
**Then** the sensitive categories adult/sexual, gambling and betting, and alcohol are blocked at app level, recorded as a versioned config note in the repo and checked by a named person before each release

**Given** Settings → Branch → Ads on a Free branch (the Story 2.25 consent view)
**When** an Owner or Manager opens it
**Then** it shows what is always excluded (adult, gambling, alcohol), a per-branch content rating choice G · PG · T labelled in plain words (default PG; MA is never offered), the ad policy link, and the consent tick with time and staff id; there is no control to turn ads off, only "Ads keep Arena 360 free. Upgrade to remove them." with an underlined "Upgrade" link

**Given** the branch content rating
**When** the plugin initialises or the rating changes
**Then** it sets the AdMob request configuration's maximum ad content rating to that value on the Hub device (one Hub per branch, so the setting is per branch), and every banner and interstitial request uses it; a stored value of MA or an unknown value is treated as PG

**Given** a stricter content rating is saved
**When** `settings.set` applies
**Then** it is audited, and the next ad request uses it without a restart

**Given** the ads consent tick is not yet recorded
**When** ads are requested
**Then** they are requested as non-personalised; after consent, standard requests are used

**Given** a Counter without settings permission
**When** they change the content rating
**Then** `inline-pin` opens for manager elevation and the change is audited with both actors

### Story 4.13: A standard interstitial only when staff come back from the idle lock

As counter staff on a Free Android Hub,
I want any full-screen ad to appear only when I come back to an idle Hub, and never mid-task,
So that ads never interrupt a sale or a session I'm handling.

**Covers:** FR185, FR106 (idle-lock return), UX-DR80 (trigger rules; custom skip UI superseded), UX-DR102 (interstitial due), UX-DR104

**Acceptance Criteria:**

**Given** ads are enabled and the Hub UI locks after its idle timeout (Story 1.12)
**When** the lock screen shows
**Then** the plugin preloads one standard AdMob interstitial while locked, without blocking the lock screen or the core

**Given** staff unlock with a correct PIN
**When** an interstitial is loaded, at least 15 minutes have passed since the last one, and no ad-exclusion surface was open at lock time
**Then** the plugin shows the standard AdMob interstitial, whose close control and timing are Google's (no custom Skip countdown or overlay is drawn), and when it is dismissed the Hub shows the exact screen from before the lock

**Given** a `tile-sheet`, `settle-bar`, `undo-toast`, `inline-pin`, dialog or any other ad-exclusion surface (including the Hub cart once it registers) was open when the lock started
**When** staff unlock
**Then** no interstitial shows (it is skipped, not delayed), and unlock returns straight to that surface

**Given** a settle
**When** it commits, its undo window runs, or the bill closes
**Then** no interstitial is ever triggered; the only trigger is a PIN unlock after an idle lock

**Given** no interstitial is loaded (no fill, WAN down, load still running), or the last one showed less than 15 minutes ago
**When** staff unlock
**Then** the Hub returns at once; unlock never waits for an ad

**Given** the interstitial is showing
**When** a kiosk alert arrives (call staff, time up, login lockout)
**Then** the alert sound plays, its `alert-badge` is waiting when the ad closes, and the core keeps applying intents throughout

**Given** the interstitial closes, fails to show, or the user leaves through the ad
**When** the Hub UI returns to the foreground
**Then** focus returns to the element focused before the lock, and the idle timer restarts from zero

### Story 4.14: Ad impressions are recorded without ids and sent with the heartbeat only

As the Arena 360 team,
I want to count ad impressions per Free branch without collecting device or creative ids,
So that we can track ad revenue health while Free branches upload nothing else.

**Covers:** AD-16 (`ad.shown` → `ad_impressions`), AD-2, AD-36 (Free uploads only heartbeat, ad impressions, catalogue pull), AD-9, NFR13, NFR20, NFR25, NFR27 (ad metrics instrumentation); Inherited Constraint "ad rows carry no kiosk device/creative ids"

**Acceptance Criteria:**

**Given** the migration added by this story
**When** the Hub schema is inspected
**Then** `ad_impressions(id, slot CHECK IN ('banner','interstitial'), at)` exists with no device, creative, advertiser or advertising-ID column, plus a single-row `ad_upload_cursor(through_id)`

**Given** the plugin emits `ad.shown{slot}` on the SDK impression callback
**When** the UI forwards it
**Then** a system intent `ad.impression_recorded{slot}` with an idempotency key from the event is applied through `apply()`, writing one row; a duplicate event writes nothing

**Given** a heartbeat from a Free branch
**When** it is built
**Then** it carries `ad_impressions{through_id, counts: [{business_day, slot, n}]}` for rows after `ad_upload_cursor`, aggregated by day and slot, and no other branch data; `last_seq_uploaded` stays 0 and the Hub makes no presign, batch, summary or rollup call

**Given** the cloud handles that heartbeat
**When** `through_id` is beyond the branch's stored cursor
**Then** it adds the counts to `gz_control.ad_impression_daily(branch_id, day, slot, count)` and stores the new cursor in the same transaction, and the response returns the acknowledged `through_id`, which the Hub saves to `ad_upload_cursor`
**And** a repeated heartbeat with a `through_id` at or below the stored cursor adds nothing

**Given** the WAN is down for days
**When** it returns
**Then** the next heartbeat sends every pending day's counts once, and no impression is lost or double-counted (integration test with a 3-day outage)

**Given** the heartbeat payload and Hub logs
**When** a test inspects them
**Then** they contain no advertising ID, creative ID, ad unit response data, kiosk device id or player data

**Given** the cloud metrics
**When** impressions arrive
**Then** daily impressions per slot across Free branches are exported to Grafana Cloud with no tenant names, ready for the ad-fill business metric

### Story 4.15: Upgrading removes ads within one heartbeat, and a lapse brings them back only after expiry

As a café owner,
I want ads to disappear as soon as my branch is paid and to come back only if my paid entitlement actually ends,
So that I get what I pay for straight away and am never shown ads while I'm still covered.

**Covers:** FR186 (upgrade removes ads within one heartbeat; lapsed branch resumes ads only after entitlement expiry), AD-10, AD-29, AD-44, NFR22

**Acceptance Criteria:**

**Given** a Free Android Hub showing the banner
**When** an operator grants a comp Basic entitlement (Story 2.7) and the next heartbeat returns the new licence
**Then** within that heartbeat the gate turns off: the banner is destroyed, its slot is removed and the layout reflows without moving focus, any preloaded interstitial is discarded, and no further ad request is made

**Given** a paid Android Hub
**When** it starts with the WAN down and a valid cached paid licence
**Then** no ad is shown at any point

**Given** a paid (comp) entitlement that ends
**When** the cloud signs the next licence as Free after the entitlement end
**Then** ads come back after that heartbeat; while the paid licence is still current on `trusted_now`, ads stay off even if the wall clock is moved forward

**Given** a paid licence that passes its `exp` on `trusted_now` with the WAN down
**When** the Hub re-evaluates the entitlement
**Then** the effective tier becomes Free, the gate turns on, and ads appear once the network allows a load

**Given** a tampered or unverifiable licence on an Android Hub
**When** the Hub falls back to Free limits (Story 2.11)
**Then** ads are enabled as for Free

**Given** each gate change
**When** it happens
**Then** an audit row records the old and new effective tier and the time, with `actor{kind: system}`

### Story 4.16: The Android Hub ships through Google Play with a staged rollout that never interrupts a session

As the Arena 360 team,
I want the Android Hub released through Play to the right devices in stages, and installed without cutting a live session,
So that Free cafés get updates safely and a bad build can be stopped early.

**Covers:** AD-32 (Play with staged rollout), AD-8 (rollout order), NFR17, NFR19; Additional Requirements "Signing and distribution", "Rollout order" (Android Play staged rollout), release train (Hub Android every two weeks)

**Acceptance Criteria:**

**Given** a release tag
**When** the CI release job runs
**Then** it uploads the signed AAB to the Play internal testing track through the Play Developer API with Play App Signing, using credentials from SOPS-managed secrets, after cloud release N is live (cloud first)

**Given** the Play listing
**When** it is configured
**Then** it declares "Contains ads", a data safety form matching the ad SDK's data use, the foreground-service type declaration, and device catalogue rules excluding devices with less than 3 GB RAM; minSdk excludes Android 8.1 and earlier

**Given** a build promoted to production
**When** the release manager starts the rollout
**Then** it goes 1% → 10% → 50% → 100% over a week by Play staged rollout, and the runbook says to halt when the Play crash rate or the fleet heartbeat-gap metric for that version goes above the recorded threshold

**Given** an update is available on the Hub
**When** the Hub checks through the Play in-app updates API
**Then** it downloads in the background and offers "Update ready" in System, and installs only when no session is active, paused or in grace; an update that Play forces while sessions run restarts the Hub service by itself after install, and kiosks continue offline meanwhile (AD-12)

**Given** the Play pre-launch report
**When** a release candidate is tested
**Then** it has no crashes or accessibility errors on the tested devices before it can go past internal testing

### Story 4.17: Android Hub proven on a 3 GB device with 20 kiosks, Doze and no WAN

As a Free café owner on a budget tablet,
I want the Android Hub proven to run a full floor on the cheapest supported device,
So that I can trust it on a busy evening with ads on screen.

**Covers:** NFR1, NFR3, NFR5 (3 GB Android device; summary/rollup timing joins when those engines exist), NFR9, NFR17, AD-33; LLD Test plan "Android Hub"

**Acceptance Criteria:**

**Given** a device-farm device with Android 9 and 3 GB RAM, and a second with the latest Android version
**When** the per-release Android job runs with 20 fake kiosks over Wi-Fi
**Then** a 60-minute mixed load of start, pause, extend, transfer, end and settle intents at 5/s keeps server-side apply under 100 ms at p95 and floor updates within 2 s, with no intent lost or duplicated; the job fails the release otherwise

**Given** the same load
**When** the device is forced into Doze and the screen is off for 30 minutes
**Then** the foreground service keeps serving, every kiosk stays connected, and timers and time-up locks fire on time

**Given** the banner and interstitial are active during the load
**When** ads load, show and fail
**Then** apply latency stays within the same budget and no UI frame blocks for longer than 100 ms while an ad loads

**Given** all WAN egress blocked
**When** the Epic 1–3 flows run on the Android Hub (sign-in, idle lock, PIN elevation, audit, settings, profiles, every session mode, settle, undo, credit, plans, pricing rules, console time-up)
**Then** results match a WAN-up run, the banner shows its empty strip, and no outbound connection is attempted other than the connectivity probe and ad requests

**Given** the process killed during the load
**When** the service restarts
**Then** every committed change is present, the timer heap is rebuilt and kiosks re-sync to the Hub's record

**Given** the job's measurement hooks
**When** a later story adds a measured operation (for example the daily rollup)
**Then** it is timed on the 3 GB device against its NFR budget in the same job without changing the harness
