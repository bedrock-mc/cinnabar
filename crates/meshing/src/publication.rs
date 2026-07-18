use crate::{
    ChunkMesh, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad,
    PackedQuadLighting,
};

/// Bytes occupied by one chunk origin in the GPU storage arena.
pub const CHUNK_PUBLICATION_ORIGIN_BYTES: u64 = 32;

/// Exact non-growth GPU payload admitted for one chunk publication.
///
/// Empty meshes are zero-byte operations even when their source biome record is
/// non-fallback: no biome or origin allocation survives an empty publication.
#[must_use]
pub fn chunk_publication_byte_len(mesh: &ChunkMesh, biome: &PackedBiomeRecord) -> u64 {
    if mesh.is_empty() {
        return 0;
    }

    stream_bytes::<PackedQuad>(mesh.cube_quads().len())
        .saturating_add(stream_bytes::<PackedQuadLighting>(
            mesh.cube_lighting().len(),
        ))
        .saturating_add(stream_bytes::<PackedModelRef>(mesh.model_refs().len()))
        .saturating_add(stream_bytes::<PackedQuadLighting>(
            mesh.model_lighting().len(),
        ))
        .saturating_add(stream_bytes::<PackedModelDrawRef>(
            mesh.model_draw_refs().len(),
        ))
        .saturating_add(stream_bytes::<PackedModelDrawRef>(
            mesh.transparent_model_draw_refs().len(),
        ))
        .saturating_add(stream_bytes::<PackedLiquidQuad>(mesh.liquid_quads().len()))
        .saturating_add(stream_bytes::<PackedQuadLighting>(
            mesh.liquid_lighting().len(),
        ))
        .saturating_add(if biome.is_fallback() {
            0
        } else {
            biome.byte_len()
        })
        .saturating_add(CHUNK_PUBLICATION_ORIGIN_BYTES)
}

fn stream_bytes<T>(len: usize) -> u64 {
    u64::try_from(len)
        .unwrap_or(u64::MAX)
        .saturating_mul(std::mem::size_of::<T>() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FaceConnectivity;

    #[test]
    fn empty_mesh_is_always_a_zero_byte_publication() {
        let storage = world::DecodedBiomeColumn::decode(0, 1, &[1, 2])
            .unwrap()
            .storage(0)
            .unwrap();
        let non_fallback = PackedBiomeRecord::from_storage(&storage, |id| id);
        assert!(!non_fallback.is_fallback());
        assert_eq!(
            chunk_publication_byte_len(&ChunkMesh::default(), &non_fallback),
            0
        );
    }

    #[test]
    fn non_empty_mesh_includes_stream_and_origin_bytes_once() {
        let mesh = ChunkMesh::from_streams(
            Vec::new(),
            vec![PackedModelRef::default()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            FaceConnectivity::default(),
        );
        assert_eq!(
            chunk_publication_byte_len(&mesh, &PackedBiomeRecord::fallback()),
            std::mem::size_of::<PackedModelRef>() as u64 + CHUNK_PUBLICATION_ORIGIN_BYTES
        );
    }
}
