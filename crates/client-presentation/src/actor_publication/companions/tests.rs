use super::*;

struct Terrain(Vec<sim::Aabb>);
impl CollisionWorld for Terrain {
    fn collision_boxes(
        &self,
        query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(
            self.0
                .iter()
                .copied()
                .filter(|bounds| bounds.intersects(query))
                .collect(),
        ))
    }
}

/// Makes a collider from world coordinates without consulting any game assets.
fn bounds(min: [f64; 3], max: [f64; 3]) -> sim::Aabb {
    sim::Aabb::new(
        sim::Vec3::new(min[0], min[1], min[2]),
        sim::Vec3::new(max[0], max[1], max[2]),
    )
}

#[test]
fn perspective_resets_keep_the_independent_follow_history() {
    let item = hats::fixture::ground_companion();
    let rig = hats::rig(item).companion().unwrap();
    let mut input = render_model::ActorRigRenderInput {
        identity: render_model::ActorRenderIdentity {
            session_id: 1,
            dimension: 0,
            runtime_id: 7,
            spawn_revision: 1,
            ingress_sequence: 1,
            source_tick: Some(1),
            movement_revision: 1,
            pose_generation: render_api::PoseGeneration::new(1),
            layer: 0,
        },
        rig: render_model::EntityRigId(0),
        previous_bones: Arc::from([]),
        current_bones: Arc::from([]),
        completed_tick: 1,
        reset_generation: 1,
    };
    let mut actual = State::default();
    let mut control = State::default();
    for state in [&mut actual, &mut control] {
        state.retain_owner(&input);
        state
            .follower
            .advance(
                rig,
                Vec3::ZERO,
                0.0,
                0.05,
                |_, _, _| Some(0.0),
                |_, _, delta| delta,
                |_, _| true,
            )
            .unwrap();
        state
            .follower
            .advance(
                rig,
                Vec3::new(0.0, 0.0, 4.0),
                0.0,
                0.1,
                |_, _, _| Some(0.0),
                |_, _, delta| delta,
                |_, _| true,
            )
            .unwrap();
    }
    input.reset_generation = 2;
    actual.retain_owner(&input);
    let advance = |state: &mut State| {
        state
            .follower
            .advance(
                rig,
                Vec3::new(0.0, 0.0, 4.0),
                0.0,
                0.05,
                |_, _, _| Some(0.0),
                |_, _, delta| delta,
                |_, _| true,
            )
            .unwrap()
    };
    let after = advance(&mut actual);
    let expected = advance(&mut control);
    assert_eq!(after.feet, expected.feet);
    assert_eq!(after.pose.seconds, expected.pose.seconds);
    input.identity.session_id = 2;
    actual.retain_owner(&input);
    assert!(advance(&mut actual).feet.distance(expected.feet) > 1.0);
}

#[test]
fn an_airborne_diagonal_cannot_enter_an_overhang_corner() {
    let obstacle = bounds([0.0, 1.0, 0.0], [1.0, 2.0, 1.0]);
    let terrain = Terrain(vec![bounds([-5.0, -1.0, -5.0], [5.0, 0.0, 5.0]), obstacle]);
    let feet = Vec3::new(-0.30, 0.70, -0.30);
    let delta = Vec3::new(0.08, 0.0, 0.08);
    let clipped = clip_motion(&terrain, feet, delta);
    assert!(clipped.distance(delta) > 0.001);
    let end = feet + clipped;
    let body = bounds(
        [
            f64::from(end.x - RADIUS),
            f64::from(end.y),
            f64::from(end.z - RADIUS),
        ],
        [
            f64::from(end.x + RADIUS),
            f64::from(end.y + HEIGHT),
            f64::from(end.z + RADIUS),
        ],
    );
    assert!(!body.intersects(obstacle));
}

