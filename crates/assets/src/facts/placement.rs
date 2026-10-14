//! Placement admission and related block identities compiled by registryc.

use std::collections::{BTreeMap, BTreeSet};

use super::{BlockProperty as P, rules::Rule};
use crate::{
    AssetError, RegistryRecord,
    registry::{BlockTraits as T, ModelFamily as F},
};

pub const BLOCK_PLACEMENT_SOURCE_PATH: &str = "crates/assets/data/block-placement-v2193.tsv";

/// Declares placement names and their parser from one exhaustive list.
macro_rules! named_facts {
    ($(#[$doc:meta])* $name:ident { $($rule:ident),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub enum $name {
            #[default]
            Unresolved,
            $($rule),+
        }
        impl $name {
            /// Reads a compiler catalog value; unknown rules remain invalid.
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    "Unresolved" => Some(Self::Unresolved),
                    $(stringify!($rule) => Some(Self::$rule),)+
                    _ => None,
                }
            }
        }
    };
}
named_facts!(
    /// The placement transformation admitted for a block's state shape.
    PlacementRule {
    Identity,
    Axis,
    Loom,
    Glazed,
    Stair,
    Slab,
    DoubleSlab,
    Trapdoor,
    Hopper,
    Connections,
    Torch,
    Lever,
    Lantern,
    Snow,
    SeaPickle,
    Candle,
    Vine,
    Multiface,
    Door,
    StandingSign,
    WallSign,
    Dispenser,
    Piston,
    Button
});

named_facts!(
    /// Support classifications retained by provisional placement prediction.
    SupportKind { Stone, PowderSnow, Leaves, Water, Lava, Air, Fire, SnowLayer, Vanilla }
);

impl SupportKind {
    /// Classifies readable support metadata once at registry or custom-block admission.
    pub fn for_identifier(identifier: &str) -> Self {
        match identifier {
            "minecraft:stone" => Self::Stone,
            "minecraft:powder_snow" => Self::PowderSnow,
            "minecraft:air" => Self::Air,
            "minecraft:water" | "minecraft:flowing_water" => Self::Water,
            "minecraft:lava" | "minecraft:flowing_lava" => Self::Lava,
            "minecraft:fire" | "minecraft:soul_fire" => Self::Fire,
            "minecraft:snow_layer" => Self::SnowLayer,
            "minecraft:leaves" | "minecraft:leaves2" => Self::Leaves,
            name if name.ends_with("_leaves") => Self::Leaves,
            name if name.starts_with("minecraft:") => Self::Vanilla,
            _ => Self::Unresolved,
        }
    }
}

/// Placement and counterpart metadata admitted for one block identity.
#[derive(Debug)]
pub struct PlacementEntry {
    pub rule: PlacementRule,
    pub support: SupportKind,
    pub alternate: Option<String>,
}

/// Selects transformations from state shape and previously compiled identity facts.
fn rule(record: &RegistryRecord, rules: &[Rule]) -> Result<PlacementRule, AssetError> {
    let state = serde_json::from_str(&record.canonical_state)
        .map_err(|error| invalid(error.to_string()))?;
    for rule in rules {
        if rule.fact.starts_with("placement:")
            && rule.matches(&record.name, &state, record.model_family)
        {
            return PlacementRule::from_name(rule.fact.strip_prefix("placement:").unwrap())
                .ok_or_else(|| invalid("unknown placement rule"));
        }
    }
    let traits = record.traits;
    Ok(if traits.contains(T::TORCH) {
        PlacementRule::Torch
    } else if traits.contains(T::MULTIFACE) {
        PlacementRule::Multiface
    } else if traits.contains(T::LANTERN_LIKE) {
        PlacementRule::Lantern
    } else if traits.contains(T::DOOR) {
        PlacementRule::Door
    } else if traits.contains(T::SNOW_LAYER) {
        PlacementRule::Snow
    } else if traits.contains(T::CANDLE) {
        PlacementRule::Candle
    } else if traits.contains(T::STANDING_SIGN) {
        PlacementRule::StandingSign
    } else if traits.contains(T::WALL_SIGN) {
        PlacementRule::WallSign
    } else if traits.contains(T::HOPPER) {
        PlacementRule::Hopper
    } else if traits.contains(T::BODY_RELATIVE_FACING) {
        if traits.contains(T::PISTON) {
            PlacementRule::Piston
        } else {
            PlacementRule::Dispenser
        }
    } else if traits.intersects(T::CONNECTS_THIN | T::CONNECTS_FENCE) {
        PlacementRule::Connections
    } else if state.contains_key(P::ClusterCount.name()) {
        PlacementRule::SeaPickle
    } else {
        match record.model_family {
            F::Stair => PlacementRule::Stair,
            F::Slab if traits.contains(T::DOUBLE_SLAB) => PlacementRule::DoubleSlab,
            F::Slab => PlacementRule::Slab,
            F::Trapdoor => PlacementRule::Trapdoor,
            F::Lever => PlacementRule::Lever,
            F::Button => PlacementRule::Button,
            F::Vine => PlacementRule::Vine,
            _ if state.contains_key(P::PillarAxis.name()) => PlacementRule::Axis,
            _ if record.properties.is_empty() && record.canonical_state.as_ref() == "{}" => {
                PlacementRule::Identity
            }
            _ => PlacementRule::Unresolved,
        }
    })
}

