//! Server pose, Riptide spin and item-use flags adopted as vanilla's flag component does.
//!
//! Vanilla's local systems read the actor flag component, so a server write
//! that sets a pose or spin flag starts it and the client's own rules then
//! continue or end it. Such writes are not start or stop actions, so the next
//! input reports edges relative to the state the server left. A write stamped
//! at a retained tick changes only flags that differ from the local state
//! entering the next tick; it applies to the live state at once and stays in
//! that frame for every later replay through it.

use super::*;

/// Pose modes in the order a start prefers when one write sets several.
const POSES: [sim::MovementMode; 3] = [
    sim::MovementMode::Gliding,
    sim::MovementMode::Swimming,
    sim::MovementMode::Crawling,
];

/// One write's glide, swim and crawl flags, in [`POSES`] order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PoseWrite([Option<bool>; 3]);

impl PoseWrite {
    pub(super) fn from_flags(flags: &client_world::MovementFlagUpdate) -> Self {
        Self([flags.gliding, flags.swimming, flags.crawling])
    }

    /// The bits of this write that differ from the local pose `mode`.
    fn differing_from(self, mode: sim::MovementMode) -> Self {
        Self(std::array::from_fn(|index| {
            self.0[index].filter(|set| *set != (POSES[index] == mode))
        }))
    }

    /// This write's bits over an earlier write's.
    fn over(self, earlier: Self) -> Self {
        Self(std::array::from_fn(|index| {
            self.0[index].or(earlier.0[index])
        }))
    }

    /// The locomotion mode after this write: a cleared pose ends, and a set
    /// pose starts unless a pose that the write did not end continues.
    /// Cinnabar keeps one pose, so a write setting several starts the first,
    /// and flight, which vanilla keeps beside pose flags, stays.
    fn resulting(self, mode: sim::MovementMode) -> sim::MovementMode {
        if mode == sim::MovementMode::Flying {
            return mode;
        }
        let ended = POSES
            .into_iter()
            .zip(self.0)
            .any(|(pose, flag)| pose == mode && flag == Some(false));
        let current = if ended {
            sim::MovementMode::Walking
        } else {
            mode
        };
        if POSES.contains(&current) {
            return current;
        }
        POSES
            .into_iter()
            .zip(self.0)
            .find(|(_, flag)| *flag == Some(true))
            .map_or(current, |(pose, _)| pose)
    }

    /// Ends or starts `modes`' pose as [`Self::resulting`] describes.
    fn apply(self, modes: &mut ModeTracker) {
        let current = modes.mode();
        let next = self.resulting(current);
        if next == current {
            return;
        }
        if POSES.contains(&next) {
            modes.start(next);
        } else {
            modes.end(current);
        }
    }
}

/// Writes the spin flag during replay; a new spin counts its lifetime from
/// zero, as vanilla's post-spin counter restarts after a clear flag.
fn set_spin(state: &mut PlayerState, spinning: bool) {
    if state.spin_attack_ticks.is_some() != spinning {
        state.spin_attack_ticks = spinning.then_some(0);
    }
}

/// Server writes that differ from the local state entering one retained tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlagOverlay {
    /// The tick the writes apply before: the one after their stamp.
    tick: u64,
    pose: PoseWrite,
    spinning: Option<bool>,
    using_item: Option<bool>,
    /// Live writes made before this tick ran, part of the tick's own capture.
    captured: CapturedWrites,
}

/// Live server writes captured with one tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CapturedWrites {
    pose: PoseWrite,
    spinning: Option<bool>,
    using_item: Option<bool>,
}

/// What one stamped update compares against, fixed before it records any
/// field: the following frame's capture, as vanilla's flag correction has no
/// pending type that would compare with the live actor instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WriteBases {
    captured: CapturedWrites,
}

/// The pose and spin server writes changed before one tick, which that tick's
/// start and stop actions compare against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ServerBaseline {
    pub mode: Option<sim::MovementMode>,
    pub spinning: Option<bool>,
}

