use super::plans::group;
use super::stock::{intent, owner, staff, up};
use super::*;
use crate::rbac::ROLE_MANAGER;
use arena_proto::CommandWire;
use serde_json::{Value, json};
use std::collections::BTreeMap;

type Row = BTreeMap<String, RowValue>;

fn cmd(id: i64, kind: &str, payload: Value) -> CommandWire {
    CommandWire {
        id,
        idempotency_key: ulid::Ulid::generate().to_string(),
        kind: kind.into(),
        schema_version: 1,
        payload,
        issued_by: "owner-1".into(),
    }
}

fn int(row: &Row, k: &str) -> i64 {
    match &row[k] {
        RowValue::Integer(i) => *i,
        other => panic!("{k} is {other:?}"),
    }
}

fn text(row: &Row, k: &str) -> String {
    match &row[k] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{k} is {other:?}"),
    }
}

fn payload(kind: &str, mut v: Value) -> IntentPayload {
    v["kind"] = kind.into();
    serde_json::from_value(v).unwrap()
}

struct Case {
    kind: &'static str,
    table: Table,
    tbl: &'static str,
    id_column: &'static str,
    create: Value,
    field: &'static str,
    staff_value: Value,
    owner_value: Value,
}

async fn cases(f: &Fixture) -> Vec<Case> {
    let g = group(f, "PCs", "pc").await;
    vec![
        Case {
            kind: "plan.upsert",
            table: Table::Plans,
            tbl: "plans",
            id_column: "plan_id",
            create: json!({"plan_kind": "pack", "name": "5 h", "price_minor": 30000,
                "minutes": 300, "validity_days": 7, "group_ids": [g]}),
            field: "name",
            staff_value: json!("Staff pack"),
            owner_value: json!("Owner pack"),
        },
        Case {
            kind: "rule.upsert",
            table: Table::PricingRules,
            tbl: "pricing_rules",
            id_column: "rule_id",
            create: json!({"name": "Happy hour", "group_id": g, "rule_kind": "weekly",
                "days_mask": 31, "start_min": 960, "end_min": 1080,
                "adjust_kind": "percent_bp", "adjust_value": -2000}),
            field: "name",
            staff_value: json!("Staff rule"),
            owner_value: json!("Owner rule"),
        },
        Case {
            kind: "product.upsert",
            table: Table::Products,
            tbl: "products",
            id_column: "product_id",
            create: serde_json::to_value(up("Cola")).unwrap(),
            field: "name",
            staff_value: json!("Staff cola"),
            owner_value: json!("Owner cola"),
        },
        Case {
            kind: "coupon.upsert",
            table: Table::Coupons,
            tbl: "coupons",
            id_column: "coupon_id",
            create: json!({"code": "WELCOME", "percent_bp": 1000}),
            field: "percent_bp",
            staff_value: json!(2000),
            owner_value: json!(3000),
        },
    ]
}

fn row_of(f: &Fixture, c: &Case) -> Row {
    reopen(f).rows(c.table).unwrap().remove(0)
}

fn expect(v: &Value) -> RowValue {
    match v {
        Value::String(s) => RowValue::Text(s.clone()),
        Value::Number(n) => RowValue::Integer(n.as_i64().unwrap()),
        other => panic!("{other:?}"),
    }
}

fn override_of(f: &Fixture, tbl: &str, seq: i64) -> Option<arena_store::OwnerOverride> {
    let db = reopen(f);
    let change = db
        .changes()
        .unwrap()
        .into_iter()
        .find(|c| c.seq == seq && c.tbl == tbl)
        .unwrap();
    db.owner_override(&change.epoch, change.seq).unwrap()
}

fn row_seq(f: &Fixture, tbl: &str, id: &str, after: i64) -> i64 {
    reopen(f)
        .changes()
        .unwrap()
        .into_iter()
        .filter(|c| c.tbl == tbl && c.row_id == id && c.seq > after)
        .map(|c| c.seq)
        .max()
        .unwrap()
}

fn max_seq(f: &Fixture) -> i64 {
    reopen(f).changes().unwrap().last().map_or(0, |c| c.seq)
}

