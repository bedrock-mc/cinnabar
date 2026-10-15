//! The selected tiles around the camera, rebuilt on workers when their columns change.

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::{
    ColumnPos, ColumnSummary, EncodedSummary, SummaryBudget, SummaryStore, TileGrid, TileKey,
    TileMesh, TileView,
    config::{
        LEVEL_COUNT, MAX_TILE_BUILDS, TILE_BUILD_BYTES_PER_UPDATE, TILE_BUILD_COLUMNS_PER_UPDATE,
        TILE_REBUILD_INTERVAL,
    },
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

/// A tile snapshot gathered across updates, including the columns around its skirts.
struct Gathering {
    key: TileKey,
    token: u64,
    next: usize,
    columns: Vec<(ColumnPos, EncodedSummary)>,
}

impl Gathering {
    /// Visits each halo position once and shares any stored encoding within the budget.
    fn advance(&mut self, store: &SummaryStore, budget: &mut SummaryBudget) -> bool {
        let [x, z] = self
            .key
            .origin()
            .map(|block| block.div_euclid(COLUMN_SIDE as i32) - 1);
        let side = self.key.width_blocks() as usize / COLUMN_SIDE + 2;
        while self.next < side * side {
            if !budget.has_columns() {
                return false;
            }
            let pos = ColumnPos::new(x + (self.next % side) as i32, z + (self.next / side) as i32);
            let encoded = store.encoded(pos);
            if !budget.take(encoded.map_or(0, |summary| summary.bytes.len())) {
                // The lookup still counts when retaining its payload must wait.
                budget.take(0);
                return false;
            }
            if let Some(encoded) = encoded {
                self.columns.push((pos, encoded.clone()));
            }
            self.next += 1;
        }
        true
    }
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
    gathering: Option<Gathering>,
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
            gathering: None,
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
        if self
            .gathering
            .as_ref()
            .is_some_and(|gather| !self.selected.contains_key(&gather.key))
        {
            self.gathering = None;
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
        let mut budget =
            SummaryBudget::new(TILE_BUILD_COLUMNS_PER_UPDATE, TILE_BUILD_BYTES_PER_UPDATE);
        if !self.gather(store, &mut budget) {
            return self.revision != before;
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
        for key in due.drain(..) {
            self.start_gather(key, now);
            if !self.gather(store, &mut budget) {
                break;
            }
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
        self.gathering = None;
        self.selected.clear();
        self.retired.clear();
        self.view = None;
        self.revision += 1;
    }

    /// Reserves a build token before gathering so changes keep it dirty until a later build.
    fn start_gather(&mut self, key: TileKey, now: Instant) {
        self.next_token += 1;
        let token = self.next_token;
        let tile = self.selected.get_mut(&key).expect("due tiles are selected");
        tile.dirty = false;
        tile.last_build = Some(now);
        tile.building = Some(token);
        self.gathering = Some(Gathering {
            key,
            token,
            next: 0,
            columns: Vec::new(),
        });
    }

    /// Resumes the current snapshot and submits it only once all halo lookups have finished.
    fn gather(&mut self, store: &SummaryStore, budget: &mut SummaryBudget) -> bool {
        let Some(gather) = &mut self.gathering else {
            return true;
        };
        if !gather.advance(store, budget) {
            return false;
        }
        let Gathering {
            key,
            token,
            columns,
            ..
        } = self.gathering.take().expect("gathering exists");
        if columns.is_empty() {
            let tile = self
                .selected
                .get_mut(&key)
                .expect("gathered tiles are selected");
            tile.building = None;
            tile.mesh = Some(Arc::new(TileMesh::empty(key)));
            self.revision += 1;
            self.retire_covered();
            return true;
        }
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
        true
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

#[cfg(test)]
mod tests;
