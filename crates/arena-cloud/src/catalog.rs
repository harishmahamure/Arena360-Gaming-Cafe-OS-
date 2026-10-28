//! The global game catalogue (Story 6.1): the import file (JSON or CSV), the plan that turns it
//! into idempotent writes, the media upload by content hash and the manifest a Hub reads.
//!
//! A game is keyed by trimmed, case-insensitive name + launcher. Media are immutable objects
//! `catalog/<sha256>.<ext>` in the object store, uploaded once; the manifest gives each one's
//! CDN URL, hash and size.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use arena_proto::{CatalogGame, CatalogManifest, CatalogMedia, CatalogSignature};
use arena_store::control::{
    CatalogImport, CatalogMediaRow, CatalogRow, CatalogSignatureRow, ControlDb, GameWrite,
    NewAdminAudit,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::auth::hex;
use crate::templates::TemplateStore;

/// The most games one import file may hold.
pub const MAX_GAMES: usize = 5000;
/// The largest cover.
pub const MAX_COVER_BYTES: u64 = 5 * 1024 * 1024;
/// The largest video.
pub const MAX_VIDEO_BYTES: u64 = 100 * 1024 * 1024;

const COVER_EXTS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];
const VIDEO_EXTS: [&str; 2] = ["mp4", "webm"];
const TEXT_MAX: usize = 80;
const CATEGORY_MAX: usize = 40;
const AGE_MAX: usize = 20;
const EXE_MAX: usize = 120;

/// The object key of a media file.
pub fn media_key(sha256: &str, ext: &str) -> String {
    format!("catalog/{sha256}.{ext}")
}

/// One executable signature in an import file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureIn {
    pub exe: String,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub sha256: Option<String>,
}

fn yes() -> bool {
    true
}

/// One game in an import file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameIn {
    pub name: String,
    pub launcher: String,
    pub category: String,
    #[serde(default)]
    pub age_rating: String,
    #[serde(default = "yes")]
    pub published: bool,
    #[serde(default)]
    pub signatures: Vec<SignatureIn>,
    /// A path relative to the media directory.
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub video: Option<String>,
    #[serde(default)]
    pub flash_screened: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FileIn {
    Games(Vec<GameIn>),
    Wrapped { games: Vec<GameIn> },
}

/// Splits CSV text into records (quotes, doubled quotes and newlines inside quotes).
fn csv_records(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => record.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                record.push(std::mem::take(&mut field));
                if record.iter().any(|f| !f.trim().is_empty()) {
                    records.push(std::mem::take(&mut record));
                } else {
                    record.clear();
                }
            }
            (false, c) => field.push(c),
        }
    }
    if quoted {
        return Err("a quoted CSV field is never closed".to_owned());
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        if record.iter().any(|f| !f.trim().is_empty()) {
            records.push(record);
        }
    }
    Ok(records)
}

fn truthy(v: &str, what: &str) -> Result<Option<bool>, String> {
    match v.trim().to_ascii_lowercase().as_str() {
        "" => Ok(None),
        "1" | "true" | "yes" => Ok(Some(true)),
        "0" | "false" | "no" => Ok(Some(false)),
        other => Err(format!("{what}: {other:?} is not true or false")),
    }
}

