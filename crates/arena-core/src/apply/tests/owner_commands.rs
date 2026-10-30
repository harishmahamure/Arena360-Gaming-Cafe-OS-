use super::plans::group;
use super::stock::{intent, make, owner, staff, up};
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
        elevated_by: None,
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

/// The code of the error a refused command stored.
fn refusal(c: &arena_store::AppliedCommand) -> Option<ErrorCode> {
    let e: AppError = serde_json::from_str(c.error.as_deref()?).ok()?;
    Some(e.code)
}

fn rows_of(f: &Fixture, table: Table) -> Vec<Row> {
    reopen(f).rows(table).unwrap()
}

fn named(rows: Vec<Row>, key: &str, value: &str) -> Row {
    rows.into_iter()
        .find(|r| r[key] == RowValue::Text(value.into()))
        .unwrap_or_else(|| panic!("no row with {key} = {value}"))
}

#[tokio::test]
async fn the_owner_adds_edits_and_deactivates_staff_and_edits_a_custom_role() {
    let f = bootstrapped().await;
    let id = ulid::Ulid::generate().to_string();
    let hash = crate::staff::hash_secret("K7Q49MZD").unwrap();
    let mut new_member = json!({
        "staff_id": id, "login": "+919876500001", "display_name": "Neha",
        "role_id": "Counter", "active": true,
        "setup_code_hash": hash, "setup_expires_at": T0 + 72 * 3_600_000,
    });
    let added = f
        .writer
        .apply_command(cmd(1, "staff.upsert", new_member.clone()))
        .await
        .unwrap();
    assert!(added.applied, "{:?}", added.error);
    let row = named(rows_of(&f, Table::Staff), "staff_id", &id);
    assert_eq!(
        (int(&row, "active"), text(&row, "login")),
        (1, "+919876500001".into())
    );
    assert_eq!(
        row["password_hash"],
        RowValue::Null,
        "the owner never sets a password"
    );
    assert_eq!(text(&row, "setup_code_hash"), hash);

    // An edit that names no credential keeps the member's own; owner_wins fills the base_rev.
    new_member["role_id"] = "Manager".into();
    new_member["display_name"] = "Neha R.".into();
    new_member
        .as_object_mut()
        .unwrap()
        .remove("setup_code_hash");
    new_member
        .as_object_mut()
        .unwrap()
        .remove("setup_expires_at");
    let edited = f
        .writer
        .apply_command(cmd(2, "staff.upsert", new_member))
        .await
        .unwrap();
    assert!(edited.applied, "{:?}", edited.error);
    let row = named(rows_of(&f, Table::Staff), "staff_id", &id);
    let manager = named(rows_of(&f, Table::Roles), "name", ROLE_MANAGER);
    assert_eq!(text(&row, "role_id"), text(&manager, "role_id"));
    assert_eq!(text(&row, "display_name"), "Neha R.");

    let gone = f
        .writer
        .apply_command(cmd(3, "staff.deactivate", json!({"staff_id": id})))
        .await
        .unwrap();
    assert!(gone.applied, "{:?}", gone.error);
    assert_eq!(
        int(&named(rows_of(&f, Table::Staff), "staff_id", &id), "active"),
        0
    );

    // A custom role is created, then edited with the rev the owner read.
    let made = f
        .writer
        .apply_command(cmd(
            4,
            "role.upsert",
            json!({"name": "Floor lead", "perms": [{"permission": "session.start", "effect": "allow"}]}),
        ))
        .await
        .unwrap();
    assert!(made.applied, "{:?}", made.error);
    let role = named(rows_of(&f, Table::Roles), "name", "Floor lead");
    let edited = f
        .writer
        .apply_command(cmd(
            5,
            "role.upsert",
            json!({"role_id": text(&role, "role_id"), "base_rev": int(&role, "rev"), "name": "Floor lead",
                "perms": [{"permission": "session.start", "effect": "allow"},
                          {"permission": "session.end", "effect": "allow"}]}),
        ))
        .await
        .unwrap();
    assert!(edited.applied, "{:?}", edited.error);
    let perms = reopen(&f)
        .rows(Table::RolePerms)
        .unwrap()
        .into_iter()
        .filter(|p| p["role_id"] == role["role_id"])
        .count();
    assert_eq!(perms, 2);
    // Each change is the owner's, and owner changes toast on the Hub.
    let changes = reopen(&f).changes().unwrap();
    for tbl in ["staff", "roles"] {
        let last = changes.iter().rfind(|c| c.tbl == tbl).unwrap();
        assert_eq!(last.actor_kind, "owner", "{tbl}");
        assert!(arena_proto::owner_update_for_table(tbl).is_some());
    }
}

