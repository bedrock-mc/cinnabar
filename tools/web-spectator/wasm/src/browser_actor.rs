//! Stream adapters for Cinnabar's compiled actor geometry and GPU publication.
//! Geometry, skin normalization, armor UVs and artwork pages retain native owners.

use std::{collections::BTreeMap, sync::Arc};

use assets::{EquipmentCategory, RuntimeEntityAssets, RuntimeEquipmentCatalog, RuntimeIconCatalog};
use bevy::math::Mat4;
use render::{
    ActorArtworkLocation, ActorArtworkPages, ActorRenderIdentity, ActorRenderScene,
    ActorRigFrameBuilder, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, HandItemAtlas,
    HandRigLight, HandRigScene,
};
use render_api::SkinRgba8;
use render_model::{ActorRigGeometry, ActorSkinPixels, EntityRigId, equipment::EquipmentRaster};
use sha2::{Digest, Sha256};

use super::browser_model::{Fighter, Item};
use view_presentation::equipment_display::{self, FirstPersonArms, FirstPersonShape};
use view_presentation::equipment_sprite_atlas::{Placement, SpriteAtlas};

mod animation;
mod persona;
mod update;
use animation::NativeAnimator;

const MAX_BROWSER_SKINS: usize = 64;

struct Skin {
    pixels: SkinRgba8,
    slim: bool,
}
struct Rig {
    id: EntityRigId,
    names: Vec<Box<str>>,
    source: Arc<protocol::SkinGeometrySource>,
    geometry: ActorRigGeometry,
    bounds: assets::SkinGeometryBounds,
}

pub(super) struct BrowserActors {
    scene: ActorRenderScene,
    equipment: RuntimeEquipmentCatalog,
    classic: Rig,
    entities: BTreeMap<String, Rig>,
    slim: Option<Rig>,
    armor: BTreeMap<String, Rig>,
    textures: BTreeMap<String, ActorArtworkLocation>,
    skins: BTreeMap<String, Skin>,
    custom: BTreeMap<String, Rig>,
    next_custom: u32,
    cape_rig: Option<view_presentation::cape::CapeRig>,
    capes: BTreeMap<String, SkinRgba8>,
    skin_keys: Vec<String>,
    render_skins: Vec<SkinRgba8>,
    generation: u64,
    icons: RuntimeIconCatalog,
    placements: Vec<Option<Placement>>,
    item_locations: Vec<Option<ActorArtworkLocation>>,
    meshes: BTreeMap<usize, EntityRigId>,
    hand_builder: ActorRigFrameBuilder,
    artwork: ActorArtworkPages,
    persona: persona::PersonaLayers,
    animator: NativeAnimator,
}