/// CSV columns: `name,launcher,category,age_rating,published,exe,product,publisher,sha256,
/// cover,video,flash_screened`. One row per signature; rows of the same name + launcher are one
/// game and a blank cell takes the value of an earlier row.
fn parse_csv(text: &str) -> Result<Vec<GameIn>, String> {
    let records = csv_records(text)?;
    let mut rows = records.into_iter();
    let header: Vec<String> = rows
        .next()
        .ok_or("the CSV file is empty")?
        .into_iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    const KNOWN: [&str; 12] = [
        "name",
        "launcher",
        "category",
        "age_rating",
        "published",
        "exe",
        "product",
        "publisher",
        "sha256",
        "cover",
        "video",
        "flash_screened",
    ];
    if let Some(bad) = header.iter().find(|h| !KNOWN.contains(&h.as_str())) {
        return Err(format!("unknown CSV column {bad:?}"));
    }
    for need in ["name", "launcher", "category"] {
        if !header.iter().any(|h| h == need) {
            return Err(format!("the CSV file needs a {need} column"));
        }
    }
    let mut games: Vec<GameIn> = Vec::new();
    let mut at: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (n, row) in rows.enumerate() {
        let line = n + 2;
        if row.len() > header.len() {
            return Err(format!("line {line}: more cells than columns"));
        }
        let cell = |col: &str| -> String {
            header
                .iter()
                .position(|h| h == col)
                .and_then(|i| row.get(i))
                .map(|v| v.trim().to_owned())
                .unwrap_or_default()
        };
        let (name, launcher) = (cell("name"), cell("launcher"));
        let key = (name.to_lowercase(), launcher.to_lowercase());
        let published =
            truthy(&cell("published"), "published").map_err(|e| format!("line {line}: {e}"))?;
        let flash = truthy(&cell("flash_screened"), "flash_screened")
            .map_err(|e| format!("line {line}: {e}"))?;
        let some = |v: String| (!v.is_empty()).then_some(v);
        let sig = (!cell("exe").is_empty()).then(|| SignatureIn {
            exe: cell("exe"),
            product: cell("product"),
            publisher: cell("publisher"),
            sha256: some(cell("sha256")),
        });
        if let Some(&i) = at.get(&key) {
            let g = &mut games[i];
            if let Some(s) = sig {
                g.signatures.push(s);
            }
            for (slot, v) in [(&mut g.cover, cell("cover")), (&mut g.video, cell("video"))] {
                if slot.is_none() {
                    *slot = some(v);
                }
            }
            if g.age_rating.is_empty() {
                g.age_rating = cell("age_rating");
            }
            g.flash_screened |= flash.unwrap_or(false);
            continue;
        }
        at.insert(key, games.len());
        games.push(GameIn {
            name,
            launcher,
            category: cell("category"),
            age_rating: cell("age_rating"),
            published: published.unwrap_or(true),
            signatures: sig.into_iter().collect(),
            cover: some(cell("cover")),
            video: some(cell("video")),
            flash_screened: flash.unwrap_or(false),
        });
    }
    Ok(games)
}

/// Parses an import file by its extension (`.json` or `.csv`) and checks every game.
pub fn parse_import(file_name: &str, text: &str) -> Result<Vec<GameIn>, String> {
    let lower = file_name.to_ascii_lowercase();
    let games = if lower.ends_with(".json") {
        match serde_json::from_str::<FileIn>(text).map_err(|e| e.to_string())? {
            FileIn::Games(g) | FileIn::Wrapped { games: g } => g,
        }
    } else if lower.ends_with(".csv") {
        parse_csv(text)?
    } else {
        return Err("the import file must be .json or .csv".to_owned());
    };
    if games.len() > MAX_GAMES {
        return Err(format!("an import holds at most {MAX_GAMES} games"));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(games.len());
    for (i, g) in games.into_iter().enumerate() {
        let g = clean(g).map_err(|e| format!("game {}: {e}", i + 1))?;
        if !seen.insert((g.name.to_lowercase(), g.launcher.to_lowercase())) {
            return Err(format!(
                "game {}: {} on {} is listed twice",
                i + 1,
                g.name,
                g.launcher
            ));
        }
        out.push(g);
    }
    Ok(out)
}

fn plain(label: &str, v: &str, max: usize, required: bool) -> Result<String, String> {
    let v = v.trim();
    if (required && v.is_empty()) || v.chars().count() > max || v.chars().any(char::is_control) {
        return Err(format!(
            "{label} must be {}{max} characters of plain text",
            if required { "1 to " } else { "up to " }
        ));
    }
    Ok(v.to_owned())
}

fn clean(mut g: GameIn) -> Result<GameIn, String> {
    g.name = plain("name", &g.name, TEXT_MAX, true)?;
    g.launcher = plain("launcher", &g.launcher, TEXT_MAX, true)?;
    g.category = plain("category", &g.category, CATEGORY_MAX, true)?;
    g.age_rating = plain("age_rating", &g.age_rating, AGE_MAX, false)?;
    for s in &mut g.signatures {
        s.exe = plain("exe", &s.exe, EXE_MAX, true)?;
        if s.exe.contains(['/', '\\']) {
            return Err("exe is a file name, without a folder".to_owned());
        }
        s.product = plain("product", &s.product, TEXT_MAX, false)?;
        s.publisher = plain("publisher", &s.publisher, TEXT_MAX, false)?;
        s.sha256 = match s.sha256.take().map(|h| h.trim().to_ascii_lowercase()) {
            Some(h) if h.is_empty() => None,
            Some(h) if h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()) => Some(h),
            Some(_) => return Err("sha256 must be 64 hex characters".to_owned()),
            None => None,
        };
    }
    for m in [&mut g.cover, &mut g.video] {
        *m = m
            .take()
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty());
    }
    Ok(g)
}

