use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN, ROLE_MANAGER};
use arena_proto::{
    AlertAck, ProductDelete, ProductUpsert, StaffUpsert, StockExpiryCheck, StockIn,
    StockInReverted, StockWaste, StockWasteReverted, StockWriteOff,
};

pub(super) fn owner() -> Actor {
    Actor {
        kind: ActorKind::Owner,
        id: "o".into(),
    }
}

pub(super) fn intent(actor: Actor, payload: IntentPayload) -> Intent {
    Intent {
        kind: payload.kind(),
        payload,
        actor,
        issued_at: T0,
        ..bootstrap()
    }
}

fn up(name: &str) -> ProductUpsert {
    ProductUpsert {
        id: None,
        name: name.into(),
        category: String::new(),
        price_minor: 100,
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
    }
}

async fn make(f: &Fixture, u: ProductUpsert) -> String {
    f.writer
        .submit_intent(intent(owner(), IntentPayload::ProductUpsert(u)))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone()
}

pub(super) async fn staff(f: &Fixture, login: &str, role: &str) -> Actor {
    let rid = match reopen(f)
        .rows(Table::Roles)
        .unwrap()
        .into_iter()
        .find(|r| r["name"] == RowValue::Text(role.into()))
        .unwrap()["role_id"]
        .clone()
    {
        RowValue::Text(t) => t,
        other => panic!("{other:?}"),
    };
    let id = f
        .writer
        .submit_intent(intent(
            owner(),
            IntentPayload::StaffUpsert(StaffUpsert {
                staff_id: None,
                login: login.into(),
                display_name: login.into(),
                role_id: rid,
                password_hash: Some(crate::staff::hash_secret("pw").unwrap()),
                pin_hash: None,
                active: true,
                setup_code_hash: None,
                setup_expires_at: None,
            }),
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    Actor {
        kind: ActorKind::Staff,
        id,
    }
}

fn sin(id: &str, qty: i64, cost: i64) -> StockIn {
    StockIn {
        product_id: id.into(),
        qty,
        unit_cost_minor: cost,
        supplier: "Acme".into(),
        expiry: None,
    }
}

async fn put(f: &Fixture, actor: Actor, s: StockIn) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(actor, IntentPayload::StockIn(s)))
        .await
}

async fn stock_in(f: &Fixture, id: &str, qty: i64, cost: i64) -> Applied {
    put(f, owner(), sin(id, qty, cost)).await.unwrap()
}

fn move_of(a: &Applied) -> String {
    a.changes
        .iter()
        .find(|c| c.tbl == "stock_moves")
        .unwrap()
        .row_id
        .clone()
}

async fn undo(f: &Fixture, actor: Actor, move_id: &str) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(
            actor,
            IntentPayload::StockInReverted(StockInReverted {
                move_id: move_id.into(),
            }),
        ))
        .await
}

fn held(f: &Fixture, id: &str) -> (i64, i64) {
    reopen(f)
        .stock(id)
        .unwrap()
        .map_or((0, 0), |s| (s.qty, s.avg_cost_minor))
}

fn state(f: &Fixture) -> (Vec<Row>, Vec<Row>, Vec<Row>, usize) {
    let s = reopen(f);
    (
        s.rows(Table::Stock).unwrap(),
        s.rows(Table::StockLots).unwrap(),
        s.rows(Table::StockMoves).unwrap(),
        s.changes().unwrap().len(),
    )
}

async fn refused(f: &Fixture, s: StockIn) -> (ErrorCode, Option<String>) {
    let before = state(f);
    let e = put(f, owner(), s).await.unwrap_err();
    assert_eq!(state(f), before);
    (e.code, e.field)
}

fn field(f: &str) -> (ErrorCode, Option<String>) {
    (ErrorCode::ValidationFailed, Some(f.into()))
}

fn view(f: &Fixture, id: &str) -> arena_proto::Product {
    crate::SnapshotReader::open(&f.path)
        .unwrap()
        .products(T0)
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
}

#[tokio::test]
async fn ten_at_20_and_thirty_at_24_average_23() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let first = stock_in(&f, &id, 10, 2000).await;
    assert_eq!(held(&f, &id), (10, 2000));
    assert_eq!(
        first
            .changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect::<Vec<_>>(),
        [
            ("stock", ChangeOp::Insert),
            ("stock_lots", ChangeOp::Insert),
            ("stock_moves", ChangeOp::Insert)
        ]
    );
    let second = stock_in(&f, &id, 30, 2400).await;
    assert_eq!(held(&f, &id), (40, 2300));
    assert_eq!(second.changes[0].op, ChangeOp::Update);
    let s = reopen(&f);
    let lots = s.lots(&id).unwrap();
    assert_eq!(lots.len(), 2);
    assert_eq!(lots.iter().map(|l| l.qty_remaining).sum::<i64>(), 40);
    let moves = s.moves(&id).unwrap();
    assert_eq!(moves.len(), 2);
    assert_eq!(
        (
            moves[1].reason.as_str(),
            moves[1].delta,
            moves[1].before_qty,
            moves[1].before_avg_cost_minor
        ),
        ("stock_in", 30, 10, 2000)
    );
    assert_eq!(moves[1].supplier, "Acme");
    assert_eq!(s.stock(&id).unwrap().unwrap().rev, 2);
}

