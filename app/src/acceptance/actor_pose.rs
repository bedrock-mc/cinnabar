use std::collections::VecDeque;

use client_world::CommittedActorMove;
use render::ActorPresentedFrameAck;

use super::model_witness::{
    actor_pose_witness_marker, committed_actor_move_matches_presented_frame,
};

pub(crate) const MAX_PENDING_ACTOR_POSE_WITNESSES: usize = 128;
pub(crate) const MAX_ACTOR_POSE_WITNESS_EMISSIONS: u64 = 64;

#[derive(Debug, Default)]
pub(crate) struct ActorPoseWitnessTracker {
    session: Option<(u64, i32)>,
    pending: VecDeque<CommittedActorMove>,
    previous_frame: Option<ActorPresentedFrameAck>,
    rejected_commits: u64,
    rejected_frames: u64,
    stream_dropped_commits: u64,
    emissions: u64,
}

impl ActorPoseWitnessTracker {
    pub(crate) fn reset(&mut self) {
        self.session = None;
        self.pending.clear();
        self.previous_frame = None;
        self.rejected_commits = 0;
        self.rejected_frames = 0;
        self.stream_dropped_commits = 0;
        self.emissions = 0;
    }

    pub(crate) fn record_stream_drops(&mut self, dropped: u64) {
        self.stream_dropped_commits = self.stream_dropped_commits.max(dropped);
    }

    pub(crate) fn reconcile_session(&mut self, session_id: u64, dimension: i32) {
        let identity = Some((session_id, dimension));
        if self.session != identity {
            self.reset();
            self.session = identity;
        }
    }

    pub(crate) fn admit(&mut self, commits: impl IntoIterator<Item = CommittedActorMove>) {
        let Some((session_id, dimension)) = self.session else {
            return;
        };
        for commit in commits {
            let valid = commit.session_id == session_id
                && commit.dimension == dimension
                && commit.movement.dimension == dimension
                && commit.applied.as_ref().is_some_and(|applied| {
                    applied.lifetime.session_id == session_id
                        && applied.lifetime.dimension == dimension
                        && applied.lifetime.runtime_id == commit.movement.runtime_id
                        && applied.movement_revision == commit.sequence
                });
            if !valid || self.pending.len() == MAX_PENDING_ACTOR_POSE_WITNESSES {
                self.rejected_commits = self.rejected_commits.saturating_add(1);
                continue;
            }
            self.pending.push_back(commit);
        }
    }

    pub(crate) fn observe(
        &mut self,
        frames: impl IntoIterator<Item = ActorPresentedFrameAck>,
    ) -> Vec<String> {
        let mut markers = Vec::new();
        for frame in frames {
            if !frame.is_exact() {
                self.rejected_frames = self.rejected_frames.saturating_add(1);
                self.previous_frame = None;
                continue;
            }
            let Some(previous) = self.previous_frame.replace(frame.clone()) else {
                continue;
            };
            if !previous.forms_consecutive_pair_with(&frame) {
                self.rejected_frames = self.rejected_frames.saturating_add(1);
                continue;
            }
            let Some(index) = self.pending.iter().position(|commit| {
                committed_actor_move_matches_presented_frame(commit, &previous)
                    && committed_actor_move_matches_presented_frame(commit, &frame)
            }) else {
                continue;
            };
            for _ in 0..index {
                self.pending.pop_front();
                self.rejected_commits = self.rejected_commits.saturating_add(1);
            }
            let commit = self
                .pending
                .pop_front()
                .expect("matched pending actor commit exists");
            if self.emissions == MAX_ACTOR_POSE_WITNESS_EMISSIONS {
                self.rejected_commits = self.rejected_commits.saturating_add(1);
                continue;
            }
            markers.push(actor_pose_witness_marker(
                &commit,
                &previous,
                &frame,
                self.rejected_commits,
                self.rejected_frames,
                self.stream_dropped_commits,
            ));
            self.emissions += 1;
        }
        markers
    }

