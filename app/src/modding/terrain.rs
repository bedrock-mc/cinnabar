//! Samples terrain for terrain-authorized mods before their frame callbacks.
use std::sync::Arc;

use super::gameplay::GameplayContext;
use assets::NetworkIdMode;
use bevy::prelude::DetectChanges;
use client_extensions::modding::multi::ModHosts;
use client_interaction::movement::PhysicsCollisionRegistries;
use mod_host::{
    TerrainBlockState, TerrainCheck, TerrainChunkRevision, TerrainGeometry, TerrainPhysics,
    TerrainPhysicsFlags, TerrainQuery, TerrainReadError, TerrainRefresh, TerrainRegion,
    TerrainShape, TerrainShapeBinding, TerrainShapeOverride, TerrainStamp, TerrainSurfaceResponse,
};
use sim::{BlockPhysicsFlags, PaletteWorld, SurfaceResponse};
use world::{ChunkKey, ChunkStore};

/// Each simulation physics flag beside its contract flag.
const PHYSICS_FLAGS: [(BlockPhysicsFlags, TerrainPhysicsFlags); 7] = [
    (BlockPhysicsFlags::CLIMBABLE, TerrainPhysicsFlags::CLIMBABLE),
    (BlockPhysicsFlags::WATER, TerrainPhysicsFlags::WATER),
    (BlockPhysicsFlags::LAVA, TerrainPhysicsFlags::LAVA),
    (BlockPhysicsFlags::COBWEB, TerrainPhysicsFlags::COBWEB),
    (
        BlockPhysicsFlags::POWDER_SNOW,
        TerrainPhysicsFlags::POWDER_SNOW,
    ),
    (
        BlockPhysicsFlags::SCAFFOLDING,
        TerrainPhysicsFlags::SCAFFOLDING,
    ),
    (BlockPhysicsFlags::PASSABLE, TerrainPhysicsFlags::PASSABLE),
];

/// Sampler scratch, cached block metadata, the registry generation they describe and the
/// round-robin cursor across every mod's subscriptions.
#[derive(Default)]
pub(super) struct TerrainSampling {
    sampler: sim::TerrainSampler,
    descriptors: mod_host::TerrainDescriptors,
    scheduler: mod_host::TerrainScheduler,
    scope: Option<(u64, NetworkIdMode)>,
    generation: u64,
}

/// The block registry's metadata in the session's runtime-ID space.
struct Metadata<'a>(&'a PhysicsCollisionRegistries, NetworkIdMode);
impl mod_host::TerrainMetadata for Metadata<'_> {
    fn identifier(&self, runtime_id: u32) -> Option<&str> {
        self.0.block_identifier(self.1, runtime_id)
    }
    fn canonical_state(&self, runtime_id: u32) -> Option<&str> {
        self.0.block_canonical_state(self.1, runtime_id)
    }
    fn tags(&self, runtime_id: u32) -> &[Arc<str>] {
        self.0.block_tags(self.1, runtime_id)
    }
}

/// This frame's world, with the scratch that samples it.
struct Source<'a> {
    store: &'a ChunkStore,
    registries: &'a PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    observation: &'a mod_host::WorldObservation,
    generation: u64,
    sampler: &'a mut sim::TerrainSampler,
    descriptors: &'a mut mod_host::TerrainDescriptors,
}

impl TerrainSampling {
    /// Refreshes every mod's subscriptions once, before callbacks, under one cell budget.
    /// Without a current world, subscriptions keep their samples until it returns.
    pub(super) fn refresh(
        &mut self,
        hosts: &mut ModHosts,
        context: &GameplayContext<'_>,
        observation: Option<&mod_host::WorldObservation>,
    ) {
        let authority = context
            .world
            .as_ref()
            .and_then(|world| world.stream.as_ref())
            .map(|stream| stream.authority());
        let (Some(observation), Some(authority), Some(registries)) =
            (observation, authority, context.collisions.as_ref())
        else {
            return;
        };
        let mode = authority.network_id_mode();
        let scope = Some((observation.session, mode));
        if self.scope != scope || registries.is_changed() {
            self.scope = scope;
            self.generation += 1;
        }
        let mut source = Source {
            store: authority.terrain(),
            registries,
            mode,
            observation,
            generation: self.generation,
            sampler: &mut self.sampler,
            descriptors: &mut self.descriptors,
        };
        let mut hosts: Vec<_> = hosts.iter_mut().collect();
        self.scheduler
            .refresh(&mut hosts, mod_api::MAX_TERRAIN_FRAME_CELLS, &mut source);
    }
}

impl Source<'_> {
    fn stamp(&self) -> TerrainStamp {
        TerrainStamp {
            session: self.observation.session,
            dimension: self.observation.dimension,
            registry: self.generation,
            revision: 0,
        }
    }

    /// Moves a sample's bulk arrays into a response and copies metadata once per state.
    fn region(&mut self, sample: sim::TerrainSample) -> TerrainRefresh {
        let metadata = Metadata(self.registries, self.mode);
        let mut bytes = 0;
        let mut palette = Vec::with_capacity(sample.palette.len());
        for state in &sample.palette {
            let (description, size) =
                self.descriptors
                    .describe(self.generation, state.runtime_id, &metadata);
            bytes += size;
            if bytes > mod_api::MAX_TERRAIN_METADATA_BYTES {
                return TerrainRefresh::Failed(TerrainReadError::LimitExceeded);
            }
            palette.push(TerrainBlockState {
                physics: state.physics.map(physics),
                collision: binding(state.collision),
                selection: binding(state.selection),
                ..description.clone()
            });
        }
        TerrainRefresh::Replace(Box::new(TerrainRegion {
            stamp: self.stamp(),
            chunks: sample
                .columns
                .iter()
                .map(|column| TerrainChunkRevision {
                    x: column.x,
                    z: column.z,
                    revision: column.revision,
                })
                .collect(),
            layer_count: sample.layer_count,
            loaded: sample.loaded,
            states: sample.states,
            palette,
            shape_offsets: sample.shape_offsets,
            shape_boxes: sample.shape_boxes,
            overrides: sample
                .overrides
                .iter()
                .map(|entry| TerrainShapeOverride {
                    slot: entry.slot,
                    collision: entry.collision.map(shape),
                    selection: entry.selection.map(shape),
                })
                .collect(),
        }))
    }
}

