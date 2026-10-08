//! Publishes dropped items, falling blocks, primed TNT and ropes as renderer geometry.
use std::{collections::HashMap, sync::Arc};

use assets::{ItemVisualRoute, NetworkIdMode, RuntimeAssets, VisualKind};
use bevy::{
    ecs::system::SystemParam,
    prelude::{Local, Res, ResMut},
};
use chunk_pipeline::WorldStream;
use client_world::{BlockEntityKind, RopeKind};
use render::{
    ChunkTextureAssets, DroppedItemInstance, DroppedItemModel, DroppedItemScene, DroppedItemShape,
    DroppedItemSpawnPose, ItemMeshVertex, MAX_ITEM_LAYERS, MAX_ITEM_SPRITE_SIDE,
    StaticItemPlacements, TerrainItemInstance, TerrainItemTransition, dropped_item_transform,
    native_dropped_item_transform, pack_overlay_rgba8, rope_color, rope_ribbon,
};
use render_model::{DroppedItemSprite, dropped_item_block_cube, dropped_item_block_model};

use client_ui::ui_runtime::presentation::UiPresentationRuntime;

mod lighting;

// Provisional world sizes and colours; each needs independent measurement.
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
    Icon(Arc<str>, u32, bool),
    Block { hashed: bool, id: u32 },
    CarriedBlock { hashed: bool, id: u32 },
}

/// Models resolved so far; failed block lookups are remembered so they are not rebuilt per frame.
#[derive(Default)]
pub(super) struct ModelCache {
    session: u64,
    assets: Option<render::ChunkTextureAssetIdentity>,
    /// Sprite models copy icon pixels, so a pack reload's new icons invalidate them.
    icons: u64,
    revision: u64,
    models: Vec<DroppedItemModel>,
    layers: usize,
    shared: Arc<[DroppedItemModel]>,
    index: HashMap<ModelKey, Option<u32>>,
    /// Native first-render capture, keyed by exact actor lifetime, not mutable item identity.
    spawn_poses: HashMap<(u64, u64), DroppedItemSpawnPose>,
}

impl ModelCache {
    /// Drops every model built from another session, block texture set or icon generation.
    fn sync(
        &mut self,
        session: u64,
        assets: Option<render::ChunkTextureAssetIdentity>,
        icons: u64,
    ) {
        if self.session != session || self.assets != assets || self.icons != icons {
            *self = Self {
                session,
                assets,
                icons,
                revision: self.revision.wrapping_add(1),
                ..Self::default()
            };
        }
    }

