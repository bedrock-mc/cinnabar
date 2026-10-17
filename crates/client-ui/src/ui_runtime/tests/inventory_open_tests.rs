use super::*;
use client_world::WorldAuthority;
use protocol::{
    ActorEvent, ActorKind, ActorSpawnEvent, InventoryAuthority, InventoryEvent, WorldBootstrap,
};

/// Provides a committed remote mount whose runtime ID differs from both persistent identities.
fn mounted_authority() -> WorldAuthority {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 11,
            local_player_runtime_id: 42,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: -91,
                runtime_id: 900,
                kind: ActorKind::Entity {
                    identifier: "test:mount".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: [].into(),
                attributes: [].into(),
                properties: [].into(),
                links: [].into(),
            })),
            Some(1),
        )
        .unwrap();
    authority
}

/// Reads the actual pending open packet after the UI chooses its target.
fn open_packet(mount: Option<i64>, authority: Option<&WorldAuthority>) -> protocol::Packet {
    let mut player = player_state::PlayerState::new(4);
    let mut runtime = UiRuntime::new(4);
    runtime
        .publish_local_runtime_id(&mut player, 4, 42)
        .unwrap();
    player
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    player.facts.set_mount(mount);
    runtime.toggle_inventory(&mut player, authority);
    let mut sent = None;
    super::super::interaction::flush_inventory_send(&mut player, &mut runtime, 0, |packet| {
        sent = Some(packet);
        Ok::<_, ()>(())
    })
    .unwrap();
    sent.expect("one inventory open")
}

/// The same input opens a retained mount, while missing mounts keep the personal target.
#[test]
fn inventory_open_targets_the_live_mount_instead_of_the_local_player() {
    let authority = mounted_authority();
    assert_eq!(
        open_packet(Some(-91), Some(&authority)),
        protocol::open_inventory_packet(900).unwrap()
    );
    assert_eq!(
        open_packet(None, Some(&authority)),
        protocol::open_inventory_packet(42).unwrap()
    );
    assert_eq!(
        open_packet(Some(-92), Some(&authority)),
        protocol::open_inventory_packet(42).unwrap()
    );
    assert_eq!(
        open_packet(None, None),
        protocol::open_inventory_packet(42).unwrap()
    );
}
