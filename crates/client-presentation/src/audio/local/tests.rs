use super::*;

/// Creates a dry movement sample for motion-edge tests.
fn at(x: f64, y: f64, vy: f64, on_ground: bool) -> MotionSample {
    MotionSample {
        position: [x, y, 0.0],
        velocity_y: vy,
        entry_velocity: [0.0, vy as f32, 0.0],
        movement: [0.0; 3],
        on_ground,
        sneaking: false,
        in_water: false,
    }
}

#[test]
fn walking_steps_once_per_stride_and_sneaking_is_silent() {
    let mut motion = LocalMotion::default();
    let mut steps = 0;
    for tick in 0..=40 {
        steps += motion
            .advance(at(f64::from(tick) * 0.5, 0.0, 0.0, true))
            .iter()
            .filter(|cue| **cue == LocalCue::Step)
            .count();
    }
    assert_eq!(steps, 10);
    motion.reset();
    for tick in 0..=40 {
        let mut sample = at(f64::from(tick) * 0.5, 0.0, 0.0, true);
        sample.sneaking = true;
        assert!(motion.advance(sample).is_empty());
    }
}

#[test]
fn jump_and_land_are_edge_triggered() {
    let mut motion = LocalMotion::default();
    motion.advance(at(0.0, 0.0, 0.0, true));
    assert_eq!(motion.advance(at(0.0, 0.4, 0.42, false)), [LocalCue::Jump]);
    assert!(motion.advance(at(0.0, 0.7, 0.2, false)).is_empty());
    motion.advance(at(0.0, 0.3, -0.5, false));
    let landed = motion.advance(at(0.0, 0.0, 0.0, true));
    assert_eq!(landed, [LocalCue::Land { speed: 0.5 }]);
}

#[test]
fn water_entry_splashes_without_a_downward_speed_gate() {
    let mut motion = LocalMotion::default();
    motion.advance(at(0.0, 5.0, 0.1, false));
    let mut wet = at(0.1, 5.1, 0.05, false);
    wet.in_water = true;
    assert!(
        motion
            .advance(wet)
            .iter()
            .any(|cue| matches!(cue, LocalCue::Splash { .. }))
    );
    assert!(
        !motion
            .advance(wet)
            .iter()
            .any(|cue| matches!(cue, LocalCue::Splash { .. }))
    );
}

#[test]
fn entering_water_splashes_and_teleports_are_ignored() {
    let mut motion = LocalMotion::default();
    motion.advance(at(0.0, 5.0, -0.4, false));
    let mut wet = at(0.0, 4.5, -0.4, false);
    wet.in_water = true;
    assert!(
        motion
            .advance(wet)
            .iter()
            .any(|cue| matches!(cue, LocalCue::Splash { .. }))
    );
    assert!(motion.advance(at(100.0, 5.0, 0.0, true)).is_empty());
}

/// A water column with empty collision geometry isolates fall and water-travel motion.
struct WaterColumn;

impl sim::CollisionWorld for WaterColumn {
    fn collision_boxes(
        &self,
        _query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(vec![]))
    }

    fn block_physics(
        &self,
        block: [i32; 3],
    ) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        let water = block[1] <= 0;
        Ok(sim::BlockPhysicsSample {
            layers: Box::new([sim::BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if water { 1.0 } else { 0.0 },
                flags: if water {
                    sim::BlockPhysicsFlags::WATER
                } else {
                    sim::BlockPhysicsFlags::default()
                },
                surface_response: sim::SurfaceResponse::None,
            }]),
            identity: sim::CollisionQuery::synthetic(()).identity,
        })
    }
}

#[test]
fn splash_volume_uses_undamped_tick_start_speed_at_several_fall_heights() {
    let simulator = sim::Simulator::default();
    let mut volumes = Vec::new();
    for height in [1.0, 4.0, 20.0, 100.0] {
        let mut state = sim::PlayerState::new(sim::Vec3::new(0.5, height + 1.0, 0.5));
        let mut motion = LocalMotion::default();
        motion.advance(at(0.5, height + 1.0, 0.0, false));
        let mut splash = None;
        for _ in 0..200 {
            let entry_velocity = [
                state.velocity.x as f32,
                state.velocity.y as f32,
                state.velocity.z as f32,
            ];
            let result = simulator
                .tick(&mut state, sim::MovementInput::default(), &WaterColumn)
                .unwrap();
            let sample = MotionSample {
                position: [state.position.x, state.position.y, state.position.z],
                velocity_y: state.velocity.y,
                entry_velocity,
                movement: [
                    state.movement.x as f32,
                    state.movement.y as f32,
                    state.movement.z as f32,
                ],
                on_ground: state.on_ground,
                sneaking: false,
                in_water: result.environment.in_water,
            };
            let cues = motion.advance(sample);
            if let Some(LocalCue::Splash { volume }) = cues
                .iter()
                .find(|cue| matches!(cue, LocalCue::Splash { .. }))
            {
                let scale = super::super::water::VOLUME_DATA_SCALE;
                let expected_data = (entry_velocity[1].abs() * 0.2).min(1.0) * scale;
                let expected = expected_data as i32 as f32 / scale;
                assert_eq!(*volume, expected);
                assert_ne!(
                    *volume,
                    super::super::water::motion_volume(
                        [
                            state.velocity.x as f32,
                            state.velocity.y as f32,
                            state.velocity.z as f32
                        ],
                        super::super::water::SPLASH_SCALE,
                    )
                );
                assert!(
                    !motion
                        .advance(sample)
                        .iter()
                        .any(|cue| matches!(cue, LocalCue::Splash { .. }))
                );
                splash = Some(*volume);
                break;
            }
        }
        volumes.push(splash.expect("the falling player must reach water"));
    }
    assert!(volumes.windows(2).all(|pair| pair[0] < pair[1]));
}
