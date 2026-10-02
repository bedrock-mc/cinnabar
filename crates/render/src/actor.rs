use std::sync::{Arc, Mutex, OnceLock};

use bevy::{
    math::{Mat4, Vec3, Vec4},
    prelude::Resource,
    render::extract_resource::ExtractResource,
};
use bytemuck::{Pod, Zeroable};

#[path = "actor/artwork.rs"]
mod artwork;
#[path = "actor/asset_geometry.rs"]
mod asset_geometry;
#[path = "actor/texture_mesh.rs"]
mod texture_mesh;
pub use texture_mesh::attachable_geometry;
#[path = "actor/geometry.rs"]
mod geometry;
#[path = "actor/skin_poly_mesh.rs"]
mod skin_poly_mesh;
pub use artwork::{
    ActorArtworkLocation, ActorArtworkPages, ActorTexturePage, EquipmentRaster,
    MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
};
#[cfg(test)]
pub(crate) use geometry::ONE_SIDED_BACK_UV;
pub use item_mesh::{extruded_sprite_vertices, held_sprite_vertices, textured_cube_vertices};
#[path = "actor/gpu.rs"]
pub(crate) mod gpu;
#[path = "actor/item_mesh.rs"]
mod item_mesh;
#[path = "actor/rig.rs"]
mod rig;
#[path = "actor/witness.rs"]
mod witness;

pub use asset_geometry::{
    entity_geometry, equipment_geometry, find_geometry_index, geometry_bone_names,
    geometry_bone_pivots, skin_geometry, skull_geometry,
};
pub use gpu::{
    ActorDrawFrame, ActorPresentationGate, ActorPresentedFrameAck,
    MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS,
};
pub use rig::{
    ACTOR_BONE_MATRIX_BYTES, ACTOR_GPU_INSTANCE_WORDS, ACTOR_LAYER_BODY, ActorDrawManifestEntry,
    ActorGpuInstance, ActorRenderIdentity, ActorRigFrameBuilder, ActorRigGeometry,
    ActorRigGeometryError, ActorRigGeometrySpan, ActorRigRejects, ActorRigRenderFrame,
    ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, ActorRigVertex, ActorRigVertexSegments,
    EntityRigId, IDENTITY_UV_ANIM, MAX_ACTOR_BONE_ARENA_BYTES, MAX_ACTOR_RENDER_INSTANCES,
    MAX_ACTOR_RIG_VERTICES, MAX_RENDER_BONES_PER_ACTOR, RenderBoneTransform, UNIT_AXIS_SCALE,
    actor_bounds_are_visible, actor_rig_submission_is_visible, equipment_rig_id, item_mesh_rig_id,
    layer_geometry_rig_id, pack_actor_light, pack_equipment_rig_id, pack_overlay_rgba8,
    pack_rig_id, skin_rig_id,
};
pub(crate) use witness::{
    ActorDrawWitness, ActorPrepareWitness, ActorQueueWitness, ActorSubmitWitness,
};
pub use witness::{ActorMainWitness, ActorRuntimeWitness};

pub const MAX_RENDERED_PLAYERS: usize = 128;
pub const MAX_ACTOR_RENDER_DISTANCE_BLOCKS: f32 = 192.0;
/// Vanilla gathers non-player render candidates no farther than this from the camera on any
/// axis (`LevelRendererCamera::queueRenderEntities`, `min(radius, 72)`); players are added apart.
pub const ACTOR_CANDIDATE_RADIUS_BLOCKS: f32 = 72.0;
/// Classic skin UV layouts use this many texels per side regardless of image resolution.
const CLASSIC_SKIN_SIDE: usize = client_world::CLASSIC_SKIN_SIDE;
/// The shared player array preserves every texel of every admitted skin resolution.
pub const STANDARD_SKIN_SIDE: usize = client_world::MAX_STANDARD_SKIN_SIDE as usize;
pub const STANDARD_SKIN_BYTES: usize = STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE * 4;
pub const STANDARD_BIPED_VERTEX_COUNT: usize = 6 * 6 * 6;
pub const DEFAULT_SKIN_PROVENANCE: &str = "locally generated Cinnabar Default skin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorSkinPixels {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorRenderSource {
    pub runtime_id: u64,
    pub unique_id: i64,
    pub spawn_revision: u64,
    pub movement_revision: u64,
    pub previous_position: [f32; 3],
    pub previous_pitch_degrees: f32,
    pub previous_yaw_degrees: f32,
    pub previous_head_yaw_degrees: f32,
    pub position: [f32; 3],
    pub pitch_degrees: f32,
    pub yaw_degrees: f32,
    pub head_yaw_degrees: f32,
    pub teleported: bool,
    pub skin: Option<ActorSkinPixels>,
}

