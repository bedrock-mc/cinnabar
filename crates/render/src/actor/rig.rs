use std::{collections::BTreeMap, sync::Arc};

use assets::{EntityGeometryBone, RuntimeEntityAssets, validate_entity_geometry_inheritance};
use bevy::math::{Vec3, Vec4};
use bytemuck::{Pod, Zeroable};

use super::{ActorCullView, MAX_RENDERED_PLAYERS};

pub const MAX_RENDER_BONES_PER_ACTOR: usize = 96;
pub const ACTOR_BONE_MATRIX_BYTES: usize = 48;
pub const MAX_ACTOR_BONE_ARENA_BYTES: usize =
    MAX_RENDERED_PLAYERS * MAX_RENDER_BONES_PER_ACTOR * 2 * ACTOR_BONE_MATRIX_BYTES;
pub const MAX_ACTOR_RIG_VERTICES: usize = 1_048_576;

const DIAGNOSTIC_RIG_ID: EntityRigId = EntityRigId(u32::MAX);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActorRenderIdentity {
    pub session_id: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub ingress_sequence: u64,
    pub source_tick: Option<i64>,
    pub movement_revision: u64,
    pub pose_generation: u64,
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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct EntityRigId(pub u32);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RenderBoneTransform {
    pub rotation: [f32; 4],
    pub translation_scale: [f32; 4],
}

impl RenderBoneTransform {
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.rotation
            .iter()
            .chain(self.translation_scale.iter())
            .all(|value| value.is_finite())
            && self.translation_scale[3] != 0.0
    }

    #[must_use]
    pub fn from_model_space(rotation: [f32; 4], translation_scale: [f32; 4]) -> Option<Self> {
        let converted = Self {
            rotation,
            translation_scale: [
                translation_scale[0] / 16.0,
                translation_scale[1] / 16.0,
                translation_scale[2] / 16.0,
                translation_scale[3],
            ],
        };
        converted.is_finite().then_some(converted)
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
    NoDraw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActorRigSubmission {
    pub input: ActorRigRenderInput,
    pub world_from_actor: [[f32; 4]; 3],
    pub texture_layer: u32,
    pub route: ActorRigRoute,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ActorGpuInstance {
    pub world_from_actor: [[f32; 4]; 3],
    pub previous_bone_base: u32,
    pub current_bone_base: u32,
    pub geometry_id: u32,
    pub texture_layer: u32,
    pub partial_tick: f32,
    pub reset_generation: u32,
}

const _: () = assert!(std::mem::size_of::<ActorGpuInstance>() == 72);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ActorRigVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub bone_index: u32,
}

const _: () = assert!(std::mem::size_of::<ActorRigVertex>() == 36);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
pub struct ActorRigGeometrySpan {
    pub first_vertex: u32,
    pub vertex_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActorRigGeometry {
    pub id: EntityRigId,
    pub vertices: Arc<[ActorRigVertex]>,
    pub bone_pivots: Arc<[[f32; 3]]>,
}

impl ActorRigGeometry {
    pub fn new(
        id: EntityRigId,
        vertices: impl Into<Arc<[ActorRigVertex]>>,
        bone_pivots: impl Into<Arc<[[f32; 3]]>>,
    ) -> Result<Self, ActorRigGeometryError> {
        let vertices = vertices.into();
        let bone_pivots = bone_pivots.into();
        if vertices.is_empty() || vertices.len() > MAX_ACTOR_RIG_VERTICES {
            return Err(ActorRigGeometryError::VertexCount);
        }
        if bone_pivots.is_empty() || bone_pivots.len() > MAX_RENDER_BONES_PER_ACTOR {
            return Err(ActorRigGeometryError::BoneCount);
        }
        if vertices.iter().any(|vertex| {
            vertex
                .position
                .iter()
                .chain(vertex.normal.iter())
                .chain(vertex.uv.iter())
                .any(|value| !value.is_finite())
                || vertex.bone_index as usize >= bone_pivots.len()
        }) || bone_pivots.iter().flatten().any(|value| !value.is_finite())
        {
            return Err(ActorRigGeometryError::InvalidVertex);
        }
        Ok(Self {
            id,
            vertices,
            bone_pivots,
        })
    }

    pub fn synthetic_cuboid(
        id: EntityRigId,
        min: [f32; 3],
        max: [f32; 3],
        bone_count: usize,
    ) -> Result<Self, ActorRigGeometryError> {
        if min.iter().chain(max.iter()).any(|value| !value.is_finite())
            || min
                .iter()
                .zip(max)
                .any(|(minimum, maximum)| *minimum >= maximum)
            || bone_count == 0
            || bone_count > MAX_RENDER_BONES_PER_ACTOR
        {
            return Err(ActorRigGeometryError::InvalidVertex);
        }
        let vertices = super::geometry::cuboid_vertices(min, max, 0);
        Self::new(
            id,
            Arc::from(vertices),
            Arc::from(vec![[0.0; 3]; bone_count]),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorRigGeometryError {
    VertexCount,
    BoneCount,
    InvalidVertex,
    DuplicateRig,
    CatalogCapacity,
    InvalidAssetGeometry,
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
    pub geometry_vertices: Arc<[ActorRigVertex]>,
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
            geometry_vertices: Arc::from([]),
            geometry_spans: Arc::from([]),
            manifest: Arc::from([]),
            maximum_vertex_count: 0,
            rejects: ActorRigRejects::default(),
        }
    }
}

#[derive(Debug)]
pub struct ActorRigFrameBuilder {
    geometries: BTreeMap<EntityRigId, ActorRigGeometry>,
    geometry_indices: BTreeMap<EntityRigId, u32>,
    geometry_vertices: Arc<[ActorRigVertex]>,
    geometry_spans: Arc<[ActorRigGeometrySpan]>,
    frame_generation: u64,
    geometry_revision: u64,
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
        Self::new(geometries)
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
        let mut geometry_indices = BTreeMap::new();
        let mut vertices = Vec::new();
        let mut spans = Vec::with_capacity(by_id.len());
        for (id, geometry) in &by_id {
            let first_vertex = u32::try_from(vertices.len())
                .map_err(|_| ActorRigGeometryError::CatalogCapacity)?;
            let vertex_count = u32::try_from(geometry.vertices.len())
                .map_err(|_| ActorRigGeometryError::CatalogCapacity)?;
            if vertices
                .len()
                .checked_add(geometry.vertices.len())
                .is_none_or(|count| count > MAX_ACTOR_RIG_VERTICES)
            {
                return Err(ActorRigGeometryError::CatalogCapacity);
            }
            geometry_indices.insert(
                *id,
                u32::try_from(spans.len()).map_err(|_| ActorRigGeometryError::CatalogCapacity)?,
            );
            vertices.extend_from_slice(&geometry.vertices);
            spans.push(ActorRigGeometrySpan {
                first_vertex,
                vertex_count,
            });
        }
        let geometry_revision = geometry_catalog_revision(&vertices, &spans);
        Ok(Self {
            geometries: by_id,
            geometry_indices,
            geometry_vertices: Arc::from(vertices),
            geometry_spans: Arc::from(spans),
            frame_generation: 0,
            geometry_revision,
        })
    }

    #[must_use]
    pub fn geometry_vertices(&self) -> &[ActorRigVertex] {
        &self.geometry_vertices
    }

    #[must_use]
    pub fn build(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        submissions: impl IntoIterator<Item = ActorRigSubmission>,
    ) -> ActorRigRenderFrame {
        let Some(frame_generation) = self.frame_generation.checked_add(1) else {
            return ActorRigRenderFrame {
                rejects: ActorRigRejects {
                    generation_exhaustion: 1,
                    ..ActorRigRejects::default()
                },
                geometry_vertices: Arc::clone(&self.geometry_vertices),
                geometry_spans: Arc::clone(&self.geometry_spans),
                geometry_revision: self.geometry_revision,
                ..ActorRigRenderFrame::default()
            };
        };
        self.frame_generation = frame_generation;
        let partial_tick = if partial_tick.is_finite() {
            partial_tick.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut latest = BTreeMap::<(u64, i32, u64), ActorRigSubmission>::new();
        for submission in submissions {
            let key = (
                submission.input.identity.session_id,
                submission.input.identity.dimension,
                submission.input.identity.runtime_id,
            );
            match latest.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(submission);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if submission.input.identity > entry.get().input.identity {
                        entry.insert(submission);
                    }
                }
            }
        }
        let mut instances = Vec::new();
        let mut previous_bones = Vec::new();
        let mut current_bones = Vec::new();
        let mut manifest = Vec::new();
        let mut maximum_vertex_count = 0;
        let mut rejects = ActorRigRejects::default();

        for submission in latest.into_values() {
            if submission.route == ActorRigRoute::NoDraw {
                rejects.no_draw = rejects.no_draw.saturating_add(1);
                continue;
            }
            let diagnostic = submission.route == ActorRigRoute::Diagnostic;
            if (!diagnostic
                && (!submission.input.identity.is_exact() || submission.input.completed_tick == 0))
                || submission.input.reset_generation == 0
            {
                rejects.invalid_identity = rejects.invalid_identity.saturating_add(1);
                continue;
            }
            if submission
                .world_from_actor
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
            {
                rejects.invalid_world_transform = rejects.invalid_world_transform.saturating_add(1);
                continue;
            }
            if !actor_rig_submission_is_visible(&submission, view) {
                continue;
            }
            if instances.len() == MAX_RENDERED_PLAYERS {
                rejects.actor_capacity = rejects.actor_capacity.saturating_add(1);
                continue;
            }
            let previous = &submission.input.previous_bones;
            let current = &submission.input.current_bones;
            if previous.len() != current.len() {
                rejects.pose_length_mismatch = rejects.pose_length_mismatch.saturating_add(1);
                continue;
            }
            if previous.is_empty() || previous.len() > MAX_RENDER_BONES_PER_ACTOR {
                rejects.bone_capacity = rejects.bone_capacity.saturating_add(1);
                continue;
            }
            if previous
                .iter()
                .chain(current.iter())
                .any(|bone| !bone.is_finite())
            {
                rejects.non_finite_pose = rejects.non_finite_pose.saturating_add(1);
                continue;
            }
            let geometry_id = match submission.route {
                ActorRigRoute::Compiled | ActorRigRoute::StaticFallback => submission.input.rig,
                ActorRigRoute::Diagnostic => DIAGNOSTIC_RIG_ID,
                ActorRigRoute::NoDraw => unreachable!(),
            };
            let Some(geometry) = self.geometries.get(&geometry_id) else {
                rejects.missing_geometry = rejects.missing_geometry.saturating_add(1);
                continue;
            };
            if geometry.vertices.iter().any(|vertex| {
                vertex.bone_index as usize >= previous.len()
                    || vertex.bone_index as usize >= geometry.bone_pivots.len()
            }) {
                rejects.invalid_geometry = rejects.invalid_geometry.saturating_add(1);
                continue;
            }
            let Some(next_bone_count) = previous_bones.len().checked_add(previous.len()) else {
                rejects.bone_capacity = rejects.bone_capacity.saturating_add(1);
                continue;
            };
            if next_bone_count > MAX_RENDERED_PLAYERS * MAX_RENDER_BONES_PER_ACTOR {
                rejects.bone_capacity = rejects.bone_capacity.saturating_add(1);
                continue;
            }
            let previous_bone_base = previous_bones.len() as u32;
            let current_bone_base = current_bones.len() as u32;
            let previous_matrices = previous
                .iter()
                .enumerate()
                .map(|(index, transform)| {
                    affine_matrix(
                        *transform,
                        geometry.bone_pivots.get(index).copied().unwrap_or([0.0; 3]),
                    )
                })
                .collect::<Option<Vec<_>>>();
            let current_matrices = current
                .iter()
                .enumerate()
                .map(|(index, transform)| {
                    affine_matrix(
                        *transform,
                        geometry.bone_pivots.get(index).copied().unwrap_or([0.0; 3]),
                    )
                })
                .collect::<Option<Vec<_>>>();
            let (Some(previous_matrices), Some(current_matrices)) =
                (previous_matrices, current_matrices)
            else {
                rejects.non_finite_pose = rejects.non_finite_pose.saturating_add(1);
                continue;
            };
            let Some(&geometry_index) = self.geometry_indices.get(&geometry_id) else {
                rejects.invalid_geometry = rejects.invalid_geometry.saturating_add(1);
                continue;
            };
            let span = self.geometry_spans[geometry_index as usize];
            maximum_vertex_count = maximum_vertex_count.max(span.vertex_count);
            let Ok(reset_generation) = u32::try_from(submission.input.reset_generation) else {
                rejects.invalid_identity = rejects.invalid_identity.saturating_add(1);
                continue;
            };
            let instance_index = instances.len() as u32;
            previous_bones.extend(previous_matrices);
            current_bones.extend(current_matrices);
            instances.push(ActorGpuInstance {
                world_from_actor: submission.world_from_actor,
                previous_bone_base,
                current_bone_base,
                geometry_id: geometry_index,
                texture_layer: submission.texture_layer,
                partial_tick,
                reset_generation,
            });
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
        ActorRigRenderFrame {
            frame_generation,
            geometry_revision: self.geometry_revision,
            instances: Arc::from(instances),
            previous_bones: Arc::from(previous_bones),
            current_bones: Arc::from(current_bones),
            geometry_vertices: Arc::clone(&self.geometry_vertices),
            geometry_spans: Arc::clone(&self.geometry_spans),
            manifest: Arc::from(manifest),
            maximum_vertex_count,
            rejects,
        }
    }
}

fn geometry_from_runtime_assets(
    assets: &RuntimeEntityAssets,
    binding_index: usize,
) -> Result<ActorRigGeometry, ActorRigGeometryError> {
    let binding = assets
        .rig_geometries()
        .get(binding_index)
        .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?;
    let bones = resolve_geometry_bones(assets, binding.geometry as usize)?;
    if bones.is_empty() || bones.len() > MAX_RENDER_BONES_PER_ACTOR {
        return Err(ActorRigGeometryError::BoneCount);
    }
    let mut vertices = Vec::new();
    for (bone_index, bone) in bones.iter().enumerate() {
        if bone.never_render == Some(true) {
            continue;
        }
        for cube in &bone.cubes {
            super::geometry::append_entity_cube_vertices(
                &mut vertices,
                cube,
                bone_index as u32,
                assets
                    .geometries()
                    .get(binding.geometry as usize)
                    .map(|geometry| (geometry.texture_width, geometry.texture_height))
                    .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?,
                bone.mirror.unwrap_or(false),
                bone.inflate.map_or(0.0, |inflate| inflate.get()),
            )?;
            if vertices.len() > MAX_ACTOR_RIG_VERTICES {
                return Err(ActorRigGeometryError::CatalogCapacity);
            }
        }
    }
    let bone_pivots = bones
        .iter()
        .map(|bone| {
            bone.pivot
                .map_or([0.0; 3], |pivot| pivot.map(|value| value.get() / 16.0))
        })
        .collect::<Vec<_>>();
    ActorRigGeometry::new(
        EntityRigId(
            u32::try_from(binding_index)
                .map_err(|_| ActorRigGeometryError::InvalidAssetGeometry)?,
        ),
        Arc::from(vertices),
        Arc::from(bone_pivots),
    )
}

fn resolve_geometry_bones(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
) -> Result<Vec<EntityGeometryBone>, ActorRigGeometryError> {
    let parents = validate_entity_geometry_inheritance(assets.geometries())
        .map_err(|_| ActorRigGeometryError::InvalidAssetGeometry)?;
    let mut chain = Vec::new();
    let mut current = geometry_index;
    for _ in 0..=parents.len() {
        chain.push(current);
        let Some(parent) = parents.get(current).copied().flatten() else {
            break;
        };
        current = parent;
    }
    if chain
        .last()
        .and_then(|index| parents.get(*index))
        .copied()
        .flatten()
        .is_some()
    {
        return Err(ActorRigGeometryError::InvalidAssetGeometry);
    }
    chain.reverse();
    let mut merged: Vec<EntityGeometryBone> = Vec::new();
    for index in chain {
        for child in assets
            .geometries()
            .get(index)
            .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?
            .bones
            .iter()
        {
            if let Some(existing) = merged
                .iter_mut()
                .find(|bone| bone.name.eq_ignore_ascii_case(&child.name))
            {
                overlay_geometry_bone(existing, child);
            } else {
                merged.push(child.clone());
            }
        }
    }
    Ok(merged)
}

fn overlay_geometry_bone(base: &mut EntityGeometryBone, child: &EntityGeometryBone) {
    if child.parent.is_some() {
        base.parent.clone_from(&child.parent);
    }
    if child.pivot.is_some() {
        base.pivot = child.pivot;
    }
    if child.rotation.is_some() {
        base.rotation = child.rotation;
    }
    if child.mirror.is_some() {
        base.mirror = child.mirror;
    }
    if child.inflate.is_some() {
        base.inflate = child.inflate;
    }
    if child.never_render.is_some() {
        base.never_render = child.never_render;
    }
    if child.reset.is_some() {
        base.reset = child.reset;
    }
    if !child.cubes.is_empty() {
        base.cubes.clone_from(&child.cubes);
    }
}

fn geometry_catalog_revision(vertices: &[ActorRigVertex], spans: &[ActorRigGeometrySpan]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytemuck::cast_slice::<ActorRigVertex, u8>(vertices)
        .iter()
        .chain(bytemuck::cast_slice::<ActorRigGeometrySpan, u8>(spans))
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash.max(1)
}

#[must_use]
pub fn actor_rig_submission_is_visible(
    submission: &ActorRigSubmission,
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
    let feet = Vec3::new(
        submission.world_from_actor[0][3],
        submission.world_from_actor[1][3],
        submission.world_from_actor[2][3],
    );
    if (feet + Vec3::Y).distance_squared(view.camera_position)
        > view.max_distance * view.max_distance
    {
        return false;
    }
    let half_width = 0.5;
    let corners = [
        Vec3::new(-half_width, 0.0, -half_width),
        Vec3::new(half_width, 0.0, -half_width),
        Vec3::new(-half_width, 2.0, -half_width),
        Vec3::new(half_width, 2.0, -half_width),
        Vec3::new(-half_width, 0.0, half_width),
        Vec3::new(half_width, 0.0, half_width),
        Vec3::new(-half_width, 2.0, half_width),
        Vec3::new(half_width, 2.0, half_width),
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
    let scale = transform.translation_scale[3];
    let rows = [
        [
            (1.0 - 2.0 * (y * y + z * z)) * scale,
            2.0 * (x * y - z * w) * scale,
            2.0 * (x * z + y * w) * scale,
        ],
        [
            2.0 * (x * y + z * w) * scale,
            (1.0 - 2.0 * (x * x + z * z)) * scale,
            2.0 * (y * z - x * w) * scale,
        ],
        [
            2.0 * (x * z - y * w) * scale,
            2.0 * (y * z + x * w) * scale,
            (1.0 - 2.0 * (x * x + y * y)) * scale,
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

fn diagnostic_geometry() -> ActorRigGeometry {
    let mut vertices = super::standard_biped_vertices()
        .into_iter()
        .map(|vertex| ActorRigVertex {
            position: vertex.position,
            normal: [0.0, 1.0, 0.0],
            uv: vertex.uv,
            bone_index: vertex.part,
        })
        .collect::<Vec<_>>();
    for triangle in vertices.chunks_exact_mut(3) {
        let normal = super::geometry::triangle_normal(
            triangle[0].position,
            triangle[1].position,
            triangle[2].position,
        );
        for vertex in triangle {
            vertex.normal = normal;
        }
    }
    let pivots = [
        [0.0, 1.5, 0.0],
        [0.0, 1.5, 0.0],
        [-0.3125, 1.375, 0.0],
        [-0.11875, 0.75, 0.0],
        [0.3125, 1.375, 0.0],
        [0.11875, 0.75, 0.0],
    ];
    ActorRigGeometry::new(DIAGNOSTIC_RIG_ID, Arc::from(vertices), Arc::from(pivots))
        .expect("authored diagnostic actor geometry is finite and bounded")
}
