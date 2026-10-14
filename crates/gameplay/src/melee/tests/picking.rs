use super::*;

#[test]
fn ordered_hitboxes_use_the_first_intersection_within_reach() {
    let actor = actor(9, "minecraft:pig", [0.0; 3], None);
    let boxes = [
        ([5.0, 0.0, -2.0], [6.0, 3.0, -1.0]),
        ([-0.5, 0.0, -9.0], [0.5, 3.0, -8.0]),
        ([-0.5, 0.0, -4.0], [0.5, 3.0, -3.0]),
        ([-0.5, 0.0, -2.0], [0.5, 3.0, -1.0]),
    ];
    let hit = pick_actor_by([&actor].into_iter(), |_| boxes, None, EYE, NORTH, 5.0).unwrap();
    assert!((hit.distance - 2.9).abs() < 1e-6, "{}", hit.distance);
}

#[test]
fn advertised_projectile_base_excludes_picking_and_placement_obstruction() {
    let actor = actor_with_base(
        80,
        "fixture:projectile",
        Some("minecraft:arrow"),
        [0.0, 0.0, -2.0],
        HashMap::from([
            (
                BOUNDING_BOX_WIDTH_METADATA_KEY,
                ActorMetadataValue::Float(0.6),
            ),
            (
                BOUNDING_BOX_HEIGHT_METADATA_KEY,
                ActorMetadataValue::Float(2.0),
            ),
        ]),
    );
    assert_eq!(
        pick_actor(std::iter::once(&actor), None, EYE, NORTH, 5.0),
        None
    );
    assert!(!obstructs_placement(&actor));
}

#[test]
fn nearest_pickable_actor_wins_and_reports_the_inflated_entry_point() {
    let near = actor(1, "minecraft:zombie", [0.0, 0.0, -2.0], Some((0.6, 1.95)));
    let far = actor(2, "minecraft:zombie", [0.0, 0.0, -4.0], Some((0.6, 1.95)));
    let drop = actor(3, "minecraft:item", [0.0, 0.0, -1.0], Some((0.25, 0.25)));
    let invalid = actor(4, "minecraft:cow", [0.0, 0.0, -1.5], Some((f32::NAN, 1.95)));
    let actors = [far, drop, invalid, near];
    let hit = pick_actor(actors.iter(), None, EYE, NORTH, 5.7).unwrap();
    assert_eq!(hit.runtime_id, 1);
    // Front face at z = -2 + 0.3, grown by the pick radius.
    assert!((hit.distance - 1.6).abs() < 1e-6, "{}", hit.distance);
    assert!((hit.point[2] + 1.6).abs() < 1e-6);
    // The ridden vehicle is never picked.
    let hit = pick_actor(actors.iter(), Some(10), EYE, NORTH, 5.7).unwrap();
    assert_eq!(hit.runtime_id, 2);
    assert_eq!(pick_actor(actors[..1].iter(), None, EYE, NORTH, 3.0), None);
}

/// A zombie whose only hitbox is a 1×1×1 cube centred 3 blocks above its feet.
fn zombie_with_raised_hitbox(feet: [f32; 3]) -> ActorSnapshot {
    actor_with_raised_hitbox("minecraft:zombie", Some(""), feet)
}

/// An actor, after an optional advertised base, whose only hitbox is a raised unit cube.
fn actor_with_raised_hitbox(identifier: &str, base: Option<&str>, feet: [f32; 3]) -> ActorSnapshot {
    let mut entry = world::NbtCompound::default();
    for (name, value) in [
        ("MinX", -0.5),
        ("MinY", 0.0),
        ("MinZ", -0.5),
        ("MaxX", 0.5),
        ("MaxY", 1.0),
        ("MaxZ", 0.5),
        ("PivotX", 0.0),
        ("PivotY", 3.0),
        ("PivotZ", 0.0),
    ] {
        entry.insert(name, world::NbtValue::Float(value));
    }
    let mut root = world::NbtCompound::default();
    root.insert(
        "Hitboxes",
        world::NbtValue::List(vec![world::NbtValue::Compound(entry)]),
    );
    let bytes = root.encode_root().unwrap();
    actor_with_base(
        5,
        identifier,
        base,
        feet,
        HashMap::from([(
            client_world::HITBOX_METADATA_KEY,
            ActorMetadataValue::Compound(Arc::from(bytes)),
        )]),
    )
}

