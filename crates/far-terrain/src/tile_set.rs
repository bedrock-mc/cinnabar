//! The selected tiles around the camera, rebuilt on workers when their columns change.

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::{
    ColumnPos, ColumnSummary, EncodedSummary, SummaryStore, TileGrid, TileKey, TileMesh, TileView,
    config::{LEVEL_COUNT, MAX_TILE_BUILDS, TILE_BUILD_COLUMNS_PER_UPDATE, TILE_REBUILD_INTERVAL},
    mesh_tile, select_tiles,
    summary::COLUMN_SIDE,
};

#[derive(Default)]
struct Tile {
    mesh: Option<Arc<TileMesh>>,
    /// Columns changed since the mesh, or its build, started.
    dirty: bool,
    /// The build whose result is still wanted.
    building: Option<u64>,
    last_build: Option<Instant>,
}

struct Built {
    key: TileKey,
    token: u64,
    mesh: TileMesh,
}

/// Selected far-terrain tiles and their meshes. A tile replaced by a finer or coarser one stays
/// drawn until everything covering its area has a mesh, so changing level never opens a hole.
pub struct FarTiles {
    view: Option<TileView>,
    selected: BTreeMap<TileKey, Tile>,
    retired: Vec<Arc<TileMesh>>,
    next_token: u64,
    in_flight: usize,
    tx: Sender<Built>,
    rx: Receiver<Built>,
    revision: u64,
    /// Reused each update so an idle frame allocates nothing.
    due: Vec<TileKey>,
}

impl Default for FarTiles {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            view: None,
            selected: BTreeMap::new(),
            retired: Vec::new(),
            next_token: 0,
            in_flight: 0,
            tx,
            rx,
            revision: 0,
            due: Vec::new(),
        }
    }
}

impl FarTiles {
    /// Reselects tiles when the view differs from the last one.
    pub fn set_view(&mut self, view: TileView) {
        if self.view == Some(view) {
            return;
        }
        self.view = Some(view);
        let wanted = select_tiles(view);
        let mut kept = BTreeMap::new();
        for key in wanted {
            kept.insert(key, self.selected.remove(&key).unwrap_or_default());
        }
        for (_, tile) in std::mem::replace(&mut self.selected, kept) {
            if let Some(mesh) = tile.mesh.filter(|mesh| !mesh.quads.is_empty()) {
                self.retire(mesh);
            }
        }
        self.retire_covered();
        self.revision += 1;
    }

