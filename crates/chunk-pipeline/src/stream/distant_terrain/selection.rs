//! Which distant terrain sections are drawn: an opt-in Cinnabar extension, not vanilla
//! behaviour.
//!
//! The frame hands the camera, projection scale, quality and distance to
//! [`crate::WorldStream::set_distant_terrain_view`] every frame; a
//! [`lod::select::policy::WalkPolicy`] decides on the frame whether that calls for a walk
//! (movement, never rotation, at most 30 a second), and only then are the resident chunk
//! columns gathered and sent to a `lod-select` thread. That thread reads the record headers
//! of the sections a walk needs, walks the explored tree ([`lod::select`]), wants and
//! releases meshes from the mesher thread ([`super::meshes`]), takes the meshes it finishes
//! and publishes one [`DrawUpdate`] at a time: the next only once the frame took the
//! previous one ([`crate::WorldStream::take_distant_terrain_update`]), so a renderer applies
//! them in order and at its own pace.

pub(in crate::stream) mod frame;
#[cfg(test)]
mod tests;
pub(super) mod worker;

use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, PoisonError,
    atomic::{AtomicBool, Ordering},
};

use lod::{
    geometry::SectionKey,
    select::{
        View,
        shown::{DrawUpdate, ShownStats},
        walk::WalkStats,
    },
};
use lod_store::identity::Dimension;

use super::meshes::{MeshObserver, MeshShared};

/// Bytes of new meshes one update carries at most, beyond its first unit; a renderer
/// uploading a few megabytes a frame applies one in a few frames.
pub const MAX_LOD_UPDATE_BYTES: usize = 8 << 20;

/// What the frame knows of the view each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistantTerrainView {
    /// Camera position in blocks.
    pub camera: [f64; 3],
    /// Pixels per unit of `tan(angle)`: [`lod::select::focal_px`] of the viewport height
    /// and the current vertical field of view.
    pub focal_px: f64,
    pub quality: render_api::DistantTerrainQuality,
    /// Drawing distance in chunks, at most [`lod::select::MAX_DISTANCE_CHUNKS`].
    pub distance_chunks: u32,
}

/// Selection counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistantTerrainSelectionStats {
    pub walks: u64,
    /// Counters of the last walk.
    pub last_walk: WalkStats,
    /// Sections the last walk selected.
    pub selected: usize,
    /// Record headers read for walks.
    pub header_reads: u64,
    /// Header reads the store failed or refused; the next walk reads them again.
    pub read_failures: u64,
    /// Sections whose existence and children the selection knows, at most the tree's limit.
    pub tree_nodes: usize,
    /// Header reads the tree had no room for; their sections stay unknown.
    pub tree_refused: u64,
    /// Times the tree forgot its farthest sections to make room for a walk's reads.
    pub tree_trims: u64,
    pub shown: ShownStats,
    /// Updates published.
    pub updates: u64,
    /// True once selection stopped because meshing or the store stopped.
    pub stopped: bool,
}

/// A walk the frame asks for.
#[derive(Debug, Clone)]
pub(super) struct WalkRequest {
    pub(super) view: View,
    pub(super) dimension: Dimension,
    /// Forget which sections were refined first.
    pub(super) reset: bool,
    /// The `x` and `z` of every resident chunk column, when they changed since the last
    /// request; the selection thread indexes them.
    pub(super) resident: Option<Vec<[i32; 2]>>,
}

/// What the frame, the mesher and the writers hand the selection thread.
#[derive(Default)]
pub(super) struct SelectionInbox {
    pub(super) walk: Option<WalkRequest>,
    pub(super) written: Vec<(Dimension, SectionKey)>,
    /// Inputs changed since the thread last looked.
    pending: bool,
}

impl SelectionInbox {
    /// Asks for `request`, replacing a walk the thread has not taken yet but keeping what
    /// that one carried and `request` lacks: its resident columns and its reset.
    pub(super) fn ask_walk(&mut self, mut request: WalkRequest) {
        if let Some(replaced) = self.walk.take() {
            request.reset |= replaced.reset;
            if request.resident.is_none() {
                request.resident = replaced.resident;
            }
        }
        self.walk = Some(request);
    }
}

/// What the selection thread hands back.
#[derive(Default)]
pub(super) struct SelectionOutput {
    pub(super) update: Option<DrawUpdate>,
    pub(super) stats: DistantTerrainSelectionStats,
}

/// State shared by the frame, the mesher, the writers and the selection thread.
pub(super) struct SelectionShared {
    pub(super) meshes: Arc<MeshShared>,
    enabled: AtomicBool,
    inbox: Mutex<SelectionInbox>,
    ready: Condvar,
    output: Mutex<SelectionOutput>,
    /// Sections were stored, or a walk could not read what it needed, since the frame
    /// last looked: a walk is due once [`lod::select::policy::DATA_WALK_INTERVAL`] passed.
    stored: AtomicBool,
}

impl SelectionShared {
    fn lock_inbox(&self) -> MutexGuard<'_, SelectionInbox> {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn lock_output(&self) -> MutexGuard<'_, SelectionOutput> {
        self.output.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the inbox and wakes the thread; does nothing once selection stopped.
    pub(super) fn send(&self, change: impl FnOnce(&mut SelectionInbox)) {
        let mut inbox = self.lock_inbox();
        if !self.enabled() {
            return;
        }
        change(&mut inbox);
        inbox.pending = true;
        drop(inbox);
        self.ready.notify_all();
    }

    /// Stops the thread.
    pub(super) fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
        self.lock_output().stats.stopped = true;
        self.ready.notify_all();
    }

    /// False once selection stopped.
    pub(super) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// A selection thread's state with nothing asked yet, driving `meshes`.
    fn new(meshes: Arc<MeshShared>) -> Self {
        Self {
            meshes,
            enabled: AtomicBool::new(true),
            inbox: Mutex::new(SelectionInbox::default()),
            ready: Condvar::new(),
            output: Mutex::new(SelectionOutput::default()),
            stored: AtomicBool::new(false),
        }
    }

    /// Asks the frame for another walk once the data interval passed.
    pub(super) fn walk_again(&self) {
        self.stored.store(true, Ordering::Release);
    }

    /// True once since the last call when a walk was asked for by stored data.
    fn take_walk_again(&self) -> bool {
        self.stored.swap(false, Ordering::AcqRel)
    }

    /// Blocks the selection thread until its inputs changed; `None` once it stopped.
    pub(super) fn wait(&self) -> Option<SelectionInbox> {
        let mut inbox = self.lock_inbox();
        loop {
            if !self.enabled() {
                return None;
            }
            if inbox.pending {
                inbox.pending = false;
                return Some(std::mem::take(&mut *inbox));
            }
            inbox = self
                .ready
                .wait(inbox)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl MeshObserver for SelectionShared {
    fn meshes_ready(&self) {
        self.send(|_| {});
    }

    fn written(&self, dimension: Dimension, keys: &[SectionKey]) {
        self.send(|inbox| {
            inbox
                .written
                .extend(keys.iter().map(|&key| (dimension, key)));
        });
        self.walk_again();
    }
}
