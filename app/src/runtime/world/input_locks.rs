use client_interaction::movement::LocalPhysicsController;
use client_world::LocalPlayerFacts;
use protocol::ClientInputLocks;

/// Applies committed permissions even when prediction cannot advance this frame.
pub(super) fn apply_committed_input_locks(
    session: u64,
    locks: ClientInputLocks,
    facts: &mut LocalPlayerFacts,
    physics: &mut LocalPhysicsController,
) {
    if facts.session_id() != session {
        return;
    }
    facts.apply_input_locks(session, locks);
    physics.apply_input_locks(locks);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gameplay::movement::PhysicsSampleContext;
    use protocol::ClientInputLock;
    use semantic_input::ActionPhase;
    use std::time::Duration;

    struct Floor;

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
        apply_committed_input_locks(
            session,
            ClientInputLocks::from_bits(lock.map_or(0, |lock| lock as u32)),
            &mut facts,
            &mut physics,
        );
        apply_committed_input_locks(7, ClientInputLocks::default(), &mut facts, &mut physics);
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
