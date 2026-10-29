//! CSV import of products with a dry-run (Story 5.33).
//!
//! One piece of logic serves the preview and the `import.products` intent, so what the dry-run
//! promises is what a commit does: [`check_row`] runs the Story 5.3 product rules and the
//! opening-stock rules on a row against the products the Hub holds, [`duplicates`] finds rows
//! of one file that collide, and [`analyse`] reads a file and counts. Nothing here writes.

use std::collections::HashMap;

use arena_proto::{
    AppError, ErrorCode, IMPORT_MAX_BYTES, IMPORT_MAX_FAILURES, IMPORT_MAX_ROWS,
    IMPORT_PRODUCT_COLUMNS, ImportFailure, ImportProductRow, MAX_PRICE_MINOR, MAX_STOCK_IN_QTY,
    MAX_STOCK_QTY, ProductImportPreview, TaxSettings,
};
use arena_store::ProductRow;

use crate::apply::products::check_import_row;
use crate::import_csv::{CsvRecord, DecimalError, parse_csv, parse_decimal};

/// The file name of the template.
pub const TEMPLATE_FILE: &str = "products-template.csv";

/// The template: the exact header and two sample rows. It passes its own dry-run on any branch.
pub fn template() -> String {
    let mut out = IMPORT_PRODUCT_COLUMNS.join(",");
    out.push_str("\r\n");
    out.push_str("Cola 500 ml,Drinks,40,5,220210,8901234567890,true,false,24,28.50\r\n");
    out.push_str("Veg sandwich,Snacks,90,5,210690,,false,true,0,0\r\n");
    out
}

/// What a row or the file did wrong, before it becomes an [`ImportFailure`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub field: String,
    pub reason: &'static str,
    pub message: String,
}

pub(crate) fn problem(field: &str, reason: &'static str, message: impl Into<String>) -> Problem {
    Problem {
        field: field.to_owned(),
        reason,
        message: message.into(),
    }
}

/// What the Hub holds that an import is checked against.
pub(crate) struct ImportContext {
    /// Every live product (not deleted), active or not.
    pub existing: Vec<ProductRow>,
    pub tax: Option<TaxSettings>,
    /// On-hand quantity by product id.
    pub stock: HashMap<String, i64>,
}

impl ImportContext {
    /// Builds the context from the stored `branch.tax` value, if any.
    pub fn new(
        existing: Vec<ProductRow>,
        tax: Option<&str>,
        stock: HashMap<String, i64>,
    ) -> Result<Self, AppError> {
        let tax = tax
            .map(|v| {
                TaxSettings::parse(v).ok_or_else(|| {
                    AppError::new(ErrorCode::Internal, "the tax settings cannot be read")
                })
            })
            .transpose()?;
        Ok(Self {
            existing,
            tax,
            stock,
        })
    }
}

/// The context read from the store (outside a write transaction).
pub(crate) fn context(store: &arena_store::Store) -> Result<ImportContext, AppError> {
    let existing = store.products()?;
    let tax = store
        .settings()?
        .into_iter()
        .find(|s| s.key == arena_proto::KEY_TAX)
        .map(|s| s.value);
    let mut stock = HashMap::new();
    for p in existing.iter().filter(|p| p.track_stock && !p.bundle) {
        if let Some(s) = store.stock(&p.product_id)? {
            stock.insert(p.product_id.clone(), s.qty);
        }
    }
    ImportContext::new(existing, tax.as_deref(), stock)
}

/// The product a row updates: the one with its barcode, else the one with exactly its name.
/// An active product wins over an inactive one.
pub(crate) fn resolve(ctx: &ImportContext, row: &ImportProductRow) -> Option<usize> {
    let barcode = row.barcode.trim();
    let name = row.name.trim();
    let pick = |found: &mut dyn Iterator<Item = usize>| {
        let all: Vec<usize> = found.collect();
        all.iter()
            .copied()
            .find(|&i| ctx.existing[i].active)
            .or_else(|| all.first().copied())
    };
    if !barcode.is_empty()
        && let Some(i) = pick(
            &mut (0..ctx.existing.len())
                .filter(|&i| ctx.existing[i].barcode.as_deref() == Some(barcode)),
        )
    {
        return Some(i);
    }
    pick(&mut (0..ctx.existing.len()).filter(|&i| ctx.existing[i].name == name))
}

