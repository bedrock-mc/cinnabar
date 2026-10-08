//! Dropped inventory sprites retain the native extruded mesh and artwork path.
use crate::browser_model::Frame;
use assets::RuntimeIconCatalog;
use render::{DroppedItemInstance, DroppedItemModel, DroppedItemScene};
use std::{collections::BTreeMap, sync::Arc};
use render_model::DroppedItemSprite;

pub(super) struct BrowserItems {
    icons: RuntimeIconCatalog,
    keys: BTreeMap<(String, i32, String), Option<u32>>,
    terrain: Arc<assets::RuntimeAssets>,
    canonical: Arc<crate::canonical::CanonicalIndex>,
    models: Vec<DroppedItemModel>,
    shared: Arc<[DroppedItemModel]>,
    revision: u64,
    model_layers: usize,
}
impl BrowserItems {
    pub(super) fn new(
        bytes: &[u8],
        terrain: &crate::textured_assets::TerrainAssets,
    ) -> Result<Self, String> {
        Ok(Self {
            icons: RuntimeIconCatalog::decode(bytes).map_err(|error| error.to_string())?,
            keys: BTreeMap::new(),
            terrain: Arc::clone(&terrain.runtime),
            canonical: Arc::clone(&terrain.canonical),
            models: Vec::new(),
            shared: Arc::from([]),
            revision: 1,
            model_layers: 0,
        })
    }
    pub(super) fn update(
        &mut self,
        current: &Frame,
        previous: Option<&Frame>,
        partial: f32,
        output: &mut DroppedItemScene,
    ) {
        let mut instances = Vec::new();
        for entity in &current.entities {
            let Some(item) = entity
                .item
                .as_ref()
                .filter(|_| entity.kind == "minecraft:item")
            else {
                continue;
            };
            let state_key = item
                .block
                .as_ref()
                .map(|block| {
                    serde_json::to_string(&(&block.name, &block.states)).unwrap_or_default()
                })
                .unwrap_or_default();
            let key = (item.name.clone(), item.meta, state_key);
            if self.keys.len() >= 1024 && !self.keys.contains_key(&key) {
                continue;
            }
            let model = if let Some(model) = self.keys.get(&key) {
                *model
            } else {
                let block_model = item.block.as_ref().and_then(|block| {
                    let entry = crate::model::PaletteEntry {
                        name: block.name.clone(),
                        states: block.states.clone(),
                    };
                    let id = crate::canonical::palette_ids(&self.canonical, &[entry])
                        .ok()?
                        .first()
                        .copied()?;
                    render_model::dropped_item_block_cube(&self.terrain, assets::NetworkIdMode::Sequential, id, render::MAX_ITEM_SPRITE_SIDE)
                        .map(|cube| (DroppedItemModel::Cube(cube), 6))
                        .or_else(|| render_model::dropped_item_block_model(&self.terrain, assets::NetworkIdMode::Sequential, id, render::MAX_ITEM_SPRITE_SIDE)
                            .map(|model| { let layers = model.materials.len(); (DroppedItemModel::Block(model), layers) }))
                });
                let model = if let Some((block, layers)) = block_model.filter(|(_, layers)| {
                    self.models.len() < 128 && self.model_layers + layers <= render::MAX_ITEM_LAYERS
                }) {
                    let index = self.models.len() as u32;
                    self.models.push(block);
                    self.model_layers += layers;
                    self.shared = Arc::from(self.models.as_slice());
                    self.revision = self.revision.wrapping_add(1);
                    Some(index)
                } else {
                    self.icons
                        .lookup_index(&item.name, item.meta.max(0) as u32)
                        .and_then(|index| self.icons.sprites().get(index))
                        .filter(|sprite| {
                            sprite.width <= render::MAX_ITEM_SPRITE_SIDE as u16
                                && sprite.height <= render::MAX_ITEM_SPRITE_SIDE as u16
                        })
                        .filter(|_| {
                            self.models.len() < 128
                                && self.model_layers + 1 < render::MAX_ITEM_LAYERS
                        })
                        .map(|sprite| {
                            let index = self.models.len() as u32;
                            self.model_layers += 1;
                            self.models
                                .push(DroppedItemModel::Sprite(DroppedItemSprite {
                                    width: u32::from(sprite.width),
                                    height: u32::from(sprite.height),
                                    rgba8: Arc::clone(&sprite.rgba8),
                                }));
                            self.shared = Arc::from(self.models.as_slice());
                            self.revision = self.revision.wrapping_add(1);
                            index
                        })
                };
                self.keys.insert(key, model);
                model
            };
            let Some(model) = model else {
                continue;
            };
            let cube_kind = matches!(
                self.models.get(model as usize),
                Some(DroppedItemModel::Cube(_) | DroppedItemModel::Block(_))
            );
            let old = previous
                .and_then(|frame| frame.entities.iter().find(|old| old.id == entity.id))
                .unwrap_or(entity);
            let position = std::array::from_fn(|axis| {
                old.position[axis] + (entity.position[axis] - old.position[axis]) * partial
            });
            instances.push(DroppedItemInstance {
                model,
                world_from_item: render::dropped_item_transform(
                    position,
                    entity.yaw.to_radians(),
                    if cube_kind { 0.25 } else { 0.5 },
                ),
                block_level: 0,
                sky_level: 15,
                overlay_rgba8: 0,
            });
        }
        output.publish(
            self.revision,
            Arc::clone(&self.shared),
            &instances,
            &[],
            1.0,
        );
    }
}
