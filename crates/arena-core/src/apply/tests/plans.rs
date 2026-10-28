use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::{GroupUpsert, PlanUpsert};

pub(super) async fn group(f: &Fixture, name: &str, kind: &str) -> String {
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupUpsert(GroupUpsert {
                id: None,
                name: name.into(),
                group_kind: kind.into(),
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

fn pack(groups: &[&str]) -> PlanUpsert {
    PlanUpsert {
        id: None,
        plan_kind: "pack".into(),
        name: "5 h".into(),
        price_minor: 30_000,
        minutes: Some(300),
        validity_days: Some(7),
        window_start_min: None,
        window_end_min: None,
        group_ids: groups.iter().map(|g| (*g).to_owned()).collect(),
        active: true,
    }
}

fn block(groups: &[&str]) -> PlanUpsert {
    PlanUpsert {
        plan_kind: "block".into(),
        name: "Night block".into(),
        price_minor: 20_000,
        minutes: None,
        validity_days: None,
        window_start_min: Some(22 * 60),
        window_end_min: Some(6 * 60),
        ..pack(groups)
    }
}

fn pass(groups: &[&str]) -> PlanUpsert {
    PlanUpsert {
        plan_kind: "pass".into(),
        name: "Night pass".into(),
        price_minor: 90_000,
        validity_days: Some(30),
        ..block(groups)
    }
}

async fn save(
    f: &Fixture,
    actor: Actor,
    u: PlanUpsert,
    base_rev: Option<i64>,
) -> Result<Applied, AppError> {
    let mut i = intent(actor, IntentPayload::PlanUpsert(u));
    i.base_rev = base_rev;
    f.writer.submit_intent(i).await
}

fn plan_rows(f: &Fixture) -> usize {
    reopen(f).rows(Table::Plans).unwrap().len()
}

fn group_rows(f: &Fixture) -> Vec<String> {
    let mut v: Vec<_> = reopen(f)
        .rows(Table::PlanGroups)
        .unwrap()
        .iter()
        .map(|r| match &r["group_id"] {
            RowValue::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn a_pack_a_block_and_a_pass_are_stored_with_their_groups() {
    let f = bootstrapped().await;
    let basic = group(&f, "Basic", "pc").await;
    let vip = group(&f, "VIP", "pc").await;
    let a = save(&f, owner(), pack(&[&basic]), None).await.unwrap();
    assert_eq!(a.changes.len(), 2);
    save(&f, owner(), block(&[&basic, &vip]), None)
        .await
        .unwrap();
    save(&f, owner(), pass(&[&vip]), None).await.unwrap();
    let listed = reopen(&f).plans().unwrap();
    assert_eq!(listed.len(), 3);
    let by = |name: &str| listed.iter().find(|(p, _)| p.name == name).unwrap();
    let (p, g) = by("5 h");
    assert_eq!(
        (p.minutes, p.validity_days, p.price_minor, g.clone()),
        (Some(300), Some(7), 30_000, vec![basic.clone()])
    );
    let (b, g) = by("Night block");
    assert_eq!(
        (
            b.window_start_min,
            b.window_end_min,
            b.validity_days,
            g.len()
        ),
        (Some(1320), Some(360), None, 2)
    );
    let (n, _) = by("Night pass");
    assert_eq!(
        (n.validity_days, n.window_start_min),
        (Some(30), Some(1320))
    );
    assert_eq!(group_rows(&f).len(), 4);
}

#[tokio::test]
async fn bad_terms_write_nothing() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic", "pc").await;
    let cases: Vec<(PlanUpsert, &str)> = vec![
        (
            PlanUpsert {
                price_minor: 0,
                ..pack(&[&g])
            },
            "price_minor",
        ),
        (
            PlanUpsert {
                price_minor: -1,
                ..pack(&[&g])
            },
            "price_minor",
        ),
        (
            PlanUpsert {
                name: "  ".into(),
                ..pack(&[&g])
            },
            "name",
        ),
        (
            PlanUpsert {
                plan_kind: "bundle".into(),
                ..pack(&[&g])
            },
            "plan_kind",
        ),
        (
            PlanUpsert {
                minutes: Some(0),
                ..pack(&[&g])
            },
            "minutes",
        ),
        (
            PlanUpsert {
                minutes: Some(-60),
                ..pack(&[&g])
            },
            "minutes",
        ),
        (
            PlanUpsert {
                minutes: None,
                ..pack(&[&g])
            },
            "minutes",
        ),
        (
            PlanUpsert {
                validity_days: Some(0),
                ..pack(&[&g])
            },
            "validity_days",
        ),
        (
            PlanUpsert {
                validity_days: None,
                ..pack(&[&g])
            },
            "validity_days",
        ),
        (
            PlanUpsert {
                window_start_min: Some(0),
                window_end_min: Some(60),
                ..pack(&[&g])
            },
            "window_start_min",
        ),
        (
            PlanUpsert {
                window_end_min: Some(1320),
                ..block(&[&g])
            },
            "window_end_min",
        ),
        (
            PlanUpsert {
                window_start_min: None,
                ..block(&[&g])
            },
            "window_start_min",
        ),
        (
            PlanUpsert {
                window_start_min: Some(1440),
                ..block(&[&g])
            },
            "window_start_min",
        ),
        (
            PlanUpsert {
                minutes: Some(60),
                ..block(&[&g])
            },
            "minutes",
        ),
        (
            PlanUpsert {
                window_end_min: Some(1320),
                ..pass(&[&g])
            },
            "window_end_min",
        ),
        (
            PlanUpsert {
                validity_days: None,
                ..pass(&[&g])
            },
            "validity_days",
        ),
        (pack(&[]), "group_ids"),
        (pack(&[&g, &g]), "group_ids"),
        (pack(&["nope"]), "group_ids"),
    ];
    for (u, field) in cases {
        let e = save(&f, owner(), u.clone(), None).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some(field)),
            "{u:?}"
        );
    }
    assert_eq!(plan_rows(&f), 0);
    assert!(group_rows(&f).is_empty());
}

#[tokio::test]
async fn console_and_pc_groups_never_share_a_plan() {
    let f = bootstrapped().await;
    let pc = group(&f, "Basic", "pc").await;
    let tv = group(&f, "PS5", "console").await;
    let tv2 = group(&f, "Switch", "console").await;
    let e = save(&f, owner(), pack(&[&pc, &tv]), None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::GroupKindMismatch, Some("group_ids"))
    );
    assert_eq!(plan_rows(&f), 0);
    save(&f, owner(), pack(&[&tv, &tv2]), None).await.unwrap();
    save(&f, owner(), pack(&[&pc]), None).await.unwrap();
    assert_eq!(plan_rows(&f), 2);
}

#[tokio::test]
async fn a_deleted_group_is_not_offered() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic", "pc").await;
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupDelete(arena_proto::GroupDelete { id: g.clone() }),
        ))
        .await
        .unwrap();
    let e = save(&f, owner(), pack(&[&g]), None).await.unwrap_err();
    assert_eq!(e.field.as_deref(), Some("group_ids"));
    assert_eq!(plan_rows(&f), 0);
}