impl BrowserActors {
    pub(super) fn new(
        entity_bytes: &[u8],
        actor_bytes: &[u8],
        equipment_bytes: &[u8],
        icon_bytes: &[u8],
    ) -> Result<Self, String> {
        let entities = RuntimeEntityAssets::decode(entity_bytes).map_err(|e| e.to_string())?;
        let equipment =
            RuntimeEquipmentCatalog::decode(equipment_bytes).map_err(|e| e.to_string())?;
        let icons = RuntimeIconCatalog::decode(icon_bytes).map_err(|e| e.to_string())?;
        if icons.source_manifest_sha256() != entities.source_manifest_sha256() {
            return Err("held item icons do not belong to the compiled entity carrier".into());
        }
        let entity_hash: [u8; 32] = Sha256::digest(entity_bytes).into();
        if equipment.entity_blob_sha256() != entity_hash
            || equipment.source_manifest_sha256() != entities.source_manifest_sha256()
        {
            return Err("equipment does not belong to the compiled entity carrier".into());
        }
        let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities)
            .map_err(|e| format!("compiled actor geometry: {e:?}"))?;
        let cape_rig = view_presentation::cape::CapeRig::resolve(&entities);
        if let Some(rig) = &cape_rig {
            scene
                .insert_geometry(rig.geometry.clone())
                .map_err(|e| format!("cape geometry: {e:?}"))?;
        }
        let classic = register_rig(
            &mut scene,
            &entities,
            "geometry.humanoid.custom",
            render_model::skin_rig_id(0),
        )?;
        let slim = render_model::find_geometry_index(&entities, "geometry.humanoid.customSlim")
            .map(|_| {
                register_rig(
                    &mut scene,
                    &entities,
                    "geometry.humanoid.customSlim",
                    render_model::skin_rig_id(1),
                )
            })
            .transpose()?;
        let mut armor = BTreeMap::new();
        for binding in equipment.bindings() {
            if !matches!(binding.category, EquipmentCategory::Armor { .. })
                || armor.contains_key(binding.geometry.identifier.as_ref())
            {
                continue;
            }
            let Some(index) =
                render_model::find_geometry_index(&entities, &binding.geometry.identifier)
            else {
                continue;
            };
            let rig = register_rig(
                &mut scene,
                &entities,
                &binding.geometry.identifier,
                render_model::equipment_rig_id(index),
            )?;
            armor.insert(binding.geometry.identifier.to_string(), rig);
        }
        let catalog = assets::RuntimeActorCatalog::decode(actor_bytes, &entities)
            .map_err(|error| error.to_string())?;
        let mut world_entities = BTreeMap::new();
        for (symbol, binding) in crate::browser_entity_catalog::world_entity_bindings(
            entities.symbols(),
            catalog.bindings(),
        ) {
            if world_entities.contains_key(symbol.identifier.as_ref()) {
                continue;
            }
            let Some(geometry) = entities.geometries().get(binding.geometry as usize) else {
                continue;
            };
            // Optional catalog rigs follow native admission; required player rigs above stay strict.
            let Ok(prepared) = render_model::entity_geometry(
                &entities,
                binding.geometry as usize,
                render_model::pack_rig_id(binding.geometry_candidate),
            ) else {
                continue;
            };
            let rig = register_geometry(
                &mut scene,
                &entities,
                &geometry.identifier,
                binding.geometry as usize,
                prepared,
            )?;
            world_entities.insert(symbol.identifier.to_string(), rig);
        }
        let atlas = SpriteAtlas::pack(icons.sprites());
        let rasters = equipment
            .textures()
            .iter()
            .map(|texture| EquipmentRaster {
                width: texture.width,
                height: texture.height,
                rgba8: Arc::clone(&texture.rgba8),
            })
            .collect::<Vec<_>>();
        let (artwork, locations) = ActorArtworkPages::default()
            .with_pack_artwork(catalog.textures(), catalog.bindings())
            .with_equipment_rasters(&rasters);
        let (artwork, item_locations) = artwork.with_equipment_rasters(&atlas.layers);
        let hand_builder = ActorRigFrameBuilder::new(
            std::iter::once(classic.geometry.clone())
                .chain(slim.as_ref().map(|rig| rig.geometry.clone())),
        )
        .map_err(|e| format!("compiled first-person geometry: {e:?}"))?;
        let textures = equipment
            .textures()
            .iter()
            .zip(locations)
            .filter_map(|(texture, location)| Some((texture.identifier.to_string(), location?)))
            .collect();
        scene.configure_artwork(artwork.clone());
        Ok(Self {
            scene,
            equipment,
            classic,
            entities: world_entities,
            slim,
            armor,
            textures,
            skins: BTreeMap::new(),
            custom: BTreeMap::new(),
            next_custom: 2,
            cape_rig,
            capes: BTreeMap::new(),
            skin_keys: Vec::new(),
            render_skins: Vec::new(),
            generation: 0,
            icons,
            placements: atlas.placements,
            item_locations,
            meshes: BTreeMap::new(),
            hand_builder,
            artwork,
            persona: persona::PersonaLayers::default(),
            animator: NativeAnimator::new(Arc::new(entities)),
        })
    }

    pub(super) fn set_skin(
        &mut self,
        id: &str,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
        slim: bool,
    ) -> Result<(), String> {
        if id.is_empty() || id.len() > 128 {
            return Err("skin identity is invalid".into());
        }
        if slim && self.slim.is_none() {
            return Err("compiled slim player geometry is missing".into());
        }
        let pixels = render_model::normalize_actor_skin(&ActorSkinPixels {
            width,
            height,
            rgba8: rgba8.into(),
        })
        .ok_or_else(|| "skin raster is not a supported bounded Minecraft skin".to_string())?;
        if !self.skins.contains_key(id) && self.skins.len() >= MAX_BROWSER_SKINS {
            let unused = self
                .skins
                .keys()
                .find(|key| !self.skin_keys.contains(key))
                .cloned()
                .ok_or_else(|| "browser skin cache is full".to_string())?;
            self.skins.remove(&unused);
        }
        self.skins.insert(id.into(), Skin { pixels, slim });
        self.animator.invalidate_pose(id);
        self.skin_keys.clear(); // Repack only when the skin payload or admission changes.
        Ok(())
    }

    pub(super) fn set_cape(
        &mut self,
        id: &str,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<(), String> {
        if id.is_empty() || id.len() > 128 || width > 256 || height > 256 {
            return Err("invalid cape bounds".into());
        }
        if width == 0 || height == 0 {
            self.capes.remove(id);
        } else {
            if self.cape_rig.is_none() {
                return Err("canonical cape geometry unavailable".into());
            }
            let pixels = view_presentation::cape::cape_layer(width, height, &pixels)
                .ok_or("invalid cape raster")?;
            if self.capes.len() >= MAX_BROWSER_SKINS && !self.capes.contains_key(id) {
                return Err("cape cache is full".into());
            }
            self.capes.insert(id.into(), SkinRgba8::new(pixels));
        }
        self.skin_keys.clear();
        Ok(())
    }

    pub(super) fn set_animation(
        &mut self,
        id: &str,
        image: protocol::SkinAnimation,
    ) -> Result<(), String> {
        let rig = self
            .custom
            .get_mut(id)
            .ok_or("persona geometry unavailable")?;
        let mut animations = rig.source.animations.to_vec();
        animations.retain(|known| known.kind != image.kind);
        animations.push(image);
        if animations.len() > 3 {
            return Err("too many persona layers".into());
        }
        rig.source = Arc::new(protocol::SkinGeometrySource {
            resource_patch: Arc::clone(&rig.source.resource_patch),
            geometry_data: Arc::clone(&rig.source.geometry_data),
            animations: animations.into(),
        });
        self.animator.invalidate_pose(id);
        Ok(())
    }

    pub(super) fn set_geometry(&mut self, id: &str, patch: &str, data: &str) -> Result<(), String> {
        if id.is_empty() || id.len() > 128 || patch.len() > 16384 || data.len() > 262144 {
            return Err("invalid skin geometry bounds".into());
        }
        let model = assets::parse_skin_geometry(patch, data)
            .map_err(|e| format!("skin geometry: {e:?}"))?;
        let Some(model) = model else {
            self.custom.remove(id);
            self.animator.invalidate_pose(id);
            return Ok(());
        };
        let index = if let Some(existing) = self.custom.get(id) {
            existing.id
        } else {
            if self.next_custom >= 2 + MAX_BROWSER_SKINS as u32 {
                return Err("custom geometry cache is full".into());
            }
            {
                let id = render_model::skin_rig_id(self.next_custom);
                self.next_custom += 1;
                id
            }
        };
        let geometry =
            render_model::skin_geometry(&model, index).map_err(|e| format!("skin rig: {e:?}"))?;
        let names = model.bones.iter().map(|bone| bone.name.clone()).collect();
        let source = Arc::new(protocol::SkinGeometrySource {
            resource_patch: Arc::from(patch),
            geometry_data: Arc::from(data),
            animations: Arc::from([]),
        });
        self.scene
            .insert_geometry(geometry.clone())
            .map_err(|e| format!("register skin rig: {e:?}"))?;
        self.custom.insert(
            id.into(),
            Rig {
                bounds: model.visible_bounds.unwrap_or_default(),
                id: index,
                names,
                source,
                geometry,
            },
        );
        self.animator.invalidate_pose(id);
        Ok(())
    }

    fn mesh_for(&mut self, item: &Item) -> Option<(EntityRigId, ActorArtworkLocation)> {
        let index = self
            .icons
            .lookup_index(&item.name, item.meta.max(0) as u32)?;
        let placement = self.placements.get(index).copied().flatten()?;
        let location = self
            .item_locations
            .get(placement.layer)
            .copied()
            .flatten()?;
        if let Some(id) = self.meshes.get(&index) {
            return Some((*id, location));
        }
        if self.meshes.len() >= 128 {
            return None;
        }
        let sprite = self.icons.sprites().get(index)?;
        let vertices = render_model::held_sprite_vertices(
            usize::from(sprite.width),
            usize::from(sprite.height),
            &sprite.rgba8,
            placement.uv_rect(),
        )?;
        let id = render_model::item_mesh_rig_id(index as u32);
        let geometry = ActorRigGeometry::new(id, vertices, vec![[0.0; 3]]).ok()?;
        self.scene.insert_geometry(geometry.clone()).ok()?;
        self.hand_builder.insert_geometry(geometry).ok()?;
        self.meshes.insert(index, id);
        Some((id, location))
    }

    fn held_layer(
        &mut self,
        body: &ActorRigSubmission,
        item: &Item,
        bone: Option<usize>,
        layer: u8,
    ) -> Option<(ActorRigSubmission, ActorArtworkLocation)> {
        let bone = bone?;
        let (mesh, location) = self.mesh_for(item)?;
        let display = render_model::equipment::held_sprite_display(
            render_model::equipment::is_hand_equipped(&item.name),
        );
        let previous = render_model::equipment::attach_to_bone(
            *body.input.previous_bones.get(bone)?,
            display,
        )?;
        let current =
            render_model::equipment::attach_to_bone(*body.input.current_bones.get(bone)?, display)?;
        let mut held = body.clone();
        held.input.identity.layer = layer;
        held.input.rig = mesh;
        held.input.previous_bones = Arc::from([previous]);
        held.input.current_bones = Arc::from([current]);
        held.texture_layer = location.layer();
        Some((held, location))
    }

    pub(super) fn update_hands(
        &mut self,
        fighter: Option<&Fighter>,
        _partial_tick: f32,
        fov_radians: f32,
        motion: Mat4,
    ) -> HandRigScene {
        let partial_tick = self.animator.partial_tick();
        let mut scene = HandRigScene::default();
        let Some(fighter) = fighter.filter(|fighter| !fighter.dead) else {
            return scene;
        };
        let Some(pov) = &fighter.pov else {
            return scene;
        };
        let Some(skin) = self.skins.get(&fighter.id) else {
            return scene;
        };
        let skin_pixels = skin.pixels.clone();
        let rig = if let Some(rig) = self.custom.get(&fighter.id) {
            rig
        } else if skin.slim {
            self.slim.as_ref().unwrap_or(&self.classic)
        } else {
            &self.classic
        };
        let names = rig.names.clone();
        let Some(animated) = self.animator.pose(&fighter.id, rig.id, &rig.names) else {
            return scene;
        };
        let generation = self.generation.max(1);
        let identity = ActorRenderIdentity {
            session_id: hash_id(&fighter.id),
            dimension: 0,
            runtime_id: hash_id(&fighter.id),
            spawn_revision: 1,
            ingress_sequence: generation,
            source_tick: Some(generation),
            movement_revision: generation,
            pose_generation: animated.completed_tick,
            layer: render::ACTOR_LAYER_BODY,
        };
        let body = ActorRigSubmission {
            material: render::ActorMaterial::default(),
            culling_bounds: assets::SkinGeometryBounds::default(),
            input: ActorRigRenderInput {
                identity,
                rig: rig.id,
                previous_bones: animated.previous,
                current_bones: animated.current,
                completed_tick: animated.completed_tick,
                reset_generation: animated.reset_generation,
            },
            world_from_actor: equipment_display::hand_camera_from_rig(
                animated.scale,
                pov.eye_height,
                motion,
            ),
            texture_layer: 0,
            route: ActorRigRoute::Compiled,
            tint: 0,
            overlay_rgba8: 0,
            uv_anim: render::IDENTITY_UV_ANIM,
            light: render::pack_actor_light(0, 15),
        };
        let main = fighter
            .equipment
            .as_ref()
            .and_then(|equipment| equipment.main_hand.as_ref())
            .or_else(|| pov.hotbar.get(pov.selected_slot).and_then(Option::as_ref));
        let off = fighter
            .equipment
            .as_ref()
            .and_then(|equipment| equipment.off_hand.as_ref());
        let mut submissions = Vec::new();
        let mut item_atlas = None;
        if let Some(item) = main
            && let Some((mesh, location)) = self.mesh_for(item)
        {
            let consume = gameplay::item_use::classify::is_consumed(&item.name)
                .then(|| animation::use_ticks(&self.equipment, &item.name))
                .flatten();
            let hand = equipment_display::hand_progress(
                animated.hand.map(|hand| equipment_display::HandProgress {
                    attack_time: hand.attack_time,
                    arm_height: hand.arm_height,
                    use_ticks: hand.use_ticks,
                }),
                consume,
                partial_tick.clamp(0.0, 1.0),
            );
            if let Some(bone) =
                equipment_display::view_bone(equipment_display::first_person_display(
                    FirstPersonShape::Sprite {
                        mirrored_art: render_model::equipment::is_rod(&item.name),
                    },
                    hand,
                ))
            {
                let mut held = body.clone();
                held.input.identity.layer = equipment_display::LAYER_MAIN_HAND;
                held.input.rig = mesh;
                held.input.previous_bones = Arc::from([bone]);
                held.input.current_bones = Arc::from([bone]);
                held.world_from_actor = equipment_display::hand_view_placement(motion);
                held.texture_layer = location.layer() | render::HAND_ITEM_LAYER_FLAG;
                if let Some(page) = usize::from(location.page())
                    .checked_sub(1)
                    .and_then(|page| self.artwork.pages().get(page))
                {
                    let (width, height) = page.dimensions();
                    item_atlas = Some(HandItemAtlas {
                        width,
                        height,
                        layers: page.layers(),
                        rgba8: page.shared_pixels(),
                    });
                    submissions.push(held);
                }
            }
        }
        let arms = FirstPersonArms::for_hands(
            main.map(|item| item.name.as_str()),
            off.map(|item| item.name.as_str()),
        )
        .with_undrawn_main(item_atlas.is_some());
        if arms.right || arms.left {
            let mut masked = body;
            masked.input.previous_bones = equipment_display::mask_first_person_bones(
                &names,
                &masked.input.previous_bones,
                arms,
            )
            .into();
            masked.input.current_bones = equipment_display::mask_first_person_bones(
                &names,
                &masked.input.current_bones,
                arms,
            )
            .into();
            submissions.push(masked);
        }
        let frame = self
            .hand_builder
            .build(partial_tick.clamp(0.0, 1.0), None, submissions);
        if scene.publish(
            frame,
            skin_pixels,
            HandRigLight {
                block_level: 0,
                sky_level: 15,
                daylight: 1.0,
                ..HandRigLight::default()
            },
            fov_radians,
            generation,
        ) {
            scene.set_item_atlases([item_atlas, None]);
        }
        scene
    }
}

