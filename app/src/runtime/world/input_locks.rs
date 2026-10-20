use client_interaction::movement::{LocalPhysicsController, LocomotionState};
use client_presentation::camera::look::LookSmoother;
use client_world::LocalPlayerFacts;
use protocol::ClientInputLocks;

/// Commits permissions and revokes every blocked request before any frame consumer runs.
pub(super) fn apply_committed_input_locks(
    session: u64,
    locks: ClientInputLocks,
    facts: &mut LocalPlayerFacts,
    physics: &mut LocalPhysicsController,
    locomotion: &mut LocomotionState,
    look: &mut LookSmoother,
) {
    if facts.session_id() != session {
        return;
    }
    facts.apply_input_locks(session, locks);
    physics.revoke_input_requests(locks);
    locomotion.revoke_input_requests(locks);
    if locks.blocks(protocol::ClientInputLock::Camera) {
        look.reset();
    }
}

#[cfg(test)]
#[path = "input_locks/matrix.rs"]
mod matrix;

#[cfg(test)]
mod tests {
    use super::*;
    use gameplay::movement::PhysicsSampleContext;
    use protocol::ClientInputLock;
    use semantic_input::ActionPhase;
    use std::time::Duration;

    /// Supplies a live frame for retaining movement requests independently of transport.
    pub(super) fn movement_frame() -> gameplay::movement::PhysicsFrameInput {
        gameplay::movement::PhysicsFrameInput {
            delta: Duration::from_millis(10),
            now: Duration::from_secs(1),
            active: true,
            input_authority_released: false,
            resolved_movement: false,
            movement: [0.0; 2],
            raw_movement: [0.0; 2],
            analogue_movement: [0.0; 2],
            movement_buttons: Default::default(),
            yaw: 180.0,
            pitch: 0.0,
            camera_orientation: [0.0, 0.0, -1.0],
            input_mode: protocol::PlayerInputMode::Mouse,
            jump: Default::default(),
            sprint: Default::default(),
            sneak: Default::default(),
            toggle_sprint: false,
            always_sprint: false,
            toggle_sneak: false,
            facts: Default::default(),
            item_use_modifier: None,
            hold: None,
        }
    }

    #[test]
    fn server_input_locks_revoke_dismount_before_a_paused_lock_unlock_pair() {
        let mut locals = LocomotionState::default();
        let mut physics = LocalPhysicsController::default();
        let mut look = LookSmoother::default();
        let mut facts = LocalPlayerFacts::new(7);
        let mut frame = movement_frame();
        frame.facts.ride = Some(gameplay::movement::RideKind::Other);
        frame.facts.mount_runtime_id = Some(9);
        frame.sneak.pressed = true;
        locals.observe_dismount(&frame, 7);
        let mut attempts = 0;
        locals.flush_dismount([0.0; 3], |_| {
            attempts += 1;
            Err(protocol::BatchSendError::Full)
        });
        assert_eq!(attempts, 1);
        for locks in [
            ClientInputLocks::from_bits(ClientInputLock::Dismount as u32),
            ClientInputLocks::default(),
        ] {
            apply_committed_input_locks(7, locks, &mut facts, &mut physics, &mut locals, &mut look);
        }
        frame.sneak = Default::default();
        locals.observe_dismount(&frame, 7);
        locals.flush_dismount([0.0; 3], |_| {
            panic!("lock/unlock replayed a retained dismount")
        });
        frame.sneak.pressed = true;
        locals.observe_dismount(&frame, 7);
        locals.flush_dismount([0.0; 3], |_| {
            attempts += 1;
            Ok(())
        });
        assert_eq!(attempts, 2);
    }

    pub(super) struct Floor;

    impl sim::CollisionWorld for Floor {
        /// Supplies loaded ground so retained requests can produce a real takeoff.
        fn collision_boxes(
            &self,
            query: sim::Aabb,
        ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
            let floor = sim::Aabb::new(
                sim::Vec3::new(-16.0, 0.0, -16.0),
                sim::Vec3::new(16.0, 1.0, 16.0),
            );
            Ok(sim::CollisionQuery::synthetic(
                floor
                    .intersects(query)
                    .then_some(floor)
                    .into_iter()
                    .collect(),
            ))
        }
    }

    /// Retains one request, commits lock and unlock without physics, then resumes.
    fn paused_sample(
        lock: Option<ClientInputLock>,
        session: u64,
        jump: bool,
        sneak: bool,
        pulse: bool,
    ) -> gameplay::movement::PhysicsMovementSample {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(
            [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
            100,
            true,
        );
        let mut context = PhysicsSampleContext::default();
        context.input.jump.pressed = jump;
        context.input.sneak.pressed = sneak;
        assert!(
            physics
                .advance_with_context(
                    Duration::from_millis(10),
                    sim::MovementInput::default(),
                    context,
                    &Floor,
                )
                .samples
                .is_empty()
        );
        if pulse {
            physics.set_jump_pulse_scope(Some((7, 0)));
            physics.request_jump_pulse();
        }
        let mut facts = LocalPlayerFacts::new(7);
        let mut locals = LocomotionState::default();
        let mut look = LookSmoother::default();
        apply_committed_input_locks(
            session,
            ClientInputLocks::from_bits(lock.map_or(0, |lock| lock as u32)),
            &mut facts,
            &mut physics,
            &mut locals,
            &mut look,
        );
        apply_committed_input_locks(
            7,
            ClientInputLocks::default(),
            &mut facts,
            &mut physics,
            &mut locals,
            &mut look,
        );
        let result = physics.advance(
            Duration::from_millis(40),
            sim::MovementInput::default(),
            &Floor,
        );
        assert_eq!(result.samples.len(), 1);
        result.samples.into_iter().next().unwrap()
    }

    #[test]
    fn server_input_locks_revoke_retained_jump_during_paused_physics() {
        for (lock, session, blocked) in [
            (Some(ClientInputLock::Jump), 7, true),
            (Some(ClientInputLock::Movement), 7, true),
            (Some(ClientInputLock::Camera), 7, false),
            (Some(ClientInputLock::Jump), 8, false),
            (None, 7, false),
        ] {
            let sample = paused_sample(lock, session, true, false, false);
            assert_eq!(sample.processed.jump_initiated, !blocked, "{lock:?}");
            assert_eq!(sample.input.jump.pressed, !blocked, "{lock:?}");
        }
    }

    #[test]
    fn server_input_locks_revoke_retained_mod_jump_during_paused_physics() {
        for (lock, blocked) in [
            (ClientInputLock::Jump, true),
            (ClientInputLock::Movement, true),
            (ClientInputLock::Camera, false),
        ] {
            let sample = paused_sample(Some(lock), 7, false, false, true);
            assert_eq!(sample.processed.jump_initiated, !blocked, "{lock:?}");
        }
    }

    #[test]
    fn server_input_locks_revoke_retained_sneak_during_paused_physics() {
        for (lock, blocked) in [
            (ClientInputLock::Sneak, true),
            (ClientInputLock::Movement, true),
            (ClientInputLock::Camera, false),
        ] {
            let sample = paused_sample(Some(lock), 7, false, true, false);
            assert_eq!(
                sample.input.sneak,
                ActionPhase {
                    pressed: !blocked,
                    ..Default::default()
                },
                "{lock:?}"
            );
        }
    }
}
