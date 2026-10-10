//! Publishes dropped items, falling blocks, primed TNT and ropes as renderer geometry.
use std::{collections::HashMap, sync::Arc};

use assets::{
    BlockFace, ItemVisualRoute, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_WATER_TINT, NetworkIdMode, RuntimeAssets, VisualKind,
};
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
use render_model::{DroppedItemBlock, DroppedItemCube, DroppedItemSprite};

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
const GRASS_TINT_RGB: [u8; 3] = [0x79, 0xc0, 0x5a];
const FOLIAGE_TINT_RGB: [u8; 3] = [0x77, 0xab, 0x2f];
const WATER_TINT_RGB: [u8; 3] = [0x3f, 0x76, 0xe4];
/// Daylight scale until the celestial curve feeds world-space actors.
pub(super) const DAYLIGHT: f32 = 1.0;

#[derive(Clone, PartialEq, Eq, Hash)]
enum ModelKey {
    Block { hashed: bool, id: u32 },
    CarriedBlock { hashed: bool, id: u32 },
}

/// CPU model cache with separately bounded, reclaimable atlas residency.
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
    index: HashMap<ModelKey, usize>,
    icon_index: HashMap<Arc<str>, HashMap<(u32, bool), usize>>,
    records: Vec<ModelRecord>,
    frame: u64,
    dirty: bool,
    /// Native first-render capture, keyed by exact actor lifetime, not mutable item identity.
    spawn_poses: HashMap<(u64, u64), DroppedItemSpawnPose>,
}

/// CPU data survives atlas eviction; a resident slot stays stable while demanded.
struct ModelRecord {
    model: Option<DroppedItemModel>,
    slot: Option<u32>,
    requested: u64,
}

