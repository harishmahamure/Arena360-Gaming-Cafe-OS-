//! CSV import of plans with a dry-run (Story 5.34).
//!
//! One piece of logic serves the preview and the `import.plans` intent: [`check_row`] runs the
//! `plan.upsert` rules (Story 3.21) on a row whose group names were matched to the branch's
//! groups, [`duplicates`] finds rows of one file that name the same plan, and [`analyse`] reads a
//! file and counts. A row updates the plan with exactly its name, else it is new. Nothing here
//! writes.

use std::collections::HashMap;

use arena_proto::{
    AppError, ErrorCode, IMPORT_PLAN_COLUMNS, ImportFailure, ImportPlanRow, PlanImportPreview,
};
use arena_store::{GroupRow, PlanRow};

use crate::apply::plans::check_import_row;
use crate::import_csv::CsvRecord;
use crate::import_file::{File, Header, Read, line_of, problems, read_file};
use crate::import_products::{Problem, number, problem, take};

/// The file name of the template.
pub const TEMPLATE_FILE: &str = "plans-template.csv";

/// The template: the exact header and two sample rows. The sample rows name the groups `PC` and
/// `Vip` (several groups are separated by `;`), so they pass in a branch that has those groups.
pub fn template() -> String {
    let mut out = IMPORT_PLAN_COLUMNS.join(",");
    out.push_str("\r\n");
    out.push_str("2 hour pack,pack,2,100,30,,,PC\r\n");
    out.push_str("Night pass,pass,,400,30,22:00,06:00,PC;Vip\r\n");
    out
}

/// What the Hub holds that a plan import is checked against.
pub(crate) struct PlanContext {
    /// Every plan with the ids of its groups.
    pub plans: Vec<(PlanRow, Vec<String>)>,
    /// The live groups.
    pub groups: Vec<GroupRow>,
}

/// The context read from the store (outside a write transaction).
pub(crate) fn context(store: &arena_store::Store) -> Result<PlanContext, AppError> {
    let groups = store.groups()?;
    Ok(PlanContext {
        plans: store.plans()?,
        groups,
    })
}

impl PlanContext {
    /// The plan with exactly `name`, if any.
    pub fn resolve(&self, name: &str) -> Option<usize> {
        self.plans.iter().position(|(p, _)| p.name == name)
    }

    /// The live group a file calls `name`: an exact match, else the only one that differs by
    /// case.
    fn group(&self, name: &str) -> Option<&GroupRow> {
        self.groups.iter().find(|g| g.name == name).or_else(|| {
            let mut same = self
                .groups
                .iter()
                .filter(|g| g.name.eq_ignore_ascii_case(name));
            match (same.next(), same.next()) {
                (Some(g), None) => Some(g),
                _ => None,
            }
        })
    }
}

/// The `plan.upsert` rules for one row, against the plans and groups the Hub holds. On success:
/// the index of the plan it updates, or `None` for a new one.
pub(crate) fn check_row(ctx: &PlanContext, row: &ImportPlanRow) -> Result<Option<usize>, Problem> {
    let target = ctx.resolve(row.name.trim());
    let groups = row
        .group_ids
        .iter()
        .map(|id| ctx.groups.iter().find(|g| g.group_id == *id).cloned())
        .collect();
    check_import_row(row, target.map(|i| &ctx.plans[i].0), groups)
        .map_err(|e| from_app_error(&e))?;
    Ok(target)
}

fn column_of(field: &str) -> &str {
    match field {
        "plan_kind" => "type",
        "minutes" => "hours",
        "price_minor" => "price",
        "window_start_min" => "window_start",
        "window_end_min" => "window_end",
        "group_ids" => "groups",
        other => other,
    }
}

fn from_app_error(e: &AppError) -> Problem {
    let field = column_of(e.field.as_deref().unwrap_or("name"));
    let reason = match e.code {
        ErrorCode::GroupKindMismatch => "group_kind",
        _ if field == "groups" => "bad_groups",
        _ if e.message.contains("cannot change") => "kind_change",
        _ => "bad_value",
    };
    problem(field, reason, e.message.clone())
}

/// Rows of one file that name the same plan, each as `(index into items, problem)`. `items` holds
/// only the rows that passed [`check_row`], as `(line, row)`; the first of a pair stays valid.
pub(crate) fn duplicates(items: &[(i64, &ImportPlanRow)]) -> Vec<(usize, Problem)> {
    let mut names: HashMap<&str, i64> = HashMap::new();
    let mut out = Vec::new();
    for (n, (line, row)) in items.iter().enumerate() {
        match names.get(row.name.trim()) {
            Some(first) => out.push((
                n,
                problem(
                    "name",
                    "duplicate_plan",
                    format!("row {first} already adds or updates this plan"),
                ),
            )),
            None => {
                names.insert(row.name.trim(), *line);
            }
        }
    }
    out
}

