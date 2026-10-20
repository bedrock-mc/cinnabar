//! Builds equipment rig layers (held sprite meshes and worn armor) for a body submission.

use std::{collections::BTreeMap, sync::Arc};

use assets::{
    ArmorSlot, EntityDependencyResolution, EquipmentCategory, IconSprite, RuntimeAssets,
    RuntimeBlockEntityAssets, RuntimeEntityAssets, RuntimeEquipmentCatalog, RuntimeIconCatalog,
};
use bevy::prelude::Resource;
use render_model::{
    ACTOR_LAYER_BODY, ActorArtworkLocation, ActorArtworkPages, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission, BlockEntityAtlas, EquipmentRaster, SkullKind,
    block_entity::HeldBlockEntity, skull_geometry,
};
use render_model::{
    ActorRigGeometry, EntityRigId, RenderBoneTransform, equipment_rig_id, find_geometry_index,
    geometry_bone_binding_expressions, geometry_bone_names, geometry_bone_pivots,
    held_sprite_vertices, item_mesh_rig_id, textured_cube_vertices,
};

mod alpha;
mod attack;
mod block_items;
mod camera;
mod combat;
mod diagnostics;
mod dials;
mod elytra;
mod external_armor;
mod held_block_entities;
mod java;
mod modern;
pub(crate) use modern::HeldSelection;
mod native;
mod pack;
mod placement_cache;
mod push;
mod readiness;
mod routing;
mod session;
mod style;
mod types;
pub use block_items::{BlockItemPreparer, PreparedBlockItems};
pub use java::java_draws_attachable;
pub use pack::PackEquipment;
pub use session::StagedSessionIcons;
pub use types::{
    ActorEquipmentInput, EquipmentAnimation, EquipmentLayers, EquipmentPresentation,
    FirstPersonArms, FirstPersonItem, HeldKind, JavaGrip, WornItem,
};
use types::{ArmorGeometry, AttachableMeshKey, BodyBones, JavaRasterFrame, MeshKey};

use {
    super::{
        armor::{bone_map, hidden_bone, pack_tint, remap_pose},
        atlas::{Placement, SpriteAtlas},
        display::{
            FirstPersonHand, FirstPersonShape, LAYER_BOOTS, LAYER_CHESTPLATE, LAYER_HELMET,
            LAYER_LEGGINGS, LAYER_MAIN_HAND, LAYER_OFF_HAND, head_block_display, view_bone,
        },
    },
    render_model::equipment::{self as attachable, blocks::BlockSheets},
};

fn body_bones(names: Vec<Box<str>>) -> BodyBones {
    let find = |wanted: &str| {
        names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(wanted))
    };
    BodyBones {
        right_arm: find("rightArm"),
        right_item: find("rightItem"),
        left_item: find("leftItem"),
        head: find("head"),
        names,
    }
}

/// Equipment geometry index space reserved for worn skulls (above any entity geometry index).
const SKULL_RIG_INDEX_BASE: u32 = 0x00ff_0000;
/// Reads the shared model ordinal for a packed head rig.
fn kind_index(kind: SkullKind) -> u8 {
    kind.packed_rig_index().unwrap_or(u8::MAX)
}

/// The head an item identifier wears as, for the heads with a packed texture.
pub(super) fn skull_kind(identifier: &str) -> Option<SkullKind> {
    SkullKind::from_block_identifier(identifier).filter(|kind| kind.packed_rig_index().is_some())
}

/// Generated item meshes kept resident; further distinct items draw nothing.
const MAX_ITEM_MESHES: usize = 512;

/// Body bone index per armor bone; `None` where the body lacks it.
type ArmorBoneMap = Arc<[Option<usize>]>;

