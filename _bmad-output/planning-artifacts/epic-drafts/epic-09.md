## Epic 9: Resilience, backups and data rights

A branch survives Hub failure through standby and promote, and keeps exports, cloud backups, the GST archive and an erasure that holds up.

> Draft notes: this epic builds on the Epic 1 store and lease (Story 1.4: WAL + fsync, `hub_instance_id`, provisional epoch, follower role, `NOT_PRIMARY`), `apply()` and the change log (Stories 1.5–1.6), the LAN envelope and client classes (Story 1.7), inline PIN (Story 1.11), the audit viewer (Story 1.13) and `chip-offline` (Story 1.20); on the Epic 2 heartbeat (Story 2.10: `{licence, fence, cloud_time}`, first-heartbeat fence creation), entitlement (Story 2.11), Hub pairing and the installer (Stories 2.8–2.9), kiosk discovery and pinned-fingerprint pairing (Stories 2.16–2.17), the device cap that already excludes standby Hubs (Story 2.18), the backup location setting (Story 2.25), Account → Branches with the standby-code placeholder (Story 2.29) and the go-live "standby PC" item (Story 2.30); on Epic 3 player profiles with per-player keys in `player_keys` wrapped by the branch KEK (Story 3.6) and lookup that already hides erased players (Story 3.7); on the Android folder picker (Story 4.7); on Hub report exports (Story 5.32) and the epoch-segmented invoice series (Story 5.14); on the reference cache that a standby or promoted Hub keeps or refetches (Story 6.2) and kiosk reconnect-and-reconcile (Story 6.36); and on the Kitchen display reconnect (Story 7.10). From Epic 8 it uses, by title: "Presign issues batch and summary upload URLs under the branch's own R2 prefix" (extended here with `kind: keys` and `kind: gst_archive`), "The Hub uploads gap-free change-log batches at its tier cadence" (which already writes the `fork` header from the `lease` fork fields), "The heartbeat detects upload gaps and the Hub resends them", "The Hub uploads a daily snapshot after close" (`snapshot_log`), "The Hub applies owner commands exactly once, and the owner's change wins" (`backup.now`), "Commands are swept, expire after 7 days, and are fenced by epoch" (`FENCED` results and re-queue when the fence moves), "Step-up MFA for sensitive commands" (reserved sensitive kinds `hub.revoke` and `backup.restore`, delivered here), "The Hub drains its notification outbox after the undo window" (skips erased players), "Owner alerts by web push and WhatsApp, and the Alerts tab" and "The audit builder rebuilds a branch month on demand" (`arena-replay` honouring fork points). **Epoch (decided by the spine):** `(counter, hub_instance_id)`; the counter comes from the cloud by compare-and-set on the single `gz_control.branch_fence` row, written only by the control role; offline, the Hub mints `max seen + 1` provisionally and the first heartbeat confirms it or tells it to self-lock. Promote and restore are the only things that open a new epoch, and both re-apply every `player.erased` tombstone before the writer opens. **Promotion is manual only** — there is no auto-promote anywhere. **Roles (decided):** `ops.promote` is granted to the built-in Manager role (Journey 8: "any manager's PIN"); `ops.restore` stays Owner-only. This amends the LLD role table ("Manager: all except `ops.*`"). **Standby** is Windows-only and needs the `standby` licence flag (paid tiers); MVP allows one LAN standby per branch. Off-site standby (FR172) is Epic 14. **Snapshots:** the daily snapshot is Epic 8's; this epic adds the upgrade-first snapshot, retention, the GST archive, the key backup, restore and the backup status. **Data rights:** erasure is crypto-shredding — batches, snapshots, exports and audit caches only ever carry ciphertext, so deleting the key is the erasure. The branch KEK is escrowed in `gz_control` on every tier, Free included (decided): this is the one key-only exception to "Free uploads nothing" — it is a control row, never an object under the branch prefix — so a Free branch can restore its own export on new hardware. This amends the LLD role table ("Manager: all except `ops.*`") as stated above. **Known limits (accepted):** (1) on Free, a local export taken before an erasure, restored on a new PC with no access to the old database, cannot learn of that erasure; paid branches close the gap with the cloud key backup; (2) after a cloud restore onto new hardware, kiosks must pair again, because they pinned the old PC's certificate; (3) a standby needs two codes — the owner's standby code from the cloud, then the 6-digit "Link standby" code shown at the main Hub. **Out of this epic:** off-site standby (Epic 14), the Android → Windows migration (Epic 10, which reuses the export format from Story 9.4), owner-side report exports from rollups (Epic 8). Every story adds its new intents to the Story 1.6 `proptest` generator.

### Story 9.1: Erase a player's personal data by crypto-shredding

As a café manager,
I want to erase a player's personal data when they ask,
So that the branch meets DPDP and that player's details can never be read again.

**Covers:** FR200 (Hub erasure), AD-24, AD-2, AD-30, NFR13, UX-DR66, UX-DR103

**Acceptance Criteria:**

**Given** a player profile on the Hub
**When** a Manager chooses "Erase personal data"
**Then** an in-place panel shows what is erased (name, phone, email, DOB, notes, username) and what is kept (player id, money and session history, consent record), with `inline-pin` for the manager PIN

**Given** the PIN is confirmed
**When** `player.erased` (permission `player.manage` + manager PIN) is applied
**Then** in one transaction it clears the PII columns, sets `erased_at`, deletes the player's `player_keys` row and removes the `phone_lookup` hash; the consent row is kept with the tombstone; the change rows carry only the player id and `erased_at`

