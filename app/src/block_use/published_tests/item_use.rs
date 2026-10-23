use super::*;
use gameplay::melee::{ActorHit, Crosshair, PressContext};

/// Selects a stack of snowballs in hotbar slot 0 and returns the world scope.
pub(super) fn hold_snowball(world: &mut World) -> (u64, i32) {
    hold_air_item(world, "minecraft:snowball", 16, std::sync::Arc::from([]))
}

/// Publishes one negotiated item and its authoritative hotbar stack to both owners.
pub(super) fn hold_air_item(
    world: &mut World,
    identifier: &str,
    count: u8,
    extra_data: std::sync::Arc<[u8]>,
) -> (u64, i32) {
    let network_id = 300;
    let registry = protocol::ItemRegistryEvent {
        entries: [protocol::ItemRegistryEntry {
            identifier: identifier.into(),
            network_id,
            component_based: false,
            version: protocol::ItemRegistryVersion::None,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(count),
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
        }]
        .into(),
    };
    let scope = {
        let mut client_world = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client_world.stream.as_mut().unwrap();
        assert!(stream.seed_item_registry(registry.clone()));
        (
            stream.authority().actor_session_id(),
            stream.authority().current_dimension(),
        )
    };
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        player.inventory.ledger_mut().apply_registry(&registry);
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
                    count: u16::from(count),
                    nbt_digest: <sha2::Sha256 as sha2::Digest>::digest(&extra_data).into(),
                    block_runtime_id: 0,
                    extra_data,
                },
                storage_item: None,
            }));
    });
    world.init_resource::<client_presentation::aim_assist::AimAssistFrame>();
    world.init_resource::<client_presentation::camera::ServerCameraView>();
    scope
}

/// Supplies a throwable, both mouse buttons, and an attack accepted in this frame.
fn attack_and_throw_fixture(enabled: bool) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = fixture();
    let scope = hold_snowball(&mut world);
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
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world.insert_resource(input);
    if enabled {
        world.insert_resource(crate::item_use::ModItemUsePolicy { scope: Some(scope) });
    }
    let press = PressContext {
        refused_destroy: None,
        tick: 101,
        player_position: [4.5, 2.620_01, 8.5],
        input_mode: protocol::PlayerInputMode::Mouse,
        local_runtime_id: 42,
        selection: client_interaction::block_use::verified_use_selection(
            world.resource::<client_state::PlayerRuntime>(),
            world.resource::<UiRuntimeResource>(),
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
        produce_air_use(&mut world);
        let throws = captured
            .drain()
            .into_iter()
            .filter(|packet| {
                protocol::test_support::transactions::observe_transaction(packet)
                    .is_some_and(|transaction| transaction.item_use().is_some())
            })
            .count();
        assert_eq!(throws, usize::from(enabled));
    }
}

#[test]
fn pending_melee_preserves_an_active_crossbow_release_before_charge_completion() {
    let (mut world, mut captured) = fixture();
    hold_air_item(&mut world, "minecraft:crossbow", 1, [].into());
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    produce_air_use(&mut world);
    let (started, duration) = world
        .resource::<client_interaction::item_use::ItemUseRuntime>()
        .active_timing()
        .expect("the crossbow starts charging");
    captured.drain();
    let release_tick = started + u64::from(duration) - 1;
    for tick in started..release_tick {
        let mut sample = gameplay::test_support::survival_mining::completed(tick);
        sample.position = [4.5, 2.620_01, 8.5];
        world
            .resource_mut::<MovementTicker>()
            .enqueue_completed_physics(sample)
            .unwrap();
    }
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: vec![1],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(snapshot.phases[Action::Use as usize].released);
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world
        .resource_mut::<MeleeRuntime>()
        .observe_input(true, true);
    produce_air_use(&mut world);
    assert!(world.resource::<MeleeRuntime>().press_pending());
    let packets = captured.drain();
    assert!(
        packets.iter().any(|packet| {
            protocol::test_support::transactions::observe_transaction(packet)
                .is_some_and(|tx| tx.release().is_some())
        }),
        "an accepted early release must leave while the melee waits"
    );
    assert!(
        !world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .is_using()
    );
}

#[test]
fn a_written_release_does_not_hold_later_inputs_for_another_frame() {
    let (mut world, mut captured) = fixture();
    hold_snowball(&mut world);
    let selection = client_interaction::block_use::verified_use_selection(
        world.resource::<client_state::PlayerRuntime>(),
        world.resource::<UiRuntimeResource>(),
    )
    .unwrap();
    let sample = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap();
    let release = protocol::release_item_packet(protocol::HeldItemRequest {
        hand: protocol::ItemUseHand::Mainhand,
        selected_slot: selection.slot,
        selected_item: selection.item,
        player_position: sample.position,
    })
    .unwrap();
    world
        .resource_mut::<MovementTicker>()
        .hold_release_after_tick(sample.tick, vec![release]);
    let mut written = None;
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        let network = world.resource::<NetworkHandle<PackApplication>>();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut movement,
            gameplay::movement::OUTBOX_CAPACITY,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| {
                written = Some(identity);
                network.send_physics_packet(identity, packet, guard)
            },
        )
        .unwrap();
    });
    assert!(
        world
            .resource_mut::<MovementTicker>()
            .acknowledge_physics_send(written.unwrap())
    );
    captured.drain();
    let mut next = gameplay::test_support::survival_mining::completed(sample.tick + 1);
    next.position = sample.position;
    world
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(next)
        .unwrap();
    world
        .run_system_cached(crate::item_use::prepare_item_release)
        .unwrap();
    produce_air_use(&mut world);
    let packets = captured.drain();
    let release = packets
        .iter()
        .position(|packet| {
            protocol::test_support::transactions::observe_transaction(packet)
                .is_some_and(|transaction| transaction.release().is_some())
        })
        .unwrap();
    let input = packets
        .iter()
        .position(|packet| {
            protocol::player_auth_input_trace_sample(packet)
                .is_some_and(|input| input.tick == sample.tick + 1)
        })
        .expect("the next input must leave in the frame that admits the release");
    assert!(release < input);
}