#[derive(Resource)]
pub struct EquipmentRuntime {
    assets: Arc<RuntimeEntityAssets>,
    catalog: Option<Arc<RuntimeEquipmentCatalog>>,
    icons: Arc<RuntimeIconCatalog>,
    placements: Vec<Option<Placement>>,
    blocks: crate::prepared_actor_artwork::PlacedBlockItems,
    startup_blocks: crate::prepared_actor_artwork::PlacedBlockItems,
    atlas_locations: Vec<Option<ActorArtworkLocation>>,
    texture_locations: BTreeMap<Box<str>, pack::TextureArtwork>,
    body_bones: BTreeMap<u32, Option<Arc<BodyBones>>>,
    /// Bones of player skins' own models, by skin rig id.
    skin_bones: BTreeMap<EntityRigId, Arc<BodyBones>>,
    armor_geometry: BTreeMap<Box<str>, Option<Arc<ArmorGeometry>>>,
    external_armor: external_armor::ExternalArmor,
    armor_maps: BTreeMap<(u32, Box<str>), ArmorBoneMap>,
    meshes: BTreeMap<MeshKey, Option<EntityRigId>>,
    pending: Vec<ActorRigGeometry>,
    /// Worn head geometry and texture location per skull kind.
    skulls: BTreeMap<u8, (EntityRigId, ActorArtworkLocation)>,
    /// Held model rigs, artwork locations and texture-local glint spans.
    held_block_entities: crate::prepared_actor_artwork::PreparedHeldBlockEntities,
    startup_held_block_entities: crate::prepared_actor_artwork::PreparedHeldBlockEntities,
    item_use: Arc<BTreeMap<Box<str>, u32>>,
    use_overlays: Arc<BTreeMap<Box<str>, assets::equipment::CompiledItemUseOverlay>>,
    /// The startup catalog's use durations, before session items join them.
    base_item_use: Arc<BTreeMap<Box<str>, u32>>,
    /// Compiled attack facts with the session's component overrides.
    item_attack: Arc<BTreeMap<Box<str>, protocol::ItemAttackTiming>>,
    base_item_attack: Arc<BTreeMap<Box<str>, protocol::ItemAttackTiming>>,
    /// The session's custom item facts and icon sprites.
    session: session::SessionLayer,
    /// Next startup item mesh index; session icons use their own range.
    next_mesh: u32,
    /// Retired pack slots, reused only after replacement geometry is queued.
    free_meshes: Vec<EntityRigId>,
    /// The session's server-pack attachables, consulted before `catalog`.
    pack: Option<PackEquipment>,
    /// `(identifier, reason)` pairs already logged as drawing no layer.
    logged_misses: std::collections::HashSet<(Box<str>, &'static str)>,
    poses: PoseMemo,
    attachment_poses: placement_cache::PlacementCache,
    /// Reused output storage for one actor, bounded by held and worn layer counts.
    layers: Vec<EquipmentPresentation>,
    attachables: actor_animation::AttachablesRuntime,
    attachable_meshes: BTreeMap<AttachableMeshKey, EntityRigId>,
    native_meshes: BTreeMap<(u16, u32), EntityRigId>,
    /// Raster attachables' image-to-rig frame and rest pose under Java's hand, by mesh key.
    java_rasters: BTreeMap<AttachableMeshKey, Option<JavaRasterFrame>>,
    /// Render-controller models queued beyond the scene's binding geometries.
    selected_geometries: std::collections::BTreeSet<EntityRigId>,
    /// The strip frame each held dial (compass or clock) shows this frame.
    dial_frames: inventory::DialFrames,
}

impl EquipmentRuntime {
    /// Releases memoized poses of layers no recent frame drew.
    pub fn begin_frame(&mut self) {
        self.poses.begin_frame();
        self.attachment_poses.retain(self.poses.frame);
    }