/// Returns the texture layers required by a model, excluding the shared white layer.
fn layer_cost(model: &DroppedItemModel) -> usize {
    match model {
        DroppedItemModel::Vacant => 0,
        DroppedItemModel::Cube(_) => 6,
        DroppedItemModel::Block(block) => block.materials.len(),
        DroppedItemModel::Sprite(_) | DroppedItemModel::NativeSprite(_) => 1,
    }
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

    /// Remembers supported or missing CPU geometry and attempts transient atlas admission.
    fn insert(&mut self, key: ModelKey, model: Option<DroppedItemModel>) -> Option<u32> {
        let record = self.records.len();
        self.records.push(ModelRecord {
            model,
            slot: None,
            requested: self.frame,
        });
        self.index.insert(key, record);
        self.admit(record)
    }

    /// Starts demand collection before any residency decisions are made.
    fn begin_frame(&mut self) {
        self.frame = self
            .frame
            .checked_add(1)
            .expect("model demand frame overflow");
    }

    /// Protects an existing block model from reclamation during this frame.
    fn request(&mut self, key: &ModelKey) {
        if let Some(&record) = self.index.get(key) {
            self.records[record].requested = self.frame;
        }
    }

    /// Looks up an icon using borrowed text without constructing an owned key.
    fn icon_record(&self, identifier: &str, metadata: u32, native: bool) -> Option<usize> {
        self.icon_index
            .get(identifier)?
            .get(&(metadata, native))
            .copied()
    }

    /// Protects an existing sprite model, including both placement routes.
    fn request_icon(&mut self, identifier: &str, metadata: u32, native: bool) {
        if let Some(record) = self.icon_record(identifier, metadata, native) {
            self.records[record].requested = self.frame;
        }
    }

    /// Reclaims only unrequested slots and retries records rejected by capacity previously.
    fn admit(&mut self, record: usize) -> Option<u32> {
        self.records[record].requested = self.frame;
        if let Some(slot) = self.records[record].slot {
            return Some(slot);
        }
        let cost = layer_cost(self.records[record].model.as_ref()?);
        if cost == 0 || cost >= MAX_ITEM_LAYERS {
            return None;
        }
        if self.layers + cost >= MAX_ITEM_LAYERS {
            for entry in &mut self.records {
                if entry.requested == self.frame {
                    continue;
                }
                if let Some(slot) = entry.slot.take() {
                    self.layers -= layer_cost(&self.models[slot as usize]);
                    self.models[slot as usize] = DroppedItemModel::Vacant;
                    self.dirty = true;
                    if self.layers + cost < MAX_ITEM_LAYERS {
                        break;
                    }
                }
            }
        }
        if self.layers + cost >= MAX_ITEM_LAYERS {
            return None;
        }
        let model = self.records[record].model.as_ref()?.clone();
        let slot = if let Some(slot) = self
            .models
            .iter()
            .position(|model| matches!(model, DroppedItemModel::Vacant))
        {
            self.models[slot] = model;
            slot
        } else {
            self.models.push(model);
            self.models.len() - 1
        };
        self.layers += cost;
        self.records[record].slot = Some(slot as u32);
        self.dirty = true;
        Some(slot as u32)
    }

    /// Shares the resident table once after all admissions, rather than after each model.
    fn finish_frame(&mut self) {
        if self.dirty {
            self.shared = Arc::from(self.models.as_slice());
            self.revision = self.revision.wrapping_add(1);
            self.dirty = false;
        }
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

fn tint_rgba(flags: u32) -> u32 {
    let [r, g, b] = match flags & MATERIAL_FLAG_TINT_MASK {
        MATERIAL_FLAG_GRASS_TINT => GRASS_TINT_RGB,
        MATERIAL_FLAG_FOLIAGE_TINT => FOLIAGE_TINT_RGB,
        MATERIAL_FLAG_WATER_TINT => WATER_TINT_RGB,
        _ => [255; 3],
    };
    rope_color(r, g, b)
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

/// Builds a unit cube from a cube-kind block's six face textures, or `None` for other kinds.
fn block_cube(assets: &RuntimeAssets, mode: NetworkIdMode, id: u32) -> Option<DroppedItemCube> {
    let block = assets.resolve(mode, id);
    if !block.is_known() || block.kind() != VisualKind::Cube {
        return None;
    }
    let mut tile_size = None;
    let mut faces: Vec<Arc<[u8]>> = Vec::with_capacity(6);
    let mut tints = [0_u32; 6];
    for (index, face) in BlockFace::ALL.into_iter().enumerate() {
        let material = assets.material(block.face(face).material_id());
        let page = assets
            .texture_pages()
            .get(material.texture.page() as usize)?;
        let mip = page.texture.mips.first()?;
        let size = mip.size;
        if size == 0 || size > MAX_ITEM_SPRITE_SIDE || *tile_size.get_or_insert(size) != size {
            return None;
        }
        let bytes = (size * size * 4) as usize;
        let start = material.texture.layer() as usize * bytes;
        faces.push(Arc::from(mip.rgba8.get(start..start + bytes)?));
        tints[index] = tint_rgba(material.flags);
    }
    Some(DroppedItemCube {
        tile: tile_size?,
        faces: faces.try_into().ok()?,
        tints,
    })
}

fn block_template(
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    id: u32,
) -> Option<DroppedItemBlock> {
    let block = assets.resolve(mode, id);
    if !block.is_known() || !matches!(block.kind(), VisualKind::Model | VisualKind::Cross) {
        return None;
    }
    let mut template_id = block.model_template()?;
    let mut quads = Vec::new();
    let mut materials = Vec::new();
    let mut material_indices = HashMap::new();
    loop {
        let template = assets.model_templates().get(template_id as usize)?;
        let first = template.quad_start as usize;
        for quad in assets
            .model_quads()
            .get(first..first + template.quad_count as usize)?
        {
            let mut quad = *quad;
            let material_index = if let Some(index) = material_indices.get(&quad.material) {
                *index
            } else {
                let material = assets.material(quad.material);
                let page = assets
                    .texture_pages()
                    .get(material.texture.page() as usize)?;
                let mip = page.texture.mips.first()?;
                let size = mip.size;
                if size == 0 || size > MAX_ITEM_SPRITE_SIDE {
                    return None;
                }
                let bytes = (size * size * 4) as usize;
                let first = material.texture.layer() as usize * bytes;
                let index = materials.len() as u32;
                materials.push((
                    DroppedItemSprite {
                        width: size,
                        height: size,
                        rgba8: Arc::from(mip.rgba8.get(first..first + bytes)?),
                    },
                    tint_rgba(material.flags),
                ));
                material_indices.insert(quad.material, index);
                index
            };
            quad.material = material_index;
            quads.push(quad);
        }
        if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            break;
        }
        template_id = template_id.checked_add(1)?;
    }
    Some(DroppedItemBlock {
        materials: materials.into(),
        quads: quads.into(),
        rotation: block.variant() & 3,
    })
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
        if let Some(&record) = cache.index.get(&key) {
            return cache.admit(record);
        }
        let model = block_cube(assets, mode, id)
            .map(DroppedItemModel::Cube)
            .or_else(|| block_template(assets, mode, id).map(DroppedItemModel::Block));
        cache.insert(key, model)
    }

    fn icon_model(
        cache: &mut ModelCache,
        icons: &UiPresentationRuntime,
        identifier: &str,
        metadata: u32,
        native_drop: bool,
    ) -> Option<u32> {
        if let Some(record) = cache.icon_record(identifier, metadata, native_drop) {
            return cache.admit(record);
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
        let record = cache.records.len();
        cache.records.push(ModelRecord {
            model: Some(model),
            slot: None,
            requested: cache.frame,
        });
        cache
            .icon_index
            .entry(Arc::from(identifier))
            .or_default()
            .insert((metadata, native_drop), record);
        cache.admit(record)
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
        if let Some(&record) = cache.index.get(&key) {
            return cache.admit(record);
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
        cache.begin_frame();
        // Protect all current consumers before admitting new models, regardless of iteration order.
        for view in &dropped {
            if let Some((mode, id)) = item_block_id(stream, view.item.visual) {
                let hashed = mode == NetworkIdMode::Hashed;
                cache.request(&ModelKey::CarriedBlock { hashed, id });
                cache.request(&ModelKey::Block { hashed, id });
            }
            if let Some(identifier) = view.item.identifier.as_ref() {
                let (identifier, metadata) = UiPresentationRuntime::item_icon_key(
                    identifier,
                    view.item.identity.metadata,
                    view.item.charged_projectile.as_deref(),
                    None,
                );
                cache.request_icon(identifier, metadata, true);
            }
        }
        if let Some(placements) = self.placements.as_ref() {
            for placement in &placements.0 {
                cache.request_icon(&placement.identifier, placement.metadata, false);
            }
        }
        let candidates = stream.authority().block_entity_candidates(partial_tick);
        for candidate in &candidates {
            if let Some((mode, id, _)) = entity_block_id(stream, &candidate.view.kind) {
                cache.request(&ModelKey::Block {
                    hashed: mode == NetworkIdMode::Hashed,
                    id,
                });
            }
        }
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
            for candidate in candidates {
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
        cache.finish_frame();
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
        let key = ModelKey::Block {
            hashed: false,
            id: 1,
        };
        cache.insert(key.clone(), Some(DroppedItemModel::NativeSprite(sprite)));
        cache.sync(1, None, 0);
        assert!(cache.index.contains_key(&key));
        cache.sync(1, None, 1);
        assert!(!cache.index.contains_key(&key) && cache.models.is_empty());
    }
}

#[cfg(test)]
mod residency_tests {
    use super::*;

    /// Creates one independently keyed, one-layer model for atlas pressure tests.
    fn sprite() -> DroppedItemModel {
        DroppedItemModel::Sprite(DroppedItemSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([255; 4]),
        })
    }

    /// Makes a block-route key without requiring local asset carriers.
    fn key(id: u32) -> ModelKey {
        ModelKey::Block { hashed: false, id }
    }

    #[test]
    fn atlas_reclaims_despawned_models_and_keeps_live_handles() {
        let mut cache = ModelCache::default();
        cache.begin_frame();
        for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
            assert_eq!(cache.insert(key(id), Some(sprite())), Some(id));
        }
        let delayed = key(MAX_ITEM_LAYERS as u32);
        assert_eq!(cache.insert(delayed.clone(), Some(sprite())), None);
        let delayed_record = cache.index[&delayed];
        cache.begin_frame();
        cache.request(&key(0));
        cache.request(&key(100));
        let live = [
            cache.admit(cache.index[&key(0)]),
            cache.admit(cache.index[&key(100)]),
        ];
        assert!(cache.admit(delayed_record).is_some());
        assert_eq!(live, [Some(0), Some(100)]);
        assert_eq!(cache.admit(cache.index[&key(100)]), Some(100));
        assert!(cache.layers < MAX_ITEM_LAYERS);
        cache.finish_frame();
        assert_eq!(cache.shared.len(), MAX_ITEM_LAYERS - 1);
        assert!(!matches!(cache.shared[100], DroppedItemModel::Vacant));
    }

    #[test]
    fn evicted_cpu_geometry_can_be_readmitted_without_rebuilding() {
        let mut cache = ModelCache::default();
        cache.begin_frame();
        for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
            cache.insert(key(id), Some(sprite()));
        }
        cache.begin_frame();
        let replacement = cache.insert(key(900), Some(sprite())).unwrap();
        assert_eq!(replacement, 0);
        assert!(cache.records[0].model.is_some());
        cache.begin_frame();
        let readmitted = cache.admit(0).expect("evicted CPU geometry is retried");
        let DroppedItemModel::Sprite(resident) = &cache.models[readmitted as usize] else {
            panic!("readmission must preserve the model kind");
        };
        let Some(DroppedItemModel::Sprite(cpu)) = &cache.records[0].model else {
            panic!("CPU geometry must survive eviction");
        };
        assert!(Arc::ptr_eq(&resident.rgba8, &cpu.rgba8));
    }

    #[test]
    fn borrowed_warm_icon_lookup_and_admission_do_not_allocate() {
        let mut cache = ModelCache::default();
        cache.records.push(ModelRecord {
            model: Some(sprite()),
            slot: None,
            requested: 0,
        });
        cache
            .icon_index
            .entry(Arc::from("custom:warm"))
            .or_default()
            .insert((7, true), 0);
        assert_eq!(cache.admit(0), Some(0));
        cache.finish_frame();
        let before = crate::test_allocations::count();
        for _ in 0..100 {
            cache.request_icon("custom:warm", 7, true);
            let record = cache.icon_record("custom:warm", 7, true).unwrap();
            assert_eq!(cache.admit(record), Some(0));
        }
        assert_eq!(crate::test_allocations::count() - before, 0);
        assert_eq!(cache.icon_record("custom:warm", 7, false), None);
        assert_eq!(cache.icon_record("custom:warm", 8, true), None);
    }
}

#[cfg(test)]
mod mixed_layer_tests {
    use super::*;

    #[test]
    fn cube_admission_reclaims_six_layers_without_moving_a_live_sprite() {
        let mut cache = ModelCache::default();
        cache.begin_frame();
        let sprite = DroppedItemModel::Sprite(DroppedItemSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([255; 4]),
        });
        for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
            cache.insert(ModelKey::Block { hashed: false, id }, Some(sprite.clone()));
        }
        cache.begin_frame();
        let live_key = ModelKey::Block {
            hashed: false,
            id: 50,
        };
        cache.request(&live_key);
        let cube = DroppedItemModel::Cube(DroppedItemCube {
            tile: 1,
            faces: std::array::from_fn(|_| Arc::from([255; 4])),
            tints: [0; 6],
        });
        assert!(
            cache
                .insert(
                    ModelKey::Block {
                        hashed: false,
                        id: 900
                    },
                    Some(cube)
                )
                .is_some()
        );
        assert_eq!(cache.admit(cache.index[&live_key]), Some(50));
        assert_eq!(cache.layers, MAX_ITEM_LAYERS - 1);
        assert_eq!(
            cache
                .models
                .iter()
                .filter(|model| matches!(model, DroppedItemModel::Vacant))
                .count(),
            5
        );
    }
}
