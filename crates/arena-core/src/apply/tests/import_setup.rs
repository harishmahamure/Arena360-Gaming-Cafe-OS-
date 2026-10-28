use super::plans::group;
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::{
    ImportSetup, SetupBundle, SetupComponent, SetupCoupon, SetupCouponTarget, SetupFile,
    SetupFileGroup, SetupPlan, SetupProduct, SetupRule,
};

const NOW: i64 = 1_700_000_000_000;

/// An empty file in the branch's currency (what a bare branch exports).
fn blank(f: &Fixture) -> SetupFile {
    reopen(f)
        .read_with(|c| crate::setup_file::export(c, NOW))
        .unwrap()
}

fn full(f: &Fixture) -> SetupFile {
    SetupFile {
        groups: vec![
            SetupFileGroup {
                name: "PC".into(),
                kind: "pc".into(),
                colour: "teal".into(),
                sort_order: 1,
                rate_minor: Some(6_000),
            },
            SetupFileGroup {
                name: "PS5".into(),
                kind: "console".into(),
                colour: "orange".into(),
                sort_order: 2,
                rate_minor: None,
            },
        ],
        products: vec![
            SetupProduct {
                name: "Cola".into(),
                category: "Drinks".into(),
                price_minor: 4_000,
                tax_rate_bp: None,
                hsn: "2202".into(),
                track_stock: true,
                barcode: "890123".into(),
                kitchen: false,
                favourite: true,
                active: true,
            },
            SetupProduct {
                name: "Chips".into(),
                category: "Snacks".into(),
                price_minor: 2_000,
                tax_rate_bp: None,
                hsn: "2005".into(),
                track_stock: true,
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active: true,
            },
        ],
        bundles: vec![SetupBundle {
            name: "Combo".into(),
            category: "Combos".into(),
            price_minor: 5_000,
            tax_rate_bp: None,
            hsn: "2106".into(),
            barcode: String::new(),
            kitchen: false,
            favourite: false,
            active: true,
            components: vec![
                SetupComponent {
                    product: "Cola".into(),
                    qty: 2,
                },
                SetupComponent {
                    product: "Chips".into(),
                    qty: 1,
                },
            ],
        }],
        plans: vec![SetupPlan {
            name: "2 hour pack".into(),
            plan_kind: "pack".into(),
            price_minor: 10_000,
            minutes: Some(120),
            validity_days: Some(30),
            window_start_min: None,
            window_end_min: None,
            groups: vec!["PC".into()],
            active: true,
        }],
        rules: vec![SetupRule {
            name: "Happy hour".into(),
            group: "PC".into(),
            rule_kind: "weekly".into(),
            days_mask: Some(0b001_1111),
            start_min: Some(16 * 60),
            end_min: Some(18 * 60),
            date_from: None,
            date_to: None,
            adjust_kind: "percent_bp".into(),
            adjust_value: -2_000,
            priority: 0,
            active: true,
        }],
        coupons: vec![SetupCoupon {
            code: "SAVE10".into(),
            percent_bp: Some(1_000),
            amount_minor: None,
            valid_from: None,
            valid_to: None,
            usage_limit: None,
            min_bill_minor: 0,
            targets: vec![
                SetupCouponTarget {
                    kind: "plan".into(),
                    name: "2 hour pack".into(),
                },
                SetupCouponTarget {
                    kind: "product".into(),
                    name: "Combo".into(),
                },
            ],
            active: true,
        }],
        ..blank(f)
    }
}

async fn import(f: &Fixture, actor: Actor, file: SetupFile) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(
            actor,
            IntentPayload::ImportSetup(ImportSetup { file }),
        ))
        .await
}

/// Row counts of every table a setup file writes, and the change log's length.
fn state(f: &Fixture) -> usize {
    let s = reopen(f);
    [
        Table::DeviceGroups,
        Table::Rates,
        Table::Products,
        Table::BundleComponents,
        Table::Plans,
        Table::PlanGroups,
        Table::PricingRules,
        Table::Coupons,
        Table::CouponTargets,
    ]
    .into_iter()
    .map(|t| s.rows(t).unwrap().len())
    .sum::<usize>()
        + s.changes().unwrap().len()
}