fn column_of(field: &str) -> &str {
    match field {
        "price_minor" => "price",
        "tax_rate_bp" => "tax_rate",
        other => other,
    }
}

fn from_app_error(e: &AppError) -> Problem {
    let field = column_of(e.field.as_deref().unwrap_or("name"));
    let reason = match e.code {
        ErrorCode::HsnRequired => "needs_hsn",
        ErrorCode::BarcodeTaken => "barcode_taken",
        _ if e.message.contains("range") => "out_of_range",
        _ if e.message.contains("characters") => "too_long",
        _ => "bad_value",
    };
    problem(field, reason, e.message.clone())
}

/// The Story 5.3 rules and the opening-stock rules for one row. On success: the index of the
/// product it updates, or `None` for a new one.
pub(crate) fn check_row(
    ctx: &ImportContext,
    row: &ImportProductRow,
) -> Result<Option<usize>, Problem> {
    let target = resolve(ctx, row);
    let old = target.map(|i| &ctx.existing[i]);
    if old.is_some_and(|o| o.bundle) {
        return Err(problem(
            "name",
            "bundle",
            "this is a bundle; change it in Settings, not by import",
        ));
    }
    let barcode = row.barcode.trim();
    let holder = (!barcode.is_empty())
        .then(|| {
            ctx.existing
                .iter()
                .find(|p| p.active && p.barcode.as_deref() == Some(barcode))
                .map(|p| p.product_id.as_str())
        })
        .flatten();
    check_import_row(row, old, holder, ctx.tax.as_ref()).map_err(|e| from_app_error(&e))?;
    if row.opening_qty < 0 {
        return Err(problem(
            "opening_qty",
            "out_of_range",
            "quantity is out of range",
        ));
    }
    if row.opening_qty > 0 {
        if !row.track_stock {
            return Err(problem(
                "opening_qty",
                "needs_tracking",
                "opening stock needs track_stock to be true",
            ));
        }
        let held = old.map_or(0, |o| ctx.stock.get(&o.product_id).copied().unwrap_or(0));
        if row.opening_qty > MAX_STOCK_IN_QTY || held + row.opening_qty > MAX_STOCK_QTY {
            return Err(problem(
                "opening_qty",
                "out_of_range",
                "quantity is out of range",
            ));
        }
    }
    if !(0..=MAX_PRICE_MINOR).contains(&row.unit_cost_minor) {
        return Err(problem("unit_cost", "out_of_range", "cost is out of range"));
    }
    Ok(target)
}

/// Rows of one file that collide, each as `(index into items, problem)`. `items` holds only the
/// rows that passed [`check_row`], as `(line, row, target)`; the first of a collision stays valid.
pub(crate) fn duplicates(
    items: &[(i64, &ImportProductRow, Option<usize>)],
) -> Vec<(usize, Problem)> {
    let mut barcodes: HashMap<&str, i64> = HashMap::new();
    let mut targets: HashMap<usize, i64> = HashMap::new();
    let mut names: HashMap<&str, i64> = HashMap::new();
    let mut out = Vec::new();
    for (n, (line, row, target)) in items.iter().enumerate() {
        let barcode = row.barcode.trim();
        if !barcode.is_empty()
            && let Some(first) = barcodes.get(barcode)
        {
            out.push((
                n,
                problem(
                    "barcode",
                    "duplicate_barcode",
                    format!("the barcode is already used by row {first}"),
                ),
            ));
            continue;
        }
        let clash = match target {
            Some(t) => targets.get(t),
            None if barcode.is_empty() => names.get(row.name.trim()),
            None => None,
        };
        if let Some(first) = clash {
            out.push((
                n,
                problem(
                    "name",
                    "duplicate_product",
                    format!("row {first} already adds or updates this product"),
                ),
            ));
            continue;
        }
        if !barcode.is_empty() {
            barcodes.insert(barcode, *line);
        }
        match target {
            Some(t) => {
                targets.insert(*t, *line);
            }
            None if barcode.is_empty() => {
                names.insert(row.name.trim(), *line);
            }
            None => {}
        }
    }
    out
}

/// Where each known column sits in the file.
struct Columns([Option<usize>; IMPORT_PRODUCT_COLUMNS.len()]);

impl Columns {
    fn cell<'a>(&self, rec: &'a CsvRecord, column: &str) -> &'a str {
        IMPORT_PRODUCT_COLUMNS
            .iter()
            .position(|c| *c == column)
            .and_then(|k| self.0[k])
            .and_then(|i| rec.cells.get(i))
            .map_or("", String::as_str)
    }
}