    /// Places the item atlas and attachable textures on new artwork pages. Returns the runtime,
    /// the extended artwork, and the entity-catalog geometries the actor scene must register.
    pub fn build(
        assets: Arc<RuntimeEntityAssets>,
        catalog: Option<Arc<RuntimeEquipmentCatalog>>,
        icons: Arc<RuntimeIconCatalog>,
        world: Option<Arc<RuntimeAssets>>,
        block_entities: Option<Arc<RuntimeBlockEntityAssets>>,
        artwork: ActorArtworkPages,
    ) -> (Self, ActorArtworkPages, Vec<u32>) {
        let blocks = PreparedBlockItems::build(world, &assets, &icons);
        let atlas = SpriteAtlas::pack(icons.sprites());
        let atlas_layers = atlas.layers.len();
        let textures = catalog
            .as_ref()
            .map_or(&[][..], |catalog| catalog.textures());
        let mut rasters = atlas.layers;
        rasters.extend_from_slice(&blocks.rasters);
        let texture_start = rasters.len();
        rasters.extend(textures.iter().map(|texture| EquipmentRaster {
            width: texture.width,
            height: texture.height,
            rgba8: Arc::clone(&texture.rgba8),
        }));
        let skull_atlas = block_entities
            .as_deref()
            .map(BlockEntityAtlas::from_assets)
            .filter(|atlas| {
                let static_bytes = atlas.size()[0] as usize * atlas.static_height() as usize * 4;
                atlas.size()[0] <= u32::from(u16::MAX)
                    && atlas.static_height() <= u32::from(u16::MAX)
                    && atlas.static_rgba8().len() == static_bytes
            });
        let skull_raster_index = skull_atlas.as_ref().map(|atlas| {
            rasters.push(EquipmentRaster {
                width: atlas.size()[0] as u16,
                height: atlas.static_height() as u16,
                rgba8: Arc::clone(atlas.static_rgba8()),
            });
            rasters.len() - 1
        });
        let artwork = match catalog.as_deref().and_then(Self::actor_glint) {
            Some(glint) => artwork.with_actor_glint(glint),
            None => artwork,
        };
        let artwork = match catalog.as_deref().and_then(Self::item_glint) {
            Some(glint) => artwork.with_item_glint(glint),
            None => artwork,
        };
        let (mut artwork, locations) = artwork.with_equipment_rasters(&rasters);
        let texture_locations =
            pack::placed_textures(textures, locations[texture_start..].iter().copied());
        let blocks = crate::prepared_actor_artwork::PlacedBlockItems {
            source: blocks,
            locations: locations[atlas_layers..texture_start].to_vec(),
        };
        let skull_location = skull_raster_index.and_then(|index| locations[index]);
        let mut pending = Vec::new();
        let mut skulls = BTreeMap::new();
        if let (Some(atlas), Some(location)) = (&skull_atlas, skull_location) {
            for kind in SkullKind::packed_kinds() {
                let id = equipment_rig_id(SKULL_RIG_INDEX_BASE + u32::from(kind_index(kind)));
                if let Some(geometry) = skull_geometry(id, atlas, kind) {
                    pending.push(geometry);
                    skulls.insert(kind_index(kind), (id, location));
                }
            }
        }
        let held_block_entities =
            held_block_entities::build(skull_atlas.as_ref(), skull_location, &mut artwork);
        pending.extend(held_block_entities.geometries.iter().cloned());
        let mut geometries = catalog
            .iter()
            .flat_map(|catalog| catalog.bindings())
            .filter(|binding| {
                !matches!(binding.category, EquipmentCategory::Held)
                    || binding.third_person.literal().is_some()
            })
            .filter(|binding| binding.geometry.resolution == EntityDependencyResolution::Catalog)
            .filter_map(|binding| find_geometry_index(&assets, &binding.geometry.identifier))
            .collect::<Vec<_>>();
        geometries.sort_unstable();
        geometries.dedup();
        let mut item_use: BTreeMap<Box<str>, u32> = assets
            .item_use_profiles()
            .iter()
            .filter_map(|entry| {
                let assets::item_use::ItemUseProfile::Provisional { duration, .. } = &entry.profile
                else {
                    return None;
                };
                let ticks = match duration {
                    assets::item_use::ItemUseDurationRule::Fixed(ticks) => *ticks,
                    assets::item_use::ItemUseDurationRule::LongWeapon => {
                        assets::item_use::LONG_WEAPON_USE_TICKS
                    }
                    assets::item_use::ItemUseDurationRule::Crossbow => {
                        assets::item_use::CROSSBOW_CHARGE_TICKS
                    }
                    assets::item_use::ItemUseDurationRule::Declared => return None,
                };
                Some((entry.identifier.clone(), ticks))
            })
            .collect();
        item_use.extend(
            catalog
                .iter()
                .flat_map(|catalog| catalog.item_use())
                .map(|entry| (entry.identifier.clone(), entry.ticks)),
        );
        let item_use = Arc::new(item_use);
        let item_attack: Arc<BTreeMap<Box<str>, protocol::ItemAttackTiming>> = Arc::new(
            catalog
                .iter()
                .flat_map(|catalog| catalog.item_attack_timings())
                .map(|entry| (entry.identifier.clone(), attack::timing(entry)))
                .collect(),
        );
        let use_overlays = Arc::new(
            catalog
                .iter()
                .flat_map(|catalog| catalog.item_use_overlays())
                .map(|overlay| (overlay.identifier.clone(), overlay.clone()))
                .collect(),
        );
        let runtime = Self {
            attachables: actor_animation::AttachablesRuntime::new(Arc::clone(&assets)),
            attachable_meshes: BTreeMap::new(),
            native_meshes: BTreeMap::new(),
            java_rasters: BTreeMap::new(),
            selected_geometries: Default::default(),
            dial_frames: Default::default(),
            external_armor: external_armor::ExternalArmor::new(catalog.as_deref()),
            assets,
            icons,
            placements: atlas.placements,
            startup_blocks: blocks.clone(),
            blocks,
            atlas_locations: locations[..atlas_layers].to_vec(),
            texture_locations,
            body_bones: BTreeMap::new(),
            skin_bones: BTreeMap::new(),
            armor_geometry: BTreeMap::new(),
            armor_maps: BTreeMap::new(),
            meshes: BTreeMap::new(),
            use_overlays,
            base_item_use: Arc::clone(&item_use),
            item_use,
            base_item_attack: Arc::clone(&item_attack),
            item_attack,
            session: session::SessionLayer::default(),
            next_mesh: 0,
            free_meshes: Vec::new(),
            catalog,
            pending,
            skulls,
            startup_held_block_entities: held_block_entities.clone(),
            held_block_entities,
            pack: None,
            logged_misses: Default::default(),
            poses: PoseMemo::default(),
            attachment_poses: placement_cache::PlacementCache::default(),
            layers: Vec::new(),
        };
        (runtime, artwork, geometries)
    }