impl mod_host::TerrainSource for Source<'_> {
    /// Current when the session, dimension, registry and every column read are unchanged.
    fn check(&mut self, query: &TerrainQuery, retained: Option<&TerrainRegion>) -> TerrainCheck {
        let position = self.observation.position;
        let size = match mod_host::terrain_query_size(query, [position.0, position.1, position.2]) {
            Ok(size) => size,
            Err(error) => return TerrainCheck::Failed(error),
        };
        let stamp = self.stamp();
        let current = retained.is_some_and(|region| {
            (
                region.stamp.session,
                region.stamp.dimension,
                region.stamp.registry,
            ) == (stamp.session, stamp.dimension, stamp.registry)
                && region.chunks.iter().all(|chunk| {
                    let key = ChunkKey::new(stamp.dimension, chunk.x, chunk.z);
                    self.store
                        .collision_revision(key)
                        .map(|value| value.revision)
                        == chunk.revision
                })
        });
        if current {
            return TerrainCheck::Current;
        }
        TerrainCheck::Stale {
            cells: size.iter().map(|&side| usize::from(side)).product(),
        }
    }

    fn sample(&mut self, query: &TerrainQuery) -> TerrainRefresh {
        let position = self.observation.position;
        let size = match mod_host::terrain_query_size(query, [position.0, position.1, position.2]) {
            Ok(size) => size,
            Err(error) => return TerrainRefresh::Failed(error),
        };
        let geometry = sim::TerrainGeometry {
            collision: query.geometry.contains(TerrainGeometry::COLLISION),
            selection: query.geometry.contains(TerrainGeometry::SELECTION),
        };
        let registry = self.registries.registry(self.mode);
        let world = PaletteWorld::new(
            self.store,
            registry,
            self.observation.dimension,
            Default::default(),
        );
        let min = [query.min.x, query.min.y, query.min.z];
        match self
            .sampler
            .sample(&world, min, size, geometry, mod_api::MAX_TERRAIN_BOXES)
        {
            Ok(sample) => self.region(sample),
            Err(sim::TerrainSampleError::OutOfRange) => {
                TerrainRefresh::Failed(TerrainReadError::OutOfRange)
            }
            Err(_) => TerrainRefresh::Failed(TerrainReadError::LimitExceeded),
        }
    }
}

fn physics(facts: sim::BlockPhysicsFacts) -> TerrainPhysics {
    TerrainPhysics {
        friction: facts.friction,
        horizontal_speed_factor: facts.horizontal_speed_factor,
        vertical_speed_factor: facts.vertical_speed_factor,
        fluid_height: facts.fluid_height_blocks,
        flags: PHYSICS_FLAGS
            .iter()
            .filter(|(flag, _)| facts.flags.contains(*flag))
            .fold(TerrainPhysicsFlags::empty(), |flags, (_, flag)| {
                flags | *flag
            }),
        surface: match facts.surface_response {
            SurfaceResponse::None => TerrainSurfaceResponse::None,
            SurfaceResponse::Slime => TerrainSurfaceResponse::Slime,
            SurfaceResponse::Bed => TerrainSurfaceResponse::Bed,
            SurfaceResponse::Honey => TerrainSurfaceResponse::Honey,
            SurfaceResponse::SoulSand => TerrainSurfaceResponse::SoulSand,
            SurfaceResponse::BubbleUp => TerrainSurfaceResponse::BubbleUp,
            SurfaceResponse::BubbleDown => TerrainSurfaceResponse::BubbleDown,
        },
    }
}

fn shape(value: sim::TerrainShape) -> TerrainShape {
    match value {
        sim::TerrainShape::Unavailable => TerrainShape::Unavailable,
        sim::TerrainShape::Known(index) => TerrainShape::Known(index),
    }
}

fn binding(value: sim::TerrainShapeBinding) -> TerrainShapeBinding {
    match value {
        sim::TerrainShapeBinding::NotRequested => TerrainShapeBinding::NotRequested,
        sim::TerrainShapeBinding::Fixed(value) => TerrainShapeBinding::Fixed(shape(value)),
        sim::TerrainShapeBinding::PerCell => TerrainShapeBinding::PerCell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every simulation flag maps to its own contract flag, so none is dropped.
    #[test]
    fn physics_flags_cover_every_simulation_flag() {
        let mut covered = 0;
        let mut seen = TerrainPhysicsFlags::empty();
        for (flag, contract) in PHYSICS_FLAGS {
            assert_eq!(flag.bits().count_ones(), 1);
            assert!(!seen.intersects(contract));
            covered |= flag.bits();
            seen |= contract;
        }
        assert_eq!(covered, BlockPhysicsFlags::KNOWN_BITS);
        assert_eq!(seen, TerrainPhysicsFlags::all());
    }
}
