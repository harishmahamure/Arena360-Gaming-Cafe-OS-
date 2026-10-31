---
title: 'Story 9.20: The standby keeps its own catalogue, media and licence and shows its status'
type: 'feature'
created: '2026-10-31'
status: 'review'
baseline_revision: '61219ba6'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-19-the-standby-bootstraps-from-the-main-hub-and-follows-it-live.md'
warnings: []
deferred:
  - 'Running-Hub wiring (same cause as Story 9.19): starting the catalogue, licence and status-publisher tasks in a Hub binary that runs as a standby, the real pinned-TLS `LanMedia` and `ReplicationLink::report` over the live WebSocket, merging `ui_standby_router` / `router_with_standby` into the Hub binary, and the periodic call to `publish_following`. Needs the standby runtime role and the 9.18 LAN link, neither of which exists yet. Every piece below is built and tested without them.'
  - 'The Hub UI: the standby screen ("Standby — following <main Hub name>" with the disabled floor and "Promote to main"), the "Standby behind" `status-badge` on both screens, and the Settings -> System -> Standby panel. They read `GET /ui/standby` (`StandbyView`), which is served and tested here.'
  - '"Unlink standby" with the manager PIN: removing a `standby_links` row needs the PIN flow and the intent that goes with it; the panel shows the linked standby read-only until then.'
---

<intent-contract>

## Intent

**Problem:** A standby follows the main Hub's rows (Story 9.19) but has no reference cache, no licence of its own, and nobody can see how far behind it is: not the standby's staff, not the main Hub's, not `/healthz`.

**Approach:** The standby runs the catalogue pull for its own cache, taking media from the main Hub over the LAN first and the CDN second, and refreshes its own licence from the cloud with its own token. The follower counts its lag in rows and seconds, tells the main Hub on the replication stream, and both Hubs show it with one shared "Standby behind" rule. `GET /healthz` states role, epoch and standby lag; `GET /ui/standby` serves the standby screen and the main Hub's System -> Standby panel.

## Boundaries & Constraints