#[tokio::test]
async fn every_owner_kind_applies_as_the_owner_and_wins_over_a_staff_edit() {
    let f = bootstrapped().await;
    let manager = staff(&f, "asha", ROLE_MANAGER).await;
    for (n, c) in cases(&f).await.into_iter().enumerate() {
        let n = n as i64 * 10;
        let with = |extra: Value| {
            let mut v = c.create.clone();
            for (k, val) in extra.as_object().unwrap() {
                v[k] = val.clone();
            }
            v
        };
        let first = f
            .writer
            .apply_command(cmd(100 + n, c.kind, c.create.clone()))
            .await
            .unwrap();
        assert!(first.applied, "{}: {:?}", c.kind, first.error);
        let id = text(&row_of(&f, &c), c.id_column);
        let seq = row_seq(&f, c.tbl, &id, 0);
        assert!(
            override_of(&f, c.tbl, seq).is_none(),
            "{}: a create",
            c.kind
        );
        let created = reopen(&f)
            .changes()
            .unwrap()
            .into_iter()
            .find(|x| x.seq == seq)
            .unwrap();
        assert_eq!(
            (created.actor_kind.as_str(), created.actor_id.as_str()),
            ("owner", "owner-1")
        );

        let mut staff_intent_id = String::new();
        for _ in 0..2 {
            let rev = int(&row_of(&f, &c), "rev");
            let mut v = with(json!({"id": id}));
            v[c.field] = c.staff_value.clone();
            let mut i = intent(manager.clone(), payload(c.kind, v));
            i.base_rev = Some(rev);
            staff_intent_id = f.writer.submit_intent(i).await.unwrap().intent_id;
        }
        assert_eq!(int(&row_of(&f, &c), "rev"), 3, "{}", c.kind);

        let before = max_seq(&f);
        let mut body = with(json!({"id": id, "base_rev": 1}));
        body[c.field] = c.owner_value.clone();
        let won_cmd = cmd(101 + n, c.kind, body);
        let won = f.writer.apply_command(won_cmd.clone()).await.unwrap();
        assert!(won.applied, "{}: {:?}", c.kind, won.error);
        assert!(won.applied_seq.is_some());
        assert_eq!(
            row_of(&f, &c)[c.field],
            expect(&c.owner_value),
            "{}",
            c.kind
        );
        assert_eq!(int(&row_of(&f, &c), "rev"), 4, "{}", c.kind);

        let owner_seq = row_seq(&f, c.tbl, &id, before);
        let o = override_of(&f, c.tbl, owner_seq).expect("the override is stored");
        assert_eq!(o.intent_id, staff_intent_id, "{}", c.kind);
        assert_eq!(o.actor_kind, "staff", "{}", c.kind);
        assert_eq!((o.tbl.as_str(), o.row_id.as_str()), (c.tbl, id.as_str()));
        let changes = reopen(&f).changes().unwrap();
        let overwritten = changes
            .iter()
            .find(|x| x.intent_id == staff_intent_id && x.row_id == id)
            .expect("the audit keeps the overwritten change");
        assert_eq!(o.at, overwritten.at);

        let log = changes.len();
        let again = f.writer.apply_command(won_cmd).await.unwrap();
        assert_eq!(again.applied_seq, won.applied_seq);
        assert_eq!(
            reopen(&f).changes().unwrap().len(),
            log,
            "a repeat writes nothing"
        );

        let rev = int(&row_of(&f, &c), "rev");
        let before = max_seq(&f);
        let mut body = with(json!({"id": id, "base_rev": rev}));
        body[c.field] = c.staff_value.clone();
        let plain = f
            .writer
            .apply_command(cmd(102 + n, c.kind, body))
            .await
            .unwrap();
        assert!(plain.applied, "{}: {:?}", c.kind, plain.error);
        let seq = row_seq(&f, c.tbl, &id, before);
        assert!(
            override_of(&f, c.tbl, seq).is_none(),
            "{}: a correct base_rev",
            c.kind
        );

        let before = max_seq(&f);
        let mut body = with(json!({"id": id}));
        body[c.field] = c.owner_value.clone();
        let none = f
            .writer
            .apply_command(cmd(103 + n, c.kind, body))
            .await
            .unwrap();
        assert!(none.applied, "{}: {:?}", c.kind, none.error);
        let seq = row_seq(&f, c.tbl, &id, before);
        assert!(
            override_of(&f, c.tbl, seq).is_none(),
            "{}: no base_rev",
            c.kind
        );
    }
}

