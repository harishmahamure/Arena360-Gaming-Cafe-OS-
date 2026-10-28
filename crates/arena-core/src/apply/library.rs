//! `library.upsert` and `library.delete` (Story 6.3, both need `games.manage`): the branch game
//! and app library, one `library_entries` row per entry.
//!
//! - **base_rev** is required on an edit and must match the entry's `rev`, else `STALE`; an owner
//!   actor may omit it or send an old one (as `zone.upsert`).
//! - An entry linked to the global catalogue (`catalog_id`) keeps its `name`, `launcher`,
//!   `age_rating` and `catalog_id`: they come from the catalogue, which only the API layer can
//!   read, so `apply()` only refuses to change them. A duplicate is the same kind, launcher and
//!   executable path (without case) among live entries.
//! - Every write rewrites two derived settings in the same transaction. `kiosk.library` is the
//!   enabled live entries ordered by name. `kiosk.process_allow` is
//!   `(current − (derived(before) − derived(after))) ∪ derived(after)`, where `derived(set)` is
//!   the de-duplicated union over the enabled live entries of the entry's executable path and
//!   its launcher's install roots as unsigned allow rules: rules an operator wrote are never
//!   removed, and a rule another enabled entry still needs stays. A library rule the allow-list
//!   refuses (an unsigned path under a user-writable root) refuses the entry. A setting is
//!   rewritten, and a change row emitted, only when its value changes.

use std::collections::HashSet;

use arena_proto::{
    ActorKind, AppError, ErrorCode, Intent, IntentPayload, KEY_KIOSK_LIBRARY, KEY_PROCESS_ALLOW,
    KioskLibraryEntry, LIBRARY_AGE_RATING_MAX, LIBRARY_ARGS_MAX, LIBRARY_CATALOG_ID_MAX,
    LIBRARY_CATEGORY_MAX, LIBRARY_KINDS, LIBRARY_LAUNCHERS, LIBRARY_NAME_MAX, LIBRARY_PATH_MAX,
    LIBRARY_TAG_MAX, LIBRARY_TAGS_MAX, LIBRARY_URI_MAX, LibraryDelete, LibraryUpsert,
    MAX_LIBRARY_ENTRIES, MAX_LIBRARY_VALUE_CHARS, ProcessList, ProcessRule, is_launch_uri,
    is_library_path, library_rules, parse_process_rules,
};
use arena_store::{
    ChangeOp, LibraryRow, LibraryWrite, RowKey, SettingRef, StoreTx, current_branch_id,
    find_library_entry, find_setting, insert_library_entry, insert_setting, list_library_entries,
    set_library_entry_deleted, update_library_entry, update_setting,
};

use super::mutate::Written;

#[derive(Default)]
pub(crate) struct LibraryTarget {
    /// The live entry the intent edits or deletes.
    entry: Option<LibraryRow>,
    /// Every live entry before the write.
    live: Vec<LibraryRow>,
    branch_id: Option<String>,
    library: Option<SettingRef>,
    allow: Option<SettingRef>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<LibraryTarget, AppError> {
    let id = match p {
        IntentPayload::LibraryUpsert(u) => u.id.as_deref(),
        IntentPayload::LibraryDelete(d) => Some(d.id.as_str()),
        _ => None,
    };
    let entry = match id {
        Some(id) => find_library_entry(tx, id)?.filter(|e| e.deleted_at.is_none()),
        None => None,
    };
    Ok(LibraryTarget {
        entry,
        live: list_library_entries(tx)?,
        branch_id: current_branch_id(tx)?,
        library: find_setting(tx, KEY_KIOSK_LIBRARY)?,
        allow: find_setting(tx, KEY_PROCESS_ALLOW)?,
    })
}

pub(super) fn base_rev(intent: &Intent, t: &LibraryTarget) -> Result<(), AppError> {
    match (t.entry.as_ref().map(|e| e.rev), intent.base_rev) {
        (Some(_), None) if intent.actor.kind != ActorKind::Owner => {
            Err(invalid("base_rev", "editing needs base_rev"))
        }
        (Some(rev), Some(base)) if rev != base && intent.actor.kind != ActorKind::Owner => {
            Err(AppError::with_field(
                ErrorCode::Stale,
                "base_rev",
                "this changed since it was read",
            ))
        }
        _ => Ok(()),
    }
}

/// The payload with its text trimmed, the tags de-duplicated and an empty catalogue id dropped:
/// what is validated and stored.
struct Norm {
    kind: String,
    name: String,
    launcher: String,
    executable_path: String,
    launch_uri: String,
    launch_args: String,
    category: String,
    tags: Vec<String>,
    age_rating: String,
    enabled: bool,
    catalog_id: Option<String>,
}

fn normalise(u: &LibraryUpsert) -> Norm {
    let mut tags: Vec<String> = Vec::new();
    for tag in &u.tags {
        let tag = tag.trim();
        if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) || tag.is_empty() {
            tags.push(tag.to_owned());
        }
    }
    Norm {
        kind: u.entry_kind.trim().to_owned(),
        name: u.name.trim().to_owned(),
        launcher: u.launcher.trim().to_owned(),
        executable_path: u.executable_path.trim().to_owned(),
        launch_uri: u.launch_uri.trim().to_owned(),
        launch_args: u.launch_args.trim().to_owned(),
        category: u.category.trim().to_owned(),
        tags,
        age_rating: u.age_rating.trim().to_owned(),
        enabled: u.enabled,
        catalog_id: u
            .catalog_id
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_owned),
    }
}

