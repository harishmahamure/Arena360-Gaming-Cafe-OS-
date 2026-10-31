---
title: 'Story 9.19: The standby bootstraps from the main Hub and follows it live'
type: 'feature'
created: '2026-10-31'
status: 'in-review'
baseline_revision: '4ca5f42f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-9-18-owner-issues-a-standby-code-and-a-second-pc-pairs-as-standby.md'
warnings: []
deferred:
  - 'Running-Hub wiring: the Hub binary starting in the standby role (config, `Role::Follower` with a follower task, the "Update this PC to follow the main Hub" banner) and the Hub installer pairing with `role: standby`. Needs the 9.18 LAN link (the "Link standby" code, the `kind = standby` token and the certificate pin), which 9.18 deferred and this story does not build. The follower engine, the main-side server and the store pieces here are complete and tested without it.'
---

<intent-contract>

## Intent

**Problem:** A linked standby has nothing to follow: the main Hub serves no snapshot or change stream, and no code applies another Hub's rows verbatim.

**Approach:** The main Hub serves `GET /v1/replicate/snapshot` and a WebSocket at `/v1/replicate` to a paired device the owner linked as standby. A follower engine loads the snapshot into an empty database, then applies closed batches and live rows with `arena-replay` into its own `changes` (same epoch, same `seq`), carries `player_keys` changes on the same stream, resumes after a drop from its last applied change, and refuses every intent as a read-only follower.

## Boundaries & Constraints

**Always:**
- Replication is open only to a `paired` device whose id is in `standby_links`; any other token is `FORBIDDEN`, an unknown or revoked one `TOKEN_INVALID`. The device token rides in the first frame (`open`), with `from`, the follower's last applied `(epoch, seq)`, and its schema version. A primary Hub only; a follower answers `NOT_PRIMARY`.
- A follower whose schema version is below the main Hub's is refused `SCHEMA_BEHIND` before any data; the engine then stops retrying and reports `behind`.
- The snapshot is a `VACUUM INTO` copy at one consistent point, with the Hub-local tables cleared (lease, meta, licence/reference caches, upload and backup state, dedupe, `standby_links`); `X-Arena-Epoch` / `X-Arena-Seq` state its position. Loading needs an empty destination and keeps the destination's own identity.
- The stream sends rows in log order (`epoch_counter`, `epoch`, `seq`): closed history first in batches of at most 500, then live rows as they commit. Each batch is applied in one transaction, written verbatim into `changes`, and a gap, a duplicate or an epoch change is an error (never silently skipped). A batch also carries the `player_keys` state of every player it touches: a key present is stored, one absent is deleted.
- Resume sends the last applied position; the server streams everything after it. A position the server does not hold is `GAP`, and the engine re-bootstraps from a fresh snapshot.
- Reconnect backoff is jittered between 2 s and 30 s, doubling.
- A follower writes only through the engine; every intent is `NOT_PRIMARY` (existing follower path).

**Never:**
- No standby runtime in the Hub binary, no link flow, no certificate pinning of kiosks (deferred above); no promote.

## I/O & Edge-Case Matrix

| Scenario | State | Behavior |
|---|---|---|
| Bootstrap | empty standby, linked token | snapshot loaded at `(epoch, seq)`; stream resumes after it |
| Live rows | main commits | standby `changes` equal main's, same `seq` |
| Key erased | player erased on main | key row deleted on the standby with the batch holding the `players` row |
| Old standby | its schema < main's | `SCHEMA_BEHIND`, engine stops, status `behind` |
| Drop | stream closes | reconnect with backoff, resume, no gap, no duplicate |
| Unlinked token | paired kiosk token | `FORBIDDEN`, no data |
| Unknown position | `from` not in main's log | `GAP`, re-bootstrap |
| Intent on the standby | follower role | `NOT_PRIMARY`, nothing written |

</intent-contract>

## Code Map

- `crates/arena-proto/src/{replicate.rs,error.rs,lib.rs}` -- frames, paths, backoff constants, `ErrorCode::SchemaBehind`, `Gap`.
- `crates/arena-store/migrations/0090_standby_links.sql`, `src/replicate.rs` -- `standby_links`, log-order reads, snapshot build and load, key reads and writes.
- `crates/arena-replay/src/follow.rs` -- `apply_replicated` (verbatim, one transaction).
- `crates/arena-api/src/lan/{replicate.rs,listener.rs,mod.rs}` -- snapshot route and stream; reuses `media::authenticate` throttle and `LanHub.devices`.
- `crates/arena-tasks/src/follow.rs` -- follower engine over a `ReplicationLink`, backoff, `behind` status.

## Tasks & Acceptance

**Execution:**
- [x] `arena-proto` -- frames, `SCHEMA_BEHIND`, `GAP`, paths; unit tests
- [x] `arena-store` -- `standby_links`, ordered change reads, snapshot build and load, key helpers; tests
- [x] `arena-replay` -- `apply_replicated`; tests (verbatim, gap, duplicate, epoch change, keys)
- [x] `arena-api` -- snapshot route and `/v1/replicate` stream with auth and `SCHEMA_BEHIND`; tests
- [x] `arena-tasks` -- follower engine with bootstrap, resume, backoff; tests
- [ ] Hub binary standby role, link flow, installer role (deferred, see front matter)

**Acceptance Criteria:**
- Given a linked standby with an empty database, when it starts following, then it loads the main Hub's snapshot at its stated position and continues from there.
- Given commits on the main Hub, when the stream delivers them, then the standby holds the same `changes` (epoch, `seq`), one transaction per batch.
- Given a key created or erased on the main Hub, when its player's row arrives, then the standby's `player_keys` matches.
- Given a schema-behind standby, when it connects, then `SCHEMA_BEHIND` and no data.
- Given a dropped stream, when the engine reconnects, then it resumes with no gap and no duplicate.
- Given any intent on a follower, then `NOT_PRIMARY` and no write.

## Spec Change Log

## Review Triage Log

- Implementation pass only; the independent code review is the next step (story status `review`).
- Verified: `cargo test` for arena-proto, arena-store, arena-replay (new `follow` tests), arena-tasks, arena-api (new `tests/replicate.rs`: bootstrap, live follow, restart resume, dropped stream, gap resync, `SCHEMA_BEHIND`, unlinked token `FORBIDDEN`, non-primary), arena-core, arena-hub, arena-kiosk-core; clippy `-D warnings` clean on the touched crates; `cargo fmt --check`, `gen:types`, `check:strings` pass.
- Known, not from this story: `arena-replay` `replaying_the_seeded_log_reproduces_branch_and_settings` fails on the baseline (`library_entries has no row`); `arena-cloud` `binary::unreachable_at_start_retries_then_exits_1...` fails on the baseline; `arena-e2e` and `arena-kiosk-svc` test targets do not build clippy-clean on Windows on the baseline.
- Scope: the follower is read-only because it is a `Role::Follower` Hub (existing `NOT_PRIMARY` path); wiring that role into the Hub binary, the link flow, the real pinned-TLS link and the "Update this PC" banner are deferred (front matter). Not replicated: `applied_intents` (idempotency keys) — to settle in Story 9.21 (promote). Epoch changes on the main Hub (restore) make the standby resync from a fresh snapshot.

## Design Notes

`player_keys` is not in the change log, so keys ride beside the rows that make them matter: the server reads the key state of each player a batch touches at send time. Position is the log order, not a row id, so it survives the standby having started from a snapshot.

## Verification

**Commands:**
- `cargo test -p arena-proto -p arena-store -p arena-replay -p arena-api -p arena-tasks`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`
- `pnpm run gen:types`, `pnpm run check:strings`
