use std::sync::Arc;

use bevy::{
    math::{Mat4, Vec3, Vec4},
    prelude::Resource,
    render::extract_resource::ExtractResource,
};
use render_api::SkinRgba8;
use render_model::{
    ActorRigGeometry, ActorRigGeometryError, ActorSkinPixels, EntityRigId, MAX_RENDERED_PLAYERS,
    RenderBoneTransform, default_actor_skin_rgba8, normalize_actor_skin, pack_geometries,
};

#[path = "actor/artwork.rs"]
mod artwork;
pub use artwork::{
    ActorArtworkLocation, ActorArtworkPageId, ActorArtworkPages, ActorTexturePage, EquipmentRaster,
    MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
};
#[path = "actor/glint.rs"]
mod glint;
#[path = "actor/gpu.rs"]
pub(crate) mod gpu;
pub(crate) mod material;
pub use glint::ActorGlint;
mod pipeline_readiness;
pub use pipeline_readiness::ActorPipelineReadiness;
#[path = "actor/rig.rs"]
mod rig;
#[path = "actor/skin_slots.rs"]
mod skin_slots;
#[path = "actor/witness.rs"]
mod witness;

pub use gpu::{
    ActorDrawFrame, ActorPresentationGate, ActorPresentedFrameAck,
    MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS,
};
pub use rig::{
    ACTOR_BONE_MATRIX_BYTES, ACTOR_GPU_INSTANCE_WORDS, ACTOR_LAYER_BODY, ActorDrawManifestEntry,
    ActorGpuInstance, ActorMaterial, ActorRenderIdentity, ActorRigFrameBuilder,
    ActorRigGeometrySpan, ActorRigRejects, ActorRigRenderFrame, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission, ActorRigVertexSegments, IDENTITY_UV_ANIM, MAX_ACTOR_BONE_ARENA_BYTES,
    MAX_ACTOR_POSE_BONES, MAX_ACTOR_RENDER_INSTANCES, actor_bounds_are_visible,
    actor_rig_submission_is_visible, pack_actor_light, pack_actor_light_without_lightmap,
    pack_overlay_rgba8,
};
pub use skin_slots::{ActorSkinResidency, ResidentSkin, pack_skin_slot};
pub(crate) use witness::{
    ActorDrawWitness, ActorPrepareWitness, ActorQueueWitness, ActorSubmitWitness,
};
pub use witness::{ActorMainWitness, ActorRuntimeWitness};

pub const MAX_ACTOR_RENDER_DISTANCE_BLOCKS: f32 = 192.0;
/// Vanilla gathers non-player render candidates no farther than this from the camera on any
/// axis (`min(radius, 72)`); players are added apart.
pub const ACTOR_CANDIDATE_RADIUS_BLOCKS: f32 = 72.0;

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

impl ActorCullView {
    /// Tests actor distance independently of model scale, failing open for invalid views.
    pub fn contains_distance(&self, feet: [f32; 3]) -> bool {
        if !self.is_valid() {
            return true;
        }
        let distance = (Vec3::from_array(feet) + Vec3::Y).distance_squared(self.camera_position);
        distance.partial_cmp(&(self.max_distance * self.max_distance))
            != Some(std::cmp::Ordering::Greater)
    }

    /// Whether the view can safely reject an actor using distance or clip bounds.
    fn is_valid(&self) -> bool {
        self.clip_from_world.is_finite()
            && self.camera_position.is_finite()
            && self.max_distance.is_finite()
            && self.max_distance > 0.0
    }
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
    pub skins: Arc<ActorSkinResidency>,
    pub instance_revision: u64,
    pub skin_revision: u64,
    pub rig: ActorRigRenderFrame,
    pub(crate) artwork: Arc<ActorArtworkPages>,
    pub(crate) instance_pages: Arc<[ActorArtworkPageId]>,
}

impl ActorRenderFrame {
    /// The standard raster a page-0 instance's texture layer samples.
    #[must_use]
    pub fn player_skin(&self, texture_layer: u32) -> Option<&SkinRgba8> {
        self.skins
            .resident(texture_layer)
            .map(|resident| &resident.skin)
    }