fn plain(value: &str, max: usize) -> bool {
    value.chars().count() <= max && !value.chars().any(char::is_control)
}

fn check_fields(n: &Norm) -> Result<(), AppError> {
    if !LIBRARY_KINDS.contains(&n.kind.as_str()) {
        return Err(invalid("entry_kind", "kind must be game or app"));
    }
    if n.name.is_empty() || !plain(&n.name, LIBRARY_NAME_MAX) {
        return Err(invalid(
            "name",
            "name must be 1–80 characters with no control characters",
        ));
    }
    if !LIBRARY_LAUNCHERS.contains(&n.launcher.as_str()) {
        return Err(invalid(
            "launcher",
            "launcher must be steam, epic, riot, battlenet or none",
        ));
    }
    if n.executable_path.is_empty() {
        if n.launch_uri.is_empty() {
            return Err(invalid(
                "executable_path",
                "an entry needs an executable path or a launch URI",
            ));
        }
    } else if n.executable_path.chars().count() > LIBRARY_PATH_MAX
        || !is_library_path(&n.executable_path)
    {
        return Err(invalid(
            "executable_path",
            "the executable path must be a full Windows path (C:\\…) with no * ? { or }",
        ));
    }
    if !n.launch_uri.is_empty()
        && (n.launch_uri.chars().count() > LIBRARY_URI_MAX || !is_launch_uri(&n.launch_uri))
    {
        return Err(invalid(
            "launch_uri",
            "the launch URI must start with a scheme such as steam:// and be at most 512 characters",
        ));
    }
    if !plain(&n.launch_args, LIBRARY_ARGS_MAX) {
        return Err(invalid(
            "launch_args",
            "launch arguments must be at most 512 characters with no control characters",
        ));
    }
    if !plain(&n.category, LIBRARY_CATEGORY_MAX) {
        return Err(invalid(
            "category",
            "category must be at most 40 characters with no control characters",
        ));
    }
    if n.tags.len() > LIBRARY_TAGS_MAX
        || n.tags
            .iter()
            .any(|t| t.is_empty() || !plain(t, LIBRARY_TAG_MAX))
    {
        return Err(invalid(
            "tags",
            "at most 10 tags of 1–32 characters with no control characters",
        ));
    }
    if !plain(&n.age_rating, LIBRARY_AGE_RATING_MAX) {
        return Err(invalid(
            "age_rating",
            "age rating must be at most 16 characters with no control characters",
        ));
    }
    if n.catalog_id
        .as_deref()
        .is_some_and(|c| !plain(c, LIBRARY_CATALOG_ID_MAX))
    {
        return Err(invalid("catalog_id", "the catalogue id is not valid"));
    }
    Ok(())
}

/// A row for an entry as it would be after the write (`id` placeholder while validating).
fn row_of(id: &str, n: &Norm, rev: i64) -> LibraryRow {
    LibraryRow {
        id: id.to_owned(),
        kind: n.kind.clone(),
        name: n.name.clone(),
        launcher: n.launcher.clone(),
        executable_path: n.executable_path.clone(),
        launch_uri: n.launch_uri.clone(),
        launch_args: n.launch_args.clone(),
        category: n.category.clone(),
        tags: n.tags.clone(),
        age_rating: n.age_rating.clone(),
        enabled: n.enabled,
        catalog_id: n.catalog_id.clone(),
        rev,
        deleted_at: None,
    }
}

type RuleKey = (String, Option<String>);

