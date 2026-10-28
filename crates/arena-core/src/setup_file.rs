//! The branch setup file (Story 5.36): export a branch's catalogue as a versioned JSON file, and
//! read a file against another Hub.
//!
//! [`export`] writes the file from what the Hub holds. [`parse`] reads one (refusing a newer
//! format `UNSUPPORTED`). [`analyse`] is the dry-run *and* the plan of the `import.setup` intent:
//! it walks the file in dependency order (groups, rates, products, bundles, plans, rules,
//! coupons), skips what exists by name or fails the rules the single-item intents enforce, and
//! lists the steps that remain. Entries that point at others (a plan at its groups, a bundle at its
//! products) do so by name; an entry that points at something the Hub lacks and the file does not
//! add is skipped. Nothing here writes.

use std::collections::{HashMap, HashSet};

use arena_proto::{
    AppError, CouponTarget, CouponUpsert, ErrorCode, GroupUpsert, ImportPlanRow, ImportProductRow,
    KEY_TAX, ProductUpsert, RuleUpsert, SETUP_FORMAT, SETUP_MAX_BYTES, SETUP_MAX_ENTRIES,
    SETUP_UPDATE_HUB, SETUP_VERSION, SetupBundle, SetupComponent, SetupCoupon, SetupCouponTarget,
    SetupFile, SetupFileGroup, SetupItem, SetupKindCount, SetupPlan, SetupPreview, SetupProduct,
    SetupRule, coupon_code_normal,
};
use arena_store::{
    CouponRow, DbConn, GroupRow, PlanRow, PricingRuleRow, ProductRow, find_setting,
    list_active_rates, list_components, list_coupon_targets, list_coupons, list_groups,
    list_plan_groups, list_plans, list_pricing_rules, list_products,
};

use crate::apply::{coupon, groups, products, rates, rules};
use crate::import_plans::{PlanContext, check_row as check_plan};
use crate::import_products::{ImportContext, check_row as check_product, resolve};
use crate::locale;

/// What the Hub holds that a setup file is read against.
pub(crate) struct Context {
    pub currency: String,
    pub groups: Vec<GroupRow>,
    /// The active hourly rate by group id.
    pub rates: HashMap<String, i64>,
    pub plans: PlanContext,
    pub rules: Vec<PricingRuleRow>,
    pub coupons: Vec<CouponRow>,
    pub products: ImportContext,
}