#[tokio::test]
async fn an_untouched_product_reads_zero_and_the_first_stock_in_creates_its_row() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    assert!(reopen(&f).stock(&id).unwrap().is_none());
    let p = view(&f, &id);
    assert_eq!((p.quantity, p.avg_cost_minor, p.available), (0, 0, false));
    assert_eq!(p.reason.as_deref(), Some("out_of_stock"));
    stock_in(&f, &id, 5, 150).await;
    let p = view(&f, &id);
    assert_eq!(
        (p.quantity, p.avg_cost_minor, p.available, p.reason),
        (5, 150, true, None)
    );
}

#[tokio::test]
async fn expiry_supplier_and_cost_edges_are_stored_or_refused() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let mut s = sin(&id, 1, 0);
    s.supplier = "  ".into();
    s.expiry = Some("2027-01-15".into());
    put(&f, owner(), s.clone()).await.unwrap();
    s.expiry = Some("2099-12-31".into());
    s.unit_cost_minor = arena_proto::MAX_PRICE_MINOR;
    s.supplier = "x".repeat(80);
    put(&f, owner(), s).await.unwrap();
    let lots = reopen(&f).lots(&id).unwrap();
    let mut got: Vec<_> = lots.iter().map(|l| l.expiry.clone()).collect();
    got.sort();
    assert_eq!(got, [Some("2027-01-15".into()), Some("2099-12-31".into())]);
    assert_eq!(reopen(&f).moves(&id).unwrap()[0].supplier, "");
}

#[tokio::test]
async fn every_invalid_stock_in_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let loose = {
        let mut u = up("Water");
        u.track_stock = false;
        make(&f, u).await
    };
    let mut b = up("Combo");
    b.bundle = true;
    b.components = vec![arena_proto::BundleComponent {
        product_id: id.clone(),
        qty: 1,
    }];
    let bundle = make(&f, b).await;
    let gone = make(&f, up("Old")).await;
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::ProductDelete(ProductDelete { id: gone.clone() }),
        ))
        .await
        .unwrap();
    stock_in(&f, &id, 1, 100).await;
    for p in [&loose, &bundle, &gone, "nope"] {
        assert_eq!(
            refused(&f, sin(p, 1, 100)).await,
            field("product_id"),
            "{p}"
        );
    }
    assert!(reopen(&f).stock(&bundle).unwrap().is_none());
    assert!(reopen(&f).stock(&loose).unwrap().is_none());
    for q in [0, -1, i64::MIN, 1_000_001, i64::MAX] {
        assert_eq!(refused(&f, sin(&id, q, 100)).await, field("qty"), "{q}");
    }
    for c in [-1, i64::MIN, arena_proto::MAX_PRICE_MINOR + 1] {
        assert_eq!(
            refused(&f, sin(&id, 1, c)).await,
            field("unit_cost_minor"),
            "{c}"
        );
    }
    for s in ["x".repeat(81), "a\nb".into(), "\u{7}".into()] {
        let mut v = sin(&id, 1, 1);
        v.supplier = s;
        assert_eq!(refused(&f, v).await, field("supplier"));
    }
    for e in [
        "2027-01-14",
        "2020-01-01",
        "2026-02-30",
        "26-10-25",
        "",
        "tomorrow",
    ] {
        let mut v = sin(&id, 1, 1);
        v.expiry = Some(e.into());
        assert_eq!(refused(&f, v).await, field("expiry"), "{e}");
    }
}

#[tokio::test]
async fn the_stock_total_is_capped() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    for _ in 0..99 {
        stock_in(&f, &id, 1_000_000, 1).await;
    }
    stock_in(&f, &id, 999_999, 1).await;
    assert_eq!(held(&f, &id).0, 99_999_999);
    assert_eq!(refused(&f, sin(&id, 2, 1)).await, field("qty"));
    stock_in(&f, &id, 1, 1).await;
    assert_eq!(held(&f, &id).0, 100_000_000);
    assert_eq!(refused(&f, sin(&id, 1, 1)).await, field("qty"));
}

#[tokio::test]
async fn the_expiry_day_follows_the_business_day() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let mut s = sin(&id, 1, 1);
    s.expiry = Some("2027-01-15".into());
    put(&f, owner(), s.clone()).await.unwrap();
    let day = 120 * 24 * 3600 * 1000;
    f.clock.advance(day);
    let late = Intent {
        issued_at: T0 + day,
        ..intent(owner(), IntentPayload::StockIn(s))
    };
    let e = f.writer.submit_intent(late).await.unwrap_err();
    assert_eq!((e.code, e.field), field("expiry"));
}

#[tokio::test]
async fn counters_stock_in_and_others_cannot() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let kitchen = staff(&f, "kit", ROLE_KITCHEN).await;
    put(&f, counter.clone(), sin(&id, 2, 10)).await.unwrap();
    assert_eq!(reopen(&f).moves(&id).unwrap()[0].staff_id, counter.id);
    let before = state(&f);
    let e = put(&f, kitchen.clone(), sin(&id, 2, 10)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    let m = reopen(&f).moves(&id).unwrap()[0].move_id.clone();
    let e = undo(&f, kitchen, &m).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(state(&f), before);
}