impl ActorRenderSource {
    fn is_finite(&self) -> bool {
        self.previous_position
            .iter()
            .chain(&self.position)
            .all(|value| value.is_finite())
            && self.previous_pitch_degrees.is_finite()
            && self.previous_yaw_degrees.is_finite()
            && self.previous_head_yaw_degrees.is_finite()
            && self.pitch_degrees.is_finite()
            && self.yaw_degrees.is_finite()
            && self.head_yaw_degrees.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorCullView {
    pub clip_from_world: Mat4,
    pub camera_position: Vec3,
    pub max_distance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorRenderInstance {
    pub runtime_id: u64,
    pub position: [f32; 3],
    pub pitch_radians: f32,
    pub yaw_radians: f32,
    pub head_yaw_radians: f32,
    pub skin_layer: u32,
}

#[derive(Debug, Clone, Resource, ExtractResource)]
pub struct ActorRenderFrame {
    pub instances: Arc<[ActorRenderInstance]>,
    pub skins_rgba8: Arc<[u8]>,
    pub instance_revision: u64,
    pub skin_revision: u64,
    pub rig: ActorRigRenderFrame,
    pub(crate) artwork: Arc<ActorArtworkPages>,
    pub(crate) instance_pages: Arc<[u8]>,
}

#[cfg(feature = "publication-test-support")]
impl ActorRenderFrame {
    /// The artwork page each rig instance samples; 0 is the player-skin array.
    #[must_use]
    pub fn instance_pages(&self) -> &[u8] {
        &self.instance_pages
    }

    /// Artwork sampled by this exact published frame, including transient skin animation pages.
    #[must_use]
    pub fn artwork_pages(&self) -> &ActorArtworkPages {
        &self.artwork
    }
}

impl Default for ActorRenderFrame {
    fn default() -> Self {
        Self {
            instances: Arc::from([]),
            skins_rgba8: Arc::from([]),
            instance_revision: 0,
            skin_revision: 0,
            rig: ActorRigRenderFrame::default(),
            artwork: Arc::new(ActorArtworkPages::default()),
            instance_pages: Arc::from([]),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Pose {
    position: [f32; 3],
    pitch_degrees: f32,
    yaw_degrees: f32,
    head_yaw_degrees: f32,
}

impl Pose {
    fn sample(source: &ActorRenderSource, alpha: f32) -> Self {
        if source.teleported {
            return Self {
                position: source.position,
                pitch_degrees: source.pitch_degrees,
                yaw_degrees: source.yaw_degrees,
                head_yaw_degrees: source.head_yaw_degrees,
            };
        }
        Self {
            position: std::array::from_fn(|axis| {
                source.previous_position[axis]
                    + (source.position[axis] - source.previous_position[axis]) * alpha
            }),
            pitch_degrees: lerp_degrees(source.previous_pitch_degrees, source.pitch_degrees, alpha),
            yaw_degrees: lerp_degrees(source.previous_yaw_degrees, source.yaw_degrees, alpha),
            head_yaw_degrees: lerp_degrees(
                source.previous_head_yaw_degrees,
                source.head_yaw_degrees,
                alpha,
            ),
        }
    }
}

#[derive(Debug, Resource)]
pub struct ActorRenderScene {
    frame: ActorRenderFrame,
    rig_builder: ActorRigFrameBuilder,
}

impl Default for ActorRenderScene {
    fn default() -> Self {
        Self {
            frame: ActorRenderFrame::default(),
            rig_builder: ActorRigFrameBuilder::new([])
                .expect("authored diagnostic actor geometry is valid"),
        }
    }
}

impl ActorRenderScene {
    pub fn configure_artwork(&mut self, artwork: ActorArtworkPages) {
        self.reset();
        self.frame.artwork = Arc::new(artwork);
    }
    pub fn with_runtime_entity_assets(
        assets: &assets::RuntimeEntityAssets,
    ) -> Result<Self, ActorRigGeometryError> {
        Ok(Self {
            frame: ActorRenderFrame::default(),
            rig_builder: ActorRigFrameBuilder::from_runtime_assets(assets)?,
        })
    }

    /// Like [`Self::with_runtime_entity_assets`], also registering equipment geometries by
    /// entity-catalog geometry index under [`equipment_rig_id`].
    pub fn with_runtime_entity_assets_and_equipment(
        assets: &assets::RuntimeEntityAssets,
        equipment_geometries: &[u32],
    ) -> Result<Self, ActorRigGeometryError> {
        Ok(Self {
            frame: ActorRenderFrame::default(),
            rig_builder: ActorRigFrameBuilder::from_runtime_assets_with_equipment(
                assets,
                equipment_geometries,
            )?,
        })
    }

    /// Registers or replaces one geometry, such as a generated item mesh.
    pub fn insert_geometry(
        &mut self,
        geometry: ActorRigGeometry,
    ) -> Result<(), ActorRigGeometryError> {
        self.rig_builder.insert_geometry(geometry)
    }

    /// Registers several geometries under one catalog rebuild; on error none is registered.
    pub fn insert_geometries(
        &mut self,
        geometries: Vec<ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
        self.rig_builder.insert_geometries(geometries)
    }

    #[must_use]
    pub fn contains_geometry(&self, id: EntityRigId) -> bool {
        self.rig_builder.contains_geometry(id)
    }

    pub fn replace_runtime_entity_assets(
        &mut self,
        assets: &assets::RuntimeEntityAssets,
    ) -> Result<(), ActorRigGeometryError> {
        let replacement = ActorRigFrameBuilder::from_runtime_assets(assets)?;
        self.rig_builder = replacement;
        self.frame = ActorRenderFrame {
            artwork: Arc::clone(&self.frame.artwork),
            ..ActorRenderFrame::default()
        };
        Ok(())
    }

    /// Registers pack equipment geometries under pack equipment rig ids, replacing the
    /// previous session's; an empty list removes them.
    pub fn replace_pack_equipment(
        &mut self,
        geometries: Vec<ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
        self.rig_builder
            .replace_pack_equipment_geometries(geometries)
    }

    /// Registers the geometry of a session's server-pack entity catalog under pack rig
    /// ids, replacing the previous session's; `None` removes them.
    pub fn replace_pack_entities(
        &mut self,
        assets: Option<&assets::RuntimeEntityAssets>,
    ) -> Result<(), ActorRigGeometryError> {
        let geometries = assets
            .map(asset_geometry::pack_geometries)
            .unwrap_or_default();
        self.rig_builder.replace_pack_geometries(geometries)?;
        self.frame = ActorRenderFrame {
            artwork: Arc::clone(&self.frame.artwork),
            ..ActorRenderFrame::default()
        };
        Ok(())
    }

    /// Publishes both session ranges once, keeping each range's existing failure behavior.
    pub fn replace_session_pack_geometries(
        &mut self,
        assets: Option<&assets::RuntimeEntityAssets>,
        equipment: Vec<ActorRigGeometry>,
    ) -> (
        Result<(), ActorRigGeometryError>,
        Result<(), ActorRigGeometryError>,
    ) {
        let geometries = assets
            .map(asset_geometry::pack_geometries)
            .unwrap_or_default();
        let results = self
            .rig_builder
            .replace_session_pack_geometries(geometries, equipment);
        if results.0.is_ok() {
            self.frame = ActorRenderFrame::default();
        }
        results
    }

    pub fn reset(&mut self) {
        self.frame.instance_pages = Arc::from([]);
        if !self.frame.instances.is_empty() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from([]);
        }
        if !self.frame.skins_rgba8.is_empty() {
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            self.frame.skins_rgba8 = Arc::from([]);
        }
        self.frame.rig = self.rig_builder.build(0.0, None, []);
    }

    pub fn update(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        sources: impl IntoIterator<Item = ActorRenderSource>,
    ) -> &ActorRenderFrame {
        self.update_with_local(partial_tick, view, sources, None)
    }

    pub fn update_with_local(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        sources: impl IntoIterator<Item = ActorRenderSource>,
        local: Option<ActorRenderSource>,
    ) -> &ActorRenderFrame {
        let partial_tick = if partial_tick.is_finite() {
            partial_tick.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let local = local.filter(ActorRenderSource::is_finite);
        let mut sources = sources
            .into_iter()
            .filter(ActorRenderSource::is_finite)
            .filter(|source| {
                local
                    .as_ref()
                    .is_none_or(|local| source.runtime_id != local.runtime_id)
            })
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|source| source.runtime_id);
        sources.dedup_by_key(|source| source.runtime_id);
        let remote_capacity = if local.is_some() {
            MAX_RENDERED_PLAYERS.saturating_sub(1)
        } else {
            MAX_RENDERED_PLAYERS
        };
        let mut visible = sources
            .into_iter()
            .filter_map(|source| {
                let pose = Pose::sample(&source, partial_tick);
                actor_is_visible(&pose, view).then_some((source, pose))
            })
            .take(remote_capacity)
            .collect::<Vec<_>>();
        if let Some(local) = local {
            let pose = Pose::sample(&local, partial_tick);
            visible.push((local, pose));
        }

        let mut instances = Vec::with_capacity(visible.len());
        let mut skins = Vec::with_capacity(visible.len() * STANDARD_SKIN_BYTES);
        let mut rig_submissions = Vec::with_capacity(visible.len());
        for (source, pose) in visible {
            let skin_layer = u32::try_from(instances.len()).expect("bounded actor layer count");
            instances.push(ActorRenderInstance {
                runtime_id: source.runtime_id,
                position: pose.position,
                pitch_radians: wrap_degrees(pose.pitch_degrees).to_radians(),
                yaw_radians: wrap_degrees(pose.yaw_degrees).to_radians(),
                head_yaw_radians: wrap_degrees(pose.head_yaw_degrees).to_radians(),
                skin_layer,
            });
            let head_rotation = quaternion_from_euler_degrees([
                wrap_degrees(pose.pitch_degrees),
                wrap_degrees(pose.head_yaw_degrees - pose.yaw_degrees),
                0.0,
            ]);
            let pivots = [
                [0.0, 1.5, 0.0],
                [0.0, 1.5, 0.0],
                [-0.3125, 1.375, 0.0],
                [-0.11875, 0.75, 0.0],
                [0.3125, 1.375, 0.0],
                [0.11875, 0.75, 0.0],
            ];
            let bones = pivots.map(|pivot| RenderBoneTransform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation_scale: [pivot[0], pivot[1], pivot[2], 1.0],
                axis_scale: rig::UNIT_AXIS_SCALE,
            });
            let mut posed_bones = bones;
            posed_bones[0].rotation = head_rotation;
            let yaw = wrap_degrees(pose.yaw_degrees).to_radians();
            let (sine, cosine) = yaw.sin_cos();
            rig_submissions.push(ActorRigSubmission {
                culling_bounds: Default::default(),
                input: ActorRigRenderInput {
                    identity: ActorRenderIdentity {
                        session_id: 0,
                        dimension: 0,
                        runtime_id: source.runtime_id,
                        spawn_revision: source.spawn_revision,
                        ingress_sequence: source.movement_revision,
                        source_tick: None,
                        movement_revision: source.movement_revision,
                        pose_generation: source.movement_revision,
                        layer: ACTOR_LAYER_BODY,
                    },
                    rig: EntityRigId(u32::MAX),
                    previous_bones: Arc::from(posed_bones),
                    current_bones: Arc::from(posed_bones),
                    completed_tick: source.movement_revision,
                    reset_generation: source.spawn_revision.max(1),
                },
                world_from_actor: [
                    [cosine, 0.0, sine, pose.position[0]],
                    [0.0, 1.0, 0.0, pose.position[1]],
                    [-sine, 0.0, cosine, pose.position[2]],
                ],
                texture_layer: skin_layer,
                route: ActorRigRoute::Diagnostic,
                tint: 0,
                uv_anim: crate::IDENTITY_UV_ANIM,
                light: 0,
                overlay_rgba8: 0,
            });
            skins.extend_from_slice(&normalize_skin(source.skin.as_ref()));
        }

        if self.frame.instances.as_ref() != instances.as_slice() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from(instances);
        }
        if self.frame.skins_rgba8.as_ref() != skins.as_slice() {
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            self.frame.skins_rgba8 = Arc::from(skins);
        }
        self.frame.rig = self.rig_builder.build(1.0, None, rig_submissions);
        self.frame.instance_pages = vec![0; self.frame.rig.instances.len()].into();
        &self.frame
    }

    pub fn update_rigs(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        submissions: impl IntoIterator<Item = ActorRigSubmission>,
        skins_rgba8: Arc<[u8]>,
    ) -> &ActorRenderFrame {
        self.update_rigs_with_artwork(
            partial_tick,
            view,
            submissions,
            skins_rgba8,
            &std::collections::HashMap::new(),
        )
    }

    pub fn update_rigs_with_artwork(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        submissions: impl IntoIterator<Item = ActorRigSubmission>,
        skins_rgba8: Arc<[u8]>,
        assignments: &std::collections::HashMap<ActorRenderIdentity, ActorArtworkLocation>,
    ) -> &ActorRenderFrame {
        let skin_payload_is_aligned = skins_rgba8.len().is_multiple_of(STANDARD_SKIN_BYTES);
        let skin_layer_count = skins_rgba8.len() / STANDARD_SKIN_BYTES;
        let artwork = &self.frame.artwork;
        let mut invalid_references = 0u64;
        let submissions = submissions.into_iter().filter(|submission| {
            let valid = submission.route == ActorRigRoute::NoDraw
                || if let Some(location) = assignments.get(&submission.input.identity) {
                    artwork.valid(submission.input.rig, *location)
                        && submission.texture_layer == location.layer
                } else {
                    (submission.texture_layer as usize) < skin_layer_count
                };
            if !valid {
                invalid_references = invalid_references.saturating_add(1);
            }
            valid
        });
        let mut rig = self
            .rig_builder
            .build_paged(partial_tick, view, submissions, |identity| {
                assignments
                    .get(identity)
                    .map_or(0, |location| location.page)
            });
        rig.rejects.invalid_geometry = rig
            .rejects
            .invalid_geometry
            .saturating_add(invalid_references);
        let instance_pages: Vec<_> = rig
            .manifest
            .iter()
            .map(|entry| {
                assignments
                    .get(&entry.identity)
                    .map_or(0, |location| location.page)
            })
            .collect();
        if !skin_payload_is_aligned || skin_layer_count > MAX_RENDERED_PLAYERS {
            let rejects = rig.rejects;
            self.frame.rig = ActorRigRenderFrame {
                geometry_revision: rig.geometry_revision,
                geometry_vertices: rig.geometry_vertices,
                geometry_spans: rig.geometry_spans,
                frame_generation: rig.frame_generation,
                rejects: ActorRigRejects {
                    invalid_geometry: rejects.invalid_geometry.saturating_add(1),
                    ..rejects
                },
                ..ActorRigRenderFrame::default()
            };
            self.frame.instances = Arc::from([]);
            self.frame.instance_pages = Arc::from([]);
            self.frame.skins_rgba8 = Arc::from([]);
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            return &self.frame;
        }
        let compatibility_instances = rig
            .instances
            .iter()
            .zip(rig.manifest.iter())
            .map(|(instance, manifest)| ActorRenderInstance {
                runtime_id: manifest.identity.runtime_id,
                position: [
                    instance.world_from_actor[0][3],
                    instance.world_from_actor[1][3],
                    instance.world_from_actor[2][3],
                ],
                pitch_radians: 0.0,
                yaw_radians: 0.0,
                head_yaw_radians: 0.0,
                skin_layer: instance.texture_layer,
            })
            .collect::<Vec<_>>();
        if self.frame.instances.as_ref() != compatibility_instances.as_slice() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from(compatibility_instances);
        }
        if !Arc::ptr_eq(&self.frame.skins_rgba8, &skins_rgba8)
            && self.frame.skins_rgba8 != skins_rgba8
        {
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            self.frame.skins_rgba8 = skins_rgba8;
        }
        self.frame.rig = rig;
        self.frame.instance_pages = instance_pages.into();
        &self.frame
    }

    #[must_use]
    pub fn frame(&self) -> &ActorRenderFrame {
        &self.frame
    }
}

fn actor_is_visible(pose: &Pose, view: Option<ActorCullView>) -> bool {
    let Some(view) = view.filter(|view| {
        view.clip_from_world.is_finite()
            && view.camera_position.is_finite()
            && view.max_distance.is_finite()
            && view.max_distance > 0.0
    }) else {
        return true;
    };
    let feet = Vec3::from_array(pose.position);
    let center = feet + Vec3::Y;
    if center.distance_squared(view.camera_position) > view.max_distance * view.max_distance {
        return false;
    }

    const HALF_WIDTH: f32 = 0.5;
    const HEIGHT: f32 = 2.0;
    let corners = [
        Vec3::new(-HALF_WIDTH, 0.0, -HALF_WIDTH),
        Vec3::new(HALF_WIDTH, 0.0, -HALF_WIDTH),
        Vec3::new(-HALF_WIDTH, HEIGHT, -HALF_WIDTH),
        Vec3::new(HALF_WIDTH, HEIGHT, -HALF_WIDTH),
        Vec3::new(-HALF_WIDTH, 0.0, HALF_WIDTH),
        Vec3::new(HALF_WIDTH, 0.0, HALF_WIDTH),
        Vec3::new(-HALF_WIDTH, HEIGHT, HALF_WIDTH),
        Vec3::new(HALF_WIDTH, HEIGHT, HALF_WIDTH),
    ]
    .map(|offset| view.clip_from_world * (feet + offset).extend(1.0));

    !outside_clip_plane(&corners, |clip| clip.x < -clip.w)
        && !outside_clip_plane(&corners, |clip| clip.x > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.y < -clip.w)
        && !outside_clip_plane(&corners, |clip| clip.y > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.z < 0.0)
        && !outside_clip_plane(&corners, |clip| clip.z > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.w <= 0.0)
}

fn outside_clip_plane(corners: &[Vec4; 8], outside: impl Fn(&Vec4) -> bool) -> bool {
    corners.iter().all(outside)
}

fn lerp_degrees(start: f32, end: f32, alpha: f32) -> f32 {
    wrap_degrees(start + wrap_degrees(end - start) * alpha)
}

fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

fn quaternion_from_euler_degrees(rotation: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = rotation.map(|value| value.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

fn normalize_skin(skin: Option<&ActorSkinPixels>) -> Vec<u8> {
    skin.and_then(normalize_actor_skin)
        .unwrap_or_else(default_actor_skin_rgba8)
        .to_vec()
}

/// Pack path of the player entity's default texture, the stand-in for skins that cannot load.
pub const DEFAULT_PLAYER_SKIN_PATH: &str = "textures/entity/steve.png";

static VANILLA_DEFAULT_SKIN: OnceLock<Arc<[u8]>> = OnceLock::new();

/// Installs the classic vanilla default texture once, packing it for the player array.
pub fn install_default_player_skin(skin: Arc<[u8]>) {
    let side = CLASSIC_SKIN_SIDE as u32;
    if let Some(skin) = normalize_actor_skin(&ActorSkinPixels {
        width: side,
        height: side,
        rgba8: skin,
    }) {
        let _ = VANILLA_DEFAULT_SKIN.set(skin);
    }
}

/// The vanilla Steve skin once installed, else a generated diagnostic skin.
#[must_use]
pub fn default_actor_skin_rgba8() -> Arc<[u8]> {
    static GENERATED: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(
        VANILLA_DEFAULT_SKIN
            .get()
            .unwrap_or_else(|| GENERATED.get_or_init(|| generated_default_skin().into())),
    )
}

#[must_use]
pub fn normalize_actor_skin(skin: &ActorSkinPixels) -> Option<Arc<[u8]>> {
    if !skin.width.is_power_of_two()
        || skin.width < CLASSIC_SKIN_SIDE as u32
        || skin.width > client_world::MAX_STANDARD_SKIN_SIDE
        || (skin.height != skin.width && skin.height.checked_mul(2) != Some(skin.width))
    {
        return None;
    }
    let side = usize::try_from(skin.width).expect("bounded standard skin side");
    let height = usize::try_from(skin.height).expect("bounded standard skin height");
    if skin.rgba8.len() != side * height * 4 {
        return None;
    }
    if height != side {
        return normalize_actor_skin(&ActorSkinPixels {
            width: skin.width,
            height: skin.width,
            rgba8: client_world::expand_legacy_skin_rgba8(&skin.rgba8, side).into(),
        });
    }
    if side == STANDARD_SKIN_SIDE {
        return Some(Arc::clone(&skin.rgba8));
    }
    let mut normalized = vec![0; STANDARD_SKIN_BYTES];
    for y in 0..STANDARD_SKIN_SIDE {
        for x in 0..STANDARD_SKIN_SIDE {
            let source_x = x * side / STANDARD_SKIN_SIDE;
            let source_y = y * side / STANDARD_SKIN_SIDE;
            let source = (source_y * side + source_x) * 4;
            let target = (y * STANDARD_SKIN_SIDE + x) * 4;
            normalized[target..target + 4].copy_from_slice(&skin.rgba8[source..source + 4]);
        }
    }
    Some(normalized.into())
}

/// Resampled skins retained by source raster; bounded like the player skin array.
const NORMALIZED_SKIN_CACHE: usize = MAX_RENDERED_PLAYERS;

/// [`normalize_actor_skin`] memoized by source raster, so HD and legacy skins are not resampled
/// every frame. The entry holds its source, so a matched pointer is never a reused allocation.
#[must_use]
pub fn normalize_actor_skin_cached(skin: &ActorSkinPixels) -> Option<Arc<[u8]>> {
    if skin.width as usize == STANDARD_SKIN_SIDE && skin.height == skin.width {
        return normalize_actor_skin(skin);
    }
    type Entry = (Arc<[u8]>, u32, u32, Option<Arc<[u8]>>);
    static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((.., normalized)) = cache.iter().find(|(source, width, height, _)| {
        Arc::ptr_eq(source, &skin.rgba8) && *width == skin.width && *height == skin.height
    }) {
        return normalized.clone();
    }
    let normalized = normalize_actor_skin(skin);
    if cache.len() == NORMALIZED_SKIN_CACHE {
        cache.remove(0);
    }
    cache.push((
        Arc::clone(&skin.rgba8),
        skin.width,
        skin.height,
        normalized.clone(),
    ));
    normalized
}

fn generated_default_skin() -> Vec<u8> {
    let skin_tone = [198, 134, 91, 255];
    let mut rgba8 = skin_tone.repeat(STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE);
    fill_rect(&mut rgba8, 16, 16, 24, 16, [42, 91, 99, 255]);
    fill_rect(&mut rgba8, 0, 16, 16, 16, [47, 54, 67, 255]);
    fill_rect(&mut rgba8, 16, 48, 16, 16, [47, 54, 67, 255]);
    fill_rect(&mut rgba8, 8, 8, 8, 8, [112, 72, 48, 255]);
    // The generated fallback has no authored second-layer clothing. Keep its
    // standard 64x64 overlay regions transparent so the shared outer-layer
    // geometry does not turn the diagnostic skin into an accidental jacket.
    for (x, y, width, height) in [
        (32, 0, 8, 8),
        (16, 32, 8, 12),
        (40, 32, 4, 12),
        (48, 48, 4, 12),
        (0, 32, 4, 12),
        (0, 48, 4, 12),
    ] {
        fill_rect(&mut rgba8, x, y, width, height, [0, 0, 0, 0]);
    }
    rgba8
}

fn fill_rect(rgba8: &mut [u8], x: usize, y: usize, width: usize, height: usize, color: [u8; 4]) {
    let scale = STANDARD_SKIN_SIDE / CLASSIC_SKIN_SIDE;
    for py in y * scale..(y + height) * scale {
        for px in x * scale..(x + width) * scale {
            let offset = (py * STANDARD_SKIN_SIDE + px) * 4;
            rgba8[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ActorVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub part: u32,
}

#[derive(Clone, Copy)]
struct Cuboid {
    min: [f32; 3],
    max: [f32; 3],
    uv_origin: [f32; 2],
    dimensions: [f32; 3],
}

#[must_use]
pub fn standard_biped_vertices() -> Vec<ActorVertex> {
    const P: f32 = 1.0 / 16.0;
    let cuboids = [
        Cuboid {
            min: [-4.0 * P, 24.0 * P, -4.0 * P],
            max: [4.0 * P, 32.0 * P, 4.0 * P],
            uv_origin: [0.0, 0.0],
            dimensions: [8.0, 8.0, 8.0],
        },
        Cuboid {
            min: [-4.0 * P, 12.0 * P, -2.0 * P],
            max: [4.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [16.0, 16.0],
            dimensions: [8.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-8.0 * P, 12.0 * P, -2.0 * P],
            max: [-4.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [40.0, 16.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [4.0 * P, 12.0 * P, -2.0 * P],
            max: [8.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [32.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-4.0 * P, 0.0, -2.0 * P],
            max: [0.0, 12.0 * P, 2.0 * P],
            uv_origin: [0.0, 16.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [0.0, 0.0, -2.0 * P],
            max: [4.0 * P, 12.0 * P, 2.0 * P],
            uv_origin: [16.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
    ];
    let mut vertices = Vec::with_capacity(STANDARD_BIPED_VERTEX_COUNT);
    for (part, cuboid) in cuboids.into_iter().enumerate() {
        append_cuboid(&mut vertices, cuboid, part as u32);
    }
    vertices
}

/// Returns the optional second skin layer (hat, jacket, sleeves, and pants)
/// around the shared base biped. Bedrock and LCE both keep this layer in the
/// same player-model path as the base cuboids; exposing it here lets the HUD
/// preview and first-person carrier use the authoritative skin appearance
/// without duplicating the UV contract in the UI crate.
#[must_use]
pub fn standard_biped_overlay_vertices() -> Vec<ActorVertex> {
    const P: f32 = 1.0 / 16.0;
    const OUTER: f32 = 0.5 * P;
    let cuboids = [
        Cuboid {
            min: [-4.0 * P - OUTER, 24.0 * P - OUTER, -4.0 * P - OUTER],
            max: [4.0 * P + OUTER, 32.0 * P + OUTER, 4.0 * P + OUTER],
            uv_origin: [32.0, 0.0],
            dimensions: [8.0, 8.0, 8.0],
        },
        Cuboid {
            min: [-4.0 * P - OUTER, 12.0 * P - OUTER, -2.0 * P - OUTER],
            max: [4.0 * P + OUTER, 24.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [16.0, 32.0],
            dimensions: [8.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-8.0 * P - OUTER, 12.0 * P - OUTER, -2.0 * P - OUTER],
            max: [-4.0 * P + OUTER, 24.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [40.0, 32.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [4.0 * P - OUTER, 12.0 * P - OUTER, -2.0 * P - OUTER],
            max: [8.0 * P + OUTER, 24.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [48.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-4.0 * P - OUTER, -OUTER, -2.0 * P - OUTER],
            max: [OUTER, 12.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [0.0, 32.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-OUTER, -OUTER, -2.0 * P - OUTER],
            max: [4.0 * P + OUTER, 12.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [0.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
    ];
    let mut vertices = Vec::with_capacity(STANDARD_BIPED_VERTEX_COUNT);
    for (part, cuboid) in cuboids.into_iter().enumerate() {
        append_cuboid(&mut vertices, cuboid, part as u32);
    }
    vertices
}

fn append_cuboid(vertices: &mut Vec<ActorVertex>, cuboid: Cuboid, part: u32) {
    let [x0, y0, z0] = cuboid.min;
    let [x1, y1, z1] = cuboid.max;
    let [u, v] = cuboid.uv_origin;
    let [dx, dy, dz] = cuboid.dimensions;
    let faces = [
        (
            [[x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]],
            [u, v + dz, dz, dy],
        ),
        (
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            [u + dz, v + dz, dx, dy],
        ),
        (
            [[x0, y0, z1], [x0, y0, z0], [x0, y1, z0], [x0, y1, z1]],
            [u + dz + dx, v + dz, dz, dy],
        ),
        (
            [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
            [u + dz + dx + dz, v + dz, dx, dy],
        ),
        (
            [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]],
            [u + dz, v, dx, dz],
        ),
        (
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            [u + dz + dx, v, dx, dz],
        ),
    ];
    for (positions, [face_u, face_v, face_width, face_height]) in faces {
        let u0 = face_u / 64.0;
        let v0 = face_v / 64.0;
        let u1 = (face_u + face_width) / 64.0;
        let v1 = (face_v + face_height) / 64.0;
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        for index in [0, 1, 2, 0, 2, 3] {
            vertices.push(ActorVertex {
                position: positions[index],
                uv: uvs[index],
                part,
            });
        }
    }
}

#[cfg(test)]
#[path = "actor/tests.rs"]
mod tests;
