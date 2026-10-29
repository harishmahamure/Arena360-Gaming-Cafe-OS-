---
title: 'Story 6.10: A new player signs up at the kiosk with one consent box'
type: 'feature'
created: '2026-10-29'
status: 'done'
baseline_commit: 'a888ce77cdc57b21e88a919225a8b8e22f275e3f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epics.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-8-a-player-logs-in-at-the-kiosk-with-username-and-password.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-6-9-the-plan-picker-starts-a-prepaid-session-from-the-kiosk.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** "Create account" on the 6.8 login panel only says "Ask at the counter to sign up.", so a first-time player still needs staff. The kiosk has no sign-up form, no frame that carries one, and a device actor may not apply `player.create` (it needs the staff permission `player.manage`).

**Approach:** A `signup-form` opens in the same panel. The app sends `{username, password, phone, name, consent}` to the Kiosk service, which sends the paired-only frame `player.signup`. The Hub validates, hashes and seals exactly as the Story 3.6 counter path, applies `player.create` as the paired device, records the login (as 6.8/6.9 do) and answers `ok{player}`, so the app goes straight to the plan picker. Duplicates and bad fields come back typed and show inline.

## Boundaries & Constraints

**Always:**

- **Frames.** `player.signup{username, password, phone, name, consent}` (paired sessions only, else `FORBIDDEN`) -> `player.signup_result{outcome}`; `outcome` = `ok{player{player_id, name}}` | `refused{code, field?}`. Types in `arena-proto/src/player_signup.rs`; no hand-written TS types. A Hub that is not primary or has no player directory answers an `error` frame as 6.8. Credentials and personal data never appear in logs or `Debug`.
- **Same rules as the counter.** The Hub runs the Story 3.6 checks (`PlayerForm::check`: username 3-32 `[a-z0-9._-]` after normalising, password 6-128, phone `+` and 8-15 digits, name 1-80), hashes with Argon2id on a blocking thread under the Hub's verify permits and seals name and phone with `arena_core::players::seal` (a fresh player key wrapped by the branch KEK). Email, date of birth, notes and tags are empty. The policy version is the Hub's constant `PLAYER_POLICY_VERSION` ("1", the value of the `player.policy_version` string); the kiosk never names it.
- **Consent.** `consent` must be true: false answers `refused{VALIDATION_FAILED, field "consent"}` and writes nothing. The consent row is the granted `transactional` consent with the current policy version (existing `player.create` apply). There is no OTP and no marketing consent.
- **Apply as the device.** The Hub submits `PlayerCreate` through the writer with `actor{kind: device, id: this PC}`. `rbac::check` lets `ActorKind::Device` apply `player.create` only when `consent` is true; no other player kind (`update`, `password_reset`, `import`) is opened to a device. `PAIRED_SCOPE` is unchanged: a kiosk cannot send `player.create` itself. The change log shows the device actor and the player id; it carries no name or phone (sealed ciphertext and a phone hash only).
- **Duplicates.** `USERNAME_TAKEN` and `PHONE_TAKEN` come back as `refused{code, field}` with `field` `username` / `phone`; nothing is written. Other `AppError`s (`VALIDATION_FAILED` + field, `PERMISSION_DENIED`, ...) come back as their code.
- **Logged in after sign-up.** On `ok` the Hub stores the same login record as a correct `player.authenticate` (`LoginRecords::set`) for this device and connection, so `player.plans`/`player.start_session` work at once. No new `kiosk.log` is written (the `player.create` change row already shows player and device).
- **Form.** The `signup-form` opens in the same login panel from "Create account" (replacing the 6.8 inline note) and shows: username, password (show/hide, paste allowed), phone (default `+91`), name, one consent checkbox (unticked, hit area at least 44px) with the text `player.signup.consent` where "Terms" and "Privacy Policy" are underlined links that open read-only pages in the kiosk (bundled text, `legal-page`, with a Back button returning to the form with its fields kept), and a primary "Create account". No email, no OTP, no marketing box. A secondary "Back to log in" returns to the login form.
- **Disabled until ticked.** With the box unticked the primary is disabled and the inline reason "Tick the box to continue" shows beside it. The Hub link not `connected` disables it too, with "Counter is reconnecting — try again shortly." inline (nothing is sent).
- **Field errors.** Username / password / phone / name are checked on blur and on submit with the same rules (strings `hub.players.invalid.*` reused); an invalid form sends nothing and moves focus to the first invalid field. `USERNAME_TAKEN` shows "That username's taken." on username and `PHONE_TAKEN` shows "That phone number already has an account — ask staff if it's yours." on phone, each with focus moved to the field. The password is cleared from the form the moment it is sent and never reaches `localStorage`, saved state or logs.
- **After sign-up.** `ok` hands the player to the existing `PlanPicker` exactly as a 6.8 login does (same `hold(player)` path, "Not you? Log out", 60 s idle). The 6.7 panel idle close applies to the form; Esc closes the panel.
- Copy from the catalogue; money not involved; no `backdrop-filter`; no hex colours; no player context written to disk by the kiosk.