#[test]
fn custom_hitbox_replaces_the_collision_box_for_picks() {
    let zombie = zombie_with_raised_hitbox([0.0, 0.0, -2.0]);
    // Eye-level ray passes through the collision box but under the raised hitbox.
    assert_eq!(
        pick_actor([&zombie].into_iter(), None, EYE, NORTH, 3.0),
        None
    );
    let raised = [0.0, 3.0, 0.0];
    let hit = pick_actor([&zombie].into_iter(), None, raised, NORTH, 3.0)
        .expect("the ray enters the raised hitbox");
    assert_eq!(hit.runtime_id, 5);
    // Front face at z = -2 + 0.5, grown by the pick radius.
    assert!((hit.distance - 1.4).abs() < 1e-6, "{}", hit.distance);
}

/// Unresolved custom identities keep their server hitboxes pickable.
#[test]
fn unresolved_custom_actor_is_picked_through_its_server_hitbox() {
    for base in [None, Some("example:unadmitted_base")] {
        let npc = actor_with_raised_hitbox("example:hub_npc", base, [0.0, 0.0, -2.0]);
        let hit = pick_actor([&npc].into_iter(), None, [0.0, 3.0, 0.0], NORTH, 3.0)
            .unwrap_or_else(|| panic!("base {base:?}: the ray enters the server hitbox"));
        assert_eq!(hit.runtime_id, 5);
        assert!((hit.distance - 1.4).abs() < 1e-6, "{}", hit.distance);
    }
}

#[test]
fn custom_npc_without_size_metadata_sends_attack_for_picked_world_point() {
    // Spawned without an identifier advertisement, so its interaction class stays unresolved.
    let npc = actor_with_base(
        70,
        "example:game_selection_npc",
        None,
        [10.0, 20.0, 28.0],
        HashMap::new(),
    );
    let origin = [10.0, 21.0, 30.0];
    let hit = pick_actor([npc].iter(), None, origin, NORTH, 3.0)
        .expect("a custom NPC retains its default collision box before size metadata arrives");
    assert_eq!(hit.runtime_id, 70);
    assert_eq!([hit.point[0], hit.point[1]], [origin[0], origin[1]]);
    assert!(hit.point[2] > 28.0 && hit.point[2] < origin[2]);

    let mut runtime = MeleeRuntime::default();
    runtime.observe_input(true, false);
    let mut press = press(PlayerInputMode::Mouse);
    press.player_position = origin;
    let outcome = runtime.resolve(
        classify(Some(hit), None, 3.0),
        &press,
        &mut SwingTracker::default(),
    );
    assert_eq!(
        kinds(&outcome.packets),
        ["AnimatePacket", "InventoryTransactionPacket"]
    );
    assert!(!outcome.missed_swing);
    let observed = observe_transaction(&outcome.packets[1])
        .expect("the picked NPC receives an attack transaction");
    let transaction = observed
        .actor_use()
        .expect("the transaction targets an actor");
    assert_eq!(transaction.action, Some(ActorUseAction::Attack));
    assert_eq!(transaction.runtime_id, hit.runtime_id);
    assert_eq!(transaction.hit_position, hit.point);
    assert_eq!(transaction.from_position, press.player_position);
}

#[test]
fn a_positive_actor_scale_expands_the_pick_box() {
    let mut npc = actor(
        71,
        "example:game_selection_npc",
        [0.0, 0.0, -2.0],
        Some((0.6, 1.0)),
    );
    assert_eq!(pick_actor([&npc].into_iter(), None, EYE, NORTH, 3.0), None);
    npc.metadata
        .insert(SCALE_METADATA_KEY, ActorMetadataValue::Float(2.0));
    let hit = pick_actor([npc].iter(), None, EYE, NORTH, 3.0)
        .expect("the ray intersects the scaled actor above its unscaled height");
    assert_eq!(hit.runtime_id, 71);
    assert_eq!(hit.point[1], EYE[1]);
}

#[test]
fn an_eye_inside_the_box_hits_at_zero_distance() {
    let around = actor(5, "minecraft:slime", [0.0, 0.0, 0.0], Some((2.0, 2.0)));
    let hit = pick_actor([around].iter(), None, EYE, [1.0, 0.0, 0.0], 3.0).unwrap();
    assert_eq!(hit.distance, 0.0);
    assert_eq!(pick_actor([].iter(), None, EYE, [0.0; 3], 3.0), None);
}