/// Retained server flag writes and state awaiting adoption.
#[derive(Debug, Clone, Default)]
pub(super) struct ServerFlagAuthority {
    overlays: VecDeque<FlagOverlay>,
    /// Server `UsingItem` value the item-use owner has yet to adopt.
    pending_using_item: Option<bool>,
    /// A spin counter a live clear removed, with the tick it was cleared on.
    /// Vanilla's counter outlives its flag until a tick processes the clear.
    cleared_spin: Option<(u64, u8)>,
}

impl ServerFlagAuthority {
    /// Drops retained writes and the cleared spin counter when their frames
    /// and life are discarded.
    pub(super) fn clear_overlays(&mut self) {
        self.overlays.clear();
        self.cleared_spin = None;
    }

    fn overlay(&self, tick: u64) -> Option<&FlagOverlay> {
        self.overlays.iter().find(|overlay| overlay.tick == tick)
    }

    /// Forgets the live writes captured with ticks a replay from `anchor`
    /// through `last` rebuilt. Vanilla keeps the capture of the frame the
    /// replay starts from (the one after `anchor`) and refreshes later ones.
    fn refresh_captures(&mut self, anchor: u64, last: u64) {
        let first = anchor.saturating_add(1);
        for overlay in self
            .overlays
            .iter_mut()
            .filter(|overlay| overlay.tick > first && overlay.tick <= last)
        {
            overlay.captured = CapturedWrites::default();
        }
    }

    /// Returns the writes before `tick`, pruning ones older than `oldest` and
    /// bounding the count to the retained window.
    fn overlay_mut(&mut self, tick: u64, oldest: u64, capacity: usize) -> &mut FlagOverlay {
        self.overlays.retain(|overlay| overlay.tick > oldest);
        if let Some(index) = self
            .overlays
            .iter()
            .position(|overlay| overlay.tick == tick)
        {
            return &mut self.overlays[index];
        }
        if self.overlays.len() >= capacity {
            self.overlays.pop_front();
        }
        let index = self
            .overlays
            .iter()
            .position(|overlay| overlay.tick > tick)
            .unwrap_or(self.overlays.len());
        self.overlays.insert(
            index,
            FlagOverlay {
                tick,
                pose: PoseWrite::default(),
                spinning: None,
                using_item: None,
                captured: CapturedWrites::default(),
            },
        );
        &mut self.overlays[index]
    }

    /// Applies the retained writes for the tick after `state` before it is
    /// selected and simulated; returns the pose and spin they changed, the
    /// action baseline for that tick's input.
    pub(super) fn apply_before(
        &self,
        modes: &mut ModeTracker,
        state: &mut PlayerState,
    ) -> ServerBaseline {
        let (mode, spinning) = (modes.mode(), state.spin_attack_ticks.is_some());
        if let Some(overlay) = state
            .tick
            .checked_add(1)
            .and_then(|tick| self.overlay(tick))
        {
            overlay.pose.apply(modes);
            if let Some(spinning) = overlay.spinning {
                set_spin(state, spinning);
            }
        }
        ServerBaseline {
            mode: (modes.mode() != mode).then_some(modes.mode()),
            spinning: (state.spin_attack_ticks.is_some() != spinning).then_some(!spinning),
        }
    }
}

impl LocalPhysicsController {
    /// Ends and starts the live pose from a server write, records the pose it
    /// leaves for the next tick's action edges, and retains it for the next
    /// tick so a replay through that tick keeps it.
    pub(super) fn adopt_live_pose(&mut self, write: PoseWrite) {
        let before = self.modes.mode();
        let differing = write.differing_from(before);
        if differing.resulting(before) == before {
            return;
        }
        differing.apply(&mut self.modes);
        self.server_control_flags.get_or_insert_default().mode = Some(self.modes.mode());
        if let Some(overlay) = self.live_overlay() {
            overlay.pose = differing.over(overlay.pose);
            overlay.captured.pose = differing.over(overlay.captured.pose);
        }
    }

    /// Writes before the tick after the live state, created on demand.
    fn live_overlay(&mut self) -> Option<&mut FlagOverlay> {
        let tick = self.state.as_ref()?.tick;
        self.retained_overlay(tick)
    }