#[tokio::test]
async fn an_edit_carries_base_rev_and_replaces_the_groups() {
    let f = bootstrapped().await;
    let a = group(&f, "Basic", "pc").await;
    let b = group(&f, "VIP", "pc").await;
    let created = save(&f, owner(), pack(&[&a]), None).await.unwrap();
    let id = created.changes[0].row_id.clone();
    let manager = staff(&f, "boss", ROLE_MANAGER).await;

    let edit = PlanUpsert {
        id: Some(id.clone()),
        name: "6 h".into(),
        minutes: Some(360),
        ..pack(&[&b])
    };
    let e = save(&f, manager.clone(), edit.clone(), None)
        .await
        .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("base_rev"));
    let e = save(&f, manager.clone(), edit.clone(), Some(99))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    assert_eq!(group_rows(&f), vec![a.clone()]);

    let rev = reopen(&f).plans().unwrap()[0].0.rev;
    let applied = save(&f, manager.clone(), edit, Some(rev)).await.unwrap();
    assert_eq!(applied.changes.len(), 3);
    let (p, groups) = reopen(&f).plans().unwrap().remove(0);
    assert_eq!(
        (p.name.as_str(), p.minutes, p.rev, groups),
        ("6 h", Some(360), rev + 1, vec![b.clone()])
    );
    assert_eq!(plan_rows(&f), 1);
}

#[tokio::test]
async fn an_edit_cannot_change_the_kind_or_name_a_missing_plan() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic", "pc").await;
    let created = save(&f, owner(), pack(&[&g]), None).await.unwrap();
    let id = created.changes[0].row_id.clone();
    let e = save(
        &f,
        owner(),
        PlanUpsert {
            id: Some(id),
            ..pass(&[&g])
        },
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("plan_kind"));
    let e = save(
        &f,
        owner(),
        PlanUpsert {
            id: Some("missing".into()),
            ..pack(&[&g])
        },
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("id"));
    assert_eq!(plan_rows(&f), 1);
}

#[tokio::test]
async fn a_deactivated_plan_stays_listed_and_can_return() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic", "pc").await;
    let created = save(&f, owner(), pack(&[&g]), None).await.unwrap();
    let id = created.changes[0].row_id.clone();
    let off = PlanUpsert {
        id: Some(id.clone()),
        active: false,
        ..pack(&[&g])
    };
    let applied = save(&f, owner(), off.clone(), None).await.unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert!(!reopen(&f).plans().unwrap()[0].0.active);
    save(
        &f,
        owner(),
        PlanUpsert {
            active: true,
            ..off
        },
        None,
    )
    .await
    .unwrap();
    assert!(reopen(&f).plans().unwrap()[0].0.active);
}

#[tokio::test]
async fn a_manager_edits_plans_and_a_counter_cannot() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic", "pc").await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let e = save(&f, counter, pack(&[&g]), None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(plan_rows(&f), 0);
    let manager = staff(&f, "boss", ROLE_MANAGER).await;
    save(&f, manager, pack(&[&g]), None).await.unwrap();
    assert_eq!(plan_rows(&f), 1);
}
