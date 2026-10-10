//! Actor rig contracts shared by CPU geometry builders and the GPU rig renderer.
use std::{
    hash::{DefaultHasher, Hasher},
    sync::Arc,
};

use bytemuck::{Pod, Zeroable};

use super::ActorRigSurface;
use super::ids::DIAGNOSTIC_RIG_ID;

pub const MAX_RENDER_BONES_PER_ACTOR: usize = assets::MAX_ENTITY_GEOMETRY_BONES;
pub const MAX_ACTOR_RIG_VERTICES: usize = assets::MAX_SKIN_GEOMETRY_VERTICES;
/// Aggregate geometry storage, independent of the vertex limit for one model.
pub const MAX_ACTOR_CATALOG_VERTEX_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_ACTOR_CATALOG_VERTICES: usize =
    MAX_ACTOR_CATALOG_VERTEX_BYTES / std::mem::size_of::<ActorRigVertex>();

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct EntityRigId(pub u32);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RenderBoneTransform {
    pub rotation: [f32; 4],
    pub translation_scale: [f32; 4],
    /// Per-axis scale in the bone's own frame, applied with the uniform scale; `w` is unused.
    pub axis_scale: [f32; 4],
}

/// Per-axis scale of a bone that scales only uniformly.
pub const UNIT_AXIS_SCALE: [f32; 4] = [1.0; 4];

impl RenderBoneTransform {
    /// Whether every component is finite. A zero scale is valid: vanilla hides bones with it.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.rotation
            .iter()
            .chain(self.translation_scale.iter())
            .chain(self.axis_scale.iter())
            .all(|value| value.is_finite())
    }

    #[must_use]
    pub fn from_model_space(rotation: [f32; 4], translation_scale: [f32; 4]) -> Option<Self> {
        Self::from_model_space_scaled(rotation, translation_scale, [1.0; 3])
    }

    /// Converts a pixel-space pose that also scales per axis in the bone's frame.
    #[must_use]
    pub fn from_model_space_scaled(
        rotation: [f32; 4],
        translation_scale: [f32; 4],
        axis_scale: [f32; 3],
    ) -> Option<Self> {
        let converted = Self {
            rotation,
            translation_scale: [
                translation_scale[0] / 16.0,
                translation_scale[1] / 16.0,
                translation_scale[2] / 16.0,
                translation_scale[3],
            ],
            axis_scale: [axis_scale[0], axis_scale[1], axis_scale[2], 1.0],
        };
        converted.is_finite().then_some(converted)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ActorRigVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub back_uv: [f32; 2],
    pub bone_index: u32,
    pub surface: ActorRigSurface,
}

pub const ACTOR_RIG_VERTEX_WORDS: usize = std::mem::size_of::<ActorRigVertex>() / 4;
const _: () = assert!(std::mem::size_of::<ActorRigVertex>() == ACTOR_RIG_VERTEX_WORDS * 4);

#[derive(Clone, Debug)]
pub struct ActorRigGeometry {
    pub id: EntityRigId,
    pub vertices: Arc<[ActorRigVertex]>,
    pub bone_pivots: Arc<[[f32; 3]]>,
    bones_used: usize,
    prepared: Arc<PreparedGeometry>,
}

/// Strong witnesses force public mutable access onto a different allocation.
#[derive(Debug)]
struct PreparedGeometry {
    vertices: Arc<[ActorRigVertex]>,
    bone_pivots: Arc<[[f32; 3]]>,
    fingerprint: u64,
}

impl PartialEq for ActorRigGeometry {
    /// Preparation metadata does not change the geometry's observable value.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.vertices == other.vertices
            && self.bone_pivots == other.bone_pivots
            && self.bones_used == other.bones_used
    }
}

impl ActorRigGeometry {
    /// Validates immutable allocations and prepares their catalog fingerprint.
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
        #[cfg(test)]
        preparation_tests::record_validation(vertices.len());
        if vertices.iter().any(|vertex| {
            vertex
                .position
                .iter()
                .chain(vertex.normal.iter())
                .chain(vertex.uv.iter())
                .chain(vertex.back_uv.iter())
                .any(|value| !value.is_finite())
                || vertex.bone_index as usize >= bone_pivots.len()
                || !vertex.surface.is_valid()
        }) || bone_pivots.iter().flatten().any(|value| !value.is_finite())
        {
            return Err(ActorRigGeometryError::InvalidVertex);
        }
        let bones_used = vertices
            .iter()
            .map(|vertex| vertex.bone_index as usize + 1)
            .max()
            .unwrap_or(0);
        let mut hasher = DefaultHasher::new();
        hasher.write(bytemuck::cast_slice::<ActorRigVertex, u8>(&vertices));
        let prepared = Arc::new(PreparedGeometry {
            vertices: Arc::clone(&vertices),
            bone_pivots: Arc::clone(&bone_pivots),
            fingerprint: hasher.finish(),
        });
        Ok(Self {
            id,
            vertices,
            bone_pivots,
            bones_used,
            prepared,
        })
    }

    /// One past the highest bone any vertex uses; a pose needs at least this many bones.
    #[must_use]
    pub const fn bones_used(&self) -> usize {
        self.bones_used
    }

    /// Reuses the exact prepared vertex allocation; changed public data has no trusted fingerprint.
    #[must_use]
    pub fn vertex_fingerprint(&self) -> Option<u64> {
        Arc::ptr_eq(&self.vertices, &self.prepared.vertices).then_some(self.prepared.fingerprint)
    }

    /// Exact byte equality permits canonical storage to inherit the original validation witness.
    pub fn share_vertex_allocation(&mut self, vertices: &Arc<[ActorRigVertex]>) -> bool {
        if Arc::ptr_eq(&self.vertices, vertices) {
            return true;
        }
        if bytemuck::cast_slice::<ActorRigVertex, u8>(&self.vertices)
            != bytemuck::cast_slice::<ActorRigVertex, u8>(vertices)
        {
            return false;
        }
        let fingerprint = self.vertex_fingerprint();
        self.vertices = Arc::clone(vertices);
        if let Some(fingerprint) = fingerprint {
            self.prepared = Arc::new(PreparedGeometry {
                vertices: Arc::clone(vertices),
                bone_pivots: Arc::clone(&self.prepared.bone_pivots),
                fingerprint,
            });
        }
        true
    }

    /// Rechecks replaced public data while preserving preparation for unchanged allocations.
    pub fn revalidate(&mut self) -> Result<(), ActorRigGeometryError> {
        if Arc::ptr_eq(&self.vertices, &self.prepared.vertices)
            && Arc::ptr_eq(&self.bone_pivots, &self.prepared.bone_pivots)
        {
            return Ok(());
        }
        *self = Self::new(
            self.id,
            Arc::clone(&self.vertices),
            Arc::clone(&self.bone_pivots),
        )?;
        Ok(())
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

/// The fallback rig drawn for actors whose geometry is missing.
pub fn diagnostic_geometry() -> ActorRigGeometry {
    let mut vertices = super::standard_biped_vertices()
        .into_iter()
        .map(|vertex| ActorRigVertex {
            position: vertex.position,
            normal: [0.0, 1.0, 0.0],
            uv: vertex.uv,
            back_uv: vertex.uv,
            bone_index: vertex.part,
            surface: ActorRigSurface::SINGLE_FACE,
        })
        .collect::<Vec<_>>();
    for triangle in vertices.as_chunks_mut::<3>().0 {
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

#[cfg(test)]
#[path = "rig/preparation_tests.rs"]
mod preparation_tests;
