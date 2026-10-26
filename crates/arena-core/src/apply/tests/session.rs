use super::players::{add, form};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::{
    ClientClass, DeviceAssign, DeviceKind, DeviceMaintenance, DevicePower, DeviceRegister,
    GroupUpsert, PcAction, RateUpsert, SessionMode, SessionStart,
};

fn text(row: &Row, col: &str) -> String {
    match &row[col] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{col}: {other:?}"),
    }
}

fn int(row: &Row, col: &str) -> i64 {
    match &row[col] {
        RowValue::Integer(n) => *n,
        other => panic!("{col}: {other:?}"),
    }
}

async fn submit(f: &Fixture, actor: Actor, p: IntentPayload) -> Result<Applied, AppError> {
    let mut i = intent(actor, p);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn pc(f: &Fixture, kind: DeviceKind, group_kind: &str, rate: Option<i64>) -> String {
    let mut d = DeviceRegister::new(
        ClientClass::Paired,
        "PC",
        crate::devices::hash_token_secret("s").unwrap(),
    );
    d.device_kind = kind;
    let id = submit(f, owner(), IntentPayload::DeviceRegister(d))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    let g = submit(
        f,
        owner(),
        IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: format!("G{id}"),
            group_kind: group_kind.into(),
            colour: "teal".into(),
            sort_order: 1,
        }),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone();
    if let Some(price) = rate {
        submit(
            f,
            owner(),
            IntentPayload::RateUpsert(RateUpsert {
                group_id: g.clone(),
                price_per_hour_minor: price,
            }),
        )
        .await
        .unwrap();
    }
    submit(
        f,
        owner(),
        IntentPayload::DeviceAssign(DeviceAssign {
            device_id: id.clone(),
            group_id: Some(g),
        }),
    )
    .await
    .unwrap();
    id
}

fn start_of(device: &str, player: Option<&str>) -> IntentPayload {
    IntentPayload::SessionStart(SessionStart {
        device_id: device.into(),
        mode: SessionMode::Postpaid,
        player_id: player.map(Into::into),
    })
}

async fn start(f: &Fixture, device: &str, player: Option<&str>) -> Result<Applied, AppError> {
    submit(f, owner(), start_of(device, player)).await
}

fn refusal(r: Result<Applied, AppError>) -> (ErrorCode, Option<String>) {
    let e = r.unwrap_err();
    (e.code, e.field)
}

fn sessions(f: &Fixture) -> Vec<Row> {
    reopen(f).rows(Table::Sessions).unwrap()
}

fn state_of(f: &Fixture, device: &str) -> String {
    reopen(f)
        .rows(Table::DevicesMeta)
        .unwrap()
        .iter()
        .find(|r| text(r, "device_id") == device)
        .map(|r| text(r, "state"))
        .unwrap()
}

#[tokio::test]
async fn a_player_session_writes_session_segment_and_flips_the_device() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    let (player, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let a = start(&f, &d, Some(&player)).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["sessions", "session_segments", "devices_meta"]
    );
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "player_id"), player);
    assert_eq!(s["guest_no"], RowValue::Null);
    assert_eq!(text(s, "device_id"), d);
    assert_eq!(text(s, "mode"), "postpaid");
    assert_eq!(text(s, "state"), "active");
    assert_eq!(int(s, "started_at"), T0);
    assert_eq!(int(s, "rev"), 1);
    assert_eq!(
        (
            int(s, "rounding_min"),
            int(s, "minimum_min"),
            int(s, "grace_min"),
            text(s, "time_up_mode")
        ),
        (15, 30, 2, "clean_up".into())
    );
    assert_eq!(text(s, "business_day"), "2027-01-15");
    assert!(!text(s, "fy").is_empty() && !text(s, "epoch").is_empty());
    let seg = &reopen(&f).rows(Table::SessionSegments).unwrap()[0];
    assert_eq!(text(seg, "session_id"), text(s, "id"));
    assert_eq!(int(seg, "from_at"), T0);
    assert_eq!(seg["to_at"], RowValue::Null);
    assert_eq!(int(seg, "rate_minor_per_hour"), 12_000);
    assert_eq!(seg["rule_id"], RowValue::Null);
    assert_eq!(state_of(&f, &d), "in_session");

    let log: Vec<u8> = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .filter(|c| c.tbl == "sessions")
        .flat_map(|c| c.row)
        .collect();
    let log = String::from_utf8_lossy(&log).into_owned();
    assert!(!log.contains("Asha") && !log.contains("9876543210") && !log.contains("asha"));
    let snap = crate::snapshot::read_snapshot(&reopen(&f), T0).unwrap();
    assert_eq!(snap.sessions.len(), 1);
    assert_eq!(snap.sessions[0].username.as_deref(), Some("asha"));
    assert_eq!(snap.sessions[0].guest_no, None);
}