#[tokio::test]
async fn undo_restores_quantity_average_and_removes_the_lot() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    stock_in(&f, &id, 10, 2000).await;
    let before = (held(&f, &id), reopen(&f).lots(&id).unwrap());
    f.clock.advance(1);
    let a = put(&f, counter.clone(), sin(&id, 30, 2400)).await.unwrap();
    assert_eq!(held(&f, &id), (40, 2300));
    f.clock.advance(8_000);
    let u = undo(&f, counter.clone(), &move_of(&a)).await.unwrap();
    assert_eq!(
        u.changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect::<Vec<_>>(),
        [
            ("stock", ChangeOp::Update),
            ("stock_lots", ChangeOp::Delete),
            ("stock_moves", ChangeOp::Insert)
        ]
    );
    assert_eq!((held(&f, &id), reopen(&f).lots(&id).unwrap()), before);
    let moves = reopen(&f).moves(&id).unwrap();
    assert_eq!(moves.len(), 3);
    assert_eq!(
        (moves[2].reason.as_str(), moves[2].delta),
        ("stock_in_reverted", -30)
    );
    assert_eq!(moves[2].before_qty, 40);
    let e = undo(&f, counter, &move_of(&a)).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::Stale, Some("move_id"))
    );
}

#[tokio::test]
async fn undo_of_a_first_stock_in_returns_to_zero_and_free_cost_restores_exactly() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let a = stock_in(&f, &id, 7, 333).await;
    undo(&f, owner(), &move_of(&a)).await.unwrap();
    assert_eq!(held(&f, &id), (0, 0));
    let view = view(&f, &id);
    assert_eq!((view.quantity, view.available), (0, false));
    stock_in(&f, &id, 3, 10).await;
    let b = stock_in(&f, &id, 4, 11).await;
    assert_eq!(held(&f, &id), (7, 11));
    undo(&f, owner(), &move_of(&b)).await.unwrap();
    assert_eq!(held(&f, &id), (3, 10));
}

#[tokio::test]
async fn undo_is_refused_late_by_others_and_after_anything_else() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let other = make(&f, up("Tea")).await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let other_staff = staff(&f, "ctr2", ROLE_COUNTER).await;
    let a = put(&f, counter.clone(), sin(&id, 5, 100)).await.unwrap();
    let m = move_of(&a);
    let before = state(&f);
    let e = undo(&f, other_staff, &m).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    let e = undo(&f, owner(), &m).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    let e = undo(&f, counter.clone(), "nope").await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("move_id"))
    );
    f.clock.advance(8_001);
    let e = undo(&f, counter.clone(), &m).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Expired);
    assert_eq!(state(&f), before);

    let b = put(&f, counter.clone(), sin(&id, 5, 100)).await.unwrap();
    put(&f, counter.clone(), sin(&id, 1, 100)).await.unwrap();
    let before = state(&f);
    let e = undo(&f, counter.clone(), &move_of(&b)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    assert_eq!(state(&f), before);

    let c = put(&f, counter.clone(), sin(&other, 5, 100)).await.unwrap();
    stock_in(&f, &id, 1, 100).await;
    undo(&f, counter.clone(), &move_of(&c)).await.unwrap();
    let reverted = reopen(&f)
        .moves(&other)
        .unwrap()
        .into_iter()
        .find(|m| m.reason == "stock_in_reverted")
        .unwrap();
    let e = undo(&f, counter, &reverted.move_id).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("move_id"))
    );
}

#[tokio::test]
async fn undo_is_refused_once_a_lot_was_drawn_down() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let a = stock_in(&f, &id, 5, 100).await;
    let mut store = reopen(&f);
    store
        .write_tx(|tx| decrement(tx, &id, 2, "waste", "s", T0).map(|_| ()))
        .unwrap();
    let e = undo(&f, owner(), &move_of(&a)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
}

fn lots_of(f: &Fixture, id: &str) -> Vec<(Option<String>, i64)> {
    let mut v: Vec<_> = reopen(f)
        .lots(id)
        .unwrap()
        .into_iter()
        .map(|l| (l.expiry, l.qty_remaining))
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn a_decrement_takes_the_earliest_expiry_first_and_never_goes_below_zero() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let mut s = sin(&id, 10, 100);
    put(&f, owner(), s.clone()).await.unwrap();
    s.qty = 5;
    s.expiry = Some("2027-03-01".into());
    put(&f, owner(), s.clone()).await.unwrap();
    s.qty = 3;
    s.expiry = Some("2027-02-01".into());
    put(&f, owner(), s).await.unwrap();
    let mut store = reopen(&f);
    let before = state(&f);
    for bad in [0, -1, 19] {
        let r = store.write_tx(|tx| decrement(tx, &id, bad, "waste", "s", T0).map(|_| ()));
        assert_eq!(r.unwrap_err().field.as_deref(), Some("qty"), "{bad}");
        assert_eq!(state(&f), before);
    }
    let written = store
        .write_tx(|tx| decrement(tx, &id, 7, "waste", "s", T0 + 1))
        .unwrap();
    assert_eq!(written.len(), 4);
    assert_eq!(
        lots_of(&f, &id),
        [
            (None, 10),
            (Some("2027-02-01".into()), 0),
            (Some("2027-03-01".into()), 1)
        ]
    );
    assert_eq!(held(&f, &id), (11, 100));
    let last = reopen(&f).moves(&id).unwrap().pop().unwrap();
    assert_eq!(
        (last.delta, last.reason.as_str(), last.before_qty),
        (-7, "waste", 18)
    );
    let used: serde_json::Value = serde_json::from_str(&last.lots_used).unwrap();
    assert_eq!(used.as_array().unwrap().len(), 2);
    assert_eq!(used[0]["qty"], 3);
    assert_eq!(used[1]["qty"], 4);
    store
        .write_tx(|tx| decrement(tx, &id, 11, "waste", "s", T0 + 2).map(|_| ()))
        .unwrap();
    assert_eq!(held(&f, &id), (0, 100));
    let r = store.write_tx(|tx| decrement(tx, &id, 1, "waste", "s", T0 + 3).map(|_| ()));
    assert!(r.is_err());
    let free = stock_in(&f, &id, 4, 200).await;
    assert_eq!(held(&f, &id), (4, 200));
    assert_eq!(free.changes[0].op, ChangeOp::Update);
}

