---
review: adversarial
target: ../ARCHITECTURE-SPINE.md (+ ../.memlog.md)
date: 2026-10-23
lens: two units, one level down, each obeying every AD literally, that still build incompatibly
holes: 28
---

# Adversarial Review: Arena 360 Architecture Spine

Method: for each pair of units, I looked for two builds that each follow every AD word for word and still do not fit together. Every finding has the two units, how a literal build of each one diverges, why the current ADs allow it, and a proposed AD fix. Severity: **C** = data loss, legal or money error; **H** = wrong state reaches users; **M** = rework or a degraded feature; **L** = hygiene.

## Summary (ranked)

| # | Sev | Units | Divergence | Fix |
|---|-----|-------|-----------|-----|
| 1 | C | Hub restore vs Standby force-promote | Both mint epoch N+1 locally, so two writers share an epoch and both issue `E<N+1>/000001` | AD-11: epoch = `(counter, hub_instance_id)`; mint at the cloud when online |
| 2 | C | Cloud ingest vs promoted Standby | The new epoch "continues the sequence" from the standby's seq. The old primary's unreplicated tail stays in the mirror as ghost rows | AD-4/11: each epoch declares a fork point; ingest rolls back the old tail above it |
| 3 | C | Hub poller (old epoch) vs Command relay | The old primary applies the command and POSTs `applied`. The relay closes it, but the new primary never had it | AD-5: fetch and result carry epoch; the relay re-queues after a fence |
| 4 | C | Owner PWA offline queue vs Hub poller | Command id = client ULID (AD-14), and the Hub reads `?after=` in id order (AD-5). A late-posted, older-ULID command gets skipped | AD-5: the relay assigns a monotonic `cursor_seq`. The idempotency key is a separate field |
| 5 | C | Hub erasure vs R2 / standby / mirror | AD-2 says the log is append-only, but AD-24 says "anonymise in the change log". Batches kept 1–3 y still hold PII | AD-24: crypto-shred PII with a per-player key |
| 6 | H | Cloud ingest vs Command relay vs heartbeat | Each tracks "current epoch" separately, and none owns it | New AD: one `branch_fence` row in `gz_control` with a single writer |
| 7 | H | Hub undo vs Command poller / other staff | The reverse intent restores the old state over an owner change made in between | AD-13: reverse carries `expected_rev`, else `STALE` |
| 8 | H | KDS/kiosk offline queue vs Owner command | A queued local edit replays after the owner command and overwrites it, breaking owner-wins | AD-14: edit intents carry `base_rev`. Replay after an owner change is rejected `STALE` |
| 9 | H | Hub Z-report vs Cloud owner reports | Each derives business day and FY from tz on its own, with no cut-off past midnight and no rule for a tz change | AD-7: stamp `business_day` and `fy` on rows inside `apply()`. Reports never recompute them |
| 10 | H | Onboarding / gz_control vs Hub | Two owners of the branch profile (tz, GSTIN, branch code, hours). Branch code can change mid-FY | AD-3: bootstrap via first command. Branch code immutable after first invoice |
| 11 | H | Billing (commands) vs Entitlement (JWT) | AD-5 lists "billing writes" as commands, while AD-10/20 say entitlement comes through the JWT. That gives two paths | AD-20: entitlement only via JWT. Billing never sends branch commands that change gates |
| 12 | H | Hub `apply()` gates vs existing state on downgrade/expiry | Offline JWT expiry and caps over a downgrade leave live sessions and over-cap devices undefined | AD-29: a downgrade never breaks in-flight sessions. Over-cap devices are fixed by pairing order |
| 13 | H | Hub consent check vs Cloud sender | Consent is checked at request. Withdrawal or erasure before send is ignored, and the template version is unknown at the Hub | AD-27: sender re-checks at send. Hub stamps purpose, cloud picks the template |
| 14 | H | Primary vs Standby on different releases | The replayer ignores unknown columns, so after promotion an older standby silently loses data | AD-8/11: follow and promote only if standby schema ≥ primary's |
| 15 | H | Old primary invoices vs Ingest quarantine | Invoices printed by the fenced-out primary are quarantined, so the GST store of record has no copy | AD-26: quarantined money rows are kept and surfaced for reconciliation |
| 16 | M | Hub undo vs Notification sender | A notify row can upload (500-row trigger) and send inside the undo window | AD-27: `not_before = commit + undo window`. A cancel before send suppresses it |
| 17 | M | Ingest retries / quarantine vs Sender | Re-ingest or quarantined batches resend messages | AD-27: send ledger keyed by outbox row ULID. Quarantined rows never send |
| 18 | M | Owner PWA `branch: all` vs Relay fan-out | One user key becomes N command ids, so the dedupe key is ambiguous | AD-14: dedupe on `(idempotency_key, branch_id)` |
| 19 | M | Kiosk offline elevation vs Hub AD-30 | Kiosk verifies the PIN from a cached verifier, then queues an intent with `elevated_by` that the Hub never checked | AD-30/31: `elevation_mode` field. Hub flags cached-verifier elevations |
| 20 | M | Actor fields across Hub / cloud / admin / system | Staff ULID, owner account id, SSO admin email and system tasks share one `actor` field | AD-2: typed `actor{kind,id}` |
| 21 | M | Owner PWA (new) vs Hub (old) mid-rollout | The PWA sends command `schema_version` v+1 to a branch still on v, which returns `UNSUPPORTED` during a week-long rollout | AD-8: relay knows each branch's capabilities from heartbeat. PWA sends only supported versions |
| 22 | M | Rollout rollback vs forward-only migrations | Previous build re-flagged meets `SCHEMA_TOO_NEW` and will not start | Operational Envelope: rollback = roll-forward fix, or n-1 reads n |
| 23 | M | Catalogue sync / JWT / media vs promoted Standby | Not in the change log, so a promoted standby has no catalogue and no cloud credential | AD-17/1: reference cache outside branch state, refetched by any primary. Standby paired as its own Hub device |
| 24 | M | Kiosk offline lock vs Hub grace / time-up mode | Kiosk locks at remaining=0, but the Hub allows grace, "staff handles" mode and changes rate at pricing windows | AD-12: Hub sends `lock_at_remaining_ms` with grace and mode already applied |
| 25 | M | Pi agent vs Hub / QR phone | No offline rule for Pi, and no path for the phone QR request (LAN or cloud) | AD-12 covers Pi. New rule: QR page served by Hub over LAN, anonymous `console.plan_requested` |
| 26 | M | Owner live view (tunnel) vs Command relay | Live view is on the LAN WS, so it could send owner writes directly and skip AD-5 | AD-5/25: live view read-only. All owner writes go through the relay |
| 27 | M | Billing refunds vs Partner payouts | The payout job on the 1st counts revenue later refunded within 7 days. Month boundary tz is unspecified | AD-19: net of refunds with clawback ledger. Control-plane tz is Asia/Kolkata |
| 28 | L | Idempotency dedupe store vs queue age / pruning; snapshot producer | Dedupe retention, intent TTL and the snapshot owner for standby bootstrap are undefined | AD-14: dedupe window ≥ intent TTL. AD-4: name the snapshot producer and format |

