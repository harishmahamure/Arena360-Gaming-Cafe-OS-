use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::{GroupDelete, GroupUpsert, RateUpsert};

fn rate(group: &str, price: i64) -> IntentPayload {
    IntentPayload::RateUpsert(RateUpsert {
        group_id: group.into(),
        price_per_hour_minor: price,
    })
}

async fn group(f: &Fixture, name: &str) -> String {
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupUpsert(GroupUpsert {
                id: None,
                name: name.into(),
                group_kind: "pc".into(),
                colour: "teal".into(),
                sort_order: 1,
            }),
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone()
}

async fn set(f: &Fixture, actor: Actor, group: &str, price: i64) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(actor, rate(group, price)))
        .await
}

fn rows(f: &Fixture) -> Vec<(i64, i64)> {
    reopen(f)
        .rows(Table::Rates)
        .unwrap()
        .iter()
        .map(|r| match (&r["price_per_hour_minor"], &r["active"]) {
            (RowValue::Integer(p), RowValue::Integer(a)) => (*p, *a),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[tokio::test]
async fn a_new_rate_replaces_the_active_one_and_keeps_history() {
    let f = bootstrapped().await;
    let g = group(&f, "PCs").await;
    assert_eq!(reopen(&f).groups_without_rate().unwrap(), 1);

    let first = set(&f, owner(), &g, 12_000).await.unwrap();
    assert_eq!(first.changes.len(), 1);
    assert_eq!(rows(&f), [(12_000, 1)]);
    assert_eq!(reopen(&f).groups_without_rate().unwrap(), 0);

    let second = set(&f, owner(), &g, 15_000).await.unwrap();
    assert_eq!(second.changes.len(), 2);
    let mut kept = rows(&f);
    kept.sort();
    assert_eq!(kept, [(12_000, 0), (15_000, 1)]);
    let active = reopen(&f).rates().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].price_per_hour_minor, 15_000);
}

#[tokio::test]
async fn bad_prices_and_groups_write_nothing() {
    let f = bootstrapped().await;
    let g = group(&f, "PCs").await;
    for price in [0, -5, 1_000_000_000_000] {
        let e = set(&f, owner(), &g, price).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("price_per_hour_minor"))
        );
    }
    let e = set(&f, owner(), "nope", 100).await.unwrap_err();
    assert_eq!(e.field.as_deref(), Some("group_id"));
    assert!(rows(&f).is_empty());

    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupDelete(GroupDelete { id: g.clone() }),
        ))
        .await
        .unwrap();
    let e = set(&f, owner(), &g, 100).await.unwrap_err();
    assert_eq!(e.field.as_deref(), Some("group_id"));
    assert!(rows(&f).is_empty());
}

#[tokio::test]
async fn a_manager_sets_a_rate_and_a_counter_cannot() {
    let f = bootstrapped().await;
    let g = group(&f, "PCs").await;
    let manager = staff(&f, "boss", ROLE_MANAGER).await;
    set(&f, manager, &g, 9_000).await.unwrap();
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let e = set(&f, counter, &g, 1).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(rows(&f), [(9_000, 1)]);
}

#[tokio::test]
async fn the_snapshot_carries_each_groups_rate() {
    let f = bootstrapped().await;
    let rated = group(&f, "PCs").await;
    let bare = group(&f, "VR").await;
    set(&f, owner(), &rated, 12_000).await.unwrap();
    let store = reopen(&f);
    let snap = crate::read_snapshot(&store, T0).unwrap();
    let rate_of = |id: &str| snap.groups.iter().find(|g| g.id == id).unwrap().rate_minor;
    assert_eq!((rate_of(&rated), rate_of(&bare)), (Some(12_000), None));
    assert_eq!(snap.setup.unwrap().groups_without_rate, 1);
}
