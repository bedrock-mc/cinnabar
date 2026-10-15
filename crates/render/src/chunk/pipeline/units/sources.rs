//! Retained section descriptors in the exact order used by the per-section commands.

use bevy::render::sync_world::MainEntityHashMap;
use render_model::terrain_units::TerrainUnit;

use crate::chunk::pipeline::batches::DrawableTerrain;
use crate::chunk::*;

/// The validated geometry identity for one stream of an allocation.
#[derive(PartialEq, Eq)]
struct SourceIdentity {
    key: SubChunkKey,
    generation: u64,
    origin: u32,
    ranges: [Option<Range<u32>>; 6],
}

impl SourceIdentity {
    /// Includes address reuse, metadata moves and command validation, not just mesh generation.
    fn new(allocation: &GpuChunkAllocation, stream: usize) -> Self {
        let mut ranges: [Option<Range<u32>>; 6] = default();
        if stream == 0 {
            if let Some((cube, layout, _)) = cube_draw_base(allocation) {
                ranges = CubeQuadLayout::SOLID_FACE_ORDER.map(|face| {
                    let range = layout.solid_range(face);
                    Some(cube.start + range.start..cube.start + range.end)
                });
            }
        } else {
            let command = match stream {
                1 => cutout_indirect_command(allocation),
                2 => model_direct_draw_command(allocation),
                3 => depth_liquid_direct_draw_command(allocation),
                _ => unreachable!("opaque terrain has four streams"),
            };
            ranges[0] = command.map(|command| {
                command.first_instance..command.first_instance + command.instance_count
            });
        }
        Self {
            key: allocation.key,
            generation: allocation.generation,
            origin: allocation.metadata_index,
            ranges,
        }
    }

    /// Splits validated commands without crossing section, stream or directional-run boundaries.
    fn units(&self, stream: usize) -> Vec<TerrainUnit> {
        let bounds = if stream == 2 {
            bounds::MODEL_BOUNDS
        } else {
            bounds::FULL_BOUNDS
        };
        self.ranges
            .iter()
            .enumerate()
            .flat_map(|(index, range)| {
                range.iter().flat_map(move |range| {
                    TerrainUnit::build(
                        range.clone(),
                        self.origin,
                        bounds,
                        (stream == 0).then(|| CubeQuadLayout::SOLID_FACE_ORDER[index] as u8),
                        self.generation,
                    )
                })
            })
            .collect()
    }
}

/// A section's immutable descriptors, shared across unrelated source-list changes.
pub(super) struct SectionUnits {
    pub(super) entity: Entity,
    pub(super) key: SubChunkKey,
    pub(super) units: Vec<TerrainUnit>,
    identity: SourceIdentity,
}

/// The ordinary pass keeps Bevy's bin insertion order and each bin's entity hash order.
#[derive(Default)]
pub(in crate::chunk) struct UnitSources {
    pub(super) streams: [Vec<Arc<SectionUnits>>; 4],
    pub(super) lookup: [HashMap<Entity, usize>; 4],
    pub(in crate::chunk) order: Vec<usize>,
}

impl UnitSources {
    /// Recreates pass ordering while keeping every unchanged section's descriptor allocation.
    pub(in crate::chunk) fn build(
        drawable: &[DrawableTerrain],
        allocations: &Query<&GpuChunkAllocation>,
        previous: Option<&Self>,
    ) -> Self {
        #[cfg(feature = "tracy")]
        let _span =
            bevy::log::info_span!("terrain.unit_descriptors", sections = drawable.len()).entered();
        let mut bins: [MainEntityHashMap<Entity>; 4] = default();
        let mut result = Self::default();
        for terrain in drawable {
            for (stream, enabled) in [
                terrain.solid,
                terrain.cutout,
                terrain.model,
                terrain.depth_liquid,
            ]
            .into_iter()
            .enumerate()
            {
                if enabled {
                    if bins[stream].is_empty() {
                        result.order.push(stream);
                    }
                    bins[stream].insert(terrain.main, terrain.entity);
                }
            }
        }
        for (stream, bin) in bins.iter().enumerate() {
            for &entity in bin.values() {
                let Ok(allocation) = allocations.get(entity) else {
                    continue;
                };
                let identity = SourceIdentity::new(allocation, stream);
                let old = previous.and_then(|old| {
                    old.lookup[stream]
                        .get(&entity)
                        .map(|&index| &old.streams[stream][index])
                });
                let section = if let Some(old) = old.filter(|old| old.identity == identity) {
                    Arc::clone(old)
                } else {
                    Arc::new(SectionUnits {
                        entity,
                        key: identity.key,
                        units: identity.units(stream),
                        identity,
                    })
                };
                result.lookup[stream].insert(entity, result.streams[stream].len());
                result.streams[stream].push(section);
            }
        }
        result
    }
}

/// Builds a stream's descriptors for the direct-command equivalence fixtures.
#[cfg(test)]
pub(super) fn allocation_units(allocation: &GpuChunkAllocation, stream: usize) -> Vec<TerrainUnit> {
    SourceIdentity::new(allocation, stream).units(stream)
}

#[cfg(test)]
impl SectionUnits {
    /// Builds isolated slot-allocation fixtures without requiring a render device.
    pub(super) fn fixture(entity: Entity, units: Vec<TerrainUnit>) -> Arc<Self> {
        let key = SubChunkKey::new(0, 0, 0, 0);
        Arc::new(Self {
            entity,
            key,
            units,
            identity: SourceIdentity {
                key,
                generation: 0,
                origin: 0,
                ranges: default(),
            },
        })
    }
}