/// `HH:MM` as minutes after midnight.
fn clock(field: &str, text: &str) -> Result<i64, Problem> {
    let bad = || problem(field, "bad_time", "time must be written HH:MM, like 22:00");
    let (h, m) = text.split_once(':').ok_or_else(bad)?;
    let digits = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    if !(h.len() == 1 || digits(h, 2)) || !digits(m, 2) {
        return Err(bad());
    }
    let (h, m): (i64, i64) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if h > 23 || m > 59 {
        return Err(bad());
    }
    Ok(h * 60 + m)
}

/// Hours with up to two decimals as whole minutes (`1.5` is 90); a fraction of a minute is
/// refused.
fn hours(text: &str) -> Result<i64, Problem> {
    let hundredths = number("hours", text, 2, "hours")?;
    let minutes = hundredths * 60;
    if minutes % 100 != 0 {
        return Err(problem(
            "hours",
            "bad_number",
            "hours must come to a whole number of minutes",
        ));
    }
    Ok(minutes / 100)
}

fn parse_row(
    rec: &CsvRecord,
    header: &Header,
    exp: u32,
    ctx: &PlanContext,
) -> Result<ImportPlanRow, Vec<Problem>> {
    let mut found = Vec::new();
    if header.overflows(rec) {
        found.push(problem(
            "",
            "bad_value",
            "the row has more values than the header has columns",
        ));
    }
    let name = header.cell(rec, "name");
    if name.is_empty() {
        found.push(problem("name", "required", "name is required"));
    }
    let plan_kind = header.cell(rec, "type").to_ascii_lowercase();
    if plan_kind.is_empty() {
        found.push(problem("type", "required", "type is required"));
    } else if !matches!(plan_kind.as_str(), "pack" | "block" | "pass") {
        found.push(problem(
            "type",
            "bad_value",
            "type must be pack, block or pass",
        ));
    }
    let price = header.cell(rec, "price");
    let price_minor = if price.is_empty() {
        found.push(problem("price", "required", "price is required"));
        0
    } else {
        take(&mut found, number("price", price, exp, "price"), 0)
    };
    let optional =
        |found: &mut Vec<Problem>, text: &str, read: &dyn Fn(&str) -> Result<i64, Problem>| {
            if text.is_empty() {
                None
            } else {
                take(found, read(text).map(Some), None)
            }
        };
    let minutes = optional(&mut found, header.cell(rec, "hours"), &hours);
    let validity_days = optional(&mut found, header.cell(rec, "validity_days"), &|t| {
        number("validity_days", t, 0, "validity")
    });
    let window_start_min = optional(&mut found, header.cell(rec, "window_start"), &|t| {
        clock("window_start", t)
    });
    let window_end_min = optional(&mut found, header.cell(rec, "window_end"), &|t| {
        clock("window_end", t)
    });
    let mut group_ids = Vec::new();
    for g in header
        .cell(rec, "groups")
        .split([';', '|'])
        .map(str::trim)
        .filter(|g| !g.is_empty())
    {
        match ctx.group(g) {
            Some(group) => group_ids.push(group.group_id.clone()),
            None => found.push(problem(
                "groups",
                "unknown_group",
                format!("there is no device group called \"{g}\""),
            )),
        }
    }
    if !found.is_empty() {
        return Err(found);
    }
    Ok(ImportPlanRow {
        name: name.to_owned(),
        plan_kind,
        price_minor,
        minutes,
        validity_days,
        window_start_min,
        window_end_min,
        group_ids,
    })
}

/// A file and the rows of it that can be imported.
pub(crate) struct Analysis {
    pub preview: PlanImportPreview,
    /// The rows that passed, in file order.
    pub rows: Vec<ImportPlanRow>,
}

fn unusable(failures: Vec<ImportFailure>) -> Analysis {
    Analysis {
        preview: PlanImportPreview {
            rows: 0,
            new: 0,
            updated: 0,
            failed: 0,
            failures,
            truncated: false,
            unusable: true,
            can_commit: false,
            can_commit_valid: false,
        },
        rows: Vec::new(),
    }
}

