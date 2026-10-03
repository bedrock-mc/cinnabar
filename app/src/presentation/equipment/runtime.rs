//! Builds equipment rig layers (held sprite meshes and worn armor) for a body submission.

use std::{collections::BTreeMap, sync::Arc};

use assets::{
    ArmorSlot, EntityDependencyResolution, EquipmentCategory, IconSprite, RuntimeAssets,
    RuntimeBlockEntityAssets, RuntimeEntityAssets, RuntimeEquipmentCatalog, RuntimeIconCatalog,
};
use bevy::prelude::Resource;
use render::{
    ACTOR_LAYER_BODY, ActorArtworkLocation, ActorArtworkPages, ActorRigGeometry,
    ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, BlockEntityAtlas, EntityRigId,
    EquipmentRaster, RenderBoneTransform, SkullKind, equipment_rig_id, find_geometry_index,
    geometry_bone_names, geometry_bone_pivots, held_sprite_vertices, item_mesh_rig_id,
    skull_geometry, textured_cube_vertices,
};

mod diagnostics;
mod modern;
mod pack;
mod push;
mod session;
mod types;
pub(crate) use pack::PackEquipment;
pub(crate) use session::StagedSessionIcons;
pub(crate) use types::{
    ActorEquipmentInput, EquipmentPresentation, FirstPersonArms, FirstPersonItem, HeldKind,
    WornItem,
};
use types::{ArmorGeometry, BodyBones, ElytraStance, MeshKey};

use super::{
    armor::{DEFAULT_LEATHER_RGB, bone_map, hidden_bone, pack_tint, remap_pose},
    atlas::{Placement, SpriteAtlas},
    attachable::{self, BoneChannels},
    blocks::{self, BlockSheets},
    display::{
        FirstPersonHand, FirstPersonShape, ItemDisplay, LAYER_BOOTS, LAYER_CHESTPLATE,
        LAYER_HELMET, LAYER_LEGGINGS, LAYER_MAIN_HAND, LAYER_OFF_HAND, attach_to_bone,
        first_person_display, head_block_display, held_block_display, held_sprite_display,
        is_hand_equipped, is_mirrored_art, view_bone,
    },
    elytra,
};

fn body_bones(names: Vec<Box<str>>) -> BodyBones {
    let find = |wanted: &str| {
        names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(wanted))
    };
    BodyBones {
        right_item: find("rightItem"),
        left_item: find("leftItem"),
        head: find("head"),
        names,
    }
}

/// Equipment geometry index space reserved for worn skulls (above any entity geometry index).
const SKULL_RIG_INDEX_BASE: u32 = 0x00ff_0000;
const SKULL_KINDS: [SkullKind; 5] = [
    SkullKind::Skeleton,
    SkullKind::WitherSkeleton,
    SkullKind::Zombie,
    SkullKind::Player,
    SkullKind::Creeper,
];

fn kind_index(kind: SkullKind) -> u8 {
    SKULL_KINDS
        .iter()
        .position(|candidate| *candidate == kind)
        .map_or(u8::MAX, |index| index as u8)
}

/// The head an item identifier wears as, for the heads with a packed texture.
pub(super) fn skull_kind(identifier: &str) -> Option<SkullKind> {
    Some(match identifier.strip_prefix("minecraft:")? {
        "skeleton_skull" => SkullKind::Skeleton,
        "wither_skeleton_skull" => SkullKind::WitherSkeleton,
        "zombie_head" => SkullKind::Zombie,
        "player_head" => SkullKind::Player,
        "creeper_head" => SkullKind::Creeper,
        _ => return None,
    })
}

/// Generated item meshes kept resident; further distinct items draw nothing.
const MAX_ITEM_MESHES: usize = 512;

/// Body bone index per armor bone; `None` where the body lacks it.
type ArmorBoneMap = Arc<[Option<usize>]>;

