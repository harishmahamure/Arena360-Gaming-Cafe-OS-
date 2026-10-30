//! Story 8.4: `summary.publish` writes only `rollup_log`, and the summary the Hub builds.

use super::stock::{intent, make, owner, set_threshold, up};
use super::*;
use crate::snapshot::SnapshotReader;
use arena_proto::{
    ATTENTION_ALERT, ATTENTION_LOW_STOCK, AlertKind, AlertRaise, SUMMARY_LOG_DATE, SUMMARY_SCHEMA,
    SummaryPublish,
};

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "hub-summary".into(),
    }
}

fn publish(built_at: i64, key: &str) -> IntentPayload {
    IntentPayload::SummaryPublish(SummaryPublish {
        schema: SUMMARY_SCHEMA,
        built_at,
        key: key.into(),
    })
}

async fn booted() -> Fixture {
    let f = primary();
    f.writer.submit_intent(bootstrap()).await.unwrap();
    f
}

#[tokio::test]
async fn a_publish_writes_only_the_latest_rollup_log_row() {
    let f = booted().await;
    let first = f
        .writer
        .submit_intent(intent(system(), publish(T0, "state/latest.json")))
        .await
        .unwrap();
    assert_eq!(first.changes.len(), 1);
    let c = &first.changes[0];
    assert_eq!(
        (c.tbl.as_str(), c.row_id.as_str(), c.op),
        ("rollup_log", SUMMARY_LOG_DATE, ChangeOp::Insert)
    );
    let second = f
        .writer
        .submit_intent(intent(system(), publish(T0 + 900_000, "state/latest.json")))
        .await
        .unwrap();
    assert_eq!(second.changes[0].op, ChangeOp::Update);
    let rows = reopen(&f).rows(Table::RollupLog).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["built_at"], RowValue::Integer(T0 + 900_000));
    assert_eq!(rows[0]["schema"], RowValue::Integer(1));
    assert_eq!(rows[0]["rev"], RowValue::Integer(2));
    assert_eq!(
        reopen(&f)
            .rollup_log(SUMMARY_LOG_DATE)
            .unwrap()
            .unwrap()
            .key,
        "state/latest.json"
    );
}

#[tokio::test]
async fn only_the_system_can_publish_and_the_payload_is_checked() {
    let f = booted().await;
    let refused = f
        .writer
        .submit_intent(intent(owner(), publish(T0, "state/latest.json")))
        .await;
    assert!(refused.is_err(), "an owner cannot publish a summary");
    for (built_at, key) in [(T0, ""), (-1, "state/latest.json"), (T0, "a\nb")] {
        let e = f
            .writer
            .submit_intent(intent(system(), publish(built_at, key)))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed, "{built_at} {key:?}");
    }
    let mut with_rev = intent(system(), publish(T0, "state/latest.json"));
    with_rev.base_rev = Some(1);
    assert_eq!(
        f.writer.submit_intent(with_rev).await.unwrap_err().code,
        ErrorCode::ValidationFailed
    );
    assert!(reopen(&f).rows(Table::RollupLog).unwrap().is_empty());
}

