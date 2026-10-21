use sim::{Aabb, BlockPhysicsFacts, BlockPhysicsSample, CollisionQuery};

use super::*;

struct Ceiling(Option<f64>);

impl CollisionWorld for Ceiling {
    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(Some(CollisionQuery::synthetic(
            self.0.is_none_or(|height| f64::from(block[1]) < height),
        )))
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.0
                .map(|y| Aabb::new(Vec3::new(-8.0, y, -8.0), Vec3::new(8.0, y + 1.0, 8.0)))
                .filter(|shape| shape.intersects(query))
                .into_iter()
                .collect(),
        ))
    }
}

/// Open water up to `surface`.
struct Pool(f64);

impl CollisionWorld for Pool {
    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(Some(CollisionQuery::synthetic(
            f64::from(block[1]) >= self.0,
        )))
    }

    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let depth = (self.0 - f64::from(block[1])).clamp(0.0, 1.0);
        let mut sample = Ceiling(None).block_physics(block)?;
        sample.layers = Box::new([BlockPhysicsFacts {
            fluid_height_blocks: depth,
            flags: if depth > 0.0 {
                sim::BlockPhysicsFlags::WATER
            } else {
                sim::BlockPhysicsFlags::default()
            },
            ..sample.layers[0]
        }]);
        Ok(sample)
    }
}

/// Every block carries `flags` with no collision.
struct Filled(sim::BlockPhysicsFlags);

impl CollisionWorld for Filled {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = Ceiling(None).block_physics(block)?;
        sample.layers = Box::new([BlockPhysicsFacts {
            fluid_height_blocks: if self.0.contains(sim::BlockPhysicsFlags::LAVA) {
                1.0
            } else {
                0.0
            },
            flags: self.0,
            ..sample.layers[0]
        }]);
        Ok(sample)
    }
}

fn airborne() -> ModeObservation {
    ModeObservation {
        feet: Vec3::new(0.0, 10.0, 0.0),
        on_ground: false,
        in_water: false,
        sprinting: false,
        sprint_blinded: false,
        sprint_down: false,
        input_mode: protocol::PlayerInputMode::Mouse,
        requested_movement: Vec3::ZERO,
        move_sideways: 0.0,
        move_forward: 0.0,
        sneaking: false,
        pitch: 0.0,
        yaw: 0.0,
        liquid_attach_height: protocol::STANDING_PLAYER_EYE_HEIGHT,
        liquid_contact_height: None,
        jump_edge: false,
        jump_down: false,
    }
}

#[test]
fn spectator_stays_flying_without_mayfly_or_a_flying_ability_edge() {
    let mut tracker = ModeTracker::default();
    let intent = ModeIntent {
        spectator: true,
        ..Default::default()
    };
    let grounded = ModeObservation {
        on_ground: true,
        ..airborne()
    };
    for observed in [grounded, pressed(grounded), pressed(grounded), airborne()] {
        assert_eq!(pick(&mut tracker, intent, observed), MovementMode::Flying);
    }
    assert_eq!(
        pick(&mut tracker, ModeIntent::default(), grounded),
        MovementMode::Walking
    );
}

#[test]
fn spectator_forward_double_tap_starts_and_retains_sprint_without_terrain() {
    let mut tracker = ModeTracker::default();
    let intent = ModeIntent {
        spectator: true,
        ..Default::default()
    };
    for (forward, sprinting) in [
        (1.0, false),
        (0.0, false),
        (1.0, true),
        (1.0, true),
        (0.0, false),
    ] {
        let observed = ModeObservation {
            move_forward: forward,
            ..airborne()
        };
        let choice = tracker.select(intent, observed, &Unavailable).unwrap();
        assert_eq!(choice.mode, MovementMode::Flying);
        assert_eq!(choice.sprinting, sprinting);
        tracker.record_controls(
            sim::MovementInput {
                forward: f64::from(forward),
                mode: choice.mode,
                ..Default::default()
            },
            false,
        );
    }
}

fn pick(tracker: &mut ModeTracker, intent: ModeIntent, observed: ModeObservation) -> MovementMode {
    tracker
        .select(intent, observed, &Ceiling(None))
        .unwrap()
        .mode
}