#[test]
fn a_protruding_muzzle_hops_before_entering_an_uphill_riser() {
    let obstacle = bounds([-5.0, 0.0, 0.0], [5.0, 1.0, 5.0]);
    let terrain = Terrain(vec![bounds([-5.0, -1.0, -5.0], [5.0, 0.0, 5.0]), obstacle]);
    let head = hats::companion::Clearance {
        centre: [0.0, 0.0],
        radius: 0.65,
        bottom: 0.35,
        top: 0.8,
    };
    let feet = Vec3::new(0.0, 0.0, -0.7);
    let movement = Vec3::new(0.0, 0.0, 0.15);
    assert_eq!(
        clip_motion(&terrain, feet, movement),
        movement,
        "the support body alone misses the protruding muzzle"
    );
    assert!(clip_body_motion(&terrain, feet, 0.0, movement, &[head]).z < movement.z);
    let item = hats::fixture::ground_companion();
    let rig = hats::rig(item).companion().unwrap();
    let mut follower = hats::companion::Follower::default();
    let mut step = |owner| {
        follower
            .advance(
                rig,
                owner,
                0.0,
                0.05,
                |feet, rise, drop| floor(&terrain, feet, rise, drop),
                |feet, _, delta| clip_body_motion(&terrain, feet, 0.0, delta, &[head]),
                |feet, _| clear_body(&terrain, feet, 0.0, &[head]),
            )
            .unwrap()
    };
    step(Vec3::ZERO);
    let mut last = None;
    let mut hopped_early = false;
    for _ in 0..150 {
        let frame = step(Vec3::new(0.0, 1.0, 4.0));
        let p = frame.feet;
        // Sweeps use f32 world positions; discard sub-millimetre contact rounding.
        let radius = head.radius - 0.0001;
        let muzzle = bounds(
            [
                f64::from(p.x - radius),
                f64::from(p.y + head.bottom + 0.0001),
                f64::from(p.z - radius),
            ],
            [
                f64::from(p.x + radius),
                f64::from(p.y + head.top - 0.0001),
                f64::from(p.z + radius),
            ],
        );
        assert!(
            !muzzle.intersects(obstacle),
            "muzzle entered the uphill block at {p:?}"
        );
        hopped_early |= p.y > 0.1 && p.z < -RADIUS;
        last = Some(frame);
    }
    let last = last.unwrap();
    assert!(
        hopped_early,
        "the hop must start before the support footprint reaches the riser"
    );
    assert!(
        last.feet.z > 1.0,
        "the wider head must not leave the follower stuck"
    );
    assert!((last.feet.y - 1.0).abs() < 0.001);
}

#[test]
fn a_slab_hop_under_a_low_ceiling_never_intersects_the_ceiling() {
    let terrain = Terrain(vec![
        bounds([-5.0, -1.0, -5.0], [5.0, 0.0, 5.0]),
        bounds([-5.0, 0.0, 0.0], [5.0, 0.5, 5.0]),
        bounds([-5.0, 1.5, -5.0], [5.0, 2.0, 5.0]),
    ]);
    let head = hats::companion::Clearance {
        centre: [0.0, 0.0],
        radius: 0.65,
        bottom: 0.35,
        top: 0.8,
    };
    let item = hats::fixture::ground_companion();
    let rig = hats::rig(item).companion().unwrap();
    let mut follower = hats::companion::Follower::default();
    let mut step = |owner| {
        follower
            .advance(
                rig,
                owner,
                0.0,
                0.05,
                |feet, rise, drop| floor(&terrain, feet, rise, drop),
                |feet, _, delta| clip_body_motion(&terrain, feet, 0.0, delta, &[head]),
                |feet, _| clear_body(&terrain, feet, 0.0, &[head]),
            )
            .unwrap()
    };
    step(Vec3::ZERO);
    let mut last = None;
    for _ in 0..150 {
        let frame = step(Vec3::new(0.0, 0.5, 4.0));
        assert!(
            frame.feet.y + HEIGHT <= 1.501,
            "the airborne body must stay below the ceiling"
        );
        last = Some(frame);
    }
    let last = last.unwrap();
    assert!(last.feet.z > 1.0);
    assert!((last.feet.y - 0.5).abs() < 0.001);
}