/// Reads `csv` against `ctx` (a dry-run): the counts, every problem with its line, and the rows
/// that can be imported. `exponent` is the currency's decimal places. `Err` only for a file over
/// the size or row limits.
pub(crate) fn analyse(csv: &str, ctx: &PlanContext, exponent: u8) -> Result<Analysis, AppError> {
    let File {
        header,
        records,
        unknown,
    } = match read_file(csv, &IMPORT_PLAN_COLUMNS, &["name", "type", "price"])? {
        Read::Unusable(failures) => return Ok(unusable(failures)),
        Read::File(f) => f,
    };
    let exp = u32::from(exponent);
    let mut bad: Vec<(i64, Problem)> = Vec::new();
    let mut ok: Vec<(i64, ImportPlanRow, Option<usize>)> = Vec::new();
    for rec in &records {
        let line = line_of(rec);
        match parse_row(rec, &header, exp, ctx) {
            Err(found) => bad.extend(found.into_iter().map(|p| (line, p))),
            Ok(row) => match check_row(ctx, &row) {
                Err(p) => bad.push((line, p)),
                Ok(target) => ok.push((line, row, target)),
            },
        }
    }
    let colliding = {
        let items: Vec<(i64, &ImportPlanRow)> = ok.iter().map(|(l, r, _)| (*l, r)).collect();
        duplicates(&items)
    };
    let mut dropped = vec![false; ok.len()];
    for (n, p) in colliding {
        dropped[n] = true;
        bad.push((ok[n].0, p));
    }
    let mut valid = Vec::new();
    let (mut new, mut updated) = (0i64, 0i64);
    for ((_, row, target), dropped) in ok.into_iter().zip(dropped) {
        if dropped {
            continue;
        }
        if target.is_some() {
            updated += 1;
        } else {
            new += 1;
        }
        valid.push(row);
    }
    let found = problems(unknown.clone(), bad);
    Ok(Analysis {
        preview: PlanImportPreview {
            rows: i64::try_from(records.len()).unwrap_or(i64::MAX),
            new,
            updated,
            failed: found.failed,
            failures: found.list,
            truncated: found.truncated,
            unusable: false,
            can_commit: found.failed == 0 && unknown.is_empty() && !valid.is_empty(),
            can_commit_valid: !valid.is_empty(),
        },
        rows: valid,
    })
}

#[cfg(test)]
mod tests {
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

    fn plan(id: &str, name: &str, kind: &str, active: bool) -> PlanRow {
        PlanRow {
            plan_id: id.to_owned(),
            kind: kind.to_owned(),
            name: name.to_owned(),
            price_minor: 5000,
            minutes: (kind == "pack").then_some(60),
            validity_days: Some(30),
            window_start_min: (kind != "pack").then_some(600),
            window_end_min: (kind != "pack").then_some(900),
            active,
            rev: 3,
        }
    }

    fn ctx() -> PlanContext {
        PlanContext {
            plans: vec![(
                plan("P1", "2 hour pack", "pack", false),
                vec!["G1".to_owned()],
            )],
            groups: vec![
                group("G1", "PC", "pc"),
                group("G2", "PS5", "console"),
                group("G3", "Vip", "pc"),
            ],
        }
    }

    fn run(csv: &str) -> Analysis {
        analyse(csv, &ctx(), 2).unwrap()
    }

    const HEAD: &str = "name,type,hours,price,validity_days,window_start,window_end,groups\n";

    #[test]
    fn the_template_passes_its_own_dry_run_in_a_branch_with_those_groups() {
        let mut c = ctx();
        c.plans.clear();
        let a = analyse(&template(), &c, 2).unwrap();
        assert_eq!(a.preview.failures, vec![], "{:?}", a.preview.failures);
        assert_eq!((a.preview.new, a.preview.failed), (2, 0));
        let pass = &a.rows[1];
        assert_eq!(pass.window_start_min, Some(22 * 60));
        assert_eq!(pass.window_end_min, Some(6 * 60));
    }

    #[test]
    fn packs_blocks_and_passes_are_read_into_minutes_and_minor_units() {
        let a = run(&format!(
            "{HEAD}Pack 90,pack,1.5,75.50,60,,,PC\nEvening,block,,200,,18:00,22:30,Vip\nWeek,Pass,,900,7,9:00,21:00,PC;Vip\n"
        ));
        assert_eq!(a.preview.failures, vec![], "{:?}", a.preview.failures);
        assert_eq!(a.rows[0].minutes, Some(90));
        assert_eq!(a.rows[0].price_minor, 7550);
        assert_eq!(a.rows[1].window_end_min, Some(22 * 60 + 30));
        assert_eq!(a.rows[2].plan_kind, "pass");
        assert_eq!(a.rows[2].group_ids, vec!["G1", "G3"]);
        assert_eq!((a.preview.new, a.preview.updated), (3, 0));
    }

