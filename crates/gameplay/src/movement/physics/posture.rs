//! Current posture for correction syncs, independent of uncompleted local input.

use super::*;

/// Actor flags the client sets on its own flag words, as of the latest completed sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::movement) struct SyncPose {
    pub sneaking: bool,
    pub sprinting: bool,
    pub pose: LocalPose,
    /// Nonzero processed movement input.
    pub moving: bool,
    /// Terrain collision, absent until a controller frame exists.
    pub has_collision: Option<bool>,
}

/// The pose flags both the client and the server write: gliding, a riptide
/// spin, swimming and crawling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::movement) struct LocalPose {
    pub gliding: bool,
    pub spinning: bool,
    pub swimming: bool,
    pub crawling: bool,
}

impl LocalPose {
    /// The pose a completed tick produced.
    pub(in crate::movement) fn of(processed: &super::super::state::ProcessedMovementState) -> Self {
        Self::from_mode(processed.mode, processed.spinning)
    }

    fn from_mode(mode: sim::MovementMode, spinning: bool) -> Self {
        Self {
            gliding: mode == sim::MovementMode::Gliding,
            spinning,
            swimming: mode == sim::MovementMode::Swimming,
            crawling: mode == sim::MovementMode::Crawling,
        }
    }

    fn lanes(&mut self) -> [&mut bool; 4] {
        [
            &mut self.gliding,
            &mut self.spinning,
            &mut self.swimming,
            &mut self.crawling,
        ]
    }
}

/// Server pose values that win over the local pose, as in vanilla's flag
/// component, where the latest writer of each flag wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::movement) struct ServerPoseFlags {
    gliding: Option<bool>,
    spinning: Option<bool>,
    swimming: Option<bool>,
    crawling: Option<bool>,
}

impl ServerPoseFlags {
    fn lanes(&mut self) -> [&mut Option<bool>; 4] {
        [
            &mut self.gliding,
            &mut self.spinning,
            &mut self.swimming,
            &mut self.crawling,
        ]
    }

    /// Records the server values that disagree with the local pose at their
    /// stamp while the client has not changed that flag since. Vanilla merges
    /// server flag words as differences against the frame they correct, so an
    /// echo of an earlier local state never undoes a later local change.
    fn note(&mut self, update: [Option<bool>; 4], mut at_stamp: LocalPose, mut now: LocalPose) {
        let lanes = self.lanes().into_iter();
        for (((lane, server), stamped), current) in
            lanes.zip(update).zip(at_stamp.lanes()).zip(now.lanes())
        {
            if let Some(server) = server
                && server != *stamped
                && *stamped == *current
            {
                *lane = Some(server);
            }
        }
    }

    /// A local transition of a flag makes the client its latest writer again.
    pub(in crate::movement) fn yield_to_local(
        &mut self,
        mut previous: LocalPose,
        mut completed: LocalPose,
    ) {
        for ((lane, before), after) in self
            .lanes()
            .into_iter()
            .zip(previous.lanes())
            .zip(completed.lanes())
        {
            if before != after {
                *lane = None;
            }
        }
    }

    fn apply(mut self, mut local: LocalPose) -> LocalPose {
        for (value, server) in local.lanes().into_iter().zip(self.lanes()) {
            if let Some(server) = *server {
                *value = server;
            }
        }
        local
    }
}

impl LocalPhysicsController {
    /// Overlays server posture accepted after the latest completed movement sample.
    pub(in crate::movement) fn prediction_sync_posture(&self) -> Option<(bool, bool)> {
        let (sneaking, sprinting) = self.latest_sneak_sprint()?;
        Some((
            self.live_posture.sneaking.unwrap_or(sneaking),
            self.live_posture.sprinting.unwrap_or(sprinting),
        ))
    }

    /// Actor flags as vanilla's flag component holds them once a movement
    /// sample has completed: the accepted posture, the current pose with any
    /// server pose value that won over it, movement input and terrain collision.
    pub(in crate::movement) fn prediction_sync_pose(&self) -> Option<SyncPose> {
        let sample = self.sample_history.back()?;
        let (sneaking, sprinting) = self.prediction_sync_posture()?;
        Some(SyncPose {
            sneaking,
            sprinting,
            pose: self.server_pose.apply(LocalPose::from_mode(
                self.modes.mode(),
                sample.processed.spinning,
            )),
            moving: sample.move_vector != [0.0; 2],
            has_collision: self
                .controller_history
                .back()
                .map(|frame| !frame.input.no_clip),
        })
    }

    /// Records server pose flags stamped `stamp` (`Live` when current) against
    /// the local pose at that frame and now.
    pub(in crate::movement) fn note_server_pose(
        &mut self,
        stamp: Option<u64>,
        flags: &client_world::MovementFlagUpdate,
    ) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let now = LocalPose::from_mode(self.modes.mode(), state.spin_attack_ticks.is_some());
        let at_stamp = stamp
            .and_then(|tick| {
                let input = self.history.input_at(tick)?;
                let state = self.history.state_at(tick)?;
                Some(LocalPose::from_mode(
                    input.mode,
                    state.spin_attack_ticks.is_some(),
                ))
            })
            .unwrap_or(now);
        self.server_pose.note(
            [
                flags.gliding,
                flags.spinning,
                flags.swimming,
                flags.crawling,
            ],
            at_stamp,
            now,
        );
    }

    /// Uses completed walking size with any newer accepted server sneak state.
    pub(in crate::movement) fn prediction_sync_walking_height(&self) -> Option<f32> {
        let frame = self.controller_history.back()?;
        let state = self.state.as_ref()?;
        if frame.tick != state.tick
            || frame.input.mode != sim::MovementMode::Walking
            || state.spin_attack_ticks.is_some()
        {
            return None;
        }
        let sneaking = self.live_posture.sneaking.unwrap_or(frame.input.sneaking);
        Some(frame.input.mode.hitbox_height(sneaking) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalPose, ServerPoseFlags};

    const SPINNING: LocalPose = LocalPose {
        gliding: false,
        spinning: true,
        swimming: false,
        crawling: false,
    };
    const IDLE: LocalPose = LocalPose {
        gliding: false,
        spinning: false,
        swimming: false,
        crawling: false,
    };

    /// A server clear of a spin the client still predicts wins until the spin changes locally.
    #[test]
    fn a_disagreeing_server_value_wins_until_a_local_transition() {
        let mut server = ServerPoseFlags::default();
        server.note([None, Some(false), None, None], SPINNING, SPINNING);
        assert_eq!(server.apply(SPINNING), IDLE);
        server.yield_to_local(IDLE, SPINNING);
        assert_eq!(server.apply(SPINNING), SPINNING);
    }

    /// An echo of the stamped frame, or a value the client changed since, leaves the local pose.
    #[test]
    fn echoes_and_superseded_values_never_override() {
        let mut server = ServerPoseFlags::default();
        server.note([None, Some(true), None, None], SPINNING, IDLE);
        server.note(
            [Some(false), Some(false), Some(false), None],
            IDLE,
            SPINNING,
        );
        assert_eq!(server, ServerPoseFlags::default());
        assert_eq!(server.apply(SPINNING), SPINNING);
    }
}
