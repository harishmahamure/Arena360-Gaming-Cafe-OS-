use super::plans::group;
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::{ImportPlanRow, ImportPlans};

fn pack(name: &str, price: i64, groups: &[&str]) -> ImportPlanRow {
    ImportPlanRow {
        name: name.into(),
        plan_kind: "pack".into(),
        price_minor: price,
        minutes: Some(120),
        validity_days: Some(30),
        window_start_min: None,
        window_end_min: None,
        group_ids: groups.iter().map(|g| (*g).to_owned()).collect(),
    }
}

fn pass(name: &str, price: i64, groups: &[&str]) -> ImportPlanRow {
    ImportPlanRow {
        plan_kind: "pass".into(),
        minutes: None,
        window_start_min: Some(22 * 60),
        window_end_min: Some(6 * 60),
        ..pack(name, price, groups)
    }
}

async fn import(f: &Fixture, actor: Actor, rows: Vec<ImportPlanRow>) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(
            actor,
            IntentPayload::ImportPlans(ImportPlans { rows }),
        ))
        .await
}

fn state(f: &Fixture) -> (Vec<Row>, Vec<Row>, usize) {
    let s = reopen(f);
    (
        s.rows(Table::Plans).unwrap(),
        s.rows(Table::PlanGroups).unwrap(),
        s.changes().unwrap().len(),
    )
}

fn plan_named(f: &Fixture, name: &str) -> (arena_store::PlanRow, Vec<String>) {
    reopen(f)
        .plans()
        .unwrap()
        .into_iter()
        .find(|(p, _)| p.name == name)
        .unwrap()
}

#[tokio::test]
async fn a_chunk_creates_plans_updates_the_one_with_the_name_and_writes_its_groups() {
    let f = bootstrapped().await;
    let pc = group(&f, "PC", "pc").await;
    let vip = group(&f, "Vip", "pc").await;
    let existing = f
        .writer
        .submit_intent(intent(
            owner(),
            IntentPayload::PlanUpsert(arena_proto::PlanUpsert {
                id: None,
                plan_kind: "pack".into(),
                name: "2 h".into(),
                price_minor: 1000,
                minutes: Some(120),
                validity_days: Some(7),
                window_start_min: None,
                window_end_min: None,
                group_ids: vec![pc.clone()],
                active: false,
            }),
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    let a = import(
        &f,
        owner(),
        vec![
            pack("2 h", 15_000, &[&pc, &vip]),
            pass("Night pass", 90_000, &[&vip]),
        ],
    )
    .await
    .unwrap();
    assert!(a.changes.iter().any(|c| c.tbl == "plans"));
    let (two, groups) = plan_named(&f, "2 h");
    assert_eq!(two.plan_id, existing, "the exact name matched");
    assert_eq!(
        (two.price_minor, two.validity_days, two.active),
        (15_000, Some(30), false),
        "the price and terms changed; whether it is on sale did not"
    );
    assert_eq!(groups.len(), 2);
    let (night, groups) = plan_named(&f, "Night pass");
    assert_eq!(
        (
            night.kind.as_str(),
            night.window_start_min,
            night.window_end_min,
            night.active
        ),
        ("pass", Some(1320), Some(360), true)
    );
    assert_eq!(groups, vec![vip]);
    assert_eq!(reopen(&f).plans().unwrap().len(), 2);
}

#[tokio::test]
async fn a_retry_with_the_same_key_never_duplicates() {
    let f = bootstrapped().await;
    let pc = group(&f, "PC", "pc").await;
    let it = intent(
        owner(),
        IntentPayload::ImportPlans(ImportPlans {
            rows: vec![pack("2 h", 15_000, &[&pc])],
        }),
    );
    let first = f.writer.submit_intent(it.clone()).await.unwrap();
    let before = state(&f);
    let again = f.writer.submit_intent(it).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(state(&f), before);
    assert_eq!(reopen(&f).plans().unwrap().len(), 1);
}

#[tokio::test]
async fn one_bad_row_fails_the_whole_chunk_and_writes_nothing() {
    let f = bootstrapped().await;
    let pc = group(&f, "PC", "pc").await;
    let ps5 = group(&f, "PS5", "console").await;
    import(&f, owner(), vec![pack("Old", 100, &[&pc])])
        .await
        .unwrap();
    let before = state(&f);
    let good = pack("New", 500, &[&pc]);
    for (rows, field) in [
        (
            vec![good.clone(), pack("Mixed", 500, &[&pc, &ps5])],
            "rows[1].groups",
        ),
        (
            vec![
                good.clone(),
                pack("Ghost", 500, &["01JNOSUCHGROUPAAAAAAAAAAAA"]),
            ],
            "rows[1].groups",
        ),
        (vec![good.clone(), pack("Free", 0, &[&pc])], "rows[1].price"),
        (vec![good.clone(), good.clone()], "rows[1].name"),
        (vec![good.clone(), pass("Old", 100, &[&pc])], "rows[1].type"),
        (vec![good.clone(), pack("", 100, &[&pc])], "rows[1].name"),
        (vec![good.clone(), pack("None", 100, &[])], "rows[1].groups"),
        (Vec::new(), "rows"),
    ] {
        let e = import(&f, owner(), rows).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some(field)),
            "{e:?}"
        );
        assert_eq!(state(&f), before);
    }
    let many: Vec<ImportPlanRow> = (0..501)
        .map(|i| pack(&format!("P{i}"), 100, &[&pc]))
        .collect();
    let e = import(&f, owner(), many).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("rows"))
    );
    assert_eq!(state(&f), before);
}