#[tokio::test]
async fn a_file_creates_groups_rates_products_bundles_plans_rules_and_coupons_by_name() {
    let f = bootstrapped().await;
    let a = import(&f, owner(), full(&f)).await.unwrap();
    let tables: std::collections::HashSet<&str> =
        a.changes.iter().map(|c| c.tbl.as_str()).collect();
    for t in [
        "device_groups",
        "rates",
        "products",
        "bundle_components",
        "plans",
        "plan_groups",
        "pricing_rules",
        "coupons",
        "coupon_targets",
    ] {
        assert!(tables.contains(t), "{t} was written: {tables:?}");
    }
    let copy = blank(&f);
    let mut original = full(&f);
    // The Hub fills in the branch's product tax rate and lists components by name.
    original.bundles[0].tax_rate_bp = Some(0);
    original.bundles[0].components.reverse();
    assert_eq!(copy.groups, original.groups);
    assert_eq!(copy.bundles, original.bundles);
    assert_eq!(copy.plans, original.plans);
    assert_eq!(copy.rules, original.rules);
    assert_eq!(copy.coupons, original.coupons);
    assert_eq!(copy.products.len(), 2);
}

#[tokio::test]
async fn what_exists_by_name_is_left_alone_and_the_rest_is_added() {
    let f = bootstrapped().await;
    // "pc" is "PC" without case: it is kept as it is, and takes the file's rate.
    let pc = group(&f, "pc", "pc").await;
    let a = import(&f, owner(), full(&f)).await.unwrap();
    let s = reopen(&f);
    let groups = s.groups().unwrap();
    assert_eq!(groups.len(), 2, "only PS5 was added");
    assert_eq!(groups.iter().find(|g| g.name == "pc").unwrap().group_id, pc);
    assert_eq!(s.rates().unwrap().len(), 1);
    let (plan, plan_groups) = s.plans().unwrap().remove(0);
    assert_eq!((plan.name.as_str(), plan_groups), ("2 hour pack", vec![pc]));
    assert!(a.changes.iter().any(|c| c.tbl == "pricing_rules"));
}

#[tokio::test]
async fn a_file_with_nothing_new_or_the_wrong_currency_changes_nothing() {
    let f = bootstrapped().await;
    import(&f, owner(), full(&f)).await.unwrap();
    let before = state(&f);
    for (file, field) in [
        (full(&f), "file"),
        (blank(&f), "file"),
        (
            SetupFile {
                currency: "USD".into(),
                ..full(&f)
            },
            "currency",
        ),
        (
            SetupFile {
                format: "something-else".into(),
                ..full(&f)
            },
            "file",
        ),
    ] {
        let e = import(&f, owner(), file).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some(field)),
            "{e:?}"
        );
        assert_eq!(state(&f), before);
    }
    let e = import(
        &f,
        owner(),
        SetupFile {
            version: 2,
            ..full(&f)
        },
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported);
    assert_eq!(state(&f), before);
}

#[tokio::test]
async fn entries_that_break_a_rule_or_point_at_nothing_are_skipped_not_applied() {
    let f = bootstrapped().await;
    let mut file = full(&f);
    file.plans.push(SetupPlan {
        name: "Ghost pack".into(),
        groups: vec!["Nowhere".into()],
        ..file.plans[0].clone()
    });
    file.bundles.push(SetupBundle {
        name: "Broken".into(),
        components: vec![SetupComponent {
            product: "Missing".into(),
            qty: 1,
        }],
        ..file.bundles[0].clone()
    });
    file.rules.push(SetupRule {
        name: "Too eager".into(),
        priority: 5_000,
        ..file.rules[0].clone()
    });
    import(&f, owner(), file).await.unwrap();
    let s = reopen(&f);
    assert_eq!(s.plans().unwrap().len(), 1);
    assert_eq!(s.pricing_rules().unwrap().len(), 1);
    assert_eq!(
        s.products().unwrap().iter().filter(|p| p.bundle).count(),
        1,
        "only the good bundle"
    );
}

#[tokio::test]
async fn a_retry_with_the_same_key_never_duplicates() {
    let f = bootstrapped().await;
    let it = intent(
        owner(),
        IntentPayload::ImportSetup(ImportSetup { file: full(&f) }),
    );
    let first = f.writer.submit_intent(it.clone()).await.unwrap();
    let before = state(&f);
    let again = f.writer.submit_intent(it).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(state(&f), before);
}

#[tokio::test]
async fn catalog_edit_is_required() {
    let f = bootstrapped().await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let before = state(&f);
    let e = import(&f, counter, full(&f)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(state(&f), before);
    let manager = staff(&f, "mgr", ROLE_MANAGER).await;
    import(&f, manager, full(&f)).await.unwrap();
    assert_eq!(reopen(&f).groups().unwrap().len(), 2);
}
