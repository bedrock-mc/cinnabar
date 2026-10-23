use super::*;

/// Independent server pose bits affect the swim blend even beside glide or flight.
#[test]
fn server_combination_keeps_swim_state_beside_glide_and_flight() {
    for flying in [false, true] {
        let (mut physics, mut ticker) = airborne();
        let context = PhysicsSampleContext {
            mode_intent: super::super::super::ModeIntent {
                can_fly: flying,
                flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(flying),
                elytra_ready: true,
                ..Default::default()
            },
            ..Default::default()
        };
        tick(&mut physics, &mut ticker, context);
        physics.apply_server_movement_flags(
            0,
            flags(|flags| {
                flags.gliding = Some(true);
                flags.swimming = Some(true);
            }),
        );
        assert!(
            physics.state().unwrap().swim_pose_active,
            "the accepted swimming bit was lost with flying={flying}",
        );
        if flying {
            assert_eq!(physics.mode(), sim::MovementMode::Flying);
        }
    }
}

/// Each pose can produce its own stop action after a combined server write.
#[test]
fn server_combination_reports_each_local_stop_request() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, elytra(false));
    physics.apply_server_movement_flags(
        0,
        flags(|flags| {
            flags.gliding = Some(true);
            flags.swimming = Some(true);
        }),
    );
    tick(&mut physics, &mut ticker, elytra(false));
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_GLIDING), 1);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SWIMMING), 1);
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::START_SWIMMING), 0);
}

/// A compact accepted pose keeps a flying player beneath a low roof.
#[test]
fn server_combination_flight_uses_the_crawling_body() {
    let (mut physics, mut ticker) = airborne();
    let context = PhysicsSampleContext {
        mode_intent: super::super::super::ModeIntent {
            can_fly: true,
            flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
            ..Default::default()
        },
        ..Default::default()
    };
    tick(&mut physics, &mut ticker, context);
    physics.apply_server_movement_flags(0, flags(|flags| flags.crawling = Some(true)));
    let before = physics.state().unwrap().position;
    let eye_before =
        physics.render_eye_position().unwrap()[1] - physics.render_feet_position().unwrap()[1];
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        MovementInput {
            forward: 1.0,
            ..Default::default()
        },
        context,
        &LowRoof(before.y + 0.8),
    );
    assert_eq!(frame.completed_ticks, 1, "{:?}", frame.blocked);
    let after = physics.state().unwrap().position;
    assert!((after.x - before.x).abs() < 1.0e-6);
    assert!((after.y - before.y).abs() < 1.0e-5);
    assert!(after.z > before.z);
    assert_eq!(physics.mode(), sim::MovementMode::Flying);
    assert!(physics.pose_flags().crawling);
    physics.advance_with_context(
        simulation_clock::TICK_DURATION / 2,
        MovementInput::default(),
        context,
        &LowRoof(before.y + 0.8),
    );
    let eye_after =
        physics.render_eye_position().unwrap()[1] - physics.render_feet_position().unwrap()[1];
    assert!(eye_after < eye_before);
}

/// A current-tick correction and a spatial snap preserve every live pose bit.
#[test]
fn server_combination_survives_corrections_without_replayed_ticks() {
    for mode in [
        PhysicsCorrectionMode::ReplayIfRetained,
        PhysicsCorrectionMode::Snap,
    ] {
        let (mut physics, mut ticker) = airborne();
        let sample = tick(&mut physics, &mut ticker, elytra(true));
        physics.apply_server_movement_flags(
            0,
            flags(|flags| {
                flags.gliding = Some(true);
                flags.swimming = Some(true);
            }),
        );
        let expected = physics.pose_flags();
        assert!(expected.gliding && expected.swimming);
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            sample.position,
            sample.tick,
            false,
            mode,
            &VersionedFloor(1),
        )
        .unwrap();
        assert_eq!(physics.pose_flags(), expected);
        assert!(physics.state().unwrap().swim_pose_active);
    }
}

struct LowRoof(f64);

impl sim::CollisionWorld for LowRoof {
    /// Places one roof above the compact body while leaving horizontal travel open.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(vec![sim::Aabb::new(
            sim::Vec3::new(-8.0, self.0, -8.0),
            sim::Vec3::new(8.0, self.0 + 1.0, 8.0),
        )]))
    }
}
