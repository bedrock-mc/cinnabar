//! Cached actor points retain the ordinary attack admission gates.

use super::attack_hit_position_tests::{
    actor_pick_bounds, actor_pick_lifetime, attack, attack_fixture, unpublished_attack_fixture,
};
use super::*;
use client_presentation::aim_assist::{AimAssistFrame, AimAssistTarget, TargetKind};
use protocol::test_support::transactions::observe_transaction;

#[path = "ordered_mouse.rs"]
mod ordered_mouse_tests;

/// Publishes attack input with an explicit logical input mode.
fn press(world: &mut World, mode: semantic_input::InputMode) {
    let mut snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: vec![1],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    snapshot.input_mode = mode;
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
}

/// Runs the production attack/mining order without updating the movement timeline.
fn produce(world: &mut World) {
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
}

/// Publishes a selected actor point along the fixture ray and optionally removes its block.
fn assisted_fixture(distance: f32, clear_block: bool) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = unpublished_attack_fixture(8.5 - distance);
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    if clear_block {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream.predict_block([4, 2, 6], 0, stream.air_block_id());
    }
    let point = Vec3::new(4.5, 2.5, 8.5 - distance);
    let (minimum, maximum) = actor_pick_bounds(&world);
    let lifetime = actor_pick_lifetime(&world);
    world.resource_mut::<AimAssistFrame>().target = Some(AimAssistTarget {
        kind: TargetKind::Actor(lifetime),
        minimum,
        maximum,
        point,
        direction: Vec3::NEG_Z,
        score: 0.0,
    });
    (world, captured)
}

/// Publishes an idle selected target, then resolves an attack with the same input mode.
fn assisted_attack(world: &mut World, mode: semantic_input::InputMode) {
    let mut idle = world
        .resource::<client_state::SemanticInputSnapshot>()
        .snapshot()
        .unwrap()
        .clone();
    idle.input_mode = mode;
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(idle),
    ));
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    press(world, mode);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
}

/// Counts only actor-attack transactions, ignoring swings and unrelated packet variants.
fn actor_attacks(
    captured: &mut client_session::CapturedPackets,
) -> Vec<protocol::test_support::transactions::ActorUseObservation> {
    captured
        .drain()
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.actor_use().cloned())
        .filter(|tx| tx.action == Some(protocol::ActorUseAction::Attack))
        .collect()
}

#[test]
fn attack_press_waits_for_backing_drop_admission() {
    let (mut world, mut captured) = unpublished_attack_fixture(7.5);
    world.resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
        world
            .resource_mut::<UiRuntimeResource>()
            .publish_local_runtime_id(&mut player, 7, 42)
            .unwrap();
    });
    {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let ledger = player.inventory.ledger_mut();
        ledger.apply(&InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
        let entries = protocol::vanilla_item_registry();
        let network_id = entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:stone")
            .unwrap()
            .network_id;
        ledger.apply_registry(&protocol::ItemRegistryEvent { entries });
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: protocol::ContainerIdentity::window(
                    protocol::PLAYER_INVENTORY_WINDOW_ID,
                ),
                slot: 0,
            },
            stack: protocol::NetworkItemStack {
                network_id,
                stack_network_id: 9,
                count: 1,
                ..protocol::NetworkItemStack::empty()
            },
            storage_item: None,
        }));
        ledger.begin_world_drop(0, None).unwrap();
    }
    press(&mut world, semantic_input::InputMode::KeyboardMouse);
    produce(&mut world);
    assert!(actor_attacks(&mut captured).is_empty());
    assert!(world.resource::<MeleeRuntime>().press_pending());

    {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        player.inventory.set_local_selected_slot(1);
        let mode = player.facts.player_game_mode();
        player.inventory.queue_local_hotbar_selection(0, mode);
    }
    assert!(
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .inventory
            .ledger_mut()
            .mark_transport_enqueued(1)
    );
    let idle = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(Default::default()),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(idle),
    ));
    produce(&mut world);
    assert!(actor_attacks(&mut captured).is_empty());
    assert!(world.resource::<MeleeRuntime>().press_pending());
    world
        .run_system_cached(crate::hotbar::flush_hotbar_selection)
        .unwrap();
    assert_eq!(
        world
            .resource::<client_state::PlayerRuntime>()
            .inventory
            .pending_hotbar_selection(),
        None
    );
    produce(&mut world);
    let attacks = actor_attacks(&mut captured);
    assert_eq!(attacks.len(), 1);
    assert_eq!(
        attacks[0].item.network_id, 0,
        "the admitted drop leaves an empty hand"
    );
    assert!(!world.resource::<MeleeRuntime>().press_pending());
}

#[test]
fn attack_admission_uncached_actor_press_does_not_mine_the_block_behind_it() {
    let (mut world, mut captured) = unpublished_attack_fixture(7.5);
    world.init_resource::<client_interaction::survival_mining::SurvivalMiningRuntime>();
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    press(&mut world, semantic_input::InputMode::KeyboardMouse);
    produce(&mut world);
    assert_eq!(
        world
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        0,
        "the fresh actor fallback must not break the block behind it"
    );
    assert_eq!(actor_attacks(&mut captured).len(), 1);
    press(&mut world, semantic_input::InputMode::KeyboardMouse);
    produce(&mut world);
    assert_eq!(
        actor_attacks(&mut captured).len(),
        0,
        "holding after the fresh fallback must not repeat the attack"
    );
}

