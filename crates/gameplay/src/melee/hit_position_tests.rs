//! Packet positions must preserve the selected simulation box's intercept.
use super::*;
use crate::entity_hover::{EntityHover, HoverSnapshot, HoveredActor};
use client_world::WorldAuthority;
use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue, WorldEvent};
use protocol_ids::{ActorDataKey, ActorFlag};
use simulation_clock::identity::SessionGeneration;

/// Spawns one actor at collision feet, with authoritative pose dimensions when supplied.
fn target(kind: ActorKind, size: Option<[f32; 2]>, scale: f32, flags: u128) -> WorldAuthority {
    let mut metadata = vec![
        ActorMetadata {
            key: ActorDataKey::Scale.wire_value(),
            value: ActorMetadataValue::Float(scale),
        },
        ActorMetadata {
            key: ActorDataKey::Flags.wire_value(),
            value: ActorMetadataValue::Flags(flags as u64),
        },
        ActorMetadata {
            key: ActorDataKey::FlagsTwo.wire_value(),
            value: ActorMetadataValue::FlagsExtended((flags >> 64) as u64),
        },
    ];
    if let Some([width, height]) = size {
        for (key, value) in [(ActorDataKey::Width, width), (ActorDataKey::Height, height)] {
            metadata.push(ActorMetadata {
                key: key.wire_value(),
                value: ActorMetadataValue::Float(value),
            });
        }
    }
    let mut world = super::actor_pick_tests::world();
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind,
                position: [10.0, 64.0, 20.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: protocol::NetworkItemStack::empty(),
                metadata: metadata.into(),
                attributes: [].into(),
                properties: [].into(),
                links: [].into(),
            })),
            Some(1),
        )
        .unwrap();
    world
}

/// Sends the picked point through attack admission, actor use and tick-time hover encoding.
fn assert_packets(hit: ActorHit, min: [f32; 3], max: [f32; 3]) {
    let press = super::tests::press(PlayerInputMode::Mouse);
    let mut melee = MeleeRuntime::default();
    melee.observe_input(true, false);
    let attack = melee.resolve(Crosshair::Actor(hit), &press, &mut SwingTracker::default());
    let mut packets = attack.packets;
    let selection = press.selection.unwrap();
    packets.push(
        protocol::use_actor_packet(ActorUseRequest {
            actor_runtime_id: hit.runtime_id,
            action: ActorUseAction::Interact,
            selected_slot: selection.slot,
            selected_item: selection.item,
            player_position: press.player_position,
            hit_position: hit.point,
        })
        .unwrap(),
    );
    EntityHover::default()
        .send_tick(
            SessionGeneration::new(1),
            1,
            HoverSnapshot {
                actor: Some(HoveredActor {
                    unique_id: 1,
                    runtime_id: hit.runtime_id,
                    point: hit.point,
                }),
                held_item: (0, 0),
            },
            |packet| {
                packets.push(packet);
                Ok::<(), ()>(())
            },
        )
        .unwrap();
    let mut reports = 0;
    for packet in &packets {
        let position =
            if let Some(tx) = protocol::test_support::transactions::observe_transaction(packet) {
                let actor = tx.actor_use().unwrap();
                assert_eq!(actor.runtime_id, hit.runtime_id);
                Some(actor.hit_position)
            } else {
                protocol::test_support::packets::entity_hover(packet).and_then(|(id, point)| {
                    assert_eq!(id, hit.runtime_id);
                    point
                })
            };
        if let Some(position) = position {
            reports += 1;
            assert_eq!(position, hit.point);
            assert!(
                (0..3).all(|i| position[i] >= min[i] - 1e-5 && position[i] <= max[i] + 1e-5),
                "point {position:?} outside {min:?}..{max:?}"
            );
        }
    }
    assert_eq!(
        reports, 3,
        "attack, interact and hover must all carry the intercept"
    );
}

/// Checks entry and inside-origin packets against independently supplied body dimensions.
fn check_box(world: &WorldAuthority, feet: [f32; 3], size: [f32; 2]) {
    let actor = world.actor(1).unwrap();
    let radius = actor.entity_fact.pick_radius;
    let half = size[0] * 0.5;
    let [x, y, z] = feet;
    let min = [x - half - radius, y - radius, z - half - radius];
    let max = [x + half + radius, y + size[1] + radius, z + half + radius];
    let origin = [x, y + size[1] * 0.5, z + 2.0];
    let pick = |eye| {
        pick_actor_by(
            world.remote_actors().filter(|actor| actor.runtime_id == 1),
            |actor| world.pick_hit_boxes(actor),
            None,
            eye,
            [0.0, 0.0, -1.0],
            3.0,
        )
    };
    let hit = pick(origin).expect("the eye ray enters the target");
    assert!(
        (hit.point[2] - max[2]).abs() < 1e-5,
        "entry {:?}, expected face {}",
        hit.point,
        max[2]
    );
    assert_packets(hit, min, max);
    let inside = [x, y + size[1] * 0.5, z];
    let hit = pick(inside).unwrap();
    assert_eq!(hit.point, inside);
    assert_packets(hit, min, max);
    assert!(
        pick([x, max[1] + 0.05, z + 2.0]).is_none(),
        "a ray above the body must miss"
    );
    assert_eq!(
        classify(Some(pick(origin).unwrap()), Some(0.5), 3.0),
        Crosshair::Block
    );
}

