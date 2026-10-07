//! Borrowed tick poses separate aim evaluation from render interpolation.

use super::*;

impl LocalPhysicsController {
    /// Returns each unseen retained tick in order; a newly activated policy starts at the current tick.
    pub fn aim_poses_after(
        &self,
        last_tick: Option<u64>,
        through_tick: u64,
    ) -> impl Iterator<Item = (u64, [f32; 3], [f32; 3])> + '_ {
        let first = last_tick.map_or(through_tick, |tick| tick.saturating_add(1));
        let samples = self
            .sample_history
            .partition_point(|sample| sample.tick < first);
        let controllers = self
            .controller_history
            .partition_point(|frame| frame.tick < first);
        self.sample_history
            .range(samples..)
            .zip(self.controller_history.range(controllers..))
            .take_while(move |(sample, _)| sample.tick <= through_tick)
            .filter_map(|(sample, frame)| {
                if sample.tick != frame.tick {
                    return None;
                }
                let mut eye = sample.position;
                eye[1] += frame.eye_height - PLAYER_NETWORK_OFFSET;
                Some((sample.tick, eye, sample.camera_orientation))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movement::integration_tests::VersionedFloor;

    #[test]
    fn catch_up_aim_poses_use_each_tick_eye_and_borrow_retained_storage() {
        let world = VersionedFloor(1);
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 1.0 + PLAYER_NETWORK_OFFSET, 0.0], 0, true);
        let context = PhysicsSampleContext {
            camera_orientation: [0.0, 0.0, 1.0],
            ..Default::default()
        };
        let mut input = MovementInput {
            sneaking: true,
            ..Default::default()
        };
        input.forward = 1.0;
        let completed =
            physics.advance_with_context(Duration::from_millis(150), input, context, &world);
        assert_eq!(completed.samples.len(), 3);
        let poses: Vec<_> = physics.aim_poses_after(Some(0), 3).collect();
        assert_eq!(
            poses.iter().map(|pose| pose.0).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!((poses[0].1[1] - (1.0 + PLAYER_NETWORK_OFFSET - 0.175)).abs() < 1e-6);
        assert!((poses[1].1[1] - (1.0 + PLAYER_NETWORK_OFFSET - 0.2625)).abs() < 1e-6);
        assert_eq!(poses[2].2, context.camera_orientation);
        assert_eq!(physics.aim_poses_after(None, 3).count(), 1);
        assert_eq!(physics.aim_poses_after(Some(3), 3).count(), 0);
        physics.reanchor_network_position([0.0, 1.0 + PLAYER_NETWORK_OFFSET, 0.0], 5, true);
        assert_eq!(physics.aim_poses_after(Some(0), 5).count(), 0);
    }
}
