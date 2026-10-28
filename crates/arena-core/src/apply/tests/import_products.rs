use super::stock::{held, intent, make, owner, staff, up};
use super::*;
use crate::rbac::ROLE_MANAGER;
use arena_proto::{ImportProductRow, ImportProducts, PermEffect, RolePerm, RoleUpsert};

const GST_ON: &str = r#"{"country":"IN","gst":true,"gstin":"27AAPFU0939F1ZV","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"9996","label":"Tax","rate_bp":0}"#;

fn row(name: &str, barcode: &str, price: i64, qty: i64) -> ImportProductRow {
    ImportProductRow {
        name: name.into(),
        category: "Drinks".into(),
        price_minor: price,
        tax_rate_bp: None,
        hsn: String::new(),
        barcode: barcode.into(),
        track_stock: true,
        kitchen: false,
        opening_qty: qty,
        unit_cost_minor: if qty > 0 { 600 } else { 0 },
    }
}

async fn import(
    f: &Fixture,
    actor: Actor,
    rows: Vec<ImportProductRow>,
) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(
            actor,
            IntentPayload::ImportProducts(ImportProducts { rows }),
        ))
        .await
}

fn state(f: &Fixture) -> (Vec<Row>, Vec<Row>, Vec<Row>, Vec<Row>, usize) {
    let s = reopen(f);
    (
        s.rows(Table::Products).unwrap(),
        s.rows(Table::Stock).unwrap(),
        s.rows(Table::StockLots).unwrap(),
        s.rows(Table::StockMoves).unwrap(),
        s.changes().unwrap().len(),
    )
}

fn named(f: &Fixture, name: &str) -> arena_store::ProductRow {
    reopen(f)
        .products()
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
}

#[tokio::test]
async fn a_chunk_creates_products_updates_matches_and_books_opening_stock() {
    let f = bootstrapped().await;
    let cola = make(&f, up("Cola")).await;
    let mut tea = up("Tea");
    tea.barcode = "222".into();
    let tea = make(&f, tea).await;
    super::stock::stock_in(&f, &tea, 5, 100).await;
    let a = import(
        &f,
        owner(),
        vec![
            row("Juice", "111", 3500, 12),
            row("Cola", "", 4500, 0),
            row("Chai", "222", 1500, 10),
            ImportProductRow {
                track_stock: false,
                ..row("Service", "", 100, 0)
            },
        ],
    )
    .await
    .unwrap();
    let products = reopen(&f).products().unwrap();
    assert_eq!(
        products.len(),
        4,
        "Juice and Service are new; Cola and Tea→Chai are updated"
    );
    let juice = named(&f, "Juice");
    assert_eq!(
        (juice.price_minor, juice.barcode.as_deref(), juice.active),
        (3500, Some("111"), true)
    );
    assert_eq!(
        held(&f, &juice.product_id),
        (12, 600),
        "an opening lot at the unit cost"
    );
    let lots = reopen(&f).lots(&juice.product_id).unwrap();
    assert_eq!(
        (lots.len(), lots[0].qty_in, lots[0].expiry.clone()),
        (1, 12, None)
    );
    let moves = reopen(&f).moves(&juice.product_id).unwrap();
    assert_eq!(
        (moves[0].reason.as_str(), moves[0].supplier.as_str()),
        ("stock_in", "CSV import")
    );
    let cola_row = named(&f, "Cola");
    assert_eq!(
        (cola_row.product_id.as_str(), cola_row.price_minor),
        (cola.as_str(), 4500)
    );
    let chai = named(&f, "Chai");
    assert_eq!(
        chai.product_id, tea,
        "the barcode matched Tea, which took the file's name"
    );
    assert_eq!(
        held(&f, &tea).0,
        15,
        "opening stock adds a lot to what is held"
    );
    assert!(!named(&f, "Service").track_stock);
    assert!(a.changes.iter().any(|c| c.tbl == "products"));
}

#[tokio::test]
async fn a_retry_with_the_same_key_never_duplicates() {
    let f = bootstrapped().await;
    let it = intent(
        owner(),
        IntentPayload::ImportProducts(ImportProducts {
            rows: vec![row("Juice", "111", 3500, 12)],
        }),
    );
    let first = f.writer.submit_intent(it.clone()).await.unwrap();
    let before = state(&f);
    let again = f.writer.submit_intent(it).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(state(&f), before);
    assert_eq!(held(&f, &named(&f, "Juice").product_id).0, 12);
}

