//! Story 10.17: the Android Hub hands the branch to a Windows Hub.

use super::session::refusal;
use super::stock::{intent, owner, up};
use super::*;
use crate::SnapshotReader;
use crate::handover::{Handover, PROOF_CLAIM, handover_proof};
use arena_proto::{
    HANDOVER_FAILURES_PER_CODE, HANDOVER_STATE_KEY, HandoverAck, HandoverClaim, HandoverPhase,
    MigrationNotice,
};

const FP: &str = "ab";
const PC: &str = "Front desk";

struct Move {
    f: Fixture,
    handover: Handover,
    scratch: tempfile::TempDir,
}

async fn open_move(f: Fixture, stored: Option<&str>) -> Move {
    let handover = Handover::open(
        f.writer.clone(),
        Arc::new(SnapshotReader::open(&f.path).unwrap()),
        f.status.clone(),
        f.clock.clone(),
        stored,
    )
    .unwrap();
    Move {
        f,
        handover,
        scratch: tempfile::tempdir().unwrap(),
    }
}

async fn asked() -> Move {
    let m = open_move(bootstrapped().await, None).await;
    ask(&m);
    m
}

fn ask(m: &Move) {
    m.f.status.send_modify(|s| {
        s.move_notice = Some(MigrationNotice {
            id: "M1".into(),
            target_hub_name: PC.into(),
        });
    });
}

fn claim_with(code: &str) -> HandoverClaim {
    HandoverClaim {
        hub_name: PC.into(),
        proof: handover_proof(code, FP, PROOF_CLAIM, PC),
        successor: None,
    }
}

async fn code_of(m: &Move) -> String {
    m.handover.view().await.unwrap().code.unwrap()
}