#[tokio::test]
async fn bundles_read_component_stock_and_stock_changes_reach_the_catalog() {
    let f = bootstrapped().await;
    let cola = make(&f, up("Cola")).await;
    let mut b = up("Combo");
    b.bundle = true;
    b.components = vec![arena_proto::BundleComponent {
        product_id: cola.clone(),
        qty: 3,
    }];
    let bundle = make(&f, b).await;
    assert_eq!(
        (view(&f, &bundle).quantity, view(&f, &bundle).available),
        (0, false)
    );
    stock_in(&f, &cola, 8, 100).await;
    let v = view(&f, &bundle);
    assert_eq!((v.quantity, v.available, v.avg_cost_minor), (2, true, 0));
    stock_in(&f, &cola, 1, 100).await;
    assert_eq!(view(&f, &bundle).quantity, 3);
    let entries = crate::SnapshotReader::open(&f.path)
        .unwrap()
        .catalog(std::slice::from_ref(&cola), T0)
        .unwrap();
    let ids: Vec<_> = entries
        .iter()
        .map(|e| (e.product_id.as_str(), e.available))
        .collect();
    assert!(ids.contains(&(cola.as_str(), true)) && ids.contains(&(bundle.as_str(), true)));
    assert!(reopen(&f).stock(&bundle).unwrap().is_none());
}

#[tokio::test]
async fn replayed_stock_in_is_idempotent_and_writes_once() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let i = intent(owner(), IntentPayload::StockIn(sin(&id, 4, 50)));
    f.writer.submit_intent(i.clone()).await.unwrap();
    let again = f.writer.submit_intent(i).await.unwrap();
    assert_eq!(held(&f, &id), (4, 50));
    assert_eq!(reopen(&f).moves(&id).unwrap().len(), 1);
    assert_eq!(again.changes.len(), 3);
}

// ---- Story 5.6: waste ----

fn waste_intent(actor: Actor, id: &str, qty: i64, reason: Option<ReasonCode>) -> Intent {
    Intent {
        reason,
        ..intent(
            actor,
            IntentPayload::StockWaste(StockWaste {
                product_id: id.into(),
                qty,
            }),
        )
    }
}

async fn waste(
    f: &Fixture,
    actor: Actor,
    id: &str,
    qty: i64,
    reason: ReasonCode,
) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(waste_intent(actor, id, qty, Some(reason)))
        .await
}

async fn waste_undo(f: &Fixture, actor: Actor, move_id: &str) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(
            actor,
            IntentPayload::StockWasteReverted(StockWasteReverted {
                move_id: move_id.into(),
            }),
        ))
        .await
}

async fn manager_with_pin(f: &Fixture) -> String {
    let rid = reopen(f)
        .rows(Table::Roles)
        .unwrap()
        .into_iter()
        .find(|r| r["name"] == RowValue::Text(ROLE_MANAGER.into()))
        .map(|r| match &r["role_id"] {
            RowValue::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        })
        .unwrap();
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::StaffUpsert(StaffUpsert {
                staff_id: None,
                login: "mgr".into(),
                display_name: "mgr".into(),
                role_id: rid,
                password_hash: Some(crate::staff::hash_secret("pw").unwrap()),
                pin_hash: Some(crate::staff::hash_secret("4321").unwrap()),
                active: true,
                setup_code_hash: None,
                setup_expires_at: None,
            }),
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone()
}

fn kinds(a: &Applied) -> Vec<(&str, ChangeOp)> {
    a.changes.iter().map(|c| (c.tbl.as_str(), c.op)).collect()
}

