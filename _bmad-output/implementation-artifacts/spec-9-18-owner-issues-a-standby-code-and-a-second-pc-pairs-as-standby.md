---
title: 'Story 9.18: Owner issues a standby code and a second PC pairs as standby'
type: 'feature'
created: '2026-10-31'
status: 'done'
baseline_revision: '34f1d1c3666e354217a800c5460e51497fa59e5e'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-8-owner-replaces-a-failed-hub-and-gets-a-new-pairing-code.md'
warnings: []
deferred:
  - 'The LAN half: the "Link standby" 6-digit code and panel on the main Hub (Settings -> System -> Standby), the standby finding the main Hub by mDNS, the main Hub minting a `kind = standby` paired-device token and storing the standby certificate fingerprint, and the fingerprint reaching kiosks (AC 3 and AC 4). Needs the Hub-side standby identity and runtime role that Story 9.19 introduces.'
  - 'The Hub installer on the second PC sending `role: standby` (the Hub `CloudPairer` still pairs as `main` only), and the Hub showing `tier-lock` / "Needs the Windows Hub" for a refused standby (AC 5, Hub half). The cloud contract is final and tested.'
  - 'Go-live "Standby PC" is done while a standby Hub is paired in the cloud; it becomes "linked" (LAN) when the deferred link step lands.'
---

<intent-contract>

## Intent

**Problem:** A paid branch has one live Hub per branch in the cloud (`hubs_one_active`), so a second PC cannot pair, the Story 2.30 "standby pairing code" is a `tier-lock` placeholder, and the Story 2.31 go-live "Standby PC" item is always not done.

**Approach:** A Hub device gets a `role` (`main` or `standby`) and so does a pairing code. A branch keeps one live Hub and one live code of each role. The owner app asks for a standby code on Account -> Branches; `POST /v1/pair` with `role: standby` registers the second PC as a standby Hub device (own `hub_instance_id`, token, licence) beside the main Hub without touching the main Hub, its fence, its owner setup code or `branch.bootstrap`.

## Boundaries & Constraints

**Always:**
- `POST /v1/wizard/install-hub/standby-code` (owner session, `{branch_id?}`) returns the same `PairingCodeIssued` as the Hub code (8 characters, 15 minutes, single use). A new standby code invalidates the unused one; it never touches the Hub code (and a new Hub code never touches the standby code).
- Refusals, in this order: pairing steps not done / no paired main Hub `STEP_NOT_READY`; the plan has no `standby` flag (Free and Basic) `TIER_LOCKED`; the main Hub is Android `PLATFORM_UNSUPPORTED`; a live standby Hub exists `STANDBY_EXISTS` (decided in the same statement that writes the code, so two taps cannot both issue). Nothing is written on a refusal.
- `/v1/pair` with `role: standby`: the code must be a standby code and the request must be Windows; the plan must still carry the `standby` flag (`TIER_LOCKED`); the PC must not be the main Hub's `hub_instance_id`. A code of the other role is `PAIR_CODE_INVALID` and is not spent. The response carries `role: standby`, `bootstrap` absent, `restore_pending` false and a licence for the branch's tier.
- A standby pairing writes one `hubs` row (`role = standby`) and spends the code; it makes no owner setup code, deletes no fence, locks no branch code and revokes only an earlier live standby. Pairing the main Hub revokes only the earlier main Hub; `hub.revoke` (Story 9.8), the `watch_rows` alert sweep, command expiry and the command-schema read count `role = 'main'` Hubs only.
- Every cloud endpoint that authenticates a Hub token refuses a standby token (`FORBIDDEN`) for now; the standby calls none of them until Story 9.19.
- `OwnerBranch.standby` is present only while a standby Hub is paired or its code is waiting (`{paired, last_seen_at?, code?: {expires_at}}`). The go-live "Standby PC" item is done while a standby Hub is paired and never blocks Go live.
- The Owner app shows, per branch card: no `standby` flag -> `tier-lock` "Available on Premium"; flag and Android Hub -> "Needs the Windows Hub"; flag and no paired Hub -> "Pair the main Hub first."; paired standby -> its state; otherwise "Add standby PC" -> the code card with its expiry (same `PairingCode` card as the Hub code). The page-level Standby placeholder is removed.

