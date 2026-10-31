//! Story 9.21: the database part of promoting a standby to main.

use arena_proto::{ErrorCode, PAIR_AGAIN_TOKEN};

use crate::branch::ChangeRecord;
use crate::promote::{META_MAIN_SCHEMA, PromoteInput};
use crate::{
    ChangeOp, LeaseRow, META_HUB_INSTANCE_ID, NewDisplay, Store, append_replicated, db_error,
    erase_player, insert_display, raise_max_seen_counter, read_lease_fork, read_max_seen_counter,
    set_display_revoked, set_meta, write_lease,
};

const NOW: i64 = 1_800_000_000_000;
const TTL: i64 = 15_000;

fn standby() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("standby.db"), 1).unwrap();
    s.write_tx(|tx| set_meta(tx, META_HUB_INSTANCE_ID, "STBY"))
        .unwrap();
    (dir, s)
}

fn change(epoch: &str, seq: i64, tbl: &str, row_id: &str, kind: &str) -> ChangeRecord {
    ChangeRecord {
        epoch: epoch.into(),
        seq,
        tbl: tbl.into(),
        row_id: row_id.into(),
        op: ChangeOp::Insert,
        row: vec![0x80],
        intent_id: format!("i{seq}"),
        intent_kind: kind.into(),
        actor_kind: "system".into(),
        actor_id: "t".into(),
        elevated_by_kind: None,
        elevated_by_id: None,
        elevated_by_mode: None,
        elevated_by_verifier_version: None,
        at: seq,
        business_day: None,
        fy: None,
        reason: None,
    }
}

fn follow(s: &mut Store, epoch: &str, rows: i64) {
    let rows: Vec<ChangeRecord> = (1..=rows)
        .map(|n| change(epoch, n, "settings", &format!("b/k{n}"), "settings.set"))
        .collect();
    s.write_tx(|tx| append_replicated(tx, &rows)).unwrap();
}

fn player(s: &mut Store, id: &str, key: bool) {
    s.write_tx(|tx| {
        tx.execute(
            "INSERT INTO players (player_id, username, password_hash, phone_lookup, tags, rev,
                                  created_at, updated_at, name_enc, phone_enc)
             VALUES (?1, 'u-' || lower(?1), '', ?1, '[]', 0, 0, 0, 'n', 'p')",
            [id],
        )
        .map_err(db_error)?;
        if key {
            tx.execute("INSERT INTO player_keys VALUES (?1, 'w')", [id])
                .map_err(db_error)?;
        }
        Ok(())
    })
    .unwrap();
}

fn input(claimed: Option<&str>) -> PromoteInput<'_> {
    PromoteInput {
        now_ms: NOW,
        lease_ttl_ms: TTL,
        claimed_epoch: claimed,
    }
}

#[test]
fn the_basis_is_where_the_followed_log_ends() {
    let (_d, mut s) = standby();
    let empty = s.promote_basis().unwrap();
    assert_eq!((empty.old_epoch.as_str(), empty.fork_seq), ("", 0));
    follow(&mut s, "3:MAIN", 7);
    let basis = s.promote_basis().unwrap();
    assert_eq!(
        (
            basis.old_epoch.as_str(),
            basis.fork_seq,
            basis.instance_id.as_str(),
            basis.max_seen
        ),
        ("3:MAIN", 7, "STBY", 3)
    );
    assert_eq!(basis.main_schema, None);
}

#[test]
fn a_claimed_epoch_becomes_the_lease_and_the_fork_is_recorded() {
    let (_d, mut s) = standby();
    follow(&mut s, "3:MAIN", 7);
    let out = s.promote_to_main(&input(Some("4:STBY"))).unwrap();
    assert_eq!(
        (out.new_epoch.as_str(), out.counter, out.provisional),
        ("4:STBY", 4, false)
    );
    assert_eq!((out.old_epoch.as_str(), out.fork_seq), ("3:MAIN", 7));
    assert_eq!(
        s.lease().unwrap().unwrap(),
        LeaseRow {
            counter: 4,
            hub_instance_id: "STBY".into(),
            provisional: false,
            renewed_at: NOW,
            expires_at: NOW + TTL,
        }
    );
    let fork = s.write_tx(|tx| read_lease_fork(tx)).unwrap().unwrap();
    assert_eq!((fork.parent_epoch.as_str(), fork.parent_seq), ("3:MAIN", 7));
    assert_eq!(s.write_tx(|tx| read_max_seen_counter(tx)).unwrap(), 4);
}