**Given** the player has an active, paused or grace session, or credit outstanding above zero
**When** erasure is attempted
**Then** it fails `ERASE_BLOCKED` with the reason ("End the session first" / "Collect or write off credit first") and nothing changes

**Given** the player has a wallet balance above zero
**When** erasure is confirmed
**Then** the panel first requires the balance to be refunded at the counter or forfeited with a manager reason, recorded as a normal wallet intent before the tombstone

**Given** an erased player
**When** any screen, report, receipt reprint, audit row or export shows them
**Then** they appear as "Erased player" with their id and no personal data, and the in-memory lookup index drops them at once

**Given** queued notifications for an erased player
**When** the drain from "The Hub drains its notification outbox after the undo window" re-checks the row at send time
**Then** the row is marked `skipped` with reason `erased` and nothing is sent

**Given** a ciphertext column of an erased player in any older change row
**When** anything tries to decrypt it
**Then** decryption fails with `KEY_ERASED` and the caller shows "Erased player"

### Story 9.2: Player keys are backed up and the branch key is escrowed

As a café owner,
I want the keys that protect my players' data kept safely off the Hub,
So that I can restore my branch on a new PC without ever being able to bring an erased player back.

**Covers:** FR200 (key backup), AD-24, AD-22, AD-36, AD-9, NFR13, NFR14

**Acceptance Criteria:**

**Given** a Hub pairs (any tier), or an already paired Hub has no escrow record
**When** pairing completes or the next heartbeat runs
**Then** the Hub sends its branch KEK wrapped with the cloud escrow public key to `POST /v1/kek/escrow`, the control role stores it in `gz_control.kek_escrow(branch_id, hub_instance_id, wrapped_kek, created_at)`, and the escrow private key exists only as a SOPS-managed cloud secret

**Given** a paid-tier Hub and any change to `player_keys` (create, erase)
**When** the change commits
**Then** within one upload interval the key backup task wraps the whole table with the branch KEK and PUTs it to `keys/player-keys.bin` via presign `kind: keys` (new presign kind, same prefix check and `FENCED` rule as snapshots; overwritten, unversioned), and an erased player's key is absent from that object

**Given** a Free-tier Hub
**When** `player_keys` changes
**Then** nothing is uploaded under the branch prefix; the escrowed KEK in `gz_control` is the only key material held by the cloud, the single key-only exception to "Free uploads nothing"

**Given** the key backup PUT fails or the WAN is down
**When** the task retries with backoff
**Then** only the latest table state is uploaded once connectivity returns, and the time of the last successful key backup and the last error are recorded in `backup_state` (new Hub table, outside the change log)

**Given** the escrow endpoint
**When** a caller reads or writes it
**Then** only the control role can read the wrapped KEK, no endpoint in this story returns it, and every access is written to the admin audit

**Given** a privacy test scanning a batch, a key backup object and the Hub log after creating and erasing a player
**When** it runs on every push
**Then** no plaintext name, phone or email appears in any of them

### Story 9.3: The cloud issues epoch counters by compare-and-set on the branch fence

As a café owner,
I want the cloud to decide which PC is the one true main Hub whenever my branch changes Hubs,
So that two PCs can never both take orders and money for my branch.

**Covers:** AD-38, AD-11, AD-1, NFR10

**Acceptance Criteria:**

**Given** a Hub with a valid device token
**When** it calls `POST /v1/fence/claim {expected: <current fence>, instance_id, reason: promote|restore|confirm, fork: {parent_epoch, parent_seq}}`
**Then** the control role compare-and-sets `branch_fence{epoch_counter + 1, instance_id, fork_seq, updated_at}` and returns the new epoch; if `expected` no longer matches it returns `FENCED` with the current fence and writes nothing

**Given** two claims for the same branch at the same moment
**When** both reach the control role
**Then** exactly one succeeds and the other gets `FENCED` (integration test with concurrent claims)

**Given** a Hub holding a provisional epoch minted offline
**When** its first heartbeat after reconnect arrives
**Then** if no higher fence exists the cloud confirms it by compare-and-set and the Hub clears `lease.provisional`; if a higher fence exists the response carries it and the Hub closes its writer channel (Story 1.4 demotion) and stops accepting LAN intents with `NOT_PRIMARY`

**Given** two Hubs that each minted the same provisional counter offline
**When** both confirm on reconnect
**Then** the first compare-and-set wins and the second receives `FENCED` with the winner's epoch and demotes, whichever has the higher instance id

**Given** a successful claim
**When** the control role commits it
**Then** it stores `fork_seq` from the claim, so the re-queue rule in "Commands are swept, expire after 7 days, and are fenced by epoch" uses the real fork point instead of a fixture

**Given** any role other than control (command, builder, jobs)
**When** it needs the current epoch
**Then** it reads `branch_fence` and never writes it (code review gate: one write path in `arena-cloud`)

**Given** the Hub
**When** it sees an epoch on the LAN, in a heartbeat or in a claim reply
**Then** it records the highest seen counter in `lease`, so an offline claim mints `max seen + 1`

### Story 9.4: One-tap full database export to a folder or USB drive

As a café owner on any tier,
I want to save my whole branch database to a folder or USB drive with one tap,
So that I always hold my own copy of my data.

**Covers:** FR173 (export), FR199 (DB export), AD-8 (export format in `arena-proto`), AD-11, AD-24, NFR6 (1 GB < 10 s), NFR13, UX-DR88

**Acceptance Criteria:**