/// A media file found on disk and described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFile {
    pub row: CatalogMediaRow,
    pub path: PathBuf,
}

/// Reads `rel` under `base` for its hash and size. `kind` is `cover` or `video`.
pub fn describe_media(
    base: &Path,
    rel: &str,
    kind: &str,
    flash_screened: bool,
) -> Result<MediaFile, String> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!(
            "{kind} {rel:?} must be a path under the media folder"
        ));
    }
    let ext = rel_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let (allowed, max): (&[&str], u64) = if kind == "cover" {
        (&COVER_EXTS, MAX_COVER_BYTES)
    } else {
        (&VIDEO_EXTS, MAX_VIDEO_BYTES)
    };
    if !allowed.contains(&ext.as_str()) {
        return Err(format!("{kind} {rel:?} must be {}", allowed.join(", ")));
    }
    let path = base.join(rel_path);
    let body = std::fs::read(&path).map_err(|e| format!("{kind} {}: {e}", path.display()))?;
    let bytes = body.len() as u64;
    if bytes == 0 || bytes > max {
        return Err(format!(
            "{kind} {rel:?} must be 1 byte to {} MiB",
            max / (1024 * 1024)
        ));
    }
    Ok(MediaFile {
        row: CatalogMediaRow {
            kind: kind.to_owned(),
            sha256: hex(&Sha256::digest(&body)),
            ext,
            bytes: bytes as i64,
            flash_screened: kind == "video" && flash_screened,
        },
        path,
    })
}

/// A game ready to plan: its fields and its media, described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGame {
    pub game: GameIn,
    pub media: Vec<MediaFile>,
}

/// Reads the media each game names, under `base`.
pub fn resolve(games: Vec<GameIn>, base: &Path) -> Result<Vec<ResolvedGame>, String> {
    games
        .into_iter()
        .map(|game| {
            let mut media = Vec::new();
            if let Some(c) = &game.cover {
                media.push(describe_media(base, c, "cover", false)?);
            }
            if let Some(v) = &game.video {
                media.push(describe_media(base, v, "video", game.flash_screened)?);
            }
            Ok(ResolvedGame { game, media })
        })
        .collect()
}

/// The signatures of a game, in the order the store returns them.
fn sig_rows(sigs: &[SignatureIn]) -> Vec<CatalogSignatureRow> {
    let mut rows: Vec<CatalogSignatureRow> = sigs
        .iter()
        .map(|s| CatalogSignatureRow {
            exe_name: s.exe.clone(),
            product_name: s.product.clone(),
            publisher: s.publisher.clone(),
            sha256: s.sha256.clone().unwrap_or_default(),
        })
        .collect();
    rows.sort_by_key(|s| (s.exe_name.to_lowercase(), s.sha256.clone()));
    rows.dedup_by_key(|s| (s.exe_name.to_lowercase(), s.sha256.clone()));
    rows
}

/// What an import will do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub writes: Vec<GameWrite>,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
}

/// Compares `games` with the stored `existing` rows and keeps the ones that change. `new_id`
/// mints the id of a new game.
pub fn plan(
    existing: &[CatalogRow],
    games: &[ResolvedGame],
    mut new_id: impl FnMut() -> String,
) -> Plan {
    let by_key: BTreeMap<(String, String), &CatalogRow> = existing
        .iter()
        .map(|r| ((r.name.to_lowercase(), r.launcher.to_lowercase()), r))
        .collect();
    let mut plan = Plan::default();
    for rg in games {
        let g = &rg.game;
        let sigs = sig_rows(&g.signatures);
        let media: Vec<CatalogMediaRow> = rg.media.iter().map(|m| m.row.clone()).collect();
        let key = (g.name.to_lowercase(), g.launcher.to_lowercase());
        let write = |id: String, is_new: bool, bump: bool| GameWrite {
            id,
            is_new,
            name: g.name.clone(),
            launcher: g.launcher.clone(),
            category: g.category.clone(),
            age_rating: g.age_rating.clone(),
            published: g.published,
            bump,
            signatures: Some(sigs.clone()),
            media: media.clone(),
        };
        let Some(old) = by_key.get(&key) else {
            plan.created += 1;
            plan.writes.push(write(new_id(), true, g.published));
            continue;
        };
        let same = old.name == g.name
            && old.launcher == g.launcher
            && old.category == g.category
            && old.age_rating == g.age_rating
            && old.published == g.published
            && !old.deleted
            && old.signatures == sigs
            && media.iter().all(|m| old.media.iter().any(|o| o == m));
        if same {
            plan.unchanged += 1;
        } else {
            plan.updated += 1;
            plan.writes
                .push(write(old.id.clone(), false, old.is_live() || g.published));
        }
    }
    plan
}