#[test]
fn descending_a_ledge_keeps_the_whole_body_clear_and_settles_on_its_feet() {
    let terrain = Terrain(vec![
        bounds([-5.0, -1.0, -5.0], [5.0, 0.0, 5.0]),
        bounds([-5.0, 0.0, -5.0], [5.0, 1.0, 0.0]),
    ]);
    let head = hats::companion::Clearance {
        centre: [0.0, 0.0],
        radius: 0.65,
        bottom: 0.35,
        top: 0.8,
    };
    let rig = hats::rig(hats::fixture::ground_companion())
        .companion()
        .unwrap();
    let mut follower = hats::companion::Follower::default();
    advance_case(
        &mut follower,
        rig,
        &terrain,
        Vec3::new(0.0, 1.0, 0.0),
        &[head],
    )
    .unwrap();
    let mut previous = follower.position().unwrap();
    for _ in 0..180 {
        let frame = advance_case(
            &mut follower,
            rig,
            &terrain,
            Vec3::new(0.0, 0.0, 4.0),
            &[head],
        )
        .unwrap();
        assert!(frame.feet.distance(previous) < 0.65);
        assert!(clear_body(&terrain, frame.feet, frame.yaw, &[head]));
        previous = frame.feet;
    }
    assert!(
        previous.y.abs() < 0.001,
        "feet did not settle on the lower ground: {previous:?}"
    );
    assert!(previous.z > 1.0);
}

/// Runs one real support/sweep/clearance step against a synthetic world.
fn advance_case(
    follower: &mut hats::companion::Follower,
    rig: &hats::companion::Rig,
    terrain: &Terrain,
    owner: Vec3,
    sections: &[hats::companion::Clearance],
) -> Option<hats::companion::Frame> {
    follower.advance(
        rig,
        owner,
        0.0,
        0.05,
        |feet, rise, drop| floor(terrain, feet, rise, drop),
        |feet, yaw, delta| clip_body_motion(terrain, feet, yaw, delta, sections),
        |feet, yaw| clear_body(terrain, feet, yaw, sections),
    )
}

#[test]
fn a_block_added_over_the_muzzle_settles_on_support_with_a_stationary_owner() {
    let mut terrain = Terrain(vec![bounds([-20.0, -1.0, -20.0], [20.0, 0.0, 20.0])]);
    let head = hats::companion::Clearance {
        centre: [0.0, 0.0],
        radius: 0.65,
        bottom: 0.35,
        top: 0.8,
    };
    let rig = hats::rig(hats::fixture::ground_companion())
        .companion()
        .unwrap();
    let mut follower = hats::companion::Follower::default();
    let first = advance_case(&mut follower, rig, &terrain, Vec3::ZERO, &[head]).unwrap();
    terrain.0.push(bounds([1.0, 0.0, -1.0], [2.0, 1.0, 0.0]));
    assert!(!clear_body(&terrain, first.feet, 0.0, &[head]));
    let mut previous = first.feet;
    let mut visible_again = false;
    for _ in 0..120 {
        if let Some(frame) = advance_case(&mut follower, rig, &terrain, Vec3::ZERO, &[head]) {
            assert!(clear_body(&terrain, frame.feet, frame.yaw, &[head]));
            visible_again = true;
        }
        let next = follower.position().unwrap();
        assert!(
            next.distance(previous) < 0.65,
            "recovery must not snap position"
        );
        previous = next;
    }
    assert!(
        visible_again,
        "an ordinary reachable escape remained hidden forever"
    );
    let support = floor(&terrain, previous, 0.05, 0.1).unwrap();
    assert!(
        (support - previous.y).abs() < 0.01,
        "unsupported recovery: {previous:?}"
    );
}

