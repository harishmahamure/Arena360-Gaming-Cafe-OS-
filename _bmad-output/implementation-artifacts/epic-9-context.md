# Epic 9 Context: Resilience, backups and data rights

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

A branch survives Hub failure and keeps its data in its own hands. A paid branch pairs a LAN standby Hub that follows the main Hub live and can be promoted by a manager in under 10 s; a returning old main locks itself so exactly one Hub ever serves devices. Every tier can export the whole database and restore it; paid tiers get cloud backup with a restore path onto replacement hardware, plus an 8-year GST archive. Player erasure is crypto-shredding that stays in force across backups, promote and restore.

## Stories

- Story 9.1: Erase a player's personal data by crypto-shredding
- Story 9.2: Player keys are backed up and the branch key is escrowed
- Story 9.3: The cloud issues epoch counters by compare-and-set on the branch fence
- Story 9.4: One-tap full database export to a folder or USB drive
- Story 9.5: The Hub saves a daily export to the backup location
- Story 9.6: The export format is documented and every report exports alongside it
- Story 9.7: Restore a branch from an export file as a new epoch
- Story 9.8: Owner replaces a failed Hub and gets a new pairing code
- Story 9.9: A replacement Hub opens in "Restore this branch" and serves nothing until restored
- Story 9.10: Owner releases the branch key to the replacement Hub
- Story 9.11: Upgrading from Free uploads a full snapshot before the first batch
- Story 9.12: Fetch a paid branch's cloud backup onto a replacement Hub
- Story 9.13: Rebuild the branch from the fetched backup as a new epoch
- Story 9.14: The Hub emits a yearly GST archive kept for 8 years
- Story 9.15: Backups are kept for the plan's retention and GST records for 8 years
- Story 9.16: Cloud backup status in the Hub header and System screen
- Story 9.17: Owner sees backup status and sends "Backup now"
- Story 9.18: Owner issues a standby code and a second PC pairs as standby
- Story 9.19: The standby bootstraps from the main Hub and follows it live
- Story 9.20: The standby keeps its own catalogue, media and licence and shows its status
- Story 9.21: Promote the standby to main with a manager PIN
- Story 9.22: Force promote after a data-loss warning
- Story 9.23: The new main Hub lists what was set aside after a promote or restore
- Story 9.24: A returning old main Hub locks itself
- Story 9.25: Make the old main Hub the standby
- Story 9.26: Failover and split-brain proven on the test LAN
- Story 9.27: Restore drill and privacy proof

## Requirements & Constraints

- Promotion is manual only; there is no auto-promote. Promote completes in under 10 s (PIN confirmation to first accepted intent) and kiosks reconnect by themselves (mDNS re-resolve, jittered backoff 2-30 s). Force promote after "Data since the last sync may be lost. Promote anyway?" is audited and alerts the owner.
- Exactly one main Hub serves devices: a returning old main shows "This PC is no longer the main Hub", rejects hellos with `FENCED`, offers "Make this the standby" and keeps its post-fork rows for reconciliation.
- Durability: no committed transaction is lost on Hub crash; at most one upload interval is lost on disk failure without a standby.
- Export is available on every tier with no tier-lock: one tap, 1 GB in under 10 s, documented format, reports exported alongside. Restore from file opens a new epoch.
- Cloud backup by tier: Free uploads nothing (upgrade uploads a full snapshot first); Basic 15-min batches / 1-year retention; Premium 5-min / 3-year. Yearly GST archive kept 8 years on paid tiers; GST money rows stay in the Hub 8 years on all tiers. Hub `changes` pruned 90 days after upload (never unuploaded rows); audit cache 24 h.
- Privacy (DPDP): player PII is encrypted per player; batches, snapshots, exports and audit caches carry ciphertext only; logs carry player ids only. Erasure must hold in Hub, standby, backups, snapshots, and after restore or promote. Erased players show as "Erased player" with id; decrypt fails `KEY_ERASED`.
- Owner-side actions on the Owner app are cloud commands with step-up MFA; the backup status on a paid Hub header is `upload_cursor`/`snapshot_log`/`backup_state`; "Backup now" is rate-limited to once per 15 minutes.

## Technical Decisions