#[tokio::test]
async fn a_rate_retires_the_old_one_and_never_records_an_override() {
    let f = bootstrapped().await;
    let g = group(&f, "PCs", "pc").await;
    for (id, price) in [(1, 5000), (2, 6500)] {
        let row = f
            .writer
            .apply_command(cmd(
                id,
                "rate.upsert",
                json!({"group_id": g, "price_per_hour_minor": price, "base_rev": 0}),
            ))
            .await
            .unwrap();
        assert!(row.applied, "{:?}", row.error);
    }
    let rates = reopen(&f).rows(Table::Rates).unwrap();
    let active: Vec<_> = rates.iter().filter(|r| int(r, "active") == 1).collect();
    assert_eq!(rates.len(), 2);
    assert_eq!(int(active[0], "price_per_hour_minor"), 6500);
    let db = reopen(&f);
    for c in db.changes().unwrap().iter().filter(|c| c.tbl == "rates") {
        assert_eq!(db.owner_override(&c.epoch, c.seq).unwrap(), None);
        assert_eq!(c.actor_kind, "owner");
    }
}

#[tokio::test]
async fn an_invalid_inner_intent_is_rejected_for_every_kind_and_changes_nothing() {
    let f = bootstrapped().await;
    let g = group(&f, "Gone", "pc").await;
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupDelete(arena_proto::GroupDelete { id: g.clone() }),
        ))
        .await
        .unwrap();
    let bad = [
        (
            "plan.upsert",
            json!({"plan_kind": "pack", "name": "x", "price_minor": 100,
                "minutes": 60, "validity_days": 1, "group_ids": [g]}),
        ),
        (
            "rule.upsert",
            json!({"name": "x", "group_id": g, "rule_kind": "weekly", "days_mask": 31,
                "start_min": 0, "end_min": 60, "adjust_kind": "percent_bp",
                "adjust_value": -100}),
        ),
        (
            "rate.upsert",
            json!({"group_id": g, "price_per_hour_minor": 100}),
        ),
        (
            "coupon.upsert",
            json!({"code": "XYZ", "percent_bp": 1000, "id": "NOPE"}),
        ),
        ("product.upsert", {
            let mut v = serde_json::to_value(up("x")).unwrap();
            v["price_minor"] = (-5).into();
            v
        }),
    ];
    let before = reopen(&f).changes().unwrap().len();
    for (n, (kind, body)) in bad.into_iter().enumerate() {
        let c = cmd(200 + n as i64, kind, body);
        let row = f.writer.apply_command(c.clone()).await.unwrap();
        assert!(!row.applied && row.applied_seq.is_none(), "{kind}");
        let err: AppError = serde_json::from_str(row.error.as_deref().unwrap()).unwrap();
        assert_eq!(err.code, ErrorCode::ValidationFailed, "{kind}");
        let again = f.writer.apply_command(c).await.unwrap();
        assert!(!again.applied, "{kind}: not retried");
    }
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
}

#[tokio::test]
async fn other_kinds_and_newer_schemas_stay_unsupported_and_backup_now_does_not_run_here() {
    let f = bootstrapped().await;
    let before = reopen(&f).changes().unwrap().len();
    for (id, kind) in [
        (1, "rule.delete"),
        (2, "hub.revoke"),
        (3, "backup.restore"),
        (4, "backup.now"),
    ] {
        let row = f
            .writer
            .apply_command(cmd(id, kind, json!({})))
            .await
            .unwrap();
        let err: AppError = serde_json::from_str(row.error.as_deref().unwrap()).unwrap();
        assert_eq!(
            (row.applied, err.code),
            (false, ErrorCode::Unsupported),
            "{kind}"
        );
    }
    let mut newer = cmd(
        5,
        "plan.upsert",
        json!({"plan_kind": "pack", "name": "x", "price_minor": 1, "group_ids": []}),
    );
    newer.schema_version = 2;
    let row = f.writer.apply_command(newer).await.unwrap();
    let err: AppError = serde_json::from_str(row.error.as_deref().unwrap()).unwrap();
    assert_eq!(err.code, ErrorCode::Unsupported);
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
}