#[tokio::test]
async fn catalog_edit_is_required() {
    let f = bootstrapped().await;
    let pc = group(&f, "PC", "pc").await;
    let counter = staff(&f, "till", ROLE_COUNTER).await;
    let e = import(&f, counter, vec![pack("2 h", 100, &[&pc])])
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    let manager = staff(&f, "mgr", ROLE_MANAGER).await;
    import(&f, manager, vec![pack("2 h", 100, &[&pc])])
        .await
        .unwrap();
    assert_eq!(reopen(&f).plans().unwrap().len(), 1);
}

#[tokio::test]
async fn a_thousand_plan_file_previews_and_commits_in_two_chunks_well_inside_30_seconds() {
    let started = std::time::Instant::now();
    let f = bootstrapped().await;
    group(&f, "PC", "pc").await;
    group(&f, "Vip", "pc").await;
    let mut csv =
        String::from("name,type,hours,price,validity_days,window_start,window_end,groups\r\n");
    for i in 0..1000 {
        if i % 2 == 0 {
            csv.push_str(&format!(
                "Pack {i},pack,{}.5,{}.00,30,,,PC;Vip\r\n",
                1 + i % 9,
                50 + i
            ));
        } else {
            csv.push_str(&format!(
                "Pass {i},pass,,{}.00,7,22:00,06:00,Vip\r\n",
                300 + i
            ));
        }
    }
    let analysis = {
        let store = reopen(&f);
        let ctx = crate::import_plans::context(&store).unwrap();
        crate::import_plans::analyse(&csv, &ctx, 2).unwrap()
    };
    assert_eq!(analysis.preview.failures, Vec::new());
    assert_eq!((analysis.preview.new, analysis.preview.updated), (1000, 0));
    for chunk in analysis.rows.chunks(arena_proto::IMPORT_CHUNK_ROWS) {
        import(&f, owner(), chunk.to_vec()).await.unwrap();
    }
    assert_eq!(reopen(&f).plans().unwrap().len(), 1000);
    let (p, groups) = plan_named(&f, "Pack 998");
    assert_eq!(
        (p.minutes, groups.len()),
        (Some((1 + 998 % 9) * 60 + 30), 2)
    );
    let again = {
        let store = reopen(&f);
        let ctx = crate::import_plans::context(&store).unwrap();
        crate::import_plans::analyse(&csv, &ctx, 2).unwrap()
    };
    assert_eq!(
        (again.preview.new, again.preview.updated),
        (0, 1000),
        "a second dry-run finds every plan by its name"
    );
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
    eprintln!(
        "1,000 plans previewed and committed in {:?}",
        started.elapsed()
    );
}