#[tokio::test]
async fn the_summary_reports_the_open_shift_the_catalogue_and_what_needs_attention() {
    let f = booted().await;
    let cola = make(&f, up("Cola")).await;
    set_threshold(&f, owner(), &cola, Some(5)).await.unwrap();
    f.writer
        .submit_intent(intent(
            system(),
            IntentPayload::AlertRaise(AlertRaise::new(AlertKind::Fixture, None)),
        ))
        .await
        .unwrap();

    let reader = SnapshotReader::open(&f.path).unwrap();
    let s = reader.branch_summary(T0).unwrap().unwrap();
    assert_eq!(s.schema, SUMMARY_SCHEMA);
    assert_eq!(s.as_of, T0);
    assert_eq!(s.currency, "INR");
    assert!(s.open && s.sessions_live == 0);
    assert_eq!(s.revenue_so_far_minor, 0);
    assert_eq!(s.cash_in_drawer_minor, 0);
    assert_eq!(s.shifts.len(), 1);
    assert_eq!(s.shifts[0].shift_id, "SHIFT0");
    assert!(s.shifts[0].variance_minor.is_none());
    assert_eq!(s.staff_on_shift, vec![s.shifts[0].staff_name.clone()]);

    let product = s.catalogue.products.iter().find(|p| p.id == cola).unwrap();
    assert_eq!(
        (product.name.as_str(), product.active),
        ("Cola", Some(true))
    );
    assert!(product.rev >= 1);
    assert!(!s.catalogue.roles.is_empty());

    let low = s
        .attention
        .iter()
        .find(|a| a.kind == ATTENTION_LOW_STOCK)
        .expect("the product at its threshold needs attention");
    assert_eq!(low.reference, cola);
    assert_eq!(low.text_params["product"], "Cola");
    let alert = s
        .attention
        .iter()
        .find(|a| a.kind == ATTENTION_ALERT)
        .expect("the open alert needs attention");
    assert_eq!(alert.text_params["alert_kind"], AlertKind::Fixture.as_str());
    assert_eq!(
        s.attention
            .iter()
            .filter(|a| a.reference == cola || a.text_params.contains_key("product"))
            .count(),
        1,
        "a low-stock alert is not listed twice"
    );

    let json = serde_json::to_string(&s).unwrap();
    for secret in ["password_hash", "pin_hash", "token_hash", "phone"] {
        assert!(!json.contains(secret), "{secret} in the summary");
    }
}

/// The reference PC: 60 devices and a full business day of settled sales. The file is built in
/// well under 50 ms on the Hub (release); a debug build gets more room.
#[tokio::test]
async fn the_summary_of_a_full_day_builds_within_its_budget() {
    let f = booted().await;
    let mut store = reopen(&f);
    let branch = store.branch_summary().unwrap().unwrap().branch_id;
    let day = crate::locale::current_business_day(&store, T0)
        .unwrap()
        .unwrap();
    store
        .write_tx(|tx| {
            tx.execute_batch(&format!(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 60)
                 INSERT INTO devices (device_id, branch_id, class, label, token_hash, rev, created_at, updated_at)
                 SELECT 'D' || i, '{branch}', 'paired', 'PC ' || i, 'x', 1, 0, 0 FROM n;
                 INSERT INTO devices_meta (device_id, kind, label, group_id, rev, created_at, updated_at)
                 SELECT device_id, 'pc', label, NULL, 1, 0, 0 FROM devices WHERE device_id LIKE 'D%';
                 WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1500)
                 INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                     created_at, updated_at, settled_at, kind)
                 SELECT 'O' || i, 'settled', 5000, 5000, '{day}', '2026-27', 1, {T0}, {T0}, {T0}, 'pos_sale' FROM n;
                 INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position,
                                       business_day, fy, rev, created_at, updated_at)
                 SELECT 'P' || id, id, 'cash', 5000, 'test', 0, '{day}', '2026-27', 1, {T0}, {T0}
                 FROM orders WHERE id LIKE 'O%';"
            ))
            .unwrap();
            Ok(())
        })
        .unwrap();
    let reader = SnapshotReader::open(&f.path).unwrap();
    reader.branch_summary(T0).unwrap(); // warm the connection
    let started = std::time::Instant::now();
    let s = reader.branch_summary(T0).unwrap().unwrap();
    let took = started.elapsed();
    assert_eq!(s.catalogue.devices.len(), 60);
    assert_eq!(s.revenue_so_far_minor, 1500 * 5000);
    let budget = if cfg!(debug_assertions) { 500 } else { 50 };
    assert!(
        took.as_millis() < budget,
        "the summary took {took:?} (budget {budget} ms)"
    );
}

#[tokio::test]
async fn a_branch_with_nothing_bootstrapped_has_no_summary() {
    let f = primary();
    let reader = SnapshotReader::open(&f.path).unwrap();
    assert!(reader.branch_summary(T0).unwrap().is_none());
}

