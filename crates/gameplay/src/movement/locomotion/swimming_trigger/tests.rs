use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, PLAYER_HEIGHT,
    SurfaceResponse,
};

use super::*;

struct Pool {
    top: i32,
    depth: u8,
    ceiling: Option<f64>,
    secondary_water: bool,
}

impl Pool {
    fn deep() -> Self {
        Self {
            top: 4,
            depth: 0,
            ceiling: None,
            secondary_water: false,
        }
    }

    fn facts(&self, water: bool) -> BlockPhysicsFacts {
        BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: if water {
                if self.depth >= 8 {
                    1.0
                } else {
                    f64::from(8 - self.depth) / 9.0
                }
            } else {
                0.0
            },
            flags: if water {
                BlockPhysicsFlags::WATER
            } else {
                BlockPhysicsFlags::default()
            },
            surface_response: SurfaceResponse::None,
        }
    }
}

impl CollisionWorld for Pool {
    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(Some(CollisionQuery::synthetic(
            block[1] >= self.top || self.secondary_water,
        )))
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.ceiling
                .map(|y| Aabb::new(Vec3::new(-4.0, y, -4.0), Vec3::new(4.0, y + 1.0, 4.0)))
                .filter(|shape| shape.intersects(query))
                .into_iter()
                .collect(),
        ))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let water = block[1] < self.top;
        let layers = if self.secondary_water {
            vec![self.facts(false), self.facts(water)]
        } else {
            vec![self.facts(water)]
        };
        Ok(BlockPhysicsSample {
            layers: layers.into_boxed_slice(),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn observed() -> ModeObservation {
    ModeObservation {
        feet: Vec3::ZERO,
        on_ground: false,
        velocity_y: 0.0,
        in_water: true,
        in_lava: false,
        sprinting: true,
        sprint_blinded: false,
        sprint_down: false,
        input_mode: protocol::PlayerInputMode::Mouse,
        requested_movement: Vec3::ZERO,
        move_sideways: 0.0,
        move_forward: 1.0,
        sneaking: false,
        pitch: 0.0,
        yaw: 0.0,
        liquid_attach_height: protocol::STANDING_PLAYER_EYE_HEIGHT,
        jumping: false,
        jump_edge: false,
    }
}

#[test]
fn wet_swimming_retains_actual_sprint_through_backward_sideways_and_release() {
    let mut tracker = super::super::ModeTracker::default();
    let pool = Pool::deep();
    let started = tracker
        .select(ModeIntent::default(), false, observed(), &pool)
        .unwrap();
    assert_eq!(started.mode, MovementMode::Swimming);
    assert!(started.sprinting);
    for (move_sideways, move_forward) in [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0)] {
        let continuing = ModeObservation {
            sprinting: false,
            move_sideways,
            move_forward,
            ..observed()
        };
        let choice = tracker
            .select(ModeIntent::default(), false, continuing, &pool)
            .unwrap();
        assert_eq!(choice.mode, MovementMode::Swimming);
        assert!(choice.sprinting);
    }
    let dry = Pool {
        top: -2,
        ..Pool::deep()
    };
    let choice = tracker
        .select(
            ModeIntent::default(),
            false,
            ModeObservation {
                sprinting: true,
                move_forward: -1.0,
                ..observed()
            },
            &dry,
        )
        .unwrap();
    assert_eq!(choice.mode, MovementMode::Walking);
    assert!(!choice.sprinting);
}

#[test]
fn first_stop_swimming_tick_retains_sprint_until_the_old_pose_is_dry() {
    let mut tracker = super::super::ModeTracker::default();
    let pool = Pool::deep();
    tracker
        .select(ModeIntent::default(), false, observed(), &pool)
        .unwrap();
    let idle = ModeObservation {
        sprinting: false,
        move_forward: 0.0,
        ..observed()
    };
    let stopping = tracker
        .select(ModeIntent::default(), false, idle, &pool)
        .unwrap();
    assert_eq!(stopping.mode, MovementMode::Walking);
    assert!(stopping.sprinting);
    let next = tracker
        .select(ModeIntent::default(), false, idle, &pool)
        .unwrap();
    assert!(!next.sprinting);
}

#[test]
fn wet_swimming_does_not_invent_a_missing_sprint_flag() {
    let mut tracker = super::super::ModeTracker::default();
    tracker.restore_mode(MovementMode::Swimming);
    let choice = tracker
        .select(
            ModeIntent::default(),
            false,
            ModeObservation {
                sprinting: false,
                ..observed()
            },
            &Pool::deep(),
        )
        .unwrap();
    assert_eq!(choice.mode, MovementMode::Swimming);
    assert!(!choice.sprinting);
}

#[test]
fn surface_nan_angle_stops_upward_swimming_and_retains_level_or_downward() {
    assert!(!surface_angle_keeps_swimming(Vec3::new(
        1.000_001, 0.01, 0.0
    )));
    assert!(surface_angle_keeps_swimming(Vec3::new(1.000_001, 0.0, 0.0)));
    assert!(surface_angle_keeps_swimming(Vec3::new(
        1.000_001, -0.01, 0.0
    )));
}

#[test]
fn source_water_head_sensing_uses_native_level_above_rendered_surface() {
    let pool = Pool {
        top: 2,
        ..Pool::deep()
    };
    let input = ModeObservation {
        feet: Vec3::new(0.0, 0.32, 0.0),
        ..observed()
    };
    let rendered = 1.0 + pool.facts(true).fluid_height_blocks;
    assert!(input.feet.y + f64::from(input.liquid_attach_height) > rendered);
    assert!(head_in_water(&pool, input).unwrap());
    assert!(select(MovementMode::Walking, ModeIntent::default(), input, &pool).unwrap());
}

#[test]
fn shallow_flowing_water_uses_its_native_level_boundary() {
    let pool = Pool {
        top: 2,
        depth: 4,
        ..Pool::deep()
    };
    let input = ModeObservation {
        liquid_attach_height: 1.5,
        ..observed()
    };
    assert!(head_in_water(&pool, input).unwrap());
    let above = ModeObservation {
        liquid_attach_height: 1.56,
        ..input
    };
    assert!(!head_in_water(&pool, above).unwrap());
}

#[test]
fn secondary_water_does_not_assert_the_primary_head_material() {
    let pool = Pool {
        secondary_water: true,
        ..Pool::deep()
    };
    assert!(!head_in_water(&pool, observed()).unwrap());
    assert!(
        !select(
            MovementMode::Walking,
            ModeIntent::default(),
            observed(),
            &pool
        )
        .unwrap()
    );
}

#[test]
fn a_swimmer_keeps_backward_and_sideways_input_without_sprint() {
    for (sideways, forward) in [(0.0, -1.0), (1.0, 0.0)] {
        let input = ModeObservation {
            sprinting: false,
            move_sideways: sideways,
            move_forward: forward,
            ..observed()
        };
        assert!(
            select(
                MovementMode::Swimming,
                ModeIntent::default(),
                input,
                &Pool::deep()
            )
            .unwrap()
        );
    }
}

#[test]
fn swim_continuation_uses_native_input_threshold_and_hunger_request() {
    let pool = Pool::deep();
    let threshold = ModeObservation {
        move_forward: MIN_SWIM_INPUT,
        ..observed()
    };
    assert!(
        select(
            MovementMode::Swimming,
            ModeIntent::default(),
            threshold,
            &pool
        )
        .unwrap()
    );
    let below = ModeObservation {
        move_forward: f32::from_bits(MIN_SWIM_INPUT.to_bits() - 1),
        ..threshold
    };
    assert!(!select(MovementMode::Swimming, ModeIntent::default(), below, &pool).unwrap());
    let hungry = ModeIntent {
        swim_hunger_blocked: true,
        ..ModeIntent::default()
    };
    assert!(!select(MovementMode::Swimming, hungry, observed(), &pool).unwrap());
}

#[test]
fn upward_surface_exit_uses_horizontal_squared_angle_and_captured_pose_height() {
    let pool = Pool {
        top: 1,
        ..Pool::deep()
    };
    let input = ModeObservation {
        feet: Vec3::new(0.0, 0.6, 0.0),
        liquid_attach_height: 0.5,
        pitch: -45.0,
        ..observed()
    };
    assert!(!select(MovementMode::Swimming, ModeIntent::default(), input, &pool).unwrap());
    let level = ModeObservation {
        pitch: -30.0,
        ..input
    };
    assert!(select(MovementMode::Swimming, ModeIntent::default(), level, &pool).unwrap());
    let diving = ModeObservation {
        pitch: 45.0,
        ..input
    };
    assert!(select(MovementMode::Swimming, ModeIntent::default(), diving, &pool).unwrap());
    let submerged_eye = ModeObservation {
        liquid_attach_height: 0.25,
        ..input
    };
    assert!(
        select(
            MovementMode::Swimming,
            ModeIntent::default(),
            submerged_eye,
            &pool
        )
        .unwrap()
    );
}

#[test]
fn native_stop_waits_for_standing_fit_instead_of_relabeling_as_crawl() {
    let mut tracker = super::super::ModeTracker::default();
    tracker.restore_mode(MovementMode::Swimming);
    let idle = ModeObservation {
        move_forward: 0.0,
        sprinting: false,
        ..observed()
    };
    let blocked = Pool {
        ceiling: Some(1.5),
        ..Pool::deep()
    };
    assert_eq!(
        tracker
            .select(ModeIntent::default(), false, idle, &blocked)
            .unwrap()
            .mode,
        MovementMode::Swimming
    );
    assert_eq!(
        tracker
            .select(ModeIntent::default(), false, idle, &Pool::deep())
            .unwrap()
            .mode,
        MovementMode::Walking
    );
}

#[test]
fn swim_trigger_samples_current_position_instead_of_stale_wet_contact() {
    let mut tracker = super::super::ModeTracker::default();
    tracker.restore_mode(MovementMode::Swimming);
    let stale_wet = ModeObservation {
        feet: Vec3::new(0.0, 4.0, 0.0),
        liquid_attach_height: 0.5,
        ..observed()
    };
    assert_eq!(
        tracker
            .select(ModeIntent::default(), false, stale_wet, &Pool::deep())
            .unwrap()
            .mode,
        MovementMode::Walking
    );
    let fresh_wet = ModeObservation {
        in_water: false,
        ..observed()
    };
    assert_eq!(
        tracker
            .select(ModeIntent::default(), false, fresh_wet, &Pool::deep())
            .unwrap()
            .mode,
        MovementMode::Swimming
    );
}

#[test]
fn standing_fit_has_native_inset_at_the_ceiling() {
    let pool = Pool {
        ceiling: Some(PLAYER_HEIGHT - 0.005),
        ..Pool::deep()
    };
    assert!(standing_fits(&pool, Vec3::ZERO).unwrap());
    let overlap = Pool {
        ceiling: Some(PLAYER_HEIGHT - 0.02),
        ..pool
    };
    assert!(!standing_fits(&overlap, Vec3::ZERO).unwrap());
}

#[test]
fn standing_fit_shrinks_horizontal_faces_before_querying() {
    struct Wall(f64);
    impl CollisionWorld for Wall {
        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            let wall = Aabb::new(Vec3::new(self.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0));
            Ok(CollisionQuery::synthetic(if wall.intersects(query) {
                vec![wall]
            } else {
                Vec::new()
            }))
        }
    }
    let edge = Aabb::player_at(Vec3::ZERO).max.x;
    assert!(standing_fits(&Wall(edge - 0.005), Vec3::ZERO).unwrap());
    assert!(!standing_fits(&Wall(edge - 0.02), Vec3::ZERO).unwrap());
}
