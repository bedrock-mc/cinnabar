use super::*;
use client_world::WorldAuthority;
use protocol::{
    ActorEvent, ActorKind, ActorSpawnEvent, ContainerCloseEvent, ContainerIdentity,
    ContainerOpenEvent, InventoryAuthority, InventoryContentEvent, InventoryEvent,
    NetworkItemStack, WorldBootstrap,
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

/// Opens a mounted inventory and admits its initial request to the transport.
fn opening_mount() -> (UiRuntime, player_state::PlayerState, WorldAuthority) {
    let authority = mounted_authority();
    let mut player = player_state::PlayerState::new(4);
    let mut runtime = UiRuntime::new(4);
    runtime
        .publish_local_runtime_id(&mut player, 4, 42)
        .unwrap();
    player
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    player.facts.set_mount(Some(-91));
    runtime.toggle_inventory(&mut player, Some(&authority));
    assert!(player.inventory.ledger_mut().mark_transport_enqueued(0));
    (runtime, player, authority)
}

/// Delivers a server reply through the same queue that updates inventory presentation.
fn deliver_inventory(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    sequence: u64,
    event: InventoryEvent,
) {
    runtime
        .enqueue_inventory_event(player, 4, sequence, event)
        .unwrap();
    runtime.drain_pending_inventory(player);
}

/// Builds the actor-window reply for the requested mount.
fn mount_open() -> InventoryEvent {
    InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: protocol::WINDOW_TYPE_HORSE,
        position: [0; 3],
        runtime_entity_id: -91,
    })
}

#[test]
fn mounted_reply_clears_the_open_deadline_and_allows_immediate_close_and_reopen() {
    let (mut runtime, mut player, authority) = opening_mount();
    deliver_inventory(&mut runtime, &mut player, 1, mount_open());
    assert!(runtime.inventory_open());
    runtime.poll_inventory_timeout(
        &mut player,
        inventory::inventory_ledger::INVENTORY_REQUEST_TIMEOUT_MILLIS,
    );
    assert!(player.inventory.ledger().pending_batch().unwrap().is_none());

    runtime.toggle_inventory(&mut player, Some(&authority));
    assert!(!runtime.inventory_open());
    assert_eq!(
        player
            .inventory
            .ledger()
            .pending_batch()
            .unwrap()
            .unwrap()
            .0,
        protocol::container_close_packet(7).unwrap()
    );
    assert!(player.inventory.ledger_mut().mark_transport_enqueued(1_600));
    deliver_inventory(
        &mut runtime,
        &mut player,
        2,
        InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(7),
            window_type: protocol::WINDOW_TYPE_HORSE,
            server_initiated: false,
        }),
    );
    runtime.toggle_inventory(&mut player, Some(&authority));
    assert!(runtime.inventory_open());
    assert_eq!(
        player
            .inventory
            .ledger()
            .pending_batch()
            .unwrap()
            .unwrap()
            .0,
        protocol::open_inventory_packet(900).unwrap()
    );
}

#[test]
fn server_mounted_reply_reopens_after_cancellation() {
    for reopen in [false, true] {
        let (mut runtime, mut player, authority) = opening_mount();
        runtime.toggle_inventory(&mut player, Some(&authority));
        assert!(!runtime.inventory_open());
        if reopen {
            runtime.toggle_inventory(&mut player, Some(&authority));
        }
        deliver_inventory(&mut runtime, &mut player, 1, mount_open());
        assert!(runtime.inventory_open());
        deliver_inventory(
            &mut runtime,
            &mut player,
            2,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(7),
                slots: vec![NetworkItemStack::empty(); 2].into(),
                storage_item: NetworkItemStack::empty(),
            }),
        );
        assert!(runtime.inventory_open());
        assert!(player.inventory.ledger().pending_batch().unwrap().is_none());
    }
}
