use super::*;

#[test]
fn shulker_pick_does_not_expand_its_interaction_box() {
    let actor =
        super::super::tests::actor(1, "minecraft:shulker", [0.0, 0.0, -2.0], Some((1.0, 1.0)));
    let hit = pick_actor(
        [&actor].into_iter(),
        None,
        [0.0, 0.5, 0.0],
        [0.0, 0.0, -1.0],
        3.0,
    )
    .unwrap();
    assert_eq!(hit.point, [0.0, 0.5, -1.5]);
    assert!(
        pick_actor(
            [&actor].into_iter(),
            None,
            [0.55, 0.5, 0.0],
            [0.0, 0.0, -1.0],
            3.0
        )
        .is_none()
    );
}

#[test]
fn block_occlusion_rounds_the_world_intercept_before_measuring() {
    let origin = [8192.0, 65.62, 4096.0];
    let block = [8194, 66, 4097];
    let relative: [f32; 3] = [0.0, 0.084_676_59, 0.143_673_03];
    let point: [f64; 3] =
        std::array::from_fn(|axis| f64::from(block[axis]) + f64::from(relative[axis]));
    let distance = block_pick_distance(origin, point.map(|axis| axis as f32));
    assert_eq!(distance, f64::from(2.350_242_9_f32));
    let actor = Some(ActorHit {
        runtime_id: 1,
        distance: f64::from(2.250_259_9_f32),
        point: [0.0; 3],
    });
    assert!(matches!(
        classify(actor, Some(distance), 3.0),
        Crosshair::Block
    ));
    let unrounded = (0..3)
        .map(|axis| (point[axis] - f64::from(origin[axis])).powi(2))
        .sum::<f64>()
        .sqrt();
    assert!(matches!(
        classify(actor, Some(unrounded), 3.0),
        Crosshair::Actor(_)
    ));
}

#[test]
fn shallow_axis_does_not_supply_a_pick_face() {
    let hit = pick_box_point(
        [0.0, 1.0, 0.0],
        [0.00003, 1.0, 3.0],
        [0.000015, 0.0, 1.0],
        [1.0, 2.0, 1.6],
    )
    .unwrap();
    // The shallow X face is skipped, so the Z face supplies the click position.
    assert_eq!(hit[2].to_bits(), 0x3fcc_ccce);
    assert!(hit[0] >= 0.000015);
}

#[test]
fn segment_endpoint_rounding_controls_the_world_space_hit() {
    let actor = super::super::tests::actor(1, "minecraft:pig", [0.0; 3], None);
    let origin = [16_384.0, 1.0, 0.0];
    let hit = pick_actor_by(
        [&actor].into_iter(),
        |_| [([16_385.0, 0.0, 0.0], [16_386.0, 2.0, 3.0])],
        None,
        origin,
        [0.6, 0.0, 0.8],
        3.0,
    )
    .unwrap();
    let end_x = origin[0] + 0.6_f32 * 3.0;
    let min_x = 16_385.0_f32 - ACTOR_PICK_RADIUS;
    let fraction = (min_x - origin[0]) / (end_x - origin[0]);
    assert_eq!(hit.point, [min_x, 1.0, (0.8_f32 * 3.0) * fraction]);
    assert_ne!(
        hit.point[2],
        ((f64::from(min_x) - f64::from(origin[0])) * 4.0 / 3.0) as f32
    );
}

#[test]
fn segment_hits_include_endpoints_and_inside_origins() {
    let min = [1.0, 0.0, 0.0];
    let max = [2.0, 2.0, 2.0];
    assert_eq!(
        pick_box_point([0.0, 1.0, 1.0], [1.0, 1.0, 1.0], min, max),
        Some([1.0; 3])
    );
    assert_eq!(pick_box_point([1.0; 3], [3.0; 3], min, max), Some([1.0; 3]));
    assert_eq!(
        pick_box_point([0.0, 1.0, 1.0], [0.9, 1.0, 1.0], min, max),
        None
    );
}

#[test]
fn actor_use_occlusion_ignores_the_cached_double_distance() {
    let origin = [8192.0, 65.62, 4096.0];
    let block = sim::BlockHit {
        block_pos: [8194, 66, 4097],
        face: 4,
        hit_local: sim::Vec3::new(
            0.0,
            f64::from(0.084_676_59_f32),
            f64::from(0.143_673_03_f32),
        ),
        hit_world: sim::Vec3::new(
            8194.0,
            66.0 + f64::from(0.084_676_59_f32),
            4097.0 + f64::from(0.143_673_03_f32),
        ),
        runtime_id: 7,
        distance: 10.0,
        identity: sim::CollisionQuery::synthetic(()).identity,
    };
    let actor = Some(ActorHit {
        runtime_id: 1,
        distance: f64::from(2.250_259_9_f32),
        point: [0.0; 3],
    });
    assert_eq!(
        classify_block_hit(actor, origin, Some(&block), 3.0),
        Crosshair::Block
    );
    assert!(matches!(
        classify(actor, Some(block.distance), 3.0),
        Crosshair::Actor(_)
    ));
}

#[test]
fn block_occlusion_uses_the_intercept_outside_the_block_cell() {
    let origin = [3.0, 0.5, 0.5];
    let block = sim::BlockHit {
        block_pos: [0, 0, 0],
        face: 5,
        hit_local: sim::Vec3::new(1.0, 0.5, 0.5),
        hit_world: sim::Vec3::new(1.5, 0.5, 0.5),
        runtime_id: 7,
        distance: 1.5,
        identity: sim::CollisionQuery::synthetic(()).identity,
    };
    let actor = Some(ActorHit {
        runtime_id: 1,
        distance: 1.7,
        point: [1.3, 0.5, 0.5],
    });
    assert_eq!(
        classify_block_hit(actor, origin, Some(&block), 3.0),
        Crosshair::Block
    );

    let stack = protocol::NetworkItemStack::empty();
    let item =
        protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
    let mut observed = crate::interaction_authority::FrozenBlockObservation::fixture(
        block.block_pos,
        block.face,
        item,
    );
    observed.ray.origin = origin;
    observed.target.relative_hit = [1.0, 0.5, 0.5];
    observed.target.hit_world = [1.5, 0.5, 0.5];
    assert_eq!(observed.hit_distance(), 1.5);
    assert_eq!(
        classify(actor, Some(observed.hit_distance()), 3.0),
        Crosshair::Block
    );
}
