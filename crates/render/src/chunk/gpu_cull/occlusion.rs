//! Which sub-chunks a direct-draw frame may skip on the strength of read-back occlusion bits.
//!
//! A verdict comes from a depth one or more frames old, so it is honoured only while the view
//! matches the frame it was computed on exactly. Any eye translation voids it, because parallax
//! past a near occluder uncovers far terrain by tens of pixels per block; so does any turn,
//! because the near plane swings with the view and can clip an occluder just beyond it. A
//! settled verdict stays true until something changes, so a still camera stops asking for more.

/// Consecutive occluded readbacks under one basis before a slot is skipped.
pub const REQUIRED_OCCLUDED_READBACKS: u8 = 2;

/// What a verdict's depth was rendered from; a verdict holds only while all of it matches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OcclusionBasis {
    pub eye: [f32; 3],
    /// Column-major `world_from_view` rotation; it orients the near plane.
    pub view_rotation: [f32; 9],
    /// Column-major `clip_from_view`; the near plane decides which occluders rasterise.
    pub clip_from_view: [f32; 16],
    /// Origin and size inside the depth target; resizing changes which pixels the depth covers.
    pub viewport: [u32; 4],
    pub depth_size: [u32; 2],
    /// A changed coverage pattern voids verdicts computed from the old depth samples.
    pub depth_samples: u32,
    /// Bumped whenever resident geometry may have uncovered something.
    pub world: u64,
}

/// The frame and view one readback's bits were computed for, over its first `slots` slots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VerdictTag {
    pub frame: u64,
    pub basis: OcclusionBasis,
    pub slots: u32,
}

/// Per-slot runs of occluded verdicts under the latest basis.
#[derive(Debug, Default)]
pub struct OcclusionHistory {
    runs: Vec<u8>,
    /// Frame each slot's current record was written; older verdicts describe another record.
    since: Vec<u64>,
    basis: Option<OcclusionBasis>,
    world: u64,
    /// Verdicts since the basis or any record last changed.
    settled: u8,
    assigned_at: u64,
}

impl OcclusionHistory {
    pub fn world(&self) -> u64 {
        self.world
    }

    /// Geometry left, changed or was hidden: every verdict so far may hide what it uncovered.
    pub fn invalidate_world(&mut self) {
        self.world += 1;
        self.basis = None;
        self.settled = 0;
        self.runs.fill(0);
    }

    /// `slot` received a new record on `frame`.
    pub fn assign(&mut self, slot: u32, frame: u64) {
        let slot = slot as usize;
        if self.runs.len() <= slot {
            self.runs.resize(slot + 1, 0);
            self.since.resize(slot + 1, 0);
        }
        self.runs[slot] = 0;
        self.since[slot] = frame;
        self.assigned_at = frame;
        self.settled = 0;
    }

    /// Folds one readback in; a new basis restarts every run.
    pub fn apply(&mut self, tag: &VerdictTag, occluded: &[u32]) {
        if self.basis != Some(tag.basis) {
            self.basis = Some(tag.basis);
            self.runs.fill(0);
            self.settled = 0;
        }
        if tag.frame >= self.assigned_at {
            self.settled = self.settled.saturating_add(1);
        }
        for (slot, run) in self.runs.iter_mut().enumerate() {
            let hit = slot < tag.slots as usize
                && occluded
                    .get(slot / 32)
                    .is_some_and(|word| word >> (slot % 32) & 1 != 0)
                && self.since[slot] <= tag.frame;
            *run = if hit { run.saturating_add(1) } else { 0 };
        }
    }

    /// Whether `slot` may be skipped when drawing from `basis`.
    pub fn skips(&self, slot: u32, basis: &OcclusionBasis) -> bool {
        self.current(basis)
            && self
                .runs
                .get(slot as usize)
                .is_some_and(|&run| run >= REQUIRED_OCCLUDED_READBACKS)
    }

    /// Whether another verdict for `basis` would repeat the last ones: the same view and
    /// geometry, with enough verdicts since any record changed.
    pub fn settled(&self, basis: &OcclusionBasis) -> bool {
        self.current(basis) && self.settled >= REQUIRED_OCCLUDED_READBACKS
    }

    fn current(&self, basis: &OcclusionBasis) -> bool {
        self.basis == Some(*basis) && basis.world == self.world
    }
}