#[tokio::test]
async fn waste_takes_lots_earliest_expiry_first_and_records_reason_value_and_staff() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let mut s = sin(&id, 10, 100);
    put(&f, owner(), s.clone()).await.unwrap();
    s.qty = 5;
    s.unit_cost_minor = 200;
    s.expiry = Some("2027-03-01".into());
    put(&f, owner(), s.clone()).await.unwrap();
    s.qty = 3;
    s.expiry = Some("2027-02-01".into());
    put(&f, owner(), s).await.unwrap();
    let avg = held(&f, &id).1;
    let a = waste(&f, owner(), &id, 7, ReasonCode::Spoiled)
        .await
        .unwrap();
    assert_eq!(
        kinds(&a),
        [
            ("stock", ChangeOp::Update),
            ("stock_lots", ChangeOp::Update),
            ("stock_lots", ChangeOp::Update),
            ("stock_moves", ChangeOp::Insert)
        ]
    );
    assert_eq!(
        lots_of(&f, &id),
        [
            (None, 10),
            (Some("2027-02-01".into()), 0),
            (Some("2027-03-01".into()), 1)
        ]
    );
    assert_eq!(held(&f, &id), (11, avg));
    let m = reopen(&f).moves(&id).unwrap().pop().unwrap();
    assert_eq!(
        (
            m.reason.as_str(),
            m.waste_reason.as_deref(),
            m.delta,
            m.staff_id.as_str(),
            m.unit_cost_minor,
            m.value_minor
        ),
        ("waste", Some("spoiled"), -7, "o", avg, 7 * avg)
    );
    let used: serde_json::Value = serde_json::from_str(&m.lots_used).unwrap();
    assert_eq!(used.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn waste_over_stock_is_out_of_stock_and_bad_input_writes_nothing() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let untracked = make(
        &f,
        ProductUpsert {
            track_stock: false,
            ..up("Water")
        },
    )
    .await;
    let never = make(&f, up("Gum")).await;
    stock_in(&f, &id, 18, 100).await;
    let before = state(&f);
    let out_of_stock = (ErrorCode::OutOfStock, Some("qty"));
    let bad_qty = (ErrorCode::ValidationFailed, Some("qty"));
    let bad_product = (ErrorCode::ValidationFailed, Some("product_id"));
    let broken = Some(ReasonCode::Broken);
    let cases = [
        (id.as_str(), 19, broken, out_of_stock),
        (never.as_str(), 1, Some(ReasonCode::Free), out_of_stock),
        (id.as_str(), 0, broken, bad_qty),
        (id.as_str(), -3, broken, bad_qty),
        (untracked.as_str(), 1, broken, bad_product),
        ("nope", 1, broken, bad_product),
        (
            id.as_str(),
            1,
            None,
            (ErrorCode::ReasonRequired, Some("reason")),
        ),
        (
            id.as_str(),
            1,
            Some(ReasonCode::Lost),
            (ErrorCode::ValidationFailed, Some("reason")),
        ),
    ];
    for (pid, qty, reason, want) in cases {
        let e = f
            .writer
            .submit_intent(waste_intent(owner(), pid, qty, reason))
            .await
            .unwrap_err();
        assert_eq!((e.code, e.field.as_deref()), want, "{pid} {qty} {reason:?}");
        assert_eq!(state(&f), before);
    }
    waste(&f, owner(), &id, 18, ReasonCode::CancelledOrder)
        .await
        .unwrap();
    assert_eq!(held(&f, &id).0, 0);
}

#[tokio::test]
async fn a_counter_needs_a_manager_pin_for_waste_and_both_people_are_audited() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let manager = manager_with_pin(&f).await;
    stock_in(&f, &id, 5, 100).await;
    let before = state(&f);
    let e = waste(&f, counter.clone(), &id, 1, ReasonCode::Broken)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(state(&f), before);
    let elevated = Intent {
        elevated_by: Some(ElevatedBy {
            kind: ElevationKind::Pin,
            id: manager.clone(),
            mode: ElevationMode::Hub,
            verifier_version: 1,
        }),
        ..waste_intent(counter.clone(), &id, 2, Some(ReasonCode::Broken))
    };
    f.writer.submit_intent(elevated).await.unwrap();
    assert_eq!(held(&f, &id).0, 3);
    let change = reopen(&f).changes().unwrap().pop().unwrap();
    assert_eq!(change.actor_id, counter.id);
    assert_eq!(change.elevated_by_id.as_deref(), Some(manager.as_str()));
    assert_eq!(change.reason.as_deref(), Some("broken"));
    let last = reopen(&f).moves(&id).unwrap().pop().unwrap();
    assert_eq!(last.staff_id, counter.id);
}

#[tokio::test]
async fn waste_undo_returns_the_quantity_to_the_same_lots() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let mut s = sin(&id, 4, 100);
    s.expiry = Some("2027-02-01".into());
    put(&f, owner(), s).await.unwrap();
    stock_in(&f, &id, 6, 300).await;
    let before = (held(&f, &id), lots_of(&f, &id));
    let a = waste(&f, owner(), &id, 7, ReasonCode::Free).await.unwrap();
    assert_ne!((held(&f, &id), lots_of(&f, &id)), before);
    f.clock.advance(8_000);
    let u = waste_undo(&f, owner(), &move_of(&a)).await.unwrap();
    assert_eq!(
        kinds(&u),
        [
            ("stock", ChangeOp::Update),
            ("stock_lots", ChangeOp::Update),
            ("stock_lots", ChangeOp::Update),
            ("stock_moves", ChangeOp::Insert)
        ]
    );
    assert_eq!((held(&f, &id), lots_of(&f, &id)), before);
    let last = reopen(&f).moves(&id).unwrap().pop().unwrap();
    assert_eq!(
        (
            last.reason.as_str(),
            last.delta,
            last.waste_reason.as_deref()
        ),
        ("waste_reverted", 7, Some("free"))
    );
    let e = waste_undo(&f, owner(), &move_of(&a)).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::Stale, Some("move_id"))
    );
}

#[tokio::test]
async fn waste_undo_is_refused_late_by_others_after_anything_else_and_for_other_moves() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    let manager = staff(&f, "m1", ROLE_MANAGER).await;
    let other = staff(&f, "m2", ROLE_MANAGER).await;
    let first = stock_in(&f, &id, 20, 100).await;
    let a = waste(&f, manager.clone(), &id, 2, ReasonCode::Spoiled)
        .await
        .unwrap();
    let m = move_of(&a);
    let before = state(&f);
    let denied = waste_undo(&f, other, &m).await.unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    let wrong_move = (ErrorCode::ValidationFailed, Some("move_id"));
    for e in [
        waste_undo(&f, manager.clone(), "nope").await.unwrap_err(),
        waste_undo(&f, manager.clone(), &move_of(&first))
            .await
            .unwrap_err(),
        undo(&f, manager.clone(), &m).await.unwrap_err(),
    ] {
        assert_eq!((e.code, e.field.as_deref()), wrong_move);
    }
    f.clock.advance(8_001);
    let e = waste_undo(&f, manager.clone(), &m).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Expired);
    assert_eq!(state(&f), before);

    let b = waste(&f, manager.clone(), &id, 1, ReasonCode::Broken)
        .await
        .unwrap();
    waste(&f, manager.clone(), &id, 1, ReasonCode::Broken)
        .await
        .unwrap();
    let before = state(&f);
    let e = waste_undo(&f, manager, &move_of(&b)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    assert_eq!(state(&f), before);
}