    #[test]
    fn a_plan_with_exactly_the_name_is_updated() {
        let a = run(&format!(
            "{HEAD}2 hour pack,pack,3,150,30,,,PC\n2 Hour Pack,pack,3,150,30,,,PC\n"
        ));
        assert_eq!(
            (a.preview.new, a.preview.updated, a.preview.failed),
            (1, 1, 0)
        );
    }

    #[test]
    fn the_kind_of_an_existing_plan_cannot_change() {
        let a = run(&format!("{HEAD}2 hour pack,pass,,150,30,10:00,12:00,PC\n"));
        assert_eq!(a.preview.failed, 1);
        assert_eq!(a.preview.failures[0].field, "type");
        assert_eq!(a.preview.failures[0].reason, "kind_change");
    }

    #[test]
    fn an_unknown_group_or_a_mix_of_console_and_pc_groups_fails_the_row() {
        let a = run(&format!(
            "{HEAD}A,pack,1,10,30,,,Nope\nB,pack,1,10,30,,,PC;PS5\nC,pack,1,10,30,,,\n"
        ));
        let got: Vec<(i64, &str, &str)> = a
            .preview
            .failures
            .iter()
            .map(|f| (f.row, f.field.as_str(), f.reason.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (2, "groups", "unknown_group"),
                (3, "groups", "group_kind"),
                (4, "groups", "bad_groups"),
            ]
        );
        assert!(!a.preview.can_commit && !a.preview.can_commit_valid);
    }

    #[test]
    fn the_plan_upsert_rules_apply_to_every_kind() {
        let a = run(&format!(
            "{HEAD}\
             A,pack,,10,30,,,PC\n\
             B,pack,1,10,,,,PC\n\
             C,pack,1,10,30,10:00,12:00,PC\n\
             D,block,,10,30,10:00,12:00,PC\n\
             E,block,,10,,10:00,10:00,PC\n\
             F,pass,,10,,10:00,12:00,PC\n\
             G,pack,1,0,30,,,PC\n\
             H,pack,0.01,10,30,,,PC\n\
             I,pack,1,10,30,,,PC\n"
        ));
        let rows: Vec<i64> = a.preview.failures.iter().map(|f| f.row).collect();
        assert_eq!(rows, vec![2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(a.preview.new, 1, "only the last row is whole");
    }

    #[test]
    fn bad_numbers_and_times_are_named_by_column() {
        let a = run(&format!(
            "{HEAD}A,pack,abc,10,30,,,PC\nB,pack,1,x,30,,,PC\nC,pack,1,10,3.5,,,PC\nD,block,,10,,25:00,12:00,PC\nE,block,,10,,10,12:00,PC\nF,bundle,1,10,30,,,PC\n"
        ));
        let got: Vec<(i64, &str, &str)> = a
            .preview
            .failures
            .iter()
            .map(|f| (f.row, f.field.as_str(), f.reason.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (2, "hours", "bad_number"),
                (3, "price", "bad_number"),
                (4, "validity_days", "bad_number"),
                (5, "window_start", "bad_time"),
                (6, "window_start", "bad_time"),
                (7, "type", "bad_value"),
            ]
        );
    }

    #[test]
    fn a_name_repeated_in_the_file_fails_the_later_row() {
        let a = run(&format!("{HEAD}X,pack,1,10,30,,,PC\nX,pack,2,20,30,,,PC\n"));
        assert_eq!(a.preview.failures.len(), 1);
        assert_eq!(a.preview.failures[0].row, 3);
        assert_eq!(a.preview.failures[0].reason, "duplicate_plan");
        assert_eq!(a.preview.new, 1);
    }

    #[test]
    fn a_missing_required_column_makes_the_file_unusable() {
        let a = run("name,price\nA,1\n");
        assert!(a.preview.unusable);
        assert_eq!(a.preview.failures[0].field, "type");
    }

    #[test]
    fn group_names_match_by_case_when_only_one_group_fits() {
        let a = run(&format!("{HEAD}A,pack,1,10,30,,,pc; vip \n"));
        assert_eq!(a.preview.failures, vec![]);
        assert_eq!(a.rows[0].group_ids, vec!["G1", "G3"]);
    }
}
