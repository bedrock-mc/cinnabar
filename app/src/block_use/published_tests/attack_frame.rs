//! Published-frame picking preserves the owning geometry and actor ordering.

use super::attack_hit_position_tests::{attack, unpublished_attack_fixture};
use super::*;
use protocol::test_support::transactions::observe_transaction;

/// Publishes original custom boxes on the fixture's actor class at the coordinate origin.
pub(super) fn actor_boxes(world: &mut World, id: u64, boxes: &[([f32; 3], [f32; 3])]) {
    let mut root = world::NbtCompound::default();
    root.insert(
        "Hitboxes",
        world::NbtValue::List(
            boxes
                .iter()
                .map(|(minimum, maximum)| {
                    let mut entry = world::NbtCompound::default();
                    for axis in 0..3 {
                        let name = ["X", "Y", "Z"][axis];
                        let half = (maximum[axis] - minimum[axis]) * 0.5;
                        let center = (maximum[axis] + minimum[axis]) * 0.5;
                        for (prefix, value) in [("Min", -half), ("Max", half), ("Pivot", center)] {
                            entry.insert(format!("{prefix}{name}"), world::NbtValue::Float(value));
                        }
                    }
                    world::NbtValue::Compound(entry)
                })
                .collect(),
        ),
    );
    let kind = world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor(99)
        .unwrap()
        .kind
        .clone();
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                runtime_id: id,
                unique_id: id as i64,
                kind,
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: [protocol::ActorMetadata {
                    key: protocol_ids::ActorDataKey::HitBox.wire_value(),
                    value: protocol::ActorMetadataValue::Compound(
                        root.encode_root().unwrap().into(),
                    ),
                }]
                .into(),
                attributes: [].into(),
                properties: [].into(),
                links: [].into(),
            })),
        )
        .unwrap();
    stream.poll([4.5, 2.620_01, 8.5], 0);
    assert_eq!(stream.committed_sequence(), sequence);
}

#[test]
fn attack_frame_visible_box_is_not_rejected_by_an_off_ray_obstructed_box() {
    for assisted in [false, true] {
        let (mut world, mut captured) = unpublished_attack_fixture(7.5);
        let visible = ([4.25, 2.0, 7.0], [4.75, 3.0, 7.5]);
        actor_boxes(
            &mut world,
            99,
            &[visible, ([5.35, 2.0, 8.05], [5.55, 3.0, 8.25])],
        );
        assert_eq!(
            super::attack_hit_position_tests::picked_point(&world)[0],
            4.5
        );
        {
            let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            let block = stream
                .collision_store()
                .sub_chunk_ref(world::SubChunkKey::new(0, 0, 0, 0))
                .unwrap()
                .runtime_id(0, 4, 2, 6)
                .unwrap();
            stream.predict_block([5, 2, 8], 0, block);
        }
        if assisted {
            let lifetime = super::attack_hit_position_tests::actor_pick_lifetime(&world);
            world
                .resource_mut::<client_presentation::aim_assist::AimAssistFrame>()
                .target = Some(client_presentation::aim_assist::AimAssistTarget {
                kind: client_presentation::aim_assist::TargetKind::Actor(lifetime),
                minimum: Vec3::from_array(visible.0),
                maximum: Vec3::from_array(visible.1),
                point: Vec3::new(4.5, 2.5, 7.25),
                direction: Vec3::NEG_Z,
                score: 0.0,
            });
        }
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
        attack(&mut world);
        let packets = captured.drain();
        let tx = packets
            .iter()
            .find_map(observe_transaction)
            .expect("the on-ray box remains visible");
        assert_eq!(tx.actor_use().unwrap().runtime_id, 99);
        if assisted {
            assert_eq!(tx.actor_use().unwrap().hit_position, [4.5, 2.5, 7.25]);
        }
    }
}

#[test]
fn attack_frame_distinct_squared_distances_keep_the_nearer_actor_when_roots_tie() {
    let (mut world, mut captured) = unpublished_attack_fixture(7.5);
    for id in [99, 100] {
        actor_boxes(&mut world, id, &[([2.1, 8193.0, -0.5], [2.1, 8195.0, 0.5])]);
    }
    let ids: Vec<_> = world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .remote_actors()
        .map(|actor| actor.runtime_id)
        .collect();
    let far = ids[0];
    let near = ids[1];
    for (id, offset) in [(far, 3), (near, 2)] {
        let x = f32::from_bits(2.1_f32.to_bits() + offset);
        actor_boxes(&mut world, id, &[([x, 8193.0, -0.5], [x, 8195.0, 0.5])]);
    }
    let ray = frame_origin(
        &world,
        Vec3::new(0.0, 8192.0, 0.0),
        Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::new(1.0, 1.0, 0.0).normalize()),
    );
    world.insert_resource(ray);
    {
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let authority = client.stream.as_ref().unwrap().authority();
        let ray = world
            .resource::<InteractionOriginSnapshot>()
            .outbound_ray()
            .unwrap();
        let pick = |id| {
            gameplay::melee::pick_actor_by(
                std::iter::once(authority.actor(id).unwrap()),
                |actor| authority.pick_hit_boxes(actor),
                None,
                ray.origin().to_array(),
                ray.direction().to_array(),
                gameplay::mining::survival_reach(protocol::PlayerInputMode::Mouse),
            )
            .unwrap()
        };
        assert_ne!(pick(near).point, pick(far).point);
        assert_eq!(pick(near).distance, pick(far).distance);
        let expected = gameplay::melee::pick_actor_by(
            authority.remote_actors(),
            |actor| authority.pick_hit_boxes(actor),
            None,
            ray.origin().to_array(),
            ray.direction().to_array(),
            5.0,
        )
        .unwrap();
        assert_eq!(expected.runtime_id, near);
    }
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    attack(&mut world);
    let packets = captured.drain();
    let tx = packets.iter().find_map(observe_transaction).unwrap();
    assert_eq!(tx.actor_use().unwrap().runtime_id, near);
}