**Never:** a referral field (13.13); OTP or marketing consent; email/date of birth/notes at the kiosk; a new `IntentKind`, `ActorKind` or `ErrorCode`; adding `PlayerCreate` to `PAIRED_SCOPE`; a kiosk-chosen player id or policy version; changing the Hub counter path (Story 3.6); fetching legal pages from the internet; an Epic 11 legal-content workflow.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Happy path | valid fields, box ticked | `ok{player}`; player, wallet, granted consent row created; plan picker shows | — |
| Box unticked | form otherwise valid | primary disabled, "Tick the box to continue"; Hub also refuses `consent` false if it is sent | nothing written |
| Username taken | existing username (any case) | `refused USERNAME_TAKEN`; "That username's taken." on username, focus there | nothing written |
| Phone taken | phone with an existing profile | `refused PHONE_TAKEN`; phone message, focus on phone | nothing written |
| Short password / bad phone | 5-char password, `+12` | inline field error on blur/submit; nothing sent | nothing written |
| Hub unreachable | link not `connected`, or no answer | not sent; "Counter is reconnecting — try again shortly." | form keeps its fields (password cleared after a sent request) |
| Non-paired caller | phone/kds class | `FORBIDDEN` | — |
| Device `player.create` with `consent` false, or `player.update` as a device | any | denied by `rbac::check` | writes nothing |
| Then plan picker | `ok` | `player.plans` works at once for this device and connection | — |

</intent-contract>

## Code Map

- `crates/arena-proto/src/player_signup.rs` (new) -- `PlayerSignup` (redacted `Debug`), `PlayerSignupResult`, `PLAYER_POLICY_VERSION`; `frame.rs` kinds `player.signup` / `player.signup_result` (pattern: `player.plans`, lines ~78-152); `lib.rs` re-exports; generated TS.
- `crates/arena-core/src/rbac.rs:90 device_starts_own_session / :114 check` -- add `device_creates_player` for `Permission::PlayerManage` + `PlayerCreate{consent: true}`; unit tests; `crates/arena-core/tests/proptest_apply.rs` device-actor denial case.
- `crates/arena-core/src/players.rs:130 seal`, `arena-proto/src/players.rs:306 PlayerForm::check`, `arena-core/src/staff.rs hash_secret` -- reused as-is (pattern: `ui_auth.rs:2623 players`).
- `crates/arena-api/src/lan/player_signup.rs` (new) -- handler; `lan/player_login.rs` (`intent()`, `LoginRecords::set`, verify permits), `lan/session.rs:597-620 / 787 / 810-900` (dispatch, `paired_device`, `check_*`), `lan/mod.rs`.
- `crates/arena-kiosk-core/src/ipc.rs:174-200` -- `IpcMsg::PlayerSignup`; `arena-kiosk-svc/src/{player_session.rs (forwarder pattern),app.rs:530-575,lib.rs}`.
- `apps/kiosk/src-tauri/{ipc-names.json,src/main.rs:111-150,src/bridge.rs:740-850}` -- command `player_signup`.
- `apps/kiosk/src/lib/{bridge.ts:360 (pattern),signup.ts (new),Signup.svelte (new),Legal.svelte (new),Login.svelte,Attract.svelte:227-239}` -- form, validators, outcome mapping, legal pages, wiring.
- `packages/ui/src/messages/en.json` -- `player.signup.*`, `player.legal.*`; reuse `player.signup.username_taken|phone_taken|consent`, `hub.players.invalid.*`, `player.hub.reconnecting_retry`, `player.login.*`. Remove `player.login.create_hint` use.
- Tests: Rust beside each; `apps/kiosk/tests/{signup.spec.ts (new),login.spec.ts}`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- signup types, `PLAYER_POLICY_VERSION`, two frame kinds; regenerate types; tests
- [x] `arena-core` -- device `player.create` rule in `rbac::check`; unit tests and a proptest denial case
- [x] `arena-api` -- `player.signup` handler (validate, hash, seal, apply as device, login record), dispatch; tests incl. non-paired `FORBIDDEN`, consent false, both duplicates, log has no name/phone
- [x] `arena-kiosk-core` / `arena-kiosk-svc` -- `IpcMsg::PlayerSignup` and forwarder; tests
- [x] Tauri shell -- `player_signup` command and names list; tests
- [x] Kiosk app -- `Signup.svelte`, `signup.ts`, `Legal.svelte`, Login/Attract wiring; Playwright `signup.spec.ts`; update `login.spec.ts` for the "Create account" change
- [x] Catalogue strings `player.signup.*`, `player.legal.*`

**Acceptance Criteria:**