    /// Shares immutable authored facts with the numerical gameplay and audio cache.
    pub fn item_use_overlays(
        &self,
    ) -> Arc<BTreeMap<Box<str>, assets::equipment::CompiledItemUseOverlay>> {
        Arc::clone(&self.use_overlays)
    }

    /// Item use durations for the animation runtime's max-duration query.
    pub fn item_use_durations(&self) -> Arc<BTreeMap<Box<str>, u32>> {
        Arc::clone(&self.item_use)
    }

    /// Item attack facts shared by action admission and actor presentation.
    pub fn item_attack_timings(&self) -> Arc<BTreeMap<Box<str>, protocol::ItemAttackTiming>> {
        Arc::clone(&self.item_attack)
    }

    /// Geometries generated since the last call; the actor scene must register them before the
    /// next frame is built.
    pub fn take_pending_geometries(&mut self) -> Vec<ActorRigGeometry> {
        std::mem::take(&mut self.pending)
    }

    /// Builds held and worn layers into reusable storage, valid until the next runtime call.
    pub fn layers_for(
        &mut self,
        body: &ActorRigSubmission,
        input: &ActorEquipmentInput,
        animation: Option<EquipmentAnimation<'_>>,
    ) -> &mut [EquipmentPresentation] {
        self.layers_for_profile(body, input, render_api::DEFAULT_COMBAT_VISUALS, animation)
            .layers
    }

