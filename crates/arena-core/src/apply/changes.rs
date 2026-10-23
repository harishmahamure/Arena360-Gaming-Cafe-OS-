//! **append_changes.** One `changes` row per written row, gap-free per epoch.

use arena_proto::{AppError, ErrorCode, Intent};
use arena_store::{ChangeOp, ChangeRecord, StoreTx, insert_change, next_change_seq, read_row};

use super::ChangeMeta;
use super::mutate::Written;
use crate::status::Epoch;

pub(super) fn append_changes(
    tx: &StoreTx<'_>,
    epoch: &Epoch,
    intent: &Intent,
    written: &[Written],
    now_ms: i64,
) -> Result<Vec<ChangeMeta>, AppError> {
    let epoch = epoch.to_string();
    let elevated = intent.elevated_by.as_ref();
    let mut metas = Vec::with_capacity(written.len());
    for w in written {
        // The full row after the write; a delete has none, so it records the row before it.
        let row = match w.op {
            ChangeOp::Delete => w.before.clone().ok_or_else(|| {
                AppError::new(
                    ErrorCode::Internal,
                    format!("delete of {} carries no prior row", w.key.row_id()),
                )
            })?,
            ChangeOp::Insert | ChangeOp::Update => read_row(tx, &w.key)?,
        };
        let bytes = rmp_serde::to_vec_named(&row)
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("encode change row: {e}")))?;
        let record = ChangeRecord {
            epoch: epoch.clone(),
            seq: next_change_seq(tx, &epoch)?,
            tbl: w.key.table().name().to_owned(),
            row_id: w.key.row_id(),
            op: w.op,
            row: bytes,
            intent_id: intent.intent_id.clone(),
            intent_kind: intent.kind.as_str().to_owned(),
            actor_kind: intent.actor.kind.as_str().to_owned(),
            actor_id: intent.actor.id.clone(),
            elevated_by_kind: elevated.map(|e| e.kind.as_str().to_owned()),
            elevated_by_id: elevated.map(|e| e.id.clone()),
            elevated_by_mode: elevated.map(|e| e.mode.as_str().to_owned()),
            elevated_by_verifier_version: elevated.map(|e| i64::from(e.verifier_version)),
            at: now_ms,
        };
        insert_change(tx, &record)?;
        metas.push(ChangeMeta {
            epoch: record.epoch,
            seq: record.seq,
            tbl: record.tbl,
            row_id: record.row_id,
            op: record.op,
        });
    }
    Ok(metas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::{Actor, ActorKind, BranchBootstrap, IntentKind, IntentPayload};
    use arena_store::{
        NewBranch, Row, RowKey, Store, Table, delete_row, insert_branch, insert_setting,
    };

    #[test]
    fn a_delete_records_the_row_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        let key = RowKey::Settings {
            branch_id: "B".into(),
            key: "k".into(),
        };
        let intent = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::BranchBootstrap,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: 0,
            base_rev: None,
            actor: Actor {
                kind: ActorKind::System,
                id: "t".into(),
            },
            elevated_by: None,
            payload: IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "n".into(),
                branch_code: "c".into(),
                currency: "INR".into(),
                tz: "UTC".into(),
                day_start_min: 0,
                fy_start_month: 1,
            }),
        };
        let epoch = Epoch {
            counter: 1,
            hub_instance_id: "H".into(),
        };
        let (before, metas) = store
            .write_tx(|tx| {
                insert_branch(
                    tx,
                    &NewBranch {
                        branch_id: "B",
                        name: "n",
                        branch_code: "c",
                        currency: "INR",
                        tz: "UTC",
                        day_start_min: 0,
                        fy_start_month: 1,
                    },
                )?;
                insert_setting(tx, "B", "k", "v")?;
                let before = read_row(tx, &key)?;
                assert_eq!(delete_row(tx, Table::Settings, &before)?, 1);
                let written = [Written {
                    key: key.clone(),
                    op: ChangeOp::Delete,
                    before: Some(before.clone()),
                }];
                Ok((before, append_changes(tx, &epoch, &intent, &written, 7)?))
            })
            .unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].op, ChangeOp::Delete);
        let change = &store.changes().unwrap()[0];
        assert_eq!(
            (change.op, change.row_id.as_str()),
            (ChangeOp::Delete, "B/k")
        );
        let row: Row = rmp_serde::from_slice(&change.row).unwrap();
        assert_eq!(row, before);
    }
}