    /// Re-applies server writes made after the last completed tick to the
    /// state a replay rebuilt, and resets the action baseline to them.
    pub(super) fn reapply_live_server_flags(&mut self, anchor: u64) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        self.server_flags.refresh_captures(anchor, state.tick);
        let ServerBaseline { mode, spinning } =
            self.server_flags.apply_before(&mut self.modes, state);
        let pending = self.server_control_flags.get_or_insert_default();
        pending.mode = mode;
        pending.spinning = spinning;
        // Baselines an earlier tickless frame already took are rebuilt too.
        self.input_edges.replace_server_pose(mode, spinning);
    }

    /// Fixes the base one update stamped at retained `tick` compares against:
    /// the following frame's capture, which includes live writes made before
    /// that frame ran.
    pub(super) fn write_bases(&self, tick: u64) -> WriteBases {
        WriteBases {
            captured: tick
                .checked_add(1)
                .and_then(|after| self.server_flags.overlay(after))
                .map(|overlay| overlay.captured)
                .unwrap_or_default(),
        }
    }

    /// Writes before the tick after retained `tick`, created on demand.
    fn retained_overlay(&mut self, tick: u64) -> Option<&mut FlagOverlay> {
        let after = tick.checked_add(1)?;
        let oldest = self.history.oldest_tick()?;
        Some(
            self.server_flags
                .overlay_mut(after, oldest, self.history_capacity),
        )
    }

    /// Retains the pose bits of a write stamped at retained `tick` that change
    /// the pose entering the next tick, and applies them live; returns whether
    /// history changed. Replay then applies the pose's own rules.
    pub(super) fn retain_server_pose(
        &mut self,
        tick: u64,
        anchor_mode: sim::MovementMode,
        bases: WriteBases,
        write: PoseWrite,
    ) -> bool {
        let Some(after) = tick.checked_add(1) else {
            return false;
        };
        let earlier = self
            .server_flags
            .overlay(after)
            .map(|overlay| overlay.pose)
            .unwrap_or_default();
        let entering = bases.captured.pose.resulting(anchor_mode);
        let differing = write.differing_from(entering);
        if differing.resulting(entering) == entering {
            return false;
        }
        let combined = differing.over(earlier);
        let Some(overlay) = self.retained_overlay(tick) else {
            return false;
        };
        overlay.pose = combined;
        let left = combined.resulting(anchor_mode);
        self.mark_server_change_after(tick, |input| {
            input.server_mode = (left != anchor_mode).then_some(left);
        });
        // Vanilla applies the correction to the live actor too; replay replaces it.
        differing.apply(&mut self.modes);
        self.server_control_flags.get_or_insert_default().mode = Some(self.modes.mode());
        true
    }

    /// Writes the live spin flag; a set restores a counter that a clear on the
    /// same tick removed. Returns whether the spin changed.
    fn write_live_spin(&mut self, spinning: bool) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };
        if state.spin_attack_ticks.is_some() == spinning {
            return false;
        }
        if spinning {
            let restored = self
                .server_flags
                .cleared_spin
                .filter(|(tick, _)| *tick == state.tick)
                .map_or(0, |(_, ticks)| ticks);
            state.spin_attack_ticks = Some(restored);
        } else {
            self.server_flags.cleared_spin =
                state.spin_attack_ticks.map(|ticks| (state.tick, ticks));
            state.spin_attack_ticks = None;
        }
        true
    }

    /// Starts or cancels the live Riptide spin as a server `DamageNearbyMobs`
    /// write does; the simulator's spin rules then end a started spin.
    pub(super) fn adopt_live_spin(&mut self, spinning: Option<bool>) {
        if let Some(spinning) = spinning
            && self.write_live_spin(spinning)
        {
            self.server_control_flags.get_or_insert_default().spinning = Some(spinning);
            if let Some(overlay) = self.live_overlay() {
                overlay.spinning = Some(spinning);
                overlay.captured.spinning = Some(spinning);
            }
        }
    }

    /// Retains a spin write stamped at retained `tick` when it differs from the
    /// spin entering the next tick, and applies it live; returns whether
    /// history changed.
    pub(super) fn retain_server_spin(
        &mut self,
        tick: u64,
        bases: WriteBases,
        spinning: Option<bool>,
    ) -> bool {
        let Some(spinning) = spinning else {
            return false;
        };
        let local = self
            .history
            .state_at(tick)
            .is_some_and(|state| state.spin_attack_ticks.is_some());
        let base = bases.captured.spinning.unwrap_or(local);
        if spinning == base {
            return false;
        }
        let Some(overlay) = self.retained_overlay(tick) else {
            return false;
        };
        // The effective write stays even when it matches the capture, so a
        // replay through this frame reapplies it whatever arrives there.
        overlay.spinning = Some(spinning);
        self.mark_server_change_after(tick, |input| input.server_spinning = Some(spinning));
        if self.write_live_spin(spinning) {
            self.server_control_flags.get_or_insert_default().spinning = Some(spinning);
        }
        true
    }

    /// Marks the retained sample after `tick` with a server change its action
    /// edges compare against.
    fn mark_server_change_after(
        &mut self,
        tick: u64,
        edit: impl FnOnce(&mut super::super::TickInput),
    ) {
        if let Some(sample) = self
            .sample_history
            .iter_mut()
            .find(|sample| Some(sample.tick) == tick.checked_add(1))
        {
            edit(&mut sample.input);
        }
    }

    /// Records a `UsingItem` write stamped at retained `tick` for the item-use
    /// owner when it differs from the use entering the next tick.
    pub(super) fn retain_server_using_item(
        &mut self,
        tick: u64,
        bases: WriteBases,
        using: Option<bool>,
    ) -> bool {
        let Some(using) = using else {
            return false;
        };
        let local = self
            .controller_history
            .iter()
            .find(|frame| frame.tick == tick)
            .map(|frame| frame.using_item);
        let base = bases.captured.using_item.or(local);
        if base == Some(using) {
            return false;
        }
        if let Some(overlay) = self.retained_overlay(tick) {
            overlay.using_item = Some(using);
        }
        self.server_flags.pending_using_item = Some(using);
        // Vanilla rewinds for this flag too, which settles the correction.
        true
    }

    /// Records a live `UsingItem` write for the item-use owner.
    pub(super) fn note_server_using_item(&mut self, using: Option<bool>) {
        let Some(using) = using else {
            return;
        };
        if let Some(overlay) = self.live_overlay() {
            overlay.using_item = Some(using);
            overlay.captured.using_item = Some(using);
        }
        self.server_flags.pending_using_item = Some(using);
    }

    /// Retires server writes awaiting the next tick when respawn discards the
    /// old life: their action baselines and the pending item-use value.
    pub fn discard_server_flag_baselines(&mut self) {
        if let Some(pending) = self.server_control_flags.as_mut() {
            pending.mode = None;
            pending.spinning = None;
        }
        self.input_edges.replace_server_pose(None, None);
        self.server_flags.pending_using_item = None;
    }

    /// Takes the latest server `UsingItem` value the item-use owner must adopt.
    pub fn take_server_using_item(&mut self) -> Option<bool> {
        self.server_flags.pending_using_item.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replay rebuilds each tick's action baseline from the pose the retained
    /// write actually leaves, not the pose it left when it arrived.
    #[test]
    fn a_retained_write_reports_only_the_pose_it_changes() {
        let mut authority = ServerFlagAuthority::default();
        authority.overlay_mut(103, 100, 8).pose = PoseWrite([Some(false), None, None]);
        let mut state = PlayerState::new(Vec3::ZERO);
        state.tick = 102;
        let mut gliding = ModeTracker::default();
        gliding.start(sim::MovementMode::Gliding);
        assert_eq!(
            authority.apply_before(&mut gliding, &mut state),
            ServerBaseline {
                mode: Some(sim::MovementMode::Walking),
                spinning: None,
            }
        );
        let mut crawling = ModeTracker::default();
        crawling.start(sim::MovementMode::Crawling);
        assert_eq!(
            authority.apply_before(&mut crawling, &mut state),
            ServerBaseline::default()
        );
    }
}
