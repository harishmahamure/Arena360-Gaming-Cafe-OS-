//! Story 9.21: the promote steps, with a scripted cloud.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::{Clock, HubStatus, ManualClock, Role};
use arena_proto::{AppError, ErrorCode, FenceClaimRequest};
use arena_store::{
    ChangeOp, ChangeRecord, META_HUB_INSTANCE_ID, Store, append_replicated, set_meta,
};
use tokio::sync::watch;

use crate::cloud::CloudFailure;
use crate::fence::Claim;
use crate::promote::{EpochClaimer, Promoter};
use crate::standby_progress::StandbyProgress;

const T0: i64 = 1_800_000_000_000;

/// Answers each claim from a script; `Hang` never answers.
enum Answer {
    Say(Result<Claim, CloudFailure>),
    Hang,
}

struct Scripted {
    answers: Mutex<VecDeque<Answer>>,
    seen: Mutex<Vec<FenceClaimRequest>>,
}

impl Scripted {
    fn new(answers: Vec<Answer>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into()),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<FenceClaimRequest> {
        self.seen.lock().unwrap().clone()
    }
}

impl EpochClaimer for Arc<Scripted> {
    async fn claim(&self, req: &FenceClaimRequest) -> Result<Claim, CloudFailure> {
        self.seen.lock().unwrap().push(req.clone());
        let next = self.answers.lock().unwrap().pop_front();
        match next {
            Some(Answer::Say(r)) => r,
            Some(Answer::Hang) | None => std::future::pending().await,
        }
    }
}

fn won(epoch: &str) -> Answer {
    Answer::Say(Ok(Claim::Won {
        epoch: epoch.into(),
    }))
}

fn fenced(current: Option<&str>) -> Answer {
    Answer::Say(Ok(Claim::Fenced {
        current: current.map(str::to_owned),
    }))
}

fn refused(status: u16, code: ErrorCode) -> Answer {
    Answer::Say(Err(CloudFailure::Refused {
        status,
        error: AppError::new(code, "no"),
        retry_after_s: None,
    }))
}

struct Fixture {
    _dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    store: Arc<Mutex<Store>>,
    progress: Arc<Mutex<StandbyProgress>>,
    stop: watch::Sender<bool>,
    stopped: watch::Receiver<bool>,
}

fn change(epoch: &str, seq: i64) -> ChangeRecord {
    ChangeRecord {
        epoch: epoch.into(),
        seq,
        tbl: "settings".into(),
        row_id: format!("b/k{seq}"),
        op: ChangeOp::Insert,
        row: vec![0x80],
        intent_id: format!("i{seq}"),
        intent_kind: "settings.set".into(),
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

/// A standby that followed epoch `3:MAIN` to row 7 and last heard from the main Hub `quiet_ms`
/// ago, 2 rows short of the main Hub's log.
fn fixture(quiet_ms: i64) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("standby.db"), 1).unwrap();
    store
        .write_tx(|tx| {
            set_meta(tx, META_HUB_INSTANCE_ID, "STBY")?;
            let rows: Vec<ChangeRecord> = (1..=7).map(|n| change("3:MAIN", n)).collect();
            append_replicated(tx, &rows)
        })
        .unwrap();
    let schema = store.schema_version().unwrap();
    let clock = Arc::new(ManualClock::new(T0));
    let mut progress = StandbyProgress::new(T0 - 600_000, schema);
    progress.start_at(Some(("3:MAIN".into(), 7)), T0 - 600_000);
    progress.ready(Some(("3:MAIN".into(), 9)), T0 - quiet_ms);
    let (stop, stopped) = watch::channel(false);
    Fixture {
        _dir: dir,
        clock,
        store: Arc::new(Mutex::new(store)),
        progress: Arc::new(Mutex::new(progress)),
        stop,
        stopped,
    }
}

fn promoter(f: &Fixture, claimer: &Arc<Scripted>) -> Promoter<Arc<Scripted>> {
    let clock: Arc<dyn Clock> = f.clock.clone();
    Promoter::new(
        Arc::clone(&f.store),
        Arc::clone(&f.progress),
        clock,
        Arc::clone(claimer),
        f.stop.clone(),
    )
    .with_claim_wait(Duration::from_millis(80))
}

fn lease_held(f: &Fixture) -> bool {
    f.store.lock().unwrap().lease().unwrap().is_some()
}

#[tokio::test]
async fn a_fresh_lease_refuses_and_changes_nothing() {
    let f = fixture(14_000);
    let cloud = Scripted::new(vec![won("4:STBY")]);
    let err = promoter(&f, &cloud).promote().await.unwrap_err();
    assert_eq!(err.error.code, ErrorCode::LeaseFresh);
    assert_eq!(err.error.message, arena_proto::LEASE_FRESH_MESSAGE);
    assert!(!err.follower_stopped);
    assert!(!*f.stopped.borrow());
    assert!(cloud.asked().is_empty());
    assert!(!lease_held(&f));
}