fn pressed(observed: ModeObservation) -> ModeObservation {
    ModeObservation {
        jump_edge: true,
        jump_down: true,
        ..observed
    }
}

/// Two jump presses on consecutive ticks: the flight double-tap.
fn double_tap(
    tracker: &mut ModeTracker,
    intent: ModeIntent,
    observed: ModeObservation,
) -> MovementMode {
    pick(tracker, intent, pressed(observed));
    pick(tracker, intent, pressed(observed))
}

/// Unavailable world queries make mode selection retryable.
struct Unavailable;

impl CollisionWorld for Unavailable {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Err(WorldQueryError::InvalidBounds)
    }
    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Err(WorldQueryError::InvalidBounds)
    }
}

#[test]
fn review_failed_mode_selection_retains_the_server_flight_clear() {
    let mut tracker = ModeTracker {
        mode: MovementMode::Flying,
        last_flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
        ..ModeTracker::default()
    };
    let intent = ModeIntent {
        can_fly: true,
        ..Default::default()
    };
    assert!(tracker.select(intent, airborne(), &Unavailable).is_err());
    assert_eq!(
        pick(&mut tracker, intent, airborne()),
        MovementMode::Walking
    );
}

#[test]
fn review_riding_does_not_query_swimming_or_pose_fit() {
    let mut tracker = ModeTracker::default();
    let intent = ModeIntent {
        ride: Some(RideKind::Boat),
        ..Default::default()
    };
    let observed = ModeObservation {
        in_water: true,
        sprinting: true,
        ..airborne()
    };
    assert_eq!(
        tracker.select(intent, observed, &Unavailable).unwrap().mode,
        MovementMode::Riding
    );
}

/// Vanilla keeps flying on the ground; only a double-tap or an ability update ends it.
#[test]
fn flight_toggles_on_and_off_and_survives_the_ground() {
    let mut tracker = ModeTracker::default();
    let intent = ModeIntent {
        can_fly: true,
        ..ModeIntent::default()
    };
    assert_eq!(
        pick(&mut tracker, intent, airborne()),
        MovementMode::Walking
    );
    assert_eq!(
        double_tap(&mut tracker, intent, airborne()),
        MovementMode::Flying
    );
    assert_eq!(pick(&mut tracker, intent, airborne()), MovementMode::Flying);
    let landed = ModeObservation {
        on_ground: true,
        ..airborne()
    };
    assert_eq!(pick(&mut tracker, intent, landed), MovementMode::Flying);
    assert_eq!(
        double_tap(&mut tracker, intent, landed),
        MovementMode::Walking
    );
    assert_eq!(
        double_tap(&mut tracker, intent, airborne()),
        MovementMode::Flying
    );
}

/// A server resync that repeats the previous flying value still overwrites local flight.
#[test]
fn every_ability_update_overwrites_local_flight() {
    let mut tracker = ModeTracker::default();
    let update = |sequence, server_flying| ModeIntent {
        can_fly: true,
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(server_flying),
        ability_sequence: Some(sequence),
        ..ModeIntent::default()
    };
    assert_eq!(
        pick(&mut tracker, update(1, true), airborne()),
        MovementMode::Flying
    );
    assert_eq!(
        double_tap(&mut tracker, update(1, true), airborne()),
        MovementMode::Walking
    );
    assert_eq!(
        pick(&mut tracker, update(1, true), airborne()),
        MovementMode::Walking
    );
    assert_eq!(
        pick(&mut tracker, update(2, true), airborne()),
        MovementMode::Flying
    );
    assert_eq!(
        pick(&mut tracker, update(2, true), pressed(airborne())),
        MovementMode::Flying
    );
    // The update lands first, so the second press starts flight instead of stopping it.
    assert_eq!(
        pick(&mut tracker, update(3, false), pressed(airborne())),
        MovementMode::Flying
    );
    assert_eq!(
        pick(&mut tracker, update(4, false), airborne()),
        MovementMode::Walking
    );
}

