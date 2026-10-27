use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::{GroupUpsert, RuleUpsert};

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

fn weekly(group: &str) -> RuleUpsert {
    RuleUpsert {
        id: None,
        name: "Happy hour".into(),
        group_id: group.into(),
        rule_kind: "weekly".into(),
        days_mask: Some(0b001_1111),
        start_min: Some(16 * 60),
        end_min: Some(18 * 60),
        date_from: None,
        date_to: None,
        adjust_kind: "percent_bp".into(),
        adjust_value: -2000,
        priority: 0,
        active: true,
    }
}

fn dated(group: &str) -> RuleUpsert {
    RuleUpsert {
        name: "Match day".into(),
        rule_kind: "date".into(),
        days_mask: None,
        start_min: None,
        end_min: None,
        date_from: Some("2027-03-10".into()),
        date_to: Some("2027-03-12".into()),
        adjust_kind: "override_minor".into(),
        adjust_value: 15_000,
        ..weekly(group)
    }
}

async fn save(
    f: &Fixture,
    actor: Actor,
    u: RuleUpsert,
    base_rev: Option<i64>,
) -> Result<Applied, AppError> {
    let mut i = intent(actor, IntentPayload::RuleUpsert(u));
    i.base_rev = base_rev;
    f.writer.submit_intent(i).await
}

fn rule_rows(f: &Fixture) -> usize {
    reopen(f).rows(Table::PricingRules).unwrap().len()
}

#[tokio::test]
async fn a_weekly_rule_and_a_date_rule_are_stored() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    save(&f, owner(), weekly(&g), None).await.unwrap();
    save(&f, owner(), dated(&g), None).await.unwrap();
    let rules = reopen(&f).pricing_rules().unwrap();
    assert_eq!(rules.len(), 2);
    let w = rules.iter().find(|r| r.name == "Happy hour").unwrap();
    assert_eq!(
        (
            w.rule_kind.as_str(),
            w.days_mask,
            w.start_min,
            w.end_min,
            w.adjust_kind.as_str(),
            w.adjust_value,
            w.group_id.clone(),
            w.date_from.clone()
        ),
        (
            "weekly",
            Some(31),
            Some(960),
            Some(1080),
            "percent_bp",
            -2000,
            g.clone(),
            None
        )
    );
    let d = rules.iter().find(|r| r.name == "Match day").unwrap();
    assert_eq!(
        (
            d.date_from.as_deref(),
            d.date_to.as_deref(),
            d.days_mask,
            d.adjust_value
        ),
        (Some("2027-03-10"), Some("2027-03-12"), None, 15_000)
    );
}

#[tokio::test]
async fn a_rule_may_wrap_midnight_and_a_single_date_is_a_range() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    save(
        &f,
        owner(),
        RuleUpsert {
            start_min: Some(22 * 60),
            end_min: Some(2 * 60),
            ..weekly(&g)
        },
        None,
    )
    .await
    .unwrap();
    save(
        &f,
        owner(),
        RuleUpsert {
            date_to: Some("2027-03-10".into()),
            ..dated(&g)
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(rule_rows(&f), 2);
}

#[tokio::test]
async fn bad_input_writes_nothing() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    let cases: Vec<(RuleUpsert, &str)> = vec![
        (
            RuleUpsert {
                days_mask: Some(0),
                ..weekly(&g)
            },
            "days_mask",
        ),
        (
            RuleUpsert {
                days_mask: Some(128),
                ..weekly(&g)
            },
            "days_mask",
        ),
        (
            RuleUpsert {
                days_mask: None,
                ..weekly(&g)
            },
            "days_mask",
        ),
        (
            RuleUpsert {
                end_min: Some(16 * 60),
                ..weekly(&g)
            },
            "end_min",
        ),
        (
            RuleUpsert {
                start_min: Some(1440),
                ..weekly(&g)
            },
            "start_min",
        ),
        (
            RuleUpsert {
                end_min: None,
                ..weekly(&g)
            },
            "start_min",
        ),
        (
            RuleUpsert {
                adjust_value: -10_001,
                ..weekly(&g)
            },
            "adjust_value",
        ),
        (
            RuleUpsert {
                adjust_kind: "override_minor".into(),
                adjust_value: 0,
                ..weekly(&g)
            },
            "adjust_value",
        ),
        (
            RuleUpsert {
                adjust_kind: "free".into(),
                ..weekly(&g)
            },
            "adjust_kind",
        ),
        (
            RuleUpsert {
                rule_kind: "monthly".into(),
                ..weekly(&g)
            },
            "rule_kind",
        ),
        (
            RuleUpsert {
                name: " ".into(),
                ..weekly(&g)
            },
            "name",
        ),
        (
            RuleUpsert {
                priority: 1001,
                ..weekly(&g)
            },
            "priority",
        ),
        (
            RuleUpsert {
                priority: -1,
                ..weekly(&g)
            },
            "priority",
        ),
        (
            RuleUpsert {
                date_from: Some("2027-03-10".into()),
                ..weekly(&g)
            },
            "date_from",
        ),
        (
            RuleUpsert {
                group_id: "nope".into(),
                ..weekly(&g)
            },
            "group_id",
        ),
        (
            RuleUpsert {
                date_from: Some("2027-03-13".into()),
                ..dated(&g)
            },
            "date_to",
        ),
        (
            RuleUpsert {
                date_from: Some("2027-3-10".into()),
                ..dated(&g)
            },
            "date_from",
        ),
        (
            RuleUpsert {
                date_to: Some("2027-02-30".into()),
                ..dated(&g)
            },
            "date_to",
        ),
        (
            RuleUpsert {
                date_to: Some("2028-03-10".into()),
                ..dated(&g)
            },
            "date_to",
        ),
        (
            RuleUpsert {
                date_from: None,
                ..dated(&g)
            },
            "date_from",
        ),
        (
            RuleUpsert {
                days_mask: Some(1),
                ..dated(&g)
            },
            "days_mask",
        ),
        (
            RuleUpsert {
                start_min: Some(60),
                ..dated(&g)
            },
            "days_mask",
        ),
    ];
    for (u, field) in cases {
        let e = save(&f, owner(), u.clone(), None).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some(field)),
            "{u:?}"
        );
    }
    assert_eq!(rule_rows(&f), 0);
}