#[tokio::test]
async fn a_command_answered_outside_apply_is_recorded_once_with_no_change_row() {
    let f = bootstrapped().await;
    let before = reopen(&f).changes().unwrap().len();
    let c = cmd(9, "backup.now", json!({}));
    let row = f.writer.record_command(c.clone(), None).await.unwrap();
    assert!(row.applied && row.applied_seq.is_none() && row.error.is_none());
    let again = f
        .writer
        .record_command(c, Some(AppError::new(ErrorCode::CloudUnreachable, "no")))
        .await
        .unwrap();
    assert!(again.applied, "the first answer stands");
    let found = f
        .writer
        .recorded_command(9, "other-key".into())
        .await
        .unwrap();
    assert_eq!(found.map(|r| r.command_id), Some(9));
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    let failed = f
        .writer
        .record_command(
            cmd(10, "backup.now", json!({})),
            Some(AppError::new(ErrorCode::CloudUnreachable, "no")),
        )
        .await
        .unwrap();
    assert!(!failed.applied && failed.applied_seq.is_none());
}

#[tokio::test]
async fn an_applied_rate_rule_and_plan_reach_the_floor_and_the_attract_view_within_a_minute() {
    use super::prepaid::{group_of, ist};
    use super::session::pc;
    use crate::attract::{GameMediaHashes, attract_views};
    use arena_proto::DeviceKind;

    fn none(_: &str) -> GameMediaHashes {
        GameMediaHashes::default()
    }
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(12_000)).await;
    let g = group_of(&f, &d);
    f.clock.set(ist("2027-01-15 16:30"));
    let applied_at = f.clock.now_ms();
    let floor = |now: i64| {
        crate::snapshot::read_snapshot(&reopen(&f), now)
            .unwrap()
            .groups
            .into_iter()
            .find(|x| x.id == g)
            .unwrap()
    };
    let attract = |now: i64| {
        attract_views(&reopen(&f), std::slice::from_ref(&d), now, &none)
            .unwrap()
            .remove(0)
            .1
    };
    assert_eq!(floor(applied_at).rate_minor, Some(12_000));

    let rate = f
        .writer
        .apply_command(cmd(
            1,
            "rate.upsert",
            json!({"group_id": g, "price_per_hour_minor": 15_000}),
        ))
        .await
        .unwrap();
    assert!(rate.applied, "{:?}", rate.error);
    f.clock.advance(59_000);
    let now = f.clock.now_ms();
    assert!(now - applied_at < 60_000);
    assert_eq!(floor(now).rate_minor, Some(15_000));
    assert_eq!(attract(now).rate_minor, Some(15_000));

    let rule = f
        .writer
        .apply_command(cmd(
            2,
            "rule.upsert",
            json!({"name": "Happy hour", "group_id": g, "rule_kind": "weekly",
                "days_mask": 31, "start_min": 960, "end_min": 1080,
                "adjust_kind": "percent_bp", "adjust_value": -2000}),
        ))
        .await
        .unwrap();
    assert!(rule.applied, "{:?}", rule.error);
    let applied_at = f.clock.now_ms();
    f.clock.advance(59_000);
    let now = f.clock.now_ms();
    assert!(now - applied_at < 60_000);
    let tile = floor(now);
    let active = tile.active_rule.expect("the rule shows on the floor tile");
    assert_eq!(
        (active.name.as_str(), active.rate_minor),
        ("Happy hour", 12_000)
    );
    let view = attract(now);
    assert_eq!(view.rate_minor, Some(12_000));
    assert_eq!(view.rule.map(|r| r.name), Some("Happy hour".to_owned()));

    let plan = f
        .writer
        .apply_command(cmd(
            3,
            "plan.upsert",
            json!({"plan_kind": "pack", "name": "5 h", "price_minor": 30000,
                "minutes": 300, "validity_days": 7, "group_ids": [g]}),
        ))
        .await
        .unwrap();
    assert!(plan.applied, "{:?}", plan.error);
    let plans = reopen(&f).plans().unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].1, vec![g]);
}