#[test]
fn a_cached_destination_is_replaced_when_a_block_obstructs_it() {
    let mut terrain = Terrain(vec![bounds([-20.0, -1.0, -20.0], [20.0, 0.0, 20.0])]);
    let head = hats::companion::Clearance {
        centre: [0.0, 0.0],
        radius: 0.65,
        bottom: 0.35,
        top: 0.8,
    };
    let rig = hats::rig(hats::fixture::ground_companion())
        .companion()
        .unwrap();
    let mut follower = hats::companion::Follower::default();
    advance_case(&mut follower, rig, &terrain, Vec3::ZERO, &[head]).unwrap();
    let owner = Vec3::new(0.0, 0.0, 4.0);
    let first = advance_case(&mut follower, rig, &terrain, owner, &[head]).unwrap();
    terrain.0.push(bounds([1.0, 0.0, 2.0], [2.0, 3.0, 3.0]));
    assert!(clear_body(&terrain, first.feet, first.yaw, &[head]));
    let mut previous = first.feet;
    for _ in 0..160 {
        let frame = advance_case(&mut follower, rig, &terrain, owner, &[head]).unwrap();
        assert!(clear_body(&terrain, frame.feet, frame.yaw, &[head]));
        assert!(frame.feet.distance(previous) < 0.65);
        previous = frame.feet;
    }
    assert!(
        previous.z > 1.5 && previous.x + head.radius < 1.0,
        "did not reach the clear alternate: {previous:?}"
    );
    assert!((floor(&terrain, previous, 0.05, 0.1).unwrap() - previous.y).abs() < 0.01);
}

#[test]
fn removing_foot_support_cannot_suspend_the_companion_on_its_muzzle() {
    let mut terrain = Terrain(vec![
        bounds([-20.0, -1.0, -20.0], [20.0, 0.0, 20.0]),
        bounds([-20.0, 0.0, -20.0], [20.0, 1.0, 0.0]),
        bounds([1.0, 0.0, 0.0], [2.0, 1.0, 1.0]),
    ]);
    let head = hats::companion::Clearance {
        centre: [0.0, 0.0],
        radius: 0.65,
        bottom: 0.35,
        top: 0.8,
    };
    let rig = hats::rig(hats::fixture::ground_companion())
        .companion()
        .unwrap();
    let mut follower = hats::companion::Follower::default();
    let owner = Vec3::new(0.0, 1.0, 1.85);
    let mut previous = advance_case(&mut follower, rig, &terrain, owner, &[head])
        .unwrap()
        .feet;
    assert_eq!(previous.y, 1.0);
    terrain.0.pop();
    assert!(clear_body(&terrain, previous, 0.0, &[head]));
    for _ in 0..160 {
        let frame = advance_case(&mut follower, rig, &terrain, owner, &[head]).unwrap();
        assert!(clear_body(&terrain, frame.feet, frame.yaw, &[head]));
        assert!(frame.feet.distance(previous) < 0.65);
        previous = frame.feet;
    }
    assert!(
        previous.y.abs() < 0.01,
        "feet stayed above the lower floor: {previous:?}"
    );
    assert!((floor(&terrain, previous, 0.05, 0.1).unwrap() - previous.y).abs() < 0.01);
}

#[test]
fn a_block_added_during_a_hop_cannot_leave_an_embedded_published_body() {
    let mut terrain = Terrain(vec![
        bounds([-5.0, -1.0, -5.0], [5.0, 0.0, 5.0]),
        bounds([-5.0, 0.0, 0.0], [5.0, 1.0, 5.0]),
    ]);
    let head = hats::companion::Clearance {
        centre: [0.0, 0.0],
        radius: 0.65,
        bottom: 0.35,
        top: 0.8,
    };
    let rig = hats::rig(hats::fixture::ground_companion())
        .companion()
        .unwrap();
    let mut follower = hats::companion::Follower::default();
    advance_case(&mut follower, rig, &terrain, Vec3::ZERO, &[head]).unwrap();
    let mut added = false;
    let mut visible_again = false;
    let mut previous = follower.position().unwrap();
    for _ in 0..180 {
        if let Some(frame) = advance_case(
            &mut follower,
            rig,
            &terrain,
            Vec3::new(0.0, 1.0, 4.0),
            &[head],
        ) {
            assert!(clear_body(&terrain, frame.feet, frame.yaw, &[head]));
            if added {
                visible_again = true;
            }
        }
        let next = follower.position().unwrap();
        assert!(next.distance(previous) < 0.65);
        previous = next;
        if !added && next.y > 0.2 {
            terrain.0.push(bounds(
                [
                    f64::from(next.x - 0.3),
                    f64::from(next.y - 0.2),
                    f64::from(next.z - 0.3),
                ],
                [
                    f64::from(next.x + 0.7),
                    f64::from(next.y + 0.8),
                    f64::from(next.z + 0.7),
                ],
            ));
            added = true;
        }
    }
    assert!(added && visible_again);
}

