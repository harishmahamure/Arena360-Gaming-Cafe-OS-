//! Story 9.14: the yearly GST archive built from invoices and credit notes the Hub really wrote,
//! when it is due, and `gst_archive.publish`.

use super::gst::{BUYER_GSTIN, GST_ON, invoice, line_id, order_row, product, refund_one, sell};
use super::prepaid::sql;
use super::session::{int, table, text};
use super::stock::{intent, owner};
use super::*;
use crate::gst_archive::decode_archive;
use arena_proto::{
    BusinessBuyer, GstArchivePublish, GstArchiveV1, ROLLUP_SCHEMA, RollupPublish, TaxMode,
    gst_archive_object_key,
};
use sha2::{Digest, Sha256};

fn reader(f: &Fixture) -> crate::SnapshotReader {
    crate::SnapshotReader::open(&f.path).unwrap()
}

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "hub-gst-archive".into(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Ledger {
    f: Fixture,
    fy: String,
    consumer_no: String,
    business_no: String,
    drink: String,
    business: String,
}

/// Two invoices (one a business invoice) and one credit note, all of one financial year.
async fn ledger() -> Ledger {
    let f = bootstrapped().await;
    sql(
        &f,
        format!("UPDATE settings SET value = '{GST_ON}' WHERE key = 'branch.tax'"),
    )
    .await;
    let snack = product(&f, "Snack", 3_100, 500, "2106").await;
    let drink = product(&f, "Drink", 1_180, 1800, "2202").await;
    let consumer = sell(&f, &[(&snack, 2), (&drink, 1)], 7_380, None).await;
    let business = sell(
        &f,
        &[(&drink, 2)],
        2_360,
        Some(BusinessBuyer {
            gstin: BUYER_GSTIN.into(),
            legal_name: "Acme Traders".into(),
        }),
    )
    .await;
    let consumer_no = invoice(&f, &consumer).await;
    let business_no = invoice(&f, &business).await;
    refund_one(&f, &consumer_no, line_id(&f, &consumer, &snack)).await;
    let fy = text(&order_row(&f, &consumer), "fy");
    Ledger {
        f,
        fy,
        consumer_no,
        business_no,
        drink,
        business,
    }
}

fn archive_of(f: &Fixture, fy: &str) -> (crate::gst_archive::GstArchiveFile, GstArchiveV1) {
    f.clock.advance(1_000);
    let file = reader(f)
        .gst_archive_file(fy, f.clock.now_ms())
        .unwrap()
        .unwrap();
    let archive = decode_archive(&file.bytes).unwrap();
    (file, archive)
}

async fn publish(f: &Fixture, file: &crate::gst_archive::GstArchiveFile) -> Applied {
    f.writer
        .publish_gst_archive(GstArchivePublish {
            fy: file.fy.clone(),
            key: gst_archive_object_key(&file.fy),
            sha256: file.sha256.clone(),
            bytes: i64::try_from(file.bytes.len()).unwrap(),
            invoices: file.invoices,
            credit_notes: file.credit_notes,
            orphaned: file.orphaned.clone(),
            built_at: file.built_at,
        })
        .await
        .unwrap()
}

async fn rollup_day(f: &Fixture, day: &str) {
    f.writer
        .submit_intent(intent(
            Actor {
                kind: ActorKind::System,
                id: "hub-rollup".into(),
            },
            IntentPayload::RollupPublish(RollupPublish {
                date: day.into(),
                schema: ROLLUP_SCHEMA,
                built_at: f.clock.now_ms(),
                key: format!("rollups/{day}.json"),
            }),
        ))
        .await
        .unwrap();
}

fn sum(rows: &[Row], col: &str) -> i64 {
    rows.iter().map(|r| int(r, col)).sum()
}

#[tokio::test]
async fn the_archive_holds_every_stored_invoice_credit_note_and_per_rate_total() {
    let l = ledger().await;
    let (file, a) = archive_of(&l.f, &l.fy);

    assert_eq!((a.schema, a.fy.as_str()), (1, l.fy.as_str()));
    assert_eq!((a.tax_mode, a.tax_label.as_str()), (TaxMode::GstIn, "Tax"));
    assert_eq!(
        (a.branch_code.as_str(), a.currency.as_str()),
        ("DEV01", "INR")
    );
    assert_eq!((file.invoices, file.credit_notes), (2, 1));
    assert_eq!(&file.bytes[..4], &[0x28, 0xb5, 0x2f, 0xfd], "a zstd frame");
    assert_eq!(file.sha256, hex(&Sha256::digest(&file.bytes)));

    // Every numbered invoice, with the number the Hub gave it and its lines.
    let orders = table(&l.f, Table::Orders);
    let mut numbers: Vec<_> = a.invoices.iter().map(|i| i.invoice_no.clone()).collect();
    numbers.sort();
    let mut stored: Vec<_> = orders.iter().map(|o| text(o, "invoice_no")).collect();
    stored.sort();
    assert_eq!(numbers, stored);
    assert!(a.invoices.iter().all(|i| !i.lines.is_empty()));
    let business = a
        .invoices
        .iter()
        .find(|i| i.invoice_no == l.business_no)
        .unwrap();
    assert_eq!(
        (
            business.buyer_gstin.as_deref(),
            business.buyer_name.as_deref()
        ),
        (Some(BUYER_GSTIN), Some("Acme Traders"))
    );
    assert_eq!(
        a.invoices.iter().map(|i| i.total_minor).sum::<i64>(),
        sum(&orders, "total_minor")
    );

    // The credit note is the stored one, against the invoice it refunds.
    let refunds = table(&l.f, Table::Refunds);
    assert_eq!(a.credit_notes.len(), 1);
    assert_eq!(
        a.credit_notes[0].credit_note_no,
        text(&refunds[0], "credit_note_no")
    );
    assert_eq!(a.credit_notes[0].invoice_no, l.consumer_no);
    assert_eq!(
        a.credit_notes[0].amount_minor,
        int(&refunds[0], "amount_minor")
    );

    // The per-rate summaries only add up what is stored.
    let inv_tax: i64 = a.invoice_summary.iter().map(|r| r.tax_minor).sum();
    let inv_taxable: i64 = a.invoice_summary.iter().map(|r| r.taxable_minor).sum();
    let cn_tax: i64 = a.credit_note_summary.iter().map(|r| r.tax_minor).sum();
    let cn_taxable: i64 = a.credit_note_summary.iter().map(|r| r.taxable_minor).sum();
    assert_eq!(inv_tax, sum(&orders, "tax_minor"));
    assert_eq!(cn_tax, -sum(&refunds, "tax_minor"));
    assert_eq!(
        inv_taxable + cn_taxable,
        sum(&orders, "subtotal_minor")
            - (sum(&refunds, "amount_minor") - sum(&refunds, "tax_minor"))
    );
    assert!(a.credit_note_summary.iter().all(|r| r.tax_minor <= 0));
    let rates: Vec<_> = a.invoice_summary.iter().map(|r| r.rate_bp).collect();
    assert_eq!(rates, vec![500, 1800]);
    assert!(
        a.invoice_summary
            .iter()
            .all(|r| r.cgst_minor.zip(r.sgst_minor).map(|(c, s)| c + s) == Some(r.tax_minor)),
        "central and state tax are the stored halves"
    );
    assert!(a.orphaned_invoices.is_empty());

    // Nothing else is in the file: another year is empty.
    let none = reader(&l.f)
        .gst_archive_file("FY1999-00", l.f.clock.now_ms())
        .unwrap()
        .unwrap();
    assert_eq!((none.invoices, none.credit_notes), (0, 0));
}

#[tokio::test]
async fn a_simple_tax_branch_gets_the_same_archive_under_the_same_key() {
    let l = ledger().await;
    let (gst, _) = archive_of(&l.f, &l.fy);
    sql(
        &l.f,
        format!(
            "UPDATE settings SET value = '{}' WHERE key = 'branch.tax'",
            arena_proto::DEFAULT_TAX
        ),
    )
    .await;
    let (simple, a) = archive_of(&l.f, &l.fy);
    assert_eq!(a.tax_mode, TaxMode::Simple);
    assert_eq!(a.tax_label, "Tax");
    assert_eq!(
        (simple.invoices, simple.credit_notes),
        (gst.invoices, gst.credit_notes)
    );
    assert_eq!(
        gst_archive_object_key(&simple.fy),
        gst_archive_object_key(&gst.fy)
    );
    publish(&l.f, &simple).await;
    let row = reopen(&l.f).rows(Table::GstArchiveLog).unwrap();
    assert_eq!(row.len(), 1);
    assert_eq!(text(&row[0], "key"), format!("gst-archive/{}.zst", l.fy));
}

#[tokio::test]
async fn a_year_is_due_after_the_first_close_that_follows_its_end_and_again_when_a_late_row_lands()
{
    let l = ledger().await;
    let f = &l.f;
    let r = reader(f);

    // No rollup published yet: nothing is due.
    let due = r.gst_archive_due(None).unwrap();
    assert_eq!((due.day, due.years), (None, vec![]));
    // The close of the last day of the year is still inside the year.
    rollup_day(f, "2027-03-31").await;
    let due = r.gst_archive_due(None).unwrap();
    assert_eq!(due.day.as_deref(), Some("2027-03-31"));
    assert!(due.years.is_empty());
    // The first close of the next year makes it due; a day already looked at is not looked at.
    rollup_day(f, "2027-04-01").await;
    let due = r.gst_archive_due(None).unwrap();
    assert_eq!(due.day.as_deref(), Some("2027-04-01"));
    assert_eq!(due.years, vec![l.fy.clone()]);
    let again = r.gst_archive_due(Some("2027-04-01")).unwrap();
    assert_eq!(
        (again.day.as_deref(), again.years.len()),
        (Some("2027-04-01"), 0)
    );

    // Published: not due, and System -> Backup lists it.
    let (file, _) = archive_of(f, &l.fy);
    let applied = publish(f, &file).await;
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(
        (applied.changes[0].tbl.as_str(), applied.changes[0].op),
        ("gst_archive_log", ChangeOp::Insert)
    );
    assert!(r.gst_archive_due(None).unwrap().years.is_empty());
    let list = r.gst_archives().unwrap();
    assert_eq!(list.archives.len(), 1);
    assert_eq!(
        (
            list.archives[0].fy.as_str(),
            list.archives[0].label.as_str()
        ),
        (l.fy.as_str(), "26-27")
    );
    assert_eq!(
        (list.archives[0].invoices, list.archives[0].credit_notes),
        (2, 1)
    );

    // A late credit note stamped with the archived year makes it due; the rebuild overwrites.
    f.clock.advance(60_000);
    refund_one(f, &l.business_no, line_id(f, &l.business, &l.drink)).await;
    assert_eq!(text(&table(f, Table::Refunds)[1], "fy"), l.fy);
    assert_eq!(r.gst_archive_due(None).unwrap().years, vec![l.fy.clone()]);
    let (file, a) = archive_of(f, &l.fy);
    assert_eq!(a.credit_notes.len(), 2);
    let applied = publish(f, &file).await;
    assert_eq!(applied.changes[0].op, ChangeOp::Update);
    assert_eq!(reopen(f).rows(Table::GstArchiveLog).unwrap().len(), 1);
    assert!(r.gst_archive_due(None).unwrap().years.is_empty());
    assert_eq!(r.gst_archives().unwrap().archives[0].credit_notes, 2);
}

#[tokio::test]
async fn numbers_a_fork_set_aside_are_listed_and_kept_on_later_rebuilds() {
    let l = ledger().await;
    let f = &l.f;
    let orphaned = f.path.parent().unwrap().join("orphaned");
    std::fs::create_dir_all(&orphaned).unwrap();
    let old_path = orphaned.join("2-HUB-20270201T000000.db");
    {
        let mut old = arena_store::Store::open(&old_path, T0).unwrap();
        old.write_tx(|tx| {
            tx.execute_batch(&format!(
                "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy,
                                     rev, created_at, updated_at, settled_at, kind, invoice_no)
                 VALUES ('X1', 'settled', 1, 1, '2027-01-20', '{fy}', 0, 1, 1, 1, 'pos_sale',
                         'DEV01/{fy}/E1/9001'),
                        ('X2', 'settled', 1, 1, '2027-01-20', '{fy}', 0, 1, 1, 1, 'pos_sale',
                         '{live}')",
                fy = l.fy,
                live = l.consumer_no
            ))
            .map_err(arena_store::db_error)
        })
        .unwrap();
    }
    let (file, a) = archive_of(f, &l.fy);
    assert_eq!(
        a.orphaned_invoices,
        vec![format!("DEV01/{}/E1/9001", l.fy)],
        "a number the live database holds is not orphaned"
    );
    assert_eq!(file.unreadable_orphans, 0);
    publish(f, &file).await;
    let logged = reopen(f).rows(Table::GstArchiveLog).unwrap();
    assert!(text(&logged[0], "orphaned").contains("9001"));

    // The 90-day sweep removes the set-aside database; the number stays in the next archive.
    std::fs::remove_file(&old_path).unwrap();
    let (_, again) = archive_of(f, &l.fy);
    assert_eq!(again.orphaned_invoices, a.orphaned_invoices);
}