**Never:**
- No change to the main Hub's pairing, fence, heartbeat, upload, restore or key paths; no standby runtime on the Hub; no LAN linking, no fingerprint exchange (deferred above).

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Premium, Windows main | owner taps "Add standby PC" | 200 code; `OwnerBranch.standby = {paired:false, code}` |
| Second PC pairs | standby code + `role: standby`, Windows | 200 `role: standby`, no bootstrap, main Hub row and token untouched |
| Second code | standby paired | 409 `STANDBY_EXISTS`, no code written |
| Free / Basic | no `standby` flag | 403 `TIER_LOCKED`; app shows `tier-lock` Premium |
| Android main Hub | flag present | 409 `PLATFORM_UNSUPPORTED`; app shows "Needs the Windows Hub" |
| Wrong installer | Hub code with `standby`, or standby code as `main` | 401 `PAIR_CODE_INVALID`, code stays valid |
| Same PC | standby request with the main Hub's instance | 401 `PAIR_CODE_INVALID`, code stays valid |
| Main re-paired | live standby exists | only the main Hub is revoked |
| Standby token used | heartbeat / commands / presign | 403 `FORBIDDEN` |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{pairing.rs,wizard.rs,error.rs,lib.rs}` -- `HubRole`, `role` on `HubPairRequest` / `HubPairResponse`, `OwnerStandby` on `OwnerBranch`, `ErrorCode::StandbyExists`.
- `crates/arena-store/control-migrations/0033_standby_hub.sql`, `src/control/{hubs.rs,pairing.rs}` -- `hubs.role`, `pairing_codes.role`, per-role live indexes, `pair_hub` by role, `standby_hub`, `active_standby_code`, `IssueOutcome::StandbyExists`; `branch_watch.rs`, `commands.rs`, `owner_commands.rs`, `key_requests.rs` -- main-Hub-only queries.
- `crates/arena-cloud/src/api/{pair.rs,wizard_install.rs,branches.rs,wizard_golive.rs,heartbeat.rs,error.rs}` -- pairing by role, `standby-code` route, `OwnerBranch.standby`, go-live item, standby token refusal.
- `crates/arena-tasks/src/cloud.rs` -- the Hub pairs as `main`.
- `apps/owner/src/lib/BranchCard.svelte`, `apps/owner/src/routes/account/branches/+page.svelte`, `packages/ui/src/messages/en.json`, `apps/owner/tests/{standby.spec.ts,branches.spec.ts,stub.ts}`.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- `HubRole`, request / response `role`, `OwnerStandby`, `STANDBY_EXISTS`; unit tests
- [x] `arena-store` (control) -- migration 0033, role-aware pairing codes and `pair_hub`, main-only Hub queries; tests (local and remote backend)
- [x] `arena-cloud` -- standby code route, `/v1/pair` by role, `OwnerBranch.standby`, go-live item, standby token refusal; tests in `tests/pair.rs`
- [x] Owner app -- per-card standby section (tier-lock, Android, no Hub, code card, paired), page placeholder removed; Playwright `standby.spec.ts`
- [ ] LAN link, fingerprint push and Hub installer (deferred, see front matter)

**Acceptance Criteria:**
- Given Account -> Branches on a branch whose licence has the `standby` flag, when the owner chooses "Add standby PC", then a single-use 8-character standby code with the Hub-code expiry card is shown; on Free it stays `tier-lock`; a second standby is refused `STANDBY_EXISTS`.
- Given a standby code, when `POST /v1/pair` is sent with `role: standby`, then a Hub device with `role: standby`, its own `hub_instance_id`, token and licence is registered and `branch.bootstrap` is never sent.
- Given an Android Hub or a branch without the flag, when a standby is attempted, then it is refused `PLATFORM_UNSUPPORTED` / `TIER_LOCKED` and the app shows "Needs the Windows Hub" / `tier-lock`.
- Given the go-live checklist, when a standby is paired (or later revoked), then "Standby PC" shows done (or not done) and never blocks Go live.
- Deferred: the main-Hub "Link standby" step and fingerprint push (AC 3, AC 4).

## Spec Change Log

## Review Triage Log

- Implementation pass only; the independent code review is the next step (story status `review`).
- Code review (2026-10-31): fixed standby-code refusal order (STEP_NOT_READY before TIER_LOCKED), standby pairing now needs a live Windows main Hub, a main Hub code is refused on the PC already paired as standby, key-request hub lookups count role = main; added tests (expiry exact code, plan downgrade TIER_LOCKED, same-PC main-after-standby, standby restore_pending). Accepted/deferred: wrong-role code message (spec-mandated), per-role issue rate limit, licence carries no role (standby token is refused everywhere until 9.19), standby last-seen UI unreachable until 9.19, narrow tier race between classify and pair_hub.
- Scope cut, recorded in `deferred`: the LAN link / fingerprint / Hub-installer half needs the standby Hub identity and runtime role of Story 9.19, which does not exist yet; the cloud and Owner app halves are complete and tested.
- Known, not from this story (Windows): `arena-cloud` `binary::unreachable_at_start_retries_then_exits_1...` fails on the baseline; `backup::a_hung_run_times_out_with_its_own_code` is timing-flaky.

## Design Notes

`HubRole` rides on the existing `hubs` table rather than a new one, so the standby gets the same token, licence and revoke machinery; the cost is that every "the branch's Hub" query must say `role = 'main'`, which is why those queries changed. The unique indexes became `(branch_id, role)`. `OwnerBranch.standby` is optional so existing clients and fixtures are unaffected. The role on the pairing request lets the cloud refuse a code typed into the wrong installer before it is spent. Flags come from the tier's defaults, as `OwnerBranch.flags` already does.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-cloud -p arena-tasks`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run gen:types`, `pnpm run check:strings`, `npx playwright test tests/standby.spec.ts tests/branches.spec.ts` in `apps/owner`
