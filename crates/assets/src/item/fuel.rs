//! Checked fuel facts resolved before inventory routing.

use super::retail;
use crate::{AssetError, EntityAssetSource, ItemVisualDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::OnceLock};

/// Logical source identity for the retained compatibility choices.
pub const SOURCE_PATH: &str = "registry/item-fuel-v2193.tsv";
/// Checkout input fingerprinted by the entity carrier recipe.
pub const INPUT_PATH: &str = "crates/assets/data/item-fuel-v2193.tsv";
/// The single retained positive compatibility input.
pub const SOURCE_BYTES: &[u8] = include_bytes!("../../data/item-fuel-v2193.tsv");

/// Unknown fuel eligibility is separate from a declared positive or negative choice.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "support", deny_unknown_fields)]
pub enum ItemFuel {
    #[default]
    Unresolved,
    Provisional {
        eligible: bool,
        ingredient_first: bool,
        burn_ticks: Option<u32>,
    },
    Session {
        burn_ticks: u32,
    },
}

impl ItemFuel {
    /// Returns the admitted eligibility; unknown facts remain absent.
    pub const fn eligible(self) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::Provisional { eligible, .. } => Some(eligible),
            Self::Session { burn_ticks } => Some(burn_ticks != 0),
        }
    }

    /// Retains a compiled ingredient preference; session-only fuel has no recipe evidence.
    pub const fn ingredient_first(self) -> Option<bool> {
        match self {
            Self::Provisional {
                ingredient_first, ..
            } => Some(ingredient_first),
            Self::Unresolved | Self::Session { .. } => None,
        }
    }

    /// Returns a supplied duration; retained compatibility choices have no verified duration.
    pub const fn burn_ticks(self) -> Option<u32> {
        match self {
            Self::Unresolved => None,
            Self::Provisional { burn_ticks, .. } => burn_ticks,
            Self::Session { burn_ticks } => Some(burn_ticks),
        }
    }
}

/// Reads exact positive identities once and rejects unsupported catalog entries.
fn positives() -> &'static BTreeMap<&'static str, bool> {
    static POSITIVES: OnceLock<BTreeMap<&'static str, bool>> = OnceLock::new();
    POSITIVES.get_or_init(|| {
        let mut positive = BTreeMap::new();
        for row in std::str::from_utf8(SOURCE_BYTES)
            .expect("fuel input is UTF-8")
            .lines()
            .filter(|row| !row.is_empty() && !row.starts_with('#'))
        {
            let (identifier, priority) = row.split_once('\t').expect("fuel row has two fields");
            let priority = match priority {
                "0" => false,
                "1" => true,
                _ => panic!("invalid fuel ingredient priority"),
            };
            assert!(
                retail::identifiers().contains(identifier),
                "unsupported fuel identity {identifier}"
            );
            assert!(
                positive.insert(identifier, priority).is_none(),
                "duplicate fuel identity"
            );
        }
        positive
    })
}

/// Resolves compatibility data during compilation; unknown identities stay unresolved.
pub fn builtin_item_fuel(identifier: &str) -> ItemFuel {
    if !retail::identifiers().contains(identifier) {
        return ItemFuel::Unresolved;
    }
    ItemFuel::Provisional {
        eligible: positives().contains_key(identifier),
        ingredient_first: positives().get(identifier).copied().unwrap_or(false),
        burn_ticks: None,
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
                return Err(invalid("item fuel source identity mismatch"));
            }
            checked[index] = true;
        }
    }
    for visual in visuals {
        if visual.fuel != ItemFuel::Unresolved
            && (checked != [true; 2] || visual.fuel != builtin_item_fuel(&visual.key.identifier))
        {
            return Err(invalid(
                "item fuel is outside its checked identity inventory",
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
            fuel: builtin_item_fuel("minecraft:coal"),
            armour: crate::item::armour::ItemArmour::Unresolved,
            durability: crate::item::durability::ItemDurability::Unresolved,
            key: crate::ItemVisualKey {
                identifier: "minecraft:coal".into(),
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
    fn fuel_facts_require_checked_sources_and_an_exact_identity() {
        let (mut visual, mut sources) = fixture();
        assert!(validate(std::slice::from_ref(&visual), &sources).is_ok());
        assert!(validate(std::slice::from_ref(&visual), &[]).is_err());
        sources[0].source_sha256[0] ^= 1;
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        sources[0].source_sha256[0] ^= 1;
        sources[1].source_bytes += 1;
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        sources[1].source_bytes -= 1;
        visual.key.identifier = "custom:coal".into();
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
        visual.fuel = ItemFuel::Session { burn_ticks: 20 };
        assert!(validate(std::slice::from_ref(&visual), &sources).is_err());
    }

    #[test]
    fn every_independent_retail_identity_has_an_explicit_fuel_disposition() {
        for identifier in std::str::from_utf8(retail::SOURCE_BYTES)
            .unwrap()
            .lines()
            .filter_map(|row| row.split('\t').nth(1))
        {
            assert!(matches!(
                builtin_item_fuel(identifier),
                ItemFuel::Provisional { .. }
            ));
        }
        assert_eq!(builtin_item_fuel("custom:coal"), ItemFuel::Unresolved);
        assert_eq!(builtin_item_fuel("minecraft:coal").eligible(), Some(true));
        assert_eq!(
            builtin_item_fuel("minecraft:diamond").eligible(),
            Some(false)
        );
        assert_eq!(
            builtin_item_fuel("minecraft:crimson_planks").eligible(),
            Some(false)
        );
    }
}
