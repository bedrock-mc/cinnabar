//! Convert the existing packed cube stream to indexed GXM/VitaGL vertices at
//! publication time. Never expand terrain again during an unchanged frame.
//!
//! This is the opaque cube geometry adapter, not model/liquid/material parity.

use crate::memory::{Budget, Lease, Pool, Pressure};
use meshing::{Face, PackedQuad};

/// A bounded upload, well below the u16 index limit (four vertices per quad).
pub const QUADS_PER_UPLOAD: usize = 4096;
pub const INDICES_PER_QUAD: usize = 6;
pub const VERTICES_PER_QUAD: usize = 4;

/// Splits without copying or reordering faces. Every upload has one material and
/// fits the index limit; transparent ordering remains the owning renderer's job.
pub fn material_runs(quads: &[PackedQuad]) -> impl Iterator<Item = &[PackedQuad]> {
    quads
        .chunk_by(|a, b| a.material_id() == b.material_id())
        .flat_map(|run| run.chunks(QUADS_PER_UPLOAD))
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    /// Local repeating UVs. Texture-array/atlas selection belongs to the draw's material.
    pub uv: [f32; 2],
}

const _: () = assert!(size_of::<Vertex>() == 20);

#[derive(Debug)]
pub enum UploadError {
    TooManyQuads,
    MixedMaterials,
    Pressure(Pressure),
    Allocation,
}

pub struct CubeUpload {
    pub material: u32,
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
    _lease: Lease,
    _capacity_padding: Option<Lease>,
}

impl CubeUpload {
    /// One contiguous material run. The owner splits longer runs and reserves the
    /// GPU destination separately; staging and destination coexist during upload.
    pub fn new(quads: &[PackedQuad], budget: &Budget) -> Result<Self, UploadError> {
        if quads.len() > QUADS_PER_UPLOAD {
            return Err(UploadError::TooManyQuads);
        }
        let material = quads.first().map_or(0, PackedQuad::material_id);
        if quads.iter().any(|quad| quad.material_id() != material) {
            return Err(UploadError::MixedMaterials);
        }
        let vertex_count = quads.len() * VERTICES_PER_QUAD;
        let index_count = quads.len() * INDICES_PER_QUAD;
        let bytes = vertex_count * size_of::<Vertex>() + index_count * size_of::<u16>();
        let lease = budget
            .reserve(Pool::Main, bytes)
            .map_err(UploadError::Pressure)?;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        vertices
            .try_reserve_exact(vertex_count)
            .map_err(|_| UploadError::Allocation)?;
        indices
            .try_reserve_exact(index_count)
            .map_err(|_| UploadError::Allocation)?;
        // Vec's contract permits an allocator to provide more capacity than
        // requested. Account that retained payload too, or roll the upload back.
        let capacity_bytes =
            vertices.capacity() * size_of::<Vertex>() + indices.capacity() * size_of::<u16>();
        let capacity_padding = if capacity_bytes > bytes {
            Some(
                budget
                    .reserve(Pool::Main, capacity_bytes - bytes)
                    .map_err(UploadError::Pressure)?,
            )
        } else {
            None
        };
        for quad in quads {
            let base = vertices.len() as u16;
            vertices.extend(corners(*quad));
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        Ok(Self {
            material,
            vertices,
            indices,
            _lease: lease,
            _capacity_padding: capacity_padding,
        })
    }

    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }
    pub fn indices(&self) -> &[u16] {
        &self.indices
    }
    pub fn bytes(&self) -> usize {
        self._lease.bytes() + self._capacity_padding.as_ref().map_or(0, Lease::bytes)
    }
}