impl Context {
    pub(crate) fn load(conn: &DbConn, now_ms: i64) -> Result<Self, AppError> {
        let currency = locale::read_rows(conn)?
            .map(|rows| locale::resolve(&rows, now_ms).1.currency)
            .unwrap_or_default();
        let groups = list_groups(conn)?;
        let plans = list_plans(conn)?
            .into_iter()
            .map(|p| {
                let g = list_plan_groups(conn, &p.plan_id)?;
                Ok((p, g))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let tax = find_setting(conn, KEY_TAX)?.map(|s| s.value);
        let products = ImportContext::new(list_products(conn)?, tax.as_deref(), HashMap::new())?;
        Ok(Self {
            currency,
            rates: list_active_rates(conn)?
                .into_iter()
                .map(|r| (r.group_id, r.price_per_hour_minor))
                .collect(),
            plans: PlanContext {
                plans,
                groups: groups.clone(),
            },
            groups,
            rules: list_pricing_rules(conn)?,
            coupons: list_coupons(conn)?,
            products,
        })
    }
}

fn refused(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

// ----------------------------------------------------------------------------------------------
// Export
// ----------------------------------------------------------------------------------------------

/// The setup file of the Hub's branch: live groups with their rates, products, bundles, plans,
/// pricing rules and coupons. No players, devices, money, stock or images.
pub fn export(conn: &DbConn, now_ms: i64) -> Result<SetupFile, AppError> {
    let ctx = Context::load(conn, now_ms)?;
    let group_name = |id: &str| {
        ctx.groups
            .iter()
            .find(|g| g.group_id == id)
            .map(|g| g.name.clone())
    };
    let live: Vec<&ProductRow> = ctx
        .products
        .existing
        .iter()
        .filter(|p| p.deleted_at.is_none())
        .collect();
    let product_name = |id: &str| {
        live.iter()
            .find(|p| p.product_id == id)
            .map(|p| p.name.clone())
    };
    let bp = |p: &ProductRow| u16::try_from(p.tax_rate_bp).ok();
    let barcode = |p: &ProductRow| p.barcode.clone().unwrap_or_default();

    let mut bundles = Vec::new();
    for p in live.iter().filter(|p| p.bundle) {
        let mut components: Vec<SetupComponent> = list_components(conn, &p.product_id)?
            .into_iter()
            .filter_map(|(id, qty)| {
                product_name(&id).map(|product| SetupComponent { product, qty })
            })
            .collect();
        components.sort_by(|a, b| a.product.cmp(&b.product));
        bundles.push(SetupBundle {
            name: p.name.clone(),
            category: p.category.clone(),
            price_minor: p.price_minor,
            tax_rate_bp: bp(p),
            hsn: p.hsn.clone(),
            barcode: barcode(p),
            kitchen: p.kitchen,
            favourite: p.favourite,
            active: p.active,
            components,
        });
    }

    let mut coupons = Vec::new();
    for c in &ctx.coupons {
        let mut targets: Vec<SetupCouponTarget> = list_coupon_targets(conn, &c.coupon_id)?
            .into_iter()
            .filter_map(|(kind, id)| {
                let name = match kind.as_str() {
                    "group" => group_name(&id),
                    "plan" => ctx
                        .plans
                        .plans
                        .iter()
                        .find(|(p, _)| p.plan_id == id)
                        .map(|(p, _)| p.name.clone()),
                    _ => product_name(&id),
                }?;
                Some(SetupCouponTarget { kind, name })
            })
            .collect();
        targets.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
        coupons.push(SetupCoupon {
            code: c.code.clone(),
            percent_bp: c.percent_bp,
            amount_minor: c.amount_minor,
            valid_from: c.valid_from.clone(),
            valid_to: c.valid_to.clone(),
            usage_limit: c.usage_limit,
            min_bill_minor: c.min_bill_minor,
            targets,
            active: c.active,
        });
    }

    Ok(SetupFile {
        format: SETUP_FORMAT.to_owned(),
        version: SETUP_VERSION,
        currency: ctx.currency.clone(),
        groups: ctx
            .groups
            .iter()
            .map(|g| SetupFileGroup {
                name: g.name.clone(),
                kind: g.kind.clone(),
                colour: g.colour.clone(),
                sort_order: g.sort_order,
                rate_minor: ctx.rates.get(&g.group_id).copied(),
            })
            .collect(),
        products: live
            .iter()
            .filter(|p| !p.bundle)
            .map(|p| SetupProduct {
                name: p.name.clone(),
                category: p.category.clone(),
                price_minor: p.price_minor,
                tax_rate_bp: bp(p),
                hsn: p.hsn.clone(),
                track_stock: p.track_stock,
                barcode: barcode(p),
                kitchen: p.kitchen,
                favourite: p.favourite,
                active: p.active,
            })
            .collect(),
        bundles,
        plans: ctx
            .plans
            .plans
            .iter()
            .map(|(p, groups)| SetupPlan {
                name: p.name.clone(),
                plan_kind: p.kind.clone(),
                price_minor: p.price_minor,
                minutes: p.minutes,
                validity_days: p.validity_days,
                window_start_min: p.window_start_min,
                window_end_min: p.window_end_min,
                groups: {
                    let mut names: Vec<String> =
                        groups.iter().filter_map(|id| group_name(id)).collect();
                    names.sort();
                    names
                },
                active: p.active,
            })
            .collect(),
        rules: ctx
            .rules
            .iter()
            .filter_map(|r| {
                Some(SetupRule {
                    name: r.name.clone(),
                    group: group_name(&r.group_id)?,
                    rule_kind: r.rule_kind.clone(),
                    days_mask: r.days_mask,
                    start_min: r.start_min,
                    end_min: r.end_min,
                    date_from: r.date_from.clone(),
                    date_to: r.date_to.clone(),
                    adjust_kind: r.adjust_kind.clone(),
                    adjust_value: r.adjust_value,
                    priority: r.priority,
                    active: r.active,
                })
            })
            .collect(),
        coupons,
    })
}

// ----------------------------------------------------------------------------------------------
// Reading a file
// ----------------------------------------------------------------------------------------------

/// Reads the text of a setup file. A file that is not a setup file, or is too big, is
/// `VALIDATION_FAILED`; one written by a newer format is `UNSUPPORTED`.
pub fn parse(text: &str) -> Result<SetupFile, AppError> {
    if text.len() > SETUP_MAX_BYTES {
        return Err(refused("file", "the setup file is larger than 1 MB"));
    }
    let not_setup = || refused("file", "This is not an Arena 360 setup file.");
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| not_setup())?;
    if value.get("format").and_then(|f| f.as_str()) != Some(SETUP_FORMAT) {
        return Err(not_setup());
    }
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(not_setup)?;
    if version > u64::from(SETUP_VERSION) {
        return Err(AppError::with_field(
            ErrorCode::Unsupported,
            "file",
            SETUP_UPDATE_HUB,
        ));
    }
    serde_json::from_value(value)
        .map_err(|e| refused("file", format!("The setup file is damaged: {e}")))
}

/// The setup file as the text an export saves.
pub fn to_text(file: &SetupFile) -> Result<String, AppError> {
    serde_json::to_string_pretty(file)
        .map_err(|_| AppError::new(ErrorCode::Internal, "the setup file cannot be written"))
}

// ----------------------------------------------------------------------------------------------
// The dry-run
// ----------------------------------------------------------------------------------------------

/// One thing to create, by its place in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Group(usize),
    Rate(usize),
    Product(usize),
    Bundle(usize),
    Plan(usize),
    Rule(usize),
    Coupon(usize),
}

pub(crate) struct Analysis {
    pub preview: SetupPreview,
    pub steps: Vec<Step>,
}

const KINDS: [&str; 7] = [
    "group", "rate", "product", "bundle", "plan", "rule", "coupon",
];

struct Tally {
    items: Vec<SetupItem>,
    steps: Vec<Step>,
}

impl Tally {
    fn add(&mut self, kind: &str, name: &str, step: Step) {
        self.steps.push(step);
        self.items.push(SetupItem {
            kind: kind.to_owned(),
            name: name.to_owned(),
            add: true,
            reason: None,
        });
    }

