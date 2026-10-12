//! Checked item grip facts, resolved before equipment rendering.

use super::retail;
use std::{collections::BTreeMap, sync::OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AssetError, EntityAssetSource, ItemVisualDefinition};

/// The retained compatibility grip catalog included in carrier fingerprints.
pub const GRIP_SOURCE_PATH: &str = "registry/item-grips-v2193.tsv";
/// Checkout input used by the entity carrier recipe.
pub const GRIP_INPUT_PATH: &str = "crates/assets/data/item-grips-v2193.tsv";
/// The single positive compatibility catalog used by compilation and validation.
pub const GRIP_SOURCE_BYTES: &[u8] = include_bytes!("../../data/item-grips-v2193.tsv");
/// Unresolved grip is separate from an admitted flat or upright placement.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "support", deny_unknown_fields)]
pub enum ItemGrip {
    #[default]
    Unresolved,
    Provisional {
        upright: bool,
        mirrored_art: bool,
    },
    Session {
        upright: bool,
        mirrored_art: bool,
    },
}

impl ItemGrip {
    /// Returns the admitted upright choice; missing evidence remains absent.
    pub const fn upright(self) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::Provisional { upright, .. } | Self::Session { upright, .. } => Some(upright),
        }
    }

    /// Uses mirrored rod art only when the compiled compatibility record requests it.
    pub const fn mirrored_art(self) -> bool {
        matches!(
            self,
            Self::Provisional {
                mirrored_art: true,
                ..
            } | Self::Session {
                mirrored_art: true,
                ..
            }
        )
    }
}

/// Builds the two independently owned admission inputs once, outside frame paths.
fn catalog() -> &'static BTreeMap<&'static str, bool> {
    static CATALOG: OnceLock<BTreeMap<&'static str, bool>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut positive = BTreeMap::new();
        for line in std::str::from_utf8(GRIP_SOURCE_BYTES)
            .expect("grip inventory is UTF-8")
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let (identifier, mirrored) = line.split_once('\t').expect("grip row has two fields");
            assert!(
                retail::identifiers().contains(identifier),
                "unadmitted grip {identifier}"
            );
            let mirrored = match mirrored {
                "0" => false,
                "1" => true,
                _ => panic!("invalid grip flag"),
            };
            assert!(
                positive.insert(identifier, mirrored).is_none(),
                "duplicate grip {identifier}"
            );
        }
        positive
    })
}

/// Resolves a compatibility fact during compilation or cold fixture admission.
pub fn builtin_item_grip(identifier: &str) -> ItemGrip {
    let catalog = catalog();
    if !retail::identifiers().contains(identifier) {
        return ItemGrip::Unresolved;
    }
    ItemGrip::Provisional {
        upright: catalog.contains_key(identifier),
        mirrored_art: catalog.get(identifier).copied().unwrap_or(false),
    }
}

/// Rejects promoted facts without both exact catalog fingerprints and identity coverage.
pub(crate) fn validate(
    visuals: &[ItemVisualDefinition],
    sources: &[EntityAssetSource],
) -> Result<(), AssetError> {
    let mut checked = [false; 2];
    for (index, (path, bytes)) in [
        (GRIP_SOURCE_PATH, GRIP_SOURCE_BYTES),
        (retail::SOURCE_PATH, retail::SOURCE_BYTES),
    ]
    .into_iter()
    .enumerate()
    {
        for source in sources.iter().filter(|source| source.path.as_ref() == path) {
            if source.source_bytes as usize != bytes.len()
                || source.source_sha256 != <[u8; 32]>::from(Sha256::digest(bytes))
            {
                return Err(invalid("item grip source identity mismatch"));
            }
            checked[index] = true;
        }
    }
    for visual in visuals {
        if visual.grip != ItemGrip::Unresolved
            && (checked != [true; 2] || visual.grip != builtin_item_grip(&visual.key.identifier))
        {
            return Err(invalid(
                "item grip is outside its checked identity inventory",
            ));
        }
    }
    Ok(())
}

/// Reports invalid checked grip data using the owning carrier error.
fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the two exact source identities required for promoted grip facts.
    fn sources() -> Vec<EntityAssetSource> {
        [
            (GRIP_SOURCE_PATH, GRIP_SOURCE_BYTES),
            (retail::SOURCE_PATH, retail::SOURCE_BYTES),
        ]
        .into_iter()
        .map(|(path, bytes)| EntityAssetSource {
            path: path.into(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(bytes).into(),
        })
        .collect()
    }

    /// Creates an original, missing-art definition for the descriptor validator.
    fn visual() -> ItemVisualDefinition {
        ItemVisualDefinition {
            grip: builtin_item_grip("minecraft:diamond_sword"),
            glint: crate::item::glint::ItemGlint::Unresolved,
            fuel: crate::item::fuel::ItemFuel::Unresolved,
            armour: crate::item::armour::ItemArmour::Unresolved,
            durability: crate::item::durability::ItemDurability::Unresolved,
            key: crate::ItemVisualKey {
                identifier: "minecraft:diamond_sword".into(),
                metadata: 0,
            },
            source: 0,
            route: crate::ItemVisualDefinitionRoute::Missing,
            first_person: crate::ItemDisplayTransform::identity(),
            third_person: crate::ItemDisplayTransform::identity(),
            dropped: crate::ItemDisplayTransform::identity(),
        }
    }

    #[test]
    fn promoted_grips_reject_missing_sources_forged_hashes_and_custom_aliases() {
        let mut visual = visual();
        let mut sources = sources();
        assert!(validate(std::slice::from_ref(&visual), &sources).is_ok());
        assert!(validate(std::slice::from_ref(&visual), &[]).is_err());
        sources[0].source_sha256[0] ^= 1;
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        sources[0].source_sha256[0] ^= 1;
        sources[1].source_bytes += 1;
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        sources[1].source_bytes -= 1;
        visual.key.identifier = "custom:diamond_sword".into();
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        visual.grip = ItemGrip::Session {
            upright: true,
            mirrored_art: false,
        };
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
    }

    #[test]
    fn unknown_names_cannot_inherit_a_tool_or_rod_grip() {
        assert_eq!(
            builtin_item_grip("custom:diamond_sword"),
            ItemGrip::Unresolved
        );
        assert_eq!(
            builtin_item_grip("custom:fishing_rod"),
            ItemGrip::Unresolved
        );
        assert_eq!(
            builtin_item_grip("minecraft:diamond_sword").upright(),
            Some(true)
        );
        assert_eq!(
            builtin_item_grip("minecraft:name_tag").upright(),
            Some(false)
        );
        assert!(builtin_item_grip("minecraft:fishing_rod").mirrored_art());
        assert_eq!(
            retail::identifiers().len(),
            std::str::from_utf8(retail::SOURCE_BYTES)
                .unwrap()
                .lines()
                .count()
        );
    }
}
