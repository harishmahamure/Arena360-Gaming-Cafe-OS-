//! Starter templates (Story 5.35): a ready-made menu and plan list for a kind of café, offered
//! under Settings → Import → Templates.
//!
//! A template is plain data. The dry-run turns it into the same rows a CSV would give
//! ([`ImportPlanRow`], [`ImportProductRow`]) and checks them with the rules the CSV imports use, so
//! a commit is nothing but an `import.products` and an `import.plans` intent. Plans are mapped to
//! the live device groups of their kind (PC, console, VR); a plan with no such group, a plan whose
//! name is taken, and a product whose name or barcode is taken are *skipped*, which also makes
//! applying a template twice add nothing. Prices are written in major units and scaled to the
//! branch currency; products leave the tax rate empty, so each takes the branch's product rate
//! (Story 5.1). Nothing here writes.

use std::collections::HashSet;

use arena_proto::{
    AppError, ErrorCode, ImportPlanRow, ImportProductRow, PLAN_MAX_GROUPS, StarterItem,
    StarterPlanInfo, StarterPreview, StarterProductInfo, StarterTemplateInfo,
};

use crate::import_plans::{PlanContext, check_row as check_plan};
use crate::import_products::{ImportContext, check_row as check_product, resolve};

/// A plan of a template. Prices are major units of the branch currency; `minutes` is for a pack,
/// the window (minutes after midnight) for a block or a pass.
struct PlanSeed {
    name: &'static str,
    kind: &'static str,
    price: i64,
    minutes: Option<i64>,
    validity_days: Option<i64>,
    window: Option<(i64, i64)>,
}

/// A product of a template.
struct ProductSeed {
    name: &'static str,
    category: &'static str,
    price: i64,
    hsn: &'static str,
    track_stock: bool,
    kitchen: bool,
}

struct Template {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    /// The kind of device group the plans are offered on.
    group_kind: &'static str,
    plans: &'static [PlanSeed],
    products: &'static [ProductSeed],
}

const fn pack(name: &'static str, price: i64, minutes: i64, validity_days: i64) -> PlanSeed {
    PlanSeed {
        name,
        kind: "pack",
        price,
        minutes: Some(minutes),
        validity_days: Some(validity_days),
        window: None,
    }
}

const fn block(name: &'static str, price: i64, start: i64, end: i64) -> PlanSeed {
    PlanSeed {
        name,
        kind: "block",
        price,
        minutes: None,
        validity_days: None,
        window: Some((start * 60, end * 60)),
    }
}

const fn pass(
    name: &'static str,
    price: i64,
    validity_days: i64,
    start: i64,
    end: i64,
) -> PlanSeed {
    PlanSeed {
        name,
        kind: "pass",
        price,
        minutes: None,
        validity_days: Some(validity_days),
        window: Some((start * 60, end * 60)),
    }
}

const fn item(
    name: &'static str,
    category: &'static str,
    price: i64,
    hsn: &'static str,
    track_stock: bool,
    kitchen: bool,
) -> ProductSeed {
    ProductSeed {
        name,
        category,
        price,
        hsn,
        track_stock,
        kitchen,
    }
}

