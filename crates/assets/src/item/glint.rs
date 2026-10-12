//! Checked intrinsic shine facts, admitted before stack presentation.

use super::retail;
use crate::{AssetError, EntityAssetSource, ItemVisualDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::OnceLock};

/// Logical source identity for the retained compatibility choices.
pub const SOURCE_PATH: &str = "registry/item-glint-v2193.tsv";
/// Checkout input fingerprinted by the entity carrier recipe.
pub const INPUT_PATH: &str = "crates/assets/data/item-glint-v2193.tsv";
/// The single retained positive compatibility input.
pub const SOURCE_BYTES: &[u8] = include_bytes!("../../data/item-glint-v2193.tsv");

/// Unknown shine is separate from a declared positive or negative choice.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "support", deny_unknown_fields)]
pub enum ItemGlint {
    #[default]
    Unresolved,
    Provisional {
        enabled: bool,
    },
    Session {
        enabled: bool,
    },
}

impl ItemGlint {
    /// Returns the admitted intrinsic choice; enchantments are separate stack facts.
    pub const fn enabled(self) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::Provisional { enabled } | Self::Session { enabled } => Some(enabled),
        }
    }
}

/// Reads exact positive identities once and rejects unsupported catalog entries.
fn positives() -> &'static BTreeSet<&'static str> {
    static POSITIVES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    POSITIVES.get_or_init(|| {
        let mut positive = BTreeSet::new();
        for identifier in std::str::from_utf8(SOURCE_BYTES)
            .expect("glint input is UTF-8")
            .lines()
            .filter(|row| !row.is_empty() && !row.starts_with('#'))
        {
            assert!(
                retail::identifiers().contains(identifier),
                "unsupported glint identity {identifier}"
            );
            assert!(positive.insert(identifier), "duplicate glint identity");
        }
        positive
    })
}

/// Resolves compatibility data during compilation; unknown identities stay unresolved.
pub fn builtin_item_glint(identifier: &str) -> ItemGlint {
    if !retail::identifiers().contains(identifier) {
        return ItemGlint::Unresolved;
    }
    ItemGlint::Provisional {
        enabled: positives().contains(identifier),
    }
}

/// Rejects promoted facts without the exact input fingerprints and admitted identity.
pub(crate) fn validate(
    visuals: &[ItemVisualDefinition],
    sources: &[EntityAssetSource],
) -> Result<(), AssetError> {
    let mut checked = [false; 2];
    for (index, (path, bytes)) in [
        (SOURCE_PATH, SOURCE_BYTES),
        (retail::SOURCE_PATH, retail::SOURCE_BYTES),
    ]
    .into_iter()
    .enumerate()
    {
        for source in sources.iter().filter(|source| source.path.as_ref() == path) {
            if source.source_bytes as usize != bytes.len()
                || source.source_sha256 != <[u8; 32]>::from(Sha256::digest(bytes))
            {
                return Err(invalid("item glint source identity mismatch"));
            }
            checked[index] = true;
        }
    }
    for visual in visuals {
        if visual.glint != ItemGlint::Unresolved
            && (checked != [true; 2] || visual.glint != builtin_item_glint(&visual.key.identifier))
        {
            return Err(invalid(
                "item glint is outside its checked identity inventory",
            ));
        }
    }
    Ok(())
}

/// Reports invalid descriptor data using the owning carrier error.
fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one missing-art descriptor with exact checked input identities.
    fn fixture() -> (ItemVisualDefinition, Vec<EntityAssetSource>) {
        let visual = ItemVisualDefinition {
            grip: crate::item::grip::ItemGrip::Unresolved,
            glint: builtin_item_glint("minecraft:enchanted_book"),
            fuel: crate::item::fuel::ItemFuel::Unresolved,
            armour: crate::item::armour::ItemArmour::Unresolved,
            durability: crate::item::durability::ItemDurability::Unresolved,
            key: crate::ItemVisualKey {
                identifier: "minecraft:enchanted_book".into(),
                metadata: 0,
            },
            source: 0,
            route: crate::ItemVisualDefinitionRoute::Missing,
            first_person: crate::ItemDisplayTransform::identity(),
            third_person: crate::ItemDisplayTransform::identity(),
            dropped: crate::ItemDisplayTransform::identity(),
        };
        let sources = [
            (SOURCE_PATH, SOURCE_BYTES),
            (retail::SOURCE_PATH, retail::SOURCE_BYTES),
        ]
        .into_iter()
        .map(|(path, bytes)| EntityAssetSource {
            path: path.into(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(bytes).into(),
        })
        .collect();
        (visual, sources)
    }

    #[test]
    fn intrinsic_glint_facts_require_checked_sources_and_an_exact_identity() {
        let (mut visual, mut sources) = fixture();
        assert!(validate(std::slice::from_ref(&visual), &sources).is_ok());
        assert!(validate(std::slice::from_ref(&visual), &[]).is_err());
        sources[0].source_sha256[0] ^= 1;
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        sources[0].source_sha256[0] ^= 1;
        sources[1].source_bytes += 1;
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        sources[1].source_bytes -= 1;
        visual.key.identifier = "custom:enchanted_book".into();
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        visual.glint = ItemGlint::Session { enabled: true };
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
    }

    #[test]
    fn every_independent_retail_identity_has_an_explicit_glint_disposition() {
        for identifier in std::str::from_utf8(retail::SOURCE_BYTES)
            .unwrap()
            .lines()
            .filter_map(|row| row.split('\t').nth(1))
        {
            assert!(matches!(
                builtin_item_glint(identifier),
                ItemGlint::Provisional { .. }
            ));
        }
        assert_eq!(
            builtin_item_glint("custom:enchanted_book"),
            ItemGlint::Unresolved
        );
        assert_eq!(
            builtin_item_glint("minecraft:enchanted_book").enabled(),
            Some(true)
        );
        assert_eq!(builtin_item_glint("minecraft:stick").enabled(), Some(false));
        assert_eq!(
            builtin_item_glint("minecraft:written_book"),
            ItemGlint::Unresolved
        );
    }
}