**Given** Hub → System → Backup
**When** a Manager taps "Export now"
**Then** `backup.export` (permission `backup.export`) writes `<branch_code>-<yyyy-mm-dd-hhmm>.a360` to the backup location from Story 2.25 (or Story 4.7 on Android), and the panel shows file name, size and time on success

**Given** the export runs
**When** the file is written
**Then** it holds a versioned header defined in `arena-proto` (format version, branch id, `branch_code`, epoch, last `seq`, schema version, exported_at, sha256) followed by a zstd-compressed `VACUUM INTO` copy of the branch database, with `player_keys` still wrapped by the branch KEK
**And** it excludes the reference cache (catalogue, media), the licence cache, device tokens and every keystore secret

**Given** trading continues during an export
**When** intents arrive
**Then** they are never blocked; the export is a consistent point-in-time copy and its header `seq` names that point

**Given** a 1 GB branch database on the reference PC (2015 i3 + HDD)
**When** the export benchmark runs in CI
**Then** it completes in under 10 s from tap to file closed

**Given** no backup location is set, the location is not writable, or the USB drive is removed mid-write
**When** export is attempted
**Then** the panel shows the plain cause inline with "Choose location" or "Try again", any partial file is deleted, and nothing else on the Hub is affected

**Given** a Counter without `backup.export`
**When** they tap "Export now"
**Then** `inline-pin` opens for manager elevation, and the export is audited with both actors

### Story 9.5: The Hub saves a daily export to the backup location

As a café owner,
I want a copy saved automatically every day,
So that I have a recent backup even if nobody remembers to tap Export.

**Covers:** FR173, NFR9, UX-DR88

**Acceptance Criteria:**

**Given** a branch with a writable backup location
**When** the business day closes (closing time + 30 min, or the last shift close)
**Then** the Hub runs the Story 9.4 export as `actor{kind: system}` and keeps the newest 7 daily files, deleting older auto-export files only (never files a person exported)

**Given** the backup location is unavailable at that time
**When** the daily export fails
**Then** the Hub raises an `alert-badge` "Backup folder not available" in the header until the next successful export, and setup status reports backup location not set (Story 2.25 / 4.7)

**Given** System → Backup
**When** it opens
**Then** it lists the last 7 exports with time, size and "auto" or the staff name

### Story 9.6: The export format is documented and every report exports alongside it

As a café owner,
I want my data in a documented format I can open without Arena 360,
So that I can leave or audit at any time and my data is truly mine.

**Covers:** FR199, AD-8, AD-41, NFR16

**Acceptance Criteria:**

**Given** the export file format in `arena-proto`
**When** the docs build runs
**Then** it generates a public reference (header fields, decompression step, every table and column with type and meaning, money in minor units, ciphertext columns marked) for the help centre, and CI fails if the reference is stale against the migrations

**Given** System → Backup → "Export everything"
**When** a Manager picks a date range
**Then** the Hub writes one folder holding the Story 9.4 database file plus every Hub report from Story 5.32 for that range as CSV, with a `README.txt` linking the format reference

**Given** an export file
**When** a CI test decompresses it with the stock `zstd` tool and opens it with stock `sqlite3`
**Then** every table listed in the reference is present and its row counts match the source database

**Given** a Free branch
**When** export or "Export everything" is used
**Then** both work exactly as on paid tiers, with no `tier-lock`

### Story 9.7: Restore a branch from an export file as a new epoch

As a café owner,
I want to restore my branch from an export file on this PC,
So that a corrupted or wrongly changed database can be put back to a known good point.

**Covers:** FR173 (restore), FR200 (stays erased after restore), AD-11, AD-38, AD-24, AD-8, AD-30, UX-DR66, UX-DR103

**Acceptance Criteria:**

**Given** Hub → System → Backup → "Restore from file"
**When** the Owner picks a `.a360` file
**Then** the Hub checks the sha256 and branch id and shows branch name, exported time, epoch and last `seq`, and the warning "Everything after <exported time> on this PC will be set aside. Kiosks reconnect by themselves."

**Given** the file's branch id differs, its sha256 fails, or its schema is newer than the binary
**When** it is checked
**Then** restore is refused with `RESTORE_WRONG_BRANCH`, `RESTORE_CORRUPT` or `SCHEMA_TOO_NEW` and the current database is untouched; an older schema is migrated forward after restore

**Given** the Owner confirms with their PIN (`ops.restore`) and types the branch code
**When** restore runs
**Then** the Hub closes its writer, moves the current database to `orphaned/<epoch>-<timestamp>.db` (kept 90 days), loads the file, claims a new epoch (Story 9.3) with `fork = {file epoch, file seq}` (provisional if the cloud is unreachable within 3 s), re-applies every `player.erased` tombstone, opens the writer and announces `epoch.changed`

**Given** tombstones in the replaced database, or (paid tier, cloud reachable) players absent from the latest `keys/player-keys.bin`
**When** erasures are re-applied
**Then** each such player is erased in the restored database before the writer opens, so a player erased after the export stays erased

**Given** a Free branch restoring, on a new PC with no replaced database, an export taken before a player was erased
**When** restore runs
**Then** that player cannot be known to be erased and comes back with their data (accepted known limit); the confirmation panel says "Players erased after <exported time> may reappear." on Free

**Given** a Manager or Counter
**When** they open "Restore from file"
**Then** it asks for the Owner PIN in place and refuses any other role

**Given** a restore
**When** it completes
**Then** the audit records it as `ops.restore` with the file header, the old and new epoch, and the number of rows set aside