async fn change(f: &Fixture) -> Result<Applied, AppError> {
    let mut i = intent(
        owner(),
        IntentPayload::ProductUpsert(up(&format!("Tea {}", ulid::Ulid::generate()))),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn last_seq(f: &Fixture) -> i64 {
    reopen(f).changes().unwrap().last().unwrap().seq
}

#[tokio::test]
async fn confirming_without_a_move_asked_for_is_refused() {
    let m = open_move(bootstrapped().await, None).await;
    let e = m.handover.begin().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::ValidationFailed);
    assert!(m.handover.view().await.is_none());
}

#[tokio::test]
async fn confirming_shows_a_six_digit_code_and_asking_again_keeps_it() {
    let m = asked().await;
    let view = m.handover.begin().await.unwrap();
    assert_eq!(view.phase, HandoverPhase::Code);
    assert_eq!(view.target_hub_name, PC);
    let code = view.code.clone().unwrap();
    assert!(code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(m.handover.begin().await.unwrap().code.unwrap(), code);
    // The Hub still serves while the code is on screen.
    assert!(m.f.status.borrow().handover.is_some());
    assert!(change(&m.f).await.is_ok());
}

#[tokio::test]
async fn a_good_claim_stops_the_hub_drains_it_and_writes_a_migrate_snapshot() {
    let m = asked().await;
    m.handover.begin().await.unwrap();
    let code = code_of(&m).await;
    let claimed = m
        .handover
        .claim(&claim_with(&code), FP, m.scratch.path())
        .await
        .unwrap();
    assert_eq!(
        claimed.proof,
        handover_proof(
            &code,
            FP,
            crate::handover::PROOF_REPLY,
            &claimed.transfer_token
        )
    );
    assert_eq!(claimed.snapshot.last_seq, last_seq(&m.f));
    // No intent is applied any more: nothing after the snapshot's last seq.
    let e = change(&m.f).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::HubHandingOver);
    assert_eq!(last_seq(&m.f), claimed.snapshot.last_seq);
    let view = m.handover.view().await.unwrap();
    assert_eq!(view.phase, HandoverPhase::Transferring);
    assert_eq!(view.code, None);

    let (path, facts) = m
        .handover
        .snapshot_for(&claimed.transfer_token)
        .await
        .unwrap();
    assert_eq!(facts, claimed.snapshot);
    let (sha, bytes) = crate::handover::file_sha256(&path).unwrap();
    assert_eq!((sha, bytes), (facts.sha256.clone(), facts.bytes));
    let header = arena_proto::ExportHeader::read(&mut std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(header.purpose.as_deref(), Some("migrate"));
    assert_eq!(header.last_seq, facts.last_seq);
    assert_eq!(header.epoch, facts.epoch);
    assert_eq!(header.branch_id, facts.branch_id);
    assert!(m.handover.snapshot_for("nope").await.is_none());
}

#[tokio::test]
async fn a_claim_with_another_certificate_or_name_is_refused_and_counted() {
    let m = asked().await;
    m.handover.begin().await.unwrap();
    let code = code_of(&m).await;
    // A certificate in the middle shows the PC another fingerprint.
    let r = m
        .handover
        .claim(&claim_with(&code), "cd", m.scratch.path())
        .await
        .unwrap_err();
    assert_eq!(r.code, ErrorCode::PairCodeInvalid);
    let wrong_name = HandoverClaim {
        hub_name: "Other PC".into(),
        proof: handover_proof(&code, FP, PROOF_CLAIM, "Other PC"),
        successor: None,
    };
    assert!(
        m.handover
            .claim(&wrong_name, FP, m.scratch.path())
            .await
            .is_err()
    );
    // Still serving: nothing was stopped.
    assert!(change(&m.f).await.is_ok());
}

#[tokio::test]
async fn the_sixth_wrong_code_invalidates_it_and_shows_a_new_one() {
    let m = asked().await;
    m.handover.begin().await.unwrap();
    let first = code_of(&m).await;
    let wrong = if first == "000000" {
        "111111"
    } else {
        "000000"
    };
    for _ in 0..HANDOVER_FAILURES_PER_CODE {
        let r = m
            .handover
            .claim(&claim_with(wrong), FP, m.scratch.path())
            .await
            .unwrap_err();
        assert!(!r.code_replaced);
    }
    // Even the right proof is refused once the code is spent.
    let r = m
        .handover
        .claim(&claim_with(&first), FP, m.scratch.path())
        .await
        .unwrap_err();
    assert!(r.code_replaced);
    let second = code_of(&m).await;
    assert_ne!(second, first);
    assert!(
        m.handover
            .claim(&claim_with(&second), FP, m.scratch.path())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn an_expired_code_is_refused() {
    let m = asked().await;
    m.handover.begin().await.unwrap();
    let code = code_of(&m).await;
    m.f.clock.advance(arena_proto::HANDOVER_CODE_TTL_MS + 1);
    let r = m
        .handover
        .claim(&claim_with(&code), FP, m.scratch.path())
        .await
        .unwrap_err();
    assert_eq!(r.code, ErrorCode::PairCodeExpired);
    // Confirming again replaces it.
    let fresh = m.handover.begin().await.unwrap().code.unwrap();
    assert!(
        m.handover
            .claim(&claim_with(&fresh), FP, m.scratch.path())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn the_acknowledgement_moves_the_hub_to_read_only_and_is_reported() {
    let m = asked().await;
    m.handover.begin().await.unwrap();
    let code = code_of(&m).await;
    let c = m
        .handover
        .claim(&claim_with(&code), FP, m.scratch.path())
        .await
        .unwrap();
    let ack = HandoverAck {
        sha256: c.snapshot.sha256.clone(),
        last_seq: c.snapshot.last_seq,
    };
    // A wrong token or another snapshot's facts change nothing.
    assert!(m.handover.acknowledge("nope", &ack).await.is_err());
    let other = HandoverAck {
        last_seq: ack.last_seq + 1,
        ..ack.clone()
    };
    assert_eq!(
        m.handover
            .acknowledge(&c.transfer_token, &other)
            .await
            .unwrap_err()
            .code,
        ErrorCode::ValidationFailed
    );
    assert!(m.handover.handed_over().await.is_none());

    m.handover
        .acknowledge(&c.transfer_token, &ack)
        .await
        .unwrap();
    let view = m.handover.view().await.unwrap();
    assert_eq!(view.phase, HandoverPhase::Moved);
    assert_eq!(view.last_seq, Some(c.snapshot.last_seq));
    let done = m.handover.handed_over().await.unwrap();
    assert_eq!(
        (done.migration_id.as_str(), done.last_seq),
        ("M1", c.snapshot.last_seq)
    );
    assert_eq!(
        change(&m.f).await.unwrap_err().code,
        ErrorCode::HubHandingOver
    );
    // The token no longer serves the snapshot; the same acknowledgement is answered again (a lost
    // answer), but another token is not.
    assert!(m.handover.snapshot_for(&c.transfer_token).await.is_none());
    m.handover
        .acknowledge(&c.transfer_token, &ack)
        .await
        .unwrap();
    assert!(m.handover.acknowledge("nope", &ack).await.is_err());
    assert_eq!(
        m.handover.cancel().await.unwrap_err().code,
        ErrorCode::HubHandingOver
    );
}

#[tokio::test]
async fn cancelling_resumes_the_hub_with_the_same_epoch_and_nothing_set_aside() {
    let m = asked().await;
    m.handover.begin().await.unwrap();
    let code = code_of(&m).await;
    let c = m
        .handover
        .claim(&claim_with(&code), FP, m.scratch.path())
        .await
        .unwrap();
    let (path, _) = m.handover.snapshot_for(&c.transfer_token).await.unwrap();
    let epoch_before = reopen(&m.f)
        .changes()
        .unwrap()
        .last()
        .unwrap()
        .epoch
        .clone();
    m.handover.cancel().await.unwrap();
    assert!(m.handover.view().await.is_none());
    assert!(m.f.status.borrow().handover.is_none());
    assert!(!path.exists(), "the snapshot is deleted");
    assert!(m.handover.snapshot_for(&c.transfer_token).await.is_none());
    // The Hub takes intents again, on the epoch it had.
    assert!(change(&m.f).await.is_ok());
    assert_eq!(
        reopen(&m.f).changes().unwrap().last().unwrap().epoch,
        epoch_before
    );
    assert!(
        reopen(&m.f)
            .meta(HANDOVER_STATE_KEY)
            .unwrap()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_file_is_written_to_the_backup_folder_and_the_hub_stays_stopped() {
    let m = asked().await;
    let folder = tempfile::tempdir().unwrap();
    let mut set = intent(
        owner(),
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: arena_proto::KEY_BACKUP_DIR.into(),
            value: folder.path().to_string_lossy().into_owned(),
        }),
    );
    let rev = match &setting(&reopen(&m.f), arena_proto::KEY_BACKUP_DIR)["rev"] {
        RowValue::Integer(i) => *i,
        other => panic!("{other:?}"),
    };
    set.base_rev = Some(rev);
    m.f.writer.submit_intent(set).await.unwrap();

    assert_eq!(
        m.handover.use_file().await.unwrap_err().code,
        ErrorCode::ValidationFailed,
        "no handover has started"
    );
    m.handover.begin().await.unwrap();
    let view = m.handover.use_file().await.unwrap();
    assert_eq!(view.phase, HandoverPhase::File);
    let name = view.file_name.clone().unwrap();
    assert!(name.ends_with("-move.a360"));
    let path = folder.path().join(&name);
    let header = arena_proto::ExportHeader::read(&mut std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(header.purpose.as_deref(), Some("migrate"));
    assert_eq!(header.last_seq, last_seq(&m.f));
    assert_eq!(
        change(&m.f).await.unwrap_err().code,
        ErrorCode::HubHandingOver
    );
    // The same answer the second time, and the PC cannot pull any more.
    assert_eq!(m.handover.use_file().await.unwrap(), view);

    // Cancel resumes, and the file goes: a copy made before the Hub wrote more would roll the
    // branch back if it were ever imported.
    m.handover.cancel().await.unwrap();
    assert!(change(&m.f).await.is_ok());
    assert!(!path.exists());
}

#[tokio::test]
async fn a_hub_that_restarts_mid_handover_is_still_stopped() {
    let m = asked().await;
    m.handover.begin().await.unwrap();
    let code = code_of(&m).await;
    m.handover
        .claim(&claim_with(&code), FP, m.scratch.path())
        .await
        .unwrap();
    let stored = reopen(&m.f)
        .meta(HANDOVER_STATE_KEY)
        .unwrap()
        .expect("the state is stored");
    // A new process: a fresh status with nothing in it, the stored state read back.
    m.f.status.send_modify(|s| s.handover = None);
    assert!(change(&m.f).await.is_ok());
    let again = open_move(
        Fixture {
            status: m.f.status.clone(),
            events: m.f.events.resubscribe(),
            ..m.f
        },
        Some(&stored),
    )
    .await;
    assert_eq!(
        again.f.status.borrow().handover.as_ref().unwrap().phase,
        HandoverPhase::Transferring
    );
    assert_eq!(
        change(&again.f).await.unwrap_err().code,
        ErrorCode::HubHandingOver
    );
}

#[tokio::test]
async fn a_move_the_cloud_no_longer_asks_for_ends_an_unclaimed_code_only() {
    let m = asked().await;
    m.handover.begin().await.unwrap();
    m.handover
        .on_notice(Some(&MigrationNotice {
            id: "M1".into(),
            target_hub_name: PC.into(),
        }))
        .await;
    assert!(m.handover.view().await.is_some());
    m.handover.on_notice(None).await;
    assert!(m.handover.view().await.is_none());

    // Once the Hub has stopped, only "Cancel move" resumes it.
    m.handover.begin().await.unwrap();
    let code = code_of(&m).await;
    m.handover
        .claim(&claim_with(&code), FP, m.scratch.path())
        .await
        .unwrap();
    m.handover.on_notice(None).await;
    assert_eq!(
        m.handover.view().await.unwrap().phase,
        HandoverPhase::Transferring
    );
}

#[tokio::test]
async fn a_stopped_hub_refuses_through_the_intent_pipeline_before_any_check() {
    let f = bootstrapped().await;
    f.status.send_modify(|s| {
        s.handover = Some(arena_proto::HandoverView {
            phase: HandoverPhase::Moved,
            target_hub_name: PC.into(),
            code: None,
            last_seq: Some(1),
            file_name: None,
        });
    });
    let (code, _) = refusal(change(&f).await);
    assert_eq!(code, ErrorCode::HubHandingOver);
}