#[tokio::test]
async fn the_owner_cannot_deactivate_or_demote_the_last_active_owner_and_cannot_edit_a_built_in_role()
 {
    let f = bootstrapped().await;
    let boss = staff(&f, "boss", crate::rbac::ROLE_OWNER).await;
    let boss_row = named(rows_of(&f, Table::Staff), "login", "boss");
    assert_eq!(text(&boss_row, "staff_id"), boss.id);

    let r = f
        .writer
        .apply_command(cmd(1, "staff.deactivate", json!({"staff_id": boss.id})))
        .await
        .unwrap();
    assert!(!r.applied);
    assert_eq!(
        refusal(&r),
        Some(ErrorCode::ValidationFailed),
        "{:?}",
        r.error
    );
    let demote = f
        .writer
        .apply_command(cmd(
            2,
            "staff.upsert",
            json!({"staff_id": boss.id, "login": "boss", "display_name": "boss",
                "role_id": "Counter", "active": true}),
        ))
        .await
        .unwrap();
    assert!(!demote.applied, "{:?}", demote.error);
    assert_eq!(
        int(&named(rows_of(&f, Table::Staff), "login", "boss"), "active"),
        1
    );

    let owner_role = named(rows_of(&f, Table::Roles), "name", crate::rbac::ROLE_OWNER);
    let builtin = f
        .writer
        .apply_command(cmd(
            3,
            "role.upsert",
            json!({"role_id": text(&owner_role, "role_id"), "base_rev": int(&owner_role, "rev"),
                "name": "Owner", "perms": []}),
        ))
        .await
        .unwrap();
    assert!(!builtin.applied);
    assert_eq!(refusal(&builtin), Some(ErrorCode::PermissionDenied));
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

/// Story 8.24: the Owner app sends no image (the summary holds none), so an edit keeps the stored
/// one; a given image replaces it, and `stock.threshold` is an owner command.
#[tokio::test]
async fn an_owner_product_edit_without_an_image_keeps_it_and_sets_the_threshold_by_command() {
    let f = bootstrapped().await;
    let image = &super::shop::jpeg();
    let id = make(
        &f,
        arena_proto::ProductUpsert {
            image: image.clone(),
            ..up("Cola")
        },
    )
    .await;
    let stored = |f: &Fixture| text(&reopen(f).rows(Table::Products).unwrap().remove(0), "image");
    let mut edit = json!({"id": id, "name": "Cola 500", "category": "", "price_minor": 100,
        "hsn": "", "barcode": "", "kitchen": false, "favourite": false});
    let applied = f
        .writer
        .apply_command(cmd(1, "product.upsert", edit.clone()))
        .await
        .unwrap();
    assert!(applied.applied, "{:?}", applied.error);
    assert_eq!(stored(&f), *image);

    edit["name"] = "Cola 600".into();
    edit["image"] = "".into();
    let cleared = f
        .writer
        .apply_command(cmd(2, "product.upsert", edit))
        .await
        .unwrap();
    assert!(cleared.applied, "{:?}", cleared.error);
    assert_eq!(stored(&f), "", "an empty image clears it");

    let set = f
        .writer
        .apply_command(cmd(
            3,
            "stock.threshold",
            json!({"product_id": id, "low_stock_qty": 6}),
        ))
        .await
        .unwrap();
    assert!(set.applied, "{:?}", set.error);
    assert_eq!(
        reopen(&f).tracked_thresholds().unwrap()[0].low_stock_qty,
        Some(6)
    );
}

/// Story 8.25: a command the cloud posted after a step-up lands in the change row as an MFA
/// elevation of the owner; the Hub accepts it only for the owner it names.
#[tokio::test]
async fn a_stepped_up_owner_command_records_the_mfa_elevation_on_the_change_row() {
    let f = bootstrapped().await;
    let create = json!({"code": "WELCOME", "percent_bp": 1000});

    let plain = f
        .writer
        .apply_command(cmd(1, "coupon.upsert", create.clone()))
        .await
        .unwrap();
    assert!(plain.applied, "{:?}", plain.error);
    let change = |f: &Fixture, seq: i64| {
        reopen(f)
            .changes()
            .unwrap()
            .into_iter()
            .find(|c| c.seq == seq)
            .unwrap()
    };
    let first = change(&f, plain.applied_seq.unwrap());
    assert_eq!(first.elevated_by_kind, None);

    let mut elevated = cmd(
        2,
        "coupon.upsert",
        json!({"code": "AGAIN", "percent_bp": 500}),
    );
    elevated.elevated_by = Some("owner-1".into());
    let done = f.writer.apply_command(elevated).await.unwrap();
    assert!(done.applied, "{:?}", done.error);
    let c = change(&f, done.applied_seq.unwrap());
    assert_eq!(
        (
            c.elevated_by_kind.as_deref(),
            c.elevated_by_id.as_deref(),
            c.elevated_by_mode.as_deref(),
            c.elevated_by_verifier_version
        ),
        (Some("mfa"), Some("owner-1"), Some("hub"), Some(0))
    );
    assert_eq!(
        (c.actor_kind.as_str(), c.actor_id.as_str()),
        ("owner", "owner-1")
    );

    let mut someone_else = cmd(
        3,
        "coupon.upsert",
        json!({"code": "OTHER", "percent_bp": 500}),
    );
    someone_else.elevated_by = Some("owner-2".into());
    let refused = f.writer.apply_command(someone_else).await.unwrap();
    assert!(!refused.applied);
    let err: AppError = serde_json::from_str(refused.error.as_deref().unwrap()).unwrap();
    assert_eq!(err.code, ErrorCode::PermissionDenied);
}
