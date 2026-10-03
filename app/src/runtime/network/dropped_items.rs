//! Publishes dropped items, falling blocks, primed TNT and ropes as renderer geometry.
use std::{collections::HashMap, sync::Arc};

use assets::{ItemVisualRoute, NetworkIdMode, RuntimeAssets};
use bevy::{
    ecs::system::SystemParam,
    prelude::{Local, Res, ResMut},
};
use client_world::{BlockEntityKind, RopeKind, WorldStream};
use render::{
    ChunkTextureAssets, DroppedItemInstance, DroppedItemModel, DroppedItemScene, DroppedItemSprite,
    ItemMeshVertex, MAX_ITEM_LAYERS, MAX_ITEM_SPRITE_SIDE, StaticItemPlacements,
    dropped_block_cube, dropped_item_transform, pack_overlay_rgba8, rope_color, rope_ribbon,
};

use crate::ui_runtime::presentation::UiPresentationRuntime;

// Provisional world sizes and colours; each needs independent measurement.
const SPRITE_WORLD_SCALE: f32 = 0.5;
const DROPPED_BLOCK_SCALE: f32 = 0.25;
const FALLING_BLOCK_SCALE: f32 = 0.98;
const TNT_FLASH_OVERLAY: [f32; 4] = [1.0, 1.0, 1.0, 0.8];
const FISHING_SEGMENTS: usize = 16;
const FISHING_SAG_FRACTION: f32 = 0.1;
const FISHING_HALF_WIDTH: f32 = 0.006;
const LEAD_SEGMENTS: usize = 16;
const LEAD_SAG_FRACTION: f32 = 0.12;
const LEAD_HALF_WIDTH: f32 = 0.0125;
/// Daylight scale until the celestial curve feeds world-space actors.
pub(super) const DAYLIGHT: f32 = 1.0;

#[derive(Clone, PartialEq, Eq, Hash)]
enum ModelKey {
    Icon(Arc<str>, u32),
    Block { hashed: bool, id: u32 },
}

/// Models resolved so far; failed block lookups are remembered so they are not rebuilt per frame.
#[derive(Default)]
pub(super) struct ModelCache {
    session: u64,
    assets: Option<render::ChunkTextureAssetIdentity>,
    revision: u64,
    models: Vec<DroppedItemModel>,
    layers: usize,
    shared: Arc<[DroppedItemModel]>,
    index: HashMap<ModelKey, Option<u32>>,
}

impl ModelCache {
    fn insert(&mut self, key: ModelKey, model: Option<DroppedItemModel>) -> Option<u32> {
        let cost = match &model {
            Some(DroppedItemModel::Cube(_)) => 6,
            Some(DroppedItemModel::Sprite(_)) => 1,
            None => 0,
        };
        let index = model.and_then(|model| {
            (self.layers + cost < MAX_ITEM_LAYERS).then(|| {
                self.layers += cost;
                self.models.push(model);
                self.shared = Arc::from(self.models.as_slice());
                self.revision = self.revision.wrapping_add(1);
                (self.models.len() - 1) as u32
            })
        });
        self.index.insert(key, index);
        index
    }
}

#[derive(SystemParam)]
pub(super) struct DroppedItemPublisher<'w, 's> {
    scene: Option<ResMut<'w, DroppedItemScene>>,
    icons: Option<Res<'w, UiPresentationRuntime>>,
    textures: Option<Res<'w, ChunkTextureAssets>>,
    placements: Option<Res<'w, StaticItemPlacements>>,
    cache: Local<'s, ModelCache>,
}

impl DroppedItemPublisher<'_, '_> {
    fn block_model(
        cache: &mut ModelCache,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
        id: u32,
    ) -> Option<u32> {
        let key = ModelKey::Block {
            hashed: mode == NetworkIdMode::Hashed,
            id,
        };
        if let Some(cached) = cache.index.get(&key) {
            return *cached;
        }
        let model = dropped_block_cube(assets, mode, id).map(DroppedItemModel::Cube);
        cache.insert(key, model)
    }

    fn icon_model(
        cache: &mut ModelCache,
        icons: &UiPresentationRuntime,
        identifier: &Arc<str>,
        metadata: u32,
    ) -> Option<u32> {
        let key = ModelKey::Icon(Arc::clone(identifier), metadata);
        if let Some(cached) = cache.index.get(&key) {
            return *cached;
        }
        // An icon that is not ready yet is retried next frame rather than cached as missing.
        let pixels = icons.item_sprite(identifier, metadata, MAX_ITEM_SPRITE_SIDE)?;
        let model = DroppedItemModel::Sprite(DroppedItemSprite {
            width: pixels.width,
            height: pixels.height,
            rgba8: Arc::from(pixels.rgba8),
        });
        cache.insert(key, Some(model))
    }