#[tokio::test]
async fn a_counted_shift_follows_the_open_one_with_its_signed_variance() {
    use arena_proto::{ATTENTION_SHIFT_VARIANCE, ShiftClose, VarianceReason};

    let f = booted().await;
    let before = SnapshotReader::open(&f.path)
        .unwrap()
        .branch_summary(T0)
        .unwrap()
        .unwrap();
    assert!(before.shifts.iter().all(|s| s.closed_at.is_none()));

    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::ShiftClose(ShiftClose {
                counted_minor: 5_000_000,
                deposit: None,
                reason: Some(VarianceReason::CountingError),
                note: None,
            }),
        ))
        .await
        .unwrap();

    let s = SnapshotReader::open(&f.path)
        .unwrap()
        .branch_summary(T0)
        .unwrap()
        .unwrap();
    assert!(s.staff_on_shift.is_empty());
    let counted = s
        .shifts
        .iter()
        .find(|c| c.shift_id == "SHIFT0")
        .expect("the counted shift is listed");
    let variance = counted.variance_minor.expect("a counted shift has one");
    assert!(variance > 0, "an over count is positive: {variance}");
    assert!(counted.closed_at.is_some());
    assert!(
        s.attention
            .iter()
            .any(|a| a.kind == ATTENTION_SHIFT_VARIANCE && a.reference == "SHIFT0")
    );
}

/// Story 8.23: the pricing rows carry the fields their upsert payload takes, so the Owner app can
/// prefill an edit; nothing else (no `id`, no `active`) rides along.
#[tokio::test]
async fn pricing_rows_carry_the_fields_of_their_upsert_payload() {
    use arena_proto::{CouponTarget, CouponUpsert, PlanUpsert, RateUpsert, RuleUpsert};
    use serde_json::json;

    let f = booted().await;
    let group = super::plans::group(&f, "Row A", "pc").await;
    let submit = |p: IntentPayload| f.writer.submit_intent(intent(owner(), p));
    submit(IntentPayload::RateUpsert(RateUpsert {
        group_id: group.clone(),
        price_per_hour_minor: 6_000,
    }))
    .await
    .unwrap();
    submit(IntentPayload::RateUpsert(RateUpsert {
        group_id: group.clone(),
        price_per_hour_minor: 8_000,
    }))
    .await
    .unwrap();
    submit(IntentPayload::PlanUpsert(PlanUpsert {
        id: None,
        plan_kind: "pass".into(),
        name: "Night pass".into(),
        price_minor: 90_000,
        minutes: None,
        validity_days: Some(30),
        window_start_min: Some(22 * 60),
        window_end_min: Some(6 * 60),
        group_ids: vec![group.clone()],
        active: true,
    }))
    .await
    .unwrap();
    submit(IntentPayload::RuleUpsert(RuleUpsert {
        id: None,
        name: "Happy hour".into(),
        group_id: group.clone(),
        rule_kind: "weekly".into(),
        days_mask: Some(0b001_1111),
        start_min: Some(16 * 60),
        end_min: Some(18 * 60),
        date_from: None,
        date_to: None,
        adjust_kind: "percent_bp".into(),
        adjust_value: -2000,
        priority: 2,
        active: true,
    }))
    .await
    .unwrap();
    submit(IntentPayload::CouponUpsert(CouponUpsert {
        id: None,
        code: "WELCOME".into(),
        percent_bp: Some(1_000),
        amount_minor: None,
        valid_from: Some("2027-01-01".into()),
        valid_to: None,
        usage_limit: Some(50),
        min_bill_minor: 10_000,
        targets: vec![CouponTarget {
            kind: "group".into(),
            id: group.clone(),
        }],
        active: true,
    }))
    .await
    .unwrap();

    let s = SnapshotReader::open(&f.path)
        .unwrap()
        .branch_summary(T0)
        .unwrap()
        .unwrap();
    let fields = |rows: &[arena_proto::CatalogueRow]| -> Vec<serde_json::Value> {
        rows.iter().filter_map(|r| r.fields.clone()).collect()
    };

    // The first rate is inactive history; the current one is the row the app shows.
    let rates = fields(&s.catalogue.rates);
    assert_eq!(rates.len(), 2);
    assert!(s.catalogue.rates.iter().any(|r| r.active == Some(false)));
    assert!(rates.contains(&json!({"group_id": group, "price_per_hour_minor": 8_000})));

    assert_eq!(
        fields(&s.catalogue.plans),
        vec![json!({
            "plan_kind": "pass", "price_minor": 90_000, "validity_days": 30,
            "window_start_min": 1_320, "window_end_min": 360, "group_ids": [group]
        })]
    );
    assert_eq!(
        fields(&s.catalogue.pricing_rules),
        vec![json!({
            "group_id": group, "rule_kind": "weekly", "days_mask": 31, "start_min": 960,
            "end_min": 1_080, "adjust_kind": "percent_bp", "adjust_value": -2_000, "priority": 2
        })]
    );
    assert_eq!(
        fields(&s.catalogue.coupons),
        vec![json!({
            "percent_bp": 1_000, "valid_from": "2027-01-01", "usage_limit": 50,
            "min_bill_minor": 10_000, "targets": [{"kind": "group", "id": group}]
        })]
    );

    // Every other list is unchanged: no `fields` key at all.
    assert!(s.catalogue.groups.iter().all(|r| r.fields.is_none()));
    let json = serde_json::to_value(&s.catalogue.roles).unwrap();
    assert!(
        json.as_array()
            .unwrap()
            .iter()
            .all(|r| r.get("fields").is_none())
    );
}