#[tokio::test]
async fn stock_moves_with_waste_columns_stay_append_only() {
    let f = bootstrapped().await;
    let id = make(&f, up("Cola")).await;
    stock_in(&f, &id, 5, 100).await;
    waste(&f, owner(), &id, 1, ReasonCode::Spoiled)
        .await
        .unwrap();
    let mut store = reopen(&f);
    store
        .write_tx(|tx| {
            for sql in [
                "UPDATE stock_moves SET waste_reason = 'free'",
                "UPDATE stock_moves SET value_minor = 1",
                "DELETE FROM stock_moves",
            ] {
                assert!(tx.execute(sql, []).is_err(), "{sql}");
            }
            Ok(())
        })
        .unwrap();
}

const DAY_MS: i64 = 24 * 3600 * 1000;

fn today(f: &Fixture, plus_days: i64) -> String {
    crate::locale::current_business_day(&reopen(f), T0 + plus_days * DAY_MS)
        .unwrap()
        .unwrap()
}

fn day_after(f: &Fixture, days: i64) -> String {
    arena_proto::add_days(&today(f, 0), days).unwrap()
}

async fn put_expiring(f: &Fixture, id: &str, qty: i64, cost: i64, expiry: &str) -> Applied {
    let mut s = sin(id, qty, cost);
    s.expiry = Some(expiry.into());
    put(f, owner(), s).await.unwrap()
}

fn open_alerts(f: &Fixture) -> Vec<arena_store::AlertRow> {
    reopen(f)
        .alerts()
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == "stock_expiring")
        .collect()
}

fn alert_rows(f: &Fixture) -> usize {
    reopen(f).rows(Table::Alerts).unwrap().len()
}

async fn ack(f: &Fixture, id: &str) {
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::AlertAck(AlertAck {
                alert_id: id.into(),
            }),
        ))
        .await
        .unwrap();
}

async fn write_off(f: &Fixture, actor: Actor, lot_id: &str) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(Intent {
            issued_at: f.clock.now_ms(),
            ..intent(
                actor,
                IntentPayload::StockWriteOff(StockWriteOff {
                    lot_id: lot_id.into(),
                }),
            )
        })
        .await
}

fn lot_id_of(f: &Fixture, id: &str, expiry: &str) -> String {
    reopen(f)
        .lots(id)
        .unwrap()
        .into_iter()
        .find(|l| l.expiry.as_deref() == Some(expiry))
        .unwrap()
        .lot_id
}

async fn set_warn_days(f: &Fixture, days: &str) {
    let sql = format!("UPDATE settings SET value = '{days}' WHERE key = 'stock.expiry_warn_days'");
    f.writer
        .submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_stock_in_inside_the_window_raises_one_branch_alert() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    let far = day_after(&f, 4);
    let a = put_expiring(&f, &id, 5, 100, &far).await;
    assert!(a.changes.iter().all(|c| c.tbl != "alerts"));
    put(&f, owner(), sin(&id, 1, 100)).await.unwrap();
    assert!(open_alerts(&f).is_empty());
    let a = put_expiring(&f, &id, 5, 100, &day_after(&f, 3)).await;
    assert_eq!(a.changes.iter().filter(|c| c.tbl == "alerts").count(), 1);
    let open = open_alerts(&f);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].device_id, None);
    let a = put_expiring(&f, &id, 5, 100, &day_after(&f, 1)).await;
    assert!(a.changes.iter().all(|c| c.tbl != "alerts"));
    assert_eq!(alert_rows(&f), 1);
}

#[tokio::test]
async fn the_warning_window_is_the_branch_setting_from_zero_to_thirty_days() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    set_warn_days(&f, "0").await;
    put_expiring(&f, &id, 1, 1, &day_after(&f, 1)).await;
    assert!(open_alerts(&f).is_empty());
    put_expiring(&f, &id, 1, 1, &day_after(&f, 0)).await;
    assert_eq!(open_alerts(&f).len(), 1);
    let open = open_alerts(&f).remove(0);
    ack(&f, &open.id).await;
    set_warn_days(&f, "30").await;
    put_expiring(&f, &id, 1, 1, &day_after(&f, 30)).await;
    assert_eq!(open_alerts(&f).len(), 1);
    // Out of range reads as the default of 3 days.
    let open = open_alerts(&f).remove(0);
    ack(&f, &open.id).await;
    set_warn_days(&f, "31").await;
    put_expiring(&f, &id, 1, 1, &day_after(&f, 4)).await;
    assert!(open_alerts(&f).is_empty());
    put_expiring(&f, &id, 1, 1, &day_after(&f, 3)).await;
    assert_eq!(open_alerts(&f).len(), 1);
}

