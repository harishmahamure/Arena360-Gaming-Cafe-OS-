//! Story 7.2: kitchen notes on a kitchen product and on its order lines.

use super::prepaid::sql;
use super::session::{int, pay, pc, refusal, sessions, start, submit, table, text};
use super::stock::{intent, make, owner, staff, stock_in, up};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{DeviceKind, OrderPlace, PosSale, PosSaleLine, ProductUpsert};

async fn flags(f: &Fixture, value: &str) {
    sql(
        f,
        format!("UPDATE settings SET value = '{value}' WHERE key = 'licence.flags'"),
    )
    .await;
}

fn notes(n: &[&str]) -> Option<Vec<String>> {
    Some(n.iter().map(|s| (*s).to_owned()).collect())
}

fn tea(presets: &[&str]) -> ProductUpsert {
    ProductUpsert {
        price_minor: 4_000,
        kitchen: true,
        kitchen_notes: notes(presets),
        ..up("Tea")
    }
}

fn line(product: &str, qty: i64, n: &[&str]) -> PosSaleLine {
    PosSaleLine {
        product_id: product.into(),
        qty,
        kitchen_notes: n.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn place(session: &str, lines: Vec<PosSaleLine>) -> IntentPayload {
    IntentPayload::OrderPlace(OrderPlace {
        counter: None,
        session_id: session.into(),
        lines,
        stock_override: None,
    })
}

fn device(id: &str) -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: id.into(),
    }
}

async fn running(f: &Fixture) -> (String, String) {
    let d = pc(f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(f, &d, None).await.unwrap();
    (text(sessions(f).last().unwrap(), "id"), d)
}

async fn upsert(f: &Fixture, u: ProductUpsert) -> Result<Applied, AppError> {
    submit(f, owner(), IntentPayload::ProductUpsert(u)).await
}

fn product_notes(f: &Fixture, id: &str) -> Vec<String> {
    reopen(f).product(id).unwrap().unwrap().kitchen_notes
}

fn stored_notes(f: &Fixture, product: &str) -> Vec<Option<String>> {
    let mut lines: Vec<_> = table(f, Table::OrderLines)
        .into_iter()
        .filter(|l| matches!(&l["product_id"], RowValue::Text(t) if t == product))
        .collect();
    lines.sort_by_key(|l| int(l, "position"));
    lines
        .iter()
        .map(|l| match &l["kitchen_notes"] {
            RowValue::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_kitchen_product_saves_up_to_eight_trimmed_presets() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let id = make(&f, tea(&["  Less sugar ", "No ice"])).await;
    assert_eq!(product_notes(&f, &id), ["Less sugar", "No ice"]);

    let eight = ["a", "b", "c", "d", "e", "f", "g", "h"];
    make(&f, tea(&eight)).await;
    let nine = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
    let e = upsert(&f, tea(&nine)).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("kitchen_notes"))
    );
}

#[tokio::test]
async fn presets_are_distinct_short_text_on_a_kitchen_product() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let forty = "x".repeat(40);
    make(&f, tea(&[&forty])).await;
    for (u, code) in [
        (tea(&[&"x".repeat(41)]), ErrorCode::FieldTooLong),
        (tea(&["same", " same"]), ErrorCode::ValidationFailed),
        (tea(&["  "]), ErrorCode::ValidationFailed),
        (tea(&["two\nlines"]), ErrorCode::ValidationFailed),
        (
            ProductUpsert {
                kitchen: false,
                ..tea(&["No ice"])
            },
            ErrorCode::ValidationFailed,
        ),
    ] {
        let e = upsert(&f, u).await.unwrap_err();
        assert_eq!(e.code, code);
        assert_eq!(e.field.as_deref(), Some("kitchen_notes"));
    }
}

#[tokio::test]
async fn an_edit_without_a_list_keeps_it_and_an_empty_list_clears_it() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let id = make(&f, tea(&["No ice"])).await;
    let rev = reopen(&f).product(&id).unwrap().unwrap().rev;
    let mut i = intent(
        owner(),
        IntentPayload::ProductUpsert(ProductUpsert {
            id: Some(id.clone()),
            price_minor: 5_000,
            kitchen_notes: None,
            ..tea(&[])
        }),
    );
    i.base_rev = Some(rev);
    f.writer.submit_intent(i).await.unwrap();
    assert_eq!(product_notes(&f, &id), ["No ice"]);

    let rev = reopen(&f).product(&id).unwrap().unwrap().rev;
    let mut i = intent(
        owner(),
        IntentPayload::ProductUpsert(ProductUpsert {
            id: Some(id.clone()),
            ..tea(&[])
        }),
    );
    i.base_rev = Some(rev);
    f.writer.submit_intent(i).await.unwrap();
    assert!(product_notes(&f, &id).is_empty());
}

#[tokio::test]
async fn a_free_branch_refuses_a_preset_list_but_not_the_same_product_without_one() {
    let f = bootstrapped().await;
    let e = upsert(&f, tea(&["No ice"])).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::TierLocked);
    assert!(table(&f, Table::Products).is_empty());
    let id = make(
        &f,
        ProductUpsert {
            kitchen_notes: None,
            ..tea(&[])
        },
    )
    .await;
    assert!(product_notes(&f, &id).is_empty());
}