/// Story 8.24: products and bundles carry the fields of their upsert payload (no image, no id, no
/// `active`) plus the low-stock threshold, and a bundle its components.
#[tokio::test]
async fn product_and_bundle_rows_carry_the_fields_of_their_upsert_payload() {
    use arena_proto::BundleComponent;
    use serde_json::json;

    let f = booted().await;
    let cola = make(
        &f,
        arena_proto::ProductUpsert {
            category: "Drinks".into(),
            price_minor: 5_000,
            tax_rate_bp: Some(1_200),
            hsn: "2202".into(),
            barcode: "890123".into(),
            kitchen: true,
            image: super::shop::jpeg(),
            ..up("Cola")
        },
    )
    .await;
    set_threshold(&f, owner(), &cola, Some(4)).await.unwrap();
    let combo = make(
        &f,
        arena_proto::ProductUpsert {
            price_minor: 9_000,
            bundle: true,
            track_stock: false,
            components: vec![BundleComponent {
                product_id: cola.clone(),
                qty: 2,
            }],
            ..up("Combo")
        },
    )
    .await;

    let s = SnapshotReader::open(&f.path)
        .unwrap()
        .branch_summary(T0)
        .unwrap()
        .unwrap();
    let cola_row = s.catalogue.products.iter().find(|r| r.id == cola).unwrap();
    assert_eq!(cola_row.detail.as_deref(), Some("Drinks"));
    assert_eq!(
        cola_row.fields,
        Some(json!({
            "category": "Drinks", "price_minor": 5_000, "tax_rate_bp": 1_200, "hsn": "2202",
            "track_stock": true, "barcode": "890123", "kitchen": true, "favourite": false,
            "bundle": false, "low_stock_qty": 4
        }))
    );
    let combo_row = s.catalogue.bundles.iter().find(|r| r.id == combo).unwrap();
    let fields = combo_row.fields.clone().unwrap();
    assert_eq!(fields["bundle"], json!(true));
    assert_eq!(fields["track_stock"], json!(false));
    assert_eq!(
        fields["components"],
        json!([{"product_id": cola, "qty": 2}])
    );
    assert!(fields.get("image").is_none() && fields.get("low_stock_qty").is_none());
}
