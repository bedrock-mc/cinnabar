use protocol::ActorInterpolation;

use super::{ACTOR_INTERPOLATION_TICKS, ActorPose, ActorSnapshot, PoseYOrigin};

/// Distance below a block surface that grounded interpolation lifts onto it.
const GROUND_SURFACE_TOLERANCE: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq)]
struct QueuedInterpolation {
    pose: ActorPose,
    ticks: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct MovementInterpolation {
    pub(super) current_y_origin: PoseYOrigin,
    last_received: Option<ActorPose>,
    must_complete: bool,
    queued: Option<QueuedInterpolation>,
    rotation_ticks: u32,
    head_ticks: u32,
}

/// Movement durations use the positive range of the native signed counter.
pub(super) fn duration(interpolation: ActorInterpolation) -> Option<u32> {
    let ticks = interpolation
        .ticks
        .max(u64::from(ACTOR_INTERPOLATION_TICKS)) as u32;
    (ticks != 0 && ticks <= i32::MAX as u32).then_some(ticks)
}

impl ActorSnapshot {
    /// Applies a native height displacement and keeps its collision-feet projection coherent.
    pub(super) fn translate_native_height(&mut self, delta: f32) {
        let mut pose = self.current_pose();
        let native_y = self.current_native_position()[1] + delta;
        pose.y_origin.native_y = Some(native_y);
        pose.position[1] = native_y - self.network_position_offset();
        self.set_current_pose(pose);
    }

    /// Returns the decoded target without a lossy feet-to-network round trip.
    pub fn last_received_native_position(&self) -> [f32; 3] {
        self.last_received_pose()
            .native_position(self.network_position_offset())
    }

    /// Returns the current pose without losing its exact network height.
    pub(super) fn current_native_position(&self) -> [f32; 3] {
        self.current_pose()
            .native_position(self.network_position_offset())
    }

    /// Returns the latest decoded movement target, including a queued target.
    pub fn last_received_pose(&self) -> ActorPose {
        self.status
            .movement_interpolation
            .last_received
            .unwrap_or(self.received_pose)
    }

    pub(super) fn start_movement_interpolation(
        &mut self,
        mut pose: ActorPose,
        ticks: u32,
        force_completion: bool,
        teleported: bool,
    ) {
        pose.pitch = signed_pitch(pose.pitch);
        let state = &mut self.status.movement_interpolation;
        state.last_received = Some(pose);
        if !teleported
            && !force_completion
            && state.must_complete
            && self.interpolation_ticks_remaining != 0
        {
            state.queued = Some(QueuedInterpolation { pose, ticks });
            self.received_pose.pitch = pose.pitch;
            self.received_pose.yaw = pose.yaw;
            self.received_pose.head_yaw = pose.head_yaw;
        } else {
            self.received_pose = pose;
            self.interpolation_ticks_remaining = ticks;
            state.must_complete = force_completion;
            state.queued = None;
        }
        state.rotation_ticks = ACTOR_INTERPOLATION_TICKS;
        state.head_ticks = ACTOR_INTERPOLATION_TICKS;
        if teleported {
            self.previous_pose = pose;
            self.set_current_pose(pose);
            self.interpolation_ticks_remaining = 0;
            self.status.movement_interpolation.rotation_ticks = 0;
            self.status.movement_interpolation.head_ticks = 0;
            self.status.movement_interpolation.must_complete = false;
        }
    }

    /// Rebases retained poses and queued targets without moving collision-feet inputs.
    pub(super) fn shift_poses_y(&mut self, network_delta: f32, spawn_delta: f32) {
        let network_offset = self.network_position_offset();
        self.update_poses(|pose| pose.shift_y(network_delta, spawn_delta, network_offset));
    }

    /// Keeps existing feet anchors when metadata changes a non-falling actor's offset.
    pub(super) fn clear_native_heights(&mut self) {
        self.update_poses(|pose| pose.y_origin.native_y = None);
    }

    /// Visits every retained pose so coordinate changes also reach buffered targets.
    fn update_poses(&mut self, mut update: impl FnMut(&mut ActorPose)) {
        let mut current = self.current_pose();
        for pose in [
            &mut current,
            &mut self.previous_pose,
            &mut self.received_pose,
        ] {
            update(pose);
        }
        let state = &mut self.status.movement_interpolation;
        for pose in state
            .last_received
            .iter_mut()
            .chain(state.queued.iter_mut().map(|queued| &mut queued.pose))
        {
            update(pose);
        }
        self.set_current_pose(current);
    }

    /// The pose state an interpolation tick reads and writes.
    pub(super) fn motion_state(&self) -> MotionState {
        MotionState {
            pose: self.current_pose(),
            received: self.received_pose,
            remaining: self.interpolation_ticks_remaining,
            interpolation: self.status.movement_interpolation,
            dying_dragon: self.is_dying_dragon(),
            network_offset: self.network_position_offset(),
            ground_offset: (self.on_ground == Some(true) && !self.is_minecart())
                .then(|| self.network_position_offset()),
        }
    }

