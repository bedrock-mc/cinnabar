use std::collections::{HashMap, HashSet};

use bevy::{prelude::*, render::extract_resource::ExtractResource};
use world::SubChunkKey;

use crate::dropped_item::{DroppedItemScene, TerrainItemInstance};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TerrainItemSessionSet;

/// Successful zero-byte changes applied in the main world before extraction.
#[derive(Resource, Default, Clone, ExtractResource)]
pub(crate) struct ImmediateTerrainMeshPublications(pub(crate) Vec<(SubChunkKey, u64)>);

#[derive(Resource, Default)]
pub(crate) struct TerrainItemMeshGenerations {
    session_id: Option<u64>,
    required: HashSet<SubChunkKey>,
    generations: HashMap<SubChunkKey, u64>,
}

impl TerrainItemMeshGenerations {
    pub(crate) fn record(&mut self, key: SubChunkKey, generation: u64) {
        if self.required.contains(&key) {
            self.generations
                .entry(key)
                .and_modify(|current| {
                    *current = (*current).max(generation);
                })
                .or_insert(generation);
        }
    }

    pub(super) fn visible(&self, candidate: &TerrainItemInstance) -> bool {
        let mut visible = candidate.visible;
        for transition in candidate.transitions.iter() {
            if self
                .generations
                .get(&transition.key)
                .is_some_and(|&generation| generation >= transition.generation)
            {
                visible = transition.visible;
            }
        }
        visible
    }
}

pub(super) fn begin_frame(
    scene: Res<DroppedItemScene>,
    immediate: Option<Res<ImmediateTerrainMeshPublications>>,
    mut ledger: ResMut<TerrainItemMeshGenerations>,
) {
    if ledger.session_id != scene.terrain_session_id {
        ledger.generations.clear();
        ledger.session_id = scene.terrain_session_id;
    }
    ledger.required.clear();
    ledger
        .required
        .extend(scene.terrain_instances.iter().flat_map(|candidate| {
            candidate
                .transitions
                .iter()
                .map(|transition| transition.key)
        }));
    let TerrainItemMeshGenerations {
        required,
        generations,
        ..
    } = &mut *ledger;
    generations.retain(|key, _| required.contains(key));
    if let Some(immediate) = immediate {
        for &(key, generation) in &immediate.0 {
            ledger.record(key, generation);
        }
    }
}