/// The counts an import reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub file: String,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub media_uploaded: usize,
    pub media_present: usize,
    pub version: i64,
    pub audit_id: String,
}

/// Why an import stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// The file or a media file is unusable; nothing was written.
    File(String),
    /// The object store or `gz_control` failed.
    Backend(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(m) => write!(f, "IMPORT_FILE_INVALID: {m}"),
            Self::Backend(m) => write!(f, "CATALOG_IMPORT_FAILED: {m}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Runs an import of `games` (already parsed from `file_name`): uploads each changed game's
/// media once by hash, then writes the games, the version bump and one `admin_audit` row in one
/// transaction.
pub async fn import(
    db: &ControlDb,
    store: Option<&TemplateStore>,
    file_name: &str,
    games: &[ResolvedGame],
    operator: &str,
    now: i64,
) -> Result<ImportSummary, ImportError> {
    let backend = |e: &dyn std::fmt::Display| ImportError::Backend(e.to_string());
    let existing = db.catalog_all().await.map_err(|e| backend(&e))?;
    let plan = plan(&existing, games, || ulid::Ulid::generate().to_string());
    // Only the games that change need their files in the store; an unchanged game's are there.
    let changed: BTreeSet<(String, String)> = plan
        .writes
        .iter()
        .map(|w| (w.name.to_lowercase(), w.launcher.to_lowercase()))
        .collect();
    let mut wanted: BTreeMap<String, &MediaFile> = BTreeMap::new();
    for rg in games {
        if changed.contains(&(rg.game.name.to_lowercase(), rg.game.launcher.to_lowercase())) {
            for m in &rg.media {
                wanted
                    .entry(media_key(&m.row.sha256, &m.row.ext))
                    .or_insert(m);
            }
        }
    }
    let (mut uploaded, mut present) = (0, 0);
    if !wanted.is_empty() {
        let store = store.ok_or_else(|| {
            ImportError::Backend(
                "no object store is configured (ARENA_BACKUP_S3_*) for the media".to_owned(),
            )
        })?;
        for (key, file) in wanted {
            if store
                .list(&key)
                .await
                .map_err(|e| backend(&e))?
                .contains(&key)
            {
                present += 1;
                continue;
            }
            let body = std::fs::read(&file.path)
                .map_err(|e| ImportError::File(format!("{}: {e}", file.path.display())))?;
            if hex(&Sha256::digest(&body)) != file.row.sha256 {
                return Err(ImportError::File(format!(
                    "{} changed while the import ran",
                    file.path.display()
                )));
            }
            store.put(&key, body).await.map_err(|e| backend(&e))?;
            uploaded += 1;
        }
    }
    let audit_id = ulid::Ulid::generate().to_string();
    let request_id = ulid::Ulid::generate().to_string();
    let summary = serde_json::json!({
        "file": file_name,
        "games": games.len(),
        "created": plan.created,
        "updated": plan.updated,
        "unchanged": plan.unchanged,
        "media_uploaded": uploaded,
    })
    .to_string();
    let outcome = db
        .apply_catalog_import(&CatalogImport {
            games: plan.writes,
            now,
            audit: NewAdminAudit {
                id: &audit_id,
                at: now,
                actor_kind: "admin",
                actor_id: operator,
                action: "catalog.import",
                target_kind: "catalog",
                target_id: "global",
                reason: None,
                summary: &summary,
                request_id: &request_id,
            },
        })
        .await
        .map_err(|e| backend(&e))?;
    Ok(ImportSummary {
        file: file_name.to_owned(),
        created: plan.created,
        updated: plan.updated,
        unchanged: plan.unchanged,
        media_uploaded: uploaded,
        media_present: present,
        version: outcome.version,
        audit_id,
    })
}

impl ImportSummary {
    pub fn lines(&self) -> Vec<String> {
        vec![
            format!(
                "imported {}: {} created, {} updated, {} unchanged",
                self.file, self.created, self.updated, self.unchanged
            ),
            format!(
                "media: {} uploaded, {} already in the store",
                self.media_uploaded, self.media_present
            ),
            format!(
                "catalog_version {}; admin_audit {}",
                self.version, self.audit_id
            ),
        ]
    }
}

/// The manifest for a Hub at version `since`: games changed after it, up to `version`. A game
/// that is not live is a tombstone, which a Hub with nothing yet (`since` 0) is not sent. A
/// video that is not flash-screened is never listed.
pub fn manifest(
    rows: Vec<CatalogRow>,
    version: i64,
    since: i64,
    cdn_base: &str,
) -> CatalogManifest {
    let base = cdn_base.trim_end_matches('/');
    let games = rows
        .into_iter()
        .filter_map(|r| {
            if !r.is_live() {
                return (since > 0).then(|| CatalogGame {
                    id: r.id,
                    rev: r.rev.max(0) as u64,
                    deleted: true,
                    name: None,
                    launcher: None,
                    category: None,
                    age_rating: None,
                    signatures: Vec::new(),
                    media: Vec::new(),
                });
            }
            Some(CatalogGame {
                id: r.id,
                rev: r.rev.max(0) as u64,
                deleted: false,
                name: Some(r.name),
                launcher: Some(r.launcher),
                category: Some(r.category),
                age_rating: Some(r.age_rating).filter(|a| !a.is_empty()),
                signatures: r
                    .signatures
                    .into_iter()
                    .map(|s| CatalogSignature {
                        exe_name: s.exe_name,
                        product_name: s.product_name,
                        publisher: s.publisher,
                        sha256: Some(s.sha256).filter(|h| !h.is_empty()),
                    })
                    .collect(),
                media: r
                    .media
                    .into_iter()
                    .filter(|m| m.kind == "cover" || m.flash_screened)
                    .map(|m| CatalogMedia {
                        url: format!("{base}/{}", media_key(&m.sha256, &m.ext)),
                        kind: m.kind,
                        sha256: m.sha256,
                        bytes: m.bytes.max(0) as u64,
                    })
                    .collect(),
            })
        })
        .collect();
    CatalogManifest {
        catalog_version: version.max(0) as u64,
        games,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_rows_of_one_game_are_one_game_with_each_signature() {
        let csv = "name,launcher,category,age_rating,published,exe,product,publisher,sha256,cover\n\
                   \"Dota 2\",Steam,MOBA,PEGI 12,,dota2.exe,Dota 2,Valve,,dota.jpg\n\
                   dota 2,steam,,,,dota2_launcher.exe,,Valve,,\n\
                   Valorant,Riot,Shooter,16+,false,,,,,\n";
        let games = parse_import("list.csv", csv).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].signatures.len(), 2);
        assert_eq!(games[0].cover.as_deref(), Some("dota.jpg"));
        assert!(games[0].published);
        assert!(!games[1].published);
    }

    #[test]
    fn json_files_may_be_a_list_or_wrapped_and_unknown_fields_are_refused() {
        let one = r#"{"name":"A","launcher":"Steam","category":"Racing"}"#;
        assert_eq!(
            parse_import("a.json", &format!("[{one}]")).unwrap().len(),
            1
        );
        assert_eq!(
            parse_import("a.json", &format!("{{\"games\":[{one}]}}"))
                .unwrap()
                .len(),
            1
        );
        assert!(
            parse_import(
                "a.json",
                r#"[{"name":"A","launcher":"S","category":"C","x":1}]"#
            )
            .is_err()
        );
        assert!(parse_import("a.txt", "[]").is_err());
        let twice = format!("[{one},{}]", one.replace("\"A\"", "\"a\""));
        assert!(
            parse_import("a.json", &twice)
                .unwrap_err()
                .contains("twice")
        );
        let bad_hash = r#"[{"name":"A","launcher":"S","category":"C","signatures":[{"exe":"a.exe","sha256":"xyz"}]}]"#;
        assert!(
            parse_import("a.json", bad_hash)
                .unwrap_err()
                .contains("sha256")
        );
        let folder =
            r#"[{"name":"A","launcher":"S","category":"C","signatures":[{"exe":"bin/a.exe"}]}]"#;
        assert!(parse_import("a.json", folder).is_err());
    }

    #[test]
    fn media_paths_stay_under_the_media_folder() {
        let dir = std::path::PathBuf::from("/nonexistent");
        for bad in ["../x.jpg", "/etc/x.jpg", "a/../x.jpg"] {
            assert!(
                describe_media(&dir, bad, "cover", false)
                    .unwrap_err()
                    .contains("under the media folder"),
                "{bad}"
            );
        }
        assert!(
            describe_media(&dir, "x.gif", "cover", false)
                .unwrap_err()
                .contains("must be jpg")
        );
    }
}
