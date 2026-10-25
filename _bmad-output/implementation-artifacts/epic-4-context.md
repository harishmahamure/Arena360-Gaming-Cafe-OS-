# Epic 4 Context: Free cafés run on an ad-supported Android Hub

## Goal

A Free café runs its branch on an Android tablet or phone: the same Hub UI and Rust core as the Windows Hub, hosted by Tauri 2 mobile. Epic 4 adds only what Android needs (app build and device-spec checks, foreground service, mDNS through `NsdManager`, Android Keystore, Bluetooth printing, folder picker, phone layout) and the Free tier's ads. Until it lands a Free branch cannot pair any Hub (Story 2.10 refuses Windows on Free); Story 4.8 proves sign-up to go-live on Android with no comp entitlement.

## Rules that shape every story

- Android Hub = Tauri 2 mobile sharing the Hub SvelteKit UI (`apps/hub`) and the `arena-hub` crates. No second UI, no Kotlin shell. Stack pins: Tauri 2.12.5, targetSdk 36, minSdk 28, NDK 28+, Kotlin 2.1+; changing a pin updates the README pin table (`pnpm run check:pins`).
- Device floor: Android 9+ (minSdk 28), 3 GB RAM, 20 GB free on the app data volume. A device that fails shows one plain screen before pairing and creates no branch database.
- Ads only on the Free Android Hub, through the in-house Tauri Kotlin plugin `tauri-plugin-arena-ads`. No ad code in Rust crates; the Windows build ships without the plugin. Banner in a permanent bottom slot; standard AdMob interstitial only on idle return, at most once per 15 min; content rating G/PG/T (default PG).
- Free uploads no batches, summaries or rollups; ad impression counts travel in the heartbeat.
- Tunnel remote devices and standby pairing are not offered on the Android Hub.
- Backups, database, licence cache and key files never leave the device through Android auto-backup or device transfer.
- Secrets: the Play upload key is SOPS-managed, never a plain file in the repo. Hub/device tokens live in the Android Keystore (Story 4.3).

## Stories

- 4.1 Android Hub app builds from the shared Hub code and checks the device spec on first run
- 4.2 Foreground service: serves with the screen off, in Doze and after a reboot
- 4.3 Pairs with the wizard code and keeps its secrets in the Keystore
- 4.4 Kiosks find and pair with an Android Hub on the LAN
- 4.5 Phone and tablet by touch
- 4.6 Bluetooth or LAN printing
- 4.7 Backups to an owner-chosen folder
- 4.8 Free café sign-up to go-live on Android
- 4.9 Free limits enforced
- 4.10 to 4.15 AdMob plugin, banner, categories and rating, interstitial, impression counts, upgrade/lapse
- 4.16 Play distribution with staged rollout
- 4.17 Proof on a 3 GB device with 20 kiosks

## Story 4.1 foundations later stories build on

- `apps/hub/src-tauri` is a lib (`arena_hub_ui_lib::run`, `tauri::mobile_entry_point`) plus a thin `main.rs`; the Android Gradle project is committed under `apps/hub/src-tauri/gen/android`.
- `arena-hub` is a lib plus bin; Android runs `arena_hub::start` in-process on its own Tokio runtime thread. `/healthz` reports `platform`.
- `arena_proto::device_spec` holds the floor and the pure check; the shell probes `/proc/meminfo` and `statvfs`.
- `scripts/check-android.mjs` checks the manifest backup rules and inspects the release AAB (16 KB ELF alignment, ABIs).
