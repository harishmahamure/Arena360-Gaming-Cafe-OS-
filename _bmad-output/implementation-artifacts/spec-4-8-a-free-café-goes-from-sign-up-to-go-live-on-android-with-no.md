---
title: 'Story 4.8: A Free café goes from sign-up to go-live on Android with no help'
type: 'feature'
created: '2026-10-28'
status: 'in-review'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-4-7-the-android-hub-saves-backups-to-a-folder-the-owner-chooses.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Each Free-on-Android step (sign-up, Free tier, Android pairing, owner sign-in, kiosk pairing, groups, rate, printer skip, backup folder, ad consent, test session, go-live) is tested alone. Nothing proves the whole path works with no comp entitlement, that Windows is refused on Free, or that CI runs it nightly and on release candidates.

**Approach:** One host-run end-to-end test starts a real cloud (in-process, over TCP) and the real `arena-hub` binary with `--platform android`, plus a kiosk speaking the real LAN protocol, and walks the whole path through public APIs only (owner cookie on the cloud, loopback `/ui` on the Hub, command mailbox, heartbeat). A device-farm job (emulator + Windows kiosk VM) is added to CI on a nightly and release-candidate schedule, failing with the step name and a screenshot.

## Boundaries & Constraints

**Always:**
- No comp entitlement is ever inserted; the tier is Free through `PUT /v1/wizard/tier`.
- Every step is named (`step("…")`) so a failure reports the step.
- Cloud-side facts are read through the owner API or the control DB, Hub-side facts through `/ui/*` or `branch.db`.
- The Windows Hub and existing tests are untouched.

**Never:**
- New product behaviour, new endpoints, or ad code. Real emulator/device steps are not run here; they are recorded as not run.

## I/O & Edge-Case Matrix

| Scenario | State | Expected |
|----------|-------|----------|
| Full path | Free owner, Android Hub, one kiosk | every step passes; `GET /v1/wizard/go-live` shows opening hours, backup location and ad consent done, standby not done and not blocking; branch Live |
| Windows first | Free, valid step 3 code | `FEATURE_NOT_ENTITLED` (field `platform`), no hub row, code unconsumed, then Android pairs with the same code |
| Onboarding data | after go-live | every wizard step has a `completed_at`; no phone, e-mail, name or setup code in `onboarding_progress` |
| Kiosk test session | kiosk online, rate set | kiosk receives `test_session`, reports `locked`; setup `test_session` is `passed` |

</intent-contract>

## Code Map

- `crates/arena-hub/tests/pairing.rs` -- reuse pattern: `start_cloud`, `start_hub(--platform)`, raw HTTP `call`; cloud harness from `crates/arena-cloud/tests/wizard.rs` (`sign_up`, `complete_setup`).
- `crates/arena-hub/tests/lan.rs` -- kiosk pairing with `arena_kiosk_core::pair` and `HubClient`.
- `crates/arena-cloud/src/api/{wizard*,heartbeat}.rs` -- owner wizard routes, go-live view, heartbeat carries `setup`.
- `crates/arena-store/src/control/setup_status.rs` -- `SETUP_KINDS`, `record_setup` (steps become done from the Hub's setup status).
- `crates/arena-core/src/apply/ads_consent.rs`, `crates/arena-proto/src/{operating,test_session}.rs` -- ad consent, backup dir key, test session states.
- `.github/workflows/ci.yml` -- `android` job; new workflow beside `release-bench.yml`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/arena-hub/tests/free_android_golive.rs` -- the full-path test, the Windows-first test and the onboarding-data checks.
- [x] `scripts/android-e2e.mjs`, `.github/workflows/android-e2e.yml` -- host test job plus an emulator install-and-start job: nightly cron and `v*-rc*` tags, failing with the step name and a screenshot artifact.
- [x] `scripts/android-e2e.test.mjs` -- the workflow has the schedule, RC trigger and failure-artifact step.
- [x] `README.md`, `deferred-work.md` -- document the job and what ran only on the host.

**Acceptance Criteria:** those of Story 4.8 in `epics.md`. The host test covers them through the real binaries; the emulator/VM run is device-only and recorded as not run.

## Verification

**Commands:**
- `cargo test --locked -p arena-hub --features dev-fixtures --test free_android_golive` -- expected: pass.
- `cargo clippy --locked -p arena-hub --features dev-fixtures --all-targets -- -D warnings`, `cargo fmt --check`.
- `node --test scripts/android-e2e.test.mjs`.

## Verification Log

Pass: `free_android_golive` (full path, about 61 s) and the existing `pairing` tests on the host, clippy with `-D warnings` on arena-hub with `dev-fixtures`, `cargo fmt --check`, `scripts/android-e2e.test.mjs` (6) and `check-android.test.mjs`, `check:pins`, `check:layers`, `check:strings`, `check:protocol`, `check:secrets`, prettier. Not run: the `android-e2e` workflow, the emulator job, a Windows kiosk VM and any real Android device; see `deferred-work.md`.
