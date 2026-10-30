//! `arena-replay`: rebuilds branch rows from the `changes` log.
//!
//! [`replay`] applies each change onto a freshly migrated, empty database, in `(epoch, seq)`
//! order with epochs compared as `(counter, hub_instance_id)`. An insert or update upserts the
//! change's MessagePack row into its table; a delete removes the row with that key. Only the
//! change-log tables ([`Table::ALL`]: `branch`, `settings`, `devices`) may be written. Replaying a
//! database's whole log must reproduce its rows exactly.
//!
//! [`replay_history`] is the same replay for the audit builder (Story 8.36): it honours fork points,
//! can continue after a snapshot, and [`MonthBuilder`] drives it batch by batch into a file that
//! becomes `audit-cache/<yyyy-mm>.db.zst`. [`state_checksum`] fingerprints the rows so a rebuilt
//! database can be compared with the Hub's.

use arena_proto::{AppError, ErrorCode};
use arena_store::{
    ChangeOp, ChangeRecord, Row, RowKey, Store, StoreTx, Table, delete_row, has_branch_data,
    upsert_row,
};
use sha2::{Digest, Sha256};

mod audit;
mod rebuild;

pub use audit::{BuiltMonth, MonthBuilder};
pub use rebuild::{RebuildSource, RebuildStage, RebuildStop, Rebuilt, rebuild};

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

/// Where an epoch was forked from: rows of `parent_epoch` above `parent_seq` were written by a Hub
/// that was replaced, and are never replayed into state (AD-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochFork {
    pub epoch: String,
    pub parent_epoch: String,
    pub parent_seq: i64,
}

/// The last change a destination already holds (a snapshot's position): replay continues after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resume {
    pub epoch: String,
    pub seq: i64,
}

impl Resume {
    /// Whether `c` is at or before this position in log order.
    pub fn covers(&self, c: &ChangeRecord) -> Result<bool, AppError> {
        let at = (epoch_key(&self.epoch)?, self.seq);
        Ok((epoch_key(&c.epoch)?, c.seq) <= at)
    }
}

/// What [`replay_history`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Replayed {
    /// The changes applied to state, in log order.
    pub applied: Vec<ChangeRecord>,
    /// Rows above a fork point, kept out of state, in log order.
    pub orphaned: Vec<ChangeRecord>,
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
    Ok(apply(changes, &[], None, dest)?.applied.len())
}

/// Replays `changes` onto `dest` honouring `forks` (Story 8.36, the audit builder). With `resume`
/// the destination already holds the log up to that position (a snapshot, or the previous
/// batch): changes at or before it are skipped. With none, the destination must be empty. Rows
/// of a forked epoch's parent above its fork point come back in [`Replayed::orphaned`] and are
/// not applied; a forked epoch starts at `parent_seq + 1`. Fails as [`replay`] does, and when
/// `resume` is itself above a fork point of its epoch.
pub fn replay_history(
    changes: impl IntoIterator<Item = ChangeRecord>,
    forks: &[EpochFork],
    resume: Option<&Resume>,
    dest: &mut StoreTx<'_>,
) -> Result<Replayed, AppError> {
    if resume.is_none() && has_branch_data(dest)? {
        return Err(bad("replay destination is not empty".to_owned()));
    }
    apply(changes, forks, resume, dest)
}

