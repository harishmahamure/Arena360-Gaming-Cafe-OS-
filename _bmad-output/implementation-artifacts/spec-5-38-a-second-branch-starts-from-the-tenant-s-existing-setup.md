---
title: "Story 5.38: A second branch starts from the tenant's existing setup"
type: 'feature'
created: '2026-10-28'
status: 'review'
review_loop_iteration: 0
baseline_commit: '6e90bb4'
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-36-export-and-import-a-branch-setup-file.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-37-paid-hubs-keep-their-setup-template-in-the-cloud.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** An owner opening another branch re-creates the first branch's groups, rates, plans and menu by hand, although the cloud already holds an up-to-date copy (Story 5.37).

**Approach:** The new branch's wizard offers "Start from <branch>'s setup", on by default when another paid branch of the tenant has a stored file, with a picker and an off switch. The choice lives in `gz_control.branch_template_choice`. When the new Hub pairs, the `branch.bootstrap` command carries a short-lived signed link to the chosen file (`BranchBootstrapCommand.template`). The Hub downloads it, checks its SHA-256, imports it through the Story 5.36 `import.setup` intent as the owner and tells the cloud the outcome. A failure shows in the wizard with Retry (which has the next heartbeat carry a fresh link) and the file path; the branch has bootstrapped either way.

## Boundaries & Constraints

**Always:**

- Sources: the tenant's other branches that are on a paid tier now (`Entitlement::tier_at`) and have a stored file (`TemplateStore::describe`), oldest first. No row in `branch_template_choice` means the default: on, from the first source. A row with no source means the owner turned it off. A stale source (lapsed, file gone) counts as off.
- Routes (owner session, own branch, `OwnedBranch`): `GET /v1/branches/{id}/setup-start` answers `SetupStartView {other_branches, sources, enabled, selected, status, error}`; `PUT` takes `SetSetupStart {enabled, source_branch_id?}` (a source that is not listed, or the branch itself, is 422 `source_branch_id`); `POST .../retry` moves `failed` to `retry`. Choosing a source on a branch whose Hub is already paired sets status `retry`, so the next heartbeat delivers it; choosing the same source again changes nothing.
- Link: `SetupTemplateLink {path, version, sha256, bytes, expires_at}`, `path` = `/v1/setup-template/<source>.<expires_at>.<sha256>.<HMAC>` (HMAC-SHA256 under the pepper with its own label), valid `SETUP_LINK_TTL_MS` (15 min). `GET /v1/setup-template/{token}` needs no token: a bad signature, an expired link, a missing store or a file whose SHA-256 no longer matches all answer the same `NOT_FOUND`.
- Pairing: `POST /v1/pair` records the choice as `pending` and puts the link in `bootstrap.template`, unless the status is already `applied`. Anything that fails while preparing it is logged and the pairing goes on without a link. A heartbeat answer carries `setup_template` once for a `retry` row (one heartbeat wins it: `retry` to `pending`).
- Outcome: `POST /v1/branches/{id}/setup-template/outcome {applied, message?}` with the Hub's token, for the token's own branch only (`FORBIDDEN` otherwise); message trimmed, control characters dropped, 200 characters. Sets `applied` or `failed` and wakes the wizard stream.
- Hub: `SetupTemplateInbox` receives the link from `CloudPairer` (after `record_pairing`) and `HeartbeatTask`; `SetupApplyTask` fetches it (3 tries, 3 s then 6 s; a 404 is not retried), refuses an expired link or a file whose SHA-256 differs, parses it (`arena_core::setup_file::parse`; a newer version says "Update this Hub to import this file."), submits `import.setup` as `Actor{owner, tenant_id}` with an idempotency key derived from the file hash, and reports. "Nothing left to add" (`VALIDATION_FAILED` on `file`) counts as applied. An outcome the cloud did not hear is sent again every 30 s until it does, or until the Hub is revoked.
- Wizard: `SetupStartPanel` on step 3 (before pairing, where the choice takes effect) and step 4. Hidden for a café with one branch. With no sources it says how to carry the file by hand ("Export the setup file at your other branch's Hub (Settings → Import → Branch setup) and import it at the new Hub."). After a failure it shows the reason, Retry and the same file-path text. Step 4 shows the copied groups through the existing devices view, and step 5 completes from the existing heartbeat setup block when every group has a rate.

**Never:**

