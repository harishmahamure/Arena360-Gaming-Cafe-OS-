---
title: 'Story 4.3: The Android Hub pairs with the wizard code and keeps its secrets in the Android Keystore'
type: 'feature'
created: '2026-10-28'
status: 'done'
baseline_commit: '5de05542428cf079dd354f28a0fa50c96eacde2a'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-2-10-hub-installer-pairs-with-the-code-and-retries-in-place.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-2-the-android-hub-keeps-serving-while-the-screen-is-off-in-doz.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** A Free branch cannot pair any Hub: wizard step 3 offers only Windows, and the Android Hub's `AndroidKeystore` is a stub that refuses to store anything, so the in-process Hub keeps its token and branch key as plain 0600 files.

**Approach:**
- Wizard step 3 becomes tier-aware: Free sees the Android Hub (Play link from configuration, minimum spec line) and no Windows download; paid and comp keep Windows first and Android second. Account → Branches shows the Hub platform.
- The Android Hub wraps the device token and branch KEK with a non-exportable Android Keystore key (AES-GCM, Kotlin) reached from Rust through an injected wrapper trait. A lost key is detected at start and shows "This Hub needs to be paired again" with the pairing screen. `branch.db` is untouched and a re-pair replaces the token.

## Boundaries & Constraints

**Always:**
- `POST /v1/pair` and the Free/Windows refusal are unchanged; Android on Free already pairs (Story 2.10). Reuse the 2.10 pairing screen, its error line and focus behaviour.
- Secrets in `hub-token.bin` and `branch-kek.bin` on Android are wrapped bytes only. Windows (DPAPI) and dev (`FileKeystore`) behaviour is unchanged.
- The wrapper is a trait in `arena-tasks` (no `jni` there; layers check stays green). The Android shell injects the JNI-backed implementation into `arena_hub::start`; tests inject a fake.
- Lost key means: the wrapped file cannot be unwrapped, or is missing while a `hub_identity` exists. Detect it at start, before any KEK can be minted.
- While lost, every `/ui/*` route except pairing and the minimal snapshot answers `HUB_NOT_PAIRED` (reuse the 2.10 gate). The snapshot carries a flag the UI uses for the "needs to be paired again" copy. Heartbeat and command tasks keep logging "no hub token" and do nothing.
- Re-pair while lost: replaces the token, `hub_identity` hub_id and tier, and the licence, and mints a fresh KEK; the database file is not opened for write beyond that transaction, and nothing is deleted. Normal pairing of an already-paired healthy Hub stays 409 `ALREADY_SET`.
- Every new string lives in `en.json`; no hex colours, tokens only.
- Pairing button must not steal the on-screen keyboard: keep the input focused after a failed code (prevent focus moving to the button on pointer down).

**Never:**
- Exporting key material from Kotlin, logging a token or KEK, or writing either in clear to the app's files on Android.
- Silently minting a new KEK for an existing branch.
- Opening the Windows download for a Free branch in the wizard.
- Owner-side "replace a failed Hub" code issuing (Story 9.8), KEK escrow/restore (9.2), mDNS (4.4), Bluetooth (4.6).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Wizard step 3, Free | tier free | "1 · Download" shows the Android Hub (Play link) and "Android 9 or later · 3 GB RAM · 20 GB free"; no Windows link; same `pairing-code` card | N/A |
| Wizard step 3, paid/comp | tier basic+ or comp | Windows first, Android link second, as 2.9 | N/A |
| Pair ok on Android | valid Free code, platform android | token and KEK stored wrapped; `hub_identity` and `licence_cache` written | N/A |
| Bad, used or expired code | typed on Android | field clears, focus stays, 2.10 line shows; keyboard stays open | N/A |
| Key lost | wrapped file fails to unwrap or is missing with identity present | Hub starts; snapshot flags re-pair; UI shows "This Hub needs to be paired again" with the pairing screen; `branch.db` bytes unchanged | N/A |
| Re-pair after loss | valid new code | new token and licence stored wrapped, flag cleared, shell loads, old data intact | invalid code: same 2.10 handling |
| First heartbeat | android Hub | wizard advances; Account → Branches shows platform Android | N/A |
| Files inspected | read the data dir | token and KEK bytes absent in clear | N/A |

</intent-contract>

## Code Map