### Story 9.8: Owner replaces a failed Hub and the new PC receives the branch key

As a café owner whose Hub PC has died,
I want to pair a replacement PC to the same branch and let it unlock my backups,
So that I can get back to trading on new hardware.

**Covers:** FR173 (restore on new hardware), FR177, AD-10, AD-24, AD-5 (sensitive kinds `hub.revoke`, `backup.restore`), AD-22, UX-DR83

**Acceptance Criteria:**

**Given** Account → Branches in the owner app for a branch with a paired Hub
**When** the owner chooses "Replace Hub" and passes step-up MFA ("Step-up MFA for sensitive commands")
**Then** the reserved sensitive kind `hub.revoke` is delivered: the cloud revokes the old Hub's token at once (its next heartbeat is refused `TOKEN_REVOKED`), and a new single-use 8-character Hub pairing code is shown with the Story 2.8 card

**Given** a fresh Hub install paired with that code to a branch that already has history
**When** pairing completes
**Then** no `branch.bootstrap` is queued, and the Hub opens in a "Restore this branch" state offering "Restore from a file" and (paid tiers) "Restore from cloud backup", serving no devices until a restore completes

**Given** the new Hub in that state
**When** it requests the branch key
**Then** the owner app shows "Release branch key to <PC name>"; approving sends the reserved sensitive kind `backup.restore` (step-up MFA), shown as a `command-row`, and the control role then unwraps the escrowed KEK and returns it re-wrapped to a one-time public key the new Hub generated at pairing; the Hub stores it in the OS keystore

**Given** the KEK has been released
**When** the Owner restores from a file
**Then** the Story 9.7 flow runs on this PC (no replaced database), and erasures are re-applied from the cloud key backup when the tier allows

**Given** no approval within 7 days, or a denial
**When** the Hub asks again
**Then** it shows "Waiting for the owner to approve on their phone" and never receives the key; every release, denial and expiry is in the admin audit

### Story 9.9: Upgrading from Free uploads a full snapshot before the first batch

As a café owner moving from Free to a paid tier,
I want my existing history uploaded first,
So that my cloud backup is complete from day one, not from the upgrade.

**Covers:** FR174 (upgrade path), AD-36, AD-4, AD-29

**Acceptance Criteria:**

**Given** a Free branch whose next heartbeat delivers a paid licence
**When** the Hub applies it
**Then** before any change-log batch it uploads a full snapshot (the "The Hub uploads a daily snapshot after close" mechanism, key `snapshots/<today>.db.zst`, recorded in `snapshot_log`) and `keys/player-keys.bin`, then starts normal batching from the snapshot's `seq + 1`

**Given** the snapshot upload fails
**When** the uploader runs
**Then** no batch is uploaded until the snapshot succeeds, and `backup_state` records "first snapshot pending" with the last error

**Given** the cloud's view after upgrade
**When** heartbeat gap detection runs
**Then** it treats the snapshot `seq` (reported in the heartbeat as `base_seq`) as the starting point, so no `resend` is requested for history before it

**Given** a branch that lapses back to Free
**When** the lapse applies
**Then** uploads stop at once, already uploaded objects stay in R2, and re-upgrading starts again with a fresh full snapshot

### Story 9.10: Fetch a paid branch's cloud backup onto a replacement Hub

As a café owner on a paid tier,
I want my replacement PC to download my branch's cloud backup safely,
So that it has everything it needs to rebuild the branch.

**Covers:** FR173 (restore), FR174, AD-36, AD-4, AD-40, NFR11

**Acceptance Criteria:**

**Given** a Hub in the "Restore this branch" state with the KEK released (Story 9.8) on a paid tier
**When** the Owner chooses "Restore from cloud backup" and confirms with their PIN
**Then** the Hub calls `POST /v1/restore/plan`, which returns read URLs (10 min) for the newest snapshot, every batch after its `seq` across epochs with their fork headers, and `keys/player-keys.bin`, all under the caller's branch prefix only

**Given** the plan
**When** the Hub downloads the objects
**Then** it verifies every batch's sha256 and header, stores them in a staging folder, refreshes expired URLs by calling the plan again, and resumes after a dropped connection without re-downloading finished objects

**Given** a missing or corrupt batch inside the range
**When** the download check runs
**Then** it stops with `RESTORE_GAP` naming the seq range and offers "Restore up to <last good seq>" with the count of changes that would be lost; nothing is replayed until the Owner chooses

**Given** progress
**When** downloads run
**Then** the Hub shows "Downloading 3 of 41" with bytes and percent, and "Cancel" leaves the PC in the "Restore this branch" state with the staging folder removed

**Given** a Free branch, a caller without the released KEK, or a token for another branch
**When** `POST /v1/restore/plan` is called
**Then** it is refused `FEATURE_NOT_ENTITLED`, `KEY_NOT_RELEASED` or `FORBIDDEN` and returns no URLs

### Story 9.11: Rebuild the branch from the fetched backup as a new epoch

As a café owner on a paid tier,
I want the replacement PC to rebuild my branch exactly and open it as the new main Hub,
So that losing the Hub PC and its disk does not lose my business records.

**Covers:** FR173 (restore), FR200 (stays erased after restore), AD-11, AD-38, AD-4, AD-24, NFR9

**Acceptance Criteria:**

**Given** a complete staging folder from Story 9.10
**When** the Hub replays it with `arena-replay`
**Then** it starts from the snapshot, applies batches in order honouring fork points (parent-epoch rows above `fork.parent_seq` are skipped and counted as orphaned), unwraps the key backup with the KEK, and drops any key not in the key backup