- A link that carries a Hub secret, or a download that needs a login. A free source in the picker. A failed download that stops the pairing. Importing over existing rows (the import is create-only).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default on | tenant has a paid branch with a stored file | view `enabled`, `selected` the first source | none |
| Only Free branches / no file | no sources | view `enabled false`, panel shows the file path | none |
| Pair, option on | new Hub pairs | `bootstrap.template` link, status `pending`; Hub imports, reports `applied` | none |
| Pair, option off | owner turned it off | no link | none |
| Download fails | WAN down, 5xx | 3 tries, outcome `failed` with a reason; wizard shows Retry and the file path | Retry gives the next heartbeat a fresh link |
| Link expired / forged / file changed | old or edited link | 404 `NOT_FOUND`, Hub reports `failed` | Retry |
| Chosen after the Hub paired | PUT while paired | status `retry`, link on the next heartbeat | none |
| Other owner / other Hub | wrong tenant | 403 `FORBIDDEN` | none |

</intent-contract>

## Code Map

- `crates/arena-proto/src/setup_file.rs` -- `SetupTemplateLink`, `TemplateStatus`, `SetupTemplateOutcome`, `SetupStartView`, `SetSetupStart`, `SETUP_LINK_TTL_MS`; `pairing.rs` -- `BranchBootstrapCommand.template`, `HeartbeatResponse.setup_template` (+ generated TS).
- `crates/arena-store/control-migrations/0016_branch_template_choice.sql`, `src/control/template_choice.rs`.
- `crates/arena-cloud/src/api/setup_start.rs` (routes, `bootstrap_link`, `retry_link`), `auth.rs` (`link_signature`, `link_signed`), `api/pair.rs`, `api/heartbeat.rs`.
- `crates/arena-tasks/src/template_apply.rs` (`SetupTemplateInbox`, `SetupApplyTask`), `cloud.rs` (`download_setup_template`, `setup_template_outcome`, `CloudPairer::with_templates`), `heartbeat.rs`; wired in `crates/arena-hub/src/lib.rs`.
- `apps/onboarding/src/lib/SetupStartPanel.svelte`, used by `InstallHubStep.svelte` and `DevicesStep.svelte`; `api.ts`; `packages/ui/src/messages/en.json`.
- Tests: cloud `tests/setup_start.rs`; Hub task unit tests in `template_apply.rs` and `heartbeat.rs`.

## Tasks & Acceptance

**Execution:**

- [x] `arena-proto` -- link, status, outcome and view types, bootstrap and heartbeat fields, generated TS
- [x] `arena-store` -- control migration 0016 and `TemplateChoice` queries
- [x] `arena-cloud` -- choice routes, signed link and download, outcome route, pairing and heartbeat wiring, tests
- [x] `arena-tasks` / `arena-hub` -- download-and-import task, inbox wiring, tests
- [x] Onboarding wizard panel and messages
- [x] Verification gate

**Acceptance Criteria:**

- Given an owner creating another branch, when the wizard reaches step 4 and a paid branch of the tenant has a stored file, then "Start from <branch>'s setup" is on by default with a picker of those branches, and can be turned off.
- Given the option is on, when the new Hub pairs, then `branch.bootstrap` carries a short-lived link to the chosen file, the Hub applies it through the Story 5.36 import, the copied groups appear in step 4, and step 5 completes if every group has a rate.
- Given the other branches are all on Free, or none has a stored file, when the wizard shows step 4, then it explains the file path and the owner can continue.
- Given the download fails, when the new Hub applies the bootstrap, then the wizard offers Retry or the file path, and the branch still bootstraps without the template.

## Spec Change Log

## Design Notes

The wizard's step 4 is reached after the Hub has paired (step 3), so the option is also shown in step 3, where a choice made before pairing rides on the bootstrap. A choice made after pairing is queued as `retry` and delivered by the next heartbeat, so turning it on later still works; turning it off after the Hub has already imported changes nothing (the import is create-only and is not undone).

The link is a signed path rather than an R2 presigned URL so it works with the in-memory dev store, keeps the object store private, and can be checked against the file's current SHA-256 (a file replaced after the link was minted is refused instead of silently applied).

## Verification

**Commands:** `cargo fmt --check`; `cargo clippy --locked --workspace --all-targets -- -D warnings`; `cargo test` for `arena-proto`, `arena-store`, `arena-cloud`, `arena-tasks`, `arena-hub`; `pnpm gen:types`, `check:generated`, `check:protocol`, `check:layers`, `check:strings`, `format:check`; onboarding `svelte-check`.

## Review Triage Log

Unattended build; see the commit message for the verification run.