pub(crate) fn number(field: &str, text: &str, places: u32, what: &str) -> Result<i64, Problem> {
    parse_decimal(text, places).map_err(|e| match e {
        DecimalError::Syntax => problem(field, "bad_number", format!("{what} must be a number")),
        DecimalError::Places => problem(
            field,
            "bad_number",
            format!("{what} has more decimal places than allowed"),
        ),
        DecimalError::Negative => {
            problem(field, "out_of_range", format!("{what} cannot be negative"))
        }
        DecimalError::Big => problem(field, "out_of_range", format!("{what} is too large")),
    })
}

fn flag(field: &str, text: &str) -> Result<bool, Problem> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "1" => Ok(true),
        "false" | "no" | "n" | "0" | "" => Ok(false),
        _ => Err(problem(field, "bad_flag", "must be true or false")),
    }
}

pub(crate) fn take<T>(problems: &mut Vec<Problem>, r: Result<T, Problem>, default: T) -> T {
    r.unwrap_or_else(|p| {
        problems.push(p);
        default
    })
}

fn parse_row(
    rec: &CsvRecord,
    cols: &Columns,
    width: usize,
    exp: u32,
) -> Result<ImportProductRow, Vec<Problem>> {
    let mut problems = Vec::new();
    if rec.cells.len() > width && rec.cells[width..].iter().any(|c| !c.is_empty()) {
        problems.push(problem(
            "",
            "bad_value",
            "the row has more values than the header has columns",
        ));
    }
    let name = cols.cell(rec, "name");
    if name.is_empty() {
        problems.push(problem("name", "required", "name is required"));
    }
    let price = cols.cell(rec, "price");
    let price_minor = if price.is_empty() {
        problems.push(problem("price", "required", "price is required"));
        0
    } else {
        take(&mut problems, number("price", price, exp, "price"), 0)
    };
    let rate = cols.cell(rec, "tax_rate");
    let tax_rate_bp = if rate.is_empty() {
        None
    } else {
        let bp = number("tax_rate", rate, 2, "tax rate").and_then(|v| {
            u16::try_from(v)
                .map_err(|_| problem("tax_rate", "out_of_range", "tax rate is out of range"))
        });
        take(&mut problems, bp.map(Some), None)
    };
    let track_stock = take(
        &mut problems,
        flag("track_stock", cols.cell(rec, "track_stock")),
        false,
    );
    let kitchen = take(
        &mut problems,
        flag("kitchen", cols.cell(rec, "kitchen")),
        false,
    );
    let qty = cols.cell(rec, "opening_qty");
    let opening_qty = if qty.is_empty() {
        0
    } else {
        take(&mut problems, number("opening_qty", qty, 0, "quantity"), 0)
    };
    let cost = cols.cell(rec, "unit_cost");
    let unit_cost_minor = if cost.is_empty() {
        0
    } else {
        take(&mut problems, number("unit_cost", cost, exp, "cost"), 0)
    };
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(ImportProductRow {
        name: name.to_owned(),
        category: cols.cell(rec, "category").to_owned(),
        price_minor,
        tax_rate_bp,
        hsn: cols.cell(rec, "hsn").to_owned(),
        barcode: cols.cell(rec, "barcode").to_owned(),
        track_stock,
        kitchen,
        opening_qty,
        unit_cost_minor,
    })
}

/// A file and the rows of it that can be imported.
pub(crate) struct Analysis {
    pub preview: ProductImportPreview,
    /// The rows that passed, in file order.
    pub rows: Vec<ImportProductRow>,
}

fn unusable(failures: Vec<ImportFailure>) -> Analysis {
    Analysis {
        preview: ProductImportPreview {
            rows: 0,
            new: 0,
            updated: 0,
            failed: 0,
            with_stock: 0,
            failures,
            truncated: false,
            unusable: true,
            can_commit: false,
            can_commit_valid: false,
        },
        rows: Vec::new(),
    }
}

fn whole_file(reason: &str, field: &str, message: impl Into<String>) -> ImportFailure {
    ImportFailure {
        row: 0,
        field: field.to_owned(),
        reason: reason.to_owned(),
        message: message.into(),
    }
}