    /// Marks the selected tiles holding `columns`, or holding them in the ring of neighbours
    /// their skirts reach down to, for a rebuild.
    pub fn mark_changed(&mut self, columns: &[ColumnPos]) {
        for column in columns {
            for (dx, dz) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dz| (dx, dz))) {
                let x = (column.x + dx) * COLUMN_SIDE as i32;
                let z = (column.z + dz) * COLUMN_SIDE as i32;
                for level in 0..LEVEL_COUNT {
                    if let Some(tile) = self.selected.get_mut(&TileKey::containing(level, x, z)) {
                        tile.dirty = true;
                        break;
                    }
                }
            }
        }
    }

    /// Collects finished meshes and starts due builds; returns true when the drawn set changed.
    pub fn update(&mut self, store: &mut SummaryStore, now: Instant) -> bool {
        let before = self.revision;
        for built in self.rx.try_iter() {
            self.in_flight -= 1;
            let Some(tile) = self.selected.get_mut(&built.key) else {
                continue;
            };
            if tile.building == Some(built.token) {
                tile.building = None;
                tile.mesh = Some(Arc::new(built.mesh));
                self.revision += 1;
            }
        }
        if self.revision != before {
            self.retire_covered();
        }
        let mut due = std::mem::take(&mut self.due);
        due.extend(
            self.selected
                .iter()
                .filter(|(_, tile)| {
                    tile.building.is_none()
                        && (tile.mesh.is_none()
                            || tile.dirty
                                && tile.last_build.is_none_or(|last| {
                                    now.saturating_duration_since(last) >= TILE_REBUILD_INTERVAL
                                }))
                })
                .map(|(key, _)| *key)
                .take(MAX_TILE_BUILDS.saturating_sub(self.in_flight)),
        );
        let mut budget = TILE_BUILD_COLUMNS_PER_UPDATE;
        for key in due.drain(..) {
            // The first build always starts, so a tile wider than the budget still builds.
            let columns = key.columns().count();
            if columns > budget && budget < TILE_BUILD_COLUMNS_PER_UPDATE {
                break;
            }
            budget = budget.saturating_sub(columns);
            self.start_build(key, store, now);
        }
        self.due = due;
        // Builds of empty tiles finish here, and may be the last a retired tile waited for.
        if self.revision != before {
            self.retire_covered();
        }
        self.revision != before
    }

    /// Meshes to draw: settled selected tiles plus retired ones still covering a gap.
    #[must_use]
    pub fn drawn(&self) -> Vec<Arc<TileMesh>> {
        let selected = self.selected.iter().filter_map(|(key, tile)| {
            let mesh = tile.mesh.as_ref()?;
            let hidden =
                mesh.quads.is_empty() || self.retired.iter().any(|old| overlaps(old.key, *key));
            (!hidden).then(|| Arc::clone(mesh))
        });
        selected.chain(self.retired.iter().cloned()).collect()
    }

    /// True when a selected tile over `column` is drawn from its current summary.
    #[must_use]
    pub fn ready(&self, column: ColumnPos) -> bool {
        let block = [column.x * COLUMN_SIDE as i32, column.z * COLUMN_SIDE as i32];
        (0..LEVEL_COUNT).any(|level| {
            let key = TileKey::containing(level, block[0], block[1]);
            self.selected.get(&key).is_some_and(|tile| {
                tile.mesh.is_some()
                    && !tile.dirty
                    && tile.building.is_none()
                    // Hidden behind a retained mesh, it is not drawn yet.
                    && !self.retired.iter().any(|old| overlaps(old.key, key))
            })
        })
    }

    /// Changes whenever [`FarTiles::drawn`] may differ.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Forgets every tile, as when the namespace changes.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.retired.clear();
        self.view = None;
        self.revision += 1;
    }

    fn start_build(&mut self, key: TileKey, store: &mut SummaryStore, now: Instant) {
        // The ring of columns around the tile sets how far its edge skirts reach down.
        let [x, z] = key
            .origin()
            .map(|block| block.div_euclid(COLUMN_SIDE as i32));
        let side = key.width_blocks() / COLUMN_SIDE as i32;
        let columns: Vec<(ColumnPos, EncodedSummary)> = (z - 1..=z + side)
            .flat_map(|cz| (x - 1..=x + side).map(move |cx| ColumnPos::new(cx, cz)))
            .filter_map(|pos| Some((pos, store.encoded(pos)?.clone())))
            .collect();
        let tile = self.selected.get_mut(&key).expect("due tiles are selected");
        tile.dirty = false;
        tile.last_build = Some(now);
        if columns.is_empty() {
            tile.mesh = Some(Arc::new(mesh_tile(&TileGrid::build(key, &[]))));
            self.revision += 1;
            return;
        }
        self.next_token += 1;
        let token = self.next_token;
        tile.building = Some(token);
        self.in_flight += 1;
        let tx = self.tx.clone();
        rayon::spawn(move || {
            let decoded: Vec<(ColumnPos, ColumnSummary)> = columns
                .into_iter()
                .filter_map(|(pos, encoded)| {
                    Some((pos, ColumnSummary::decode(&encoded.bytes).ok()?))
                })
                .collect();
            let mesh = mesh_tile(&TileGrid::build(key, &decoded));
            let _ = tx.send(Built { key, token, mesh });
        });
    }

    /// Keeps a replaced mesh drawn until its area is covered, never alongside another retired
    /// mesh over the same area: tiles nest, so the larger of two overlapping ones covers both.
    fn retire(&mut self, mesh: Arc<TileMesh>) {
        self.retired.retain(|old| !contains(mesh.key, old.key));
        if !self.retired.iter().any(|old| contains(old.key, mesh.key)) {
            self.retired.push(mesh);
        }
    }

    /// Drops retired meshes once every selected tile over their area has a mesh.
    fn retire_covered(&mut self) {
        let selected = &self.selected;
        let before = self.retired.len();
        self.retired.retain(|old| {
            selected
                .iter()
                .any(|(key, tile)| tile.mesh.is_none() && overlaps(old.key, *key))
        });
        if self.retired.len() != before {
            self.revision += 1;
        }
    }
}

/// True when `outer`'s area holds all of `inner`'s.
fn contains(outer: TileKey, inner: TileKey) -> bool {
    let ([ox, oz], ow) = (outer.origin(), outer.width_blocks());
    let ([ix, iz], iw) = (inner.origin(), inner.width_blocks());
    ox <= ix && oz <= iz && ix + iw <= ox + ow && iz + iw <= oz + ow
}

fn overlaps(a: TileKey, b: TileKey) -> bool {
    let ([ax, az], aw) = (a.origin(), a.width_blocks());
    let ([bx, bz], bw) = (b.origin(), b.width_blocks());
    ax < bx + bw && bx < ax + aw && az < bz + bw && bz < az + aw
}