#[test]
fn initialization_and_owner_teleports_require_muzzle_clearance() {
    let terrain = Terrain(vec![
        bounds([-20.0, -1.0, -20.0], [20.0, 0.0, 20.0]),
        bounds([-5.0, 0.0, -1.1], [5.0, 1.0, 5.0]),
    ]);
    let head = hats::companion::Clearance {
        centre: [0.0, 0.0],
        radius: 0.65,
        bottom: 0.35,
        top: 0.8,
    };
    let item = hats::fixture::ground_companion();
    let rig = hats::rig(item).companion().unwrap();
    let mut follower = hats::companion::Follower::default();
    let target = Vec3::new(1.05, 0.0, -1.5);
    assert!(floor(&terrain, target, 1.0, 3.0).is_some());
    assert!(!clear_body(&terrain, target, 0.0, &[head]));
    for owner in [
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -4.0),
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -15.0),
        Vec3::ZERO,
    ] {
        for _ in 0..60 {
            let frame = follower
                .advance(
                    rig,
                    owner,
                    0.0,
                    0.05,
                    |feet, rise, drop| floor(&terrain, feet, rise, drop),
                    |_, _, _| Vec3::ZERO,
                    |feet, _| clear_body(&terrain, feet, 0.0, &[head]),
                )
                .unwrap();
            assert!(
                clear_body(&terrain, frame.feet, 0.0, &[head]),
                "reset/recovery embedded the muzzle"
            );
        }
    }
}

#[test]
fn companion_lighting_uses_its_own_endpoints_in_both_perspectives() {
    let placement = [
        [1.0, 0.0, 0.0, 8.0],
        [0.0, 1.0, 0.0, 20.0],
        [0.0, 0.0, 1.0, 3.0],
    ];
    let mut sampled = Vec::new();
    let hidden = lighting(placement, |position| {
        sampled.push(position);
        Some((0, 0))
    });
    let visible = lighting(placement, |_| Some((0, 0)));
    assert_eq!(hidden, render_model::pack_actor_light(0, 0));
    assert_eq!(hidden, visible);
    assert_eq!(sampled.len(), 2);
    assert!(
        sampled
            .iter()
            .all(|p| p[0] == 8.0 && p[2] == 3.0 && p[1] >= 20.0 && p[1] < 21.0)
    );
}

#[test]
fn support_uses_actual_slab_height_and_a_step_is_found_before_intersection() {
    let terrain = Terrain(vec![
        bounds([-3.0, -1.0, -3.0], [3.0, 0.0, 0.0]),
        bounds([-3.0, 0.0, 0.0], [3.0, 0.5, 3.0]),
    ]);
    assert_eq!(
        floor(&terrain, Vec3::new(0.0, 0.0, -0.5), 1.0, 3.0),
        Some(0.0)
    );
    assert_eq!(
        floor(&terrain, Vec3::new(0.0, 0.0, -0.1), 1.0, 3.0),
        Some(0.5)
    );
    assert_eq!(
        floor(&terrain, Vec3::new(0.0, 0.5, 1.0), 0.05, 3.0),
        Some(0.5)
    );
}

