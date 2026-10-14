//! The frame's side of selection: the [`WorldStream`] calls and the walk policy.

use std::{sync::Arc, time::Duration};

use lod::select::{
    View, edge_threshold_px,
    policy::{Changes, WalkPolicy},
    shown::DrawUpdate,
};
use lod_store::identity::Dimension;
use world::SUB_CHUNK_SIDE;

use super::{
    DistantTerrainSelectionStats, DistantTerrainView, SelectionShared, WalkRequest, worker,
};
use crate::stream::{WorldStream, distant_terrain::meshes::MeshObserver};

/// The frame's handle of the selection thread; dropping it stops the thread.
pub(in crate::stream) struct LodSelection {
    shared: Arc<SelectionShared>,
    policy: WalkPolicy,
    dimension: Option<Dimension>,
    /// The resident column changes last sent.
    resident_sent: Option<u64>,
}

impl Drop for LodSelection {
    fn drop(&mut self) {
        self.shared.meshes.set_observer(None);
        self.shared.disable();
    }
}

impl WorldStream {
    /// Starts choosing which distant terrain sections to draw and driving the mesher with
    /// them. Fails when meshing is off ([`Self::enable_distant_terrain_meshes`]) or the
    /// thread cannot start.
    pub fn enable_distant_terrain_selection(&mut self) -> std::io::Result<()> {
        self.disable_distant_terrain_selection();
        let Some(meshes) = self.lod_mesh_shared() else {
            return Err(std::io::Error::other("distant terrain meshing is off"));
        };
        // Wants of an earlier selection or of direct calls are not this one's.
        meshes.release_all();
        let shared = Arc::new(SelectionShared::new(Arc::clone(&meshes)));
        worker::spawn(Arc::clone(&shared))?;
        let observer: Arc<dyn MeshObserver> = shared.clone();
        meshes.set_observer(Some(Arc::downgrade(&observer)));
        self.lod_selection = Some(Box::new(LodSelection {
            shared,
            policy: WalkPolicy::default(),
            dimension: None,
            resident_sent: None,
        }));
        Ok(())
    }

    /// Stops selection. A renderer drops what it drew for it; a new selection starts from
    /// nothing drawn and releases what the old one wanted.
    pub fn disable_distant_terrain_selection(&mut self) {
        self.lod_selection = None;
    }

    /// Hands selection this frame's view, with the caller's monotonic `uptime`; walks
    /// only when the view moved or something else changed ([`WalkPolicy`]).
    pub fn set_distant_terrain_view(&mut self, view: DistantTerrainView, uptime: Duration) {
        let dimension = self.current_dimension();
        let Some(range) = self.dimension_range(dimension) else {
            return;
        };
        let resident_changes = self.resident.column_changes();
        let Some(selection) = self.lod_selection.as_deref_mut() else {
            return;
        };
        let dimension = Dimension(dimension);
        if selection.dimension.replace(dimension) != Some(dimension) {
            selection.policy.reset();
            selection.resident_sent = None;
        }
        let side = SUB_CHUNK_SIDE as i32;
        let min_block_y = range.base_sub_chunk_y * side;
        let view = View {
            camera: view.camera,
            focal_px: view.focal_px,
            threshold_px: edge_threshold_px(view.quality),
            distance_blocks: f64::from(view.distance_chunks) * f64::from(side),
            min_block_y,
            max_block_y: min_block_y + range.sub_chunk_count as i32 * side,
        };
        let changes = Changes {
            resident: selection.resident_sent != Some(resident_changes),
            stored: selection.shared.take_walk_again(),
        };
        let Some(due) = selection.policy.poll(&view, uptime, changes) else {
            return;
        };
        let resident = (selection.resident_sent != Some(resident_changes)).then(|| {
            self.resident
                .columns()
                .map(|column| [column.x, column.z])
                .collect()
        });
        selection.resident_sent = Some(resident_changes);
        selection.shared.send(|inbox| {
            inbox.ask_walk(WalkRequest {
                view,
                dimension,
                reset: due.reset,
                resident,
            });
        });
    }

    /// The next step of what to draw, once selection published one; selection prepares
    /// the following step only after this took the last.
    pub fn take_distant_terrain_update(&mut self) -> Option<DrawUpdate> {
        let selection = self.lod_selection.as_ref()?;
        let update = selection.shared.lock_output().update.take()?;
        selection.shared.send(|_| {});
        Some(update)
    }

    /// Counts resident chunk columns added and removed so far; equal counts mean the same
    /// columns, so a renderer rebuilds its resident column mask only when this moves on.
    #[must_use]
    pub fn resident_column_changes(&self) -> u64 {
        self.resident.column_changes()
    }

    /// The `x` and `z` of every resident chunk column, in no particular order.
    pub fn resident_chunk_columns(&self) -> impl Iterator<Item = [i32; 2]> + '_ {
        self.resident.columns().map(|column| [column.x, column.z])
    }

    /// Selection counters; `None` while selection is off.
    #[must_use]
    pub fn distant_terrain_selection_stats(&self) -> Option<DistantTerrainSelectionStats> {
        let selection = self.lod_selection.as_ref()?;
        Some(selection.shared.lock_output().stats)
    }
}
