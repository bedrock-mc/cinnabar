//! Compiled item-use facts admitted before gameplay, audio or animation reads them.

use super::{CompiledEntityAssets, RuntimeEntityAssets, invalid};
use crate::{AssetError, ItemVisualDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::OnceLock};

/// Movement factor for an active item use without a `minecraft:use_modifiers` component.
pub const DEFAULT_USE_MOVEMENT_MODIFIER: f64 = 0.35;

/// Compatibility maximum duration shared by long-held weapon actions and animation.
pub const LONG_WEAPON_USE_TICKS: u32 = 72_000;

/// Eat or drink duration for a consumable that declares none.
pub const DEFAULT_CONSUME_USE_TICKS: u32 = 32;

/// Compatibility crossbow charge before stack enchantments are applied.
pub const CROSSBOW_CHARGE_TICKS: u32 = 25;
/// Charge reduction for one admitted Quick Charge enchantment level.
pub const QUICK_CHARGE_TICKS_PER_LEVEL: u32 = 5;

pub const SOURCE_PATH: &str = "registry/item-use-profiles.json";
const SOURCE_BYTES: &[u8] = include_bytes!("../../data/item-use-profiles.json");

/// Immutable use behavior; an absent admission grants no predicted behavior.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ItemUseProfile {
    #[default]
    Unresolved,
    /// Compatibility facts whose complete native census remains open.
    Provisional {
        animation: ItemUseAnimation,
        kind: ItemUseKind,
        duration: ItemUseDurationRule,
        requirement: ItemUseRequirement,
        consumption: ItemConsumption,
        movement_bits: u64,
        cooldown: Option<ItemUseCategory>,
        projectile: ItemProjectileRole,
        duration_alias: Option<Box<str>>,
    },
}

/// Item drawing route admitted independently from its displayed identity.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ItemUseAnimation {
    #[default]
    None,
    Bow,
    Crossbow,
}

/// Action selected independently from a declared use duration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ItemUseKind {
    Hold,
    /// Sends the use transaction and swings the local arm without consuming the stack.
    Swing,
    Throw,
    Crossbow,
    None,
    GlideRocket,
}

/// Stack-dependent duration rules contain no identifier routing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ItemUseDurationRule {
    Declared,
    LongWeapon,
    Fixed(u32),
    Crossbow,
}

/// Required resources checked against authoritative inventory and player state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ItemUseRequirement {
    Nothing,
    Arrow,
    ArrowOrOffhandRocket,
    Appetite,
}

/// Consumption presentation and sound, independent from duration and use kind.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ItemConsumption {
    #[default]
    Unresolved,
    None,
    Eat,
    Drink,
    Honey,
}

/// Projectile role resolved once, rather than by named inventory scans.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ItemProjectileRole {
    #[default]
    None,
    Arrow,
    Firework,
}

/// Owned cooldown text is interned into a numerical category at session admission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemUseCategory {
    pub category: Box<str>,
    pub ticks: u32,
}

/// One independently collected item identity and its use admission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemUseDescriptor {
    pub identifier: Box<str>,
    pub profile: ItemUseProfile,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u32,
    entries: BTreeMap<Box<str>, ItemUseProfile>,
}

/// Parses the single compatibility source during compilation or cold admission only.
fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let catalog: Catalog = serde_json::from_slice(SOURCE_BYTES).expect("valid item-use source");
        assert_eq!(catalog.version, 2);
        catalog
    })
}

/// Provides the canonical source identity independently from checkout whitespace.
pub fn source_identity() -> (u32, [u8; 32]) {
    let bytes = serde_json::to_vec(catalog()).expect("item-use source can be encoded");
    (bytes.len() as u32, Sha256::digest(bytes).into())
}

/// Emits an explicit unresolved entry for each independently collected item.
fn unresolved_inventory(items: &[ItemVisualDefinition]) -> BTreeMap<Box<str>, ItemUseProfile> {
    items
        .iter()
        .map(|item| (item.key.identifier.clone(), ItemUseProfile::Unresolved))
        .collect()
}

/// Joins compatibility admissions with the separate item visual inventory.
pub fn compile(items: &[ItemVisualDefinition]) -> Box<[ItemUseDescriptor]> {
    let mut entries = unresolved_inventory(items);
    entries.extend(catalog().entries.clone());
    entries
        .into_iter()
        .map(|(identifier, profile)| ItemUseDescriptor {
            identifier,
            profile,
        })
        .collect()
}

