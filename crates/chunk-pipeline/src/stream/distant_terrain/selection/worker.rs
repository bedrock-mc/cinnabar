//! The selection thread: header reads, walks, mesh interest and published updates.

use std::{io, sync::Arc};

use lod::{
    draw::order::distance_blocks,
    geometry::{SectionPos, section_blocks},
    mesh::cache::FreshMesh,
    select::{
        ROOT_LEVEL, View,
        resident::ResidentColumns,
        shown::ShownSet,
        tree::{ExploredTree, Node},
        walk::{Selector, Walk},
    },
};
use lod_store::{
    actor::{ReadOutcome, ReadTarget, SubmitError, ViewChange, ViewEpoch},
    identity::Dimension,
};

use super::{MAX_LOD_UPDATE_BYTES, SelectionInbox, SelectionShared, WalkRequest};
use crate::stream::distant_terrain::snapshots::HiddenColumns;

/// Walks one request may run, reading the headers the previous one lacked in between.
const MAX_WALK_ROUNDS: usize = 8;
/// Record headers one round reads at most, nearest first, so one batch never holds the
/// store's queue long.
const MAX_HEADER_READS_PER_ROUND: usize = 1024;
/// Sections the explored tree knows at most; reads past it are refused.
pub(in crate::stream::distant_terrain) const MAX_LOD_TREE_NODES: usize = 1 << 17;
/// Room a walk's header reads may take at most.
const WALK_READ_ROOM: usize = MAX_WALK_ROUNDS * MAX_HEADER_READS_PER_ROUND;

const _: () = assert!(WALK_READ_ROOM < MAX_LOD_TREE_NODES / 8);

/// Starts the selection thread for `shared`; it stops once selection is turned off.
pub(super) fn spawn(shared: Arc<SelectionShared>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("lod-select".into())
        .spawn(move || {
            crate::stream::workers::lower_beside_pool("lod-select");
            let mut state = SelectState::new();
            while let Some(inbox) = shared.wait() {
                state.step(&shared, inbox);
            }
        })
        .map(drop)
}

/// Why header reads stopped.
enum ReadStop {
    /// Retry at a later walk.
    Later,
    /// The store stopped.
    Stopped,
}

/// The selection thread's state.
struct SelectState {
    dimension: Option<Dimension>,
    tree: ExploredTree,
    selector: Selector,
    shown: ShownSet,
    resident: Arc<ResidentColumns>,
    /// The view of the last walk.
    view: Option<View>,
    /// What read-through showed when the tree was last checked against it.
    seen: Option<ViewEpoch>,
    walk: Walk,
    fresh: Vec<FreshMesh>,
}

impl SelectState {
    /// Nothing known, selected or shown yet.
    fn new() -> Self {
        Self {
            dimension: None,
            tree: ExploredTree::with_limit(MAX_LOD_TREE_NODES),
            selector: Selector::default(),
            shown: ShownSet::default(),
            resident: Arc::default(),
            view: None,
            seen: None,
            walk: Walk::default(),
            fresh: Vec::new(),
        }
    }

    /// Takes in `inbox`, walks when asked, follows arriving meshes and publishes an update
    /// once the frame took the last.
    fn step(&mut self, shared: &SelectionShared, inbox: SelectionInbox) {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.lod_select").entered();
        if !shared.meshes.enabled() {
            shared.disable();
            return;
        }
        if !shared.enabled() {
            return;
        }
        if let Some(request) = &inbox.walk
            && Some(request.dimension) != self.dimension
        {
            self.change_dimension(shared, request.dimension);
        }
        for (dimension, key) in inbox.written {
            if Some(dimension) == self.dimension {
                self.tree.mark_stale(key);
            }
        }
        self.follow_view(shared);
        if let Some(request) = inbox.walk
            && self.walk(shared, request).is_err()
        {
            // The store stopped: meshing cannot go on either, and says so.
            shared.meshes.disable();
            shared.disable();
            return;
        }
        if !shared.enabled() {
            // A newer selection may want sections by now: what is fresh is its.
            return;
        }
        if let Some(dimension) = self.dimension {
            shared
                .meshes
                .take_fresh(|| shared.enabled(), dimension, &mut self.fresh);
        }
        for (key, mesh) in self.fresh.drain(..) {
            self.shown.mesh_arrived(key, mesh);
        }
        self.publish(shared);
        self.send_interest(shared);
        let shown = self.shown.stats();
        let mut output = shared.lock_output();
        output.stats.tree_nodes = self.tree.len();
        output.stats.shown = shown;
    }

    /// Starts over in `dimension`: wanting its sections makes the mesher drop every mesh,
    /// so nothing is released, and the next update removes every drawn section before any
    /// of the new dimension is shown.
    fn change_dimension(&mut self, shared: &SelectionShared, dimension: Dimension) {
        self.dimension = Some(dimension);
        self.tree.clear();
        self.selector.reset();
        self.shown.reset();
        self.seen = Some(shared.meshes.client.view_epoch());
        shared
            .meshes
            .send_interest(|| shared.enabled(), dimension, [], []);
    }

