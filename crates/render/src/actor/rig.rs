use std::{collections::BTreeMap, sync::Arc};

use assets::RuntimeEntityAssets;
use bevy::math::{Vec3, Vec4};
use bytemuck::{Pod, Zeroable};

#[path = "rig/bone_arena.rs"]
mod bone_arena;
#[path = "rig/eligibility.rs"]
mod eligibility;
use bone_arena::PoseMatrixCache;
#[path = "rig/catalog.rs"]
mod catalog;
#[path = "rig/pack.rs"]
mod pack;
pub use catalog::ActorRigVertexSegments;
use catalog::GeometryCatalog;
use render_model::{
    ActorRigGeometry, ActorRigGeometryError, ActorRigVertex, DIAGNOSTIC_RIG_ID, EntityRigId,
    MAX_RENDER_BONES_PER_ACTOR, MAX_RENDERED_PLAYERS, RenderBoneTransform, diagnostic_geometry,
    equipment_rig_id, geometry_from_geometry_index, geometry_from_runtime_assets,
    is_pack_equipment_rig_id, is_pack_rig_id, layer_geometries,
};

use super::{ActorArtworkPageId, ActorCullView};

pub const ACTOR_BONE_MATRIX_BYTES: usize = 48;
/// Existing body/equipment allowance plus every animated skin layer per selected player.
pub const MAX_ACTOR_RENDER_INSTANCES: usize =
    MAX_RENDERED_PLAYERS * (4 + render_api::MAX_SKIN_ANIMATION_LAYERS);
/// Shared previous/current pose storage, independent of one model's bone limit.
pub const MAX_ACTOR_BONE_ARENA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ACTOR_POSE_BONES: usize = MAX_ACTOR_BONE_ARENA_BYTES / (2 * ACTOR_BONE_MATRIX_BYTES);

/// The body layer of an actor; equipment instances of the same actor use layers above it.
pub const ACTOR_LAYER_BODY: u8 = 0;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActorRenderIdentity {
    pub session_id: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub ingress_sequence: u64,
    pub source_tick: Option<u64>,
    pub movement_revision: u64,
    pub pose_generation: u64,
    /// `ACTOR_LAYER_BODY` for the body; equipment layers share the body's other fields.
    pub layer: u8,
}