#[tokio::test]
async fn only_the_system_publishes_an_archive_and_the_payload_is_checked() {
    let f = bootstrapped().await;
    let ok = || GstArchivePublish {
        fy: "FY2026-27".into(),
        key: "gst-archive/FY2026-27.zst".into(),
        sha256: "ab".repeat(32),
        bytes: 10,
        invoices: 2,
        credit_notes: 1,
        orphaned: vec!["A/1".into()],
        built_at: T0,
    };
    let payload = |p: GstArchivePublish| IntentPayload::GstArchivePublish(p);
    assert!(
        f.writer
            .submit_intent(intent(owner(), payload(ok())))
            .await
            .is_err(),
        "an owner cannot publish an archive"
    );
    let bad: Vec<GstArchivePublish> = vec![
        GstArchivePublish {
            fy: "FY2026-28".into(),
            ..ok()
        },
        GstArchivePublish {
            fy: "../x".into(),
            key: "gst-archive/../x.zst".into(),
            ..ok()
        },
        GstArchivePublish {
            key: "gst-archive/FY2025-26.zst".into(),
            ..ok()
        },
        GstArchivePublish {
            sha256: "AB".repeat(32),
            ..ok()
        },
        GstArchivePublish {
            sha256: "ab".into(),
            ..ok()
        },
        GstArchivePublish { bytes: -1, ..ok() },
        GstArchivePublish {
            invoices: -1,
            ..ok()
        },
        GstArchivePublish {
            built_at: -1,
            ..ok()
        },
        GstArchivePublish {
            orphaned: vec![String::new()],
            ..ok()
        },
    ];
    for (i, p) in bad.into_iter().enumerate() {
        let e = f
            .writer
            .submit_intent(intent(system(), payload(p)))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed, "case {i}");
    }
    let mut with_base = intent(system(), payload(ok()));
    with_base.base_rev = Some(1);
    assert!(f.writer.submit_intent(with_base).await.is_err());
    assert!(reopen(&f).rows(Table::GstArchiveLog).unwrap().is_empty());

    // Valid: one log row, nothing else written; a second publish updates the row.
    let applied = f
        .writer
        .submit_intent(intent(system(), payload(ok())))
        .await
        .unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(applied.changes[0].tbl, "gst_archive_log");
    assert_eq!(applied.changes[0].row_id, "FY2026-27");
    let again = f
        .writer
        .submit_intent(intent(
            system(),
            payload(GstArchivePublish { bytes: 11, ..ok() }),
        ))
        .await
        .unwrap();
    assert_eq!(again.changes[0].op, ChangeOp::Update);
    let rows = reopen(&f).rows(Table::GstArchiveLog).unwrap();
    assert_eq!((rows.len(), int(&rows[0], "bytes")), (1, 11));
}