fn register_rig(
    scene: &mut ActorRenderScene,
    assets: &RuntimeEntityAssets,
    identifier: &str,
    id: EntityRigId,
) -> Result<Rig, String> {
    let index = render_model::find_geometry_index(assets, identifier)
        .ok_or_else(|| format!("compiled geometry {identifier} is missing"))?
        as usize;
    let geometry = render_model::entity_geometry(assets, index, id)
        .map_err(|e| format!("compiled geometry {identifier}: {e:?}"))?;
    register_geometry(scene, assets, identifier, index, geometry)
}

fn register_geometry(
    scene: &mut ActorRenderScene,
    assets: &RuntimeEntityAssets,
    identifier: &str,
    index: usize,
    geometry: ActorRigGeometry,
) -> Result<Rig, String> {
    let id = geometry.id;
    let source = Arc::new(protocol::SkinGeometrySource {
        resource_patch: serde_json::json!({"geometry": {"default": identifier}})
            .to_string()
            .into(),
        geometry_data: Arc::from(""),
        animations: Arc::from([]),
    });
    let names = render_model::geometry_bone_names(assets, index)
        .ok_or_else(|| format!("compiled geometry {identifier} has no bone names"))?;
    scene
        .insert_geometry(geometry.clone())
        .map_err(|e| format!("register geometry {identifier}: {e:?}"))?;
    Ok(Rig {
        id,
        names,
        source,
        geometry,
        bounds: assets::SkinGeometryBounds::default(),
    })
}

fn hash_id(value: &str) -> u64 {
    u64::from_le_bytes(
        Sha256::digest(value)[..8]
            .try_into()
            .expect("SHA-256 prefix"),
    )
    .max(1)
}
fn parse_rgb(value: &str) -> Option<u32> {
    u32::from_str_radix(value.strip_prefix('#').unwrap_or(value), 16)
        .ok()
        .filter(|rgb| *rgb <= 0x00ff_ffff)
}