#[test]
fn hit_position_packets_cover_player_poses_and_moving_simulation_boxes() {
    for (height, flag) in [
        (1.8, None),
        (1.49, Some(ActorFlag::Sneaking)),
        (0.6, Some(ActorFlag::Swimming)),
        (0.6, Some(ActorFlag::Crawling)),
        (0.6, Some(ActorFlag::Gliding)),
        (0.2, Some(ActorFlag::Sleeping)),
    ] {
        let flags = flag.map_or(0, |flag| 1_u128 << flag.wire_value());
        let mut world = target(
            ActorKind::Player {
                uuid: [1; 16],
                username: "target".into(),
            },
            Some([0.6, height]),
            1.0,
            flags,
        );
        check_box(&world, [10.0, 64.0, 20.0], [0.6, height]);
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
                    dimension: 0,
                    runtime_id: 1,
                    position: [Some(10.9), Some(64.6), Some(20.9)],
                    position_origin: protocol::ActorPositionOrigin::Feet,
                    pitch: None,
                    yaw: None,
                    head_yaw: None,
                    on_ground: None,
                    teleported: false,
                    force_local_move: false,
                    player_mode: None,
                    source_tick: None,
                    interpolation: Default::default(),
                })),
                Some(2),
            )
            .unwrap();
        world.predict_remote_actor_motion(1);
        let expected = [10.3, 64.2, 20.3];
        check_box(&world, expected, [0.6, height]);
        assert_eq!(world.actor(1).unwrap().position, [10.0, 64.0, 20.0]);
        world.advance_actor_interpolation_frame(1);
        check_box(&world, expected, [0.6, height]);
    }
}

#[test]
fn hit_position_packets_cover_mob_defaults_babies_and_scale() {
    for (identifier, size_metadata) in [
        ("minecraft:chicken", None),
        ("minecraft:pig", None),
        ("minecraft:cow", None),
        ("minecraft:zombie", None),
        ("minecraft:shulker", Some([1.0, 1.0])),
    ] {
        let size = size_metadata
            .or_else(|| {
                assets::facts::entity::EntityFacts::pinned()
                    .get(identifier)
                    .collision
            })
            .unwrap();
        for scale in [0.5, 1.0, 2.0] {
            let flags = if scale < 1.0 {
                1_u128 << ActorFlag::Baby.wire_value()
            } else {
                0
            };
            let world = target(
                ActorKind::Entity {
                    identifier: identifier.into(),
                },
                size_metadata,
                scale,
                flags,
            );
            check_box(&world, [10.0, 64.0, 20.0], size.map(|side| side * scale));
        }
    }
}

#[test]
fn hit_position_packets_follow_same_tick_size_updates() {
    let mut world = target(
        ActorKind::Player {
            uuid: [1; 16],
            username: "target".into(),
        },
        Some([0.6, 1.8]),
        1.0,
        0,
    );
    for (index, height) in [0.6, 1.49, 1.8].into_iter().enumerate() {
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: [ActorMetadata {
                        key: ActorDataKey::Height.wire_value(),
                        value: ActorMetadataValue::Float(height),
                    }]
                    .into(),
                    properties: [].into(),
                    tick: 0,
                })),
                Some(index as u64 + 2),
            )
            .unwrap();
        check_box(&world, [10.0, 64.0, 20.0], [0.6, height]);
    }
}

#[test]
fn hit_position_packets_use_custom_boxes_without_scaling_their_pivots() {
    let mut world = target(
        ActorKind::Entity {
            identifier: "minecraft:pig".into(),
        },
        None,
        2.0,
        0,
    );
    let mut entry = world::NbtCompound::default();
    for (prefix, values) in [
        ("Min", [-0.5; 3]),
        ("Max", [0.5; 3]),
        ("Pivot", [0.0, 1.0, 0.0]),
    ] {
        for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values) {
            entry.insert(format!("{prefix}{axis}"), world::NbtValue::Float(value));
        }
    }
    let mut root = world::NbtCompound::default();
    root.insert(
        "Hitboxes",
        world::NbtValue::List(vec![world::NbtValue::Compound(entry)]),
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: [ActorMetadata {
                    key: ActorDataKey::HitBox.wire_value(),
                    value: ActorMetadataValue::Compound(root.encode_root().unwrap().into()),
                }]
                .into(),
                properties: [].into(),
                tick: 0,
            })),
            Some(2),
        )
        .unwrap();
    check_box(&world, [10.0, 64.5, 20.0], [1.0, 1.0]);
}

#[test]
fn hit_position_packets_follow_the_mounted_simulation_box() {
    let mut world = target(
        ActorKind::Player {
            uuid: [1; 16],
            username: "rider".into(),
        },
        Some([0.6, 1.8]),
        1.0,
        0,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Entity {
                    identifier: "minecraft:pig".into(),
                },
                position: [13.0, 64.0, 20.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: protocol::NetworkItemStack::empty(),
                metadata: [].into(),
                attributes: [].into(),
                properties: [].into(),
                links: [].into(),
            })),
            Some(2),
        )
        .unwrap();
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: [ActorMetadata {
                    key: ActorDataKey::SeatOffset.wire_value(),
                    value: ActorMetadataValue::Vector([0.0, 2.0, 0.0]),
                }]
                .into(),
                properties: [].into(),
                tick: 0,
            })),
            Some(3),
        )
        .unwrap();
    world
        .apply_ordered_event(
            WorldEvent::ActorLink(protocol::ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: 2,
                rider_unique_id: 1,
                link_type: protocol::ActorLinkType::Rider,
                immediate: false,
                rider_initiated: false,
            }),
            Some(4),
        )
        .unwrap();
    world.predict_remote_actor_motion(1);
    let feet = [13.0, 66.0 - protocol::PLAYER_NETWORK_OFFSET, 20.0];
    check_box(&world, feet, [0.6, 1.8]);
    world.advance_actor_interpolation_frame(1);
    check_box(&world, feet, [0.6, 1.8]);
}