- Epoch is `(counter, hub_instance_id)`. The counter comes from the cloud by compare-and-set on the single `branch_fence` row, written only by the control role (one write path in `arena-cloud`); other roles only read. Offline, a Hub mints `max seen + 1` provisionally and the first heartbeat confirms it or tells it to self-lock. Only promote and restore open a new epoch, and both re-apply every `player.erased` tombstone before the writer opens. A claim carries `fork {parent_epoch, parent_seq}`; the first batch of a new epoch declares that fork. Rows of the old epoch above the fork are never applied by replay, standby, restore or the audit builder; they are listed as orphaned. Invoice numbers are never reused (new epoch writes a new invoice segment).
- Crypto-shredding: per-player keys in `player_keys` wrapped by the branch KEK; erasure deletes the key, clears PII, drops `phone_lookup`, sets `players.erased_at`, keeps ids, money, history and consent. Key backup `keys/player-keys.bin` (presign `kind: keys`) on paid tiers only; the KEK wrapped with the cloud escrow public key is stored in `gz_control.kek_escrow` on every tier (the one key-only exception to "Free uploads nothing"), read only by the control role, every access written to `admin_audit`.
- New presign kinds `keys` and `gst_archive` follow the snapshot rules (own-prefix check, `FENCED`). Retention sweep never touches `gst-archive/`, `keys/` or `state/` and keeps the snapshot a retained batch range needs.
- Export format `.a360`: versioned header in `arena-proto` (format version, branch id, `branch_code`, epoch, last `seq`, schema version, exported_at, sha256) plus a zstd `VACUUM INTO` copy; excludes reference cache, licence cache, device tokens and keystore secrets. Epic 10 reuses it for the Android-to-Windows move.
- Restore on new hardware: owner revokes the old Hub (`hub.revoke`, sensitive), new Hub pairs and opens "Restore this branch" (serves nothing, `RESTORE_PENDING`), requests the KEK via `key_requests`, owner approves with step-up (`backup.restore`, sensitive), then restore from file or from cloud (`POST /v1/restore/plan`, read URLs, `RESTORE_GAP` handling, `arena-replay`). Kiosks must pair again after cloud restore (pinned certificate).
- Standby: Windows-only, `standby` licence flag, one per branch; needs the owner's standby code plus a 6-digit "Link standby" code at the main Hub. It bootstraps by LAN snapshot, follows `wss://<main>:7400/v1/replicate`, writes rows verbatim with same `seq`/epoch, replicates key changes, is read-only (`NOT_PRIMARY`), and keeps its own reference cache and licence. Kiosks receive and pin the standby's fingerprint. Kitchen displays cannot follow a promote and must be re-paired with a fresh code.
- Roles: `ops.promote` goes to the Manager role; `ops.restore` is Owner-only. Every new intent is added to the Story 1.8 `proptest` generator.

## UX & Interaction Patterns

- Destructive and failover actions use in-place `inline-pin` (manager or Owner PIN), not modal detours; restore also requires typing the branch code and shows exact consequences ("Everything after <time> on this PC will be set aside").
- Hub header shows muted backup status ("Backed up 4 min ago", "· 3 pending"), becomes a warning `status-badge` "Backup behind" after 2x the upload interval, hidden below 840px (still in System -> Backup). Free shows `tier-lock` "Available on Basic" for cloud backup.
- Owner app uses `command-row` states (Queued on this device -> Waiting for branch -> Applied) and the Account -> Branches pairing card with expiry for Replace Hub, standby code, key release and Backup now. Android Hub branches show "Needs the Windows Hub" for standby.

## Cross-Story Dependencies

- Builds on Epic 8: the command mailbox (`FENCED` results re-queue, `move_branch_fence` exists but is first called by Story 9.3), `backup.now`, daily snapshot and `snapshot_log`, batch uploader with fork header, `arena-replay` fork handling, the notify drain eligibility rules (Story 9.1 adds the erasure rule), and the Hub-raised owner alert path (Story 9.22 adds `standby_forced`).
- Story 9.3 (fence) underpins 9.7, 9.13, 9.21, 9.22 and 9.24; 9.2 (KEK escrow, key backup) underpins 9.10, 9.12 and 9.13; 9.4 (export) underpins 9.5-9.7; 9.8 -> 9.9 -> 9.10 -> 9.12 -> 9.13 form the replacement-Hub chain; 9.18 -> 9.19 -> 9.20 -> 9.21 -> 9.22 -> 9.24 -> 9.25 form the standby chain; 9.26 and 9.27 are release-blocking proof suites over everything before.
- Feeds Epic 10 (upgrade snapshot, `.a360` format, KEK release for the Android-to-Windows move) and Epic 14 (off-site standby). Epic 12 Pis join the device re-pair and fingerprint paths when they ship.