    /// Marks stale what read-through now shows differently: the sections overlapping newly
    /// hidden regions of the current dimension, or everything after a session change.
    fn follow_view(&mut self, shared: &SelectionShared) {
        let client = &shared.meshes.client;
        let Some(seen) = self.seen else {
            self.seen = Some(client.view_epoch());
            return;
        };
        let (view, change) = client.view_changes(seen);
        self.seen = Some(view);
        match change {
            ViewChange::Unchanged => {}
            ViewChange::Hidden(regions) => {
                let columns = HiddenColumns::of(
                    regions
                        .into_iter()
                        .filter(|(dimension, _)| Some(*dimension) == self.dimension)
                        .map(|(_, region)| region),
                );
                self.tree.mark_stale_where(|pos| columns.covers(pos));
                shared.walk_again();
            }
            ViewChange::All => {
                self.tree.mark_stale_where(|_| true);
                shared.walk_again();
            }
        }
    }

    /// Walks for `request`, reading the headers each round lacks, and sets the result as
    /// the shown set's target; fails only when the store stopped.
    fn walk(&mut self, shared: &SelectionShared, request: WalkRequest) -> Result<(), ()> {
        let Some(dimension) = self.dimension else {
            return Ok(());
        };
        if let Some(columns) = request.resident {
            self.resident = Arc::new(ResidentColumns::new(columns));
        }
        if request.reset {
            self.selector.reset();
        }
        let view = request.view;
        let mut rounds = 0;
        loop {
            self.selector
                .walk(&view, &self.tree, &self.resident, &mut self.walk);
            rounds += 1;
            if self.walk.unknown.is_empty() || rounds == MAX_WALK_ROUNDS || !shared.enabled() {
                break;
            }
            match self.read_headers(shared, dimension, &view) {
                Ok(true) => {}
                // The tree is full: what it lacks stays unknown until the trim below.
                Ok(false) => break,
                Err(ReadStop::Later) => {
                    shared.walk_again();
                    break;
                }
                Err(ReadStop::Stopped) => return Err(()),
            }
        }
        if !self.walk.unknown.is_empty() {
            shared.walk_again();
        }
        self.shown.set_target(&self.walk.selected, &self.resident);
        self.view = Some(view);
        if self.tree.len() + WALK_READ_ROOM > MAX_LOD_TREE_NODES {
            // Keep room for the next walk's reads: forget what lies beyond the distance, then
            // the farthest known sections.
            let margin = section_blocks(ROOT_LEVEL) as f64;
            self.tree.trim(
                view.camera,
                view.clamped_distance() + margin,
                MAX_LOD_TREE_NODES - WALK_READ_ROOM,
            );
            shared.lock_output().stats.tree_trims += 1;
        }
        let mut output = shared.lock_output();
        output.stats.walks += 1;
        output.stats.last_walk = self.walk.stats;
        output.stats.selected = self.walk.selected.len();
        Ok(())
    }

    /// Reads the record headers of the nearest unknown sections of the last walk into the
    /// tree in one batch; false when the tree had no room for some. Failed and refused
    /// reads stay unknown.
    fn read_headers(
        &mut self,
        shared: &SelectionShared,
        dimension: Dimension,
        view: &View,
    ) -> Result<bool, ReadStop> {
        let mut unknown: Vec<SectionPos> = self.walk.unknown.clone();
        unknown.sort_by_cached_key(|&pos| distance_blocks(view.camera, pos));
        unknown.truncate(MAX_HEADER_READS_PER_ROUND);
        let targets = unknown
            .iter()
            .map(|pos| ReadTarget::Section(pos.key()))
            .collect();
        let meshes = &shared.meshes;
        let outcomes = match meshes.client.read_batch(dimension, targets, meshes.now()) {
            Ok(outcomes) => outcomes,
            Err(SubmitError::Full(())) => {
                shared.lock_output().stats.read_failures += 1;
                return Err(ReadStop::Later);
            }
            Err(SubmitError::Stopped(())) => return Err(ReadStop::Stopped),
        };
        let mut failed = 0;
        let mut refused = 0;
        for (pos, outcome) in unknown.iter().zip(outcomes) {
            let node = match outcome {
                ReadOutcome::Found(record) => Node::of_record(Some(record.as_slice())),
                ReadOutcome::Missing | ReadOutcome::Corrupt => Node::Missing,
                ReadOutcome::ChunkHash(_) | ReadOutcome::Failed(_) => {
                    failed += 1;
                    continue;
                }
            };
            refused += u64::from(!self.tree.insert(pos.key(), node));
        }
        let mut output = shared.lock_output();
        output.stats.header_reads += unknown.len() as u64;
        output.stats.read_failures += failed;
        output.stats.tree_refused += refused;
        if failed == unknown.len() as u64 {
            return Err(ReadStop::Later);
        }
        Ok(refused == 0)
    }

    /// Publishes the next update once the frame took the last. The update is built
    /// without the output locked, so the frame never waits for it; only this thread fills
    /// the slot, so it stays empty meanwhile.
    fn publish(&mut self, shared: &SelectionShared) {
        let Some(view) = self.view else {
            return;
        };
        if shared.lock_output().update.is_some() {
            return;
        }
        let Some(update) = self.shown.update(view.camera, MAX_LOD_UPDATE_BYTES) else {
            return;
        };
        let mut output = shared.lock_output();
        output.update = Some(update);
        output.stats.updates += 1;
    }

    /// Hands the mesher the sections to want and release since the last step.
    fn send_interest(&mut self, shared: &SelectionShared) {
        let Some(dimension) = self.dimension else {
            return;
        };
        let (mut want, mut release) = (Vec::new(), Vec::new());
        self.shown.take_interest(&mut want, &mut release);
        if !want.is_empty() || !release.is_empty() {
            shared
                .meshes
                .send_interest(|| shared.enabled(), dimension, want, release);
        }
    }
}