#[tokio::test]
async fn one_bad_row_fails_the_whole_chunk_and_writes_nothing() {
    let f = bootstrapped().await;
    make(&f, up("Cola")).await;
    let before = state(&f);
    for (rows, field) in [
        (
            vec![row("Juice", "", 3500, 12), row("", "", 3500, 0)],
            "rows[1].name",
        ),
        (
            vec![row("Juice", "", 3500, 12), row("Tea", "", -1, 0)],
            "rows[1].price",
        ),
        (
            vec![ImportProductRow {
                track_stock: false,
                ..row("Juice", "", 3500, 12)
            }],
            "rows[0].opening_qty",
        ),
        (
            vec![row("Juice", "9", 3500, 0), row("Tea", "9", 3500, 0)],
            "rows[1].barcode",
        ),
        (
            vec![row("Cola", "", 1, 0), row("Cola", "", 2, 0)],
            "rows[1].name",
        ),
        (Vec::new(), "rows"),
        (
            (0..501).map(|i| row(&format!("P{i}"), "", 1, 0)).collect(),
            "rows",
        ),
    ] {
        let e = import(&f, owner(), rows).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some(field)),
            "{e:?}"
        );
        assert_eq!(state(&f), before);
    }
}

#[tokio::test]
async fn catalog_edit_is_required_and_opening_stock_also_needs_stock_in() {
    let f = bootstrapped().await;
    let manager = staff(&f, "mgr", ROLE_MANAGER).await;
    import(&f, manager, vec![row("Juice", "", 3500, 3)])
        .await
        .unwrap();
    let counter = staff(&f, "ctr", crate::rbac::ROLE_COUNTER).await;
    let before = state(&f);
    let e = import(&f, counter, vec![row("Tea", "", 100, 0)])
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::RoleUpsert(RoleUpsert {
                role_id: None,
                name: "Cataloguer".into(),
                perms: vec![RolePerm {
                    permission: "catalog.edit".into(),
                    effect: PermEffect::Allow,
                }],
            }),
        ))
        .await
        .unwrap();
    let cataloguer = staff(&f, "cat", "Cataloguer").await;
    let before2 = state(&f);
    assert_ne!(before2, before);
    let e = import(&f, cataloguer.clone(), vec![row("Tea", "", 100, 4)])
        .await
        .unwrap_err();
    assert_eq!(
        e.code,
        ErrorCode::PermissionDenied,
        "opening stock needs stock.in"
    );
    assert_eq!(state(&f), before2);
    import(&f, cataloguer, vec![row("Tea", "", 100, 0)])
        .await
        .unwrap();
    assert_eq!(held(&f, &named(&f, "Tea").product_id).0, 0);
}

#[tokio::test]
async fn gst_branches_need_an_hsn_and_use_the_default_rate() {
    let f = bootstrapped().await;
    let mut r = row("Juice", "", 3500, 0);
    super::prepaid::sql(
        &f,
        format!("UPDATE settings SET value = '{GST_ON}' WHERE key = 'branch.tax'"),
    )
    .await;
    let e = import(&f, owner(), vec![r.clone()]).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::HsnRequired, Some("rows[0].hsn"))
    );
    r.hsn = "220210".into();
    import(&f, owner(), vec![r]).await.unwrap();
    let juice = named(&f, "Juice");
    assert_eq!((juice.hsn.as_str(), juice.tax_rate_bp), ("220210", 500));
}

#[tokio::test]
async fn a_thousand_row_file_previews_and_commits_in_two_chunks_well_inside_30_seconds() {
    let started = std::time::Instant::now();
    let f = bootstrapped().await;
    let mut csv = String::from(
        "name,category,price,tax_rate,hsn,barcode,track_stock,kitchen,opening_qty,unit_cost\r\n",
    );
    for i in 0..1000 {
        csv.push_str(&format!(
            "Item {i},Cat {},{}.50,5,,B{i:05},true,false,{},2.25\r\n",
            i % 10,
            10 + i % 90,
            1 + i % 7
        ));
    }
    let analysis = {
        let store = reopen(&f);
        let ctx = crate::import_products::context(&store).unwrap();
        crate::import_products::analyse(&csv, &ctx, 2).unwrap()
    };
    assert_eq!(analysis.preview.failures, Vec::new());
    assert_eq!(
        (analysis.preview.new, analysis.preview.with_stock),
        (1000, 1000)
    );
    let before = reopen(&f).products().unwrap().len();
    for chunk in analysis.rows.chunks(arena_proto::IMPORT_CHUNK_ROWS) {
        import(&f, owner(), chunk.to_vec()).await.unwrap();
    }
    assert_eq!(reopen(&f).products().unwrap().len(), before + 1000);
    let item = named(&f, "Item 999");
    assert_eq!(held(&f, &item.product_id), (1 + 999 % 7, 225));
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
    eprintln!(
        "1,000 rows previewed and committed in {:?}",
        started.elapsed()
    );
}