    /// Selects combat visuals for worn slots without changing held items or Java poses.
    pub fn layers_for_profile(
        &mut self,
        body: &ActorRigSubmission,
        input: &ActorEquipmentInput,
        profile: render_api::CombatVisuals,
        animation: Option<EquipmentAnimation<'_>>,
    ) -> EquipmentLayers<'_> {
        let mut layers = std::mem::take(&mut self.layers);
        layers.clear();
        let wearing_elytra = self.fill_layers(body, input, profile, animation, &mut layers);
        combat::apply(&mut layers, body, input, profile, animation);
        self.layers = layers;
        EquipmentLayers {
            layers: &mut self.layers,
            wearing_elytra,
        }
    }

    /// Appends one body's equipment while retaining the caller's output capacity.
    fn fill_layers(
        &mut self,
        body: &ActorRigSubmission,
        input: &ActorEquipmentInput,
        profile: render_api::CombatVisuals,
        animation: Option<EquipmentAnimation<'_>>,
        layers: &mut Vec<EquipmentPresentation>,
    ) -> bool {
        if !matches!(
            body.route,
            ActorRigRoute::Compiled | ActorRigRoute::StaticFallback
        ) || body.input.identity.layer != ACTOR_LAYER_BODY
        {
            return false;
        }
        let Some((geometry, bones)) = self.body_bones_for(body.input.rig) else {
            return false;
        };
        let pose_len = bones.names.len();
        if body.input.previous_bones.len() != pose_len || body.input.current_bones.len() != pose_len
        {
            return false;
        }
        for (item, layer, bone) in [
            (&input.main, LAYER_MAIN_HAND, bones.right_item),
            (&input.off, LAYER_OFF_HAND, bones.left_item),
        ] {
            let Some(item) = item else { continue };
            let held_item_scale = animation.and_then(|animation| animation.rig.held_item_scale);
            let scale = actor_animation::AttachableAnimationInput {
                off_hand: layer == LAYER_OFF_HAND,
                held_item_scale,
                ..Default::default()
            }
            .main_hand_scale();
            let before = layers.len();
            let route = self.equipment_route(
                item,
                input,
                animation,
                routing::EquipmentSlot::Held {
                    off_hand: layer == LAYER_OFF_HAND,
                },
            );
            if matches!(route, routing::EquipmentRoute::NativeHeld) {
                let elapsed = animation
                    .filter(|animation| animation.owner.is_using_item())
                    .map(|animation| animation.rig.hand[1].use_ticks);
                self.push_native_held(
                    body,
                    item,
                    layer,
                    &bones,
                    input.java,
                    actor_animation::AttachableAnimationInput {
                        off_hand: layer == LAYER_OFF_HAND,
                        held_item_scale,
                        animation_frame: inventory::crossbow_animation_frame(
                            elapsed,
                            item.use_facts.ticks.unwrap_or_default(),
                            item.charged_projectile,
                            input.off.as_ref().is_some_and(|off| {
                                off.use_facts.projectile
                                    == assets::item_use::ItemProjectileRole::Firework
                            }),
                        ),
                        use_elapsed_ticks: elapsed,
                        max_use_ticks: item.use_facts.ticks.unwrap_or_default(),
                        ..Default::default()
                    },
                    layers,
                );
                if layers.len() == before {
                    self.note_missing_layer(item, None, bone);
                }
                continue;
            }
            if let routing::EquipmentRoute::Held(source) = route {
                self.push_held(
                    body,
                    item,
                    layer,
                    &bones,
                    source.as_ref(),
                    (input, animation),
                    scale,
                    layers,
                );
            }
            if layers.len() == before {
                self.note_missing_layer(item, None, bone);
            }
        }
        let slots = [
            (ArmorSlot::Helmet, LAYER_HELMET),
            (ArmorSlot::Chestplate, LAYER_CHESTPLATE),
            (ArmorSlot::Leggings, LAYER_LEGGINGS),
            (ArmorSlot::Boots, LAYER_BOOTS),
        ];
        let mut wearing_elytra = false;
        for ((slot, layer), item) in slots.into_iter().zip(&input.armor) {
            if let Some(item) = item {
                let route = self.equipment_route(
                    item,
                    input,
                    animation,
                    routing::EquipmentSlot::Worn(slot),
                );
                let source = match route {
                    routing::EquipmentRoute::Skull(kind) => {
                        self.push_skull(body, kind, layer, bones.head, layers);
                        continue;
                    }
                    routing::EquipmentRoute::HeadBlock => {
                        self.push_attached(
                            body,
                            item,
                            layer,
                            bones.head,
                            Some(head_block_display()),
                            1.0,
                            layers,
                        );
                        continue;
                    }
                    routing::EquipmentRoute::Elytra(source) => {
                        wearing_elytra = true;
                        if let Some(animation) = animation {
                            self.push_elytra(body, item, source, input, animation, layers);
                        }
                        continue;
                    }
                    routing::EquipmentRoute::Armor(source) => source,
                    _ => {
                        self.note_missing_layer(item, Some(slot), bones.head);
                        continue;
                    }
                };
                let before = layers.len();
                self.push_armor(
                    body,
                    &bones,
                    geometry,
                    (slot, layer),
                    item,
                    source,
                    (input, animation, profile),
                    layers,
                );
                if layers.len() == before {
                    self.note_missing_layer(item, Some(slot), bones.head);
                }
            }
        }
        wearing_elytra
    }