    #[cfg(test)]
    pub(crate) fn counts(&self) -> (usize, u64, u64, u64) {
        (
            self.pending.len(),
            self.rejected_commits,
            self.rejected_frames,
            self.emissions,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Instant};

    use client_world::{ActorLifetimeId, ActorPose, CommittedActorPose};
    use protocol::{ActorMoveEvent, ActorPositionOrigin};
    use render::{ActorDrawManifestEntry, ActorRenderIdentity, ActorRigRoute, EntityRigId};

    use super::*;

    fn commit(session_id: u64, dimension: i32, sequence: u64) -> CommittedActorMove {
        let pose = ActorPose {
            position: [1.0, 64.0, 2.0],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
        };
        CommittedActorMove {
            session_id,
            dimension,
            sequence,
            movement: ActorMoveEvent {
                dimension,
                runtime_id: 42,
                position: [Some(1.0), Some(65.620_01), Some(2.0)],
                position_origin: ActorPositionOrigin::NetworkOffset,
                pitch: Some(0.0),
                yaw: Some(0.0),
                head_yaw: Some(0.0),
                on_ground: Some(true),
                teleported: false,
                snap: false,
                player_mode: None,
                source_tick: Some(120),
            },
            applied: Some(CommittedActorPose {
                lifetime: ActorLifetimeId {
                    session_id,
                    dimension,
                    runtime_id: 42,
                    spawn_revision: 1,
                },
                movement_revision: sequence,
                previous_pose: pose,
                current_pose: pose,
                received_pose: pose,
                interpolation_ticks_remaining: 3,
                on_ground: Some(true),
                source_tick: Some(120),
            }),
        }
    }

    fn frame(session_id: u64, dimension: i32, sequence: u64, frame: u64) -> ActorPresentedFrameAck {
        let now = Instant::now();
        ActorPresentedFrameAck {
            frame_sequence: frame,
            frame_generation: frame,
            draw_generation: frame,
            manifest: Arc::from([ActorDrawManifestEntry {
                identity: ActorRenderIdentity {
                    session_id,
                    dimension,
                    runtime_id: 42,
                    spawn_revision: 1,
                    ingress_sequence: sequence,
                    source_tick: Some(120),
                    movement_revision: sequence,
                    pose_generation: sequence,
                },
                rig: EntityRigId(0),
                completed_tick: sequence,
                reset_generation: 1,
                route: ActorRigRoute::Compiled,
                instance_index: 0,
                previous_bone_base: 0,
                current_bone_base: 0,
                bone_count: 1,
            }]),
            present_returned_at: now,
            gpu_completed_at: now,
        }
    }

    #[test]
    fn emits_only_after_two_exact_consecutive_presented_frames() {
        let mut tracker = ActorPoseWitnessTracker::default();
        tracker.reconcile_session(3, 0);
        tracker.admit([commit(3, 0, 9)]);
        assert!(tracker.observe([frame(3, 0, 9, 1)]).is_empty());
        let markers = tracker.observe([frame(3, 0, 9, 2)]);
        assert_eq!(markers.len(), 1);
        assert!(markers[0].contains("\"consecutive\":true"));
        assert_eq!(tracker.counts(), (0, 0, 0, 1));
    }

    #[test]
    fn rejects_wrong_session_and_resets_at_dimension_boundary() {
        let mut tracker = ActorPoseWitnessTracker::default();
        tracker.reconcile_session(3, 0);
        tracker.admit([commit(4, 0, 9)]);
        assert_eq!(tracker.counts(), (0, 1, 0, 0));
        tracker.admit([commit(3, 0, 10)]);
        tracker.observe([frame(3, 0, 10, 1)]);
        tracker.reconcile_session(3, 1);
        assert_eq!(tracker.counts(), (0, 0, 0, 0));
        assert!(tracker.observe([frame(3, 0, 10, 2)]).is_empty());
    }

    #[test]
    fn rejects_non_consecutive_frames_and_bounds_pending_commits() {
        let mut tracker = ActorPoseWitnessTracker::default();
        tracker.reconcile_session(3, 0);
        tracker.admit(
            (0..=MAX_PENDING_ACTOR_POSE_WITNESSES)
                .map(|index| commit(3, 0, u64::try_from(index).unwrap() + 1)),
        );
        assert_eq!(
            tracker.counts(),
            (MAX_PENDING_ACTOR_POSE_WITNESSES, 1, 0, 0)
        );
        let first = frame(3, 0, 1, 1);
        let mut draw_gap = frame(3, 0, 1, 2);
        draw_gap.draw_generation = 3;
        tracker.observe([first, draw_gap]);
        assert_eq!(tracker.counts().2, 1);
    }

    #[test]
    fn witness_emission_is_bounded_for_the_session() {
        let mut tracker = ActorPoseWitnessTracker::default();
        tracker.reconcile_session(3, 0);
        let count = usize::try_from(MAX_ACTOR_POSE_WITNESS_EMISSIONS).unwrap() + 1;
        tracker.admit((0..count).map(|index| commit(3, 0, index as u64 + 1)));
        let frames = (0..count).flat_map(|index| {
            let sequence = index as u64 + 1;
            [
                frame(3, 0, sequence, sequence * 2 - 1),
                frame(3, 0, sequence, sequence * 2),
            ]
        });
        let markers = tracker.observe(frames);
        assert_eq!(markers.len() as u64, MAX_ACTOR_POSE_WITNESS_EMISSIONS);
        assert_eq!(
            tracker.counts(),
            (0, 1, 0, MAX_ACTOR_POSE_WITNESS_EMISSIONS)
        );
    }
}