#[test]
fn server_set_flight_starts_on_its_edge_and_survives_the_ground() {
    let mut tracker = ModeTracker::default();
    let server = ModeIntent {
        can_fly: true,
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
        ..ModeIntent::default()
    };
    let landed = ModeObservation {
        on_ground: true,
        ..airborne()
    };
    assert_eq!(pick(&mut tracker, server, landed), MovementMode::Flying);
    assert_eq!(pick(&mut tracker, server, landed), MovementMode::Flying);
    assert_eq!(
        double_tap(&mut tracker, server, landed),
        MovementMode::Walking
    );
    assert_eq!(pick(&mut tracker, server, landed), MovementMode::Walking);
}

#[test]
fn mounting_overrides_every_other_mode_and_dismounting_resumes_walking() {
    let mut tracker = ModeTracker::default();
    let mounted = ModeIntent {
        ride: Some(RideKind::Boat),
        can_fly: true,
        elytra_ready: true,
        ..ModeIntent::default()
    };
    assert_eq!(
        pick(&mut tracker, mounted, airborne()),
        MovementMode::Riding
    );
    assert_eq!(
        pick(&mut tracker, ModeIntent::default(), airborne()),
        MovementMode::Walking
    );
}

#[test]
fn rides_classify_by_native_facts() {
    for (id, kind) in [
        ("minecraft:boat", RideKind::Boat),
        ("minecraft:chest_boat", RideKind::Boat),
        ("minecraft:minecart", RideKind::Minecart),
        ("minecraft:hopper_minecart", RideKind::Minecart),
        ("minecraft:horse", RideKind::Horse),
        ("minecraft:skeleton_horse", RideKind::Horse),
        ("minecraft:mule", RideKind::Horse),
        ("minecraft:pig", RideKind::Other),
        ("minecraft:strider", RideKind::Other),
    ] {
        assert_eq!(
            RideKind::from_native_class(
                assets::facts::entity::EntityFacts::pinned()
                    .get(id)
                    .native_class
            ),
            kind,
            "{id}"
        );
    }
}

/// A registered custom mount inherits steering from its pinned native base.
#[test]
fn registered_native_base_controls_mount_class() {
    use protocol::{
        ActorEvent, ActorIdentifier, ActorIdentifierRegistry, ActorKind, ActorSpawnEvent,
        WorldEvent,
    };
    use std::sync::Arc;
    for (base, expected) in [
        ("minecraft:boat", RideKind::Boat),
        ("minecraft:horse", RideKind::Horse),
        ("minecraft:minecart", RideKind::Minecart),
    ] {
        let class_key = assets::facts::entity::EntityFacts::pinned()
            .get(base)
            .class_key;
        assert_ne!(class_key, 0);
        let identifier: Arc<str> = format!("fixture:{class_key}").into();
        let mut authority = client_world::WorldAuthority::new(
            protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            Arc::new(assets::RuntimeAssets::diagnostic()),
            None,
            [0.; 3],
            None,
        );
        authority
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Identifiers(ActorIdentifierRegistry {
                    entries: Arc::from([ActorIdentifier {
                        identifier: identifier.clone(),
                        base_identifier: base.into(),
                    }]),
                    skipped: 0,
                })),
                Some(1),
            )
            .unwrap();
        authority
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: 2,
                    runtime_id: 2,
                    kind: ActorKind::Entity { identifier },
                    position: [0.; 3],
                    velocity: [0.; 3],
                    pitch: 0.,
                    yaw: 0.,
                    head_yaw: 0.,
                    body_yaw: 0.,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
                Some(2),
            )
            .unwrap();
        assert_eq!(
            RideKind::from_native_class(authority.actor(2).unwrap().entity_fact.native_class),
            expected
        );
    }
}

#[test]
fn losing_flight_permission_ends_flight() {
    let mut tracker = ModeTracker::default();
    let can = ModeIntent {
        can_fly: true,
        ..ModeIntent::default()
    };
    double_tap(&mut tracker, can, airborne());
    assert_eq!(
        pick(&mut tracker, ModeIntent::default(), airborne()),
        MovementMode::Walking
    );
}

