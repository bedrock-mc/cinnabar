//! Far-terrain tiles and the near-column mask the far pass draws with.

use bevy::prelude::Resource;
use far_terrain::{COLUMN_SIDE, ColumnPos, NearMask, TileKey, TileMesh, config::LEVEL_COUNT};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// What the far pass draws; replaced whole when anything in it changes.
#[derive(Resource, bevy::render::extract_resource::ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct FarTerrainScene {
    pub tiles: Arc<[Arc<TileMesh>]>,
    /// Columns near terrain draws; far quads over them are hidden.
    pub near: Arc<NearMask>,
    /// Drawable GPU revisions for this world, shared with the render-world extraction.
    pub residency: FarTerrainResidency,
}

/// A world's drawable tile revisions; a new world starts with a separate acknowledgement.
#[derive(Clone, Default)]
pub struct FarTerrainResidency(Arc<Mutex<HashMap<TileKey, u64>>>);

impl FarTerrainResidency {
    /// Whether the CPU-ready replacement is drawable over this column without another LOD.
    /// An empty tile is ready only after every old covering draw has retired.
    #[must_use]
    pub fn ready(&self, column: ColumnPos, mesh: &TileMesh) -> bool {
        let block = [column.x * COLUMN_SIDE as i32, column.z * COLUMN_SIDE as i32];
        if mesh.key.level >= LEVEL_COUNT
            || mesh.key != TileKey::containing(mesh.key.level, block[0], block[1])
        {
            return false;
        }
        let resident = self.0.lock().expect("far terrain residency");
        (0..LEVEL_COUNT).all(|level| {
            let key = TileKey::containing(level, block[0], block[1]);
            if key == mesh.key && !mesh.quads.is_empty() {
                resident.get(&key) == Some(&mesh.revision)
            } else {
                !resident.contains_key(&key)
            }
        })
    }

    /// Whether two extractions acknowledge tiles in the same world.
    #[must_use]
    pub fn same_world(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Publishes only drawable revisions, after their uploads and coverage changes are queued.
    pub fn publish(&self, tiles: impl Iterator<Item = (TileKey, u64)>) {
        let mut resident = self.0.lock().expect("far terrain residency");
        resident.clear();
        resident.extend(tiles);
    }
}
