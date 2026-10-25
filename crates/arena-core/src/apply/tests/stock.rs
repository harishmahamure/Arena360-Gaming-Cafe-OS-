use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::{ProductDelete, ProductUpsert, StaffUpsert, StockIn, StockInReverted};

fn owner() -> Actor {
    Actor {
        kind: ActorKind::Owner,
        id: "o".into(),
    }
}

fn intent(actor: Actor, payload: IntentPayload) -> Intent {
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

async fn staff(f: &Fixture, login: &str, role: &str) -> Actor {
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
        .products()
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
        .catalog(std::slice::from_ref(&cola))
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