/// Expands the existing naming contract once, requiring a collected counterpart.
fn counterpart(name: &str, rule: PlacementRule, names: &BTreeSet<&str>) -> Option<String> {
    let target = match rule {
        PlacementRule::Slab => {
            if let Some(prefix) = name.strip_suffix("cut_copper_slab") {
                format!("{prefix}double_cut_copper_slab")
            } else {
                format!("{}_double_slab", name.strip_suffix("_slab")?)
            }
        }
        PlacementRule::StandingSign => format!("{}wall_sign", name.strip_suffix("standing_sign")?),
        PlacementRule::WallSign => format!("{}standing_sign", name.strip_suffix("wall_sign")?),
        _ => return None,
    };
    names.contains(target.as_str()).then_some(target)
}

/// Builds one palette's placement catalog from shared selectors and decoded traits.
pub fn placement_catalog<'a>(
    records: &'a [RegistryRecord],
    rules: &[Rule],
) -> Result<BTreeMap<&'a str, PlacementEntry>, AssetError> {
    let names = records.iter().map(|record| record.name.as_ref()).collect();
    let mut rows = BTreeMap::new();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() != super::block::RESERVED_BLOCK_NAME)
    {
        let compiled = rule(record, rules)?;
        let entry = rows.entry(record.name.as_ref()).or_insert(PlacementEntry {
            rule: compiled,
            support: SupportKind::for_identifier(&record.name),
            alternate: None,
        });
        if entry.rule != compiled {
            entry.rule = PlacementRule::Unresolved;
        }
    }
    for (&name, entry) in &mut rows {
        entry.alternate = counterpart(name, entry.rule, &names);
    }
    Ok(rows)
}

/// Describes a malformed embedded placement selector or canonical state.
fn invalid(detail: impl Into<String>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admitted_catalog_matches_the_generated_current_catalog() {
        let records = crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .unwrap();
        let catalog =
            placement_catalog(&records, &super::super::rules::policy().placement_rules).unwrap();
        let rows: Vec<_> = include_str!("../../data/block-placement-v2193.tsv")
            .lines()
            .skip(1)
            .collect();
        assert_eq!(catalog.len(), rows.len());
        for row in rows {
            let fields: Vec<_> = row.split('\t').collect();
            let entry = catalog.get(fields[0]).unwrap();
            assert_eq!(
                entry.rule,
                PlacementRule::from_name(fields[1]).unwrap(),
                "{}",
                fields[0]
            );
            assert_eq!(
                entry.support,
                SupportKind::from_name(fields[2]).unwrap(),
                "{}",
                fields[0]
            );
            assert_eq!(
                entry.alternate.as_deref().unwrap_or("-"),
                fields[3],
                "{}",
                fields[0]
            );
        }
    }

    #[test]
    fn counterparts_require_an_admitted_target_in_each_palette() {
        for records in [
            crate::read_registry(include_bytes!("../../data/block-registry-v1001.bin")).unwrap(),
            crate::read_registry_for_protocol(
                crate::pinned_block_registry_bytes(),
                crate::active_content_registry_protocol(),
            )
            .unwrap(),
        ] {
            let rules = &super::super::rules::policy().placement_rules;
            let catalog = placement_catalog(&records, rules).unwrap();
            let (&name, entry) = catalog
                .iter()
                .find(|(_, entry)| entry.alternate.is_some())
                .unwrap();
            let name = name.to_owned();
            let target = entry.alternate.as_deref().unwrap();
            assert!(catalog.contains_key(target));
            let partial: Vec<_> = records
                .into_iter()
                .filter(|record| record.name.as_ref() == name.as_str())
                .collect();
            assert!(
                placement_catalog(&partial, rules).unwrap()[name.as_str()]
                    .alternate
                    .is_none()
            );
        }
    }
}
