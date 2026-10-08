//! Stream adapters for Cinnabar's compiled actor geometry and GPU publication.
//! Geometry, skin normalization, armor UVs and artwork pages retain native owners.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use assets::{EquipmentCategory, RuntimeEntityAssets, RuntimeEquipmentCatalog, RuntimeIconCatalog};
use bevy::math::{Mat4, Vec3};
use render::{
    ActorArtworkLocation, ActorArtworkPages, ActorRenderFrame, ActorRenderIdentity,
    ActorRenderScene, ActorRigFrameBuilder, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission,
    HandItemAtlas, HandRigLight, HandRigScene,
};
use render_api::SkinRgba8;
use render_model::{ActorRigGeometry, ActorSkinPixels, EntityRigId, equipment::EquipmentRaster};
use sha2::{Digest, Sha256};

use super::browser_model::{Fighter, Frame, Item};
use view_presentation::equipment_display::{self, FirstPersonArms, FirstPersonShape};
use view_presentation::equipment_sprite_atlas::{Placement, SpriteAtlas};

mod animation;
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
}

pub(super) struct BrowserActors {
    scene: ActorRenderScene,
    equipment: RuntimeEquipmentCatalog,
    classic: Rig,
    slim: Option<Rig>,
    armor: BTreeMap<String, Rig>,
    textures: BTreeMap<String, ActorArtworkLocation>,
    skins: BTreeMap<String, Skin>,
    skin_keys: Vec<String>,
    render_skins: Vec<SkinRgba8>,
    generation: u64,
    icons: RuntimeIconCatalog,
    placements: Vec<Option<Placement>>,
    item_locations: Vec<Option<ActorArtworkLocation>>,
    meshes: BTreeMap<usize, EntityRigId>,
    hand_builder: ActorRigFrameBuilder,
    artwork: ActorArtworkPages,
    animator: NativeAnimator,
}