    pub(super) fn publish(
        &mut self,
        stream: Option<&WorldStream>,
        camera: Option<[f32; 3]>,
        partial_tick: f32,
    ) {
        let (Some(scene), Some(icons)) = (self.scene.as_mut(), self.icons.as_ref()) else {
            return;
        };
        let Some(stream) = stream else {
            scene.clear();
            return;
        };
        let cache = &mut *self.cache;
        let assets_identity = self.textures.as_ref().map(|textures| textures.identity());
        if cache.session != stream.actor_session_id() || cache.assets != assets_identity {
            *cache = ModelCache {
                session: stream.actor_session_id(),
                assets: assets_identity,
                revision: cache.revision.wrapping_add(1),
                ..ModelCache::default()
            };
        }
        let assets = self.textures.as_ref().map(|textures| textures.assets());
        let mode = stream.network_id_mode();
        let mut instances = Vec::new();

        for view in stream.dropped_items(partial_tick) {
            let Some(identifier) = view.item.identifier.as_ref() else {
                continue;
            };
            let block_id = match view.item.visual {
                ItemVisualRoute::BlockItem(id) => Some((NetworkIdMode::Sequential, id.0)),
                ItemVisualRoute::RetainedBlock { block_runtime_id } => {
                    u32::try_from(block_runtime_id).ok().map(|id| (mode, id))
                }
                _ => None,
            };
            let cube = block_id
                .zip(assets)
                .and_then(|((mode, id), assets)| Self::block_model(cache, assets, mode, id));
            let (model, scale) = match cube {
                Some(model) => (model, DROPPED_BLOCK_SCALE),
                None => {
                    let Some(model) =
                        Self::icon_model(cache, icons, identifier, view.item.identity.metadata)
                    else {
                        continue;
                    };
                    (model, SPRITE_WORLD_SCALE)
                }
            };
            let (block_level, sky_level) = stream.light_level_at(view.position);
            for offset in view.copy_offsets.iter().take(usize::from(view.copy_count)) {
                let center = std::array::from_fn(|axis| view.position[axis] + offset[axis]);
                instances.push(DroppedItemInstance {
                    model,
                    world_from_item: dropped_item_transform(center, view.yaw_radians, scale),
                    block_level: u32::from(block_level),
                    sky_level: u32::from(sky_level),
                    overlay_rgba8: 0,
                });
            }
        }

        // Items held by block entities (item frames, campfires) use the same sprite path.
        if let Some(placements) = self.placements.as_ref() {
            for placement in &placements.0 {
                let Some(model) =
                    Self::icon_model(cache, icons, &placement.identifier, placement.metadata)
                else {
                    continue;
                };
                let rows = placement.world_from_item;
                let (block_level, sky_level) = placement
                    .light
                    .unwrap_or_else(|| stream.light_level_at([rows[0][3], rows[1][3], rows[2][3]]));
                instances.push(DroppedItemInstance {
                    model,
                    world_from_item: rows,
                    block_level: u32::from(block_level),
                    sky_level: u32::from(sky_level),
                    overlay_rgba8: 0,
                });
            }
        }

        if let Some(assets) = assets {
            for view in stream.block_entities(partial_tick) {
                let (id_mode, id, base_scale) = match &view.kind {
                    BlockEntityKind::Falling { block_runtime_id } => {
                        let Ok(id) = u32::try_from(*block_runtime_id) else {
                            continue;
                        };
                        (mode, id, FALLING_BLOCK_SCALE)
                    }
                    BlockEntityKind::PrimedTnt { visual } => match visual {
                        ItemVisualRoute::BlockItem(id) => (NetworkIdMode::Sequential, id.0, 1.0),
                        _ => continue,
                    },
                };
                let Some(model) = Self::block_model(cache, assets, id_mode, id) else {
                    continue;
                };
                let (block_level, sky_level) = stream.light_level_at(view.center);
                instances.push(DroppedItemInstance {
                    model,
                    world_from_item: dropped_item_transform(
                        view.center,
                        0.0,
                        base_scale * view.scale,
                    ),
                    block_level: u32::from(block_level),
                    sky_level: u32::from(sky_level),
                    overlay_rgba8: if view.flash {
                        pack_overlay_rgba8(TNT_FLASH_OVERLAY)
                    } else {
                        0
                    },
                });
            }
        }

        let mut lines: Vec<ItemMeshVertex> = Vec::new();
        if let Some(camera) = camera {
            for rope in stream.ropes(partial_tick) {
                let length = rope
                    .from
                    .iter()
                    .zip(&rope.to)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f32>()
                    .sqrt();
                let (segments, sag, half_width, color) = match rope.kind {
                    RopeKind::FishingLine => (
                        FISHING_SEGMENTS,
                        length * FISHING_SAG_FRACTION,
                        FISHING_HALF_WIDTH,
                        rope_color(0, 0, 0),
                    ),
                    RopeKind::Lead => (
                        LEAD_SEGMENTS,
                        length * LEAD_SAG_FRACTION,
                        LEAD_HALF_WIDTH,
                        rope_color(127, 96, 55),
                    ),
                };
                rope_ribbon(
                    rope.from, rope.to, camera, segments, sag, half_width, color, &mut lines,
                );
            }
        }
        scene.publish(
            cache.revision,
            Arc::clone(&cache.shared),
            &instances,
            &lines,
            DAYLIGHT,
        );
    }
}
