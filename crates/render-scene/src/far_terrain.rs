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
    /// Near publication source; retained through feature toggles and replaced with the stream epoch.
    pub publications: Option<crate::terrain_publication::TerrainPublications>,
    pub tiles: Arc<[Arc<TileMesh>]>,
    /// CPU selection mask; used directly only without a render publication target.
    pub near: Arc<NearMask>,
    /// Drawable GPU revisions for this world, shared with the render-world extraction.
    pub residency: FarTerrainResidency,
    /// Requested near publications; render ownership determines the effective mask.
    pub target: Option<Arc<far_terrain::TerrainTarget>>,
}

/// A world's drawable tile revisions; a new world starts with a separate acknowledgement.
#[derive(Clone, Default)]
pub struct FarTerrainResidency(Arc<Mutex<Residency>>);

#[derive(Default)]
struct Residency {
    tiles: HashMap<TileKey, u64>,
    near: Option<std::collections::HashSet<far_terrain::ColumnPos>>,
}

impl FarTerrainResidency {
    /// Whether the CPU-ready replacement is drawable over this column without another LOD.
    /// An empty tile is ready only after every old covering draw has retired.
    #[must_use]
    pub fn ready(&self, column: ColumnPos, mesh: &TileMesh) -> bool {
        let block = [column.x * COLUMN_SIDE as i32, column.z * COLUMN_SIDE as i32];
        if !mesh.covers(column)
            || mesh.key.level >= LEVEL_COUNT
            || mesh.key != TileKey::containing(mesh.key.level, block[0], block[1])
        {
            return false;
        }
        let resident = self.0.lock().expect("far terrain residency");
        (0..LEVEL_COUNT).all(|level| {
            let key = TileKey::containing(level, block[0], block[1]);
            if key == mesh.key && !mesh.quads.is_empty() {
                resident.tiles.get(&key) == Some(&mesh.revision)
            } else {
                !resident.tiles.contains_key(&key)
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
        resident.tiles.clear();
        resident.tiles.extend(tiles);
    }

    /// Acknowledges actual near draw ownership, independently of physical retirement receipts.
    pub fn publish_near(&self, columns: impl Iterator<Item = ColumnPos>) {
        self.0.lock().expect("far terrain residency").near = Some(columns.collect());
    }

    /// Whether the render owner has stopped drawing this outgoing column's near geometry.
    #[must_use]
    pub fn may_remove_near(&self, column: ColumnPos) -> bool {
        self.0
            .lock()
            .expect("far terrain residency")
            .near
            .as_ref()
            .is_some_and(|near| !near.contains(&column))
    }
}