fn rule_key(r: &ProcessRule) -> RuleKey {
    (r.path_glob.to_ascii_lowercase(), r.signer.clone())
}

/// `derived(set)`: the de-duplicated union of the rules of the enabled entries.
fn derived(entries: &[LibraryRow]) -> Vec<ProcessRule> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for e in entries.iter().filter(|e| e.enabled) {
        for rule in library_rules(&e.executable_path, &e.launcher) {
            if seen.insert(rule_key(&rule)) {
                out.push(rule);
            }
        }
    }
    out
}

/// The new values of the two derived settings (`None`: unchanged).
struct Plan {
    allow: Option<String>,
    library: Option<String>,
}

fn kiosk_library_json(entries: &[LibraryRow]) -> Result<String, AppError> {
    let mut list: Vec<&LibraryRow> = entries.iter().filter(|e| e.enabled).collect();
    list.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    let list: Vec<KioskLibraryEntry> = list
        .into_iter()
        .map(|e| KioskLibraryEntry {
            id: e.id.clone(),
            kind: e.kind.clone(),
            name: e.name.clone(),
            launcher: e.launcher.clone(),
            executable_path: e.executable_path.clone(),
            launch_uri: e.launch_uri.clone(),
            launch_args: e.launch_args.clone(),
            category: e.category.clone(),
            tags: e.tags.clone(),
            age_rating: e.age_rating.clone(),
            catalog_id: e.catalog_id.clone(),
        })
        .collect();
    serde_json::to_string(&list)
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("kiosk.library: {e}")))
}

/// Works out the derived settings for the live entries `after` the write.
fn plan(t: &LibraryTarget, after: &[LibraryRow]) -> Result<Plan, AppError> {
    let library = kiosk_library_json(after)?;
    let current_library = t.library.as_ref().map_or("[]", |s| s.value.as_str());
    let allow = match &t.allow {
        None => None,
        Some(row) => {
            let current: Vec<ProcessRule> = serde_json::from_str(&row.value).map_err(|e| {
                AppError::new(
                    ErrorCode::Internal,
                    format!("kiosk.process_allow is not a rule list: {e}"),
                )
            })?;
            let before = derived(&t.live);
            let after_rules = derived(after);
            let keep: HashSet<RuleKey> = after_rules.iter().map(rule_key).collect();
            let removed: HashSet<RuleKey> = before
                .iter()
                .map(rule_key)
                .filter(|k| !keep.contains(k))
                .collect();
            let mut next: Vec<ProcessRule> = current
                .iter()
                .filter(|r| !removed.contains(&rule_key(r)))
                .cloned()
                .collect();
            let have: HashSet<RuleKey> = next.iter().map(rule_key).collect();
            next.extend(
                after_rules
                    .into_iter()
                    .filter(|r| !have.contains(&rule_key(r))),
            );
            if next == current {
                None
            } else {
                Some(serde_json::to_string(&next).map_err(|e| {
                    AppError::new(ErrorCode::Internal, format!("kiosk.process_allow: {e}"))
                })?)
            }
        }
    };
    Ok(Plan {
        allow,
        library: (library != current_library).then_some(library),
    })
}

fn refused_rules(e: AppError) -> AppError {
    invalid("executable_path", &e.message)
}

pub(super) fn validate_upsert(u: &LibraryUpsert, t: &LibraryTarget) -> Result<(), AppError> {
    let n = normalise(u);
    check_fields(&n)?;
    if u.id.is_some() && t.entry.is_none() {
        return Err(invalid("id", "no such library entry"));
    }
    if let Some(old) = t.entry.as_ref().filter(|e| e.catalog_id.is_some()) {
        for (field, same) in [
            ("name", old.name == n.name),
            ("launcher", old.launcher == n.launcher),
            ("age_rating", old.age_rating == n.age_rating),
            ("catalog_id", old.catalog_id == n.catalog_id),
        ] {
            if !same {
                return Err(invalid(
                    field,
                    "this entry comes from the game catalogue; its name, launcher and age rating are read-only",
                ));
            }
        }
    }
    let own_id = t.entry.as_ref().map(|e| e.id.as_str());
    if !n.executable_path.is_empty()
        && t.live.iter().any(|e| {
            Some(e.id.as_str()) != own_id
                && e.kind == n.kind
                && e.launcher == n.launcher
                && e.executable_path.eq_ignore_ascii_case(&n.executable_path)
        })
    {
        return Err(invalid(
            "executable_path",
            "this game or app is already in the library",
        ));
    }
    if t.entry.is_none() && t.live.len() >= MAX_LIBRARY_ENTRIES {
        return Err(invalid("name", "the library is full"));
    }
    // The entry's own rules must pass the allow-list's checks whether or not it is enabled, so
    // enabling it later cannot fail.
    let own = serde_json::to_string(&library_rules(&n.executable_path, &n.launcher))
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
    parse_process_rules(ProcessList::Allow, &own).map_err(refused_rules)?;
    // An edit replaces the entry's current row; a new entry has no row to replace.
    let after = after_upsert(t, &n, own_id.unwrap_or("00000000000000000000000000"));
    let plan = plan(t, &after)?;
    if let Some(allow) = &plan.allow {
        parse_process_rules(ProcessList::Allow, allow).map_err(refused_rules)?;
    }
    if plan.library.as_ref().map_or(0, |v| v.chars().count()) > MAX_LIBRARY_VALUE_CHARS {
        return Err(invalid("name", "the library is too large"));
    }
    Ok(())
}