#[tokio::test]
async fn an_expired_lease_promotes_on_the_epoch_the_cloud_issues() {
    let f = fixture(15_000);
    let cloud = Scripted::new(vec![won("4:STBY")]);
    let got = promoter(&f, &cloud).promote().await.unwrap();
    assert_eq!(got.outcome.new_epoch, "4:STBY");
    assert!(!got.outcome.provisional);
    assert_eq!(got.lag_rows, 2);
    assert!(*f.stopped.borrow(), "the follower was stopped");

    // The claim named the old main's epoch and the fork.
    let asked = cloud.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        (asked[0].expected.as_str(), asked[0].instance_id.as_str()),
        ("3:MAIN", "STBY")
    );
    assert_eq!(
        (
            asked[0].fork.parent_epoch.as_str(),
            asked[0].fork.parent_seq
        ),
        ("3:MAIN", 7)
    );
    assert_eq!(asked[0].reason, arena_proto::FenceClaimReason::Promote);

    // The audit record carries both epochs and the lag.
    let record = got.record();
    assert_eq!(
        (
            record.old_epoch.as_str(),
            record.new_epoch.as_str(),
            record.fork_seq,
            record.lag_rows
        ),
        ("3:MAIN", "4:STBY", 7, 2)
    );
    assert!(record.is_consistent());

    // The status says this Hub is the primary writer of the new epoch.
    let (status, rx) = arena_core::status_channel(HubStatus::opened(1, "STBY"));
    got.publish("STBY", &status);
    let s = rx.borrow();
    assert_eq!(s.role, Role::Primary);
    assert_eq!(s.lease.as_ref().unwrap().epoch().to_string(), "4:STBY");
    assert!(s.following.is_none());
}

#[tokio::test]
async fn no_answer_in_three_seconds_promotes_on_a_provisional_epoch() {
    let f = fixture(60_000);
    let cloud = Scripted::new(vec![Answer::Hang]);
    let got = promoter(&f, &cloud).promote().await.unwrap();
    assert!(got.outcome.provisional);
    assert_eq!(got.outcome.new_epoch, "4:STBY");
    assert!(
        f.store
            .lock()
            .unwrap()
            .lease()
            .unwrap()
            .unwrap()
            .provisional
    );

    // An unreachable cloud and a cloud that errors are the same: promote anyway.
    for answer in [
        Answer::Say(Err(CloudFailure::Unreachable("down".into()))),
        refused(503, ErrorCode::Internal),
    ] {
        let f = fixture(60_000);
        let cloud = Scripted::new(vec![answer]);
        assert!(
            promoter(&f, &cloud)
                .promote()
                .await
                .unwrap()
                .outcome
                .provisional
        );
    }
}

#[tokio::test]
async fn a_fence_the_standby_did_not_expect_is_tried_once_more_and_then_gives_up() {
    // The cloud's fence moved on without any rows reaching the standby: ask again from there.
    let f = fixture(60_000);
    let cloud = Scripted::new(vec![fenced(Some("4:MAIN")), won("5:STBY")]);
    let got = promoter(&f, &cloud).promote().await.unwrap();
    assert_eq!(got.outcome.new_epoch, "5:STBY");
    let asked = cloud.asked();
    assert_eq!(
        asked
            .iter()
            .map(|r| r.expected.as_str())
            .collect::<Vec<_>>(),
        ["3:MAIN", "4:MAIN"]
    );

    // Fenced again, or fenced with no fence named: a newer Hub holds the branch.
    for script in [
        vec![fenced(Some("4:MAIN")), fenced(Some("5:OTHER"))],
        vec![fenced(None)],
        vec![fenced(Some("3:MAIN"))],
    ] {
        let f = fixture(60_000);
        let err = promoter(&f, &Scripted::new(script))
            .promote()
            .await
            .unwrap_err();
        assert_eq!(err.error.code, ErrorCode::Fenced);
        assert!(err.follower_stopped, "the caller starts the follower again");
        assert!(!lease_held(&f), "nothing was written");
    }
}

#[tokio::test]
async fn a_definite_no_from_the_cloud_is_reported() {
    let f = fixture(60_000);
    let cloud = Scripted::new(vec![refused(403, ErrorCode::Forbidden)]);
    let err = promoter(&f, &cloud).promote().await.unwrap_err();
    assert_eq!(err.error.code, ErrorCode::Forbidden);
    assert!(err.follower_stopped);
    assert!(!lease_held(&f));
}

#[tokio::test]
async fn a_standby_behind_the_main_hubs_schema_is_refused_before_anything_stops() {
    let f = fixture(60_000);
    {
        let mut s = f.store.lock().unwrap();
        let ours = s.schema_version().unwrap();
        s.record_main_schema(ours + 1, false).unwrap();
    }
    let cloud = Scripted::new(vec![won("4:STBY")]);
    let err = promoter(&f, &cloud).promote().await.unwrap_err();
    assert_eq!(err.error.code, ErrorCode::SchemaBehind);
    assert!(!err.follower_stopped);
    assert!(!*f.stopped.borrow());
    assert!(cloud.asked().is_empty());
    assert!(!lease_held(&f));
}

#[tokio::test]
async fn a_standby_that_copied_nothing_promotes_without_a_fence_to_move_from() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("standby.db"), 1).unwrap();
    store
        .write_tx(|tx| set_meta(tx, META_HUB_INSTANCE_ID, "STBY"))
        .unwrap();
    let schema = store.schema_version().unwrap();
    let (stop, _stopped) = watch::channel(false);
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(T0));
    let cloud = Scripted::new(vec![won("1:STBY")]);
    let promoter = Promoter::new(
        Arc::new(Mutex::new(store)),
        Arc::new(Mutex::new(StandbyProgress::new(T0 - 60_000, schema))),
        clock,
        Arc::clone(&cloud),
        stop,
    );
    let got = promoter.promote().await.unwrap();
    assert!(got.outcome.provisional);
    assert_eq!(got.outcome.old_epoch, "");
    assert!(cloud.asked().is_empty());
}