fn apply(
    changes: impl IntoIterator<Item = ChangeRecord>,
    forks: &[EpochFork],
    resume: Option<&Resume>,
    dest: &mut StoreTx<'_>,
) -> Result<Replayed, AppError> {
    if let Some(r) = resume {
        epoch_key(&r.epoch)?;
        if let Some(f) = forks
            .iter()
            .find(|f| f.parent_epoch == r.epoch && f.parent_seq < r.seq)
        {
            return Err(bad(format!(
                "the starting point {}:{} is above the fork point {} of epoch {}",
                r.epoch, r.seq, f.parent_seq, f.epoch
            )));
        }
    }
    let mut all: Vec<ChangeRecord> = changes.into_iter().collect();
    for c in &all {
        epoch_key(&c.epoch)?;
    }
    all.sort_by(|a, b| {
        let ka = epoch_key(&a.epoch).expect("checked");
        let kb = epoch_key(&b.epoch).expect("checked");
        ka.cmp(&kb).then(a.seq.cmp(&b.seq))
    });

    let mut out = Replayed::default();
    for c in all {
        if let Some(r) = resume
            && r.covers(&c)?
        {
            continue;
        }
        let orphan = forks
            .iter()
            .any(|f| f.parent_epoch == c.epoch && c.seq > f.parent_seq);
        if orphan {
            out.orphaned.push(c);
        } else {
            out.applied.push(c);
        }
    }

    let mut prev: Option<(&str, i64)> = None;
    for c in &out.applied {
        let expected = match prev {
            Some((epoch, seq)) if epoch == c.epoch => seq + 1,
            _ => match resume {
                Some(r) if r.epoch == c.epoch => r.seq + 1,
                _ => forks
                    .iter()
                    .find(|f| f.epoch == c.epoch)
                    .map_or(1, |f| f.parent_seq + 1),
            },
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
    Ok(out)
}

/// A SHA-256 over every row of every change-log table, in primary-key order: two databases with
/// the same checksum hold the same branch rows. The audit builder's round-trip test compares the
/// Hub's against the rebuilt database's.
pub fn state_checksum(store: &Store) -> Result<String, AppError> {
    let mut hash = Sha256::new();
    for table in Table::ALL {
        hash.update(table.name().as_bytes());
        for row in store.rows(table)? {
            let bytes = rmp_serde::to_vec_named(&row)
                .map_err(|e| bad(format!("checksum {}: {e}", table.name())))?;
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(&bytes);
        }
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
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

    pub(crate) const T0: i64 = 1_800_000_000_000;

    /// A database seeded through the Writer with `branch.bootstrap`, as `dev seed` does.
    async fn seeded() -> (tempfile::TempDir, Store) {
        seed(false).await
    }

    /// Like [`seeded`], plus one `device.register` so every change-log table has a row.
    pub(crate) async fn seeded_with_device() -> (tempfile::TempDir, Store) {
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
            expires_at: i64::MAX,
        };
        store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
        let mut status = HubStatus::opened(store.schema_version().unwrap(), "01JHUB");
        status.role = Role::Primary;
        status.lease = Some(LeaseInfo::from(&lease));
        let (_tx, rx) = status_channel(status);
        let clock = Arc::new(ManualClock::new(T0));
        let writer = Writer::spawn(store, clock.clone(), rx).unwrap();
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
                    onboarding: None,
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
            // The cash settles below need an open shift.
            writer
                .submit_intent(submit(
                    IntentPayload::ShiftOpen(arena_proto::ShiftOpen { float_minor: 1_000 }),
                    None,
                ))
                .await
                .unwrap();
            let device = writer
                .submit_intent(submit(
                    IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                        device_id: None,
                        device_kind: Default::default(),
                        class: arena_proto::ClientClass::Kds,
                        label: "Pass".into(),
                        token_hash: arena_core::devices::hash_token_secret("s").unwrap(),
                        tv_number: None,
                        group_id: None,
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
                        setup_code_hash: None,
                        setup_expires_at: None,
                    }),
                    None,
                ))
                .await
                .unwrap();
            // A read elevation the Hub recorded (system actor).
            let mut grant = submit(
                IntentPayload::AuditReadGranted(arena_proto::AuditReadGranted {
                    permission: arena_proto::Permission::AuditView,
                    requester_staff_id: "01JCOUNTER".into(),
                    approver_staff_id: "01JMANAGER".into(),
                    verifier_version: 1,
                }),
                None,
            );
            grant.actor = Actor {
                kind: ActorKind::System,
                id: "hub".into(),
            };
            writer.submit_intent(grant).await.unwrap();
            // An event the device logged (device actor).
            let mut log = submit(
                IntentPayload::KioskLog(arena_proto::KioskLog {
                    level: arena_proto::KioskLogLevel::Info,
                    code: "app_started".into(),
                    detail: String::new(),
                }),
                None,
            );
            log.actor = Actor {
                kind: ActorKind::Device,
                id: device.changes[0].row_id.clone(),
            };
            writer.submit_intent(log).await.unwrap();
            // A remote unlock of a paired kiosk and its re-lock (insert + update).
            let kiosk = writer
                .submit_intent(submit(
                    IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                        device_id: None,
                        device_kind: Default::default(),
                        class: arena_proto::ClientClass::Paired,
                        label: "Kiosk".into(),
                        token_hash: arena_core::devices::hash_token_secret("k").unwrap(),
                        tv_number: None,
                        group_id: None,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            let group = writer
                .submit_intent(submit(
                    IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                        id: None,
                        name: "PCs".into(),
                        group_kind: "pc".into(),
                        colour: "teal".into(),
                        sort_order: 1,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                        device_id: kiosk.clone(),
                        group_id: Some(group.clone()),
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::RateUpsert(arena_proto::RateUpsert {
                        group_id: group.clone(),
                        price_per_hour_minor: 12_000,
                    }),
                    None,
                ))
                .await
                .unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::RuleUpsert(arena_proto::RuleUpsert {
                        id: None,
                        name: "Happy hour".into(),
                        group_id: group.clone(),
                        rule_kind: "weekly".into(),
                        days_mask: Some(31),
                        start_min: Some(960),
                        end_min: Some(1080),
                        date_from: None,
                        date_to: None,
                        adjust_kind: "percent_bp".into(),
                        adjust_value: -2000,
                        priority: 0,
                        active: true,
                    }),
                    None,
                ))
                .await
                .unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::CouponUpsert(arena_proto::CouponUpsert {
                        id: None,
                        code: "save10".into(),
                        percent_bp: Some(1_000),
                        amount_minor: None,
                        valid_from: None,
                        valid_to: None,
                        usage_limit: Some(5),
                        min_bill_minor: 0,
                        targets: vec![arena_proto::CouponTarget {
                            kind: "group".into(),
                            id: group.clone(),
                        }],
                        active: true,
                    }),
                    None,
                ))
                .await
                .unwrap();
            let pack_plan = writer
                .submit_intent(submit(
                    IntentPayload::PlanUpsert(arena_proto::PlanUpsert {
                        id: None,
                        plan_kind: "pack".into(),
                        name: "5 h".into(),
                        price_minor: 30_000,
                        minutes: Some(300),
                        validity_days: Some(7),
                        window_start_min: None,
                        window_end_min: None,
                        group_ids: vec![group.clone()],
                        active: true,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes
                .iter()
                .find(|c| c.tbl == "plans")
                .unwrap()
                .row_id
                .clone();
            let zone = writer
                .submit_intent(submit(
                    IntentPayload::ZoneUpsert(arena_proto::ZoneUpsert {
                        id: None,
                        name: "PC hall".into(),
                        sort_order: 1,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::DevicePlace(arena_proto::DevicePlace {
                        device_id: kiosk.clone(),
                        zone_id: Some(zone),
                        floor_x: Some(1),
                        floor_y: Some(2),
                    }),
                    None,
                ))
                .await
                .unwrap();
            let exit = writer
                .submit_intent(submit(
                    IntentPayload::DeviceAdminUnlock(arena_proto::DeviceAdminUnlock {
                        device_id: kiosk.clone(),
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            let mut relock = submit(
                IntentPayload::DeviceAdminRelock(arena_proto::DeviceAdminRelock { exit_id: exit }),
                None,
            );
            relock.actor = Actor {
                kind: ActorKind::Device,
                id: kiosk.clone(),
            };
            writer.submit_intent(relock).await.unwrap();
            let cola = writer
                .submit_intent(submit(
                    IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                        id: None,
                        name: "Cola".into(),
                        category: "Drinks".into(),
                        price_minor: 2000,
                        tax_rate_bp: None,
                        hsn: String::new(),
                        track_stock: true,
                        image: String::new(),
                        barcode: "123".into(),
                        kitchen: false,
                        favourite: false,
                        active: true,
                        bundle: false,
                        components: Vec::new(),
                        kitchen_notes: None,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            let combo = writer
                .submit_intent(submit(
                    IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                        id: None,
                        name: "Combo".into(),
                        category: "Drinks".into(),
                        price_minor: 3000,
                        tax_rate_bp: None,
                        hsn: String::new(),
                        track_stock: false,
                        image: String::new(),
                        barcode: String::new(),
                        kitchen: false,
                        favourite: false,
                        active: true,
                        bundle: true,
                        components: vec![arena_proto::BundleComponent {
                            product_id: cola.clone(),
                            qty: 2,
                        }],
                        kitchen_notes: None,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                        id: Some(combo),
                        name: "Combo".into(),
                        category: "Drinks".into(),
                        price_minor: 3000,
                        tax_rate_bp: None,
                        hsn: String::new(),
                        track_stock: false,
                        image: String::new(),
                        barcode: String::new(),
                        kitchen: false,
                        favourite: false,
                        active: true,
                        bundle: true,
                        components: vec![arena_proto::BundleComponent {
                            product_id: cola.clone(),
                            qty: 3,
                        }],
                        kitchen_notes: None,
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            let stock_in = |qty: i64, cost: i64| {
                submit(
                    IntentPayload::StockIn(arena_proto::StockIn {
                        product_id: cola.clone(),
                        qty,
                        unit_cost_minor: cost,
                        supplier: "Acme".into(),
                        expiry: None,
                    }),
                    None,
                )
            };
            writer.submit_intent(stock_in(10, 2000)).await.unwrap();
            writer.submit_intent(stock_in(30, 2400)).await.unwrap();
            let last = writer.submit_intent(stock_in(5, 100)).await.unwrap();
            let move_id = last
                .changes
                .iter()
                .find(|c| c.tbl == "stock_moves")
                .unwrap()
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::StockInReverted(arena_proto::StockInReverted { move_id }),
                    None,
                ))
                .await
                .unwrap();
            writer
                .submit_intent(arena_proto::Intent {
                    reason: Some(arena_proto::ReasonCode::Spoiled),
                    ..submit(
                        IntentPayload::StockWaste(arena_proto::StockWaste {
                            product_id: cola.clone(),
                            qty: 3,
                        }),
                        None,
                    )
                })
                .await
                .unwrap();
            // 37 left; a threshold of 38 raises "Low stock", the stock-in below lifts it and the
            // write-off crosses it again.
            writer
                .submit_intent(submit(
                    IntentPayload::StockThreshold(arena_proto::StockThreshold {
                        product_id: cola.clone(),
                        low_stock_qty: Some(38),
                    }),
                    None,
                ))
                .await
                .unwrap();
            let player_id = ulid::Ulid::generate().to_string();
            let player = arena_core::players::seal(
                &[5; 32],
                &arena_proto::PlayerForm {
                    username: "asha".into(),
                    password: "secret1".into(),
                    phone: "+919876543210".into(),
                    name: "Asha".into(),
                    email: String::new(),
                    dob: String::new(),
                    notes: String::new(),
                    tags: vec!["vip".into()],
                    consent: true,
                    policy_version: "1".into(),
                },
                &player_id,
                arena_core::staff::hash_secret("pw").unwrap(),
            )
            .unwrap();
            let wrapped = player.wrapped_key.clone();
            writer
                .submit_intent(submit(IntentPayload::PlayerCreate(player), None))
                .await
                .unwrap();
            let edit = arena_core::players::reseal(
                &[5; 32],
                &arena_proto::PlayerEditForm {
                    player_id: player_id.clone(),
                    base_rev: 1,
                    phone: "+919000000000".into(),
                    name: "Asha Rao".into(),
                    email: String::new(),
                    dob: String::new(),
                    notes: String::new(),
                    tags: Vec::new(),
                    consent: false,
                    policy_version: "1".into(),
                },
                &wrapped,
            )
            .unwrap();
            writer
                .submit_intent(submit(IntentPayload::PlayerUpdate(edit), Some(1)))
                .await
                .unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::PlayerPasswordReset(arena_proto::PlayerPasswordReset {
                        player_id: player_id.clone(),
                        password_hash: arena_core::staff::hash_secret("pw2").unwrap(),
                    }),
                    None,
                ))
                .await
                .unwrap();
            let started = writer
                .submit_intent(submit(
                    IntentPayload::SessionStart(arena_proto::SessionStart {
                        group_with: None,
                        pay_from_wallet: false,
                        device_id: kiosk.clone(),
                        mode: arena_proto::SessionMode::Postpaid,
                        player_id: Some(player_id.clone()),
                        pack_id: None,
                        plan_id: None,
                    }),
                    None,
                ))
                .await
                .unwrap();
            let session_id = started.changes[0].row_id.clone();
            let pause = |rev: i64| {
                submit(
                    IntentPayload::SessionPause(arena_proto::SessionPause {
                        session_id: session_id.clone(),
                        reason: arena_proto::PauseReason::Other,
                        note: Some("Fan noise".into()),
                    }),
                    Some(rev),
                )
            };
            writer.submit_intent(pause(1)).await.unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::SessionResume(arena_proto::SessionResume {
                        session_id: session_id.clone(),
                    }),
                    Some(2),
                ))
                .await
                .unwrap();
            writer.submit_intent(pause(3)).await.unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::SessionEnd(arena_proto::SessionEnd {
                        session_id: session_id.clone(),
                    }),
                    Some(4),
                ))
                .await
                .unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::SessionEndReverted(arena_proto::SessionEndReverted {
                        session_id: session_id.clone(),
                    }),
                    Some(5),
                ))
                .await
                .unwrap();
            let ended = writer
                .submit_intent(submit(
                    IntentPayload::SessionEnd(arena_proto::SessionEnd {
                        session_id: session_id.clone(),
                    }),
                    Some(6),
                ))
                .await
                .unwrap();
            let order_id = ended
                .changes
                .iter()
                .find(|c| c.tbl == "orders")
                .unwrap()
                .row_id
                .clone();
            let settle = |rev: i64| {
                submit(
                    IntentPayload::SessionSettle(arena_proto::SessionSettle {
                        business: None,
                        coupon: None,
                        discount: None,
                        order_id: order_id.clone(),
                        credit_override: None,
                        payments: vec![
                            arena_proto::PaymentRow {
                                method: arena_proto::PaymentMethod::Cash,
                                amount_minor: 5_999,
                                reference: None,
                            },
                            arena_proto::PaymentRow {
                                method: arena_proto::PaymentMethod::Upi,
                                amount_minor: 1,
                                reference: Some("0042".into()),
                            },
                        ],
                        receipt: None,
                    }),
                    Some(rev),
                )
            };
            writer.submit_intent(settle(1)).await.unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::SessionSettleReverted(arena_proto::SessionSettleReverted {
                        order_id: order_id.clone(),
                    }),
                    Some(2),
                ))
                .await
                .unwrap();
            writer.submit_intent(settle(3)).await.unwrap();
            let topup = |amount: i64| {
                submit(
                    IntentPayload::WalletTopup(arena_proto::WalletTopup {
                        player_id: player_id.clone(),
                        amount_minor: amount,
                        payments: vec![arena_proto::PaymentRow {
                            method: arena_proto::PaymentMethod::Cash,
                            amount_minor: amount,
                            reference: None,
                        }],
                    }),
                    None,
                )
            };
            writer.submit_intent(topup(1_000)).await.unwrap();
            let second = writer.submit_intent(topup(500)).await.unwrap();
            let topup_order = second
                .changes
                .iter()
                .find(|c| c.tbl == "orders")
                .unwrap()
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::WalletTopupReverted(arena_proto::WalletTopupReverted {
                        order_id: topup_order,
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            let guest = writer
                .submit_intent(submit(
                    IntentPayload::SessionStart(arena_proto::SessionStart {
                        group_with: None,
                        pay_from_wallet: false,
                        device_id: kiosk.clone(),
                        mode: arena_proto::SessionMode::Postpaid,
                        player_id: None,
                        pack_id: None,
                        plan_id: None,
                    }),
                    None,
                ))
                .await
                .unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::SessionAssignPlayer(arena_proto::SessionAssignPlayer {
                        session_id: guest.changes[0].row_id.clone(),
                        player_id: player_id.clone(),
                        create: None,
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            for limit in [50_000, 80_000] {
                writer
                    .submit_intent(submit(
                        IntentPayload::CreditSetLimit(arena_proto::CreditSetLimit {
                            player_id: player_id.clone(),
                            limit_minor: limit,
                            reminder_days: vec![3, 7, 14],
                        }),
                        None,
                    ))
                    .await
                    .unwrap();
            }
            let guest_session = guest.changes[0].row_id.clone();
            let guest_end = writer
                .submit_intent(submit(
                    IntentPayload::SessionEnd(arena_proto::SessionEnd {
                        session_id: guest_session,
                    }),
                    Some(2),
                ))
                .await
                .unwrap();
            let guest_order = guest_end
                .changes
                .iter()
                .find(|c| c.tbl == "orders")
                .unwrap()
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::SessionSettle(arena_proto::SessionSettle {
                        business: None,
                        coupon: None,
                        discount: None,
                        order_id: guest_order,
                        credit_override: None,
                        payments: vec![arena_proto::PaymentRow {
                            method: arena_proto::PaymentMethod::Credit,
                            amount_minor: 6_000,
                            reference: None,
                        }],
                        receipt: None,
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            let collect = |amount: i64| {
                submit(
                    IntentPayload::CreditCollect(arena_proto::CreditCollect {
                        player_id: player_id.clone(),
                        amount_minor: amount,
                        payments: vec![arena_proto::PaymentRow {
                            method: arena_proto::PaymentMethod::Cash,
                            amount_minor: amount,
                            reference: None,
                        }],
                    }),
                    None,
                )
            };
            writer.submit_intent(collect(2_500)).await.unwrap();
            let second = writer.submit_intent(collect(1_000)).await.unwrap();
            let collection = second
                .changes
                .iter()
                .find(|c| c.tbl == "orders")
                .unwrap()
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::CreditCollectReverted(arena_proto::CreditCollectReverted {
                        order_id: collection,
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            let sell = || {
                submit(
                    IntentPayload::PlanSell(arena_proto::PlanSell {
                        business: None,
                        coupon: None,
                        discount: None,
                        player_id: player_id.clone(),
                        plan_id: pack_plan.clone(),
                        credit_override: None,
                        payments: vec![arena_proto::PaymentRow {
                            method: arena_proto::PaymentMethod::Cash,
                            amount_minor: 30_000,
                            reference: None,
                        }],
                        receipt: None,
                    }),
                    None,
                )
            };
            let first_sale = writer.submit_intent(sell()).await.unwrap();
            let second = writer.submit_intent(sell()).await.unwrap();
            let sale = second
                .changes
                .iter()
                .find(|c| c.tbl == "orders")
                .unwrap()
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::PlanSellReverted(arena_proto::PlanSellReverted {
                        order_id: sale,
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            // A counter sale of a tracked product (stock moves, an order with tax columns),
            // one of two undone.
            let cola = writer
                .submit_intent(submit(
                    IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                        id: None,
                        name: "Cola".into(),
                        category: String::new(),
                        price_minor: 4_000,
                        tax_rate_bp: None,
                        hsn: String::new(),
                        track_stock: true,
                        image: String::new(),
                        barcode: String::new(),
                        kitchen: false,
                        favourite: false,
                        active: true,
                        bundle: false,
                        components: Vec::new(),
                        kitchen_notes: None,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::StockIn(arena_proto::StockIn {
                        product_id: cola.clone(),
                        qty: 10,
                        unit_cost_minor: 2_000,
                        supplier: "Acme".into(),
                        expiry: None,
                    }),
                    None,
                ))
                .await
                .unwrap();
            let pos = || {
                submit(
                    IntentPayload::PosSale(arena_proto::PosSale {
                        kiosk: None,
                        business: None,
                        coupon: None,
                        discount: None,
                        lines: vec![arena_proto::PosSaleLine {
                            product_id: cola.clone(),
                            qty: 2,
                            kitchen_notes: Vec::new(),
                        }],
                        payments: vec![arena_proto::PaymentRow {
                            method: arena_proto::PaymentMethod::Cash,
                            amount_minor: 8_000,
                            reference: None,
                        }],
                        player_id: None,
                        credit_override: None,
                        stock_override: None,
                        receipt: None,
                    }),
                    None,
                )
            };
            let kept_sale_id = writer.submit_intent(pos()).await.unwrap().changes[0]
                .row_id
                .clone();
            // A coupon sale replays too: the use count, the order's coupon columns and the net
            // total all come back from the change log.
            writer
                .submit_intent(submit(
                    IntentPayload::CouponUpsert(arena_proto::CouponUpsert {
                        id: None,
                        code: "COLA10".into(),
                        percent_bp: Some(1_000),
                        amount_minor: None,
                        valid_from: None,
                        valid_to: None,
                        usage_limit: None,
                        min_bill_minor: 0,
                        targets: vec![arena_proto::CouponTarget {
                            kind: "product".into(),
                            id: cola.clone(),
                        }],
                        active: true,
                    }),
                    None,
                ))
                .await
                .unwrap();
            let mut with_coupon = pos();
            if let IntentPayload::PosSale(sale) = &mut with_coupon.payload {
                sale.coupon = Some("cola10".into());
                sale.payments[0].amount_minor = 7_200;
            }
            writer.submit_intent(with_coupon).await.unwrap();
            let undone = writer.submit_intent(pos()).await.unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::PosSaleReverted(arena_proto::PosSaleReverted {
                        order_id: undone.changes[0].row_id.clone(),
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            // A paid branch with the WhatsApp receipt on: a sale for the consenting player queues
            // one message row.
            let consenting = ulid::Ulid::generate().to_string();
            let bala = arena_core::players::seal(
                &[5; 32],
                &arena_proto::PlayerForm {
                    username: "bala".into(),
                    password: "secret1".into(),
                    phone: "+919123456789".into(),
                    name: "Bala".into(),
                    email: String::new(),
                    dob: String::new(),
                    notes: String::new(),
                    tags: Vec::new(),
                    consent: true,
                    policy_version: "1".into(),
                },
                &consenting,
                arena_core::staff::hash_secret("pw").unwrap(),
            )
            .unwrap();
            writer
                .submit_intent(submit(IntentPayload::PlayerCreate(bala), None))
                .await
                .unwrap();
            writer
                .apply_licence(arena_proto::LicenceApply {
                    standing: arena_proto::LicenceStanding::Verified,
                    clock_wrong: false,
                    tier: arena_proto::Tier::Basic,
                    flags: String::new(),
                    device_cap: 60,
                    user_cap: 10,
                })
                .await
                .unwrap();
            writer
                .submit_intent(settings_set("notify.whatsapp_receipt", "true", 1))
                .await
                .unwrap();
            let mut whatsapp = pos();
            if let IntentPayload::PosSale(sale) = &mut whatsapp.payload {
                sale.player_id = Some(consenting.clone());
                sale.receipt = Some(arena_proto::ReceiptChoice::Whatsapp);
            }
            writer.submit_intent(whatsapp).await.unwrap();
            let owned_pack = first_sale
                .changes
                .iter()
                .find(|c| c.tbl == "player_packs")
                .unwrap()
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::WalletTopup(arena_proto::WalletTopup {
                        player_id: player_id.clone(),
                        amount_minor: 20_000,
                        payments: vec![arena_proto::PaymentRow {
                            method: arena_proto::PaymentMethod::Cash,
                            amount_minor: 20_000,
                            reference: None,
                        }],
                    }),
                    None,
                ))
                .await
                .unwrap();
            for (mode, pack_id) in [
                (arena_proto::SessionMode::Pack, Some(owned_pack)),
                (arena_proto::SessionMode::Wallet, None),
            ] {
                let prepaid = writer
                    .submit_intent(submit(
                        IntentPayload::SessionStart(arena_proto::SessionStart {
                            group_with: None,
                            pay_from_wallet: false,
                            device_id: kiosk.clone(),
                            mode,
                            player_id: Some(player_id.clone()),
                            pack_id,
                            plan_id: None,
                        }),
                        None,
                    ))
                    .await
                    .unwrap();
                writer
                    .submit_intent(submit(
                        IntentPayload::SessionEnd(arena_proto::SessionEnd {
                            session_id: prepaid.changes[0].row_id.clone(),
                        }),
                        Some(1),
                    ))
                    .await
                    .unwrap();
            }
            let second_pc = writer
                .submit_intent(submit(
                    IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                        device_id: None,
                        device_kind: Default::default(),
                        class: arena_proto::ClientClass::Paired,
                        label: "Kiosk 2".into(),
                        token_hash: arena_core::devices::hash_token_secret("k2").unwrap(),
                        tv_number: None,
                        group_id: None,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                        device_id: second_pc.clone(),
                        group_id: Some(group.clone()),
                    }),
                    Some(1),
                ))
                .await
                .unwrap();
            let host = |device_id: &str, group_with: Option<String>| {
                submit(
                    IntentPayload::SessionStart(arena_proto::SessionStart {
                        group_with,
                        pay_from_wallet: false,
                        device_id: device_id.to_owned(),
                        mode: arena_proto::SessionMode::Postpaid,
                        player_id: None,
                        pack_id: None,
                        plan_id: None,
                    }),
                    None,
                )
            };
            let first = writer
                .submit_intent(host(&kiosk, None))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            let friend = writer
                .submit_intent(host(&second_pc, Some(first.clone())))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            let end_of = |session_id: &String, rev: i64| {
                submit(
                    IntentPayload::SessionEnd(arena_proto::SessionEnd {
                        session_id: session_id.clone(),
                    }),
                    Some(rev),
                )
            };
            writer.submit_intent(end_of(&first, 2)).await.unwrap();
            writer.submit_intent(end_of(&friend, 1)).await.unwrap();
            // Products placed on a running session's bill: two placements, the second undone,
            // then the End joins its time line to the same order.
            let third = writer
                .submit_intent(host(&kiosk, None))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            let place = |qty: i64| {
                submit(
                    IntentPayload::OrderPlace(arena_proto::OrderPlace {
                        counter: None,
                        session_id: third.clone(),
                        lines: vec![arena_proto::PosSaleLine {
                            product_id: cola.clone(),
                            qty,
                            kitchen_notes: Vec::new(),
                        }],
                        stock_override: None,
                    }),
                    None,
                )
            };
            writer.submit_intent(place(1)).await.unwrap();
            let second = writer.submit_intent(place(2)).await.unwrap();
            let bill = second
                .changes
                .iter()
                .find(|c| c.tbl == "orders")
                .unwrap()
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::OrderPlaceReverted(arena_proto::OrderPlaceReverted {
                        order_id: bill,
                        placement_id: second.intent_id.clone(),
                    }),
                    Some(2),
                ))
                .await
                .unwrap();
            // A kitchen product holds its stock when placed: one line is served (its hold taken
            // and closed), one is cancelled (its hold released, the line and a waste move).
            let burger = writer
                .submit_intent(submit(
                    IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                        id: None,
                        name: "Burger".into(),
                        category: String::new(),
                        price_minor: 9_000,
                        tax_rate_bp: None,
                        hsn: String::new(),
                        track_stock: true,
                        image: String::new(),
                        barcode: String::new(),
                        kitchen: true,
                        favourite: false,
                        active: true,
                        bundle: false,
                        components: Vec::new(),
                        kitchen_notes: None,
                    }),
                    None,
                ))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
            writer
                .submit_intent(submit(
                    IntentPayload::StockIn(arena_proto::StockIn {
                        product_id: burger.clone(),
                        qty: 10,
                        unit_cost_minor: 2_000,
                        supplier: String::new(),
                        expiry: None,
                    }),
                    None,
                ))
                .await
                .unwrap();
            for qty in [1, 2] {
                let placed = writer
                    .submit_intent(submit(
                        IntentPayload::OrderPlace(arena_proto::OrderPlace {
                            counter: None,
                            session_id: third.clone(),
                            lines: vec![arena_proto::PosSaleLine {
                                product_id: burger.clone(),
                                qty,
                                kitchen_notes: Vec::new(),
                            }],
                            stock_override: None,
                        }),
                        None,
                    ))
                    .await
                    .unwrap();
                let line = placed
                    .changes
                    .iter()
                    .find(|c| c.tbl == "order_lines")
                    .unwrap()
                    .row_id
                    .clone();
                let step = if qty == 1 {
                    IntentPayload::OrderLineServed(arena_proto::OrderLineServed { line_id: line })
                } else {
                    IntentPayload::OrderLineCancel(arena_proto::OrderLineCancel {
                        line_id: line,
                        made: true,
                    })
                };
                let mut intent = submit(step, None);
                if qty == 2 {
                    intent.reason = Some(arena_proto::ReasonCode::ChangedMind);
                }
                writer.submit_intent(intent).await.unwrap();
            }
            writer.submit_intent(end_of(&third, 1)).await.unwrap();
            let raise = |kind: arena_proto::AlertKind| {
                let mut intent = submit(
                    IntentPayload::AlertRaise(arena_proto::AlertRaise::new(
                        kind,
                        Some(kiosk.clone()),
                    )),
                    None,
                );
                intent.actor = Actor {
                    kind: ActorKind::System,
                    id: "hub".into(),
                };
                intent
            };
            let alert = writer
                .submit_intent(raise(arena_proto::AlertKind::Fixture))
                .await
                .unwrap();
            writer
                .submit_intent(submit(
                    IntentPayload::AlertAck(arena_proto::AlertAck {
                        alert_id: alert.changes[0].row_id.clone(),
                    }),
                    None,
                ))
                .await
                .unwrap();
            writer
                .submit_intent(raise(arena_proto::AlertKind::TimeUp))
                .await
                .unwrap();
            // A lot that expires two days after T0 (a branch day 2027-01-15).
            let expiring = writer
                .submit_intent(submit(
                    IntentPayload::StockIn(arena_proto::StockIn {
                        product_id: cola.clone(),
                        qty: 4,
                        unit_cost_minor: 150,
                        supplier: "Acme".into(),
                        expiry: Some("2027-01-17".into()),
                    }),
                    None,
                ))
                .await
                .unwrap();
            let lot_id = expiring
                .changes
                .iter()
                .find(|c| c.tbl == "stock_lots")
                .unwrap()
                .row_id
                .clone();
            clock.advance(16 * 24 * 60 * 60 * 1000);
            writer
                .credit_auto_pause(None)
                .await
                .unwrap()
                .expect("the overdue tab is paused");
            // The new business day replaces the unacknowledged alert, then the lot is written off.
            writer
                .stock_expiry_check()
                .await
                .unwrap()
                .expect("the expired lot raises the alert afresh");
            writer
                .submit_intent(arena_proto::Intent {
                    issued_at: T0 + 16 * 24 * 60 * 60 * 1000,
                    ..submit(
                        IntentPayload::StockWriteOff(arena_proto::StockWriteOff { lot_id }),
                        None,
                    )
                })
                .await
                .unwrap();
            // The branch summary was published (twice: the second only updates the row).
            writer
                .publish_summary(T0, "state/latest.json".into())
                .await
                .unwrap();
            writer
                .publish_summary(T0 + 1, "state/latest.json".into())
                .await
                .unwrap();
            // A daily rollup was published, and then regenerated (the second only updates it).
            for built_at in [T0, T0 + 1] {
                writer
                    .publish_rollup(
                        "2026-10-01".into(),
                        1,
                        built_at,
                        "rollups/2026-10-01.json".into(),
                    )
                    .await
                    .unwrap();
            }
            // A yearly GST archive was published, and then rebuilt (the second only updates it).
            for (built_at, orphaned) in [
                (T0, vec![]),
                (T0 + 1, vec!["ARN/FY2026-27/E1/9".to_owned()]),
            ] {
                writer
                    .publish_gst_archive(arena_proto::GstArchivePublish {
                        fy: "FY2026-27".into(),
                        key: "gst-archive/FY2026-27.zst".into(),
                        sha256: "0f".repeat(32),
                        bytes: 123,
                        invoices: 4,
                        credit_notes: 1,
                        orphaned,
                        built_at,
                    })
                    .await
                    .unwrap();
            }
            // The counter sale kept above is long past its Undo window: the system numbers it.
            let mut number = submit(
                IntentPayload::InvoiceIssue(arena_proto::InvoiceIssue {
                    order_id: kept_sale_id.clone(),
                }),
                None,
            );
            number.actor = Actor {
                kind: ActorKind::System,
                id: "hub".into(),
            };
            number.issued_at = T0 + 16 * 24 * 60 * 60 * 1000;
            writer.submit_intent(number).await.unwrap();
            // A refund of one cola against that invoice: a credit note, a negative payment, a
            // cash event and the line's refunded counters all replay too.
            let numbered = Store::open(&path, T0).unwrap();
            let sale_row = numbered
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .find(|o| o["id"] == RowValue::Text(kept_sale_id.clone()))
                .unwrap();
            let RowValue::Text(invoice_no) = sale_row["invoice_no"].clone() else {
                panic!("the sale was numbered");
            };
            let line_id = match numbered
                .rows(Table::OrderLines)
                .unwrap()
                .into_iter()
                .find(|l| l["order_id"] == RowValue::Text(kept_sale_id.clone()))
                .map(|l| l["id"].clone())
            {
                Some(RowValue::Text(id)) => id,
                other => panic!("the sale has a line: {other:?}"),
            };
            let mut refund = submit(
                IntentPayload::RefundCreate(arena_proto::RefundCreate {
                    invoice_no,
                    lines: vec![arena_proto::RefundLineReq {
                        line_id,
                        qty: 1,
                        return_to_stock: Some(true),
                    }],
                    method: Some(arena_proto::PaymentMethod::Cash),
                    reference: None,
                    note: None,
                    receipt: None,
                }),
                None,
            );
            refund.reason = Some(arena_proto::ReasonCode::Mistake);
            refund.issued_at = T0 + 16 * 24 * 60 * 60 * 1000;
            writer.submit_intent(refund).await.unwrap();
            // A deposit and an expense carry their detail columns through the replay as well.
            let mut deposit = submit(
                IntentPayload::CashDeposit(arena_proto::CashDeposit {
                    amount_minor: 100,
                    reference: "SLIP-1".into(),
                }),
                None,
            );
            deposit.issued_at = T0 + 16 * 24 * 60 * 60 * 1000;
            writer.submit_intent(deposit).await.unwrap();
            let mut expense = submit(
                IntentPayload::ExpenseCreate(arena_proto::ExpenseCreate {
                    category: arena_proto::ExpenseCategory::Repairs,
                    amount_minor: 50,
                    paid_from: arena_proto::ExpensePaidFrom::Drawer,
                    receipt_photo: Some("data:image/jpeg;base64,/9j/AA==".into()),
                    note: Some("tape".into()),
                }),
                None,
            );
            expense.issued_at = T0 + 16 * 24 * 60 * 60 * 1000;
            writer.submit_intent(expense).await.unwrap();
            // Closing the shift updates its row with the count.
            let mut close = submit(
                IntentPayload::ShiftClose(arena_proto::ShiftClose {
                    counted_minor: 0,
                    deposit: None,
                    reason: Some(arena_proto::VarianceReason::CountingError),
                    note: None,
                }),
                None,
            );
            close.issued_at = T0 + 16 * 24 * 60 * 60 * 1000;
            writer.submit_intent(close).await.unwrap();
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
    async fn a_published_gst_archive_replays_into_its_log_row() {
        let (_a, source) = seeded_with_device().await;
        let (_b, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(source.changes().unwrap(), tx))
            .unwrap();
        let want = source.rows(Table::GstArchiveLog).unwrap();
        assert_eq!(want.len(), 1, "one row, published twice");
        assert_eq!(dest.rows(Table::GstArchiveLog).unwrap(), want);
    }

    #[tokio::test]
    async fn a_published_summary_replays_into_its_rollup_log_row() {
        let (_a, source) = seeded_with_device().await;
        let (_b, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(source.changes().unwrap(), tx))
            .unwrap();
        let want = source.rows(Table::RollupLog).unwrap();
        assert_eq!(
            want.len(),
            2,
            "the summary and one day's rollup, each published twice"
        );
        assert_eq!(dest.rows(Table::RollupLog).unwrap(), want);
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
            assert!(!want.is_empty(), "{} has no row", table.name());
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
            onboarding: None,
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

    // ---- the audit builder (Story 8.36) ----

    fn batches_of(changes: &[ChangeRecord], size: usize) -> Vec<arena_store::DecodedBatch> {
        let mut epochs: Vec<&str> = changes.iter().map(|c| c.epoch.as_str()).collect();
        epochs.dedup();
        let mut out = Vec::new();
        for epoch in epochs {
            let rows: Vec<ChangeRecord> = changes
                .iter()
                .filter(|c| c.epoch == epoch)
                .cloned()
                .collect();
            for chunk in rows.chunks(size) {
                out.push(arena_store::DecodedBatch {
                    header: arena_store::BatchHeader {
                        epoch: epoch.to_owned(),
                        fork: None,
                        first: chunk[0].seq,
                        last: chunk[chunk.len() - 1].seq,
                        sha256: String::new(),
                        schema: 0,
                    },
                    rows: chunk.to_vec(),
                });
            }
        }
        out
    }

    fn month_of(changes: &[ChangeRecord]) -> String {
        changes
            .iter()
            .filter_map(|c| c.business_day.as_deref())
            .map(|d| d[..7].to_owned())
            .max()
            .expect("a change with a business day")
    }

    fn build(
        dir: &std::path::Path,
        month: &str,
        snapshot: Option<&[u8]>,
        forks: Vec<EpochFork>,
        batches: &[arena_store::DecodedBatch],
    ) -> Result<BuiltMonth, AppError> {
        let mut b = MonthBuilder::open(dir, month, snapshot, forks, T0)?;
        for batch in batches {
            if !b.apply(batch)? {
                break;
            }
        }
        b.finish()
    }

    #[tokio::test]
    async fn a_month_rebuilt_from_batches_has_the_hubs_checksum() {
        let (_a, source) = seeded_with_device().await;
        let changes = source.changes().unwrap();
        let month = month_of(&changes);
        let work = tempfile::tempdir().unwrap();
        let built = build(work.path(), &month, None, vec![], &batches_of(&changes, 3)).unwrap();
        assert_eq!(built.checksum, state_checksum(&source).unwrap());
        assert_eq!(built.applied, changes.len());
        assert_eq!(built.orphaned, 0);
        let last = changes.last().unwrap();
        assert_eq!(
            (built.epoch.as_str(), built.seq),
            (last.epoch.as_str(), last.seq)
        );
    }

    #[tokio::test]
    async fn the_rebuilt_database_keeps_the_change_log_for_audit_search() {
        let (_a, source) = seeded_with_device().await;
        let changes = source.changes().unwrap();
        let work = tempfile::tempdir().unwrap();
        let built = build(
            work.path(),
            &month_of(&changes),
            None,
            vec![],
            &batches_of(&changes, 4),
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("audit.db");
        std::fs::write(&path, zstd::decode_all(&built.bytes[..]).unwrap()).unwrap();
        let rebuilt = Store::open(&path, T0).unwrap();
        assert_eq!(rebuilt.changes().unwrap().len(), changes.len());
    }

    #[tokio::test]
    async fn a_build_resumes_after_a_snapshot_and_skips_what_it_holds() {
        let (_a, source) = seeded_with_device().await;
        let changes = source.changes().unwrap();
        let month = month_of(&changes);
        let all = batches_of(&changes, 3);
        assert!(all.len() > 2, "the fixture spans several batches");

        // A snapshot of the log up to its second batch.
        let early = tempfile::tempdir().unwrap();
        let snap = build(early.path(), &month, None, vec![], &all[..2]).unwrap();
        let held = snap.applied;

        // Every batch is offered; the two the snapshot holds are skipped.
        let work = tempfile::tempdir().unwrap();
        let built = build(work.path(), &month, Some(&snap.bytes), vec![], &all).unwrap();
        assert_eq!(built.applied, changes.len() - held);
        assert_eq!(built.checksum, state_checksum(&source).unwrap());
    }

    #[tokio::test]
    async fn a_hub_snapshot_alone_rebuilds_the_same_rows() {
        let (a, source) = seeded_with_device().await;
        let snap = arena_store::build_snapshot_file(source.path(), a.path(), "2027-01-15").unwrap();
        let work = tempfile::tempdir().unwrap();
        let built = build(
            work.path(),
            "2027-01",
            Some(&snap.read().unwrap()),
            vec![],
            &[],
        )
        .unwrap();
        assert_eq!(built.checksum, state_checksum(&source).unwrap());
        assert_eq!(built.applied, 0);
        assert_eq!(built.seq, snap.last_seq);
    }

    #[tokio::test]
    async fn replay_stops_at_the_first_change_of_a_later_month() {
        let (_a, source) = seeded_with_device().await;
        let mut changes = source.changes().unwrap();
        let month = month_of(&changes);
        let last = changes.len() - 1;
        changes[last].business_day = Some("2099-12-31".to_owned());
        let work = tempfile::tempdir().unwrap();
        let built = build(work.path(), &month, None, vec![], &batches_of(&changes, 3)).unwrap();
        assert_eq!(built.applied, changes.len() - 1);
        assert_ne!(built.checksum, state_checksum(&source).unwrap());
    }

    #[tokio::test]
    async fn a_missing_batch_fails_the_build() {
        let (_a, source) = seeded_with_device().await;
        let changes = source.changes().unwrap();
        let mut all = batches_of(&changes, 3);
        all.remove(1);
        let work = tempfile::tempdir().unwrap();
        let err = build(work.path(), &month_of(&changes), None, vec![], &all).unwrap_err();
        assert!(err.message.contains("seq gap"), "{}", err.message);
    }

    #[tokio::test]
    async fn rows_above_a_fork_point_are_kept_apart_not_replayed() {
        let (_a, source) = seeded_with_device().await;
        let mut changes: Vec<ChangeRecord> = source.changes().unwrap();
        for c in &mut changes {
            c.epoch = "1:A".to_owned();
        }
        let month = month_of(&changes);
        let n = changes.len() as i64;
        // The last change was written by a Hub that was replaced: epoch 2 continues from n - 1.
        let setting = changes
            .iter()
            .rfind(|c| c.tbl == "settings")
            .unwrap()
            .clone();
        let mut row: Row = rmp_serde::from_slice(&setting.row).unwrap();
        row.insert("value".into(), RowValue::Text("forked".into()));
        let second = ChangeRecord {
            epoch: "2:B".to_owned(),
            seq: n,
            op: ChangeOp::Update,
            row: rmp_serde::to_vec_named(&row).unwrap(),
            ..setting
        };
        let mut log = changes.clone();
        log.push(second);
        let forks = vec![EpochFork {
            epoch: "2:B".into(),
            parent_epoch: "1:A".into(),
            parent_seq: n - 1,
        }];
        let work = tempfile::tempdir().unwrap();
        let built = build(work.path(), &month, None, forks, &batches_of(&log, 3)).unwrap();
        assert_eq!(built.orphaned, 1);
        assert_eq!(built.applied, log.len() - 1);
        assert_eq!((built.epoch.as_str(), built.seq), ("2:B", n));

        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("audit.db");
        std::fs::write(&path, zstd::decode_all(&built.bytes[..]).unwrap()).unwrap();
        let rebuilt = Store::open(&path, T0).unwrap();
        assert!(rebuilt.rows(Table::Settings).unwrap().contains(&row));
        let orphans: i64 = arena_store::DbConn::open(&path)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM orphaned_changes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(orphans, 1);
    }

    #[tokio::test]
    async fn a_snapshot_above_a_fork_point_cannot_be_resumed() {
        let (_a, source) = seeded_with_device().await;
        let changes = source.changes().unwrap();
        let month = month_of(&changes);
        let work = tempfile::tempdir().unwrap();
        let snap = build(work.path(), &month, None, vec![], &batches_of(&changes, 3)).unwrap();
        let forks = vec![EpochFork {
            epoch: "9:Z".into(),
            parent_epoch: changes[0].epoch.clone(),
            parent_seq: 1,
        }];
        let work2 = tempfile::tempdir().unwrap();
        let mut b = MonthBuilder::open(work2.path(), &month, Some(&snap.bytes), forks, T0).unwrap();
        let err = b.apply(&batches_of(&changes, 3)[0]).unwrap_err();
        assert!(
            err.message.contains("above the fork point"),
            "{}",
            err.message
        );
    }
}