- `apps/onboarding/src/lib/InstallHubStep.svelte` (lines ~88-97 download block), `apps/onboarding/src/env.ts` (`PUBLIC_HUB_DOWNLOAD_URL`; add the Play URL), `routes/setup/[step]/+page.svelte`, `tests/install_hub.spec.ts`, `tests/stub.ts` (~552-580), `tests/a11y.spec.ts:315-331`.
- `crates/arena-proto/src/pairing.rs:67` `InstallHubState` (add `tier`); `crates/arena-cloud/src/api/wizard_install.rs:57-82` `get_install_hub`; `crates/arena-cloud/tests/pairing_code.rs:382-394` (exact JSON assertion); regenerate with `pnpm gen:types`.
- `crates/arena-proto/src/wizard.rs:324-346` `BranchHub` (add `platform`); `crates/arena-cloud/src/api/branches.rs:31-57` `row()` (loads `HubRow.platform`); `apps/owner/src/lib/BranchCard.svelte`, `apps/owner/tests/branches.spec.ts`, `tests/stub.ts`.
- `packages/ui/src/messages/en.json` (`onboarding.install.*` ~1156-1170, `hub.pair.*` ~218, new keys).
- `crates/arena-tasks/src/keystore.rs`: `Keystore` trait, `named_keystore`, `KeystoreKek`, `AndroidKeystore` stub (replace; remove test `the_android_stub_refuses`), `write_private`. `crates/arena-tasks/src/cloud.rs:249-390` `map_failure`, `CloudPairer::run` (add lost-key re-pair path). `crates/arena-tasks/src/lib.rs:27` exports.
- `crates/arena-hub/src/lib.rs`: `start` (identity read ~212-218, `PairGate`, keystore built ~342 and ~351; accept an injected wrapper in the start args). `crates/arena-hub/src/config.rs:19-23` platform default.
- `crates/arena-core/src/pairing.rs` `PairGate`; `crates/arena-core/src/status.rs:67-71` `PairingState`; `crates/arena-core/src/writer.rs:241` `record_pairing`; `crates/arena-store/src/identity.rs` (`insert_hub_identity` is `ON CONFLICT DO NOTHING`; add a replace for re-pair).
- `crates/arena-api/src/ui_pair.rs` (`pair_gate`, `unpaired_snapshot`, `PAIR_EXEMPT_PATHS`), `crates/arena-api/src/health.rs:197-209` `ui_snapshot`; `crates/arena-proto/src/snapshot.rs:52` `HubSnapshot` (add the re-pair flag); regenerate types.
- `apps/hub/src/routes/pair/+page.svelte`, `apps/hub/src/lib/pair.ts`, `apps/hub/src/routes/+layout.svelte` (unpaired gate ~77, 190-199, 574-581), `apps/hub/tests/pair.spec.ts:131`.
- `apps/hub/src-tauri/src/android.rs` (JNI `nativeStart`; cache the `JavaVM` and a global ref to the Kotlin class here), `src/lib.rs`; Kotlin under `gen/android/app/src/main/java/in/arena360/hub/` (`HubService.kt`, add `KeystoreBridge.kt`); `AndroidManifest.xml` (reported duplicate `<service>`/`<receiver>`/permission entries: verify and remove duplicates), `res/xml/backup_rules.xml`, `data_extraction_rules.xml`.
- `scripts/check-android.mjs` and `scripts/check-android.test.mjs`: add a Keystore rule (the bridge uses `AndroidKeyStore`, never exports the key; backup rules still exclude the data dir).
- `crates/arena-hub/tests/pairing.rs`: model for Hub-level tests (`start_cloud`, `start_hub`).
- README Android section and `_bmad-output/implementation-artifacts/deferred-work.md`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto`, `arena-cloud`: `InstallHubState.tier` and `BranchHub.platform`; update the cloud handlers and the exact-JSON test; `pnpm gen:types`.
- [x] `apps/onboarding`: tier-aware download block, Play URL env, spec line string; update stub and tests (Free, paid, comp).
- [x] `apps/owner`: show platform on the branch card; test.
- [x] `arena-tasks`: `SecretWrapper` trait, `WrappedKeystore` (wrapped file, "lost" error kind), `AndroidKeystore` built on it; unit tests with a fake wrapper (round trip; file bytes lack the secret; wrong wrapper key means lost).
- [x] `arena-hub`/`arena-core`/`arena-store`/`arena-api`: inject wrapper, lost-key detection at start, gate and snapshot flag, re-pair path replacing token, identity and licence; Hub-level tests (pair, lose key, restart, `branch.db` hash unchanged, re-pair, KEK not minted while lost).
- [x] `apps/hub`: "This Hub needs to be paired again" state on the pairing screen, button focus fix; en.json; Playwright tests including axe.
- [x] Android shell: `KeystoreBridge.kt` (AES-GCM, `AndroidKeyStore`, key never leaves), JNI wrapper in `android.rs`, manifest cleanup; `check-android.mjs` rule and tests; README, deferred-work entries.

**Acceptance Criteria:**
- Given wizard step 3 for a Free branch, when it opens, then "1 · Download" offers the Android Hub with "Android 9 or later · 3 GB RAM · 20 GB free" and no Windows download, and a paid or comp branch still shows Windows first and Android second.
- Given the Android Hub's pairing screen, when a valid Free code is entered, then it pairs with `platform: android`, and the token and KEK are stored wrapped by a non-exportable Keystore key and the licence is cached.
- Given a wrong, used or expired code, when "Pair" is pressed, then the field clears, focus stays in it, the 2.10 line shows and the keyboard stays open.
- Given the Keystore key is lost, when the Hub starts, then it does not crash or open a blank branch, shows "This Hub needs to be paired again" with the pairing screen, and `branch.db` is untouched.
- Given the first heartbeat, then the wizard advances to step 4 and Account → Branches shows Android.
- Given the stored token and KEK, when a test reads the app's private files, then neither appears in clear.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why a trait in `arena-tasks`:** the JNI call needs the app's class loader, so the shell caches a `JavaVM` and a global ref inside `nativeStart` (called from Kotlin) and hands `arena-hub` an `Arc<dyn SecretWrapper>`. Everything else is plain Rust and testable on any host with a fake wrapper.
- **Wrapped file format:** `nonce || ciphertext` from Kotlin AES-GCM; Rust treats it as opaque bytes. An unwrap error or missing key alias is "lost"; an I/O error is not.
- **Lost vs first run:** a missing wrapped file is lost only when `hub_identity` exists; with no identity it is a fresh install.
- **Owner side of re-pair:** a code for a branch whose Hub is paired but lost is not offered in Account → Branches yet (Story 9.8 owns replacing a Hub); the screen says "Get a new code from your browser" like 2.10.

## Verification

**Commands:**
- `cargo fmt --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace --no-fail-fast` (cloud tests need `ARENA_REQUIRE_SQLD=1` where sqld is available), dev-fixtures runs for `arena-hub` and `arena-api`.
- `pnpm gen:types` then every `pnpm run check:*` and `pnpm run test:scripts`; `node scripts/check-android.mjs manifest`.
- `pnpm --filter @arena/hub check` and `test:shell` (`--workers=2`, own `ARENA_PW_PORT`), `pnpm --filter @arena/onboarding check` and `test`, `pnpm --filter @arena/owner check` and `test`.
- `cargo clippy --locked --target aarch64-linux-android -p arena-hub-ui --lib -p arena-store -- -D warnings` if the target is installed.

**Manual checks (if no CLI):**
- Not runnable locally (as in 4.1 and 4.2): the Gradle/Kotlin build and emulator or device checks (non-exportable key, Keystore reset, files on a rooted image). State this in the final report.

## Verification Log

Pass: cargo fmt, clippy (-D warnings, incl. dev-fixtures for arena-hub and arena-api), arena-tasks keystore units, arena-hub `keystore` dev-fixtures test, svelte-check (hub, onboarding, owner), Hub Playwright shell suite, onboarding `install_hub` and `a11y`, owner `branches`, `check:strings`, `check:layers`, `format:check`, `check-android` script tests. Not run: Android Gradle/Kotlin build, `android.rs` and `keystore_jni.rs` on the Android target, emulator/device ACs (non-exportable key, Keystore reset, private-file inspection), `check:deploy` (needs kubectl). Pre-existing failures unrelated to this story: arena-hub `dev_seed`/`lan`/`printer` (`USER_LIMIT`), arena-tasks timing tests, arena-cloud `backup`/`binary`.