    /// Records the bones of a skin model registered under `rig`, replacing any earlier model.
    pub fn register_skin_rig(&mut self, rig: EntityRigId, names: Vec<Box<str>>) {
        self.armor_maps.retain(|(body, _), _| *body != rig.0);
        self.skin_bones.insert(rig, Arc::new(body_bones(names)));
    }

    fn body_bones_for(&mut self, rig: EntityRigId) -> Option<(u32, Arc<BodyBones>)> {
        if let Some(bones) = self.skin_bones.get(&rig) {
            // Skin rig ids lie far above every catalog geometry index, so they key armor maps.
            return Some((rig.0, Arc::clone(bones)));
        }
        if render_model::is_pack_rig_id(rig) {
            return self.pack_body_bones(rig);
        }
        let geometry = self
            .assets
            .rig_geometries()
            .get(usize::try_from(rig.0).ok()?)?
            .geometry;
        let entry = self.body_bones.entry(geometry).or_insert_with(|| {
            Some(Arc::new(body_bones(geometry_bone_names(
                &self.assets,
                geometry as usize,
            )?)))
        });
        entry.clone().map(|bones| (geometry, bones))
    }

    pub(super) fn armor_geometry_for(
        &mut self,
        identifier: &str,
        from_pack: bool,
    ) -> Option<Arc<ArmorGeometry>> {
        if from_pack {
            return self.pack_armor_geometry_for(identifier);
        }
        if let Some(entry) = self.armor_geometry.get(identifier) {
            return entry.clone();
        }
        let entry = find_geometry_index(&self.assets, identifier).and_then(|index| {
            Some(Arc::new(ArmorGeometry {
                rig: equipment_rig_id(index),
                names: geometry_bone_names(&self.assets, index as usize)?,
                pivots: geometry_bone_pivots(&self.assets, index as usize)?,
                binding_expressions: geometry_bone_binding_expressions(
                    &self.assets,
                    index as usize,
                )?,
            }))
        });
        self.armor_geometry.insert(identifier.into(), entry.clone());
        entry
    }

    fn mesh_for(
        &mut self,
        key: MeshKey,
        placement_index: usize,
        placement: Placement,
    ) -> Option<EntityRigId> {
        if let Some(entry) = self.meshes.get(&key) {
            return *entry;
        }
        let entry = self.build_mesh(key, placement_index, placement);
        self.meshes.insert(key, entry);
        entry
    }