## Details

### 1 (C) Duplicate epoch: restore vs force promote
- **A, Standby:** obeys AD-11 by force-promoting with a manager PIN, setting epoch = N+1 and issuing `KOR/26-27/E<N+1>/000001`.
- **B, Hub restore:** a technician restores last night's export on a replacement PC while the WAN is down. AD-11 says "restore … increment[s] the epoch", so this Hub is also N+1 and issues the same invoice numbers.
- **Why the ADs allow it:** nothing says *who* mints the epoch. A local increment is legal, so two machines can reach the same N+1.
- **Fix (tighten AD-11 + AD-26):** an epoch is `(counter, hub_instance_id)`, ordered first by counter and then by instance id. When the cloud is reachable, it allocates the counter (CAS on `branch_fence`, see #6). A locally minted epoch is provisional until the cloud acknowledges it. The invoice segment uses the full epoch token, e.g. `E3a`, where the suffix is a short instance tag. Ingest rejects a second claimant of the same counter.

### 2 (C) Sequence fork after promote: ghost rows in mirror
- **A, Standby:** obeys "a new epoch continues the sequence" by starting epoch N+1 at its last replicated seq X+1.
- **B, Cloud ingest:** has already ingested epoch N up to seq Y > X, because the old primary uploaded before it died. It upserts by `(tbl,row_id)`. Rows from N/X+1..Y never existed on the new primary but stay in the mirror for good. AD-4's "gap-free" check also gets an overlap (X+1 < Y), and one ingest build calls that a gap while another calls it a duplicate.
- **Fix (AD-4 + AD-11):** the first manifest of a new epoch carries `fork = {parent_epoch, parent_seq}`. Ingest treats parent-epoch rows above `parent_seq` as orphaned. It rebuilds the affected rows from the last checkpoint, or replays a compensating delete and restore, and quarantines the orphan batches. State that sequence numbers are per-branch monotonic and the gap check runs per epoch from `fork.parent_seq+1`.

### 3 (C) Command applied by a fenced-out primary
- **A, Old primary (partitioned on LAN, WAN up):** polls, fetches command K, applies it in epoch N, and POSTs `applied`. The result POST is idempotent, so the relay accepts it.
- **B, New primary (epoch N+1):** never sees K, because `applied_commands` was not replicated and the relay has closed K.
- **Fix (AD-5):** fetches and result POSTs carry the Hub's epoch. The relay rejects any result whose epoch is below `branch_fence.epoch`. When the fence moves, commands applied at the old epoch above the fork seq go back to `pending`. The heartbeat also carries the epoch, so a WAN-connected old primary is told to self-lock even if the LAN is partitioned.

### 4 (C) Command cursor skips late commands
- **A, Owner PWA:** obeys AD-14. It mints the ULID at user action while offline in a basement, then posts 20 minutes later. That ULID is the command id.
- **B, Hub poller:** obeys AD-5, `?after=<last id>` in id order. It has already advanced past newer ids, so K is never fetched.
- **Fix (AD-5 + AD-14):** separate the identities. `idempotency_key` is a client ULID used for dedupe. `cursor_seq` is a monotonic integer the relay assigns at insert, used for `?after=`. `applied_commands` keys on `idempotency_key`.

### 5 (C) Erasure vs append-only log
- **A, Hub:** AD-2 makes the change log append-only. Erasure appends a tombstone.
- **B, R2 / standby / mirror:** earlier full-row batches (AD-4) still hold name and phone for 1–3 years (AD-24 retention). The standby writes rows verbatim. "Anonymise in the change log" is either impossible under AD-2 or means rewriting R2 objects, and two teams will pick differently.
- **Fix (AD-24):** player PII columns are stored encrypted under a per-player data key held in a Hub key table, which is mirrored as a wrapped key. Erasure deletes the key (crypto-shred) and appends a tombstone. Batches stay immutable. Outbox rows reference `player_id`, never a raw phone number.

### 6 (H) No owner of the cloud's view of the epoch
- Ingest (quarantine cursor), the command relay (#3), heartbeat and the restore flow each need "current epoch". With no named owner, each role keeps its own copy, and those copies drift.
- **Fix (new AD):** `gz_control.branch_fence{epoch, instance_id, fork_seq, updated_at}`. The control role is the single writer, using CAS from a promote, restore or first-manifest event. Every other role reads it.

### 7 (H) Undo overwrites an intervening change
- **A, Hub UI:** staff ends a session and then taps Undo within 8 s. `session.end.reverted` restores the pre-end row.
- **B, Command poller:** in between, it applied an owner rate change (or another staff member extended the session). The literal reverse restores the old row and erases the owner's change, which breaks AD-5 owner-wins.
- **Fix (AD-13):** each reverse intent carries the `rev` that the forward intent produced and fails `STALE` if the row's `rev` has moved. Undo of an elevated action needs the same elevation, or the original `elevated_by` must be within the window.

### 8 (H) Late queued intent beats the owner command
- **A, KDS:** offline, marks item X unavailable at t0 and replays at t2 (AD-14, creation order).
- **B, Poller:** applied an owner edit to X at t1. Apply order makes the stale local change win, against the inherited constraint "owner command wins over prior local change".
- **Fix (AD-14 + AD-5):** every edit intent carries `base_rev`. If an owner command changed the row since `base_rev`, a replay is rejected `STALE` and the UI drops it (existing rule). Owner commands may carry `base_rev` too, but they always apply and list the overwritten local change in the "updated by owner" event.

### 9 (H) Business day and FY computed twice
- **A, Hub Z-report:** computes the business day from branch tz at local midnight.
- **B, Cloud "Today" / FY reports:** compute it from the mirror's UTC timestamps plus tz, maybe with a 4 AM cut-off for late-night cafes. The totals differ. A tz change edited by command also re-buckets history differently on each side. Rounding: cloud reports might re-derive GST from lines.
- **Fix (AD-7 + AD-26):** `apply()` stamps `business_day` (with a per-branch `day_start_min` cut-off) and `fy` on every session, money row and invoice. Reports aggregate stamped columns and stored invoice totals, and never recompute. A tz change takes effect from the next business day.

### 10 (H) Two owners of the branch profile
- **A, Onboarding:** writes branch name, tz, GSTIN, branch code and hours into `gz_control`, as AD-3 allows ("cloud writes control tables").
- **B, Hub:** owns branch rows (AD-3) and keeps its own copy, edited locally. The cloud's branch-offline alert reads hours from `gz_control` while the Hub uses its own. Branch code is the invoice prefix (AD-26). Editing it mid-FY splits the series.
- **Fix (AD-3):** onboarding values reach the Hub as a `branch.bootstrap` command applied at first pairing. After that, the Hub is the only owner, and cloud consumers read the mirror. `branch_code` is assigned by the control plane, unique per tenant, and immutable once an invoice exists.

### 11 (H) Two entitlement paths
- AD-5 says "Owner/admin/billing writes become commands", while AD-10/20/29 say entitlement comes from the JWT. One team builds `subscription.changed` commands that flip branch rows, and another reads JWT flags. They disagree for up to one heartbeat or 7 days.
- **Fix (AD-20):** entitlement state comes only from the JWT. Billing may send informational commands (receipts, banners) and never ones that change gates.

### 12 (H) Downgrade and expiry vs live state
- A Hub offline for more than 7 days reaches JWT `exp`. AD-10 grace/lapsed is computed in the cloud and cannot be known offline. A Basic→Free downgrade leaves 40 paired devices against a lower cap, an active kiosk order and a Windows Hub that is "rejected on Free" only at pairing.
- **Fix (AD-29):** offline expiry follows a fixed timeline, `exp` + 7 d grace, after which local lapse uses Hub monotonic time plus wall time to resist clock rollback. Downgrades never end in-flight sessions or orders. Over-cap devices stay paired and read-only, chosen by newest pairing first. A Windows Hub on a lapsed licence keeps running at Free limits.

### 13 (H) Consent checked once, sent much later
- Hub checks consent at `notify.requested`. The branch is offline for 2 days, the player withdraws or is erased, and the cloud sends anyway. The Hub also cannot check "template version" consent, because templates live in the cloud.
- **Fix (AD-24 + AD-27):** the Hub stamps `purpose` (transactional/marketing) and the consent policy version. The cloud picks the template and, at send, re-checks the mirror for consent withdrawal or a tombstone with `as_of` ≥ the request. Withdrawal cancels pending sends.

### 14 (H) Standby on an older schema
- AD-8 allows mixed versions during rollout. The replayer "ignores unknown columns" (AD-4), so a standby one release behind drops new columns, and promotion silently loses them.
- **Fix (AD-8 + AD-11):** the standby follows only if its schema is ≥ the primary's. Otherwise it reports `SCHEMA_BEHIND` and refuses promotion until updated. The rollout updates the standby before the primary.

### 15 (H) Printed invoices lost to quarantine
- The old primary issued and printed `E2/000150`, and its batch arrives after the fence and is quarantined (AD-11). AD-24 names the mirror as the GST store of record, but it now has no copy of an invoice the customer holds.
- **Fix (AD-26):** quarantined invoice, credit-note and payment rows are kept in a `quarantine_money` store and appear on an owner reconciliation screen. "Audit gaps" are computed from quarantined numbers, not guessed.

### 16 (M) Undo vs a sent notification
- AD-13's list of irreversible effects leaves out notifications. A 500-row burst triggers an immediate upload (AD-4), so the WhatsApp receipt goes out inside the undo window.
- **Fix:** outbox rows carry `not_before = commit + undo_window`, and the sender skips rows whose forward intent was reverted. Alternatively, add `notify.sent` to AD-13's irreversible list.

### 17 (M) Duplicate sends
- Gap re-uploads, ingest retries and quarantined batches all replay outbox rows. AD-27 claims to prevent duplicates but names no key.
- **Fix:** a cloud send ledger with a unique key on the outbox row ULID. Quarantined batches never trigger sends.

### 18 (M) Fan-out dedupe key
- `branch: all` turns one PWA action into N per-branch commands. If command id = idempotency key (AD-14), N rows share one id, or else the key is lost.
- **Fix:** dedupe on `(idempotency_key, branch_id)`. `group_id` = the user's key.

### 19 (M) Unverified offline elevation
- The kiosk verifies a manager PIN against the cached verifier (AD-31) and queues `kiosk.admin_exit{elevated_by}`. AD-30 says elevation is verified at the Hub, so the Hub either rejects it or records an unverified claim.
- **Fix:** add `elevation_mode: hub | cached_verifier{version}`. The Hub accepts cached-verifier elevations only for the offline-exit intent and flags them in the audit. Rotating a PIN bumps the verifier version, and older versions are rejected.

### 20 (M) Actor namespace collision
- Change rows hold `actor` for staff (Hub ULID), owner (`gz_control` account), admin (SSO identity), devices, and system tasks (printer result intents, cloud delivery commands).
- **Fix (AD-2/30):** `actor = {kind: staff|owner|admin|device|system, id}`. Owner step-up MFA is recorded as `elevated_by{kind: mfa}`.

### 21 (M) Command schema ahead of the Hub
- During the week-long Hub rollout, an updated PWA emits v+1 commands to v Hubs, which return `UNSUPPORTED` (AD-5).
- **Fix (AD-8):** heartbeat reports the Hub's supported command schema range. The relay and PWA downgrade or block the action with "update the branch" before sending.

### 22 (M) Rollback vs forward-only migrations
- Operational Envelope says "rollback = previous build re-flagged", but under AD-8 the previous build returns `SCHEMA_TOO_NEW` and will not start.
- **Fix:** rollback = roll-forward hotfix, or require each build to open schema n+1 read/write, since migrations are additive.

### 23 (M) State outside the change log at promotion
- The catalogue cache, media, JWT and cloud credential are not branch rows, so the follower never receives them. Under AD-2 an implementer may also write catalogue rows outside `apply()` (side door) or inside it (bloating the log).
- **Fix:** define a "reference cache" class (catalogue, media, JWT) stored outside branch state, which any primary refetches on promotion. Game↔catalogue matches are branch rows written via a `game.matched` intent. The standby is paired as its own Hub device with its own token. Media refs are content-addressed (sha256) so CDN and Hub caches never serve stale art.

### 24 (M) Kiosk offline lock semantics
- AD-12 has the kiosk lock at remaining=0. Grace with Extend, time-up mode "staff handles" and pricing-window rate changes mean the Hub's real lock moment differs.
- **Fix:** each Hub tick carries `lock_after_ms`, already resolved for grace, mode, wallet balance and rate windows. The kiosk counts that down on its monotonic clock.

### 25 (M) Pi and phone QR path undefined
- AD-12 covers kiosks only, so Pi behaviour (TV warnings, input switch) when the Hub is unreachable is unspecified. The phone QR page could be served by the cloud (as a command) or by the Hub on the LAN. Guest Wi-Fi isolation decides whether the LAN path works at all.
- **Fix:** extend AD-12 to Pi agents. Add a rule: the QR page is served by the Hub on `:7401` with a short-lived per-console token, falling back to a cloud-relayed command. The request is an anonymous `console.plan_requested`, and staff confirmation carries the actor.

### 26 (M) Live view as a second write path
- AD-25 binds "owner live view" to the LAN WS through the tunnel. A literal build lets it send intents, which skips AD-5 dedupe, owner-wins notification and step-up MFA.
- **Fix:** live view is read-only, and every owner write goes through the command relay.

### 27 (M) Refunds vs payouts; control-plane time
- The job on the 1st pays 10% on revenue that is refunded by the 7th. Billing and payout code may use UTC or IST month edges.
- **Fix (AD-19):** commission on revenue net of refunds, with negative ledger lines carried forward (clawback against the ₹500 rollover). Control-plane business tz = Asia/Kolkata, FY Apr–Mar, same labels as AD-7.

### 28 (L) Dedupe retention and snapshot owner
- AD-14 dedupe records have no retention. With Hub `changes` pruned at 90 days (AD-24), a very old queued intent could re-apply. AD-11 and DR rely on "snapshots", but no AD names their producer, cadence or format, so standby bootstrap after pruning is undefined.
- **Fix:** intents carry a TTL (≤ 7 days, matching commands), and dedupe records are kept longer than the TTL. The Hub uploads a snapshot `{epoch, seq}` on a fixed cadence. Standby bootstrap = snapshot + follow.

## Suggested spine edits

New ADs: **branch_fence** (#1, #3, #6), **optimistic concurrency `base_rev`/`expected_rev`** (#7, #8), **stamped business_day/fy** (#9), **reference-cache class** (#23). Tightened: AD-2 (typed actor), AD-3 (branch bootstrap, immutable code), AD-4 (fork point, snapshot), AD-5 (cursor_seq, epoch on results, read-only live view), AD-8 (standby ≥ primary, capability-aware commands, rollback), AD-12 (lock_after_ms, Pi), AD-13 (expected_rev, notify), AD-14 (dedupe key per branch, TTL), AD-19 (net of refunds, IST), AD-20 (JWT-only gates), AD-24 (crypto-shred), AD-26 (quarantined money), AD-27 (send ledger, re-check, not_before), AD-29 (downgrade rules), AD-30/31 (elevation_mode).
