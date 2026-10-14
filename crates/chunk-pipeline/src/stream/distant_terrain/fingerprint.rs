//! What a column's block hash was computed from, so the frame can reuse the hash for a
//! later capture of the same content and skip it without queueing a job.
//!
//! The world replaces a sub-chunk's or biome storage's allocation whenever its content
//! changes and keeps it for an equal resend, so the same allocations mean the same
//! content. Weak handles keep only the allocations' addresses reserved, never their data.

use std::sync::{Arc, Weak};

use world::{BiomeStorage, SubChunk};

use super::{blocks::SessionBlocks, shared::ColumnInput};

/// One sub-chunk's Y, its allocation, and the biome storage allocation at that Y.
type SubChunkHandles = (i32, Weak<SubChunk>, Option<Weak<BiomeStorage>>);

/// The allocations one block hash covered, and that hash.
pub(in crate::stream) struct Fingerprint {
    blocks: Weak<SessionBlocks>,
    /// Each sub-chunk by Y with the biome storage at that Y.
    sub_chunks: Box<[SubChunkHandles]>,
    /// The block hash of these allocations.
    pub(in crate::stream) hash: u64,
}

impl Fingerprint {
    /// The fingerprint of `input`, whose blocks and biomes hash to `hash`.
    pub(in crate::stream) fn of(input: &ColumnInput, hash: u64) -> Self {
        use lod::sampler::BiomeSource;
        Self {
            blocks: Arc::downgrade(&input.blocks),
            sub_chunks: input
                .sub_chunks
                .iter()
                .map(|(&y, sub_chunk)| {
                    let biome = input.biomes.biome_storage(y);
                    (
                        y,
                        Arc::downgrade(sub_chunk),
                        biome.as_ref().map(Arc::downgrade),
                    )
                })
                .collect(),
            hash,
        }
    }

    /// True when `input` holds exactly the allocations this fingerprint covered,
    /// classified by the same block identities.
    pub(in crate::stream) fn matches(&self, input: &ColumnInput) -> bool {
        use lod::sampler::BiomeSource;
        std::ptr::eq(self.blocks.as_ptr(), Arc::as_ptr(&input.blocks))
            && self.sub_chunks.len() == input.sub_chunks.len()
            && self.sub_chunks.iter().zip(input.sub_chunks.iter()).all(
                |((y, sub_chunk, biome), (&input_y, input_sub_chunk))| {
                    let input_biome = input.biomes.biome_storage(input_y);
                    *y == input_y
                        && std::ptr::eq(sub_chunk.as_ptr(), Arc::as_ptr(input_sub_chunk))
                        && match (biome, &input_biome) {
                            (Some(biome), Some(input_biome)) => {
                                std::ptr::eq(biome.as_ptr(), Arc::as_ptr(input_biome))
                            }
                            (None, None) => true,
                            _ => false,
                        }
                },
            )
    }
}