#[test]
fn attack_admission_uncached_actor_press_holds_native_item_use() {
    for delay_fix in [false, true] {
        let (mut world, mut captured) = unpublished_attack_fixture(7.5);
        let scope = item_use_tests::hold_snowball(&mut world);
        if delay_fix {
            world.insert_resource(crate::item_use::ModItemUsePolicy { scope: Some(scope) });
        }
        let mut router = crate::semantic_controls::SemanticInputRuntime::default();
        hold(&mut world, &mut router, Vec::new(), vec![1, 2]);
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
        world.run_system_cached(produce_block_use).unwrap();
        world
            .run_system_cached(crate::melee::produce_attacks)
            .unwrap();
        produce_air_use(&mut world);
        let throws = captured
            .drain()
            .iter()
            .filter_map(observe_transaction)
            .filter(|tx| tx.item_use().is_some())
            .count();
        assert_eq!(
            throws,
            usize::from(delay_fix),
            "native use respects the attack; the explicit override permits it"
        );
    }
}

#[test]
fn attack_admission_assisted_mouse_target_beyond_pick_reach_is_rejected() {
    let (mut world, mut captured) = assisted_fixture(6.5, true);
    assisted_attack(&mut world, semantic_input::InputMode::KeyboardMouse);
    assert!(actor_attacks(&mut captured).is_empty());
}

#[test]
fn attack_admission_assisted_gamepad_target_beyond_pick_reach_is_rejected() {
    let (mut world, mut captured) = assisted_fixture(6.5, true);
    assisted_attack(&mut world, semantic_input::InputMode::GamePad);
    assert!(actor_attacks(&mut captured).is_empty());
}

#[test]
fn attack_admission_assisted_reach_uses_the_box_without_replacing_its_selected_point() {
    for (input, wire) in [
        (
            semantic_input::InputMode::KeyboardMouse,
            protocol::PlayerInputMode::Mouse,
        ),
        (
            semantic_input::InputMode::GamePad,
            protocol::PlayerInputMode::GamePad,
        ),
    ] {
        let distance = gameplay::mining::creative_reach(wire) as f32 + 0.2;
        let (mut world, mut captured) = assisted_fixture(distance, true);
        assisted_attack(&mut world, input);
        let attacks = actor_attacks(&mut captured);
        assert_eq!(attacks.len(), 1, "the near box edge remains in pick reach");
        assert_eq!(attacks[0].hit_position, [4.5, 2.5, 8.5 - distance]);
    }
}

#[test]
fn attack_admission_assisted_touch_target_within_both_limits_is_retained() {
    let (mut world, mut captured) = assisted_fixture(6.5, true);
    assisted_attack(&mut world, semantic_input::InputMode::Touch);
    let attacks = actor_attacks(&mut captured);
    assert_eq!(attacks.len(), 1);
    assert_eq!(attacks[0].hit_position, [4.5, 2.5, 2.0]);
}

#[test]
fn attack_admission_assisted_target_cannot_bypass_a_shorter_personal_reach() {
    let (mut world, mut captured) = assisted_fixture(1.5, true);
    world.insert_resource(crate::interaction_override::ModInteraction {
        attack_reach: Some(1.0),
        ..Default::default()
    });
    assisted_attack(&mut world, semantic_input::InputMode::KeyboardMouse);
    assert!(actor_attacks(&mut captured).is_empty());
}

#[test]
fn attack_admission_assisted_target_behind_a_block_is_rejected() {
    let (mut world, mut captured) = assisted_fixture(3.0, false);
    assisted_attack(&mut world, semantic_input::InputMode::KeyboardMouse);
    assert!(actor_attacks(&mut captured).is_empty());
}

#[test]
fn attack_admission_assisted_visibility_uses_the_segment_from_the_published_eye() {
    for obstructed in [false, true] {
        let (mut world, mut captured) = assisted_fixture(3.0, false);
        {
            let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            let block = stream
                .collision_store()
                .sub_chunk_ref(world::SubChunkKey::new(0, 0, 0, 0))
                .unwrap()
                .runtime_id(0, 4, 2, 6)
                .unwrap();
            stream.predict_block([4, 2, 6], 0, stream.air_block_id());
            if obstructed {
                stream.predict_block([5, 2, 7], 0, block);
            }
        }
        let eye = Vec3::new(6.5, 2.5, 8.5);
        let ray = frame_origin(&world, eye, Quat::IDENTITY);
        world.insert_resource(ray);
        let point = world.resource::<AimAssistFrame>().target.unwrap().point;
        {
            let client = world.resource::<crate::runtime::world::ClientWorld>();
            let stream = client.stream.as_ref().unwrap();
            let registries = world.resource::<PhysicsCollisionRegistries>();
            let blocks = sim::PaletteWorld::new(
                stream.collision_store(),
                registries.registry(stream.network_id_mode()),
                0,
                Default::default(),
            );
            let vector = |v: Vec3| sim::Vec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z));
            assert!(
                blocks
                    .block_interaction_ray_current(vector(eye), vector(Vec3::NEG_Z), 6.0)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                blocks
                    .block_interaction_ray_current(
                        vector(eye),
                        vector((point - eye).normalize()),
                        6.0
                    )
                    .unwrap()
                    .is_some(),
                obstructed
            );
        }
        assisted_attack(&mut world, semantic_input::InputMode::KeyboardMouse);
        assert_eq!(
            actor_attacks(&mut captured).len(),
            usize::from(!obstructed),
            "visibility must follow the eye-to-selected-point segment"
        );
    }
}

#[test]
fn attack_admission_removed_target_becomes_a_miss() {
    let (mut world, mut captured) = attack_fixture(7.5);
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let sequence = stream.committed_sequence() + 1;
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(
                    protocol::ActorRemoveEvent {
                        dimension: 0,
                        unique_id: 99,
                    },
                )),
            )
            .unwrap();
        stream.poll([4.5, 2.620_01, 8.5], 0);
        assert!(stream.authority().actor(99).is_none());
    }
    attack(&mut world);
    assert!(actor_attacks(&mut captured).is_empty());
}
