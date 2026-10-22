//! Server pose, Riptide spin and item-use flags adopted as vanilla's flag component does.
//!
//! Vanilla's local systems read the actor flag component, so a server write
//! that sets a pose or spin flag starts it and the client's own rules then
//! continue or end it. Such writes are not start or stop actions, so the next
//! input reports edges relative to the state the server left.

use super::*;

/// Server pose flags, in the order a start prefers when several are set.
pub(super) fn pose_flags(
    flags: client_world::MovementFlagUpdate,
) -> [(Option<bool>, sim::MovementMode); 3] {
    [
        (flags.gliding, sim::MovementMode::Gliding),
        (flags.swimming, sim::MovementMode::Swimming),
        (flags.crawling, sim::MovementMode::Crawling),
    ]
}

/// The pose server flags start from `current`: none while the current pose's
/// own flag stays set, otherwise the first set pose flag.
fn started_pose(
    flags: client_world::MovementFlagUpdate,
    current: sim::MovementMode,
) -> Option<sim::MovementMode> {
    let poses = pose_flags(flags);
    if poses
        .iter()
        .any(|(flag, mode)| *mode == current && *flag == Some(true))
    {
        return None;
    }
    poses
        .into_iter()
        .find(|(flag, _)| *flag == Some(true))
        .map(|(_, mode)| mode)
}

/// Writes the spin flag; a new spin counts its lifetime from zero, as vanilla's
/// post-spin counter restarts after a clear flag. Returns whether it changed.
fn set_spin(state: &mut PlayerState, spinning: bool) -> bool {
    if state.spin_attack_ticks.is_some() == spinning {
        return false;
    }
    state.spin_attack_ticks = spinning.then_some(0);
    true
}

impl LocalPhysicsController {
    /// Ends and starts the live pose from server flags and records the pose
    /// they leave for the next tick's action edges.
    pub(super) fn adopt_live_pose(&mut self, flags: client_world::MovementFlagUpdate) {
        let before = self.modes.mode();
        for (flag, mode) in pose_flags(flags) {
            if flag == Some(false) {
                self.modes.end(mode);
            }
        }
        if let Some(mode) = started_pose(flags, self.modes.mode()) {
            self.modes.start(mode);
        }
        let after = self.modes.mode();
        if after != before {
            self.server_control_flags.get_or_insert_default().mode = Some(after);
        }
    }

    /// Starts the pose a server flag set at retained `tick` for the ticks after
    /// it, when the local pose at that tick lacked it; returns whether history
    /// changed. Replay then applies the pose's own continuation rules.
    pub(super) fn retain_server_pose_start(
        &mut self,
        tick: u64,
        anchor_mode: sim::MovementMode,
        flags: client_world::MovementFlagUpdate,
    ) -> bool {
        let Some(mode) = started_pose(flags, anchor_mode) else {
            return false;
        };
        let Some(frame) = self
            .controller_history
            .iter_mut()
            .find(|frame| frame.tick == tick)
        else {
            return false;
        };
        frame.modes.start(mode);
        self.mark_server_change_after(tick, |input| input.server_mode = Some(mode));
        true
    }

    /// Starts or cancels the live Riptide spin as a server `DamageNearbyMobs`
    /// write does; the simulator's spin rules then end a started spin.
    pub(super) fn adopt_live_spin(&mut self, spinning: Option<bool>) {
        let (Some(spinning), Some(state)) = (spinning, self.state.as_mut()) else {
            return;
        };
        if set_spin(state, spinning) {
            self.server_control_flags.get_or_insert_default().spinning = Some(spinning);
        }
    }

    /// Writes a server spin flag into the state retained after `tick` when it
    /// differs from the local spin there; returns whether history changed.
    pub(super) fn retain_server_spin(&mut self, tick: u64, spinning: Option<bool>) -> bool {
        let Some(spinning) = spinning else {
            return false;
        };
        if !self
            .history
            .state_at_mut(tick)
            .is_some_and(|state| set_spin(state, spinning))
        {
            return false;
        }
        self.mark_server_change_after(tick, |input| input.server_spinning = Some(spinning));
        true
    }

    /// Marks the retained sample after `tick` with a server change its action
    /// edges compare against.
    pub(super) fn mark_server_change_after(
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

    /// Records a server `UsingItem` write for the item-use owner. A retained or
    /// stale stamp (`tick`) applies only when it differs from the local use
    /// during that tick; live writes always apply.
    pub(super) fn note_server_using_item(&mut self, tick: Option<u64>, using: Option<bool>) {
        let Some(using) = using else {
            return;
        };
        let unchanged = tick.is_some_and(|tick| {
            self.controller_history
                .iter()
                .find(|frame| frame.tick == tick)
                .is_some_and(|frame| frame.using_item == using)
        });
        if !unchanged {
            self.server_using_item = Some(using);
        }
    }

    /// Takes the latest server `UsingItem` value the item-use owner must adopt.
    pub fn take_server_using_item(&mut self) -> Option<bool> {
        self.server_using_item.take()
    }
}
