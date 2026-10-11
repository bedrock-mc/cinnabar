//! Mob models for spawner cages, read from the entity and actor catalogs without touching
//! the actor renderer.
//!
//! Each model is the mob's geometry in its rest pose; textures are appended to the block-entity
//! atlas. Which mobs are listed and their scale need native measurement.

use std::{collections::BTreeMap, sync::Arc};

use assets::{EntityAssetKind, RuntimeActorCatalog, RuntimeEntityAssets};

use super::heads::HeadModel;

/// Mobs a spawner can show; others draw an empty cage.
pub const SPAWNER_MOBS: &[&str] = &[
    "minecraft:zombie",
    "minecraft:skeleton",
    "minecraft:spider",
    "minecraft:cave_spider",
    "minecraft:blaze",
    "minecraft:silverfish",
    "minecraft:creeper",
    "minecraft:husk",
    "minecraft:stray",
    "minecraft:drowned",
    "minecraft:enderman",
    "minecraft:pig",
];

/// A texture to append to the atlas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MobTexture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MobModel {
    pub model: HeadModel,
    /// Atlas name of the mob's texture.
    pub texture: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MobModels {
    models: BTreeMap<Box<str>, MobModel>,
    textures: Vec<MobTexture>,
}

impl MobModels {
    #[must_use]
    pub fn from_assets(entities: &RuntimeEntityAssets, catalog: &RuntimeActorCatalog) -> Self {
        let mut result = Self::default();
        for identifier in SPAWNER_MOBS {
            let Some((model, texture)) = build_one(entities, catalog, identifier) else {
                continue;
            };
            let name = format!("mob/{identifier}");
            result.textures.push(MobTexture {
                name: name.clone(),
                ..texture
            });
            result.models.insert(
                (*identifier).into(),
                MobModel {
                    model,
                    texture: name,
                },
            );
        }
        result
    }

    #[must_use]
    pub fn get(&self, identifier: &str) -> Option<&MobModel> {
        self.models.get(identifier)
    }

    #[must_use]
    pub fn textures(&self) -> &[MobTexture] {
        &self.textures
    }
}

fn build_one(
    entities: &RuntimeEntityAssets,
    catalog: &RuntimeActorCatalog,
    identifier: &str,
) -> Option<(HeadModel, MobTexture)> {
    let symbol = entities
        .symbol_candidates(EntityAssetKind::Entity, identifier)
        .first()?;
    let symbol_index = entities
        .symbols()
        .iter()
        .position(|candidate| std::ptr::eq(candidate, symbol))?;
    let rig = entities
        .rig_bindings()
        .iter()
        .position(|binding| binding.entity_symbol as usize == symbol_index)?;
    let binding = catalog.binding(u32::try_from(rig).ok()?)?;
    let geometry = entities.geometries().get(binding.geometry as usize)?;
    let texture = catalog.textures().get(binding.texture as usize)?;
    Some((
        HeadModel::build_tree(geometry, None, 1.0, false)?,
        MobTexture {
            name: String::new(),
            width: u32::from(texture.width),
            height: u32::from(texture.height),
            rgba8: Arc::clone(&texture.rgba8),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawner_mob_ids_are_unique_and_namespaced() {
        let mut sorted = SPAWNER_MOBS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), SPAWNER_MOBS.len());
        assert!(SPAWNER_MOBS.iter().all(|id| id.starts_with("minecraft:")));
    }

    #[test]
    fn empty_catalogs_yield_no_models() {
        assert!(MobModels::default().get("minecraft:zombie").is_none());
        assert!(MobModels::default().textures().is_empty());
    }
}
