//! Accepted eating progress stays before each completed movement input.
use super::*;
use attack_progress_tests::{buttons, interaction_app, ticks};

/// Supplies an accepted use whose first movement tick has not drained yet.
fn eating_app() -> (App, client_session::CapturedPackets) {
    let (mut world, captured) = fixture();
    item_use_tests::hold_air_item(&mut world, "minecraft:golden_apple", 2, [].into());
    let selection = client_interaction::block_use::verified_use_selection(
        world.resource::<client_state::PlayerRuntime>(),
        world.resource::<UiRuntimeResource>(),
    );
    let frame = gameplay::item_use::UseFrame {
        riptide: None,
        riptide_facing: Default::default(),
        tick: 101,
        now_millis: 1_000,
        position: [4.5, 2.620_01, 8.5],
        held: true,
        selection,
        air_use: gameplay::item_use::fixtures::classify(
            "minecraft:golden_apple",
            false,
            0,
            Some(32),
        ),
        ready: true,
        creative: false,
        inventory_revision: Some(1),
        charge_projectile: None,
        press_consumed: false,
    };
    let mut runtime = world.resource_mut::<client_interaction::item_use::ItemUseRuntime>();
    runtime.synchronize(7);
    runtime.observe_press(true);
    assert!(runtime.step(&frame).started);
    buttons(&mut world, vec![]);
    (interaction_app(world), captured)
}

/// Reads feed payloads and pairs each with the next admitted movement tick.
fn progress(packets: &[protocol::Packet]) -> Vec<(u64, u64, i32)> {
    let mut pending = None;
    let mut events = Vec::new();
    for packet in packets {
        if let Some(protocol::WorldEvent::Actor(protocol::ActorEvent::Status(event))) =
            protocol::into_world_event(packet.clone(), 0).unwrap()
            && event.kind == protocol::ActorStatusKind::Feed
        {
            assert!(pending.replace((event.runtime_id, event.data)).is_none());
        }
        if let Some(input) = protocol::player_auth_input_trace_sample(packet)
            && let Some((runtime_id, data)) = pending.take()
        {
            events.push((input.tick, runtime_id, data));
        }
    }
    assert!(
        pending.is_none(),
        "progress must precede its movement input"
    );
    events
}

#[test]
fn eating_progress_precedes_each_due_input_in_a_catchup_frame() {
    let (mut app, mut captured) = eating_app();
    ticks(app.world_mut(), 102, 133);
    for _ in 0..2 {
        app.world_mut()
            .run_system_cached(crate::runtime::telemetry::send_player_auth_inputs)
            .unwrap();
    }
    let packets = captured.drain();
    assert_eq!(
        packets
            .iter()
            .filter_map(protocol::player_auth_input_trace_sample)
            .count(),
        32,
    );
    assert_eq!(
        progress(&packets),
        [108, 112, 116, 120, 124, 128, 132].map(|tick| (tick, 42, 300 << 16)),
    );
}

#[test]
fn progress_animation_keeps_explicit_none_and_custom_uncertainty() {
    use assets::facts::ItemUseAnimationFact;
    for (custom, animation, expected) in [
        (false, Some("none"), Some(ItemUseAnimationFact::None)),
        (true, None, None),
        (true, Some("eat"), Some(ItemUseAnimationFact::Eat)),
    ] {
        let (mut app, _) = eating_app();
        let registry = protocol::ItemRegistryEvent {
            entries: [protocol::ItemRegistryEntry {
                identifier: "minecraft:golden_apple".into(),
                network_id: 300,
                component_based: custom,
                version: if custom {
                    protocol::ItemRegistryVersion::DataDriven
                } else {
                    protocol::ItemRegistryVersion::None
                },
                components: Some(std::sync::Arc::new(protocol::ItemComponents {
                    food: true,
                    use_animation: animation.map(Into::into),
                    ..Default::default()
                })),
                component_digest: [1; 32],
                negotiated_max_stack_size: Some(64),
                canonical_empty_component_data: false,
                item_tags: [].into(),
            }]
            .into(),
        };
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .inventory
            .ledger_mut()
            .apply_registry(&registry);
        assert!(
            app.world_mut()
                .resource_mut::<crate::runtime::world::ClientWorld>()
                .stream
                .as_mut()
                .unwrap()
                .seed_item_registry(registry)
        );
        let player = app.world().resource::<client_state::PlayerRuntime>();
        let selected = client_interaction::mining::verified_selection(player).unwrap();
        let client = app.world().resource::<crate::runtime::world::ClientWorld>();
        assert_eq!(
            client_interaction::item_use::selection::progress_animation(
                player,
                client.stream.as_ref().unwrap(),
                &selected,
            ),
            expected
        );
    }
}