    fn skip(&mut self, kind: &str, name: &str, reason: &str) {
        self.items.push(SetupItem {
            kind: kind.to_owned(),
            name: name.to_owned(),
            add: false,
            reason: Some(reason.to_owned()),
        });
    }
}

fn find_group<'a>(groups: &'a [GroupRow], name: &str) -> Option<&'a GroupRow> {
    let name = name.trim();
    groups
        .iter()
        .find(|g| g.name.trim().eq_ignore_ascii_case(name))
}

fn find_product<'a>(products: &'a [ProductRow], name: &str) -> Option<&'a ProductRow> {
    let name = name.trim();
    let mut same = products
        .iter()
        .filter(|p| p.deleted_at.is_none() && p.name.trim() == name);
    let first = same.next()?;
    Some(if first.active {
        first
    } else {
        same.find(|p| p.active).unwrap_or(first)
    })
}

/// The `group.upsert` a file group stands for.
pub(crate) fn group_upsert(g: &SetupFileGroup) -> GroupUpsert {
    GroupUpsert {
        id: None,
        name: g.name.trim().to_owned(),
        group_kind: g.kind.clone(),
        colour: g.colour.clone(),
        sort_order: g.sort_order,
    }
}

/// The `product.upsert` a file product stands for.
pub(crate) fn product_upsert(p: &SetupProduct) -> ProductUpsert {
    ProductUpsert {
        id: None,
        name: p.name.trim().to_owned(),
        category: p.category.trim().to_owned(),
        price_minor: p.price_minor,
        tax_rate_bp: p.tax_rate_bp,
        hsn: p.hsn.trim().to_owned(),
        track_stock: p.track_stock,
        image: String::new(),
        barcode: p.barcode.trim().to_owned(),
        kitchen: p.kitchen,
        favourite: p.favourite,
        active: p.active,
        bundle: false,
        components: Vec::new(),
    }
}

/// The `product.upsert` a file bundle stands for, its components given as product ids.
pub(crate) fn bundle_upsert(b: &SetupBundle, ids: &[String]) -> ProductUpsert {
    ProductUpsert {
        id: None,
        name: b.name.trim().to_owned(),
        category: b.category.trim().to_owned(),
        price_minor: b.price_minor,
        tax_rate_bp: b.tax_rate_bp,
        hsn: b.hsn.trim().to_owned(),
        track_stock: false,
        image: String::new(),
        barcode: b.barcode.trim().to_owned(),
        kitchen: b.kitchen,
        favourite: b.favourite,
        active: b.active,
        bundle: true,
        components: b
            .components
            .iter()
            .zip(ids)
            .map(|(c, id)| arena_proto::BundleComponent {
                product_id: id.clone(),
                qty: c.qty,
            })
            .collect(),
    }
}