**Given** the replay finishes
**When** the Hub opens the branch
**Then** it re-applies every `player.erased` tombstone, claims a new epoch (Story 9.3) with `fork = {last epoch, last seq replayed}`, opens the writer and announces `epoch.changed`

**Given** kiosks and Pis paired to the old Hub PC
**When** they find the restored Hub
**Then** they refuse its certificate (pinned to the old PC), and the restored Hub lists them under Settings → Devices as "Pair again" so staff re-pair each one with the 6-digit code (accepted known limit); their device records, groups and history are kept

**Given** replay
**When** it runs
**Then** the Hub shows "Replaying" and "Checking" with percent, can be cancelled before the writer opens (leaving the "Restore this branch" state), and the audit records the restore with snapshot date, batch range, orphaned count and the new epoch

**Given** the restore drill in CI (Hub with snapshot + 3 days of batches + a fork + an erased player)
**When** a fresh Hub restores it
**Then** the restored database's checksum over branch rows equals the source at the last uploaded `seq`, the erased player stays erased, and orphaned rows are absent

### Story 9.12: The Hub emits a yearly GST archive kept for 8 years

As a GST-registered café owner,
I want each financial year's invoices and tax summaries archived for 8 years,
So that I can answer a tax notice even after my plan's backup retention has passed.

**Covers:** FR174 (GST archive), AD-36, AD-26, AD-7, NFR24

**Acceptance Criteria:**

**Given** a paid-tier Hub and the first business-day close after the FY ends (31 March for India)
**When** the publisher runs
**Then** it writes `gst-archive/<fy>.zst` holding every invoice, credit note and per-rate tax summary stamped with that `fy`, plus a list of invoice numbers orphaned by a fork, and PUTs it via presign `kind: gst_archive` (new presign kind, same prefix and `FENCED` rules as snapshots)

**Given** the archive is uploaded
**When** the publisher records it
**Then** a system intent records `fy`, object key, sha256 and counts in the audit, and System → Backup shows "GST archive 26-27 saved"

**Given** an invoice or credit note created later with a past `fy` (late refund)
**When** the next close runs
**Then** the archive for that `fy` is regenerated and overwritten

**Given** a branch with `tax_mode` other than `gst_in`
**When** the FY closes
**Then** the same archive is produced with its generic tax lines, under the same key and retention

**Given** a Free branch
**When** the FY closes
**Then** nothing is uploaded, and the GST money rows stay in the Hub database for 8 years and are included in every export

### Story 9.13: Backups are kept for the plan's retention and GST records for 8 years

As a café owner,
I want backups kept for as long as my plan promises and no longer,
So that I can rely on old records while storage costs stay low.

**Covers:** FR174 (retention), NFR24, NFR20, AD-36

**Acceptance Criteria:**

**Given** the jobs role's daily retention sweep
**When** it runs for a branch
**Then** it deletes `changes/`, `snapshots/` and `rollups/` objects older than the licence `retention_days` (Basic 365, Premium 1,095) and never touches `gst-archive/`, `keys/` or `state/`

**Given** `gst-archive/<fy>.zst` objects
**When** the sweep runs
**Then** each is deleted only after 8 years from the end of its FY, regardless of tier or lapse

**Given** a branch that moves from Premium to Basic, or lapses to Free
**When** the sweep runs
**Then** objects are kept under the previous tier's retention for 30 days, then the new retention applies (lapsed branches keep the last paid tier's retention)

**Given** the Hub
**When** daily housekeeping runs
**Then** it prunes `changes` rows uploaded more than 90 days ago, never prunes rows not yet uploaded, and never prunes invoices, credit notes, payments or tax rows younger than 8 years; on Free (nothing uploaded) `changes` is not pruned

**Given** a sweep that would delete the newest snapshot needed to restore a retained batch range
**When** it plans deletions
**Then** it keeps that snapshot, so every retained batch has a base to restore from

### Story 9.14: Cloud backup status in the Hub header and System screen

As counter staff and managers on a paid tier,
I want to see at a glance when the branch last backed up,
So that we notice early if backups stop.

**Covers:** FR175 (Hub status), UX-DR60, UX-DR88, UX-DR23, UX-DR113

**Acceptance Criteria:**

**Given** a paid-tier Hub
**When** the header renders
**Then** it shows backup status in muted `hub-label` ("Backed up 4 min ago", plus "· 3 pending" when batches are waiting), and tapping it opens System → Backup

**Given** the last successful upload is older than twice `upload_interval_s`
**When** the header updates
**Then** the status changes to a warning `status-badge` "Backup behind" with text, and the change is announced in the polite live region; it returns to normal after the next upload

**Given** System → Backup
**When** it opens
**Then** it shows last batch upload, last summary, last snapshot, last key backup, last GST archive, pending batch count, "First backup pending" after an upgrade, and the last error (if any) in plain words, read from `upload_cursor`, `snapshot_log` and `backup_state`

**Given** a Free Hub
**When** the header renders
**Then** no backup status is shown, and System → Backup shows local exports only, with cloud backup as `tier-lock` "Available on Basic"

**Given** the phone layout (below 840px)
**When** the header renders
**Then** backup status is dropped from the header and stays reachable in System → Backup

### Story 9.15: Owner sees backup status and sends "Backup now"

As a café owner,
I want to see each branch's backup state and force a backup from my phone,
So that I can secure the day's data before a risky change or a power cut.