#[tokio::test]
async fn the_daily_check_raises_once_per_business_day_and_replaces_a_stale_alert() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    assert!(f.writer.stock_expiry_check().await.unwrap().is_none());
    put_expiring(&f, &id, 5, 100, &day_after(&f, 40)).await;
    assert!(f.writer.stock_expiry_check().await.unwrap().is_none());
    put_expiring(&f, &id, 5, 100, &day_after(&f, 2)).await;
    let first = open_alerts(&f).remove(0);
    ack(&f, &first.id).await;
    // Raised today already: nothing more until the business day turns.
    assert!(f.writer.stock_expiry_check().await.unwrap().is_none());
    f.clock.advance(DAY_MS);
    let applied = f.writer.stock_expiry_check().await.unwrap().unwrap();
    assert_eq!(
        applied.changes.iter().filter(|c| c.tbl == "alerts").count(),
        1
    );
    let second = open_alerts(&f).remove(0);
    assert_ne!(second.id, first.id);
    assert!(f.writer.stock_expiry_check().await.unwrap().is_none());
    // Left unacknowledged for another day, it is acknowledged by the system and raised afresh.
    f.clock.advance(DAY_MS);
    let applied = f.writer.stock_expiry_check().await.unwrap().unwrap();
    assert_eq!(
        applied.changes.iter().filter(|c| c.tbl == "alerts").count(),
        2
    );
    let third = open_alerts(&f);
    assert_eq!(third.len(), 1);
    assert_ne!(third[0].id, second.id);
    let rows = reopen(&f).rows(Table::Alerts).unwrap();
    let old = rows
        .iter()
        .find(|r| r["id"] == RowValue::Text(second.id.clone()))
        .unwrap();
    assert_eq!(old["acked_by"], RowValue::Text("hub-stock-expiry".into()));
    let change = reopen(&f).changes().unwrap().pop().unwrap();
    assert_eq!(change.intent_kind, "stock.expiry_check");
    assert_eq!(change.actor_id, "hub-stock-expiry");
}

#[tokio::test]
async fn the_check_clears_the_alert_once_nothing_is_left_to_expire() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    put_expiring(&f, &id, 5, 100, &day_after(&f, 1)).await;
    assert_eq!(open_alerts(&f).len(), 1);
    assert!(f.writer.stock_expiry_check().await.unwrap().is_none());
    waste(&f, owner(), &id, 5, ReasonCode::Spoiled)
        .await
        .unwrap();
    assert_eq!(open_alerts(&f).len(), 1);
    let applied = f.writer.stock_expiry_check().await.unwrap().unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert!(open_alerts(&f).is_empty());
    assert!(f.writer.stock_expiry_check().await.unwrap().is_none());
}

#[tokio::test]
async fn only_the_system_runs_the_expiry_check() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    put_expiring(&f, &id, 5, 100, &day_after(&f, 1)).await;
    let before = state(&f);
    for actor in [owner(), staff(&f, "ctr", ROLE_COUNTER).await] {
        let e = f
            .writer
            .submit_intent(intent(
                actor,
                IntentPayload::StockExpiryCheck(StockExpiryCheck {}),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
    }
    assert_eq!(state(&f).0, before.0);
}

#[tokio::test]
async fn an_expired_lot_is_held_from_sale_and_reported_as_expired() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    put_expiring(&f, &id, 5, 100, &day_after(&f, 0)).await;
    let r = || crate::SnapshotReader::open(&f.path).unwrap();
    let p = r().products(T0).unwrap().remove(0);
    assert_eq!((p.available, p.quantity, p.reason), (true, 5, None));
    // The expiry date itself is still sellable; the day after is not.
    let later = T0 + DAY_MS;
    let p = r().products(later).unwrap().remove(0);
    assert_eq!(
        (p.available, p.quantity, p.reason.as_deref()),
        (false, 5, Some("expired"))
    );
    let e = &r().catalog(std::slice::from_ref(&id), later).unwrap()[0];
    assert_eq!((e.available, e.reason.as_deref()), (false, Some("expired")));
    // Unexpired stock keeps the product on sale; only the expired lot is held back.
    put(&f, owner(), sin(&id, 2, 100)).await.unwrap();
    let p = r().products(later).unwrap().remove(0);
    assert_eq!((p.available, p.reason), (true, None));
    // A product with nothing at all left is "out_of_stock", not "expired".
    waste(&f, owner(), &id, 7, ReasonCode::Spoiled)
        .await
        .unwrap();
    let p = r().products(later).unwrap().remove(0);
    assert_eq!(p.reason.as_deref(), Some("out_of_stock"));
}

#[tokio::test]
async fn a_bundle_cannot_be_sold_from_an_expired_component() {
    let f = bootstrapped().await;
    let milk = make(&f, up("Milk")).await;
    let mut b = up("Combo");
    b.track_stock = false;
    b.bundle = true;
    b.components = vec![arena_proto::BundleComponent {
        product_id: milk.clone(),
        qty: 1,
    }];
    let combo = make(&f, b).await;
    put_expiring(&f, &milk, 5, 100, &day_after(&f, 0)).await;
    let r = crate::SnapshotReader::open(&f.path).unwrap();
    let combo_view = |now| {
        r.products(now)
            .unwrap()
            .into_iter()
            .find(|p| p.id == combo)
            .unwrap()
    };
    assert!(combo_view(T0).available);
    let held = combo_view(T0 + DAY_MS);
    assert_eq!((held.available, held.quantity), (false, 0));
}