    /// Queues valid geometry before consuming a new or retired item mesh slot.
    fn build_item_mesh(
        &mut self,
        make: impl FnOnce(EntityRigId) -> Option<ActorRigGeometry>,
    ) -> Option<EntityRigId> {
        let id = self.free_meshes.last().copied().or_else(|| {
            (self.next_mesh < MAX_ITEM_MESHES as u32).then(|| item_mesh_rig_id(self.next_mesh))
        })?;
        let geometry = make(id)?;
        if self.free_meshes.pop().is_none() {
            self.next_mesh += 1;
        }
        self.pending.push(geometry);
        Some(id)
    }

    fn build_mesh(
        &mut self,
        key: MeshKey,
        placement_index: usize,
        placement: Placement,
    ) -> Option<EntityRigId> {
        let sprite = match key {
            MeshKey::Sprite(_) => self.icons.sprites().get(placement_index),
            MeshKey::Session(index) => self.session_sprite_pixels(index),
            MeshKey::Block(_) | MeshKey::SessionBlock(_) => None,
        };
        let model = match key {
            MeshKey::Block(visual) => self.block_model(visual),
            MeshKey::SessionBlock(index) => self.session_block_model(index),
            _ => None,
        };
        let vertices = match (key, sprite) {
            (_, _) if model.is_some() => {
                let rect = placement.uv_rect();
                model?
                    .iter()
                    .copied()
                    .map(|mut vertex| {
                        for uv in [&mut vertex.uv, &mut vertex.back_uv] {
                            for axis in 0..2 {
                                uv[axis] = rect[axis] + uv[axis] * (rect[axis + 2] - rect[axis]);
                            }
                        }
                        for axis in 0..2 {
                            vertex.glint_origin[axis] = rect[axis]
                                + vertex.glint_origin[axis] * (rect[axis + 2] - rect[axis]);
                        }
                        vertex
                    })
                    .collect()
            }
            (MeshKey::Block(_) | MeshKey::SessionBlock(_), _) => textured_cube_vertices(
                render_model::equipment::blocks::face_rects(placement.uv_rect()),
            ),
            (_, Some(sprite)) => held_sprite_vertices(
                usize::from(sprite.width),
                usize::from(sprite.height),
                &sprite.rgba8,
                placement.uv_rect(),
            )?,
            (_, None) => return None,
        };
        let vertices = match key {
            // Opaque and blended held blocks draw only their outward faces, as vanilla culls a
            // held stained glass block's far side; cutout blocks (glass, leaves) render both
            // sides. A session cube's alpha mode is not known here, so it keeps both sides.
            MeshKey::Block(visual)
                if self.block_alpha_mode(visual)
                    != Some(render_model::HandItemAlphaMode::Cutout) =>
            {
                one_sided(vertices)
            }
            MeshKey::Block(_)
            | MeshKey::SessionBlock(_)
            | MeshKey::Sprite(_)
            | MeshKey::Session(_) => vertices,
        };
        if let MeshKey::Session(index) | MeshKey::SessionBlock(index) = key {
            let id = session::session_mesh_id(index)?;
            let geometry = ActorRigGeometry::new(id, vertices, vec![[0.0; 3]]).ok()?;
            self.pending.push(geometry);
            Some(id)
        } else {
            self.build_item_mesh(|id| ActorRigGeometry::new(id, vertices, vec![[0.0; 3]]).ok())
        }
    }
}

/// Marks every face of a held block one-sided, so renderers discard its back faces.
fn one_sided(mut vertices: Vec<render_model::ActorRigVertex>) -> Vec<render_model::ActorRigVertex> {
    for vertex in &mut vertices {
        vertex.back_uv = render_model::ONE_SIDED_BACK_UV;
    }
    vertices
}

/// Memo keys of the first-person arm mask and held item, which no worn layer uses.
const FIRST_PERSON_MASK_LAYER: u8 = u8::MAX;
const FIRST_PERSON_ITEM_LAYER: u8 = u8::MAX - 1;
const FIRST_PERSON_OFFHAND_LAYER: u8 = u8::MAX - 2;