#[derive(Resource)]
pub(crate) struct EquipmentRuntime {
    assets: Arc<RuntimeEntityAssets>,
    catalog: Option<Arc<RuntimeEquipmentCatalog>>,
    icons: Arc<RuntimeIconCatalog>,
    placements: Vec<Option<Placement>>,
    /// Block visual id to its carried or fallback sheet's index in `placements`.
    block_sheets: BTreeMap<u32, usize>,
    atlas_locations: Vec<Option<ActorArtworkLocation>>,
    texture_locations: BTreeMap<Box<str>, ActorArtworkLocation>,
    body_bones: BTreeMap<u32, Option<Arc<BodyBones>>>,
    /// Bones of player skins' own models, by skin rig id.
    skin_bones: BTreeMap<EntityRigId, Arc<BodyBones>>,
    armor_geometry: BTreeMap<Box<str>, Option<Arc<ArmorGeometry>>>,
    armor_maps: BTreeMap<(u32, Box<str>), ArmorBoneMap>,
    meshes: BTreeMap<MeshKey, Option<EntityRigId>>,
    pending: Vec<ActorRigGeometry>,
    /// Worn head geometry and texture location per skull kind.
    skulls: BTreeMap<u8, (EntityRigId, ActorArtworkLocation)>,
    item_use: Arc<BTreeMap<Box<str>, u32>>,
    /// The startup catalog's use durations, before session items join them.
    base_item_use: Arc<BTreeMap<Box<str>, u32>>,
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
    attachables: client_world::AttachablesRuntime,
    attachable_meshes: BTreeMap<(bool, u32, Box<str>), EntityRigId>,
}

impl EquipmentRuntime {
    /// Releases memoized poses of layers no recent frame drew.
    pub(crate) fn begin_frame(&mut self) {
        self.poses.begin_frame();
    }

    /// Places the item atlas and attachable textures on new artwork pages. Returns the runtime,
    /// the extended artwork, and the entity-catalog geometries the actor scene must register.
    pub(crate) fn build(
        assets: Arc<RuntimeEntityAssets>,
        catalog: Option<Arc<RuntimeEquipmentCatalog>>,
        icons: Arc<RuntimeIconCatalog>,
        world: Option<Arc<RuntimeAssets>>,
        block_entities: Option<Arc<RuntimeBlockEntityAssets>>,
        artwork: ActorArtworkPages,
    ) -> (Self, ActorArtworkPages, Vec<u32>) {
        let BlockSheets { sheets, by_visual } = world.as_deref().map_or_else(
            || BlockSheets {
                sheets: Vec::new(),
                by_visual: BTreeMap::new(),
            },
            |world| blocks::collect(world, &assets),
        );
        let icon_count = icons.sprites().len();
        let mut block_sheets = by_visual
            .into_iter()
            .map(|(visual, sheet)| (visual, icon_count + sheet))
            .collect::<BTreeMap<_, _>>();
        // Authored carried textures override world faces: grass uses an opaque
        // alpha-mask overlay here, not the biome-tinted terrain material.
        if icons.source_manifest_sha256() == assets.source_manifest_sha256() {
            block_sheets.extend(
                icons
                    .block_sheets()
                    .iter()
                    .filter(|sheet| sheet.visual.0 < assets.block_visual_count())
                    .map(|sheet| (sheet.visual.0, sheet.sprite as usize)),
            );
        }
        let packed = icons
            .sprites()
            .iter()
            .cloned()
            .chain(sheets)
            .collect::<Vec<IconSprite>>();
        let atlas = SpriteAtlas::pack(&packed);
        let atlas_layers = atlas.layers.len();
        let textures = catalog
            .as_ref()
            .map_or(&[][..], |catalog| catalog.textures());
        let mut rasters = atlas.layers;
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
        let (artwork, locations) = artwork.with_equipment_rasters(&rasters);
        let texture_locations = textures
            .iter()
            .zip(&locations[atlas_layers..])
            .filter_map(|(texture, location)| Some((texture.identifier.clone(), (*location)?)))
            .collect();
        let skull_location = skull_raster_index.and_then(|index| locations[index]);
        let mut pending = Vec::new();
        let mut skulls = BTreeMap::new();
        if let (Some(atlas), Some(location)) = (&skull_atlas, skull_location) {
            for (index, kind) in SKULL_KINDS.into_iter().enumerate() {
                let id = equipment_rig_id(SKULL_RIG_INDEX_BASE + index as u32);
                if let Some(geometry) = skull_geometry(id, atlas, kind) {
                    pending.push(geometry);
                    skulls.insert(kind_index(kind), (id, location));
                }
            }
        }
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
        let item_use: Arc<BTreeMap<Box<str>, u32>> = Arc::new(
            catalog
                .iter()
                .flat_map(|catalog| catalog.item_use())
                .map(|entry| (entry.identifier.clone(), entry.ticks))
                .collect(),
        );
        let runtime = Self {
            attachables: client_world::AttachablesRuntime::new(Arc::clone(&assets)),
            attachable_meshes: BTreeMap::new(),
            assets,
            icons,
            placements: atlas.placements,
            block_sheets,
            atlas_locations: locations[..atlas_layers].to_vec(),
            texture_locations,
            body_bones: BTreeMap::new(),
            skin_bones: BTreeMap::new(),
            armor_geometry: BTreeMap::new(),
            armor_maps: BTreeMap::new(),
            meshes: BTreeMap::new(),
            base_item_use: Arc::clone(&item_use),
            item_use,
            session: session::SessionLayer::default(),
            next_mesh: 0,
            free_meshes: Vec::new(),
            catalog,
            pending,
            skulls,
            pack: None,
            logged_misses: Default::default(),
            poses: PoseMemo::default(),
        };
        (runtime, artwork, geometries)
    }