#[tokio::test]
async fn a_discount_of_exactly_one_hundred_percent_is_allowed() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    save(
        &f,
        owner(),
        RuleUpsert {
            adjust_value: -10_000,
            ..weekly(&g)
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(rule_rows(&f), 1);
}

#[tokio::test]
async fn a_deleted_group_is_refused() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupDelete(arena_proto::GroupDelete { id: g.clone() }),
        ))
        .await
        .unwrap();
    let e = save(&f, owner(), weekly(&g), None).await.unwrap_err();
    assert_eq!(e.field.as_deref(), Some("group_id"));
    assert_eq!(rule_rows(&f), 0);
}

#[tokio::test]
async fn an_overlap_still_saves_and_is_reported_with_its_winner() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    let other = group(&f, "VIP").await;
    save(&f, owner(), weekly(&g), None).await.unwrap();
    save(
        &f,
        owner(),
        RuleUpsert {
            name: "Early bird".into(),
            start_min: Some(17 * 60),
            end_min: Some(20 * 60),
            priority: 3,
            ..weekly(&g)
        },
        None,
    )
    .await
    .unwrap();
    save(
        &f,
        owner(),
        RuleUpsert {
            name: "VIP hour".into(),
            ..weekly(&other)
        },
        None,
    )
    .await
    .unwrap();
    let rules = reopen(&f).pricing_rules().unwrap();
    let found = crate::pricing::conflicts(&rules);
    let of = |name: &str| {
        let i = rules.iter().position(|r| r.name == name).unwrap();
        found[i]
            .iter()
            .map(|c| (c.name.as_str(), c.other_wins))
            .collect::<Vec<_>>()
    };
    assert_eq!(of("Happy hour"), vec![("Early bird", true)]);
    assert_eq!(of("Early bird"), vec![("Happy hour", false)]);
    assert!(of("VIP hour").is_empty());
}

#[tokio::test]
async fn an_edit_carries_base_rev() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    let created = save(&f, owner(), weekly(&g), None).await.unwrap();
    let id = created.changes[0].row_id.clone();
    let manager = staff(&f, "boss", ROLE_MANAGER).await;
    let edit = RuleUpsert {
        id: Some(id),
        name: "Late happy hour".into(),
        end_min: Some(19 * 60),
        ..weekly(&g)
    };
    let e = save(&f, manager.clone(), edit.clone(), None)
        .await
        .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("base_rev"));
    let e = save(&f, manager.clone(), edit.clone(), Some(99))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    let rev = reopen(&f).pricing_rules().unwrap()[0].rev;
    save(&f, manager, edit, Some(rev)).await.unwrap();
    let r = reopen(&f).pricing_rules().unwrap().remove(0);
    assert_eq!(
        (r.name.as_str(), r.end_min, r.rev),
        ("Late happy hour", Some(1140), rev + 1)
    );
    assert_eq!(rule_rows(&f), 1);
}

#[tokio::test]
async fn an_edit_can_move_a_rule_to_another_group_and_to_a_date() {
    let f = bootstrapped().await;
    let a = group(&f, "Basic").await;
    let b = group(&f, "VIP").await;
    let created = save(&f, owner(), weekly(&a), None).await.unwrap();
    let id = created.changes[0].row_id.clone();
    save(
        &f,
        owner(),
        RuleUpsert {
            id: Some(id),
            ..dated(&b)
        },
        None,
    )
    .await
    .unwrap();
    let r = reopen(&f).pricing_rules().unwrap().remove(0);
    assert_eq!(
        (r.group_id, r.rule_kind, r.days_mask, r.date_from),
        (b, "date".to_owned(), None, Some("2027-03-10".to_owned()))
    );
}

#[tokio::test]
async fn naming_a_missing_rule_is_refused() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    let e = save(
        &f,
        owner(),
        RuleUpsert {
            id: Some("missing".into()),
            ..weekly(&g)
        },
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("id"));
    assert_eq!(rule_rows(&f), 0);
}

#[tokio::test]
async fn a_deactivated_rule_stays_listed_and_can_return() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    let created = save(&f, owner(), weekly(&g), None).await.unwrap();
    let id = created.changes[0].row_id.clone();
    let off = RuleUpsert {
        id: Some(id),
        active: false,
        ..weekly(&g)
    };
    save(&f, owner(), off.clone(), None).await.unwrap();
    assert!(!reopen(&f).pricing_rules().unwrap()[0].active);
    save(
        &f,
        owner(),
        RuleUpsert {
            active: true,
            ..off
        },
        None,
    )
    .await
    .unwrap();
    assert!(reopen(&f).pricing_rules().unwrap()[0].active);
}

#[tokio::test]
async fn a_manager_edits_rules_and_a_counter_cannot() {
    let f = bootstrapped().await;
    let g = group(&f, "Basic").await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let e = save(&f, counter, weekly(&g), None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(rule_rows(&f), 0);
    let manager = staff(&f, "boss", ROLE_MANAGER).await;
    save(&f, manager, weekly(&g), None).await.unwrap();
    assert_eq!(rule_rows(&f), 1);
}