/// The `plan.upsert` a file plan stands for, its groups given as ids.
pub(crate) fn plan_upsert(p: &SetupPlan, ids: Vec<String>) -> arena_proto::PlanUpsert {
    arena_proto::PlanUpsert {
        id: None,
        plan_kind: p.plan_kind.clone(),
        name: p.name.trim().to_owned(),
        price_minor: p.price_minor,
        minutes: p.minutes,
        validity_days: p.validity_days,
        window_start_min: p.window_start_min,
        window_end_min: p.window_end_min,
        group_ids: ids,
        active: p.active,
    }
}

/// The `rule.upsert` a file rule stands for, its group given as an id.
pub(crate) fn rule_upsert(r: &SetupRule, group_id: String) -> RuleUpsert {
    RuleUpsert {
        id: None,
        name: r.name.trim().to_owned(),
        group_id,
        rule_kind: r.rule_kind.clone(),
        days_mask: r.days_mask,
        start_min: r.start_min,
        end_min: r.end_min,
        date_from: r.date_from.clone(),
        date_to: r.date_to.clone(),
        adjust_kind: r.adjust_kind.clone(),
        adjust_value: r.adjust_value,
        priority: r.priority,
        active: r.active,
    }
}

/// The `coupon.upsert` a file coupon stands for, its targets given as ids.
pub(crate) fn coupon_upsert(c: &SetupCoupon, ids: Vec<String>) -> CouponUpsert {
    CouponUpsert {
        id: None,
        code: c.code.clone(),
        percent_bp: c.percent_bp,
        amount_minor: c.amount_minor,
        valid_from: c.valid_from.clone(),
        valid_to: c.valid_to.clone(),
        usage_limit: c.usage_limit,
        min_bill_minor: c.min_bill_minor,
        targets: c
            .targets
            .iter()
            .zip(ids)
            .map(|(t, id)| CouponTarget {
                kind: t.kind.clone(),
                id,
            })
            .collect(),
        active: c.active,
    }
}

fn product_row(id: String, name: &str, active: bool, bundle: bool, barcode: &str) -> ProductRow {
    ProductRow {
        product_id: id,
        name: name.trim().to_owned(),
        category: String::new(),
        price_minor: 0,
        tax_rate_bp: 0,
        hsn: String::new(),
        track_stock: false,
        image: String::new(),
        barcode: (!barcode.trim().is_empty()).then(|| barcode.trim().to_owned()),
        kitchen: false,
        favourite: false,
        active,
        bundle,
        rev: 0,
        deleted_at: None,
    }
}

fn import_row(
    name: &str,
    category: &str,
    price: i64,
    bp: Option<u16>,
    hsn: &str,
    barcode: &str,
) -> ImportProductRow {
    ImportProductRow {
        name: name.to_owned(),
        category: category.to_owned(),
        price_minor: price,
        tax_rate_bp: bp,
        hsn: hsn.to_owned(),
        barcode: barcode.to_owned(),
        track_stock: false,
        kitchen: false,
        opening_qty: 0,
        unit_cost_minor: 0,
    }
}

fn count(items: &[SetupItem], kind: &str) -> SetupKindCount {
    let of = |add: bool| {
        i64::try_from(
            items
                .iter()
                .filter(|i| i.kind == kind && i.add == add)
                .count(),
        )
        .unwrap_or(i64::MAX)
    };
    SetupKindCount {
        kind: kind.to_owned(),
        added: of(true),
        skipped: of(false),
    }
}