    pub(super) fn apply_motion_state(&mut self, state: MotionState) {
        self.received_pose = state.received;
        self.interpolation_ticks_remaining = state.remaining;
        self.status.movement_interpolation = state.interpolation;
        self.set_current_pose(state.pose);
    }
}

impl ActorPose {
    /// Restores the actor origin, retaining a received height before feet conversion rounded it.
    pub(super) fn native_position(self, offset: f32) -> [f32; 3] {
        let mut position = self.position;
        position[1] = self.y_origin.native_y.unwrap_or(position[1] + offset);
        position
    }
}

/// The pose state of one actor's interpolation, advanced a tick at a time.
#[derive(Debug, Clone, Copy)]
pub(super) struct MotionState {
    pub(super) pose: ActorPose,
    pub(super) received: ActorPose,
    pub(super) remaining: u32,
    interpolation: MovementInterpolation,
    dying_dragon: bool,
    network_offset: f32,
    /// Native origin offset of a grounded non-minecart, whose lerp lifts onto block surfaces.
    ground_offset: Option<f32>,
}

/// One remote actor's motion after this frame's due ticks, for reads made before the live
/// actors advance.
#[derive(Debug, Clone)]
pub(super) struct PickState {
    pub(super) runtime_id: u64,
    /// The yaw one tick before `state`, which this frame eases from.
    pub(super) previous_yaw: f32,
    pub(super) state: MotionState,
}

impl MotionState {
    /// One interpolation tick: each step closes 1/n of the remaining gap and the final step
    /// lands exactly on the target; rotation follows its own counters.
    pub(super) fn tick(&mut self) {
        let current = self.pose;
        let mut next = if self.remaining == 0
            && (self.dying_dragon || self.interpolation.last_received.is_some())
        {
            current
        } else {
            self.received
        };
        if self.remaining > 1 {
            let fraction = 1.0 / self.remaining as f32;
            let target = self.received;
            next.y_origin = current.y_origin.interpolate(target.y_origin, fraction);
            next.position = std::array::from_fn(|axis| {
                current.position[axis] + (target.position[axis] - current.position[axis]) * fraction
            });
            if current.y_origin.native_y.is_some() || target.y_origin.native_y.is_some() {
                let from = current.native_position(self.network_offset)[1];
                let to = target.native_position(self.network_offset)[1];
                let native_y = from + (to - from) * fraction;
                next.y_origin.native_y = Some(native_y);
                next.position[1] = native_y - self.network_offset;
            }
        }
        self.interpolate_rotation(current, &mut next);
        self.remaining = self.remaining.saturating_sub(1);
        self.pose = next;
        self.advance_queue();
        self.lift_onto_surface();
    }

    /// While any counter runs, a grounded native origin within 0.01 below a block surface
    /// rises onto it.
    fn lift_onto_surface(&mut self) {
        let Some(offset) = self.ground_offset else {
            return;
        };
        let state = self.interpolation;
        if self.remaining == 0 && state.rotation_ticks == 0 && state.head_ticks == 0 {
            return;
        }
        let native = self.pose.native_position(offset)[1];
        let surface = (native + GROUND_SURFACE_TOLERANCE).floor();
        if surface != native.floor() {
            self.pose.position[1] = surface - offset;
            self.pose.y_origin.native_y = Some(surface);
        }
    }

    fn interpolate_rotation(&self, current: ActorPose, next: &mut ActorPose) {
        let state = self.interpolation;
        let step = |from: f32, to: f32, ticks: u32| match ticks {
            0 => from,
            1 => to,
            _ => from + wrap_degrees(to - from) * (1.0 / ticks as f32),
        };
        let rotation_ticks = if state.last_received.is_some() {
            state.rotation_ticks
        } else {
            self.remaining.max(1)
        };
        let head_ticks = if state.last_received.is_some() {
            state.head_ticks
        } else {
            rotation_ticks
        };
        if rotation_ticks > 0 {
            next.pitch = wrap_degrees(step(current.pitch, self.received.pitch, rotation_ticks));
            next.yaw = wrap_degrees(step(current.yaw, self.received.yaw, rotation_ticks));
        } else {
            next.pitch = current.pitch;
            next.yaw = current.yaw;
        }
        next.head_yaw = step(current.head_yaw, self.received.head_yaw, head_ticks);
    }

    fn advance_queue(&mut self) {
        let state = &mut self.interpolation;
        state.rotation_ticks = state.rotation_ticks.saturating_sub(1);
        state.head_ticks = state.head_ticks.saturating_sub(1);
        let Some(queued) = &mut state.queued else {
            return;
        };
        if state.must_complete && self.remaining != 0 {
            queued.ticks = queued.ticks.max(2) - 1;
            return;
        }
        let queued = state.queued.take().expect("queued target");
        self.received = ActorPose {
            head_yaw: self.pose.head_yaw,
            ..queued.pose
        };
        self.remaining = queued.ticks;
        state.rotation_ticks = queued.ticks;
        state.head_ticks = 0;
        state.must_complete = false;
    }
}

fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// Keeps pitch in the signed half-turn range so strike poses read an angle, not extra turns.
pub(super) fn signed_pitch(pitch: f32) -> f32 {
    if (-180.0..180.0).contains(&pitch) {
        pitch
    } else {
        wrap_degrees(pitch)
    }
}

#[cfg(test)]
mod tests;