**Always:**
- Lag in rows is the main Hub's known log end minus the newest applied row; lag in seconds is how long the standby has been continuously behind (0 when caught up).
- "Standby behind" is exactly: lag above 0 rows for more than 30 s (`lagging`), or no frame from the main Hub for more than 60 s (`no_stream`, which wins). The main Hub measures silence on its own clock from the standby's last contact, so a standby that stops reporting is still flagged.
- The standby reports to the main Hub after every applied batch and every `ping`, as a `report` frame on the replication stream. The main Hub reads it while the stream is idle.
- The catalogue pull runs on a Hub that is the primary writer, or a standby (`HubStatus.following` is set). A follower that is not a standby still does not pull. A standby fetches each media file from the main Hub first; any failure falls back to the CDN with the same size and SHA-256 checks. Nothing of the cache is in the change log, a snapshot or a batch.
- `POST /v1/standby/licence` (the standby's Hub token) returns the branch's signed licence, rate limited like the heartbeat (one per 5 s), and records the standby as seen. It touches no fence, epoch, upload record or command. A main Hub's token is `FORBIDDEN` there; a standby token is still `FORBIDDEN` on every other Hub endpoint, except the catalogue manifest, which it may read.
- The standby keeps the licence in its own `licence_cache` only after checking it against its own branch (`Entitlement::evaluate`); an untrusted licence is ignored and the cache keeps what it had. The cache survives a snapshot load and a resync.
- `GET /healthz` adds `standby: {lag_rows, lag_seconds, behind}` on a standby (its own) and on a main Hub that has heard from a standby (as last reported); the key is absent otherwise.

**Never:**
- No standby runtime, UI screens or unlink flow (deferred above); no promote.

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Caught up | live batch applied | lag 0 rows, 0 s, no warning |
| Catching up | main log ahead by 20 rows for 36 s | `lagging`, "Standby behind" |
| Quiet line | no frame for 62 s | `no_stream` |
| Never connected | linked more than 60 s ago | main Hub panel shows `no_stream` |
| Standby media | main Hub has the cover, not the video | cover from the LAN, video from the CDN |
| Main Hub lacks a file | LAN refuses or mismatches | file fetched from the CDN |
| Follower, not standby | `following` unset | no catalogue pull |
| Standby licence | genuine, other branch's, or WAN down | stored; ignored; cached licence stays standing |
| Standby calls heartbeat | standby token | `FORBIDDEN` (unchanged) |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{standby.rs,replicate.rs,lib.rs}` -- `StandbyReport`, `standby_behind` and its 30 s / 60 s constants, `StandbyPanel`, `StandbyScreen`, `StandbyView`, `StandbyLine`, the licence request and response, `ReplicateFrame::Report`.
- `crates/arena-core/src/status.rs` -- `HubStatus.following`.
- `crates/arena-tasks/src/{standby_progress.rs,follow.rs}` -- progress tracking fed by the follower, `publish_following`, `ReplicationLink::report`.
- `crates/arena-tasks/src/{catalog.rs,cloud.rs,standby_licence.rs}` -- standby catalogue pull with `LanMedia`, `standby_licence` call, `StandbyLicenceTask`.
- `crates/arena-store/src/{replicate.rs,control/hubs.rs}` -- `linked_standby`, `branch_name`, `touch_standby`.
- `crates/arena-cloud/src/api/{heartbeat.rs,catalog.rs}` -- `authenticate_any`, `POST /v1/standby/licence`.
- `crates/arena-api/src/{standby.rs,ui_standby.rs,health.rs,lan/replicate.rs,lan/mod.rs}` -- `StandbyObserver`, `GET /ui/standby`, `/healthz` standby lag, stream reads reports.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- report, panel, screen, behind rule, licence call types, `report` frame; unit tests
- [x] `arena-tasks` -- progress (lag rows and seconds, silence), follower feeds it and reports upstream; tests
- [x] `arena-tasks` -- catalogue pull on a standby with LAN-first media and CDN fallback; tests
- [x] `arena-cloud` + `arena-store` + `arena-tasks` -- standby licence endpoint, call and task; tests
- [x] `arena-api` -- observer, stream reports, `/healthz` standby lag, `GET /ui/standby`; tests
- [x] `arena-store` -- standby keeps its own licence through snapshot load and resync; test
- [ ] Standby runtime wiring, Hub UI screens and unlink (deferred, see front matter)

**Acceptance Criteria:**
- Given a standby that has applied everything, then it reports 0 rows and 0 s, and the main Hub's panel and `/healthz` agree.
- Given rows waiting for more than 30 s, or no frame for more than 60 s, then both Hubs show "Standby behind" with the cause.
- Given the standby's catalogue pull, then it asks the cloud for the manifest itself and takes media from the main Hub first, the CDN second.
- Given a standby, then it refreshes its own licence with its own token, and nothing in the reference cache or licence comes through replication.
- Given `GET /healthz`, then it carries role, epoch and standby lag on either Hub.
- Given the main Hub with a linked standby, then `GET /ui/standby` gives its name, lag, last contact and version.

## Spec Change Log

## Review Triage Log

- Implementation pass only; the independent code review is the next step (story status `review`).
- Verified: `cargo clippy --workspace --all-targets -- -D warnings` clean except `arena-e2e` and `arena-kiosk-svc` (do not build on Windows on the baseline); `cargo fmt --check`; tests for arena-proto, arena-store, arena-tasks (progress, catalogue-on-standby, licence task), arena-api (observer, `ui_standby`, `/healthz`, `tests/replicate.rs` incl. the report reaching the main Hub), arena-cloud (`tests/pair.rs` standby licence and catalogue access).
- Known, not from this story: `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings` and `arena-cloud` `binary::unreachable_at_start_retries_then_exits_1...` fail on the baseline (see Story 9.19).
- Scope: lag in rows only knows the main Hub's log end at `ready` and from each live batch, so rows committed on the main Hub during a long catch-up show on the next frame, not before.
- A `hubs.role = 'standby'` Hub is counted as seen by `touch_standby`; the owner app's `OwnerBranch.standby.last_seen_at` can use it later.

## Design Notes

The report rides the stream the standby already holds open, so the main Hub needs no new route, token or poll. The main Hub flags silence on its own clock because a standby that has stopped talking cannot report that it has. The standby's licence call is separate from the heartbeat on purpose: the heartbeat settles the fence and epoch, which only the main Hub may do.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-tasks -p arena-api`, `cargo test -p arena-cloud --test pair --test catalog`, `cargo clippy --workspace --all-targets -- -D warnings` (minus `arena-e2e`, `arena-kiosk-svc`), `cargo fmt --check`
- `pnpm run gen:types`, `pnpm run check:strings`