**Covers:** FR175 (owner "Backup now"), AD-5, UX-DR83, UX-DR81, UX-DR24

**Acceptance Criteria:**

**Given** the owner app Account → Branches on a paid tier
**When** it opens
**Then** each branch shows last upload, last snapshot and pending batches from the latest heartbeat with their data age

**Given** a paid branch
**When** the owner taps "Backup now"
**Then** the Epic 8 `backup.now` command is sent with a `command-row` (Queued on this device → Waiting for branch → Applied), and the Hub runs an upload and summary cycle at once, plus a key backup if one is pending

**Given** a "Backup now" applied less than 15 minutes ago
**When** another is sent
**Then** it is rejected `RATE_LIMITED` with "A backup ran at <time>. Try again after <time>."

**Given** the upload triggered by the command fails
**When** the Hub reports the result
**Then** the command is `rejected` with the transport error (as Epic 8 defines for `backup.now`), its row shows "Couldn't apply at branch" + Retry, and the branch's backup line shows "Backup failed" with the time

**Given** a Free branch
**When** the owner looks for "Backup now"
**Then** it shows `tier-lock` "Available on Basic"

### Story 9.16: Owner issues a standby code and a second PC pairs as standby

As a café owner on a paid tier,
I want to add a second PC as a standby Hub,
So that my branch can keep going if the main Hub PC fails.

**Covers:** FR169 (pairing), FR177 (standby codes), AD-10, AD-29, AD-42, AD-11, UX-DR24, UX-DR66

**Acceptance Criteria:**

**Given** Account → Branches on a branch whose licence has the `standby` flag
**When** the owner chooses "Add standby PC"
**Then** a single-use 8-character standby code is shown (same card and expiry as the Hub pairing code), replacing the Story 2.29 `tier-lock` placeholder; on Free it stays `tier-lock`, and a second standby is refused `STANDBY_EXISTS`

**Given** the Windows Hub installer on a second PC
**When** the standby code is entered
**Then** `POST /v1/pair` registers it as a Hub device with `role: standby`, its own `hub_instance_id`, device token and licence, and it never runs `branch.bootstrap`

**Given** the new standby on the LAN
**When** it finds the main Hub by mDNS and a Manager enters the 6-digit "Link standby" code shown at the main Hub (Settings → Standby, 10-minute expiry)
**Then** the main Hub mints a paired-device token with `kind = standby` over pinned TLS and stores the standby's certificate fingerprint

**Given** a standby has linked
**When** the main Hub next talks to each paired kiosk and Pi
**Then** it sends the standby's certificate fingerprint, and each device adds it to its pinned set, so it will accept the standby after promotion

**Given** an Android Hub, or a branch without the `standby` flag
**When** standby is attempted
**Then** pairing is refused `TIER_LOCKED` (or `PLATFORM_UNSUPPORTED` on Android) and the Hub shows `tier-lock`

**Given** the go-live checklist (Story 2.30) on a paid branch
**When** a standby is linked
**Then** "Standby PC" shows done

### Story 9.17: The standby bootstraps from the main Hub and follows it live

As a café owner,
I want the standby PC to hold an up-to-the-second copy of the branch,
So that promoting it loses nothing.

**Covers:** FR169 (follow), AD-4 (standby bootstrap), AD-8 (`SCHEMA_BEHIND`), AD-24 (keys replicated), AD-1, NFR9

**Acceptance Criteria:**

**Given** a linked standby with an empty database
**When** it starts following
**Then** it pulls a LAN snapshot from the main Hub (`GET /v1/replicate/snapshot`, consistent at a stated `seq`), loads it, then connects to `wss://<main>:7400/v1/replicate?from_seq=N` with its device token over pinned TLS

**Given** the replication stream
**When** the main Hub commits rows
**Then** the standby receives closed batches first, then live rows as they commit, applies them with `arena-replay` in one transaction per batch, and writes them verbatim into its own `changes` (same `seq`, same epoch), so after promotion it continues the sequence

**Given** `player_keys` changes on the main Hub (create, erase)
**When** they commit
**Then** they reach the standby on the same channel and an erased key is deleted on the standby within one replication message

**Given** the standby's schema version is below the main Hub's
**When** it connects
**Then** the main Hub refuses with `SCHEMA_BEHIND`, and the standby shows "Update this PC to follow the main Hub"

**Given** the stream drops
**When** the standby reconnects with jittered backoff (2–30 s)
**Then** it resumes from its last applied `seq` with no gap and no duplicate

**Given** the standby at any time
**When** any intent reaches it from a device or its own UI
**Then** it is rejected `NOT_PRIMARY` and nothing is written (read-only follower)

### Story 9.18: The standby keeps its own catalogue, media and licence and shows its status

As a café manager,
I want the standby PC to show clearly that it is the standby and how far behind it is,
So that I know it is ready before I ever need it.

**Covers:** FR169, AD-11 (reference cache), AD-17, AD-10, UX-DR88, UX-DR23

**Acceptance Criteria:**

**Given** the standby Hub app
**When** a staff member signs in (staff are replicated)
**Then** it shows a standby screen: "Standby — following <main Hub name>", lag in rows and seconds, last row received, schema version, licence state and "Promote to main"; the floor is visible read-only and every action is disabled

**Given** the standby
**When** it runs
**Then** it pulls the catalogue manifest itself and prefetches media from the main Hub over the LAN (cloud as fallback), and refreshes its own licence by heartbeat, so nothing in the reference cache comes through replication

