//! The committed pricing snapshot of the web apps (Story 10.1): `packages/ui/src/pricing.snapshot.ts`
//! is generated from the seed price book, so the web apps build offline and a price changed in a
//! migration without regenerating it fails this test (which CI runs). Regenerate with
//! `UPDATE_PRICING_SNAPSHOT=1 cargo test -p arena-cloud --test pricing_snapshot`.

use arena_proto::Band;
use arena_store::control::ControlDb;
use serde_json::{Map, Value, json};

const SNAPSHOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/ui/src/pricing.snapshot.ts"
);

const HEADER: &str = "// Generated from the seed price book by `UPDATE_PRICING_SNAPSHOT=1 cargo test -p arena-cloud\n\
// --test pricing_snapshot` (Story 10.1). Do not edit: CI fails when it differs from the book.\n";

async fn generated() -> String {
    let dir = tempfile::tempdir().unwrap();
    let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
        .await
        .unwrap();
    db.migrate(0).await.unwrap();
    let book = db.live_price_book().await.unwrap().unwrap();
    let mut bands = Map::new();
    for band in Band::ALL {
        let lines = db.price_lines(&book.id, band).await.unwrap();
        bands.insert(
            band.as_str().to_owned(),
            json!({
                "currency": band.currency(),
                "enabled": db.band_enabled(band).await.unwrap(),
                "lines": lines,
            }),
        );
    }
    let body = json!({ "version": book.version, "bands": Value::Object(bands) });
    format!(
        "{HEADER}export const PRICING_SNAPSHOT = {} as const;\n",
        serde_json::to_string_pretty(&body).unwrap()
    )
}

#[tokio::test]
async fn the_committed_snapshot_matches_the_seed_book() {
    let want = generated().await;
    if std::env::var_os("UPDATE_PRICING_SNAPSHOT").is_some() {
        std::fs::write(SNAPSHOT, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(SNAPSHOT).unwrap_or_default();
    assert!(
        have.replace("\r\n", "\n") == want,
        "packages/ui/src/pricing.snapshot.ts differs from the seed price book; regenerate it with \
         UPDATE_PRICING_SNAPSHOT=1 cargo test -p arena-cloud --test pricing_snapshot"
    );
}