impl ActorRenderIdentity {
    #[must_use]
    pub const fn is_exact(self) -> bool {
        self.session_id != 0
            && self.runtime_id != 0
            && self.spawn_revision != 0
            && self.ingress_sequence != 0
            && self.pose_generation != 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActorRigRenderInput {
    pub identity: ActorRenderIdentity,
    pub rig: EntityRigId,
    pub previous_bones: Arc<[RenderBoneTransform]>,
    pub current_bones: Arc<[RenderBoneTransform]>,
    pub completed_tick: u64,
    pub reset_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorRigRoute {
    Compiled,
    StaticFallback,
    Diagnostic,
    /// Participates only in Enhanced directional-light shadow passes.
    ShadowOnly,
    NoDraw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActorRigSubmission {
    pub material: ActorMaterial,
    /// Model-space visibility box shared with animation and cave admission.
    pub culling_bounds: assets::SkinGeometryBounds,
    pub input: ActorRigRenderInput,
    pub world_from_actor: [[f32; 4]; 3],
    pub texture_layer: u32,
    pub route: ActorRigRoute,
    /// Packed `0xAABBGGRR` dye multiplier for fully opaque texels; `0` leaves the texture untouched.
    pub tint: u32,
    /// Packed RGBA8 overlay blended over the lit skin (see [`pack_overlay_rgba8`]); 0 disables it.
    pub overlay_rgba8: u32,
    /// Render-controller `uv_anim` `[offset u, offset v, scale u, scale v]`.
    pub uv_anim: [f32; 4],
    /// World light from [`pack_actor_light`]; 0 draws unlit, as `ignore_lighting` asks.
    pub light: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorMaterial {
    /// Animated actor glint factors; unused by other material kinds.
    pub glint: super::ActorGlint,
    pub kind: assets::EntityRenderMaterial,
    pub state: Option<assets::EntityRenderMaterialState>,
    /// Alpha-test multiplier remains a float because authored dissolve values exceed one.
    pub dissolve_multiplier: f32,
    /// RGB illumination multiplier; applies to lit and unlit draws without clamping.
    pub light_color_multiplier: f32,
}

impl Default for ActorMaterial {
    fn default() -> Self {
        Self {
            glint: Default::default(),
            kind: Default::default(),
            state: None,
            dissolve_multiplier: 1.0,
            light_color_multiplier: 1.0,
        }
    }
}

/// Packs independent block/sky nibbles and the lit-material bit; time belongs to the shared table.
#[must_use]
pub fn pack_actor_light(block: u8, sky: u8) -> u32 {
    0x8000_0000 | (u32::from(sky.min(15)) << 4) | u32::from(block.min(15))
}

/// The `uv_anim` of a draw without one.
pub const IDENTITY_UV_ANIM: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn sanitized_uv_anim(uv_anim: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|axis| {
        if uv_anim[axis].is_finite() {
            uv_anim[axis]
        } else {
            IDENTITY_UV_ANIM[axis]
        }
    })
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct ActorGpuInstance {
    pub world_from_actor: [[f32; 4]; 3],
    pub previous_bone_base: u32,
    pub current_bone_base: u32,
    pub geometry_id: u32,
    pub texture_layer: u32,
    pub partial_tick: f32,
    pub reset_generation: u32,
    pub tint: u32,
    pub overlay_rgba8: u32,
    pub uv_anim: [f32; 4],
    pub light: u32,
    /// Two more samplers of a native multitexture material; MAX names no additional sampler.
    pub multitexture_layers: [u32; 2],
    pub material: u32,
    pub dissolve_multiplier: f32,
    pub light_color_multiplier: f32,
    pub glint: [f32; 3],
}

impl Default for ActorGpuInstance {
    fn default() -> Self {
        Self {
            light_color_multiplier: 1.0,
            ..Self::zeroed()
        }
    }
}

pub const ACTOR_GPU_INSTANCE_WORDS: usize = std::mem::size_of::<ActorGpuInstance>() / 4;
const _: () = assert!(std::mem::size_of::<ActorGpuInstance>() == ACTOR_GPU_INSTANCE_WORDS * 4);

/// Packs a non-premultiplied RGBA overlay (components clamped to 0..=1) into little-endian RGBA8.
#[must_use]
pub fn pack_overlay_rgba8(rgba: [f32; 4]) -> u32 {
    let byte = |value: f32| {
        if value.is_finite() {
            (value.clamp(0.0, 1.0) * 255.0).round() as u32
        } else {
            0
        }
    };
    byte(rgba[0]) | (byte(rgba[1]) << 8) | (byte(rgba[2]) << 16) | (byte(rgba[3]) << 24)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
pub struct ActorRigGeometrySpan {
    pub first_vertex: u32,
    pub vertex_count: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActorRigRejects {
    pub invalid_identity: u64,
    pub invalid_world_transform: u64,
    pub non_finite_pose: u64,
    pub pose_length_mismatch: u64,
    pub bone_capacity: u64,
    pub actor_capacity: u64,
    pub missing_geometry: u64,
    pub invalid_geometry: u64,
    pub no_draw: u64,
    pub generation_exhaustion: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorDrawManifestEntry {
    pub identity: ActorRenderIdentity,
    pub rig: EntityRigId,
    pub completed_tick: u64,
    pub reset_generation: u64,
    pub route: ActorRigRoute,
    pub instance_index: u32,
    pub previous_bone_base: u32,
    pub current_bone_base: u32,
    pub bone_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActorRigRenderFrame {
    pub frame_generation: u64,
    pub geometry_revision: u64,
    pub instances: Arc<[ActorGpuInstance]>,
    pub previous_bones: Arc<[[[f32; 4]; 3]]>,
    pub current_bones: Arc<[[[f32; 4]; 3]]>,
    pub geometry_vertices: ActorRigVertexSegments,
    pub geometry_spans: Arc<[ActorRigGeometrySpan]>,
    pub manifest: Arc<[ActorDrawManifestEntry]>,
    pub maximum_vertex_count: u32,
    pub rejects: ActorRigRejects,
}

impl Default for ActorRigRenderFrame {
    fn default() -> Self {
        Self {
            frame_generation: 0,
            geometry_revision: 0,
            instances: Arc::from([]),
            previous_bones: Arc::from([]),
            current_bones: Arc::from([]),
            geometry_vertices: ActorRigVertexSegments::default(),
            geometry_spans: Arc::from([]),
            manifest: Arc::from([]),
            maximum_vertex_count: 0,
            rejects: ActorRigRejects::default(),
        }
    }
}

#[derive(Debug)]
pub struct ActorRigFrameBuilder {
    catalog: GeometryCatalog,
    frame_generation: u64,
    matrices: PoseMatrixCache,
    scratch: BuildScratch,
}

/// Buffers a build fills, kept so steady frames reuse their capacity.
#[derive(Debug, Default)]
struct BuildScratch {
    submissions: Vec<ActorRigSubmission>,
    instances: Vec<ActorGpuInstance>,
    previous_bones: Vec<[[f32; 4]; 3]>,
    current_bones: Vec<[[f32; 4]; 3]>,
    manifest: Vec<ActorDrawManifestEntry>,
}

impl ActorRigFrameBuilder {
    pub fn from_runtime_assets(
        assets: &RuntimeEntityAssets,
    ) -> Result<Self, ActorRigGeometryError> {
        let mut geometries = Vec::new();
        for binding in 0..assets.rig_geometries().len() {
            match geometry_from_runtime_assets(assets, binding) {
                Ok(geometry) => geometries.push(geometry),
                Err(ActorRigGeometryError::CatalogCapacity) => {
                    return Err(ActorRigGeometryError::CatalogCapacity);
                }
                Err(_) => {
                    // Runtime publication retains the binding ID. An omitted
                    // geometry therefore reaches the explicit missing-rig
                    // fallback/no-draw route instead of poisoning all rigs.
                }
            }
        }
        geometries.extend(layer_geometries(assets, EntityRigId(0)));
        Self::new(geometries)
    }

    /// Like [`Self::from_runtime_assets`], also registering the listed entity-catalog geometries
    /// (by geometry index) as equipment geometry under [`equipment_rig_id`].
    pub fn from_runtime_assets_with_equipment(
        assets: &RuntimeEntityAssets,
        equipment_geometries: &[u32],
    ) -> Result<Self, ActorRigGeometryError> {
        let mut builder = Self::from_runtime_assets(assets)?;
        for &geometry_index in equipment_geometries {
            let id = equipment_rig_id(geometry_index);
            if builder.catalog.geometries.contains_key(&id) {
                continue;
            }
            // Like entity rigs, an unbuildable equipment geometry is omitted, not fatal.
            if let Ok(geometry) = geometry_from_geometry_index(assets, geometry_index as usize, id)
                && let Err(ActorRigGeometryError::CatalogCapacity) =
                    builder.insert_geometry(geometry)
            {
                return Err(ActorRigGeometryError::CatalogCapacity);
            }
        }
        Ok(builder)
    }

    pub fn new(
        geometries: impl IntoIterator<Item = ActorRigGeometry>,
    ) -> Result<Self, ActorRigGeometryError> {
        let mut by_id = BTreeMap::new();
        for geometry in geometries {
            if by_id.insert(geometry.id, geometry).is_some() {
                return Err(ActorRigGeometryError::DuplicateRig);
            }
        }
        by_id.insert(DIAGNOSTIC_RIG_ID, diagnostic_geometry());
        Ok(Self {
            catalog: GeometryCatalog::layout(by_id)?,
            frame_generation: 0,
            matrices: PoseMatrixCache::default(),
            scratch: BuildScratch::default(),
        })
    }

    /// Adds or replaces one geometry and republishes the catalog; existing rig ids keep their
    /// vertices, and the new revision makes the GPU re-upload.
    pub fn insert_geometry(
        &mut self,
        geometry: ActorRigGeometry,
    ) -> Result<(), ActorRigGeometryError> {
        self.insert_geometries(vec![geometry])
    }

    /// [`Self::insert_geometry`] for several geometries under one catalog rebuild; on error the
    /// catalog is unchanged.
    pub fn insert_geometries(
        &mut self,
        geometries: Vec<ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
        if geometries.is_empty() {
            return Ok(());
        }
        if geometries
            .iter()
            .any(|geometry| geometry.id == DIAGNOSTIC_RIG_ID)
        {
            return Err(ActorRigGeometryError::DuplicateRig);
        }
        let mut revision = self.catalog.revision;
        for geometry in &geometries {
            revision = revision
                .rotate_left(5)
                .wrapping_add((u64::from(geometry.id.0) << 24) | geometry.vertices.len() as u64);
        }
        self.catalog.append(geometries, revision.max(1))?;
        self.matrices = PoseMatrixCache::default();
        Ok(())
    }

    /// Replaces every pack-range geometry with `geometries` (empty removes them) and
    /// republishes the catalog; on error the catalog is unchanged.
    pub fn replace_pack_geometries(
        &mut self,
        geometries: Vec<ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
        self.replace_range_geometries(is_pack_rig_id, geometries)
    }

    /// Like [`Self::replace_pack_geometries`] for the pack equipment id range.
    pub fn replace_pack_equipment_geometries(
        &mut self,
        geometries: Vec<ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
        self.replace_range_geometries(is_pack_equipment_rig_id, geometries)
    }

    fn replace_range_geometries(
        &mut self,
        in_range: fn(EntityRigId) -> bool,
        geometries: Vec<ActorRigGeometry>,
    ) -> Result<(), ActorRigGeometryError> {
        if geometries.is_empty() && !self.catalog.geometries.keys().any(|id| in_range(*id)) {
            return Ok(());
        }
        let mut by_id = self.catalog.geometries.clone();
        pack::replace_range(&mut by_id, in_range, geometries);
        self.catalog = GeometryCatalog::layout_with_limit(by_id, self.catalog.maximum_vertices)?;
        self.matrices = PoseMatrixCache::default();
        Ok(())
    }

    #[must_use]
    pub fn contains_geometry(&self, id: EntityRigId) -> bool {
        self.catalog.geometries.contains_key(&id)
    }

    #[must_use]
    pub const fn geometry_vertices(&self) -> &ActorRigVertexSegments {
        &self.catalog.vertices
    }

    /// Checks one camera-space draw without advancing frame generations or rebuilding buffers.
    pub fn can_draw_submission(&self, submission: &ActorRigSubmission) -> bool {
        if self.frame_generation == u64::MAX || eligibility::validate_input(submission).is_err() {
            return false;
        }
        let Ok((id, geometry)) = eligibility::geometry(&self.catalog, submission) else {
            return false;
        };
        let Some(&index) = self.catalog.indices.get(&id) else {
            return false;
        };
        u32::try_from(submission.input.reset_generation).is_ok()
            && self.catalog.published_spans[index as usize].vertex_count > 0
            && self.matrices.pose_is_valid(
                &submission.input.previous_bones,
                id,
                &geometry.bone_pivots,
            )
            && self.matrices.pose_is_valid(
                &submission.input.current_bones,
                id,
                &geometry.bone_pivots,
            )
    }

    #[must_use]
    pub fn build(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        submissions: impl IntoIterator<Item = ActorRigSubmission>,
    ) -> ActorRigRenderFrame {
        self.build_paged(partial_tick, view, submissions, |_| 0)
    }

    /// [`Self::build`] with instances grouped by layer, then `page_of` texture page, then
    /// geometry, so each group draws once with its geometry's own vertex count.
    #[must_use]
    pub fn build_paged(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        submissions: impl IntoIterator<Item = ActorRigSubmission>,
        page_of: impl Fn(&ActorRenderIdentity) -> ActorArtworkPageId,
    ) -> ActorRigRenderFrame {
        let Some(frame_generation) = self.frame_generation.checked_add(1) else {
            return ActorRigRenderFrame {
                rejects: ActorRigRejects {
                    generation_exhaustion: 1,
                    ..ActorRigRejects::default()
                },
                geometry_vertices: self.catalog.vertices.clone(),
                geometry_spans: Arc::clone(&self.catalog.published_spans),
                geometry_revision: self.catalog.revision,
                ..ActorRigRenderFrame::default()
            };
        };
        self.frame_generation = frame_generation;
        let partial_tick = if partial_tick.is_finite() {
            partial_tick.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.matrices.begin_frame();
        let mut scratch = std::mem::take(&mut self.scratch);
        // The newest identity of each actor layer wins; equal identities keep the first.
        let key = |submission: &ActorRigSubmission| {
            let identity = submission.input.identity;
            (
                identity.session_id,
                identity.dimension,
                identity.runtime_id,
                identity.layer,
            )
        };
        let mut ordered = std::mem::take(&mut scratch.submissions);
        ordered.extend(submissions);
        ordered.sort_by(|a, b| {
            key(a)
                .cmp(&key(b))
                .then(b.input.identity.cmp(&a.input.identity))
        });
        ordered.dedup_by_key(|submission| key(submission));
        let mut instances = std::mem::take(&mut scratch.instances);
        let mut previous_bones = std::mem::take(&mut scratch.previous_bones);
        let mut current_bones = std::mem::take(&mut scratch.current_bones);
        let mut manifest = std::mem::take(&mut scratch.manifest);
        instances.clear();
        previous_bones.clear();
        current_bones.clear();
        manifest.clear();
        let mut maximum_vertex_count = 0;
        let mut rejects = ActorRigRejects::default();

        // Bodies first so equipment can never crowd a body out of the instance arena; layers in
        // ascending order so a coplanar overlay draws after the layers beneath it.
        ordered.sort_by_key(|submission| {
            let identity = submission.input.identity;
            (
                submission.route == ActorRigRoute::ShadowOnly,
                identity.layer,
                page_of(&identity),
                submission.input.rig,
            )
        });
        let mut body_count = 0usize;
        for submission in ordered.drain(..) {
            if let Err(error) = eligibility::validate_input(&submission) {
                error.count(&mut rejects);
                continue;
            }
            if !actor_rig_submission_is_visible(&submission, view) {
                continue;
            }
            let is_body = submission.input.identity.layer == ACTOR_LAYER_BODY;
            if instances.len() == MAX_ACTOR_RENDER_INSTANCES
                || (is_body && body_count == MAX_RENDERED_PLAYERS)
            {
                rejects.actor_capacity = rejects.actor_capacity.saturating_add(1);
                continue;
            }
            let previous = &submission.input.previous_bones;
            let current = &submission.input.current_bones;
            let (geometry_id, geometry) = match eligibility::geometry(&self.catalog, &submission) {
                Ok(geometry) => geometry,
                Err(error) => {
                    error.count(&mut rejects);
                    continue;
                }
            };
            let Some(next_bone_count) = previous_bones.len().checked_add(previous.len()) else {
                rejects.bone_capacity = rejects.bone_capacity.saturating_add(1);
                continue;
            };
            if next_bone_count > MAX_ACTOR_POSE_BONES {
                rejects.bone_capacity = rejects.bone_capacity.saturating_add(1);
                continue;
            }
            let previous_bone_base = previous_bones.len() as u32;
            let current_bone_base = current_bones.len() as u32;
            let previous_valid = self.matrices.append(
                &mut previous_bones,
                previous,
                geometry_id,
                &geometry.bone_pivots,
            );
            let current_valid = self.matrices.append(
                &mut current_bones,
                current,
                geometry_id,
                &geometry.bone_pivots,
            );
            if !previous_valid || !current_valid {
                previous_bones.truncate(previous_bone_base as usize);
                current_bones.truncate(current_bone_base as usize);
                rejects.non_finite_pose = rejects.non_finite_pose.saturating_add(1);
                continue;
            }
            let Some(&geometry_index) = self.catalog.indices.get(&geometry_id) else {
                previous_bones.truncate(previous_bone_base as usize);
                current_bones.truncate(current_bone_base as usize);
                rejects.invalid_geometry = rejects.invalid_geometry.saturating_add(1);
                continue;
            };
            let span = self.catalog.published_spans[geometry_index as usize];
            maximum_vertex_count = maximum_vertex_count.max(span.vertex_count);
            let Ok(reset_generation) = u32::try_from(submission.input.reset_generation) else {
                previous_bones.truncate(previous_bone_base as usize);
                current_bones.truncate(current_bone_base as usize);
                rejects.invalid_identity = rejects.invalid_identity.saturating_add(1);
                continue;
            };
            let instance_index = instances.len() as u32;
            instances.push(ActorGpuInstance {
                world_from_actor: submission.world_from_actor,
                previous_bone_base,
                current_bone_base,
                geometry_id: geometry_index,
                texture_layer: submission.texture_layer,
                partial_tick,
                reset_generation,
                tint: submission.tint,
                uv_anim: sanitized_uv_anim(submission.uv_anim),
                light: submission.light,
                overlay_rgba8: submission.overlay_rgba8,
                multitexture_layers: [u32::MAX; 2],
                material: submission.material.gpu_word(),
                glint: submission.material.glint.parameters(),
                dissolve_multiplier: if submission.material.dissolve_multiplier.is_finite() {
                    submission.material.dissolve_multiplier.max(0.0)
                } else {
                    1.0
                },
                light_color_multiplier: if submission.material.light_color_multiplier.is_finite() {
                    submission.material.light_color_multiplier
                } else {
                    1.0
                },
            });
            body_count += usize::from(is_body);
            manifest.push(ActorDrawManifestEntry {
                identity: submission.input.identity,
                rig: submission.input.rig,
                completed_tick: submission.input.completed_tick,
                reset_generation: submission.input.reset_generation,
                route: submission.route,
                instance_index,
                previous_bone_base,
                current_bone_base,
                bone_count: previous.len() as u32,
            });
        }

        debug_assert!(
            previous_bones.len() * ACTOR_BONE_MATRIX_BYTES * 2 <= MAX_ACTOR_BONE_ARENA_BYTES
        );
        let frame = ActorRigRenderFrame {
            frame_generation,
            geometry_revision: self.catalog.revision,
            instances: Arc::from(instances.as_slice()),
            previous_bones: Arc::from(previous_bones.as_slice()),
            current_bones: Arc::from(current_bones.as_slice()),
            geometry_vertices: self.catalog.vertices.clone(),
            geometry_spans: Arc::clone(&self.catalog.published_spans),
            manifest: Arc::from(manifest.as_slice()),
            maximum_vertex_count,
            rejects,
        };
        self.scratch = BuildScratch {
            submissions: ordered,
            instances,
            previous_bones,
            current_bones,
            manifest,
        };
        frame
    }
}

#[must_use]
pub fn actor_rig_submission_is_visible(
    submission: &ActorRigSubmission,
    view: Option<ActorCullView>,
) -> bool {
    // The culling box grows with the instance's scale so scaled models are not cut early.
    let scale = (0..3)
        .map(|axis| {
            Vec3::new(
                submission.world_from_actor[0][axis],
                submission.world_from_actor[1][axis],
                submission.world_from_actor[2][axis],
            )
            .length()
        })
        .fold(1.0_f32, f32::max);
    let feet = submission.world_from_actor.map(|row| row[3]);
    actor_bounds_are_visible(feet, scale, submission.culling_bounds, view)
}

/// Tests an authored model visibility box against the same distance and frustum as default actors.
#[must_use]
pub fn actor_bounds_are_visible(
    feet: [f32; 3],
    scale: f32,
    bounds: assets::SkinGeometryBounds,
    view: Option<ActorCullView>,
) -> bool {
    let Some(view) = view.filter(|view| {
        view.clip_from_world.is_finite()
            && view.camera_position.is_finite()
            && view.max_distance.is_finite()
            && view.max_distance > 0.0
    }) else {
        return true;
    };
    let feet = Vec3::from_array(feet);
    if (feet + Vec3::Y).distance_squared(view.camera_position)
        > view.max_distance * view.max_distance
    {
        return false;
    }
    let (low, high) = bounds.at(feet.to_array(), scale);
    let corners: [Vec4; 8] = std::array::from_fn(|index| {
        let point = Vec3::from_array(std::array::from_fn(|axis| {
            if index & (1 << axis) == 0 {
                low[axis]
            } else {
                high[axis]
            }
        }));
        view.clip_from_world * point.extend(1.0)
    });
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

fn affine_matrix(transform: RenderBoneTransform, bind_pivot: [f32; 3]) -> Option<[[f32; 4]; 3]> {
    if !transform.is_finite() || bind_pivot.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let [x, y, z, w] = transform.rotation;
    let norm = x * x + y * y + z * z + w * w;
    if !norm.is_finite() || norm <= f32::EPSILON {
        return None;
    }
    let inverse_norm = norm.sqrt().recip();
    let (x, y, z, w) = (
        x * inverse_norm,
        y * inverse_norm,
        z * inverse_norm,
        w * inverse_norm,
    );
    // Column c of the linear part carries the bone-frame scale on axis c.
    let [sx, sy, sz] =
        std::array::from_fn(|axis| transform.axis_scale[axis] * transform.translation_scale[3]);
    let rows = [
        [
            (1.0 - 2.0 * (y * y + z * z)) * sx,
            2.0 * (x * y - z * w) * sy,
            2.0 * (x * z + y * w) * sz,
        ],
        [
            2.0 * (x * y + z * w) * sx,
            (1.0 - 2.0 * (x * x + z * z)) * sy,
            2.0 * (y * z - x * w) * sz,
        ],
        [
            2.0 * (x * z - y * w) * sx,
            2.0 * (y * z + x * w) * sy,
            (1.0 - 2.0 * (x * x + y * y)) * sz,
        ],
    ];
    let rotated_pivot =
        rows.map(|row| row[0] * bind_pivot[0] + row[1] * bind_pivot[1] + row[2] * bind_pivot[2]);
    let translation: [f32; 3] =
        std::array::from_fn(|axis| transform.translation_scale[axis] - rotated_pivot[axis]);
    Some(std::array::from_fn(|axis| {
        [
            rows[axis][0],
            rows[axis][1],
            rows[axis][2],
            translation[axis],
        ]
    }))
}
