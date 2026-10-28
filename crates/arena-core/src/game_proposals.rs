//! Matching scanned executables to the catalogue and proposing them for the library (Story 6.5).
//!
//! [`match_scan`] matches one device's scan against the cached catalogue signatures (the API
//! layer reads `reference.db` and records the result with `game.matched`). [`build_proposals`]
//! turns the stored scans, matches, library and dismissals into the "Found on PCs" list.

use std::collections::{HashMap, HashSet};

use arena_proto::{
    GameMatch, LibraryEntryView, ProposalDevice, ProposalView, ProposalsView, ScanEntry,
    launcher_from_catalogue, proposal_key,
};
use arena_store::RefSignature;

/// Matches each executable of a scan against the catalogue signatures. An executable matches a
/// signature when its file name is the signature's `exe_name` and every product name or
/// publisher the signature gives is the executable's (all without case). The signature with the
/// most such fields wins, then the lowest game id; an executable matches at most one game and a
/// path is listed once. (The scan carries no file hash, so a signature's `sha256` is not used.)
pub fn match_scan(entries: &[ScanEntry], signatures: &[RefSignature]) -> Vec<GameMatch> {
    let mut by_exe: HashMap<String, Vec<&RefSignature>> = HashMap::new();
    for s in signatures {
        by_exe
            .entry(s.exe_name.trim().to_lowercase())
            .or_default()
            .push(s);
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for e in entries {
        let Some(candidates) = by_exe.get(&e.file_name.trim().to_lowercase()) else {
            continue;
        };
        let product = e.product_name.trim().to_lowercase();
        let publisher = e.publisher.trim().to_lowercase();
        let best = candidates
            .iter()
            .filter_map(|s| {
                let (sp, sb) = (
                    s.product_name.trim().to_lowercase(),
                    s.publisher.trim().to_lowercase(),
                );
                if (!sp.is_empty() && sp != product) || (!sb.is_empty() && sb != publisher) {
                    return None;
                }
                Some((
                    usize::from(!sp.is_empty()) + usize::from(!sb.is_empty()),
                    *s,
                ))
            })
            .max_by(|(a, sa), (b, sb)| a.cmp(b).then_with(|| sb.game_id.cmp(&sa.game_id)));
        if let Some((_, sig)) = best
            && seen.insert(e.path.to_ascii_lowercase())
        {
            out.push(GameMatch {
                catalog_id: sig.game_id.clone(),
                path: e.path.clone(),
            });
        }
    }
    out
}

/// A paired PC with the scan it last reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedDevice {
    pub device_id: String,
    pub label: String,
    pub entries: Vec<ScanEntry>,
    pub matches: Vec<GameMatch>,
}

/// What the proposals are built from, read in one consistent snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProposalInputs {
    pub devices: Vec<ScannedDevice>,
    pub library: Vec<LibraryEntryView>,
    pub dismissed: Vec<String>,
}

/// What the catalogue says about a matched game.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatalogueGame {
    pub name: String,
    pub launcher: Option<String>,
    pub category: Option<String>,
    pub age_rating: Option<String>,
    pub cover_sha256: Option<String>,
}