const TEMPLATES: [Template; 3] = [
    Template {
        id: "indian-cafe",
        name: "Indian café",
        description: "PC gaming hours and a chai-and-snacks menu.",
        group_kind: "pc",
        plans: &[
            pack("PC 1 hour pack", 60, 60, 30),
            pack("PC 3 hour pack", 150, 180, 30),
            pack("PC 10 hour pack", 450, 600, 90),
            block("PC morning block", 200, 6, 12),
            pass("PC night pass", 500, 30, 22, 6),
        ],
        products: &[
            item("Masala chai", "Drinks", 20, "0902", false, true),
            item("Cold coffee", "Drinks", 60, "2101", false, true),
            item("Bottled water", "Drinks", 20, "2201", true, false),
            item("Cola 500 ml", "Drinks", 40, "2202", true, false),
            item("Samosa", "Snacks", 20, "2106", false, true),
            item("Veg sandwich", "Snacks", 60, "2106", false, true),
            item("Masala maggi", "Snacks", 50, "1902", false, true),
            item("French fries", "Snacks", 70, "2004", false, true),
            item("Potato chips", "Snacks", 20, "2005", true, false),
        ],
    },
    Template {
        id: "console-lounge",
        name: "Console lounge",
        description: "Console hours by the hour, pack, block and pass, with drinks and bites.",
        group_kind: "console",
        plans: &[
            pack("Console 1 hour pack", 120, 60, 30),
            pack("Console 3 hour pack", 330, 180, 30),
            pack("Console 10 hour pack", 1000, 600, 90),
            block("Console evening block", 450, 18, 22),
            pass("Console weekend pass", 1500, 30, 10, 22),
        ],
        products: &[
            item("Bottled water", "Drinks", 20, "2201", true, false),
            item("Cola 500 ml", "Drinks", 40, "2202", true, false),
            item("Energy drink", "Drinks", 110, "2202", true, false),
            item("Cold coffee", "Drinks", 60, "2101", false, true),
            item("Nachos with cheese", "Snacks", 120, "2106", false, true),
            item("Veg sandwich", "Snacks", 60, "2106", false, true),
            item("Potato chips", "Snacks", 20, "2005", true, false),
        ],
    },
    Template {
        id: "vr-arcade",
        name: "VR arcade",
        description: "VR sessions by the half hour and hour, with packs and a weekend pass.",
        group_kind: "vr",
        plans: &[
            pack("VR 30 minute pack", 200, 30, 30),
            pack("VR 1 hour pack", 350, 60, 30),
            pack("VR 5 hour pack", 1500, 300, 90),
            block("VR afternoon block", 600, 12, 17),
            pass("VR weekend pass", 1800, 30, 10, 22),
        ],
        products: &[
            item("Bottled water", "Drinks", 20, "2201", true, false),
            item("Energy drink", "Drinks", 110, "2202", true, false),
            item("Cold coffee", "Drinks", 60, "2101", false, true),
            item("Popcorn", "Snacks", 80, "1904", false, true),
            item("Nachos with cheese", "Snacks", 120, "2106", false, true),
        ],
    },
];

fn minor(price: i64, exponent: u8) -> i64 {
    price * 10i64.pow(u32::from(exponent))
}

fn find(id: &str) -> Result<&'static Template, AppError> {
    TEMPLATES.iter().find(|t| t.id == id).ok_or_else(|| {
        AppError::with_field(
            ErrorCode::ValidationFailed,
            "template_id",
            "there is no starter template with this id",
        )
    })
}

/// The templates and what each adds, priced in the branch currency (`exponent` decimal places).
pub fn templates(exponent: u8) -> Vec<StarterTemplateInfo> {
    TEMPLATES
        .iter()
        .map(|t| StarterTemplateInfo {
            id: t.id.to_owned(),
            name: t.name.to_owned(),
            description: t.description.to_owned(),
            plans: t
                .plans
                .iter()
                .map(|p| StarterPlanInfo {
                    name: p.name.to_owned(),
                    plan_kind: p.kind.to_owned(),
                    group_kind: t.group_kind.to_owned(),
                    price_minor: minor(p.price, exponent),
                })
                .collect(),
            products: t
                .products
                .iter()
                .map(|p| StarterProductInfo {
                    name: p.name.to_owned(),
                    category: p.category.to_owned(),
                    price_minor: minor(p.price, exponent),
                })
                .collect(),
        })
        .collect()
}

/// A template read against the Hub: what to tell the manager, and the rows to commit.
pub(crate) struct Analysis {
    pub preview: StarterPreview,
    pub plan_rows: Vec<ImportPlanRow>,
    pub product_rows: Vec<ImportProductRow>,
}

fn skipped(name: &str, reason: &str) -> StarterItem {
    StarterItem {
        name: name.to_owned(),
        add: false,
        reason: Some(reason.to_owned()),
    }
}

fn added(name: &str) -> StarterItem {
    StarterItem {
        name: name.to_owned(),
        add: true,
        reason: None,
    }
}