#[test]
fn a_low_ceiling_and_tall_wall_are_not_walkable_support() {
    let floor_box = bounds([-3.0, -1.0, -3.0], [3.0, 0.0, 3.0]);
    let ceiling = bounds([-3.0, 0.6, -3.0], [3.0, 1.0, 3.0]);
    assert_eq!(
        floor(&Terrain(vec![floor_box, ceiling]), Vec3::ZERO, 0.05, 3.0),
        None
    );
    let wall = bounds([-3.0, 0.0, 0.0], [3.0, 3.0, 3.0]);
    assert_eq!(
        floor(
            &Terrain(vec![floor_box, wall]),
            Vec3::new(0.0, 0.0, -0.1),
            1.0,
            3.0
        ),
        None
    );
    assert_eq!(floor(&Terrain(vec![]), Vec3::ZERO, 1.0, 3.0), None);
}

#[derive(serde::Deserialize)]
struct Replay {
    catalog: std::path::PathBuf,
    assets: std::path::PathBuf,
    boxes: Vec<[[f64; 3]; 2]>,
    owners: Vec<[f32; 3]>,
    yaw: f32,
    #[serde(default)]
    headings: Vec<f32>,
    seconds: f32,
}

#[test]
fn external_companion_route_is_continuous_and_settles_after_the_owner_stops() {
    let Some(path) = std::env::var_os("CINNABAR_COMPANION_REPLAY") else {
        eprintln!(
            "missing-fixture: set CINNABAR_COMPANION_REPLAY for an external companion terrain replay"
        );
        return;
    };
    let replay: Replay = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let catalog: cosmetics::Catalog =
        serde_json::from_slice(&std::fs::read(&replay.catalog).unwrap()).unwrap();
    let descriptor = catalog
        .items
        .into_iter()
        .find(|item| item.slot == "pet")
        .unwrap();
    let read = |asset: &cosmetics::Asset| std::fs::read(replay.assets.join(&asset.sha256)).unwrap();
    let geometry = read(&descriptor.assets.geometry);
    let texture = read(&descriptor.assets.texture);
    let animation = read(descriptor.assets.animation.as_ref().unwrap());
    let item = hats::admit(descriptor, &geometry, &texture, &animation).unwrap();
    let rig = hats::rig(item).companion().unwrap();
    let terrain = Terrain(
        replay
            .boxes
            .into_iter()
            .map(|b| bounds(b[0], b[1]))
            .collect(),
    );
    let mut follower = hats::companion::Follower::default();
    let mut previous: Option<Vec3> = None;
    let mut positions = Vec::new();
    for (index, owner) in replay.owners.iter().enumerate() {
        let yaw = replay.headings.get(index).copied().unwrap_or(replay.yaw);
        let owner = Vec3::from_array(*owner);
        let frame = follower
            .advance(
                rig,
                owner,
                yaw,
                replay.seconds,
                |feet, rise, drop| floor(&terrain, feet, rise, drop),
                |feet, yaw, delta| clip_body_motion(&terrain, feet, yaw, delta, rig.clearance()),
                |feet, yaw| clear_body(&terrain, feet, yaw, rig.clearance()),
            )
            .unwrap();
        if let Some(previous) = previous {
            assert!(
                frame.feet.distance(previous) <= 12.0 * replay.seconds + 0.001,
                "discontinuous navigation: {previous:?} -> {:?}",
                frame.feet
            );
        }
        assert!(
            clear_body(&terrain, frame.feet, frame.yaw, rig.clearance()),
            "body enters terrain at {:?}",
            frame.feet
        );
        previous = Some(frame.feet);
        positions.push(frame.feet.to_array());
    }
    if let Some(path) = std::env::var_os("CINNABAR_COMPANION_REPLAY_TRACE") {
        std::fs::write(path, serde_json::to_vec(&positions).unwrap()).unwrap();
    }
    let last = Vec3::from_array(*positions.last().unwrap());
    let owner = Vec3::from_array(*replay.owners.last().unwrap());
    assert!(
        last.distance(owner) < 3.5,
        "companion failed to catch up: {last:?}, owner {owner:?}"
    );
    for position in positions.iter().rev().take(40) {
        assert!(
            Vec3::from_array(*position).distance(last) < 0.02,
            "companion keeps moving after settling: {position:?} -> {last:?}"
        );
    }
}
