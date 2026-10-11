use super::*;
use assets::{
    NetworkIdMode,
    block_shapes::{PickRule, ShapeBox, ShapeFlags},
};
use protocol::{
    ContainerIdentity, InventoryEvent, InventorySlotEvent, PlayerGameMode, SlotIdentity,
};

#[test]
fn aim_world_preserves_creative_pick_context() {
    check_player_pick_context(PickRule::Creative);
}

#[test]
fn aim_world_preserves_held_light_pick_context() {
    check_player_pick_context(PickRule::HeldLight);
}

/// Tests native aim's production world view with current player facts and selected stacks.
fn check_player_pick_context(rule: PickRule) {
    let bytes = assets::pinned_block_registry_bytes();
    let version = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(bytes, version).unwrap();
    let preg = include_bytes!("../../../../crates/assets/data/block-physics-v2193.bin");
    let physics =
        assets::read_physics_registry_for_protocol(preg, bytes, &records, version).unwrap();
    let mut compiled: Vec<_> = records
        .iter()
        .map(|record| {
            physics
                .by_sequential_id(record.sequential_id)
                .unwrap()
                .clone()
        })
        .collect();
    let target_name = "minecraft:stone";
    let target = records
        .iter()
        .find(|record| record.name.as_ref() == target_name)
        .unwrap();
    // A synthetic target tests the supplied rule independently of native block names.
    let shapes = compiled[target.sequential_id as usize]
        .shapes
        .as_mut()
        .unwrap();
    shapes.pick_rule = rule;
    shapes.selection = Box::new([ShapeBox::new([0.0; 3], [1.0; 3])]);
    shapes
        .flags
        .set(ShapeFlags::LIGHT_BLOCK, rule == PickRule::HeldLight);
    let fixture_preg =
        assets::encode_physics_registry(version, bytes, &records, &compiled).unwrap();
    let collisions =
        PhysicsCollisionRegistries::from_assets(bytes, &records, &fixture_preg, version).unwrap();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let id = |name: &str| {
            let record = records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap();
            match mode {
                NetworkIdMode::Sequential => record.sequential_id,
                NetworkIdMode::Hashed => record.network_hash,
            }
        };
        for (game_mode, held, admitted) in [
            (PlayerGameMode::Survival, None, false),
            (PlayerGameMode::Creative, None, rule == PickRule::Creative),
            (
                PlayerGameMode::Survival,
                Some(target_name),
                rule == PickRule::HeldLight,
            ),
            (PlayerGameMode::Survival, Some("minecraft:dirt"), false),
        ] {
            let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 42,
                local_player_unique_id: 1,
                player_position: [8.5, 8.5, 6.5],
                world_spawn_position: [8, 8, 6],
                air_network_id: id("minecraft:air"),
                block_network_ids_are_hashes: mode == NetworkIdMode::Hashed,
            });
            let mut payload = vec![1, 2];
            payload.extend(std::iter::repeat_n(
                0xff,
                protocol::vanilla_dimension_range(0)
                    .unwrap()
                    .sub_chunk_count
                    - 1,
            ));
            payload.push(0);
            stream
                .submit(
                    1,
                    protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                        dimension: 0,
                        x: 0,
                        z: 0,
                        mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                        payload,
                    }),
                )
                .unwrap();
            stream
                .submit(
                    2,
                    protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                        dimension: 0,
                        position: [8, 8, 8],
                        layer: 0,
                        network_id: id(target_name),
                    }]),
                )
                .unwrap();
            test_time::eventually("the aim fixture terrain to commit", || {
                stream.poll([8.5, 8.5, 6.5], 0);
                stream.committed_sequence() == 2
            });
            let mut player = PlayerRuntime(player_state::PlayerState::new(1));
            player.facts.publish_player_game_mode(game_mode);
            let mut stack = protocol::NetworkItemStack::empty();
            if let Some(held) = held {
                stack.network_id = 1;
                stack.count = 1;
                stack.block_runtime_id = i32::from_ne_bytes(id(held).to_ne_bytes());
            }
            player
                .inventory
                .ledger_mut()
                .apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity {
                            window_id: Some(0),
                            slot_type: None,
                            dynamic_id: None,
                        },
                        slot: 0,
                    },
                    stack,
                    storage_item: None,
                }));
            let world = aim_world(&stream, &collisions, &player);
            let origin = sim::Vec3::new(8.5, 8.5, 6.5);
            let end = sim::Vec3::new(8.5, 8.5, 10.5);
            let authoritative = world
                .block_interaction_ray_current(origin, end - origin, 4.0)
                .unwrap()
                .map(|hit| hit.runtime_id);
            assert_eq!(authoritative, admitted.then_some(id(target_name)));
            for outlines in [false, true] {
                let hit = world
                    .camera_aim_ray(origin, end, 8, outlines, false)
                    .unwrap();
                assert_eq!(
                    hit.map(|hit| hit.runtime_id),
                    admitted.then_some(id(target_name)),
                    "{mode:?} {rule:?} {game_mode:?} {held:?} outlines={outlines}"
                );
            }
        }
    }
}