**Given** lag above 0 rows for more than 30 s, or no stream for more than 60 s
**When** the standby screen updates
**Then** it shows a warning `status-badge` "Standby behind" with the cause, and the main Hub's System → Standby shows the same

**Given** the main Hub's System → Standby
**When** it opens
**Then** it shows the linked standby's name, lag, last contact and version, plus "Unlink standby" (manager PIN)

**Given** `GET /healthz` on either Hub
**When** it is called
**Then** it includes role, epoch and standby lag

### Story 9.19: Promote the standby to main with a manager PIN

As a counter manager during a busy night,
I want to make the standby the main Hub with one tap and my PIN,
So that the kiosks reconnect and nobody's game stops.

**Covers:** FR169 (promote < 10 s, kiosks reconnect), AD-11, AD-38, AD-1, AD-24, AD-30, NFR8, UX-DR66, UX-DR102

**Acceptance Criteria:**

**Given** the standby screen and a main Hub whose lease has expired (no renewal for 15 s)
**When** a Manager taps "Promote to main" and enters their PIN in place (`ops.promote`)
**Then** `lease.promote` checks schema ≥ the last seen main schema (`SCHEMA_BEHIND` otherwise), claims a new epoch from the cloud with `fork = {old epoch, last replicated seq}` (provisional if no reply within 3 s), re-applies every `player.erased` tombstone, opens the writer, and announces `epoch.changed` on the LAN and in its mDNS TXT record

**Given** promotion
**When** it is measured from PIN confirmation to the first accepted intent
**Then** it takes under 10 s on the reference PC

**Given** kiosks and Pis that lost the old Hub
**When** they see `epoch.changed` or re-resolve by mDNS with jittered backoff (2–30 s)
**Then** they connect to the new main using the pinned standby fingerprint (Story 9.16), present the highest epoch they have seen, reconcile running sessions by the Story 6.36 rules, and from then on refuse any Hub with a lower epoch

**Given** a Kitchen display
**When** it loses the old Hub
**Then** it tries the Hub address list it was given in its feed (main and standby), reconnects to the new main with its existing scoped token, and resumes by the Story 7.10 rules

**Given** promotion completes
**When** the new main runs
**Then** the audit records `lease.promote` with the manager, old and new epoch, and lag at promotion; the old main's tile on the System screen shows "Former main — offline"

**Given** the main Hub's lease is still fresh
**When** "Promote to main" is tapped
**Then** it fails `LEASE_FRESH` with "The main Hub is still running. Check it before promoting." and nothing changes

### Story 9.20: Force promote after a data-loss warning

As a counter manager whose main Hub is frozen but not dead,
I want to force the standby to take over after a clear warning,
So that the floor can keep running, with the owner told what happened.

**Covers:** FR170, AD-11, AD-30, AD-27 (owner alert), NFR8, UX-DR66, UX-DR103

**Acceptance Criteria:**

**Given** promotion fails with `LEASE_FRESH`, or the cloud claim or schema check cannot complete
**When** the panel shows the failure
**Then** it shows "Data since the last sync may be lost. Promote anyway?" with the lag ("12 changes not yet copied") and a "Force promote" action that opens `inline-pin`

**Given** a manager PIN and the data-loss acknowledgement
**When** force promote runs
**Then** `lease.promote{force: true}` takes the lease regardless of freshness and runs the Story 9.19 steps; on `SCHEMA_BEHIND` force is not offered

**Given** a force promote
**When** it completes
**Then** the audit records `force: true`, the manager, the lag and the reason, and the owner receives an alert "Standby forced to main at <branch> by <manager>" through "Owner alerts by web push and WhatsApp, and the Alerts tab" (raised at the Hub and sent when the WAN is next up)

**Given** the old main is still running and on the LAN
**When** the new main announces its epoch
**Then** the old main stops writing within one lease tick (it sees a higher epoch), so the two never accept intents at the same time for more than one renewal tick

### Story 9.21: The new main Hub lists what was set aside after a promote or restore

As a café owner,
I want every promote or restore to leave a clear record of which old changes were set aside,
So that my books and audit stay correct and nothing is silently lost.

**Covers:** AD-4 (fork declaration end to end), AD-11, AD-26 (orphaned invoices), AD-37, NFR10

**Acceptance Criteria:**

**Given** a Hub that opened a new epoch by promote or restore, with `fork_parent_epoch` and `fork_parent_seq` in `lease`
**When** an integration test lets it upload
**Then** its first batch (from "The Hub uploads gap-free change-log batches at its tier cadence") declares that fork with first `seq = parent_seq + 1`, and `branch_fence.fork_seq` equals `parent_seq`

**Given** the old main uploads late batches under its old epoch after the fence moved
**When** standby replay, cloud restore (Story 9.11) and "The audit builder rebuilds a branch month on demand" replay the branch
**Then** old-epoch rows above the fork are never applied to state, and the builder lists them as orphaned with orphaned invoice numbers shown separately

**Given** the new main Hub
**When** a Manager opens System → Backup → "Set aside after promote or restore"
**Then** it lists each fork (time, old and new epoch, manager, forced or not) and, from any `orphaned/` database on this PC, the count of set-aside changes and every orphaned invoice number with amount and time, for the owner's accountant
**And** "Export list" writes it as CSV to the backup location

**Given** an `orphaned/` database older than 90 days
**When** daily housekeeping runs
**Then** it is deleted after its list has been written once into the audit as a system intent (invoice numbers and amounts only, no PII)

**Given** orphaned invoice numbers
**When** later invoices are issued
**Then** their numbers are never reused, because the new epoch writes a new invoice segment (Story 5.14)