/// The file-level rules: the format, the branch currency and the size of each list.
fn check_file(file: &SetupFile, ctx: &Context) -> Result<(), AppError> {
    if file.format != SETUP_FORMAT {
        return Err(refused("file", "This is not an Arena 360 setup file."));
    }
    if file.version > SETUP_VERSION {
        return Err(AppError::with_field(
            ErrorCode::Unsupported,
            "file",
            SETUP_UPDATE_HUB,
        ));
    }
    if file.currency != ctx.currency {
        return Err(refused(
            "currency",
            format!(
                "This file is priced in {}; this branch uses {}.",
                file.currency, ctx.currency
            ),
        ));
    }
    let longest = [
        file.groups.len(),
        file.products.len(),
        file.bundles.len(),
        file.plans.len(),
        file.rules.len(),
        file.coupons.len(),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    if longest > SETUP_MAX_ENTRIES {
        return Err(refused("file", "the setup file has too many entries"));
    }
    Ok(())
}

/// Reads `file` against what the Hub holds: what would be created and what is skipped.
pub(crate) fn analyse(file: &SetupFile, ctx: &Context) -> Result<Analysis, AppError> {
    check_file(file, ctx)?;
    let mut t = Tally {
        items: Vec::new(),
        steps: Vec::new(),
    };

    // Groups and their rates.
    let mut groups = ctx.groups.clone();
    let mut fresh: HashSet<String> = HashSet::new();
    for (i, g) in file.groups.iter().enumerate() {
        if find_group(&groups, &g.name).is_some() {
            t.skip("group", &g.name, "exists");
        } else if groups::check_import(&group_upsert(g)).is_err() {
            t.skip("group", &g.name, "invalid");
        } else {
            let id = format!("@g{i}");
            groups.push(GroupRow {
                group_id: id.clone(),
                name: g.name.trim().to_owned(),
                kind: g.kind.clone(),
                colour: g.colour.clone(),
                sort_order: g.sort_order,
                rev: 0,
                deleted_at: None,
            });
            fresh.insert(id);
            t.add("group", &g.name, Step::Group(i));
        }
    }
    for (i, g) in file.groups.iter().enumerate() {
        let Some(price) = g.rate_minor else { continue };
        match find_group(&groups, &g.name) {
            None => t.skip("rate", &g.name, "no_group"),
            Some(row)
                if !fresh.contains(&row.group_id) && ctx.rates.contains_key(&row.group_id) =>
            {
                t.skip("rate", &g.name, "exists");
            }
            Some(_) if !rates::price_ok(price) => t.skip("rate", &g.name, "invalid"),
            Some(_) => t.add("rate", &g.name, Step::Rate(i)),
        }
    }

    // Products, then bundles (which are made of products).
    let mut pctx = ImportContext {
        existing: ctx.products.existing.clone(),
        tax: ctx.products.tax.clone(),
        stock: HashMap::new(),
    };
    for (i, p) in file.products.iter().enumerate() {
        let row = ImportProductRow {
            track_stock: p.track_stock,
            kitchen: p.kitchen,
            ..import_row(
                p.name.trim(),
                p.category.trim(),
                p.price_minor,
                p.tax_rate_bp,
                p.hsn.trim(),
                p.barcode.trim(),
            )
        };
        if resolve(&pctx, &row).is_some() {
            t.skip("product", &p.name, "exists");
        } else if check_product(&pctx, &row).is_err() {
            t.skip("product", &p.name, "invalid");
        } else {
            pctx.existing.push(product_row(
                format!("@p{i}"),
                &p.name,
                p.active,
                false,
                &p.barcode,
            ));
            t.add("product", &p.name, Step::Product(i));
        }
    }
    for (i, b) in file.bundles.iter().enumerate() {
        let row = ImportProductRow {
            kitchen: b.kitchen,
            ..import_row(
                b.name.trim(),
                b.category.trim(),
                b.price_minor,
                b.tax_rate_bp,
                b.hsn.trim(),
                b.barcode.trim(),
            )
        };
        if resolve(&pctx, &row).is_some() {
            t.skip("bundle", &b.name, "exists");
            continue;
        }
        let parts: Vec<Option<ProductRow>> = b
            .components
            .iter()
            .map(|c| find_product(&pctx.existing, &c.product).cloned())
            .collect();
        if parts.iter().any(Option::is_none) {
            t.skip("bundle", &b.name, "no_product");
            continue;
        }
        let ids: Vec<String> = parts
            .iter()
            .flatten()
            .map(|p| p.product_id.clone())
            .collect();
        let barcode = b.barcode.trim();
        let holder = (!barcode.is_empty())
            .then(|| {
                pctx.existing
                    .iter()
                    .find(|p| p.active && p.barcode.as_deref() == Some(barcode))
                    .map(|p| p.product_id.as_str())
            })
            .flatten();
        if products::check_bundle(&bundle_upsert(b, &ids), parts, holder, pctx.tax.as_ref())
            .is_err()
        {
            t.skip("bundle", &b.name, "invalid");
        } else {
            pctx.existing.push(product_row(
                format!("@b{i}"),
                &b.name,
                b.active,
                true,
                &b.barcode,
            ));
            t.add("bundle", &b.name, Step::Bundle(i));
        }
    }

    // Plans.
    let mut plans = PlanContext {
        plans: ctx.plans.plans.clone(),
        groups: groups.clone(),
    };
    for (i, p) in file.plans.iter().enumerate() {
        if plans.resolve(p.name.trim()).is_some() {
            t.skip("plan", &p.name, "exists");
            continue;
        }
        let ids: Vec<String> = p
            .groups
            .iter()
            .filter_map(|n| find_group(&groups, n).map(|g| g.group_id.clone()))
            .collect();
        if ids.len() != p.groups.len() {
            t.skip("plan", &p.name, "no_group");
            continue;
        }
        let row = ImportPlanRow {
            name: p.name.trim().to_owned(),
            plan_kind: p.plan_kind.clone(),
            price_minor: p.price_minor,
            minutes: p.minutes,
            validity_days: p.validity_days,
            window_start_min: p.window_start_min,
            window_end_min: p.window_end_min,
            group_ids: ids.clone(),
        };
        if check_plan(&plans, &row).is_err() {
            t.skip("plan", &p.name, "invalid");
        } else {
            plans.plans.push((
                PlanRow {
                    plan_id: format!("@l{i}"),
                    kind: p.plan_kind.clone(),
                    name: p.name.trim().to_owned(),
                    price_minor: p.price_minor,
                    minutes: p.minutes,
                    validity_days: p.validity_days,
                    window_start_min: p.window_start_min,
                    window_end_min: p.window_end_min,
                    active: p.active,
                    rev: 0,
                },
                ids,
            ));
            t.add("plan", &p.name, Step::Plan(i));
        }
    }

    // Pricing rules: a rule is "the same" when its group already has a rule of that name.
    let mut rule_names: HashSet<(String, String)> = ctx
        .rules
        .iter()
        .map(|r| (r.group_id.clone(), r.name.trim().to_lowercase()))
        .collect();
    for (i, r) in file.rules.iter().enumerate() {
        let Some(group) = find_group(&groups, &r.group) else {
            t.skip("rule", &r.name, "no_group");
            continue;
        };
        let key = (group.group_id.clone(), r.name.trim().to_lowercase());
        if rule_names.contains(&key) {
            t.skip("rule", &r.name, "exists");
        } else if rules::check_import(&rule_upsert(r, group.group_id.clone()), group).is_err() {
            t.skip("rule", &r.name, "invalid");
        } else {
            rule_names.insert(key);
            t.add("rule", &r.name, Step::Rule(i));
        }
    }

    // Coupons.
    let mut codes: HashSet<String> = ctx.coupons.iter().map(|c| c.code.clone()).collect();
    for (i, c) in file.coupons.iter().enumerate() {
        let code = coupon_code_normal(&c.code);
        if codes.contains(&code) {
            t.skip("coupon", &c.code, "exists");
            continue;
        }
        let mut ids = Vec::new();
        let mut missing = None;
        for target in &c.targets {
            let found = match target.kind.as_str() {
                "group" => find_group(&groups, &target.name)
                    .map(|g| g.group_id.clone())
                    .ok_or("no_group"),
                "plan" => plans
                    .plans
                    .iter()
                    .find(|(p, _)| p.name == target.name.trim())
                    .map(|(p, _)| p.plan_id.clone())
                    .ok_or("no_plan"),
                "product" => find_product(&pctx.existing, &target.name)
                    .map(|p| p.product_id.clone())
                    .ok_or("no_product"),
                _ => Err("invalid"),
            };
            match found {
                Ok(id) => ids.push(id),
                Err(reason) => {
                    missing = Some(reason);
                    break;
                }
            }
        }
        if let Some(reason) = missing {
            t.skip("coupon", &c.code, reason);
        } else if coupon::check_import(&coupon_upsert(c, ids)).is_err() {
            t.skip("coupon", &c.code, "invalid");
        } else {
            codes.insert(code);
            t.add("coupon", &c.code, Step::Coupon(i));
        }
    }

    let counts: Vec<SetupKindCount> = KINDS.iter().map(|k| count(&t.items, k)).collect();
    let added = counts.iter().map(|c| c.added).sum();
    let skipped = counts.iter().map(|c| c.skipped).sum();
    Ok(Analysis {
        preview: SetupPreview {
            counts,
            items: t.items,
            added,
            skipped,
            can_commit: !t.steps.is_empty(),
        },
        steps: t.steps,
    })
}

/// The name a target of a file coupon is listed under (for the apply step's lookups).
pub(crate) fn target_name(t: &SetupCouponTarget) -> &str {
    t.name.trim()
}
