use super::{JavaMotion, LocalSwingProgress, java::body};

/// Resolved local motion and attack observations from one completed simulation tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalSwingMotionSample {
    pub tick: u64,
    pub delta: [f32; 3],
    pub yaw: f32,
    pub progress: LocalSwingProgress,
}

/// One local body clock retains only the latest tick's reversible heading step.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct State {
    authority: Option<(u64, u64)>,
    last: Option<LocalSwingMotionSample>,
    before: [f32; 2],
}

impl State {
    /// Changing simulation ownership collapses the old endpoint and clears retry history.
    pub(super) fn set_authority(&mut self, authority: Option<(u64, u64)>, motion: &mut JavaMotion) {
        if self.authority != authority {
            *self = Self {
                authority,
                ..Self::default()
            };
            motion.body_yaw = [motion.body_yaw[1]; 2];
            motion.body_frame_alpha = None;
        }
    }

    /// Physics-owned headings never advance on the independent actor clock.
    pub(super) fn active(&self) -> bool {
        self.authority.is_some()
    }

    /// A changed latest tick replays its heading from the retained pre-tick checkpoint.
    pub(super) fn apply(
        &mut self,
        authority: (u64, u64),
        sample: LocalSwingMotionSample,
        motion: &mut JavaMotion,
    ) -> bool {
        if self.authority != Some(authority)
            || self.last.is_some_and(|last| sample.tick < last.tick)
        {
            return false;
        }
        let previous_heading = motion.body_yaw;
        if let Some(last) = self.last.filter(|last| sample.tick == last.tick) {
            motion.body_frame_alpha = sample.progress.frame_alpha;
            if sample.delta == last.delta
                && sample.yaw == last.yaw
                && sample.progress.java[1] == last.progress.java[1]
            {
                return false;
            }
            motion.body_yaw = self.before;
        } else {
            self.before = motion.body_yaw;
        }
        body::advance(
            &mut motion.body_yaw,
            sample.delta,
            sample.yaw,
            sample.progress.java[1],
        );
        motion.body_frame_alpha = sample.progress.frame_alpha;
        self.last = Some(sample);
        motion.body_yaw != previous_heading
    }
}

impl super::ActorAnimationStore {
    /// Binds Java torso ticks to the local simulation, including a rig created after binding.
    pub(crate) fn set_local_motion_authority(
        &mut self,
        runtime_id: u64,
        authority: Option<(u64, u64)>,
    ) {
        if let Some((previous, _)) = self.local_motion_authority
            && previous != runtime_id
            && let Some(state) = self
                .runtime_to_lifetime
                .get(&previous)
                .and_then(|lifetime| self.rigs.get_mut(lifetime))
        {
            state
                .java
                .local_body
                .set_authority(None, &mut state.java.motion);
        }
        self.local_motion_authority = authority.map(|identity| (runtime_id, identity));
        if let Some(state) = self
            .runtime_to_lifetime
            .get(&runtime_id)
            .and_then(|lifetime| self.rigs.get_mut(lifetime))
        {
            state
                .java
                .local_body
                .set_authority(authority, &mut state.java.motion);
        }
    }

    /// Applies only local torso motion; tick clocks, equip, limbs and cape remain untouched.
    pub(crate) fn sync_local_swing_motion(
        &mut self,
        runtime_id: u64,
        authority: (u64, u64),
        samples: impl IntoIterator<Item = LocalSwingMotionSample>,
    ) -> bool {
        if self.local_motion_authority != Some((runtime_id, authority)) {
            return false;
        }
        let Some(state) = self
            .runtime_to_lifetime
            .get(&runtime_id)
            .and_then(|lifetime| self.rigs.get_mut(lifetime))
        else {
            return false;
        };
        let mut changed = false;
        for sample in samples {
            changed |= state
                .java
                .local_body
                .apply(authority, sample, &mut state.java.motion);
        }
        changed
    }
}