    /// Texture bytes the player skin arrays allocate.
    #[must_use]
    pub fn skin_bytes(&self) -> usize {
        self.skins.allocated_bytes()
    }
    /// The artwork page each rig instance samples; 0 is the player-skin array.
    #[must_use]
    pub fn instance_pages(&self) -> &[ActorArtworkPageId] {
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
            skins: Arc::default(),
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
    skin_slots: skin_slots::SkinSlots,
}

impl Default for ActorRenderScene {
    fn default() -> Self {
        Self {
            frame: ActorRenderFrame::default(),
            rig_builder: ActorRigFrameBuilder::new([])
                .expect("authored diagnostic actor geometry is valid"),
            skin_slots: Default::default(),
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
            skin_slots: Default::default(),
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
            skin_slots: Default::default(),
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
        self.replace_pack_entity_geometries(assets.map(pack_geometries).unwrap_or_default())
    }

    /// Publishes meshes prepared by the pack worker without rebuilding their vertices.
    pub fn replace_pack_entity_geometries(
        &mut self,
        geometries: Vec<ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
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
        self.replace_prepared_session_geometries(
            assets.map(pack_geometries).unwrap_or_default(),
            equipment,
        )
    }

    /// Atomically installs prebuilt entity and equipment ranges under the existing admission rules.
    pub fn replace_prepared_session_geometries(
        &mut self,
        geometries: Vec<ActorRigGeometry>,
        equipment: Vec<ActorRigGeometry>,
    ) -> (
        Result<(), ActorRigGeometryError>,
        Result<(), ActorRigGeometryError>,
    ) {
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
        let mut skins = Vec::with_capacity(visible.len());
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
                axis_scale: render_model::UNIT_AXIS_SCALE,
            });
            let mut posed_bones = bones;
            posed_bones[0].rotation = head_rotation;
            let yaw = wrap_degrees(pose.yaw_degrees).to_radians();
            let (sine, cosine) = yaw.sin_cos();
            rig_submissions.push(ActorRigSubmission {
                material: Default::default(),
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
            skins.push(normalize_skin(source.skin.as_ref()));
        }
        let slots = self.skin_slots.assign(&skins);
        for (instance, submission) in instances.iter_mut().zip(&mut rig_submissions) {
            instance.skin_layer = slots[instance.skin_layer as usize];
            submission.texture_layer = instance.skin_layer;
        }

        if self.frame.instances.as_ref() != instances.as_slice() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from(instances);
        }
        self.publish_skins();
        self.frame.rig = self.rig_builder.build(1.0, None, rig_submissions);
        self.frame.instance_pages = vec![0; self.frame.rig.instances.len()].into();
        &self.frame
    }

    pub fn update_rigs(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        submissions: impl IntoIterator<Item = ActorRigSubmission>,
        skins: &[SkinRgba8],
    ) -> &ActorRenderFrame {
        self.update_rigs_with_artwork(
            partial_tick,
            view,
            submissions,
            skins,
            &std::collections::HashMap::new(),
        )
    }

    pub fn update_rigs_with_artwork(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        submissions: impl IntoIterator<Item = ActorRigSubmission>,
        skins: &[SkinRgba8],
        assignments: &std::collections::HashMap<ActorRenderIdentity, ActorArtworkLocation>,
    ) -> &ActorRenderFrame {
        let skins_are_valid = skins.len() <= MAX_RENDERED_PLAYERS
            && skins
                .iter()
                .all(|skin| render_model::actor_skin_side(skin).is_some());
        let skin_layer_count = skins.len();
        if skins_are_valid {
            // Frame-local skin indices become stable slots, so a visible-set change uploads nothing.
            self.skin_slots.assign(skins);
            self.publish_skins();
        }
        let artwork = &self.frame.artwork;
        let mut invalid_references = 0u64;
        let slots = self.skin_slots.assigned();
        let submissions = submissions.into_iter().filter_map(|mut submission| {
            let artwork_location = assignments.get(&submission.input.identity).copied();
            let valid = submission.route == ActorRigRoute::NoDraw
                || if let Some(location) = artwork_location {
                    artwork.valid(submission.input.rig, location)
                        && submission.texture_layer == location.layer
                } else {
                    (submission.texture_layer as usize) < skin_layer_count
                };
            if !valid {
                invalid_references = invalid_references.saturating_add(1);
                return None;
            }
            if skins_are_valid
                && submission.route != ActorRigRoute::NoDraw
                && artwork_location.is_none()
            {
                submission.texture_layer = slots[submission.texture_layer as usize];
            }
            Some((submission, artwork_location))
        });
        // Each submission's location is looked up once and travels with it.
        let mut rig =
            self.rig_builder
                .build_located(partial_tick, view, submissions, |_, location| {
                    location.map_or(0, |location| location.page)
                });
        rig.rejects.invalid_geometry = rig
            .rejects
            .invalid_geometry
            .saturating_add(invalid_references);
        let instance_pages: Vec<_> = self
            .rig_builder
            .instance_locations()
            .iter()
            .map(|location| location.map_or(0, |location| location.page))
            .collect();
        if !skins_are_valid {
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
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
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
        self.frame.rig = rig;
        self.frame.instance_pages = instance_pages.into();
        &self.frame
    }

    #[must_use]
    pub fn frame(&self) -> &ActorRenderFrame {
        &self.frame
    }

    fn publish_skins(&mut self) {
        let residency = self.skin_slots.residency();
        if !Arc::ptr_eq(&self.frame.skins, residency) {
            self.frame.skins = Arc::clone(residency);
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
        }
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

fn normalize_skin(skin: Option<&ActorSkinPixels>) -> SkinRgba8 {
    skin.and_then(normalize_actor_skin)
        .unwrap_or_else(default_actor_skin_rgba8)
}

#[cfg(test)]
#[path = "actor/tests.rs"]
mod tests;

#[cfg(test)]
use render_model::STANDARD_SKIN_BYTES;