/// The "Found on PCs" list: every executable found on a PC that is not covered by the library
/// and not dismissed, grouped by [`proposal_key`]. A found item is covered when a library entry
/// is linked to its catalogue game, an install of an entry has its key, or an entry's
/// executable or install path is its path (without case). Ordered by name.
pub fn build_proposals(
    inputs: &ProposalInputs,
    catalogue: &dyn Fn(&str) -> Option<CatalogueGame>,
) -> ProposalsView {
    let dismissed: HashSet<&str> = inputs.dismissed.iter().map(String::as_str).collect();
    let linked: HashSet<&str> = inputs
        .library
        .iter()
        .filter_map(|e| e.catalog_id.as_deref())
        .collect();
    let mut keys: HashSet<&str> = HashSet::new();
    let mut paths: HashSet<String> = HashSet::new();
    for e in &inputs.library {
        if !e.executable_path.is_empty() {
            paths.insert(e.executable_path.to_ascii_lowercase());
        }
        for i in &e.installs {
            paths.insert(i.path.to_ascii_lowercase());
            if !i.match_key.is_empty() {
                keys.insert(i.match_key.as_str());
            }
        }
    }
    let mut groups: Vec<(String, ProposalView)> = Vec::new();
    for d in &inputs.devices {
        let matched: HashMap<String, &str> = d
            .matches
            .iter()
            .map(|m| (m.path.to_ascii_lowercase(), m.catalog_id.as_str()))
            .collect();
        for e in &d.entries {
            let cid = matched.get(&e.path.to_ascii_lowercase()).copied();
            let key = proposal_key(cid, &e.file_name, &e.product_name, &e.publisher);
            if dismissed.contains(key.as_str())
                || keys.contains(key.as_str())
                || cid.is_some_and(|c| linked.contains(c))
                || paths.contains(&e.path.to_ascii_lowercase())
            {
                continue;
            }
            let at = match groups.iter().position(|(k, _)| *k == key) {
                Some(at) => at,
                None => {
                    let game = cid.and_then(catalogue);
                    let name = match (&game, e.product_name.trim(), e.file_name.trim()) {
                        (Some(g), _, _) if !g.name.trim().is_empty() => g.name.trim().to_owned(),
                        (_, p, _) if !p.is_empty() => p.to_owned(),
                        (_, _, f) => f.to_owned(),
                    };
                    let launcher = match &game {
                        Some(g) => launcher_from_catalogue(g.launcher.as_deref()).to_owned(),
                        None => e.launcher.clone(),
                    };
                    groups.push((
                        key.clone(),
                        ProposalView {
                            key,
                            name,
                            file_name: e.file_name.clone(),
                            product_name: e.product_name.clone(),
                            publisher: e.publisher.clone(),
                            catalog_id: cid.map(str::to_owned),
                            cover_sha256: game.as_ref().and_then(|g| g.cover_sha256.clone()),
                            launcher,
                            category: game
                                .as_ref()
                                .and_then(|g| g.category.clone())
                                .unwrap_or_default(),
                            age_rating: game
                                .as_ref()
                                .and_then(|g| g.age_rating.clone())
                                .unwrap_or_default(),
                            pc_count: 0,
                            devices: Vec::new(),
                        },
                    ));
                    groups.len() - 1
                }
            };
            let view = &mut groups[at].1;
            if !view.devices.iter().any(|x| x.device_id == d.device_id) {
                view.devices.push(ProposalDevice {
                    device_id: d.device_id.clone(),
                    label: d.label.clone(),
                    path: e.path.clone(),
                });
                view.pc_count += 1;
            }
        }
    }
    let mut proposals: Vec<ProposalView> = groups.into_iter().map(|(_, v)| v).collect();
    proposals.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.key.cmp(&b.key))
    });
    ProposalsView {
        proposals,
        dismissed: u32::try_from(inputs.dismissed.len()).unwrap_or(u32::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::LibraryInstall;

    fn sig(game: &str, exe: &str, product: &str, publisher: &str) -> RefSignature {
        RefSignature {
            game_id: game.into(),
            exe_name: exe.into(),
            product_name: product.into(),
            publisher: publisher.into(),
            sha256: None,
        }
    }

    fn entry(path: &str, product: &str, publisher: &str) -> ScanEntry {
        let file_name = path.rsplit('\\').next().unwrap().to_owned();
        ScanEntry {
            path: path.into(),
            file_name,
            product_name: product.into(),
            publisher: publisher.into(),
            launcher: "none".into(),
        }
    }

    #[test]
    fn a_file_name_matches_when_the_signatures_other_fields_agree() {
        let sigs = [
            sig("A", "Game.exe", "", ""),
            sig("B", "game.exe", "Big Game", "Acme"),
            sig("C", "other.exe", "", ""),
        ];
        let found = [
            entry(r"C:\g\GAME.EXE", "big game", "ACME"),
            entry(r"C:\h\game.exe", "Different", "Acme"),
            entry(r"C:\h\none.exe", "", ""),
        ];
        let m = match_scan(&found, &sigs);
        // The more specific signature wins; a mismatching product falls back to the bare one.
        assert_eq!(
            m,
            [
                GameMatch {
                    catalog_id: "B".into(),
                    path: r"C:\g\GAME.EXE".into()
                },
                GameMatch {
                    catalog_id: "A".into(),
                    path: r"C:\h\game.exe".into()
                }
            ]
        );
    }

    #[test]
    fn equal_signatures_go_to_the_lowest_game_id_and_a_path_is_matched_once() {
        let sigs = [sig("Z", "x.exe", "", ""), sig("M", "x.exe", "", "")];
        let found = [entry(r"C:\x.exe", "", ""), entry(r"c:\X.EXE", "", "")];
        let m = match_scan(&found, &sigs);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].catalog_id, "M");
    }

    fn device(id: &str, entries: Vec<ScanEntry>, matches: Vec<GameMatch>) -> ScannedDevice {
        ScannedDevice {
            device_id: id.into(),
            label: format!("PC {id}"),
            entries,
            matches,
        }
    }

    fn lib_entry(
        catalog: Option<&str>,
        exe: &str,
        installs: Vec<LibraryInstall>,
    ) -> LibraryEntryView {
        LibraryEntryView {
            id: "L1".into(),
            kind: "game".into(),
            name: "N".into(),
            launcher: "none".into(),
            executable_path: exe.into(),
            launch_uri: String::new(),
            launch_args: String::new(),
            category: String::new(),
            tags: vec![],
            age_rating: String::new(),
            enabled: true,
            catalog_id: catalog.map(str::to_owned),
            installs,
            rev: 1,
        }
    }

    fn cat(id: &str) -> Option<CatalogueGame> {
        (id == "G1").then(|| CatalogueGame {
            name: "Great Game".into(),
            launcher: Some("Steam".into()),
            category: Some("Shooter".into()),
            age_rating: Some("16+".into()),
            cover_sha256: Some("ab".repeat(32)),
        })
    }

    #[test]
    fn proposals_group_by_game_or_executable_and_count_pcs() {
        let inputs = ProposalInputs {
            devices: vec![
                device(
                    "D1",
                    vec![
                        entry(r"C:\a\g.exe", "Great", "Acme"),
                        entry(r"C:\a\tool.exe", "Tool", "Me"),
                    ],
                    vec![GameMatch {
                        catalog_id: "G1".into(),
                        path: r"C:\a\g.exe".into(),
                    }],
                ),
                device(
                    "D2",
                    vec![
                        entry(r"D:\b\g.exe", "Great", "Acme"),
                        entry(r"D:\b\TOOL.exe", "tool", "me"),
                    ],
                    vec![GameMatch {
                        catalog_id: "G1".into(),
                        path: r"D:\b\g.exe".into(),
                    }],
                ),
            ],
            ..ProposalInputs::default()
        };
        let view = build_proposals(&inputs, &cat);
        assert_eq!(view.dismissed, 0);
        assert_eq!(view.proposals.len(), 2);
        let g = &view.proposals[0];
        assert_eq!(
            (g.name.as_str(), g.key.as_str(), g.pc_count),
            ("Great Game", "game:G1", 2)
        );
        assert_eq!(g.launcher, "steam");
        assert_eq!(g.category, "Shooter");
        assert_eq!(g.cover_sha256.as_deref(), Some("ab".repeat(32).as_str()));
        assert_eq!(g.devices.len(), 2);
        let t = &view.proposals[1];
        assert_eq!(t.name, "Tool");
        assert_eq!(t.key, "exe:tool.exe|tool|me");
        assert_eq!((t.catalog_id.clone(), t.pc_count), (None, 2));
    }

    #[test]
    fn approved_dismissed_and_known_path_items_are_not_proposed() {
        let found = |path: &str, product: &str| entry(path, product, "P");
        let mk = |library, dismissed| ProposalInputs {
            devices: vec![device(
                "D1",
                vec![
                    found(r"C:\a\g.exe", "Great"),
                    found(r"C:\a\t.exe", "Tool"),
                    found(r"C:\a\u.exe", "Util"),
                ],
                vec![GameMatch {
                    catalog_id: "G1".into(),
                    path: r"C:\a\g.exe".into(),
                }],
            )],
            library,
            dismissed,
        };
        // Nothing hides them at first.
        assert_eq!(
            build_proposals(&mk(vec![], vec![]), &cat).proposals.len(),
            3
        );
        // A library entry linked to the game hides it.
        let linked = vec![lib_entry(Some("G1"), "", vec![])];
        let v = build_proposals(&mk(linked, vec![]), &cat);
        assert_eq!(v.proposals.len(), 2);
        // An entry with an install carrying the key hides the executable, and one with the
        // path (any case) hides that one.
        let installs = vec![LibraryInstall {
            device_id: "D2".into(),
            path: r"Z:\z\t.exe".into(),
            match_key: "exe:t.exe|tool|p".into(),
        }];
        let by_key = vec![lib_entry(None, r"c:\A\U.EXE", installs)];
        let v = build_proposals(&mk(by_key, vec![]), &cat);
        assert_eq!(
            v.proposals
                .iter()
                .map(|p| p.key.as_str())
                .collect::<Vec<_>>(),
            ["game:G1"]
        );
        // A dismissed key hides it and is counted.
        let v = build_proposals(&mk(vec![], vec!["game:G1".into()]), &cat);
        assert_eq!((v.proposals.len(), v.dismissed), (2, 1));
    }

    #[test]
    fn an_unmatched_executable_is_named_by_product_then_file_name_and_keeps_its_launcher() {
        let mut named = entry(r"C:\a\n.exe", "", "");
        named.launcher = "epic".into();
        let inputs = ProposalInputs {
            devices: vec![device("D1", vec![named], vec![])],
            ..ProposalInputs::default()
        };
        let v = build_proposals(&inputs, &|_| None);
        assert_eq!(v.proposals[0].name, "n.exe");
        assert_eq!(v.proposals[0].launcher, "epic");
        assert!(v.proposals[0].cover_sha256.is_none());
    }
}
