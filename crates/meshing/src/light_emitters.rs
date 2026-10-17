//! Block-state light sources retained independently of greedy surface merging.

#[cfg(feature = "enhanced")]
use assets::{BlockFace, NetworkIdMode, RuntimeAssets};
#[cfg(any(feature = "enhanced", test))]
use std::collections::BTreeMap;
#[cfg(any(feature = "enhanced", test))]
use world::{SUB_CHUNK_SIDE, SubChunk};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockLightEmitter {
    pub position: [u8; 3],
    pub emission: u8,
    pub material: u32,
}

/// Collects emitting cells from every storage without requiring a visible face.
#[cfg(feature = "enhanced")]
pub(crate) fn collect_emitters(
    source: &SubChunk,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
) -> Box<[BlockLightEmitter]> {
    collect(source, |id| {
        let block = assets.resolve(mode, id);
        (
            block.light_properties().emission(),
            block.face(BlockFace::Up).material_id(),
        )
    })
}

/// Keeps the strongest emitter at each cell across all storages.
#[cfg(any(feature = "enhanced", test))]
fn collect(source: &SubChunk, resolve: impl Fn(u32) -> (u8, u32)) -> Box<[BlockLightEmitter]> {
    let mut sources = BTreeMap::new();
    for storage in source.storages() {
        let emitting: BTreeMap<_, _> = storage
            .palette()
            .values()
            .iter()
            .copied()
            .filter_map(|id| {
                let value = resolve(id);
                (value.0 > 0).then_some((id, value))
            })
            .collect();
        if emitting.is_empty() {
            continue;
        }
        for x in 0..SUB_CHUNK_SIDE as u8 {
            for y in 0..SUB_CHUNK_SIDE as u8 {
                for z in 0..SUB_CHUNK_SIDE as u8 {
                    let Some(value) = storage
                        .runtime_id(x, y, z)
                        .and_then(|id| emitting.get(&id))
                        .copied()
                    else {
                        continue;
                    };
                    let entry = sources.entry([x, y, z]).or_insert(value);
                    if value.0 > entry.0 {
                        *entry = value;
                    }
                }
            }
        }
    }
    sources
        .into_iter()
        .map(|(position, (emission, material))| BlockLightEmitter {
            position,
            emission,
            material,
        })
        .collect()
}

impl crate::ChunkMesh {
    /// Returns block emitters collected by an Enhanced mesh build.
    #[must_use]
    pub fn light_emitters(&self) -> &[crate::BlockLightEmitter] {
        #[cfg(feature = "enhanced")]
        {
            &self.light_emitters
        }
        #[cfg(not(feature = "enhanced"))]
        {
            &[]
        }
    }

    /// Removes emitters without changing the visible geometry or its byte accounting.
    #[cfg(feature = "enhanced")]
    pub fn take_light_emitters(&mut self) -> Box<[crate::BlockLightEmitter]> {
        std::mem::take(&mut self.light_emitters)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world::RawBlockIds;

    #[test]
    fn non_emitting_palette_does_not_create_sources() {
        let source = SubChunk::decode(&[9, 1, 0, 1, 2], &RawBlockIds { air: 0 });
        assert!(collect(&source, |_| (0, 17)).is_empty());
    }

    #[test]
    fn uniform_emitter_cells_keep_positions_instead_of_one_greedy_quad_light() {
        let source = SubChunk::decode(&[9, 1, 0, 1, 2], &RawBlockIds { air: 0 });
        let sources = collect(&source, |_| (12, 17));
        assert_eq!(sources.len(), world::BLOCKS_PER_SUB_CHUNK);
        assert_eq!(
            sources[0],
            BlockLightEmitter {
                position: [0, 0, 0],
                emission: 12,
                material: 17
            }
        );
        assert_eq!(sources.last().unwrap().position, [15, 15, 15]);
    }
}