    /// Item use durations for the animation runtime's max-duration query.
    pub(crate) fn item_use_durations(&self) -> Arc<BTreeMap<Box<str>, u32>> {
        Arc::clone(&self.item_use)
    }

    /// Geometries generated since the last call; the actor scene must register them before the
    /// next frame is built.
    pub(crate) fn take_pending_geometries(&mut self) -> Vec<ActorRigGeometry> {
        std::mem::take(&mut self.pending)
    }

    /// Equipment layers for one drawn player body, riding the body's own pose and transform.
    pub(crate) fn layers_for(
        &mut self,
        body: &ActorRigSubmission,
        input: &ActorEquipmentInput,
    ) -> Vec<EquipmentPresentation> {
        let mut layers = Vec::new();
        if !matches!(
            body.route,
            ActorRigRoute::Compiled | ActorRigRoute::StaticFallback
        ) || body.input.identity.layer != ACTOR_LAYER_BODY
        {
            return layers;
        }
        let Some((geometry, bones)) = self.body_bones_for(body.input.rig) else {
            return layers;
        };
        let pose_len = bones.names.len();
        if body.input.previous_bones.len() != pose_len || body.input.current_bones.len() != pose_len
        {
            return layers;
        }
        for (item, layer, bone) in [
            (&input.main, LAYER_MAIN_HAND, bones.right_item),
            (&input.off, LAYER_OFF_HAND, bones.left_item),
        ] {
            let Some(item) = item else { continue };
            let before = layers.len();
            self.push_held(body, item, layer, bone, &mut layers);
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
        for ((slot, layer), item) in slots.into_iter().zip(&input.armor) {
            if let Some(item) = item {
                if slot == ArmorSlot::Helmet
                    && let Some(kind) = skull_kind(&item.identifier)
                {
                    self.push_skull(body, kind, layer, bones.head, &mut layers);
                    continue;
                }
                // A block worn in the helmet slot (a carved pumpkin) sits on the head bone.
                if slot == ArmorSlot::Helmet
                    && matches!(item.kind, HeldKind::Block(_))
                    && !self.has_armor_binding(&item.identifier)
                {
                    self.push_attached(
                        body,
                        item,
                        layer,
                        bones.head,
                        Some(head_block_display()),
                        &mut layers,
                    );
                    continue;
                }
                let worn = ElytraStance {
                    sneaking: input.sneaking,
                    sleeping: input.sleeping,
                };
                let before = layers.len();
                self.push_armor(
                    body,
                    &bones,
                    geometry,
                    (slot, layer, worn),
                    item,
                    &mut layers,
                );
                if layers.len() == before {
                    self.note_missing_layer(item, Some(slot), bones.head);
                }
            }
        }
        layers
    }

    /// The body pose with every bone but the visible arms (and their sleeves) zero-scaled, as
    /// vanilla's first-person part visibility hides them. `None` when no arm shows or the pose
    /// does not match the body geometry.
    pub(crate) fn mask_first_person(
        &mut self,
        body: &ActorRigSubmission,
        arms: FirstPersonArms,
    ) -> Option<ActorRigSubmission> {
        if !(arms.right || arms.left) {
            return None;
        }
        let (_, bones) = self.body_bones_for(body.input.rig)?;
        let pose_len = bones.names.len();
        if body.input.previous_bones.len() != pose_len || body.input.current_bones.len() != pose_len
        {
            return None;
        }
        let visible = |name: &str| {
            let is = |wanted: &str| name.eq_ignore_ascii_case(wanted);
            (arms.right && (is("rightArm") || is("rightSleeve")))
                || (arms.left && (is("leftArm") || is("leftSleeve")))
        };
        let mask = |pose: &[RenderBoneTransform]| {
            pose.iter()
                .zip(&bones.names)
                .map(|(bone, name)| if visible(name) { *bone } else { hidden_bone() })
                .collect::<Vec<_>>()
        };
        let mut masked = body.clone();
        let (previous, current) = (
            mask(&body.input.previous_bones),
            mask(&body.input.current_bones),
        );
        [masked.input.previous_bones, masked.input.current_bones] =
            self.poses
                .share(body, FIRST_PERSON_MASK_LAYER, [&previous, &current]);
        Some(masked)
    }

    /// The main-hand item as a first-person layer, when it is drawable. An attachable rides the
    /// posed `rightItem` bone; any other item carries a camera-space bone (`view_space`), placed
    /// by `renderFirstPerson`'s own transforms for the arm's `hand` state.
    pub(crate) fn first_person_item(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        hand: impl Into<FirstPersonHand>,
    ) -> Option<FirstPersonItem> {
        let (_, bones) = self.body_bones_for(body.input.rig)?;
        let pose_len = bones.names.len();
        if body.input.previous_bones.len() != pose_len || body.input.current_bones.len() != pose_len
        {
            return None;
        }
        // Modern attachables are evaluated in their own first-person state before this
        // fallback. Never substitute a third-person grip for their camera-space animation.
        let (mesh, location, block) = self.held_mesh(item, true)?;
        let shape = if block {
            FirstPersonShape::Block
        } else {
            FirstPersonShape::Sprite {
                mirrored_art: is_mirrored_art(&item.identifier),
            }
        };
        let hand = hand.into();
        let state = client_world::ItemAnimationState {
            attack_time: hand.swing,
            arm_height: hand.equip,
        };
        let bone = match (shape, hand.consume) {
            (FirstPersonShape::Block, None) => super::first_person::block_pose(state),
            (
                FirstPersonShape::Sprite {
                    mirrored_art: false,
                },
                None,
            ) => super::first_person::sprite_pose(state),
            _ => view_bone(first_person_display(shape, hand)),
        }?;
        let poses = self
            .poses
            .share(body, FIRST_PERSON_ITEM_LAYER, [&[bone], &[bone]]);
        Some(FirstPersonItem {
            presentation: layer_presentation(body, LAYER_MAIN_HAND, mesh, poses, location, 0),
            camera_space: true,
        })
    }

    /// Ordinary offhand geometry follows the separate native offhand stack.
    pub(crate) fn first_person_offhand(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
    ) -> Option<FirstPersonItem> {
        let (mesh, location, block) = self.held_mesh(item, true)?;
        let equipped = self.hand_equipped(&item.identifier);
        let bone = if block {
            super::first_person::offhand_pose(equipped, true)
        } else {
            let sprite = self
                .session_sprite(&item.identifier, item.metadata)
                .and_then(|(index, _, _)| self.session_sprite_pixels(index))
                .or_else(|| {
                    self.icons
                        .lookup_index(&item.identifier, item.metadata)
                        .and_then(|index| self.icons.sprites().get(index))
                })?;
            super::first_person::offhand_sprite_pose(equipped, [sprite.width, sprite.height])
        }?;
        let poses = self
            .poses
            .share(body, FIRST_PERSON_OFFHAND_LAYER, [&[bone], &[bone]]);
        Some(FirstPersonItem {
            presentation: layer_presentation(body, LAYER_OFF_HAND, mesh, poses, location, 0),
            camera_space: true,
        })
    }

    /// Records the bones of a skin model registered under `rig`, replacing any earlier model.
    pub(crate) fn register_skin_rig(&mut self, rig: EntityRigId, names: Vec<Box<str>>) {
        self.armor_maps.retain(|(body, _), _| *body != rig.0);
        self.skin_bones.insert(rig, Arc::new(body_bones(names)));
    }

    fn body_bones_for(&mut self, rig: EntityRigId) -> Option<(u32, Arc<BodyBones>)> {
        if let Some(bones) = self.skin_bones.get(&rig) {
            // Skin rig ids lie far above every catalog geometry index, so they key armor maps.
            return Some((rig.0, Arc::clone(bones)));
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

    fn has_armor_binding(&self, identifier: &str) -> bool {
        self.binding_source(identifier).is_some()
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
        let vertices = match (key, sprite) {
            (MeshKey::Block(_) | MeshKey::SessionBlock(_), _) => {
                textured_cube_vertices(blocks::face_rects(placement.uv_rect()))
            }
            (_, Some(sprite)) => held_sprite_vertices(
                usize::from(sprite.width),
                usize::from(sprite.height),
                &sprite.rgba8,
                placement.uv_rect(),
            )?,
            (_, None) => return None,
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
    entries: std::collections::HashMap<(u64, u8), ([RenderPose; 2], u64)>,
    frame: u64,
}

impl PoseMemo {
    fn begin_frame(&mut self) {
        self.frame += 1;
        let oldest = self.frame.saturating_sub(POSE_MEMO_RETENTION_FRAMES);
        self.entries.retain(|_, entry| entry.1 >= oldest);
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
            uv_anim: render::IDENTITY_UV_ANIM,
            light: body.light,
            overlay_rgba8: body.overlay_rgba8,
        },
        location,
    }
}

#[cfg(test)]
mod tests;
