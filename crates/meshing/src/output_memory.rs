use assets::{NetworkIdMode, RuntimeAssets};
use world::{BLOCKS_PER_SUB_CHUNK, SubChunk};

use crate::{
    ChunkMesh, DiagnosticGeometryCount, Face, MAX_DIAGNOSTIC_IDENTITIES_PER_MESH,
    MAX_PACKED_BIOME_RECORD_WORDS, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting,
    chunk::models::MAX_SELECTED_MODEL_TEMPLATES,
    types::{CubeStreams, ModelDrawRefs},
};

/// Conservative packed-output sizes for one immutable runtime asset set.
#[derive(Debug)]
pub struct MeshOutputBounds {
    with_models: u64,
}

impl MeshOutputBounds {
    /// Accounts for every cell, face, selected model and compound model part.
    #[must_use]
    pub fn new(assets: &RuntimeAssets) -> Self {
        let quads = assets
            .model_templates()
            .iter()
            .map(|t| u64::from(t.quad_count))
            .max()
            .unwrap_or(0);
        let offset_prefix = assets
            .model_templates()
            .iter()
            .enumerate()
            .any(|(id, _)| assets.model_random_offset(id as u32).is_some());
        let part = bytes::<PackedModelRef>(1)
            + if offset_prefix {
                bytes::<PackedQuadLighting>(2)
            } else {
                0
            }
            + quads * (bytes::<PackedQuadLighting>(1) + bytes::<PackedModelDrawRef>(1));
        let mut chain = 0_u64;
        let mut max_parts = 1_u64;
        for template in assets.model_templates() {
            chain += 1;
            max_parts = max_parts.max(chain);
            if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
                chain = 0;
            }
        }
        Self {
            with_models: plain_bound()
                + BLOCKS_PER_SUB_CHUNK as u64
                    * MAX_SELECTED_MODEL_TEMPLATES as u64
                    * max_parts
                    * part,
        }
    }

    /// Avoids reserving model streams when no source palette can emit a model.
    #[must_use]
    pub fn for_sub_chunk(
        &self,
        source: &SubChunk,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
    ) -> u64 {
        let models = source.storages().iter().any(|storage| {
            storage
                .palette()
                .values()
                .iter()
                .any(|&id| assets.resolve(mode, id).model_template().is_some())
        });
        if models {
            self.with_models
        } else {
            plain_bound()
        }
    }
}

/// Counts owned packed streams and biome words, including empty mesh allocations.
#[must_use]
pub fn mesh_output_byte_len(mesh: &ChunkMesh, biome: &PackedBiomeRecord) -> u64 {
    bytes::<ChunkMesh>(1)
        + bytes::<CubeStreams>(1)
        + bytes::<ModelDrawRefs>(1)
        + bytes::<PackedQuad>(mesh.cube_quads().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.cube_lighting().len() as u64)
        + bytes::<PackedModelRef>(mesh.model_refs().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.model_lighting().len() as u64)
        + bytes::<PackedModelDrawRef>(
            (mesh.model_draw_refs().len() + mesh.transparent_model_draw_refs().len()) as u64,
        )
        + bytes::<PackedLiquidQuad>(mesh.liquid_quads().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.liquid_lighting().len() as u64)
        + bytes::<DiagnosticGeometryCount>(mesh.diagnostic_geometry().entries().len() as u64)
        + biome.byte_len()
}

/// Reserves all cube and liquid faces, plus the largest packed biome descriptor.
fn plain_bound() -> u64 {
    bytes::<ChunkMesh>(1)
        + bytes::<CubeStreams>(1)
        + bytes::<ModelDrawRefs>(1)
        + bytes::<DiagnosticGeometryCount>(MAX_DIAGNOSTIC_IDENTITIES_PER_MESH as u64)
        + bytes::<u32>(MAX_PACKED_BIOME_RECORD_WORDS as u64)
        + BLOCKS_PER_SUB_CHUNK as u64
            * Face::ALL.len() as u64
            * (bytes::<PackedQuad>(1)
                + bytes::<PackedLiquidQuad>(1)
                + 2 * bytes::<PackedQuadLighting>(1))
}

/// Converts an owned stream length to bytes without depending on wire layout literals.
const fn bytes<T>(count: u64) -> u64 {
    count * std::mem::size_of::<T>() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_payload_is_charged_even_without_geometry() {
        let mut mesh = ChunkMesh::default();
        let biome = PackedBiomeRecord::fallback();
        let empty_bytes = mesh_output_byte_len(&mesh, &biome);
        mesh.cube_streams.diagnostic_geometry = crate::DiagnosticGeometrySummary::from_counts(
            (0..MAX_DIAGNOSTIC_IDENTITIES_PER_MESH)
                .map(|id| DiagnosticGeometryCount::new(None, id as u32, 1)),
        );
        assert_eq!(
            mesh_output_byte_len(&mesh, &biome) - empty_bytes,
            bytes::<DiagnosticGeometryCount>(MAX_DIAGNOSTIC_IDENTITIES_PER_MESH as u64)
        );
        assert!(mesh_output_byte_len(&mesh, &biome) <= plain_bound());
    }
}
