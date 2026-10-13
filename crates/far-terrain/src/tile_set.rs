//! The selected tiles around the camera, rebuilt on workers when their columns change.

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::{
    ColumnPos, ColumnSummary, EncodedSummary, SummaryStore, TileGrid, TileKey, TileMesh, TileView,
    config::{LEVEL_COUNT, MAX_TILE_BUILDS, TILE_REBUILD_INTERVAL},
    mesh_tile, select_tiles,
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
                self.retired.push(mesh);
            }
        }
        self.retire_covered();
        self.revision += 1;
    }

    /// Marks the selected tiles holding `columns` for a rebuild.
    pub fn mark_changed(&mut self, columns: &[ColumnPos]) {
        for column in columns {
            let (x, z) = (column.x * 16, column.z * 16);
            for level in 0..LEVEL_COUNT {
                if let Some(tile) = self.selected.get_mut(&TileKey::containing(level, x, z)) {
                    tile.dirty = true;
                    break;
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
        for key in due.drain(..) {
            self.start_build(key, store, now);
        }
        self.due = due;
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
        let columns: Vec<(ColumnPos, EncodedSummary)> = key
            .columns()
            .filter_map(|pos| Some((pos, store.touch_encoded(pos)?.clone())))
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

fn overlaps(a: TileKey, b: TileKey) -> bool {
    let ([ax, az], aw) = (a.origin(), a.width_blocks());
    let ([bx, bz], bw) = (b.origin(), b.width_blocks());
    ax < bx + bw && bx < ax + aw && az < bz + bw && bz < az + aw
}