/// Frames an actor layer may go undrawn before its poses are released.
const POSE_MEMO_RETENTION_FRAMES: u64 = 4;

type RenderPose = Arc<[RenderBoneTransform]>;

/// The poses each actor layer last drew: an unchanged pose keeps its allocation, so frames of a
/// tick share it and its bone matrices.
#[derive(Debug, Default)]
pub(super) struct PoseMemo {
    entries: bevy::platform::collections::HashMap<(u64, u8), ([RenderPose; 2], u64)>,
    frame: u64,
}

impl PoseMemo {
    fn begin_frame(&mut self) {
        self.frame += 1;
        let oldest = self.frame.saturating_sub(POSE_MEMO_RETENTION_FRAMES);
        self.entries.retain(|_, entry| entry.1 >= oldest);
    }

    /// Samples placed channels without allocating when the published pose is unchanged.
    pub(super) fn sample_pair(
        &mut self,
        body: &ActorRigSubmission,
        layer: u8,
        len: usize,
        mut transform: impl FnMut(usize, usize) -> Option<RenderBoneTransform>,
    ) -> Option<[RenderPose; 2]> {
        let key = (body.input.identity.runtime_id, layer);
        let old = self.entries.get(&key).map(|entry| entry.0.clone());
        let mut sampled: [Option<RenderPose>; 2] = [None, None];
        for endpoint in 0..2 {
            let retained = sampled[0]
                .iter()
                .chain(old.iter().flatten())
                .find(|pose| {
                    pose.len() == len
                        && (0..len).all(|index| transform(endpoint, index) == Some(pose[index]))
                })
                .cloned();
            sampled[endpoint] = Some(match retained {
                Some(pose) => pose,
                None => (0..len)
                    .map(|index| transform(endpoint, index))
                    .collect::<Option<Vec<_>>>()?
                    .into(),
            });
        }
        let poses = sampled.map(Option::unwrap);
        self.entries.insert(key, (poses.clone(), self.frame));
        Some(poses)
    }

    /// Shared allocations holding `poses` (previous, current) for `body`'s `layer`.
    pub(super) fn share(
        &mut self,
        body: &ActorRigSubmission,
        layer: u8,
        poses: [&[RenderBoneTransform]; 2],
    ) -> [Arc<[RenderBoneTransform]>; 2] {
        let key = (body.input.identity.runtime_id, layer);
        let old = self.entries.get(&key).map(|entry| entry.0.clone());
        let shared = poses.map(|pose| {
            old.iter()
                .flatten()
                .find(|known| ***known == *pose)
                .map_or_else(|| Arc::from(pose), Arc::clone)
        });
        self.entries.insert(key, (shared.clone(), self.frame));
        shared
    }
}

/// An equipment instance that shares `body`'s identity, transform, and generations.
/// Keeps its own tint and the body's lighting without inheriting the body's hurt overlay.
pub(super) fn layer_presentation(
    body: &ActorRigSubmission,
    layer: u8,
    rig: EntityRigId,
    [previous, current]: [Arc<[RenderBoneTransform]>; 2],
    location: ActorArtworkLocation,
    tint: u32,
) -> EquipmentPresentation {
    let mut identity = body.input.identity;
    identity.layer = layer;
    EquipmentPresentation {
        submission: ActorRigSubmission {
            material: Default::default(),
            culling_bounds: body.culling_bounds,
            input: ActorRigRenderInput {
                identity,
                rig,
                previous_bones: previous,
                current_bones: current,
                completed_tick: body.input.completed_tick,
                reset_generation: body.input.reset_generation,
            },
            world_from_actor: body.world_from_actor,
            texture_layer: location.layer(),
            route: ActorRigRoute::Compiled,
            tint,
            uv_anim: render_model::IDENTITY_UV_ANIM,
            light: body.light,
            overlay_rgba8: 0,
        },
        location,
    }
}

#[cfg(test)]
mod tests;
