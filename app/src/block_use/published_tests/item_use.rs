use super::*;
use gameplay::melee::{ActorHit, Crosshair, PressContext};

/// Supplies a throwable, both mouse buttons, and an attack accepted in this frame.
fn attack_and_throw_fixture(enabled: bool) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = fixture();
    let network_id = 300;
    let scope = {
        let mut client_world = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client_world.stream.as_mut().unwrap();
        assert!(
            stream.seed_item_registry(protocol::ItemRegistryEvent {
                entries: [protocol::ItemRegistryEntry {
                    identifier: "minecraft:snowball".into(),
                    network_id,
                    component_based: false,
                    version: protocol::ItemRegistryVersion::None,
                    component_digest: [0; 32],
                    negotiated_max_stack_size: Some(16),
                    canonical_empty_component_data: true,
                    item_tags: std::sync::Arc::from([]),
                }]
                .into(),
            })
        );
        (
            stream.authority().actor_session_id(),
            stream.authority().current_dimension(),
        )
    };
    world.resource_scope(|world, mut ui: Mut<UiRuntime>| {
        let mut player = world.resource_mut::<crate::player_runtime::PlayerRuntime>();
        let extra_data: std::sync::Arc<[u8]> = std::sync::Arc::from([]);
        ui.inventory_ledger_mut(&mut player)
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(0),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    slot: 0,
                },
                stack: protocol::NetworkItemStack {
                    network_id,
                    metadata: 0,
                    stack_network_id: 5,
                    count: 16,
                    nbt_digest: <sha2::Sha256 as sha2::Digest>::digest(&extra_data).into(),
                    block_runtime_id: 0,
                    extra_data,
                },
                storage_item: None,
            }));
    });
    let mut input = crate::semantic_controls::SemanticInputRuntime::default();
    let snapshot = input
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: vec![1, 2],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(snapshot.phases[Action::Attack as usize].held);
    assert!(snapshot.phases[Action::Use as usize].pressed);
    world
        .insert_resource(crate::semantic_controls::SemanticInputSnapshot::from_finalized(snapshot));
    world.insert_resource(input);
    if enabled {
        world.insert_resource(crate::item_use::ModItemUsePolicy { scope: Some(scope) });
    }
    world.init_resource::<client_presentation::aim_assist::AimAssistFrame>();
    world.init_resource::<crate::camera::ServerCameraView>();
    let press = PressContext {
        tick: 101,
        player_position: [4.5, 2.620_01, 8.5],
        input_mode: protocol::PlayerInputMode::Mouse,
        local_runtime_id: 42,
        selection: crate::block_use::verified_use_selection(
            world.resource::<crate::player_runtime::PlayerRuntime>(),
            world.resource::<UiRuntime>(),
        ),
        swing_duration: 6,
        item_attack: None,
        now_millis: 1_000,
    };
    world.resource_scope(|world, mut melee: Mut<MeleeRuntime>| {
        melee.observe_input(true, true);
        let outcome = melee.resolve(
            Crosshair::Actor(ActorHit {
                runtime_id: 9,
                distance: 2.0,
                point: [4.5, 2.5, 6.5],
            }),
            &press,
            &mut world.resource_mut::<SwingTracker>(),
        );
        assert!(!outcome.packets.is_empty());
        assert!(melee.blocks_use_at(press.now_millis));
    });
    (world, captured)
}

#[test]
fn delay_fix_allows_a_throw_while_attacking_without_changing_native_timing() {
    for enabled in [false, true] {
        let (mut world, mut captured) = attack_and_throw_fixture(enabled);
        world.run_system_cached(produce_block_use).unwrap();
        assert!(!world.resource::<BlockUseRuntime>().press_pending());
        world
            .run_system_cached(crate::item_use::produce_item_use)
            .unwrap();
        let throws = captured
            .drain()
            .into_iter()
            .filter(|packet| format!("{:?}", packet.data).contains("ItemUseInventoryTransaction("))
            .count();
        assert_eq!(throws, usize::from(enabled));
    }
}