pub(super) fn validate_delete(t: &LibraryTarget) -> Result<(), AppError> {
    if t.entry.is_none() {
        return Err(invalid("id", "no such library entry"));
    }
    Ok(())
}

fn after_upsert(t: &LibraryTarget, n: &Norm, id: &str) -> Vec<LibraryRow> {
    let mut after: Vec<LibraryRow> = t.live.iter().filter(|e| e.id != id).cloned().collect();
    after.push(row_of(id, n, 0));
    after
}

fn written(key: RowKey, op: ChangeOp) -> Written {
    Written {
        key,
        op,
        before: None,
    }
}

/// Writes the settings of `plan` that changed.
fn write_derived(
    tx: &StoreTx<'_>,
    t: &LibraryTarget,
    plan: Plan,
    out: &mut Vec<Written>,
) -> Result<(), AppError> {
    for (key, row, value) in [
        (KEY_KIOSK_LIBRARY, &t.library, plan.library),
        (KEY_PROCESS_ALLOW, &t.allow, plan.allow),
    ] {
        let Some(value) = value else { continue };
        match (row, &t.branch_id) {
            (Some(row), _) => {
                update_setting(tx, &row.branch_id, key, &value, None, None)?;
                out.push(written(
                    RowKey::Settings {
                        branch_id: row.branch_id.clone(),
                        key: key.to_owned(),
                    },
                    ChangeOp::Update,
                ));
            }
            // A branch bootstrapped before the library existed gets its row here.
            (None, Some(branch_id)) => {
                insert_setting(tx, branch_id, key, &value)?;
                out.push(written(
                    RowKey::Settings {
                        branch_id: branch_id.clone(),
                        key: key.to_owned(),
                    },
                    ChangeOp::Insert,
                ));
            }
            (None, None) => {}
        }
    }
    Ok(())
}

pub(super) fn mutate_upsert(
    tx: &StoreTx<'_>,
    u: &LibraryUpsert,
    t: &LibraryTarget,
) -> Result<Vec<Written>, AppError> {
    let n = normalise(u);
    let fields = LibraryWrite {
        kind: &n.kind,
        name: &n.name,
        launcher: &n.launcher,
        executable_path: &n.executable_path,
        launch_uri: &n.launch_uri,
        launch_args: &n.launch_args,
        category: &n.category,
        tags: &n.tags,
        age_rating: &n.age_rating,
        enabled: n.enabled,
        catalog_id: n.catalog_id.as_deref(),
    };
    let (id, op) = match &t.entry {
        Some(old) => {
            update_library_entry(tx, &old.id, &fields)?;
            (old.id.clone(), ChangeOp::Update)
        }
        None => {
            let id = ulid::Ulid::generate().to_string();
            insert_library_entry(tx, &id, &fields)?;
            (id, ChangeOp::Insert)
        }
    };
    let after = after_upsert(t, &n, &id);
    let mut out = vec![written(RowKey::LibraryEntries { id }, op)];
    write_derived(tx, t, plan(t, &after)?, &mut out)?;
    Ok(out)
}

pub(super) fn mutate_delete(
    tx: &StoreTx<'_>,
    d: &LibraryDelete,
    t: &LibraryTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    set_library_entry_deleted(tx, &d.id, now_ms)?;
    let after: Vec<LibraryRow> = t.live.iter().filter(|e| e.id != d.id).cloned().collect();
    let mut out = vec![written(
        RowKey::LibraryEntries { id: d.id.clone() },
        ChangeOp::Update,
    )];
    write_derived(tx, t, plan(t, &after)?, &mut out)?;
    Ok(out)
}