### Story 9.22: A returning old main Hub locks itself

As a café owner,
I want the old main Hub to lock itself if it comes back after a promote,
So that exactly one Hub ever serves my kiosks.

**Covers:** FR171 (self-lock, never serves kiosks), AD-11, AD-38, AD-1, UX-DR102, UX-DR103

**Acceptance Criteria:**

**Given** an old main Hub that restarts or reconnects
**When** it sees a higher epoch in mDNS TXT records, in a device `hello`, from the new main, or in a heartbeat response fence
**Then** it closes its writer, stops advertising itself as the Hub, rejects every device `hello` with `FENCED`, and shows a full-screen lock "This PC is no longer the main Hub" with "Make this the standby"

**Given** the LAN is partitioned so the old main cannot see the new one
**When** its next heartbeat returns a higher fence
**Then** it self-locks the same way

**Given** neither LAN nor cloud reveals a higher epoch (fully isolated)
**When** a device that has seen the higher epoch connects
**Then** the device refuses the lower-epoch Hub, and the Hub self-locks on that device's `hello`

**Given** the locked screen
**When** a staff member interacts
**Then** only "Make this the standby", the audit viewer and diagnostics are available; nothing else is reachable and the lock survives a restart

**Given** the old main had committed rows after the fork point
**When** it locks
**Then** it keeps them untouched for reconciliation and its late batches upload under the old epoch (Story 9.21)

### Story 9.23: Make the old main Hub the standby

As a café manager,
I want to turn the old main PC into the new standby with one action,
So that the branch is protected again without reinstalling anything.

**Covers:** FR171 ("Make this the standby"), AD-11, AD-4, AD-8, UX-DR66

**Acceptance Criteria:**

**Given** the self-locked old main Hub
**When** a Manager taps "Make this the standby" and enters their PIN in place
**Then** the Hub moves its database to `orphaned/<epoch>-<timestamp>.db` (kept 90 days), asks the cloud to re-role its Hub device as `standby`, and shows the Story 9.16 "Link standby" step for the new main's 6-digit code

**Given** the branch already has a standby linked
**When** "Make this the standby" is tried
**Then** it is refused `STANDBY_EXISTS` with "Unlink the current standby on the main Hub first."

**Given** the link succeeds
**When** the standby starts
**Then** it bootstraps by LAN snapshot and follows (Story 9.17), and paired devices receive its fingerprint again

**Given** the WAN is down
**When** "Make this the standby" is tried
**Then** the re-role waits and the screen shows "Waiting for internet to confirm"; the PC stays locked and serves nothing meanwhile

### Story 9.24: Failover and split-brain proven on the test LAN

As a café owner,
I want promote, force promote and the old-Hub lock tested before every release,
So that a Hub failure on a busy night is a non-event.

**Covers:** NFR8, NFR9, NFR10, FR169, FR170, FR171, AD-5 (fenced results re-queued), AD-11, AD-38

**Acceptance Criteria:**

**Given** the failover harness (cloud binary + MinIO + main Hub + standby + 5 fake kiosks + a kitchen display) with sessions running
**When** the main Hub process is killed and the standby is promoted
**Then** the first intent is accepted under 10 s after PIN confirmation, every kiosk reconnects and its timer resumes from the Hub's record, and no committed row replicated before the kill is missing

**Given** the main Hub is frozen with its lease fresh
**When** force promote runs
**Then** it succeeds, the owner alert is queued, and the audit row has `force: true`

**Given** the old main comes back on the LAN, and in a second run only via WAN with the LAN partitioned
**When** it sees the higher epoch
**Then** it self-locks, serves no device, and its late batches are stored under the old epoch and never replayed into state

**Given** an owner command fetched and applied by the old main at a `seq` above the fork
**When** its result is posted with the old epoch
**Then** the relay rejects it `FENCED`, the command returns to `pending`, and the new main applies it exactly once

**Given** a promote with the cloud unreachable
**When** the WAN returns
**Then** the provisional epoch is confirmed by the first heartbeat, or (when a higher fence exists) the Hub self-locks

**Given** the release pipeline
**When** a release candidate is built
**Then** this suite runs before release and blocks it on any failure

### Story 9.25: Restore drill and privacy proof

As a café owner,
I want backups and erasure proven end to end before every release,
So that a restore works on the day I need it and an erased player stays erased everywhere.

**Covers:** FR200, FR173, FR174, FR199, NFR9, NFR13, AD-24, AD-36

**Acceptance Criteria:**

**Given** a seeded branch with 30 days of activity, one fork from a promote and two erased players
**When** the drill restores it (a) from a local export on a new PC with the released KEK and (b) from cloud backup
**Then** both restores reach a new epoch, branch-row checksums equal the source at the restore point, and both erased players have no key and no readable PII

**Given** a player erased after the export used in (a) on a paid branch
**When** restore (a) runs with the cloud reachable
**Then** that player is erased in the restored database before the writer opens

**Given** a promote after an erasure on the main Hub
**When** the standby becomes main
**Then** the erased player has no key on the new main

**Given** every batch, snapshot, key backup, GST archive, audit cache and export file produced by the drill
**When** the privacy scanner searches them for the seeded names, phones and emails
**Then** none appears in plaintext, and the Hub and cloud logs carry only player ids

**Given** the 1 GB reference database
**When** the export benchmark runs
**Then** export completes in under 10 s and the result is recorded in the release report

**Given** the release pipeline
**When** a release candidate is built
**Then** the drill runs before release and blocks it on any failure
