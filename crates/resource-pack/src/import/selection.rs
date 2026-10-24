//! Lenient parsing of an explicitly selected global resource pack list.

use crate::MAX_LOCAL_PACKS;
use crate::manifest::Version;
use uuid::Uuid;

/// An active pack reference, in highest-priority-first file order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlobalSelectionEntry {
    pub id: Uuid,
    pub version: [u32; 3],
    pub subpack: String,
}

/// Valid references and the count of malformed, repeated or over-limit entries.
#[derive(Clone, Debug, Default)]
pub struct ParsedGlobalSelection {
    pub entries: Vec<GlobalSelectionEntry>,
    pub skipped: usize,
}

/// Preserves file order and drops only entries without a usable identity and version.
pub fn parse_global_selection(bytes: &[u8]) -> ParsedGlobalSelection {
    let mut parsed = ParsedGlobalSelection::default();
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_slice(bytes) else {
        parsed.skipped = 1;
        return parsed;
    };
    let mut seen = std::collections::HashSet::new();
    for value in entries {
        let entry = (|| {
            let id = Uuid::parse_str(value["pack_id"].as_str()?).ok()?;
            let version = Version::from_value(&value["version"])?;
            let subpack = if value["subpack"].is_null() {
                ""
            } else {
                value["subpack"].as_str()?
            };
            if id.is_nil()
                || subpack.len() > crate::MAX_PATH_BYTES
                || parsed.entries.len() >= MAX_LOCAL_PACKS
                || !seen.insert(id)
            {
                return None;
            }
            Some(GlobalSelectionEntry {
                id,
                version: version.0,
                subpack: subpack.to_owned(),
            })
        })();
        match entry {
            Some(entry) => parsed.entries.push(entry),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

/// Places compatible imported references first, then retains existing active packs within limits.
pub fn merge_global_selection(
    current: &[crate::ActivePack],
    available: &[crate::InstalledPack],
    imported: &[GlobalSelectionEntry],
    engine: [u32; 3],
) -> (Vec<crate::ActivePack>, usize) {
    let mut ordered = Vec::new();
    let mut skipped = 0;
    for entry in imported {
        let pack = available
            .iter()
            .find(|pack| pack.id == entry.id && pack.version == entry.version);
        let Some(pack) = pack.filter(|pack| {
            pack.min_engine_version
                .is_none_or(|minimum| minimum <= engine)
                && (entry.subpack.is_empty()
                    || pack
                        .subpacks
                        .iter()
                        .any(|subpack| subpack.folder == entry.subpack))
        }) else {
            skipped += 1;
            continue;
        };
        let remaining = current
            .iter()
            .filter(|old| {
                old.id != entry.id
                    && !ordered
                        .iter()
                        .any(|active: &crate::ActivePack| active.id == old.id)
            })
            .count();
        if ordered.len() + remaining >= MAX_LOCAL_PACKS
            || ordered
                .iter()
                .any(|active: &crate::ActivePack| active.id == entry.id)
        {
            skipped += 1;
            continue;
        }
        ordered.push(crate::ActivePack {
            id: entry.id,
            subpack: entry.subpack.clone(),
            revision: pack.revision,
        });
    }
    for old in current {
        if ordered.iter().any(|active| active.id == old.id) {
            continue;
        }
        if ordered.len() >= MAX_LOCAL_PACKS {
            skipped += 1;
        } else {
            ordered.push(old.clone());
        }
    }
    (ordered, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_existing_selection_is_preserved_when_imported_additions_exceed_the_limit() {
        let current = (1..=MAX_LOCAL_PACKS)
            .map(|id| crate::ActivePack {
                id: Uuid::from_u128(id as u128),
                subpack: String::new(),
                revision: 1,
            })
            .collect::<Vec<_>>();
        let added = Uuid::from_u128(MAX_LOCAL_PACKS as u128 + 1);
        let available = [crate::InstalledPack {
            id: added,
            version: [1, 0, 0],
            name: "Extra".into(),
            description: String::new(),
            min_engine_version: None,
            subpacks: Vec::new(),
            revision: 2,
        }];
        let imported = [GlobalSelectionEntry {
            id: added,
            version: [1, 0, 0],
            subpack: String::new(),
        }];
        let (selected, skipped) =
            merge_global_selection(&current, &available, &imported, [1, 26, 50]);
        assert_eq!(selected, current);
        assert_eq!(skipped, 1);
    }

    #[test]
    fn missing_versions_and_unsupported_subpacks_are_skipped_without_reordering_survivors() {
        let id = Uuid::from_u128(17);
        let old = Uuid::from_u128(18);
        let available = vec![crate::InstalledPack {
            id,
            version: [1, 0, 0],
            name: "Fixture".into(),
            description: String::new(),
            min_engine_version: None,
            subpacks: Vec::new(),
            revision: 7,
        }];
        let parsed = vec![
            GlobalSelectionEntry {
                id,
                version: [2, 0, 0],
                subpack: String::new(),
            },
            GlobalSelectionEntry {
                id,
                version: [1, 0, 0],
                subpack: "missing".into(),
            },
            GlobalSelectionEntry {
                id,
                version: [1, 0, 0],
                subpack: String::new(),
            },
        ];
        let existing = vec![crate::ActivePack {
            id: old,
            subpack: String::new(),
            revision: 1,
        }];
        let (selected, skipped) =
            merge_global_selection(&existing, &available, &parsed, [1, 26, 50]);
        assert_eq!(skipped, 2);
        assert_eq!(
            selected.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![id, old]
        );
        assert_eq!(selected[0].revision, 7);
    }

    #[test]
    fn selection_keeps_order_and_skips_bad_rows() {
        let parsed = parse_global_selection(
            br#"[
            {"pack_id":"11111111-1111-1111-1111-111111111111","version":[1,2,3],"subpack":"low"},
            {"pack_id":"bad","version":[1,0,0]},
            {"pack_id":"22222222-2222-2222-2222-222222222222","version":"2.0.1"},
            {"pack_id":"11111111-1111-1111-1111-111111111111","version":[1,2,3]}
        ]"#,
        );
        assert_eq!(parsed.skipped, 2);
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].subpack, "low");
        assert_eq!(parsed.entries[1].version, [2, 0, 1]);
        assert_eq!(parse_global_selection(b"broken").skipped, 1);
    }
}