#[tokio::test]
async fn the_expiring_and_expired_lists_split_at_the_business_day() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    let soon = day_after(&f, 2);
    let today = day_after(&f, 0);
    put_expiring(&f, &id, 4, 150, &soon).await;
    put_expiring(&f, &id, 3, 100, &today).await;
    put_expiring(&f, &id, 1, 100, &day_after(&f, 9)).await;
    put(&f, owner(), sin(&id, 1, 100)).await.unwrap();
    let r = crate::SnapshotReader::open(&f.path).unwrap();
    let v = r.expiry(T0).unwrap();
    assert_eq!((v.warn_days, v.today.as_str()), (3, today.as_str()));
    let lots: Vec<_> = v
        .expiring
        .iter()
        .map(|l| (l.expiry.as_str(), l.qty))
        .collect();
    assert_eq!(lots, [(today.as_str(), 3), (soon.as_str(), 4)]);
    assert_eq!(v.expiring[1].value_minor, 600);
    assert_eq!(v.expiring[0].product, "Milk");
    assert!(v.expired.is_empty());
    let v = r.expiry(T0 + DAY_MS).unwrap();
    assert_eq!(v.expired.len(), 1);
    assert_eq!(v.expired[0].expiry, today);
    assert_eq!(v.expiring.len(), 1);
}

#[tokio::test]
async fn writing_off_an_expired_lot_records_the_move_at_cost_and_clears_the_alert() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    let old = day_after(&f, 0);
    put_expiring(&f, &id, 4, 100, &old).await;
    put(&f, owner(), sin(&id, 6, 300)).await.unwrap();
    assert_eq!(held(&f, &id), (10, 220));
    assert_eq!(open_alerts(&f).len(), 1);
    f.clock.advance(DAY_MS);
    let lot = lot_id_of(&f, &id, &old);
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let before = state(&f);
    let e = write_off(&f, counter, &lot).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(state(&f), before);
    let applied = write_off(&f, owner(), &lot).await.unwrap();
    assert_eq!(held(&f, &id), (6, 220));
    assert_eq!(lots_of(&f, &id), [(None, 6), (Some(old.clone()), 0)]);
    let m = reopen(&f).moves(&id).unwrap().pop().unwrap();
    assert_eq!(
        (
            m.reason.as_str(),
            m.delta,
            m.before_qty,
            m.unit_cost_minor,
            m.value_minor
        ),
        ("expired", -4, 10, 100, 400)
    );
    assert_eq!(m.lot_id.as_deref(), Some(lot.as_str()));
    assert_eq!(m.staff_id, "o");
    let used: serde_json::Value = serde_json::from_str(&m.lots_used).unwrap();
    assert_eq!(used[0]["lot_id"].as_str(), Some(lot.as_str()));
    assert_eq!(used[0]["qty"].as_i64(), Some(4));
    // The only lot in or past the window went: the alert clears in the same change set.
    assert!(open_alerts(&f).is_empty());
    let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["stock", "stock_lots", "stock_moves", "alerts"]);
    // The lot is empty now.
    let before = state(&f);
    let e = write_off(&f, owner(), &lot).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    assert_eq!(state(&f), before);
}

#[tokio::test]
async fn the_alert_stays_while_another_lot_is_expired_or_expiring() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    let a = day_after(&f, 0);
    let b = day_after(&f, 1);
    put_expiring(&f, &id, 2, 100, &a).await;
    put_expiring(&f, &id, 2, 100, &b).await;
    f.clock.advance(DAY_MS);
    write_off(&f, owner(), &lot_id_of(&f, &id, &a))
        .await
        .unwrap();
    assert_eq!(open_alerts(&f).len(), 1);
    f.clock.advance(DAY_MS);
    write_off(&f, owner(), &lot_id_of(&f, &id, &b))
        .await
        .unwrap();
    assert!(open_alerts(&f).is_empty());
    assert_eq!(held(&f, &id).0, 0);
}

#[tokio::test]
async fn a_lot_that_is_not_expired_or_unknown_cannot_be_written_off() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    let on_date = day_after(&f, 0);
    put_expiring(&f, &id, 2, 100, &on_date).await;
    put(&f, owner(), sin(&id, 2, 100)).await.unwrap();
    let no_expiry = reopen(&f)
        .lots(&id)
        .unwrap()
        .into_iter()
        .find(|l| l.expiry.is_none())
        .unwrap()
        .lot_id;
    let before = state(&f);
    for lot in [lot_id_of(&f, &id, &on_date), no_expiry, "nope".into()] {
        let e = write_off(&f, owner(), &lot).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("lot_id"))
        );
        assert_eq!(state(&f), before);
    }
}

#[tokio::test]
async fn a_replayed_write_off_applies_once() {
    let f = bootstrapped().await;
    let id = make(&f, up("Milk")).await;
    let old = day_after(&f, 0);
    put_expiring(&f, &id, 2, 100, &old).await;
    f.clock.advance(DAY_MS);
    let lot = lot_id_of(&f, &id, &old);
    let i = Intent {
        issued_at: f.clock.now_ms(),
        ..intent(
            owner(),
            IntentPayload::StockWriteOff(StockWriteOff { lot_id: lot }),
        )
    };
    let first = f.writer.submit_intent(i.clone()).await.unwrap();
    let again = f.writer.submit_intent(i).await.unwrap();
    assert_eq!(first.changes.len(), again.changes.len());
    assert_eq!(reopen(&f).moves(&id).unwrap().len(), 2);
}
