use protocol::ActorInterpolation;

use super::{ACTOR_INTERPOLATION_TICKS, ActorPose, ActorSnapshot};

#[derive(Debug, Clone, Copy, PartialEq)]
struct QueuedInterpolation {
    pose: ActorPose,
    ticks: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct MovementInterpolation {
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
    pub(super) fn last_received_pose(&self) -> ActorPose {
        self.status
            .movement_interpolation
            .last_received
            .unwrap_or(self.received_pose)
    }

    pub(super) fn start_movement_interpolation(
        &mut self,
        pose: ActorPose,
        ticks: u32,
        force_completion: bool,
        teleported: bool,
    ) {
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

    /// The pose state an interpolation tick reads and writes.
    pub(super) fn motion_state(&self) -> MotionState {
        MotionState {
            pose: self.current_pose(),
            received: self.received_pose,
            remaining: self.interpolation_ticks_remaining,
            interpolation: self.status.movement_interpolation,
            dying_dragon: self.is_dying_dragon(),
        }
    }

    pub(super) fn apply_motion_state(&mut self, state: MotionState) {
        self.set_current_pose(state.pose);
        self.received_pose = state.received;
        self.interpolation_ticks_remaining = state.remaining;
        self.status.movement_interpolation = state.interpolation;
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
}

impl MotionState {
    /// One interpolation tick: each step closes 1/n of the remaining gap and the final step
    /// lands exactly on the target; rotation follows its own counters.
    pub(super) fn tick(&mut self) {
        let current = self.pose;
        let mut next = if self.remaining == 0 && self.dying_dragon {
            current
        } else {
            self.received
        };
        if self.remaining > 1 {
            let divisor = self.remaining as f32;
            let target = self.received;
            next.position = std::array::from_fn(|axis| {
                current.position[axis] + (target.position[axis] - current.position[axis]) / divisor
            });
        }
        self.interpolate_rotation(current, &mut next);
        self.remaining = self.remaining.saturating_sub(1);
        self.pose = next;
        self.advance_queue();
    }

    fn interpolate_rotation(&self, current: ActorPose, next: &mut ActorPose) {
        let state = self.interpolation;
        let step = |from: f32, to: f32, ticks: u32| match ticks {
            0 => from,
            1 => to,
            _ => from + wrap_degrees(to - from) / ticks as f32,
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
        next.pitch = step(current.pitch, self.received.pitch, rotation_ticks);
        next.yaw = step(current.yaw, self.received.yaw, rotation_ticks);
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
