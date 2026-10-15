//! The selected tiles around the camera, rebuilt on workers when their columns change.

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::{
    ColumnPos, ColumnSummary, EncodedSummary, SummaryBudget, SummaryStore, TileCoverage, TileGrid,
    TileKey, TileMesh, TileView,
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
            if let Some(mesh) = tile.mesh.filter(|mesh| {
                !mesh.quads.is_empty() && !self.retired.iter().any(|old| old.key.overlaps(mesh.key))
            }) {
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
        let mut changed = false;
        for column in columns {
            for (dx, dz) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dz| (dx, dz))) {
                let x = (column.x + dx) * COLUMN_SIDE as i32;
                let z = (column.z + dz) * COLUMN_SIDE as i32;
                for level in 0..LEVEL_COUNT {
                    if let Some(tile) = self.selected.get_mut(&TileKey::containing(level, x, z)) {
                        changed |= !tile.dirty;
                        tile.dirty = true;
                        break;
                    }
                }
            }
        }
        if changed {
            self.revision += 1;
        }
    }

    /// Collects finished meshes and starts due builds; returns true when the drawn set changed.
    pub fn update(&mut self, store: &mut SummaryStore, now: Instant) -> bool {
        let before = self.revision;
        while let Ok(built) = self.rx.try_recv() {
            self.in_flight -= 1;
            self.accept_mesh(built);
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
                mesh.quads.is_empty() || self.retired.iter().any(|old| old.key.overlaps(*key));
            (!hidden).then(|| Arc::clone(mesh))
        });
        selected.chain(self.retired.iter().cloned()).collect()
    }

    /// Known empty columns in settled selections, after covering fallback draws have retired.
    pub fn empty_columns(&self) -> impl Iterator<Item = ColumnPos> + '_ {
        self.selected
            .iter()
            .filter_map(|(&key, tile)| {
                let mesh = tile.mesh.as_deref()?;
                (!tile.dirty
                    && tile.building.is_none()
                    && mesh.quads.is_empty()
                    && !self.retired.iter().any(|old| old.key.overlaps(key)))
                .then_some(mesh)
            })
            .flat_map(TileMesh::covered_columns)
    }

    /// True when a selected tile over `column` is CPU-ready from its current summary.
    #[must_use]
    pub fn ready(&self, column: ColumnPos) -> bool {
        self.ready_mesh(column).is_some()
    }

    /// The current CPU-ready selected mesh over `column`, including an empty tile.
    /// Renderers must acknowledge its GPU coverage before withdrawing another drawer.
    #[must_use]
    pub fn ready_mesh(&self, column: ColumnPos) -> Option<&TileMesh> {
        let block = [column.x * COLUMN_SIDE as i32, column.z * COLUMN_SIDE as i32];
        (0..LEVEL_COUNT).find_map(|level| {
            let key = TileKey::containing(level, block[0], block[1]);
            self.selected.get(&key).and_then(|tile| {
                if tile.dirty
                    || tile.building.is_some()
                    || self.retired.iter().any(|old| old.key.overlaps(key))
                {
                    None
                } else {
                    tile.mesh.as_deref().filter(|mesh| mesh.covers(column))
                }
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
            self.accept_mesh(Built {
                key,
                token,
                mesh: TileMesh::empty(key),
            });
            self.retire_covered();
            return true;
        }
        self.in_flight += 1;
        let tx = self.tx.clone();
        rayon::spawn(move || {
            let decoded: Vec<(ColumnPos, ColumnSummary)> = columns
                .into_iter()
                .filter_map(|(pos, encoded)| {
                    let summary = ColumnSummary::decode(&encoded.bytes).ok()?;
                    (summary.content_hash() == encoded.hash).then_some((pos, summary))
                })
                .collect();
            let mesh = mesh_tile(&TileGrid::build(key, &decoded));
            let _ = tx.send(Built { key, token, mesh });
        });
        true
    }

    /// Installs a current build while retaining any visible coverage its evidence would lose.
    fn accept_mesh(&mut self, built: Built) {
        let Some(tile) = self.selected.get_mut(&built.key) else {
            return;
        };
        if tile.building != Some(built.token) {
            return;
        }
        tile.building = None;
        let old = tile.mesh.replace(Arc::new(built.mesh));
        let mesh = tile.mesh.as_ref().expect("installed mesh");
        let fallback = old.filter(|old| {
            !old.quads.is_empty()
                && !matches!(&mesh.coverage, TileCoverage::Complete)
                && !self
                    .retired
                    .iter()
                    .any(|fallback| fallback.key.overlaps(old.key))
                && old.covered_columns().any(|column| !mesh.covers(column))
        });
        if let Some(old) = fallback {
            self.retire(old);
        }
        self.revision += 1;
    }

    /// Keeps a replaced mesh drawn until its area is covered, never alongside another retired
    /// mesh over the same area: tiles nest, so the larger of two overlapping ones covers both.
    fn retire(&mut self, mesh: Arc<TileMesh>) {
        self.retired.retain(|old| !contains(mesh.key, old.key));
        if !self.retired.iter().any(|old| contains(old.key, mesh.key)) {
            self.retired.push(mesh);
        }
    }

    /// Drops a fallback only when settled replacements prove coverage of its known columns.
    fn retire_covered(&mut self) {
        let selected = &self.selected;
        let before = self.retired.len();
        self.retired.retain(|old| {
            selected.iter().any(|(key, tile)| {
                old.key.overlaps(*key)
                    && (tile.dirty
                        || tile.building.is_some()
                        || tile.mesh.as_ref().is_none_or(|mesh| {
                            !matches!(&mesh.coverage, TileCoverage::Complete)
                                && old
                                    .covered_columns()
                                    .filter(|column| {
                                        TileKey::containing(
                                            key.level,
                                            column.x * COLUMN_SIDE as i32,
                                            column.z * COLUMN_SIDE as i32,
                                        ) == *key
                                    })
                                    .any(|column| !mesh.covers(column))
                        }))
            })
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

#[cfg(test)]
mod tests;
