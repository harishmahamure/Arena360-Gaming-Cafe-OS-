//! `arena-replay`: rebuilds branch rows from the `changes` log.
//!
//! [`replay`] applies each change onto a freshly migrated, empty database, in `(epoch, seq)`
//! order with epochs compared as `(counter, hub_instance_id)`. An insert or update upserts the
//! change's MessagePack row into its table; a delete removes the row with that key. Only the
//! change-log tables ([`Table::ALL`]: `branch`, `settings`, `devices`) may be written. Replaying a
//! database's whole log must reproduce its rows exactly.

use arena_proto::{AppError, ErrorCode};
use arena_store::{
    ChangeOp, ChangeRecord, Row, RowKey, StoreTx, Table, delete_row, has_branch_data, upsert_row,
};

fn bad(message: String) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

/// Parses `counter:hub_instance_id` into its ordering key.
fn epoch_key(epoch: &str) -> Result<(i64, &str), AppError> {
    epoch
        .split_once(':')
        .and_then(|(c, id)| Some((c.parse::<i64>().ok()?, id)))
        .ok_or_else(|| bad(format!("malformed epoch {epoch:?}")))
}

/// Replays `changes` into `dest`. Fails `INTERNAL`, writing nothing the caller commits, on a
/// destination that already has branch, settings or change rows; a table outside the
/// allow-list; a malformed epoch or row; a row whose key does not match its `row_id`; a delete
/// of a missing row; or a `seq` gap within an epoch. Returns the number of changes applied.
pub fn replay(
    changes: impl IntoIterator<Item = ChangeRecord>,
    dest: &mut StoreTx<'_>,
) -> Result<usize, AppError> {
    if has_branch_data(dest)? {
        return Err(bad("replay destination is not empty".to_owned()));
    }
    let mut changes: Vec<ChangeRecord> = changes.into_iter().collect();
    for c in &changes {
        epoch_key(&c.epoch)?;
    }
    changes.sort_by(|a, b| {
        let ka = epoch_key(&a.epoch).expect("checked");
        let kb = epoch_key(&b.epoch).expect("checked");
        ka.cmp(&kb).then(a.seq.cmp(&b.seq))
    });

    let mut prev: Option<(&str, i64)> = None;
    for c in &changes {
        let expected = match prev {
            Some((epoch, seq)) if epoch == c.epoch => seq + 1,
            _ => 1,
        };
        if c.seq != expected {
            return Err(bad(format!(
                "seq gap in epoch {}: expected {expected}, found {}",
                c.epoch, c.seq
            )));
        }
        prev = Some((&c.epoch, c.seq));

        let table = Table::from_name(&c.tbl)
            .ok_or_else(|| bad(format!("table {:?} is not replayable", c.tbl)))?;
        let row: Row = rmp_serde::from_slice(&c.row)
            .map_err(|e| bad(format!("change {}:{} has a bad row: {e}", c.epoch, c.seq)))?;
        match RowKey::from_row(table, &row) {
            Some(key) if key.row_id() == c.row_id => {}
            _ => {
                return Err(bad(format!(
                    "change {}:{} row does not match row_id {:?}",
                    c.epoch, c.seq, c.row_id
                )));
            }
        }
        match c.op {
            ChangeOp::Insert | ChangeOp::Update => upsert_row(dest, table, &row)?,
            ChangeOp::Delete => {
                if delete_row(dest, table, &row)? == 0 {
                    return Err(bad(format!(
                        "change {}:{} deletes missing row {:?}",
                        c.epoch, c.seq, c.row_id
                    )));
                }
            }
        }
    }
    Ok(changes.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use arena_core::{HubStatus, LeaseInfo, ManualClock, Role, Writer, status_channel};
    use arena_proto::{
        Actor, ActorKind, BranchBootstrap, Intent, IntentKind, IntentPayload, SettingsSet,
    };
    use arena_store::{LeaseRow, RowValue, Store, write_lease};

    const T0: i64 = 1_800_000_000_000;

    /// A database seeded through the Writer with `branch.bootstrap`, as `dev seed` does.
    async fn seeded() -> (tempfile::TempDir, Store) {
        seed(false).await
    }

    /// Like [`seeded`], plus one `device.register` so every change-log table has a row.
    async fn seeded_with_device() -> (tempfile::TempDir, Store) {
        seed(true).await
    }

    async fn seed(with_device: bool) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, T0).unwrap();
        let lease = LeaseRow {
            counter: 1,
            hub_instance_id: "01JHUB".into(),
            provisional: true,
            renewed_at: T0,
            expires_at: T0 + 15_000,
        };
        store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
        let mut status = HubStatus::opened(store.schema_version().unwrap(), "01JHUB");
        status.role = Role::Primary;
        status.lease = Some(LeaseInfo::from(&lease));
        let (_tx, rx) = status_channel(status);
        let writer = Writer::spawn(store, Arc::new(ManualClock::new(T0)), rx).unwrap();
        writer
            .submit_intent(Intent {
                intent_id: ulid::Ulid::generate().to_string(),
                kind: IntentKind::BranchBootstrap,
                idempotency_key: ulid::Ulid::generate().to_string(),
                issued_at: T0,
                base_rev: None,
                actor: Actor {
                    kind: ActorKind::System,
                    id: "dev-seed".into(),
                },
                elevated_by: None,
                reason: None,
                payload: IntentPayload::BranchBootstrap(BranchBootstrap {
                    name: "Dev Branch".into(),
                    branch_code: "DEV01".into(),
                    currency: "INR".into(),
                    tz: "Asia/Kolkata".into(),
                    day_start_min: 360,
                    fy_start_month: 4,
                }),
            })
            .await
            .unwrap();
        if with_device {
            let owner = Actor {
                kind: ActorKind::Owner,
                id: "dev-seed".into(),
            };
            let submit = |payload: IntentPayload, base_rev: Option<i64>| Intent {
                intent_id: ulid::Ulid::generate().to_string(),
                kind: payload.kind(),
                idempotency_key: ulid::Ulid::generate().to_string(),
                issued_at: T0,
                base_rev,
                actor: owner.clone(),
                elevated_by: None,
                reason: None,
                payload,
            };
            writer
                .submit_intent(submit(
                    IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                        class: arena_proto::ClientClass::Kds,
                        label: "Pass".into(),
                        token_hash: arena_core::devices::hash_token_secret("s").unwrap(),
                    }),
                    None,
                ))
                .await
                .unwrap();
            // A custom role, then an edit that replaces its rows (delete + insert changes),
            // and a staff member holding it: every change-log table has a row.
            let perm = |p: &str, effect| arena_proto::RolePerm {
                permission: p.into(),
                effect,
            };
            let role = writer
                .submit_intent(submit(
                    IntentPayload::RoleUpsert(arena_proto::RoleUpsert {
                        role_id: None,
                        name: "Floor lead".into(),
                        perms: vec![
                            perm("pos.sell", arena_proto::PermEffect::Allow),
                            perm("pos.void", arena_proto::PermEffect::Deny),
                        ],
                    }),
                    None,
                ))
                .await
                .unwrap();
            let role_id = role.changes[0].row_id.clone();
            writer
                .submit_intent(submit(
                    IntentPayload::RoleUpsert(arena_proto::RoleUpsert {
                        role_id: Some(role_id.clone()),
                        name: "Floor lead".into(),
                        perms: vec![perm("pos.void", arena_proto::PermEffect::Allow)],
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                        staff_id: None,
                        login: "lead".into(),
                        display_name: "Lead".into(),
                        role_id,
                        password_hash: Some(arena_core::staff::hash_secret("pw").unwrap()),
                        pin_hash: None,
                        active: true,
                    }),
                    None,
                ))
                .await
                .unwrap();
        }
        writer.close().await;
        let store = Store::open(&path, T0).unwrap();
        (dir, store)
    }

    fn empty() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("replica.db"), T0).unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn replaying_the_seeded_log_reproduces_branch_and_settings() {
        let (_a, source) = seeded_with_device().await;
        let (_b, mut dest) = empty();
        let changes = source.changes().unwrap();
        assert!(!changes.is_empty());
        // Order of the input does not matter.
        let mut shuffled = changes.clone();
        shuffled.reverse();
        let n = dest.write_tx_mut(|tx| replay(shuffled, tx)).unwrap();
        assert_eq!(n, changes.len());
        for table in Table::ALL {
            let want = source.rows(table).unwrap();
            assert!(!want.is_empty());
            assert_eq!(dest.rows(table).unwrap(), want, "{}", table.name());
        }
        assert!(
            dest.changes().unwrap().is_empty(),
            "replay writes rows, not changes"
        );
    }

    #[tokio::test]
    async fn update_and_delete_changes_apply_in_order() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        let last = changes
            .iter()
            .rfind(|c| c.tbl == "settings")
            .unwrap()
            .clone();
        let mut row: Row = rmp_serde::from_slice(&last.row).unwrap();
        row.insert("value".into(), RowValue::Text("edited".into()));
        let edited = rmp_serde::to_vec_named(&row).unwrap();
        changes.push(ChangeRecord {
            seq: changes.last().unwrap().seq + 1,
            op: ChangeOp::Update,
            row: edited.clone(),
            ..last.clone()
        });
        let (_b, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(changes.clone(), tx)).unwrap();
        assert!(dest.rows(Table::Settings).unwrap().contains(&row));

        changes.push(ChangeRecord {
            seq: changes.last().unwrap().seq + 1,
            op: ChangeOp::Delete,
            row: edited,
            ..last
        });
        let (_c, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(changes, tx)).unwrap();
        assert!(!dest.rows(Table::Settings).unwrap().contains(&row));
        assert_eq!(
            dest.rows(Table::Settings).unwrap().len(),
            source.rows(Table::Settings).unwrap().len() - 1
        );
    }

    #[tokio::test]
    async fn tables_outside_the_allow_list_are_refused() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        changes[0].tbl = "lease".into();
        let (_b, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(err.message.contains("not replayable"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_seq_gap_is_refused() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        changes.remove(1);
        let (_b, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert!(err.message.contains("seq gap"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_non_empty_destination_is_refused() {
        let (_a, source) = seeded().await;
        let changes = source.changes().unwrap();
        let (_b, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(changes.clone(), tx)).unwrap();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert!(err.message.contains("not empty"), "{err}");
    }

    #[tokio::test]
    async fn a_row_id_mismatch_is_refused() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        changes[1].row_id = "someone/else".into();
        let (_b, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert!(err.message.contains("does not match row_id"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_a_missing_row_is_refused() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        let last = changes
            .iter()
            .rfind(|c| c.tbl == "settings")
            .unwrap()
            .clone();
        let end = changes.last().unwrap().seq;
        for seq in [end + 1, end + 2] {
            changes.push(ChangeRecord {
                seq,
                op: ChangeOp::Delete,
                ..last.clone()
            });
        }
        let (_b, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert!(err.message.contains("deletes missing row"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    fn settings_set(key: &str, value: &str, base_rev: i64) -> Intent {
        Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::SettingsSet,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: T0,
            base_rev: Some(base_rev),
            actor: Actor {
                kind: ActorKind::Owner,
                id: "test".into(),
            },
            elevated_by: None,
            reason: None,
            payload: IntentPayload::SettingsSet(SettingsSet {
                key: key.into(),
                value: value.into(),
            }),
        }
    }

    /// Bootstraps and edits under epoch `9:Z`, then hands the lease to `10:A` and edits the same
    /// setting again. `10:A` sorts before `9:Z` as text, so only numeric ordering gets this right.
    #[tokio::test]
    async fn multi_epoch_replay_orders_numerically_and_resets_seq() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, T0).unwrap();
        let lease = |counter, id: &str| LeaseRow {
            counter,
            hub_instance_id: id.into(),
            provisional: false,
            renewed_at: T0,
            expires_at: T0 + 15_000,
        };
        store
            .write_tx(|tx| write_lease(tx, &lease(9, "Z")))
            .unwrap();
        let mut status = HubStatus::opened(store.schema_version().unwrap(), "Z");
        status.role = Role::Primary;
        status.lease = Some(LeaseInfo::from(&lease(9, "Z")));
        let (status_tx, rx) = status_channel(status);
        let writer = Writer::spawn(store, Arc::new(ManualClock::new(T0)), rx).unwrap();
        let mut boot = settings_set("x", "x", 0);
        boot.kind = IntentKind::BranchBootstrap;
        boot.base_rev = None;
        boot.actor.kind = ActorKind::System;
        boot.payload = IntentPayload::BranchBootstrap(BranchBootstrap {
            name: "Dev Branch".into(),
            branch_code: "DEV01".into(),
            currency: "INR".into(),
            tz: "Asia/Kolkata".into(),
            day_start_min: 360,
            fy_start_month: 4,
        });
        writer.submit_intent(boot).await.unwrap();
        writer
            .submit_intent(settings_set("hub.locale", "en-NZ", 1))
            .await
            .unwrap();
        Store::open(&path, T0)
            .unwrap()
            .write_tx(|tx| write_lease(tx, &lease(10, "A")))
            .unwrap();
        status_tx.send_modify(|s| s.lease = Some(LeaseInfo::from(&lease(10, "A"))));
        let applied = writer
            .submit_intent(settings_set("hub.locale", "en-TT", 2))
            .await
            .unwrap();
        assert_eq!(
            (applied.changes[0].epoch.as_str(), applied.changes[0].seq),
            ("10:A", 1),
            "seq restarts at 1 in a new epoch"
        );
        writer.close().await;

        let source = Store::open(&path, T0).unwrap();
        let changes = source.changes().unwrap();
        let epochs: Vec<&str> = changes.iter().map(|c| c.epoch.as_str()).collect();
        assert_eq!(epochs.last(), Some(&"10:A"));
        assert!(epochs[..epochs.len() - 1].iter().all(|e| *e == "9:Z"));

        // Fed in text order (10:A first), replay still applies 9:Z first: the later epoch wins.
        let mut text_order = changes.clone();
        text_order.sort_by(|a, b| (&a.epoch, a.seq).cmp(&(&b.epoch, b.seq)));
        assert_eq!(text_order[0].epoch, "10:A");
        let (_b, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(text_order, tx)).unwrap();
        for table in Table::ALL {
            assert_eq!(dest.rows(table).unwrap(), source.rows(table).unwrap());
        }
        let locale = dest
            .rows(Table::Settings)
            .unwrap()
            .into_iter()
            .find(|r| r["key"] == RowValue::Text("hub.locale".into()))
            .unwrap();
        assert_eq!(locale["value"], RowValue::Text("en-TT".into()));
        assert_eq!(locale["rev"], RowValue::Integer(3));

        // An epoch whose first change is seq 2 is refused.
        let mut gapped = changes;
        gapped.last_mut().unwrap().seq = 2;
        let (_c, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(gapped, tx)).unwrap_err();
        assert!(err.message.contains("seq gap in epoch 10:A"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    #[test]
    fn epochs_order_numerically_by_counter() {
        assert!(epoch_key("10:A").unwrap() > epoch_key("9:Z").unwrap());
        assert!(epoch_key("1:B").unwrap() > epoch_key("1:A").unwrap());
        assert!(epoch_key("x:A").is_err());
        assert!(epoch_key("1").is_err());
    }
}