#[test]
fn elytra_deploys_on_a_jump_press_and_stops_on_landing_or_water() {
    let mut tracker = ModeTracker::default();
    let intent = ModeIntent {
        elytra_ready: true,
        ..ModeIntent::default()
    };
    assert_eq!(
        pick(&mut tracker, intent, airborne()),
        MovementMode::Walking
    );
    assert_eq!(
        pick(&mut tracker, intent, pressed(airborne())),
        MovementMode::Gliding
    );
    assert_eq!(
        pick(&mut tracker, intent, airborne()),
        MovementMode::Gliding
    );
    let wet = ModeObservation {
        in_water: true,
        ..airborne()
    };
    assert_eq!(
        tracker.select(intent, wet, &Pool(20.0)).unwrap().mode,
        MovementMode::Walking
    );

    pick(&mut tracker, intent, pressed(airborne()));
    let landed = ModeObservation {
        on_ground: true,
        ..airborne()
    };
    assert_eq!(pick(&mut tracker, intent, landed), MovementMode::Walking);
}

/// Unlike water, lava neither prevents nor ends a glide.
#[test]
fn lava_does_not_end_a_glide() {
    let mut tracker = ModeTracker::default();
    let intent = ModeIntent {
        elytra_ready: true,
        ..ModeIntent::default()
    };
    let lava = Filled(sim::BlockPhysicsFlags::LAVA);
    for observed in [pressed(airborne()), airborne()] {
        assert_eq!(
            tracker.select(intent, observed, &lava).unwrap().mode,
            MovementMode::Gliding
        );
    }
}

#[test]
fn unequipped_jump_never_glides() {
    let mut tracker = ModeTracker::default();
    assert_eq!(
        pick(&mut tracker, ModeIntent::default(), pressed(airborne())),
        MovementMode::Walking
    );
}

/// A second press ends the glide only once it has lasted eleven ticks.
#[test]
fn a_fresh_jump_press_cancels_a_glide_after_eleven_ticks() {
    let mut tracker = ModeTracker::default();
    let intent = ModeIntent {
        elytra_ready: true,
        ..ModeIntent::default()
    };
    assert_eq!(
        pick(&mut tracker, intent, pressed(airborne())),
        MovementMode::Gliding
    );
    for _ in 0..9 {
        assert_eq!(
            pick(&mut tracker, intent, airborne()),
            MovementMode::Gliding
        );
    }
    assert_eq!(
        pick(&mut tracker, intent, pressed(airborne())),
        MovementMode::Gliding
    );
    assert_eq!(
        pick(&mut tracker, intent, pressed(airborne())),
        MovementMode::Walking
    );
}

/// A server-ended glide never carries its tick count into the next glide.
#[test]
fn an_ended_glide_restarts_the_cancel_window() {
    let mut tracker = ModeTracker::default();
    let intent = ModeIntent {
        elytra_ready: true,
        ..ModeIntent::default()
    };
    pick(&mut tracker, intent, pressed(airborne()));
    for _ in 0..12 {
        pick(&mut tracker, intent, airborne());
    }
    tracker.end(MovementMode::Gliding);
    assert_eq!(
        pick(&mut tracker, intent, pressed(airborne())),
        MovementMode::Gliding
    );
    assert_eq!(
        pick(&mut tracker, intent, pressed(airborne())),
        MovementMode::Gliding,
        "a press one tick into the new glide cannot cancel it"
    );
}

/// Climbable feet end a glide; powder snow does only for a wearer who can stand on it.
#[test]
fn climbable_feet_end_a_glide() {
    let intent = ModeIntent {
        elytra_ready: true,
        ..ModeIntent::default()
    };
    let glide = |intent, world: &Filled| {
        let mut tracker = ModeTracker::default();
        tracker
            .select(intent, pressed(airborne()), world)
            .unwrap()
            .mode
    };
    let ladder = Filled(sim::BlockPhysicsFlags::CLIMBABLE);
    assert_eq!(glide(intent, &ladder), MovementMode::Walking);
    let snow = Filled(sim::BlockPhysicsFlags::POWDER_SNOW);
    assert_eq!(glide(intent, &snow), MovementMode::Gliding);
    let booted = ModeIntent {
        can_stand_on_snow: true,
        ..intent
    };
    assert_eq!(glide(booted, &snow), MovementMode::Walking);
}