    fn insert(&mut self, key: ModelKey, model: Option<DroppedItemModel>) -> Option<u32> {
        let cost = match &model {
            Some(DroppedItemModel::Cube(_)) => 6,
            Some(DroppedItemModel::Block(block)) => block.materials.len(),
            Some(DroppedItemModel::Sprite(_) | DroppedItemModel::NativeSprite(_)) => 1,
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

fn item_block_id(stream: &WorldStream, visual: ItemVisualRoute) -> Option<(NetworkIdMode, u32)> {
    match visual {
        ItemVisualRoute::BlockItem(id) => Some((NetworkIdMode::Sequential, id.0)),
        ItemVisualRoute::RetainedBlock { block_runtime_id } => Some((
            stream.network_id_mode(),
            stream.resolve_block_network_id(u32::from_ne_bytes(block_runtime_id.to_ne_bytes())),
        )),
        _ => None,
    }
}

fn entity_block_id(
    stream: &WorldStream,
    kind: &BlockEntityKind,
) -> Option<(NetworkIdMode, u32, f32)> {
    match kind {
        BlockEntityKind::Falling { block_runtime_id } => Some((
            stream.network_id_mode(),
            stream.resolve_block_network_id(u32::from_ne_bytes(block_runtime_id.to_ne_bytes())),
            1.0,
        )),
        BlockEntityKind::PrimedTnt { visual } => match visual {
            ItemVisualRoute::BlockItem(id) => Some((NetworkIdMode::Sequential, id.0, 1.0)),
            _ => None,
        },
    }
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
        let model = dropped_item_block_cube(assets, mode, id, MAX_ITEM_SPRITE_SIDE)
            .map(DroppedItemModel::Cube)
            .or_else(|| {
                dropped_item_block_model(assets, mode, id, MAX_ITEM_SPRITE_SIDE)
                    .map(DroppedItemModel::Block)
            });
        cache.insert(key, model)
    }

    fn icon_model(
        cache: &mut ModelCache,
        icons: &UiPresentationRuntime,
        identifier: &str,
        metadata: u32,
        native_drop: bool,
    ) -> Option<u32> {
        let key = ModelKey::Icon(Arc::from(identifier), metadata, native_drop);
        if let Some(cached) = cache.index.get(&key) {
            return *cached;
        }
        // An icon that is not ready yet is retried next frame rather than cached as missing.
        let pixels = icons.item_sprite(identifier, metadata, MAX_ITEM_SPRITE_SIDE)?;
        let sprite = DroppedItemSprite {
            width: pixels.width,
            height: pixels.height,
            rgba8: Arc::from(pixels.rgba8),
        };
        let model = if native_drop {
            DroppedItemModel::NativeSprite(sprite)
        } else {
            DroppedItemModel::Sprite(sprite)
        };
        cache.insert(key, Some(model))
    }

    fn carried_block_model(
        cache: &mut ModelCache,
        icons: &UiPresentationRuntime,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
        id: u32,
    ) -> Option<u32> {
        let key = ModelKey::CarriedBlock {
            hashed: mode == NetworkIdMode::Hashed,
            id,
        };
        if let Some(cached) = cache.index.get(&key) {
            return *cached;
        }
        let block = assets.resolve(mode, id);
        if !block.is_known() || block.kind() != VisualKind::Cube {
            return None;
        }
        let visual = match mode {
            NetworkIdMode::Sequential => id,
            NetworkIdMode::Hashed => assets.sequential_id_for_hash(id)?,
        };
        let cube = icons.carried_block_cube(visual, assets.provenance().source_manifest_sha256)?;
        cache.insert(key, Some(DroppedItemModel::Cube(cube)))
    }

    pub(super) fn publish(
        &mut self,
        stream: Option<&WorldStream>,
        collisions: Option<&dyn crate::observations::CollisionLookup>,
        camera: Option<([f32; 3], f32)>,
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
        cache.sync(
            stream.authority().actor_session_id(),
            self.textures.as_ref().map(|textures| textures.identity()),
            icons.session_icon_generation(),
        );
        let assets = self.textures.as_ref().map(|textures| textures.assets());
        let mut instances = Vec::new();
        let mut terrain_instances = Vec::new();

        let dropped = stream.authority().dropped_items(partial_tick);
        let live = dropped
            .iter()
            .map(|view| (view.runtime_id, view.spawn_revision))
            .collect::<std::collections::HashSet<_>>();
        cache
            .spawn_poses
            .retain(|lifetime, _| live.contains(lifetime));
        for view in dropped {
            let Some(identifier) = view.item.identifier.as_ref() else {
                continue;
            };
            let block_id = item_block_id(stream, view.item.visual);
            let cube = block_id.zip(assets).and_then(|((mode, id), assets)| {
                Self::carried_block_model(cache, icons, assets, mode, id)
                    .or_else(|| Self::block_model(cache, assets, mode, id))
            });
            let (model, shape) = match cube {
                Some(model) => (model, DroppedItemShape::Cube),
                None => {
                    let (icon_identifier, variant) = UiPresentationRuntime::item_icon_key(
                        identifier,
                        view.item.identity.metadata,
                        view.item.charged_projectile.as_deref(),
                        None,
                    );
                    let Some(model) =
                        Self::icon_model(cache, icons, icon_identifier, variant, true)
                    else {
                        continue;
                    };
                    (model, DroppedItemShape::Sprite)
                }
            };
            let pose = *cache
                .spawn_poses
                .entry((view.runtime_id, view.spawn_revision))
                .or_insert_with(|| {
                    DroppedItemSpawnPose::new(
                        stream
                            .authority()
                            .actor(view.runtime_id)
                            .map_or(view.position, |actor| {
                                let mut native_origin = actor.position;
                                native_origin[1] += protocol::ITEM_ACTOR_NETWORK_OFFSET;
                                native_origin
                            }),
                        camera,
                    )
                });
            let mut position = view.position;
            position[1] += pose.bob(view.age_ticks, view.bob_phase, shape) * view.bob_multiplier;
            let yaw = pose.yaw(view.yaw_radians);
            let (block_level, sky_level) = stream.light_level_at(position);
            for offset in view.copy_offsets.iter().take(usize::from(view.copy_count)) {
                instances.push(DroppedItemInstance {
                    model,
                    world_from_item: native_dropped_item_transform(
                        position,
                        yaw,
                        *offset,
                        view.render_scale,
                        shape,
                    ),
                    block_level: u32::from(block_level),
                    sky_level: u32::from(sky_level),
                    overlay_rgba8: 0,
                });
            }
        }

        // Items held by block entities (item frames, campfires) use the same sprite path.
        if let Some(placements) = self.placements.as_ref() {
            for placement in &placements.0 {
                let Some(model) = Self::icon_model(
                    cache,
                    icons,
                    &placement.identifier,
                    placement.metadata,
                    false,
                ) else {
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
            let mut transitions: HashMap<i64, Vec<TerrainItemTransition>> = HashMap::new();
            for fence in stream.actor_block_sync_fences() {
                transitions
                    .entry(fence.sync.actor_unique_id)
                    .or_default()
                    .push(TerrainItemTransition {
                        key: fence.key,
                        generation: fence.generation,
                        visible: fence.sync.message == 1,
                    });
            }
            for candidate in stream.authority().block_entity_candidates(partial_tick) {
                let view = candidate.view;
                let Some((id_mode, id, base_scale)) = entity_block_id(stream, &view.kind) else {
                    continue;
                };
                let Some(model) = Self::block_model(cache, assets, id_mode, id) else {
                    continue;
                };
                let light_position = lighting::block_entity_light_position(
                    stream,
                    collisions,
                    &view.kind,
                    view.center,
                );
                let (block_level, sky_level) = stream.light_level_at(light_position);
                let instance = DroppedItemInstance {
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
                };
                if matches!(view.kind, BlockEntityKind::Falling { .. }) {
                    terrain_instances.push(TerrainItemInstance {
                        instance,
                        visible: candidate.visible,
                        transitions: Arc::from(
                            transitions.remove(&candidate.unique_id).unwrap_or_default(),
                        ),
                    });
                } else if candidate.visible {
                    instances.push(instance);
                }
            }
        }

        let mut lines: Vec<ItemMeshVertex> = Vec::new();
        if let Some((camera, _)) = camera {
            for rope in stream.authority().ropes(partial_tick) {
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
        scene.publish_terrain_instances(stream.authority().actor_session_id(), &terrain_instances);
    }
}

#[cfg(test)]
#[path = "dropped_items/tests.rs"]
mod palette_tests;

#[cfg(test)]
mod tests {
    use super::*;

    // An item-only pack reload keeps session and block textures but replaces icon pixels.
    #[test]
    fn new_session_icons_drop_cached_sprite_models() {
        let mut cache = ModelCache::default();
        cache.sync(1, None, 0);
        let sprite = DroppedItemSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([0; 4]),
        };
        let key = ModelKey::Icon(Arc::from("minecraft:apple"), 0, true);
        cache.insert(key.clone(), Some(DroppedItemModel::NativeSprite(sprite)));
        cache.sync(1, None, 0);
        assert!(cache.index.contains_key(&key));
        cache.sync(1, None, 1);
        assert!(!cache.index.contains_key(&key) && cache.models.is_empty());
    }
}