/// Same corner order and positive-face offset as render/src/chunk.wesl's
/// quad_corner. GPU seam sealing, per-material rotations and lighting remain separate.
pub fn corners(quad: PackedQuad) -> [Vertex; 4] {
    let [x, y, z] = quad.origin().map(f32::from);
    let w = f32::from(quad.width());
    let h = f32::from(quad.height());
    let positions = match quad.face() {
        Face::NegativeX => [[x, y, z], [x, y, z + w], [x, y + h, z + w], [x, y + h, z]],
        Face::PositiveX => [
            [x + 1., y, z],
            [x + 1., y + h, z],
            [x + 1., y + h, z + w],
            [x + 1., y, z + w],
        ],
        Face::NegativeY => [[x, y, z], [x + w, y, z], [x + w, y, z + h], [x, y, z + h]],
        Face::PositiveY => [
            [x, y + 1., z],
            [x, y + 1., z + h],
            [x + w, y + 1., z + h],
            [x + w, y + 1., z],
        ],
        Face::NegativeZ => [[x, y, z], [x, y + h, z], [x + w, y + h, z], [x + w, y, z]],
        Face::PositiveZ => [
            [x, y, z + 1.],
            [x + w, y, z + 1.],
            [x + w, y + h, z + 1.],
            [x, y + h, z + 1.],
        ],
    };
    let uv = match quad.face() {
        Face::PositiveX | Face::PositiveY | Face::NegativeZ => [[0., 0.], [0., h], [w, h], [w, 0.]],
        _ => [[0., 0.], [w, 0.], [w, h], [0., h]],
    };
    std::array::from_fn(|i| Vertex {
        position: positions[i],
        uv: uv[i],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_runs_preserve_every_face_across_material_and_index_boundaries() {
        let face = |material| PackedQuad::new([1, 2, 3], Face::PositiveY, 1, 1, material);
        let mut quads = vec![face(7); QUADS_PER_UPLOAD + 1];
        quads.extend([face(3), face(3), face(7)]);
        let runs: Vec<_> = material_runs(&quads).collect();
        assert_eq!(
            runs.iter().map(|run| run.len()).collect::<Vec<_>>(),
            [QUADS_PER_UPLOAD, 1, 2, 1]
        );
        let budget = Budget::new(1024 * 1024, 0);
        for run in &runs {
            assert!(CubeUpload::new(run, &budget).is_ok());
        }
        assert_eq!(runs.concat(), quads);
        assert_eq!(material_runs(&[]).count(), 0);
    }

    #[test]
    fn every_face_has_outward_winding_and_correct_plane() {
        for (face, axis, sign) in [
            (Face::NegativeX, 0, -1.),
            (Face::PositiveX, 0, 1.),
            (Face::NegativeY, 1, -1.),
            (Face::PositiveY, 1, 1.),
            (Face::NegativeZ, 2, -1.),
            (Face::PositiveZ, 2, 1.),
        ] {
            let vertices = corners(PackedQuad::new([2, 3, 4], face, 5, 7, 0));
            let a = std::array::from_fn::<_, 3, _>(|i| {
                vertices[1].position[i] - vertices[0].position[i]
            });
            let b = std::array::from_fn::<_, 3, _>(|i| {
                vertices[2].position[i] - vertices[0].position[i]
            });
            let normal = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            assert_eq!(normal[axis], sign * 35.);
            assert!(
                vertices
                    .iter()
                    .all(|v| v.position[axis]
                        == [2., 3., 4.][axis] + if sign > 0. { 1. } else { 0. })
            );
        }
    }

    #[test]
    fn full_batch_preserves_last_quad_and_all_indices() {
        let quad = PackedQuad::new([15, 15, 15], Face::PositiveY, 1, 1, 7);
        let budget = Budget::new(1024 * 1024, 0);
        let batch = CubeUpload::new(&vec![quad; QUADS_PER_UPLOAD], &budget).unwrap();
        assert_eq!(batch.vertices().len(), QUADS_PER_UPLOAD * 4);
        assert!(
            batch
                .indices()
                .iter()
                .all(|&index| usize::from(index) < batch.vertices().len())
        );
        assert_eq!(
            batch.indices().last().copied(),
            Some((QUADS_PER_UPLOAD * 4 - 1) as u16)
        );
        assert_eq!(batch.vertices().last().unwrap().position, [16., 16., 15.]);
        assert_eq!(budget.usage().used[0], batch.bytes());
        drop(batch);
        assert_eq!(budget.usage().used[0], 0);
    }

    #[test]
    fn rejected_upload_does_not_consume_capacity() {
        let budget = Budget::new(1, 0);
        let quad = PackedQuad::new([0, 0, 0], Face::PositiveX, 1, 1, 0);
        assert!(matches!(
            CubeUpload::new(&[quad], &budget),
            Err(UploadError::Pressure(_))
        ));
        assert_eq!(budget.usage().used, [0, 0]);
    }

    #[test]
    fn decoded_bedrock_subchunk_reaches_indexed_upload() {
        use assets::{NetworkIdMode, RuntimeAssets};
        use meshing::{BlockClassifier, Neighbourhood, mesh_sub_chunk};
        use world::{RawBlockIds, SubChunk};
        let mut bytes = vec![9, 1, 0, 3];
        let mut words = [0_u32; 128];
        let cell = 256 + (3 * 16) + 2;
        words[cell / 32] |= 1 << (cell % 32);
        for word in words {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend([4, 0, 2]);
        let subchunk = SubChunk::decode(&bytes, &RawBlockIds { air: 0 });
        assert_eq!(subchunk.runtime_id(0, 1, 2, 3), Some(1));
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(0),
            &RuntimeAssets::diagnostic(),
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &subchunk,
        );
        let upload = CubeUpload::new(mesh.cube_quads(), &Budget::new(4096, 0)).unwrap();
        assert_eq!(upload.indices().len(), 36);
        assert!(upload.vertices().iter().all(|vertex| {
            (1. ..=2.).contains(&vertex.position[0])
                && (2. ..=3.).contains(&vertex.position[1])
                && (3. ..=4.).contains(&vertex.position[2])
        }));
    }
}
