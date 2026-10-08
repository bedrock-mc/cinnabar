use std::sync::Arc;

use protocol::{
    ActorAttribute, ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorMetadataValue,
    ActorSpawnEvent,
};

use super::super::{
    ACTOR_FLAG_INVISIBLE, ActorStore, BOUNDING_BOX_HEIGHT_METADATA_KEY,
    BOUNDING_BOX_WIDTH_METADATA_KEY, FLAG_BABY, SCALE_METADATA_KEY, VARIANT_METADATA_KEY,
    fire::ACTOR_FLAG_ON_FIRE,
};

fn spawn(store: &mut ActorStore, runtime_id: u64, identifier: &str, width: f32) {
    let kind = if identifier == "player" {
        ActorKind::Player {
            uuid: [runtime_id as u8; 16],
            username: format!("player-{runtime_id}").into(),
        }
    } else {
        ActorKind::Entity {
            identifier: format!("minecraft:{identifier}").into(),
        }
    };
    store.apply(
        1,
        runtime_id,
        ActorEvent::Spawn(ActorSpawnEvent {
            dimension: 0,
            unique_id: runtime_id as i64,
            runtime_id,
            kind,
            position: [4.0, 70.0, -2.0],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    if width > 0.0 {
        set(
            store,
            runtime_id,
            BOUNDING_BOX_WIDTH_METADATA_KEY,
            ActorMetadataValue::Float(width),
        );
    }
}

fn set(store: &mut ActorStore, runtime_id: u64, key: u32, value: ActorMetadataValue) {
    store
        .actors
        .get_mut(&runtime_id)
        .unwrap()
        .metadata
        .insert(key, value);
}

fn flag(store: &mut ActorStore, runtime_id: u64, bit: u32) {
    let actor = store.actors.get_mut(&runtime_id).unwrap();
    let flags = match actor.metadata.get(&0) {
        Some(ActorMetadataValue::Flags(flags)) => *flags,
        _ => 0,
    };
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(flags | 1 << bit));
}

fn radius(store: &ActorStore, runtime_id: u64) -> Option<f32> {
    let actor = store.actors.get(&runtime_id).unwrap();
    store.shadow_caster(actor, 1.0).map(|caster| caster.radius)
}

fn close(left: Option<f32>, right: f32) -> bool {
    left.is_some_and(|left| (left - right).abs() < 1.0e-6)
}

#[test]
fn radius_is_the_collision_width_scaled_by_the_vanilla_table() {
    let mut store = ActorStore::new(1, 0);
    let cases: &[(&str, f32, f32)] = &[
        ("player", 0.6, 0.6),
        ("cow", 0.9, 0.9),
        ("item", 0.25, 0.25),
        ("ghast", 4.0, 3.2),
        ("happy_ghast", 4.0, 3.2),
        ("creaking", 0.9, 0.72),
        ("spider", 1.4, 0.98),
        ("cave_spider", 0.7, 0.49),
        ("armadillo", 0.7, 0.4025),
        ("horse", 1.4, 0.84),
        ("zombie_horse", 1.4, 0.84),
        ("ender_dragon", 13.0, 3.9),
        ("tadpole", 0.4, 0.2),
        ("iron_golem", 1.4, 0.7),
        ("shulker", 1.0, 0.5),
        ("turtle", 1.2, 1.2),
        ("tripod_camera", 0.75, 0.5),
        ("ender_crystal", 2.0, 0.5),
        ("boat", 1.4, 1.0),
        ("chest_boat", 1.4, 1.0),
    ];
    for (index, (identifier, width, expected)) in cases.iter().enumerate() {
        let id = index as u64 + 1;
        spawn(&mut store, id, identifier, *width);
        assert!(
            close(radius(&store, id), *expected),
            "{identifier}: {:?}",
            radius(&store, id)
        );
    }
}

/// A player without streamed bounds keeps its definition width; other actors cast nothing.
#[test]
fn missing_width_falls_back_only_for_players() {
    let mut store = ActorStore::new(1, 0);
    spawn(&mut store, 1, "player", 0.0);
    spawn(&mut store, 2, "zombie", 0.0);
    assert!(close(radius(&store, 1), 0.6));
    assert_eq!(radius(&store, 2), None);
}

#[test]
fn babies_and_slimes_follow_their_own_rules() {
    let mut store = ActorStore::new(1, 0);
    for (id, identifier) in [(1, "iron_golem"), (2, "shulker"), (3, "turtle"), (4, "cow")] {
        spawn(&mut store, id, identifier, 1.0);
        flag(&mut store, id, FLAG_BABY);
    }
    assert!(close(radius(&store, 1), 0.25));
    assert!(close(radius(&store, 2), 0.25));
    assert!(close(radius(&store, 3), 0.33));
    // Other babies shrink through their streamed width alone.
    assert!(close(radius(&store, 4), 1.0));
    for (id, identifier, variant, expected) in [
        (5, "slime", 4, 1.0),
        (6, "magma_cube", 2, 0.5),
        (7, "slime", 1, 0.25),
    ] {
        spawn(&mut store, id, identifier, 2.08);
        set(
            &mut store,
            id,
            VARIANT_METADATA_KEY,
            ActorMetadataValue::Int(variant),
        );
        assert!(
            close(radius(&store, id), expected),
            "{identifier} {variant}"
        );
    }
}

#[test]
fn shadowless_actor_types_cast_nothing() {
    let mut store = ActorStore::new(1, 0);
    let shadowless = [
        "armor_stand",
        "minecart",
        "hopper_minecart",
        "xp_orb",
        "painting",
        "tnt",
        "falling_block",
        "fishing_hook",
        "arrow",
        "snowball",
        "thrown_trident",
        "wind_charge_projectile",
        "evocation_fang",
        "fireworks_rocket",
    ];
    for (index, identifier) in shadowless.iter().enumerate() {
        let id = index as u64 + 1;
        spawn(&mut store, id, identifier, 0.5);
        assert_eq!(radius(&store, id), None, "{identifier}");
    }
}

#[test]
fn burning_invisible_dead_and_submerged_actors_cast_nothing() {
    let mut store = ActorStore::new(1, 0);
    for id in 1..=6 {
        spawn(&mut store, id, "pig", 0.9);
    }
    flag(&mut store, 1, ACTOR_FLAG_ON_FIRE);
    flag(&mut store, 2, ACTOR_FLAG_INVISIBLE);
    store.actors.get_mut(&3).unwrap().status.dead = true;
    store.actors.get_mut(&4).unwrap().attributes.insert(
        "minecraft:health".into(),
        ActorAttribute {
            name: "minecraft:health".into(),
            min: 0.0,
            max: 10.0,
            current: 0.0,
            default: Some(10.0),
            modifiers: Arc::from([]),
        },
    );
    store.set_breathing_liquids(&[(5, true), (6, false)]);
    for id in 1..=5 {
        assert_eq!(radius(&store, id), None, "actor {id}");
    }
    assert!(close(radius(&store, 6), 0.9));
}

/// A rider leaves its shadow to a visible vehicle; a shoulder parrot casts none.
#[test]
fn riders_of_visible_vehicles_cast_nothing() {
    let mut store = ActorStore::new(1, 0);
    spawn(&mut store, 1, "player", 0.6);
    spawn(&mut store, 2, "horse", 1.4);
    spawn(&mut store, 3, "player", 0.6);
    spawn(&mut store, 4, "boat", 1.4);
    spawn(&mut store, 5, "parrot", 0.5);
    let link = |rider, ridden| ActorLinkEvent {
        dimension: 0,
        ridden_unique_id: ridden,
        rider_unique_id: rider,
        link_type: ActorLinkType::Rider,
        immediate: false,
        rider_initiated: false,
    };
    store.apply_link(1, 10, link(1, 2));
    store.apply_link(1, 11, link(3, 4));
    store.apply_link(1, 12, link(5, 1));
    flag(&mut store, 4, ACTOR_FLAG_INVISIBLE);
    assert_eq!(radius(&store, 1), None);
    assert!(close(radius(&store, 2), 0.84));
    assert!(close(radius(&store, 3), 0.6));
    assert_eq!(radius(&store, 5), None);
}

/// Ghasts hang their shadow a fraction of their scaled height below the feet.
#[test]
fn ghasts_lower_their_shadow_by_a_relative_height() {
    let mut store = ActorStore::new(1, 0);
    spawn(&mut store, 1, "ghast", 4.0);
    set(
        &mut store,
        1,
        BOUNDING_BOX_HEIGHT_METADATA_KEY,
        ActorMetadataValue::Float(4.0),
    );
    set(
        &mut store,
        1,
        SCALE_METADATA_KEY,
        ActorMetadataValue::Float(0.5),
    );
    spawn(&mut store, 2, "happy_ghast", 4.0);
    set(
        &mut store,
        2,
        BOUNDING_BOX_HEIGHT_METADATA_KEY,
        ActorMetadataValue::Float(4.0),
    );
    spawn(&mut store, 3, "cow", 0.9);
    set(
        &mut store,
        3,
        BOUNDING_BOX_HEIGHT_METADATA_KEY,
        ActorMetadataValue::Float(1.3),
    );
    let feet = |id: u64| {
        let actor = store.actors.get(&id).unwrap();
        store.shadow_caster(actor, 1.0).unwrap().feet[1]
    };
    assert!((feet(1) - (70.0 - 0.875 * 4.0 * 0.5)).abs() < 1.0e-5);
    assert!((feet(2) - (70.0 - 0.5 * 4.0)).abs() < 1.0e-5);
    assert_eq!(feet(3), 70.0);
}