/// A second press within six ticks of the first toggles flight.
#[test]
fn flight_double_tap_window_lasts_seven_ticks() {
    let intent = ModeIntent {
        can_fly: true,
        ..ModeIntent::default()
    };
    for (idle_ticks, toggles) in [(5, true), (6, false)] {
        let mut tracker = ModeTracker::default();
        assert_eq!(
            pick(&mut tracker, intent, pressed(airborne())),
            MovementMode::Walking
        );
        for _ in 0..idle_ticks {
            pick(&mut tracker, intent, airborne());
        }
        let mode = pick(&mut tracker, intent, pressed(airborne()));
        assert_eq!(mode == MovementMode::Flying, toggles, "{idle_ticks}");
    }
}

#[test]
fn sprint_starts_swimming_and_continuation_survives_sprint_clear() {
    let mut tracker = ModeTracker::default();
    let swim = ModeObservation {
        in_water: true,
        sprinting: true,
        move_forward: 1.0,
        ..airborne()
    };
    let intent = ModeIntent::default();
    let deep = Pool(20.0);
    assert_eq!(
        tracker.select(intent, swim, &deep).unwrap().mode,
        MovementMode::Swimming
    );
    let stopped = ModeObservation {
        sprinting: false,
        ..swim
    };
    assert_eq!(
        tracker.select(intent, stopped, &deep).unwrap().mode,
        MovementMode::Swimming
    );
    let idle = ModeObservation {
        move_forward: 0.0,
        ..stopped
    };
    assert_eq!(
        tracker.select(intent, idle, &deep).unwrap().mode,
        MovementMode::Walking
    );
}

#[test]
fn a_swimmer_retains_its_horizontal_pose_until_standing_fits() {
    let mut tracker = ModeTracker {
        mode: MovementMode::Swimming,
        ..ModeTracker::default()
    };
    let observed = ModeObservation {
        on_ground: true,
        feet: Vec3::new(0.0, 0.0, 0.0),
        ..airborne()
    };
    let low = tracker
        .select(ModeIntent::default(), observed, &Ceiling(Some(1.0)))
        .unwrap();
    assert_eq!(low.mode, MovementMode::Swimming);
    let open = tracker
        .select(ModeIntent::default(), observed, &Ceiling(None))
        .unwrap();
    assert_eq!(open.mode, MovementMode::Walking);
    assert!(!open.forced_sneak);
}

#[test]
fn closing_a_low_ceiling_starts_crawling_from_standing() {
    let mut tracker = ModeTracker::default();
    let observed = ModeObservation {
        on_ground: true,
        feet: Vec3::new(0.0, 0.0, 0.0),
        ..airborne()
    };
    let choice = tracker
        .select(ModeIntent::default(), observed, &Ceiling(Some(1.0)))
        .unwrap();
    assert_eq!(choice.mode, MovementMode::Crawling);
    assert!(!choice.forced_sneak);
}

#[test]
fn a_gap_that_fits_a_sneak_but_not_a_stand_forces_the_sneak_pose() {
    let mut tracker = ModeTracker::default();
    let observed = ModeObservation {
        on_ground: true,
        feet: Vec3::new(0.0, 0.0, 0.0),
        ..airborne()
    };
    let choice = tracker
        .select(ModeIntent::default(), observed, &Ceiling(Some(1.6)))
        .unwrap();
    assert_eq!(choice.mode, MovementMode::Walking);
    assert!(choice.forced_sneak);
}

/// Shallow water with the head above the surface never starts a swim; a swimmer keeps going.
#[test]
fn a_swim_starts_only_with_the_head_in_water() {
    let sprinting = ModeObservation {
        in_water: true,
        sprinting: true,
        move_forward: 1.0,
        ..airborne()
    };
    let shallow = Pool(10.9);
    let mut walker = ModeTracker::default();
    let choice = walker
        .select(ModeIntent::default(), sprinting, &shallow)
        .unwrap();
    assert_eq!(choice.mode, MovementMode::Walking);

    let mut swimmer = ModeTracker {
        mode: MovementMode::Swimming,
        ..ModeTracker::default()
    };
    let choice = swimmer
        .select(ModeIntent::default(), sprinting, &shallow)
        .unwrap();
    assert_eq!(choice.mode, MovementMode::Swimming);
}