impl BrowserActors {
    pub(super) fn new(
        entity_bytes: &[u8],
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
        let (artwork, locations) = ActorArtworkPages::default().with_equipment_rasters(&rasters);
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
            slim,
            armor,
            textures,
            skins: BTreeMap::new(),
            skin_keys: Vec::new(),
            render_skins: Vec::new(),
            generation: 0,
            icons,
            placements: atlas.placements,
            item_locations,
            meshes: BTreeMap::new(),
            hand_builder,
            artwork,
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

    pub(super) fn update(
        &mut self,
        current: &Frame,
        previous: Option<&Frame>,
        partial_tick: f32,
        hidden_player: Option<&str>,
    ) -> ActorRenderFrame {
        self.generation = self.generation.wrapping_add(1).max(1);
        let fraction = if partial_tick.is_finite() {
            partial_tick.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let classic = Arc::clone(&self.classic.source);
        let slim_source = self.slim.as_ref().map(|rig| Arc::clone(&rig.source));
        let skins = &self.skins;
        self.animator.advance(
            current,
            previous,
            fraction,
            hidden_player,
            &self.equipment,
            |id| {
                skins.get(id).map(|skin| {
                    if skin.slim {
                        slim_source.as_ref().unwrap_or(&classic).clone()
                    } else {
                        classic.clone()
                    }
                })
            },
        );
        let animation_fraction = self.animator.partial_tick();
        let visible = current
            .fighters
            .iter()
            .filter(|fighter| !fighter.dead && hidden_player != Some(fighter.id.as_str()))
            .filter_map(|fighter| {
                let key = fighter.id.as_str();
                self.skins
                    .get(key)
                    .map(|skin| (fighter, key.to_owned(), skin.slim))
            })
            .collect::<Vec<_>>();
        let keys = visible
            .iter()
            .map(|(_, key, _)| key.clone())
            .collect::<Vec<_>>();
        if self.skin_keys != keys {
            self.render_skins = keys
                .iter()
                .map(|key| self.skins[key].pixels.clone())
                .collect();
            self.skin_keys = keys;
        }
        let mut submissions = Vec::new();
        let mut assignments = HashMap::new();
        for (layer, (fighter, _, slim)) in visible.iter().enumerate() {
            let rig = if *slim {
                self.slim.as_ref().unwrap_or(&self.classic)
            } else {
                &self.classic
            };
            let old = previous
                .and_then(|frame| {
                    frame
                        .fighters
                        .iter()
                        .find(|old| old.id == fighter.id && old.dead == fighter.dead)
                })
                .unwrap_or(fighter);
            let Some(animated) = self.animator.pose(&fighter.id, rig.id, &rig.names) else {
                continue;
            };
            let previous_bones = animated.previous;
            let current_bones = animated.current;
            let identity = ActorRenderIdentity {
                session_id: hash_id(&current.id),
                dimension: 0,
                runtime_id: hash_id(&fighter.id),
                spawn_revision: 1,
                ingress_sequence: self.generation,
                source_tick: Some(self.generation),
                movement_revision: self.generation,
                pose_generation: animated.completed_tick,
                layer: render::ACTOR_LAYER_BODY,
            };
            let yaw_delta =
                (animated.body_yaw - animated.previous_body_yaw + 180.0).rem_euclid(360.0) - 180.0;
            let position =
                Vec3::from_array(old.position).lerp(Vec3::from_array(fighter.position), fraction);
            let body = ActorRigSubmission {
                material: render::ActorMaterial::default(),
                culling_bounds: assets::SkinGeometryBounds::default(),
                input: ActorRigRenderInput {
                    identity,
                    rig: rig.id,
                    previous_bones,
                    current_bones,
                    completed_tick: animated.completed_tick,
                    reset_generation: animated.reset_generation,
                },
                world_from_actor: equipment_display::rig_world_from_actor(
                    position.to_array(),
                    animated.previous_body_yaw + yaw_delta * animation_fraction,
                    animated.scale,
                ),
                texture_layer: layer as u32,
                route: ActorRigRoute::Compiled,
                tint: 0,
                overlay_rgba8: animated.overlay,
                uv_anim: render::IDENTITY_UV_ANIM,
                light: render::pack_actor_light(0, 15),
            };
            if let Some(equipment) = &fighter.equipment {
                for (slot, item) in equipment.armour.iter().enumerate() {
                    let Some(item) = item else {
                        continue;
                    };
                    let Some(binding) = self.equipment.binding(&item.name) else {
                        continue;
                    };
                    let Some(armor) = self.armor.get(binding.geometry.identifier.as_ref()) else {
                        continue;
                    };
                    let Some(location) = self
                        .textures
                        .get(binding.texture.identifier.as_ref())
                        .copied()
                    else {
                        continue;
                    };
                    let map = view_presentation::armor_pose::bone_map(&armor.names, &rig.names);
                    let mut armor_body = body.clone();
                    armor_body.input.identity.layer = equipment_display::LAYER_HELMET + slot as u8;
                    armor_body.input.rig = armor.id;
                    armor_body.input.previous_bones =
                        view_presentation::armor_pose::remap_pose(&map, &body.input.previous_bones)
                            .into();
                    armor_body.input.current_bones =
                        view_presentation::armor_pose::remap_pose(&map, &body.input.current_bones)
                            .into();
                    armor_body.texture_layer = location.layer();
                    armor_body.tint = if binding.material.contains("leather") {
                        view_presentation::armor_pose::pack_tint(
                            item.color
                                .as_deref()
                                .and_then(parse_rgb)
                                .unwrap_or(assets::DEFAULT_LEATHER_RGB),
                        )
                    } else {
                        0
                    };
                    assignments.insert(armor_body.input.identity, location);
                    submissions.push(armor_body);
                }
            }
            let hand_bones = ["rightItem", "leftItem"].map(|name| {
                rig.names
                    .iter()
                    .position(|bone| bone.eq_ignore_ascii_case(name))
            });
            if let Some(equipment) = &fighter.equipment {
                for (hand, item) in [&equipment.main_hand, &equipment.off_hand]
                    .into_iter()
                    .enumerate()
                {
                    if let Some(item) = item
                        && let Some((layer, location)) = self.held_layer(
                            &body,
                            item,
                            hand_bones[hand],
                            equipment_display::LAYER_MAIN_HAND + hand as u8,
                        )
                    {
                        assignments.insert(layer.input.identity, location);
                        submissions.push(layer);
                    }
                }
            }
            submissions.push(body);
        }
        self.scene
            .update_rigs_with_artwork(
                animation_fraction,
                None,
                submissions,
                &self.render_skins,
                &assignments,
            )
            .clone()
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
        let rig = if skin.slim {
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
                animated.hand.map(|phase| equipment_display::HandProgress {
                    attack_time: phase.attack_time,
                    arm_height: phase.arm_height,
                    use_ticks: phase.use_ticks,
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