#[test]
fn with_no_answer_from_the_cloud_the_epoch_is_provisional_and_above_everything_seen() {
    let (_d, mut s) = standby();
    follow(&mut s, "3:MAIN", 2);
    s.write_tx(|tx| {
        write_lease(
            tx,
            &LeaseRow {
                counter: 5,
                hub_instance_id: "STBY".into(),
                provisional: false,
                renewed_at: 0,
                expires_at: 0,
            },
        )?;
        raise_max_seen_counter(tx, 9)?;
        Ok(())
    })
    .unwrap();
    let out = s.promote_to_main(&input(None)).unwrap();
    assert_eq!((out.new_epoch.as_str(), out.provisional), ("10:STBY", true));
    assert!(s.lease().unwrap().unwrap().provisional);
    assert_eq!(s.write_tx(|tx| read_max_seen_counter(tx)).unwrap(), 10);
}

#[test]
fn an_epoch_the_hub_cannot_hold_is_refused_and_nothing_changes() {
    let (_d, mut s) = standby();
    follow(&mut s, "3:MAIN", 2);
    for bad in ["4:OTHER", "3:STBY", "nonsense"] {
        let err = s.promote_to_main(&input(Some(bad))).unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal, "{bad}");
    }
    assert_eq!(s.lease().unwrap(), None);
}

#[test]
fn a_standby_behind_the_main_hubs_schema_does_not_promote() {
    let (_d, mut s) = standby();
    follow(&mut s, "3:MAIN", 2);
    let ours = s.schema_version().unwrap();
    s.record_main_schema(ours + 1, false).unwrap();
    let err = s.promote_to_main(&input(None)).unwrap_err();
    assert_eq!(err.code, ErrorCode::SchemaBehind);
    assert_eq!(s.lease().unwrap(), None);
    // The main Hub saying it is at this schema again clears it.
    s.record_main_schema(ours, false).unwrap();
    s.promote_to_main(&input(None)).unwrap();
    assert_eq!(s.meta(META_MAIN_SCHEMA).unwrap(), Some(ours.to_string()));
}

#[test]
fn a_lower_bound_never_lowers_what_the_main_hub_said() {
    let (_d, mut s) = standby();
    s.record_main_schema(90, false).unwrap();
    s.record_main_schema(80, true).unwrap();
    assert_eq!(s.main_schema().unwrap(), Some(90));
    s.record_main_schema(95, true).unwrap();
    assert_eq!(s.main_schema().unwrap(), Some(95));
}

#[test]
fn every_erasure_holds_after_the_promote() {
    let (_d, mut s) = standby();
    follow(&mut s, "3:MAIN", 1);
    // P1 is erased in its row but its key is still here; P2's erasure is in the log but the row
    // was never tombstoned; P3 is an ordinary player.
    player(&mut s, "P1", true);
    player(&mut s, "P2", true);
    player(&mut s, "P3", true);
    s.write_tx(|tx| {
        erase_player(tx, "P1", 500)?;
        append_replicated(tx, &[change("3:MAIN", 2, "players", "P2", "player.erased")])
    })
    .unwrap();
    let out = s.promote_to_main(&input(Some("4:STBY"))).unwrap();
    assert_eq!(out.erasures_reapplied, 2);
    let keys = s
        .replication_key_states(&["P1".into(), "P2".into(), "P3".into()])
        .unwrap();
    assert_eq!(
        keys,
        vec![
            ("P1".to_owned(), None),
            ("P2".to_owned(), None),
            ("P3".to_owned(), Some("w".to_owned()))
        ]
    );
    assert!(s.write_tx(|tx| crate::player_erased(tx, "P2")).unwrap());
    assert!(!s.write_tx(|tx| crate::player_erased(tx, "P3")).unwrap());
}

#[test]
fn kitchen_displays_must_be_paired_again_and_revoked_ones_are_left_alone() {
    let (_d, mut s) = standby();
    follow(&mut s, "3:MAIN", 1);
    s.write_tx(|tx| {
        for (id, name) in [("A", "Pass"), ("B", "Grill"), ("C", "Old")] {
            insert_display(
                tx,
                &NewDisplay {
                    id,
                    name,
                    token_hash: "$argon2id$x",
                    created_by: None,
                },
            )?;
        }
        set_display_revoked(tx, "C", 5)
    })
    .unwrap();
    let out = s.promote_to_main(&input(None)).unwrap();
    assert_eq!(out.kitchen_displays_to_repair, 2);
    let shown = |id: &str| s.kitchen_display(id).unwrap().unwrap().token_hash;
    assert_eq!(shown("A"), PAIR_AGAIN_TOKEN);
    assert_eq!(shown("B"), PAIR_AGAIN_TOKEN);
    assert_eq!(shown("C"), "$argon2id$x");
}