#[tokio::test]
async fn a_placement_stores_notes_without_changing_price_or_stock() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let id = make(&f, tea(&["Less sugar", "No ice"])).await;
    stock_in(&f, &id, 10, 1_000).await;
    let (session, _) = running(&f).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;

    submit(
        &f,
        counter,
        place(&session, vec![line(&id, 2, &["Less sugar", " Extra hot "])]),
    )
    .await
    .unwrap();
    let order = &table(&f, Table::Orders)[0];
    assert_eq!(int(order, "total_minor"), 8_000);
    assert_eq!(
        stored_notes(&f, &id),
        [Some(r#"["Less sugar","Extra hot"]"#.to_owned())]
    );
    // Held until served, as the same order without notes would leave it.
    assert_eq!(super::stock::held(&f, &id).0, 10);
}

#[tokio::test]
async fn lines_of_one_product_stay_apart_only_when_their_notes_differ() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let id = make(&f, tea(&["Less sugar", "No ice"])).await;
    stock_in(&f, &id, 10, 1_000).await;
    let (session, _) = running(&f).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;

    let (code, field) = refusal(
        submit(
            &f,
            counter.clone(),
            place(
                &session,
                vec![line(&id, 1, &["No ice"]), line(&id, 1, &["No ice"])],
            ),
        )
        .await,
    );
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("lines[1]"))
    );

    // The same set in another order, or padded, is the same line.
    let (code, field) = refusal(
        submit(
            &f,
            counter.clone(),
            place(
                &session,
                vec![
                    line(&id, 1, &["No ice", "Less sugar"]),
                    line(&id, 1, &[" Less sugar", "No ice"]),
                ],
            ),
        )
        .await,
    );
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("lines[1]"))
    );

    submit(
        &f,
        counter,
        place(
            &session,
            vec![
                line(&id, 1, &["No ice"]),
                line(&id, 2, &["Less sugar"]),
                line(&id, 1, &[]),
            ],
        ),
    )
    .await
    .unwrap();
    assert_eq!(stored_notes(&f, &id).len(), 3);
}

#[tokio::test]
async fn over_long_wrong_product_and_extra_free_notes_are_refused() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let id = make(&f, tea(&["No ice"])).await;
    let plain = make(
        &f,
        ProductUpsert {
            kitchen: false,
            kitchen_notes: None,
            ..tea(&[])
        },
    )
    .await;
    stock_in(&f, &id, 10, 1_000).await;
    stock_in(&f, &plain, 10, 1_000).await;
    let (session, _) = running(&f).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let sixty = "y".repeat(60);

    submit(
        &f,
        counter.clone(),
        place(&session, vec![line(&id, 1, &[&sixty])]),
    )
    .await
    .unwrap();
    for (lines, code) in [
        (
            vec![line(&id, 1, &[&"y".repeat(61)])],
            ErrorCode::FieldTooLong,
        ),
        (
            vec![line(&plain, 1, &["No ice"])],
            ErrorCode::ValidationFailed,
        ),
        (
            vec![line(&id, 1, &["one", "two"])],
            ErrorCode::ValidationFailed,
        ),
        (vec![line(&id, 1, &[" "])], ErrorCode::ValidationFailed),
    ] {
        let (c, field) = refusal(submit(&f, counter.clone(), place(&session, lines)).await);
        assert_eq!(c, code);
        assert_eq!(field.as_deref(), Some("lines[0].kitchen_notes"));
    }
}

#[tokio::test]
async fn a_free_branch_refuses_notes_but_accepts_the_same_order_without_them() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let id = make(&f, tea(&["No ice"])).await;
    stock_in(&f, &id, 10, 1_000).await;
    flags(&f, "").await;
    let (session, _) = running(&f).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;

    let (code, _) = refusal(
        submit(
            &f,
            counter.clone(),
            place(&session, vec![line(&id, 1, &["No ice"])]),
        )
        .await,
    );
    assert_eq!(code, ErrorCode::TierLocked);
    assert!(table(&f, Table::Orders).is_empty());
    submit(&f, counter, place(&session, vec![line(&id, 1, &[])]))
        .await
        .unwrap();
    assert_eq!(stored_notes(&f, &id), [None]);
}

#[tokio::test]
async fn a_device_may_toggle_presets_but_not_type_a_note() {
    let f = bootstrapped().await;
    flags(&f, "kitchen,kiosk_orders").await;
    let id = make(&f, tea(&["No ice"])).await;
    stock_in(&f, &id, 10, 1_000).await;
    let (session, d) = running(&f).await;

    let (code, field) = refusal(
        submit(
            &f,
            device(&d),
            place(&session, vec![line(&id, 1, &["Extra hot"])]),
        )
        .await,
    );
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("lines[0].kitchen_notes"))
    );
    submit(
        &f,
        device(&d),
        place(&session, vec![line(&id, 1, &["No ice"])]),
    )
    .await
    .unwrap();
    assert_eq!(stored_notes(&f, &id), [Some(r#"["No ice"]"#.to_owned())]);
}

#[tokio::test]
async fn a_counter_sale_stores_notes_on_its_line() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let id = make(&f, tea(&["No ice"])).await;
    stock_in(&f, &id, 10, 1_000).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let sale = PosSale {
        kiosk: None,
        business: None,
        coupon: None,
        discount: None,
        lines: vec![line(&id, 1, &["No ice", "Cut in half"])],
        payments: vec![pay(Cash, 4_000, None)],
        player_id: None,
        credit_override: None,
        stock_override: None,
        receipt: None,
    };
    submit(&f, counter, IntentPayload::PosSale(sale))
        .await
        .unwrap();
    assert_eq!(
        stored_notes(&f, &id),
        [Some(r#"["No ice","Cut in half"]"#.to_owned())]
    );
}