/// Reads template `id` against the products, plans and groups the Hub holds (a dry-run).
pub(crate) fn analyse(
    id: &str,
    products: &ImportContext,
    plans: &PlanContext,
    exponent: u8,
) -> Result<Analysis, AppError> {
    let template = find(id)?;
    let group_ids: Vec<String> = plans
        .groups
        .iter()
        .filter(|g| g.deleted_at.is_none() && g.kind == template.group_kind)
        .map(|g| g.group_id.clone())
        .take(PLAN_MAX_GROUPS)
        .collect();

    let mut plan_items = Vec::new();
    let mut plan_rows = Vec::new();
    for seed in template.plans {
        let row = ImportPlanRow {
            name: seed.name.to_owned(),
            plan_kind: seed.kind.to_owned(),
            price_minor: minor(seed.price, exponent),
            minutes: seed.minutes,
            validity_days: seed.validity_days,
            window_start_min: seed.window.map(|w| w.0),
            window_end_min: seed.window.map(|w| w.1),
            group_ids: group_ids.clone(),
        };
        if plans.resolve(seed.name).is_some() {
            plan_items.push(skipped(seed.name, "exists"));
        } else if group_ids.is_empty() {
            plan_items.push(skipped(seed.name, "no_group"));
        } else if check_plan(plans, &row).is_err() {
            plan_items.push(skipped(seed.name, "invalid"));
        } else {
            plan_items.push(added(seed.name));
            plan_rows.push(row);
        }
    }

    let mut product_items = Vec::new();
    let mut product_rows = Vec::new();
    let mut names = HashSet::new();
    for seed in template.products {
        let row = ImportProductRow {
            name: seed.name.to_owned(),
            category: seed.category.to_owned(),
            price_minor: minor(seed.price, exponent),
            tax_rate_bp: None,
            hsn: seed.hsn.to_owned(),
            barcode: String::new(),
            track_stock: seed.track_stock,
            kitchen: seed.kitchen,
            opening_qty: 0,
            unit_cost_minor: 0,
        };
        if resolve(products, &row).is_some() || !names.insert(seed.name) {
            product_items.push(skipped(seed.name, "exists"));
        } else if check_product(products, &row).is_err() {
            product_items.push(skipped(seed.name, "invalid"));
        } else {
            product_items.push(added(seed.name));
            product_rows.push(row);
        }
    }

    let count = |items: &[StarterItem], add: bool| {
        i64::try_from(items.iter().filter(|i| i.add == add).count()).unwrap_or(i64::MAX)
    };
    let preview = StarterPreview {
        template_id: template.id.to_owned(),
        new_plans: count(&plan_items, true),
        new_products: count(&product_items, true),
        skipped_plans: count(&plan_items, false),
        skipped_products: count(&product_items, false),
        can_commit: !plan_rows.is_empty() || !product_rows.is_empty(),
        plans: plan_items,
        products: product_items,
    };
    Ok(Analysis {
        preview,
        plan_rows,
        product_rows,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use arena_proto::TaxSettings;
    use arena_store::{GroupRow, PlanRow, ProductRow};

    use super::*;

    fn group(id: &str, name: &str, kind: &str) -> GroupRow {
        GroupRow {
            group_id: id.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            colour: "teal".to_owned(),
            sort_order: 0,
            rev: 1,
            deleted_at: None,
        }
    }

    fn plan(name: &str) -> PlanRow {
        PlanRow {
            plan_id: "P1".to_owned(),
            kind: "pack".to_owned(),
            name: name.to_owned(),
            price_minor: 5000,
            minutes: Some(60),
            validity_days: Some(30),
            window_start_min: None,
            window_end_min: None,
            active: true,
            rev: 1,
        }
    }

    fn product(name: &str, barcode: Option<&str>) -> ProductRow {
        ProductRow {
            product_id: format!("PR-{name}"),
            name: name.to_owned(),
            category: String::new(),
            price_minor: 100,
            tax_rate_bp: 500,
            hsn: "220210".to_owned(),
            track_stock: false,
            image: String::new(),
            barcode: barcode.map(str::to_owned),
            kitchen: false,
            favourite: false,
            active: true,
            bundle: false,
            rev: 1,
            deleted_at: None,
        }
    }

    fn contexts(
        groups: Vec<GroupRow>,
        plans: Vec<PlanRow>,
        products: Vec<ProductRow>,
        tax: Option<&str>,
    ) -> (ImportContext, PlanContext) {
        (
            ImportContext::new(products, tax, HashMap::new()).unwrap(),
            PlanContext {
                plans: plans
                    .into_iter()
                    .map(|p| (p, vec!["G1".to_owned()]))
                    .collect(),
                groups,
            },
        )
    }

    fn run(id: &str, ctx: &(ImportContext, PlanContext)) -> Analysis {
        analyse(id, &ctx.0, &ctx.1, 2).unwrap()
    }

    #[test]
    fn every_template_passes_the_import_rules_in_a_branch_with_its_groups() {
        let gst = TaxSettings {
            country: "IN".to_owned(),
            gst: true,
            gstin: String::new(),
            inclusive: true,
            product_bp: 500,
            play_bp: 1800,
            sac: "9996".to_owned(),
            label: String::new(),
            rate_bp: 0,
        }
        .to_json();
        let tax = [None, Some(gst.as_str())];
        for tax in tax {
            for t in &TEMPLATES {
                let ctx = contexts(
                    vec![
                        group("G1", "Floor", t.group_kind),
                        group("G2", "Two", t.group_kind),
                    ],
                    vec![],
                    vec![],
                    tax,
                );
                let a = run(t.id, &ctx);
                assert_eq!(
                    (a.preview.skipped_plans, a.preview.skipped_products),
                    (0, 0),
                    "{}: {:?} {:?}",
                    t.id,
                    a.preview.plans,
                    a.preview.products
                );
                assert_eq!(a.plan_rows.len(), t.plans.len());
                assert_eq!(a.product_rows.len(), t.products.len());
                assert_eq!(a.plan_rows[0].group_ids, vec!["G1", "G2"]);
                assert!(a.preview.can_commit);
            }
        }
    }

    #[test]
    fn names_are_unique_within_a_template_and_plans_never_clash_across_templates() {
        let mut plan_names = HashSet::new();
        for t in &TEMPLATES {
            let products: HashSet<_> = t.products.iter().map(|p| p.name).collect();
            assert_eq!(products.len(), t.products.len(), "{}", t.id);
            for p in t.plans {
                assert!(plan_names.insert(p.name), "{} is in two templates", p.name);
            }
        }
    }

    #[test]
    fn plans_with_no_group_of_their_kind_are_skipped_and_products_still_add() {
        let ctx = contexts(vec![group("G1", "PS5", "console")], vec![], vec![], None);
        let a = run("indian-cafe", &ctx);
        assert_eq!((a.preview.new_plans, a.preview.skipped_plans), (0, 5));
        assert!(
            a.preview
                .plans
                .iter()
                .all(|p| p.reason.as_deref() == Some("no_group"))
        );
        assert_eq!(a.preview.new_products, 9);
        assert!(a.plan_rows.is_empty() && a.preview.can_commit);

        let ctx = contexts(vec![], vec![], vec![], None);
        let a = run("console-lounge", &ctx);
        assert_eq!(a.preview.new_plans, 0);
    }

    #[test]
    fn existing_names_and_barcodes_are_skipped() {
        let ctx = contexts(
            vec![group("G1", "PC", "pc")],
            vec![plan("PC 1 hour pack")],
            vec![product("Samosa", None), product("Other", Some("x"))],
            None,
        );
        let a = run("indian-cafe", &ctx);
        assert_eq!((a.preview.new_plans, a.preview.skipped_plans), (4, 1));
        assert_eq!(a.preview.plans[0].reason.as_deref(), Some("exists"));
        assert_eq!((a.preview.new_products, a.preview.skipped_products), (8, 1));
        let samosa = a
            .preview
            .products
            .iter()
            .find(|p| p.name == "Samosa")
            .unwrap();
        assert_eq!(
            (samosa.add, samosa.reason.as_deref()),
            (false, Some("exists"))
        );
    }

    #[test]
    fn a_template_already_applied_adds_nothing() {
        let t = &TEMPLATES[0];
        let ctx = contexts(
            vec![group("G1", "PC", "pc")],
            t.plans.iter().map(|p| plan(p.name)).collect(),
            t.products.iter().map(|p| product(p.name, None)).collect(),
            None,
        );
        let a = run(t.id, &ctx);
        assert_eq!((a.preview.new_plans, a.preview.new_products), (0, 0));
        assert!(!a.preview.can_commit);
    }

    #[test]
    fn prices_follow_the_currency_decimals() {
        let two = templates(2);
        assert_eq!(two[0].plans[0].price_minor, 6000);
        let none = templates(0);
        assert_eq!(none[0].plans[0].price_minor, 60);
        assert_eq!(none[0].products[0].price_minor, 20);
        let ctx = contexts(vec![group("G1", "PC", "pc")], vec![], vec![], None);
        let a = analyse("indian-cafe", &ctx.0, &ctx.1, 3).unwrap();
        assert_eq!(a.plan_rows[0].price_minor, 60_000);
    }

    #[test]
    fn an_unknown_template_is_refused() {
        let ctx = contexts(vec![], vec![], vec![], None);
        let e = analyse("nope", &ctx.0, &ctx.1, 2).err().unwrap();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        assert_eq!(e.field.as_deref(), Some("template_id"));
    }
}