- Given "Create account" on the login panel, then the `signup-form` opens in the same panel with username, password (show/hide, paste allowed), phone defaulting to `+91`, name and one unticked consent checkbox (hit area at least 44px) whose text has underlined Terms and Privacy Policy links that open read-only pages in the kiosk; there is no OTP and no marketing consent.
- Given the box is unticked, then "Create account" is disabled with "Tick the box to continue" inline.
- Given valid input and the box ticked, then the Hub creates the player, wallet and a granted consent row with the current policy version (sealed as Story 3.6), and the plan picker shows with the player logged in.
- Given a username or phone already used at the branch, then the field shows "That username's taken." or "That phone number already has an account — ask staff if it's yours.", focus moves to it and nothing is written.
- Given a password under 6 characters or an invalid phone, then the field shows its inline error (on blur and submit) and nothing is written.
- Given the Hub is unreachable, then nothing is sent and the form shows "Counter is reconnecting — try again shortly.".
- Given a kiosk sign-up applies, then the change log shows the player id and the kiosk device, the consent row shows the policy version, and neither carries a name or phone in the clear.
- Given a non-paired class sends `player.signup`, or a device applies `player.create` with `consent` false or any other player intent, then it is refused.

## Spec Change Log

## Review Triage Log

## Design Notes

The epic says `player.create` with `source: kiosk` and `actor{kind: device}`. `PlayerCreate` has no `source` column and adding one would change the change-log row shape for every player; the device actor in the change log is the source, and the consent version rides on the existing consent row. This follows 6.9: the Hub builds the intent from a frame, the paired device is the actor, and `PAIRED_SCOPE` stays closed.

The Hub, not the kiosk, owns the policy version, the id and the consent check, so a modified kiosk can only create a normal profile with a consented flag it must send as true. No per-device sign-up rate limit is added: the call is paired-only, hashing runs under the shared verify permits, and the Story 2.19 device cap bounds the fleet; a limit can be added with the Hub alert service later.

The Terms and Privacy pages are short bundled catalogue text so the kiosk never needs the internet; the full legal pages are Epic 11 (Story 11.7).

## Verification

**Commands:**

- `cargo test -p arena-proto -p arena-core -p arena-api -p arena-kiosk-core -p arena-kiosk-svc` -- expected: pass
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` -- expected: clean
- `pnpm gen:types`, `pnpm check:strings`, `pnpm check:hex`, `pnpm check:no-blur`, `pnpm format:check` -- expected: pass
- `pnpm --filter @arena/kiosk check` -- expected: 0 errors
- Playwright `apps/kiosk/tests/signup.spec.ts`, `login.spec.ts`, `attract.spec.ts` -- expected: pass

## Implementation Notes

- `PlayerSignupResult` is a flat tagged enum like `PlayerAuthResult` (`outcome` = `ok{player}` | `refused{code, field?}`); `PlayerContext` is reused for `ok`. The frame also carries byte limits (`BAD_FRAME` naming the field) so the Argon2id input is bounded before `PlayerForm::check` runs.
- The Hub checks `consent` first (`refused VALIDATION_FAILED`, field `consent`), then `PlayerForm::check`, then hashes and seals under the verify permits and applies `player.create` as the paired device. Any `AppError` from the writer other than `NOT_PRIMARY` comes back as `refused{code, field}`.
- `IpcMsg::PlayerSignup{rid, form: SignupFields}`; `SignupFields` has a redacted `Debug` and a `SecretPin` password. The shell command takes `username, password, phone, name, consent`.
- The signup form keeps the legal pages inside the same component (the form is `hidden`, not unmounted), so fields are kept on Back. The field-error rows keep their height (two lines for the username rule) so a blur message never moves the consent box out from under a click; the fields sit in two columns so the form fits 1366x768.
- Not run on this Windows host: the `cfg(unix)` `arena-kiosk-svc/tests/runtime.rs` tests added here (checked with the cfg lifted, which fails only on the pre-existing unix-only items). `arena-e2e` and `arena-kiosk-svc/tests/discovery.rs` do not build here, and 14 `arena-kiosk-svc` lib tests fail on SYSTEM ownership, all before this story. `pnpm check:generated` reports the new generated types as untracked until they are committed.

## Code Review (2026-10-29)

Reviewed the implementation commit (Rust Hub handler, rbac device rule, IPC/shell, Svelte form, tests) against the acceptance criteria. No blocking findings; no code changes. Verified: arena-proto/core/api/kiosk-core tests, fmt, check:strings/hex/no-blur/generated/protocol, prettier, svelte-check (0 errors), Playwright signup/login/attract (53 pass). Clippy errors remain only in pre-existing unix-only `arena-kiosk-svc/tests/discovery.rs` and `arena-e2e` (not part of this story). Note: app-side 10 s timeout could precede a slow Hub create; retry would show "username taken" (accepted, low risk).