#[test]
fn survival_reach_and_block_occlusion_decide_the_press() {
    let hit = |distance| ActorHit {
        runtime_id: 7,
        distance,
        point: [0.0; 3],
    };
    assert_eq!(
        classify(Some(hit(2.0)), Some(4.0), 3.0),
        Crosshair::Actor(hit(2.0))
    );
    // In front of the block but beyond melee reach: nothing is targeted.
    assert_eq!(classify(Some(hit(3.5)), Some(5.0), 3.0), Crosshair::Miss);
    assert_eq!(classify(Some(hit(3.5)), None, 3.0), Crosshair::Miss);
    // The actor must beat the block by the pick radius.
    assert_eq!(classify(Some(hit(1.95)), Some(2.0), 3.0), Crosshair::Block);
    assert_eq!(classify(None, Some(2.0), 3.0), Crosshair::Block);
    assert_eq!(classify(None, None, 3.0), Crosshair::Miss);
    assert_eq!(
        classify(Some(hit(5.0)), None, 7.0),
        Crosshair::Actor(hit(5.0))
    );
}

#[test]
fn rounded_attack_margin_rejects_the_boundary_in_every_attacking_mode() {
    use client_world::game_mode_capabilities::GameModeCapabilities;

    for mode in [
        protocol::PlayerGameMode::Survival,
        protocol::PlayerGameMode::Adventure,
        protocol::PlayerGameMode::Creative,
    ] {
        let reach = GameModeCapabilities::for_mode(mode).attack_reach;
        let hit = ActorHit {
            runtime_id: 7,
            distance: reach - 0.1 - 0.000_000_01,
            point: [0.0; 3],
        };
        assert_eq!(classify(Some(hit), None, reach), Crosshair::Miss);
        let near = ActorHit {
            distance: hit.distance - 0.001,
            ..hit
        };
        assert_eq!(classify(Some(near), None, reach), Crosshair::Actor(near));
    }
}

#[test]
fn rounded_occlusion_margin_keeps_a_nearly_tied_actor_behind_the_block() {
    let hit = ActorHit {
        runtime_id: 7,
        distance: f64::from(1.9_f32),
        point: [0.0; 3],
    };
    assert_eq!(classify(Some(hit), Some(2.0), 3.0), Crosshair::Block);
}

#[test]
fn pick_box_inflation_rounds_before_intersection() {
    let actor = actor(9, "minecraft:pig", [0.0; 3], None);
    let min = [3.2_f32, 0.0, -0.5];
    let max = [4.0, 3.0, 0.5];
    let hit = pick_actor_by(
        [&actor].into_iter(),
        |_| [(min, max)],
        None,
        EYE,
        [1.0, 0.0, 0.0],
        5.0,
    )
    .unwrap();
    assert_eq!(hit.distance, f64::from(min[0] - 0.1_f32));
    assert_eq!(hit.point[0], min[0] - 0.1_f32);
}

#[test]
fn pick_distance_uses_the_rounded_world_intercept() {
    let origin = [8192.0, 65.62, -4096.0];
    let point = [origin[0] + 1.3, origin[1] + 0.2, origin[2] + 2.4];
    let [x, y, z] = std::array::from_fn(|axis| point[axis] - origin[axis]);
    assert_eq!(pick_distance(origin, point), (z * z + y * y + x * x).sqrt());
    assert_ne!(
        f64::from(pick_distance(origin, point)),
        point
            .into_iter()
            .zip(origin)
            .map(|(point, origin)| (f64::from(point) - f64::from(origin)).powi(2))
            .sum::<f64>()
            .sqrt(),
    );
}

#[test]
fn distinct_squared_distances_keep_the_nearer_actor_when_roots_tie() {
    let actors = [
        actor(2, "minecraft:pig", [0.0; 3], None),
        actor(1, "minecraft:pig", [0.0; 3], None),
    ];
    let origin = [0.0, 8192.0, 0.0];
    let boxes = |actor: &ActorSnapshot| {
        let offset = if actor.runtime_id == 1 { 2 } else { 3 };
        let x = f32::from_bits(2.1_f32.to_bits() + offset);
        [([x, 8193.0, -0.5], [3.0, 8195.0, 0.5])]
    };
    let far = pick_actor_by(
        actors[..1].iter(),
        boxes,
        None,
        origin,
        [1.0, 1.0, 0.0],
        5.0,
    )
    .unwrap();
    let near = pick_actor_by(
        actors[1..].iter(),
        boxes,
        None,
        origin,
        [1.0, 1.0, 0.0],
        5.0,
    )
    .unwrap();
    assert!(near.point[0] < far.point[0]);
    assert_eq!(near.distance, far.distance);
    let hit = pick_actor_by(actors.iter(), boxes, None, origin, [1.0, 1.0, 0.0], 5.0).unwrap();
    assert_eq!(hit.runtime_id, near.runtime_id);
}