#[tokio::test]
async fn guests_number_per_business_day() {
    let f = bootstrapped().await;
    let a = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let b = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    start(&f, &a, None).await.unwrap();
    start(&f, &b, None).await.unwrap();
    let nos: Vec<i64> = sessions(&f).iter().map(|s| int(s, "guest_no")).collect();
    assert_eq!(nos, [1, 2]);
    f.clock.advance(24 * 3_600_000);
    f.writer
        .submit(|tx| {
            tx.execute_batch(
                "UPDATE sessions SET state = 'ended'; UPDATE devices_meta SET state = 'online'",
            )
            .map_err(arena_store::db_error)
        })
        .await
        .unwrap();
    start(&f, &a, None).await.unwrap();
    let last = sessions(&f).pop().unwrap();
    assert_eq!(
        (text(&last, "business_day"), int(&last, "guest_no")),
        ("2027-01-16".into(), 1)
    );
}

#[tokio::test]
async fn refusals_name_the_code_and_write_nothing() {
    let f = bootstrapped().await;
    let ok = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let norate = pc(&f, DeviceKind::Pc, "pc", None).await;
    let console = pc(&f, DeviceKind::Console, "console", Some(100)).await;
    let loose = {
        let d = DeviceRegister::new(
            ClientClass::Paired,
            "Loose",
            crate::devices::hash_token_secret("s").unwrap(),
        );
        submit(&f, owner(), IntentPayload::DeviceRegister(d))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    };
    let maint = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    submit(
        &f,
        owner(),
        IntentPayload::DeviceMaintenance(DeviceMaintenance {
            device_id: maint.clone(),
            on: true,
            note: None,
        }),
    )
    .await
    .unwrap();
    let kitchen = staff(&f, "cook", ROLE_KITCHEN).await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let before = reopen(&f).changes().unwrap().len();
    let field = |c, f: &str| (c, Some(f.to_owned()));
    assert_eq!(
        refusal(start(&f, "nope", None).await),
        field(ErrorCode::NotFound, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &console, None).await),
        field(ErrorCode::ValidationFailed, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &loose, None).await),
        field(ErrorCode::DeviceUnassigned, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &maint, None).await),
        field(ErrorCode::PcUnavailable, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &norate, None).await),
        field(ErrorCode::NoRate, "device_id")
    );
    assert_eq!(
        refusal(start(&f, &ok, Some("01JNOSUCHPLAYER")).await),
        field(ErrorCode::NotFound, "player_id")
    );
    let pack = IntentPayload::SessionStart(SessionStart {
        device_id: ok.clone(),
        mode: SessionMode::Pack,
        player_id: None,
    });
    assert_eq!(
        refusal(submit(&f, owner(), pack).await),
        field(ErrorCode::ValidationFailed, "mode")
    );
    let mut with_rev = intent(owner(), start_of(&ok, None));
    with_rev.base_rev = Some(1);
    assert_eq!(
        refusal(f.writer.submit_intent(with_rev).await),
        field(ErrorCode::ValidationFailed, "base_rev")
    );
    assert_eq!(
        refusal(submit(&f, kitchen, start_of(&ok, None)).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert!(sessions(&f).is_empty());
    submit(&f, counter, start_of(&ok, None)).await.unwrap();
}

#[tokio::test]
async fn a_busy_pc_is_refused_and_power_commands_are_blocked() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    start(&f, &d, None).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(start(&f, &d, None).await),
        (ErrorCode::PcBusy, Some("device_id".into()))
    );
    for action in [PcAction::Restart, PcAction::Shutdown] {
        let power = IntentPayload::DevicePower(DevicePower {
            device_id: d.clone(),
            action,
        });
        assert_eq!(
            refusal(submit(&f, owner(), power).await).0,
            ErrorCode::PcBusy
        );
    }
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert_eq!(sessions(&f).len(), 1);
}

#[tokio::test]
async fn a_retried_key_returns_the_original_outcome_and_one_session() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let i = intent(owner(), start_of(&d, None));
    let first = f.writer.submit_intent(i.clone()).await.unwrap();
    let again = f.writer.submit_intent(i).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(sessions(&f).len(), 1);
}

#[tokio::test]
async fn the_database_allows_one_live_session_per_device() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let g = text(
        &reopen(&f).rows(Table::DeviceGroups).unwrap()[0],
        "group_id",
    );
    let sql = move |id: &str, state: &str, guest: i64| {
        format!(
            "INSERT INTO sessions (id, device_id, group_id, mode, state, started_at, rounding_min,
                 minimum_min, grace_min, time_up_mode, epoch, business_day, fy, guest_no, rev,
                 created_at, updated_at)
             VALUES ('{id}', '{d}', '{g}', 'postpaid', '{state}', 1, 15, 30, 2, 'clean_up', 'e',
                     'd', 'fy', {guest}, 0, 0, 0)"
        )
    };
    f.writer
        .submit(move |tx| {
            let run = |q: String| tx.execute_batch(&q);
            run(sql("A", "ended", 1)).unwrap();
            run(sql("B", "paused", 2)).unwrap();
            assert!(run(sql("C", "active", 3)).is_err());
            assert!(run(sql("D", "ended", 2)).is_err());
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn two_simultaneous_starts_one_wins_and_the_other_is_busy() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(100)).await;
    let (a, b) = tokio::join!(start(&f, &d, None), start(&f, &d, None));
    let mut codes: Vec<_> = [a, b].into_iter().map(|r| r.map_err(|e| e.code)).collect();
    codes.sort_by_key(|r| r.is_err());
    assert!(codes[0].is_ok());
    assert_eq!(codes[1], Err(ErrorCode::PcBusy));
    assert_eq!(sessions(&f).len(), 1);
}