/// Reads `csv` against `ctx` (a dry-run): the counts, every problem with its line, and the rows
/// that can be imported. `exponent` is the currency's decimal places. `Err` only for a file over
/// the size or row limits.
pub(crate) fn analyse(csv: &str, ctx: &ImportContext, exponent: u8) -> Result<Analysis, AppError> {
    let too_big = |m: &str| AppError::with_field(ErrorCode::ValidationFailed, "csv", m);
    if csv.len() > IMPORT_MAX_BYTES {
        return Err(too_big("the file is larger than 1.5 MB"));
    }
    let records = match parse_csv(csv) {
        Ok(r) => r,
        Err(m) => return Ok(unusable(vec![whole_file("bad_file", "", m)])),
    };
    let Some((header, data)) = records.split_first() else {
        return Ok(unusable(vec![whole_file(
            "bad_file",
            "",
            "the file is empty; it needs a header row",
        )]));
    };
    if data.len() > IMPORT_MAX_ROWS {
        return Err(too_big("the file has more than 10,000 rows"));
    }
    let mut failures: Vec<ImportFailure> = Vec::new();
    let mut cols = Columns([None; IMPORT_PRODUCT_COLUMNS.len()]);
    for (i, cell) in header.cells.iter().enumerate() {
        let key = cell.to_ascii_lowercase();
        match IMPORT_PRODUCT_COLUMNS.iter().position(|c| *c == key) {
            Some(k) if cols.0[k].is_some() => {
                return Ok(unusable(vec![ImportFailure {
                    row: i64::try_from(header.line).unwrap_or(1),
                    field: key,
                    reason: "bad_file".to_owned(),
                    message: "the column appears twice in the header".to_owned(),
                }]));
            }
            Some(k) => cols.0[k] = Some(i),
            None => failures.push(ImportFailure {
                row: i64::try_from(header.line).unwrap_or(1),
                field: cell.clone(),
                reason: "unknown_column".to_owned(),
                message: "this column is not part of the template".to_owned(),
            }),
        }
    }
    let missing: Vec<ImportFailure> = ["name", "price"]
        .iter()
        .filter(|c| {
            IMPORT_PRODUCT_COLUMNS
                .iter()
                .position(|k| k == *c)
                .is_some_and(|k| cols.0[k].is_none())
        })
        .map(|c| {
            whole_file(
                "missing_column",
                c,
                format!("the header has no \"{c}\" column"),
            )
        })
        .collect();
    if !missing.is_empty() {
        return Ok(unusable(missing));
    }
    let unknown_columns = failures.len();
    let exp = u32::from(exponent);
    let mut bad: Vec<(i64, Problem)> = Vec::new();
    let mut ok: Vec<(i64, ImportProductRow, Option<usize>)> = Vec::new();
    for rec in data {
        let line = i64::try_from(rec.line).unwrap_or(i64::MAX);
        match parse_row(rec, &cols, header.cells.len(), exp) {
            Err(problems) => bad.extend(problems.into_iter().map(|p| (line, p))),
            Ok(row) => match check_row(ctx, &row) {
                Err(p) => bad.push((line, p)),
                Ok(target) => ok.push((line, row, target)),
            },
        }
    }
    let colliding = {
        let items: Vec<(i64, &ImportProductRow, Option<usize>)> =
            ok.iter().map(|(l, r, t)| (*l, r, *t)).collect();
        duplicates(&items)
    };
    let mut dropped = vec![false; ok.len()];
    for (n, p) in colliding {
        dropped[n] = true;
        bad.push((ok[n].0, p));
    }
    let mut valid = Vec::new();
    let (mut new, mut updated, mut with_stock) = (0, 0, 0);
    for ((_, row, target), dropped) in ok.into_iter().zip(dropped) {
        if dropped {
            continue;
        }
        if target.is_some() {
            updated += 1;
        } else {
            new += 1;
        }
        if row.opening_qty > 0 {
            with_stock += 1;
        }
        valid.push(row);
    }
    bad.sort_by_key(|(line, _)| *line);
    let failed = {
        let mut lines: Vec<i64> = bad.iter().map(|(l, _)| *l).collect();
        lines.dedup();
        i64::try_from(lines.len()).unwrap_or(i64::MAX)
    };
    failures.extend(bad.into_iter().map(|(row, p)| ImportFailure {
        row,
        field: p.field,
        reason: p.reason.to_owned(),
        message: p.message,
    }));
    let truncated = failures.len() > IMPORT_MAX_FAILURES;
    failures.truncate(IMPORT_MAX_FAILURES);
    let rows = i64::try_from(data.len()).unwrap_or(i64::MAX);
    Ok(Analysis {
        preview: ProductImportPreview {
            rows,
            new,
            updated,
            failed,
            with_stock,
            failures,
            truncated,
            unusable: false,
            can_commit: failed == 0 && unknown_columns == 0 && !valid.is_empty(),
            can_commit_valid: !valid.is_empty(),
        },
        rows: valid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(id: &str, name: &str, barcode: Option<&str>) -> ProductRow {
        ProductRow {
            product_id: id.to_owned(),
            name: name.to_owned(),
            category: "Drinks".to_owned(),
            price_minor: 1000,
            tax_rate_bp: 500,
            hsn: "220210".to_owned(),
            track_stock: true,
            image: String::new(),
            barcode: barcode.map(str::to_owned),
            kitchen: false,
            favourite: false,
            active: true,
            bundle: false,
            rev: 1,
            deleted_at: None,
            kitchen_notes: Vec::new(),
        }
    }

    fn gst() -> String {
        serde_json::to_string(&TaxSettings {
            country: "IN".to_owned(),
            gst: true,
            gstin: String::new(),
            inclusive: true,
            product_bp: 500,
            play_bp: 1800,
            sac: "9996".to_owned(),
            label: String::new(),
            rate_bp: 0,
        })
        .unwrap()
    }

    fn ctx(existing: Vec<ProductRow>) -> ImportContext {
        ImportContext::new(existing, Some(&gst()), HashMap::new()).unwrap()
    }

    fn head() -> &'static str {
        "name,category,price,tax_rate,hsn,barcode,track_stock,kitchen,opening_qty,unit_cost\r\n"
    }

    fn run(body: &str, existing: Vec<ProductRow>) -> Analysis {
        analyse(&format!("{}{body}", head()), &ctx(existing), 2).unwrap()
    }

    #[test]
    fn the_template_passes_its_own_dry_run() {
        let a = analyse(&template(), &ctx(Vec::new()), 2).unwrap();
        assert_eq!(a.preview.failures, Vec::new());
        assert_eq!(
            (a.preview.rows, a.preview.new, a.preview.updated),
            (2, 2, 0)
        );
        assert!(a.preview.can_commit);
        assert_eq!(a.rows[0].price_minor, 4000);
        assert_eq!(a.rows[0].unit_cost_minor, 2850);
        assert_eq!(a.rows[0].tax_rate_bp, Some(500));
        assert!(template().starts_with(&IMPORT_PRODUCT_COLUMNS.join(",")));
    }

    #[test]
    fn rows_match_by_barcode_else_exact_name() {
        let existing = vec![
            product("A", "Cola", Some("111")),
            product("B", "Water", None),
        ];
        let a = run(
            "Cola renamed,Drinks,12,5,220210,111,true,false,0,0\r\n\
             Water,Drinks,10,5,220210,,true,false,0,0\r\n\
             water,Drinks,10,5,220210,,true,false,0,0\r\n",
            existing,
        );
        assert_eq!(a.preview.failures, Vec::new());
        assert_eq!(
            (a.preview.new, a.preview.updated, a.preview.failed),
            (1, 2, 0),
            "barcode, then exact (case-sensitive) name; `water` is new"
        );
    }

    #[test]
    fn a_failed_row_names_its_line_and_reason_and_writes_nothing() {
        let a = run(
            "Ok,Drinks,10,5,220210,,true,false,0,0\r\n\
             ,Drinks,10,5,220210,,true,false,0,0\r\n\
             Bad price,Drinks,ten,5,220210,,true,false,0,0\r\n\
             Neg,Drinks,-1,5,220210,,true,false,0,0\r\n\
             Flag,Drinks,1,5,220210,,maybe,false,0,0\r\n\
             Rate,Drinks,1,99,220210,,true,false,0,0\r\n\
             Hsn,Drinks,1,5,,,true,false,0,0\r\n\
             Stock,Drinks,1,5,220210,,false,false,5,1\r\n",
            Vec::new(),
        );
        let got: Vec<_> = a
            .preview
            .failures
            .iter()
            .map(|f| (f.row, f.field.as_str(), f.reason.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (3, "name", "required"),
                (4, "price", "bad_number"),
                (5, "price", "out_of_range"),
                (6, "track_stock", "bad_flag"),
                (7, "tax_rate", "out_of_range"),
                (8, "hsn", "needs_hsn"),
                (9, "opening_qty", "needs_tracking"),
            ]
        );
        assert_eq!((a.preview.new, a.preview.failed), (1, 7));
        assert!(!a.preview.can_commit && a.preview.can_commit_valid);
        assert_eq!(a.rows.len(), 1);
    }

    #[test]
    fn a_duplicate_barcode_or_product_in_the_file_fails_the_later_row() {
        let a = run(
            "One,Drinks,10,5,220210,555,true,false,0,0\r\n\
             Two,Drinks,10,5,220210,555,true,false,0,0\r\n\
             Same,Drinks,10,5,220210,,true,false,0,0\r\n\
             Same,Drinks,11,5,220210,,true,false,0,0\r\n\
             Cola,Drinks,10,5,220210,,true,false,0,0\r\n\
             Cola again,Drinks,10,5,220210,111,true,false,0,0\r\n",
            vec![product("A", "Cola", Some("111"))],
        );
        let got: Vec<_> = a
            .preview
            .failures
            .iter()
            .map(|f| (f.row, f.reason.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (3, "duplicate_barcode"),
                (5, "duplicate_product"),
                (7, "duplicate_product")
            ]
        );
        assert_eq!((a.preview.new, a.preview.updated), (2, 1));
    }

    #[test]
    fn a_barcode_held_by_another_product_is_refused_and_a_bundle_cannot_be_updated() {
        let mut bundle = product("C", "Combo", None);
        bundle.bundle = true;
        bundle.track_stock = false;
        let a = run(
            "Water,Drinks,10,5,220210,111,true,false,0,0\r\n\
             Combo,Drinks,10,5,220210,,false,false,0,0\r\n",
            vec![product("A", "Cola", Some("111")), bundle],
        );
        let got: Vec<_> = a
            .preview
            .failures
            .iter()
            .map(|f| (f.row, f.reason.as_str()))
            .collect();
        assert_eq!(got, vec![(3, "bundle")]);
        assert_eq!(
            (a.preview.updated, a.preview.new),
            (1, 0),
            "a barcode match is an update (it renames Cola to Water)"
        );
    }

    #[test]
    fn an_unknown_column_blocks_commit_but_not_valid_rows_and_a_missing_one_blocks_all() {
        let a = analyse(
            "name,price,colour\r\nTea,10,red\r\n",
            &ImportContext::new(Vec::new(), None, HashMap::new()).unwrap(),
            2,
        )
        .unwrap();
        assert_eq!(a.preview.failures[0].reason, "unknown_column");
        assert_eq!(a.preview.failures[0].row, 1);
        assert!(!a.preview.can_commit && a.preview.can_commit_valid);
        assert_eq!(a.rows.len(), 1);
        let none = analyse("name,category\r\nTea,Drinks\r\n", &ctx(Vec::new()), 2).unwrap();
        assert!(none.preview.unusable && !none.preview.can_commit_valid);
        assert_eq!(none.preview.failures[0].reason, "missing_column");
        let empty = analyse("", &ctx(Vec::new()), 2).unwrap();
        assert!(empty.preview.unusable);
    }

    #[test]
    fn prices_follow_the_currency_decimals_and_opening_stock_is_counted() {
        let c = ImportContext::new(Vec::new(), None, HashMap::new()).unwrap();
        let a = analyse(
            "name,price,track_stock,opening_qty,unit_cost\nTea,1500,yes,10,900\n",
            &c,
            0,
        )
        .unwrap();
        assert_eq!(a.rows[0].price_minor, 1500);
        assert_eq!(a.preview.with_stock, 1);
        let b = analyse("name,price\nTea,15.50\n", &c, 0).unwrap();
        assert_eq!(b.preview.failures[0].reason, "bad_number");
    }

    #[test]
    fn a_header_only_file_commits_nothing_and_a_huge_file_is_refused() {
        let a = analyse(head(), &ctx(Vec::new()), 2).unwrap();
        assert!(!a.preview.can_commit && !a.preview.can_commit_valid);
        let huge = "x".repeat(IMPORT_MAX_BYTES + 1);
        assert!(analyse(&huge, &ctx(Vec::new()), 2).is_err());
    }
}