/// Resolves a diagnostic identity outside frame and tick paths.
pub fn diagnostic_profile(identifier: &str) -> ItemUseProfile {
    catalog()
        .entries
        .get(identifier)
        .cloned()
        .unwrap_or_default()
}

/// Rejects missing, forged or unsupported admissions against the independent item inventory.
pub(super) fn validate(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    let source = compiled
        .sources
        .iter()
        .find(|source| source.path.as_ref() == SOURCE_PATH);
    let expected = if let Some(source) = source {
        let (bytes, hash) = source_identity();
        if source.source_bytes != bytes || source.source_sha256 != hash {
            return Err(invalid(
                "item-use source identity mismatch; rebuild with make assets",
            ));
        }
        compile(&compiled.item_visuals)
    } else {
        unresolved_inventory(&compiled.item_visuals)
            .into_iter()
            .map(|(identifier, profile)| ItemUseDescriptor {
                identifier,
                profile,
            })
            .collect()
    };
    if compiled.item_use_profiles != expected {
        return Err(invalid(
            "item-use coverage or admission mismatch; rebuild with make assets",
        ));
    }
    Ok(())
}

impl CompiledEntityAssets {
    /// Declares every original unsupported fixture item without manufacturing compatibility facts.
    pub fn with_unresolved_item_use(mut self) -> Self {
        self.item_use_profiles = unresolved_inventory(&self.item_visuals)
            .into_iter()
            .map(|(identifier, profile)| ItemUseDescriptor {
                identifier,
                profile,
            })
            .collect();
        self
    }
}

impl RuntimeEntityAssets {
    /// Borrows compiled profiles when copying a carrier without changing its item admissions.
    pub fn item_use_profiles(&self) -> &[ItemUseDescriptor] {
        &self.item_use_profiles
    }

    /// Resolves a compiled item profile during numerical registry admission only.
    pub fn item_use_profile(&self, identifier: &str) -> ItemUseProfile {
        self.item_use_profiles
            .binary_search_by(|entry| entry.identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| self.item_use_profiles[index].profile.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_item_inventory_requires_explicit_unresolved_entries() {
        let mut compiled =
            crate::carriers::startup::test_carriers::synthetic_entity_assets_for_item_use();
        compiled.item_use_profiles = compile(&compiled.item_visuals);
        let (source_bytes, source_sha256) = source_identity();
        let mut sources = compiled.sources.into_vec();
        sources.push(super::super::EntityAssetSource {
            path: SOURCE_PATH.into(),
            source_bytes,
            source_sha256,
        });
        compiled.sources = sources.into();
        validate(&compiled).unwrap();
        let unknown = compiled
            .item_use_profiles
            .iter()
            .find(|entry| entry.identifier.as_ref() == "fixture:unadmitted")
            .unwrap();
        assert_eq!(unknown.profile, ItemUseProfile::Unresolved);
        let blob = crate::encode_entity_blob(&compiled).unwrap();
        assert_eq!(
            RuntimeEntityAssets::decode(&blob)
                .unwrap()
                .item_use_profile("fixture:unadmitted"),
            ItemUseProfile::Unresolved
        );
        let mut omitted = compiled.clone();
        omitted.item_use_profiles = omitted
            .item_use_profiles
            .into_vec()
            .into_iter()
            .filter(|entry| entry.identifier != unknown.identifier)
            .collect();
        assert!(validate(&omitted).is_err());
        let mut forged = compiled.clone();
        forged
            .item_use_profiles
            .iter_mut()
            .find(|entry| entry.identifier.as_ref() == "fixture:unadmitted")
            .unwrap()
            .profile = diagnostic_profile("minecraft:bread");
        assert!(validate(&forged).is_err());
        let mut changed_source = compiled;
        changed_source.sources.last_mut().unwrap().source_sha256[0] ^= 1;
        assert!(validate(&changed_source).is_err());
    }

    #[test]
    fn source_free_inventory_cannot_omit_unknown_items() {
        let mut compiled =
            crate::carriers::startup::test_carriers::synthetic_entity_assets_for_item_use();
        validate(&compiled).unwrap();
        compiled.item_use_profiles = Box::from([]);
        assert!(validate(&compiled).is_err());
    }
}
