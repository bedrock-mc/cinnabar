//! Selected component durations through final production publication on frames without input.
use std::{sync::Arc, time::Duration};

use bevy::prelude::*;
use gameplay::movement::MovementEffectSource;
use protocol::{ContainerIdentity, InventoryEvent, InventorySlotEvent, SlotIdentity};

use super::{ClientWorld, custom_emotes, prepare_actor_render_frame};

/// Installs an original component-defined item through the registry and inventory owners.
fn select_authored_swing(world: &mut World, ticks: u32) {
    let identifier: Arc<str> = "fixture:long_swing".into();
    let network_id = 300;
    let mut client = world.resource_mut::<ClientWorld>();
    assert!(
        client
            .stream
            .as_mut()
            .unwrap()
            .seed_item_registry(protocol::ItemRegistryEvent {
                entries: [protocol::ItemRegistryEntry {
                    identifier: identifier.clone(),
                    network_id,
                    component_based: true,
                    version: protocol::ItemRegistryVersion::DataDriven,
                    components: None,
                    component_digest: [0; 32],
                    negotiated_max_stack_size: Some(1),
                    canonical_empty_component_data: true,
                    item_tags: Arc::from([]),
                }]
                .into(),
            })
    );
    client.session_items = Some(Arc::new(
        client_presentation::session_assets::SessionItems {
            components: Arc::new(
                [(
                    identifier,
                    protocol::ItemComponents {
                        attack: Some(protocol::ItemAttackTiming {
                            swing_duration_ticks: Some(ticks),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            ),
            icons: None,
        },
    ));
    world.resource_scope(
        |world, mut ui: Mut<client_shell::ui_resources::UiRuntimeResource>| {
            let mut player = world.resource_mut::<client_state::PlayerRuntime>();
            player
                .facts
                .publish_player_game_mode(protocol::PlayerGameMode::Survival);
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
                        count: 1,
                        stack_network_id: 5,
                        ..protocol::NetworkItemStack::empty()
                    },
                    storage_item: None,
                }));
        },
    );
}

#[test]
fn final_publication_preserves_authored_swing_ticks_without_attack_input() {
    let authored_ticks = 19;
    let mut world = custom_emotes::fixture();
    select_authored_swing(&mut world, authored_ticks);
    world.init_resource::<client_interaction::movement::MovementTicker>();
    world.init_resource::<client_interaction::movement::LocalMovementEffectTimeline>();
    world.init_resource::<client_interaction::melee::SwingTracker>();
    assert!(!world.contains_resource::<client_state::SemanticInputSnapshot>());

    for frame in 0..4_u64 {
        *world.resource_mut::<client_interaction::movement::MovementTicker>() = {
            let mut ticker = client_interaction::movement::MovementTicker::default();
            *ticker = gameplay::test_support::survival_mining::ticker_with_ticks(frame + 1);
            ticker
        };
        let mut effects =
            world.resource_mut::<client_interaction::movement::LocalMovementEffectTimeline>();
        effects.begin_frame();
        MovementEffectSource::commit_successful_tick(&mut **effects);
        world
            .resource_mut::<Time<bevy::time::Real>>()
            .advance_by(Duration::from_millis(50));
        world.run_system_cached(super::advance_actor_frame).unwrap();
        assert_eq!(
            crate::melee::selected_attack_timing(
                world.resource::<client_state::PlayerRuntime>(),
                world.resource::<ClientWorld>(),
            )
            .and_then(|timing| timing.swing_duration_ticks),
            Some(authored_ticks),
            "the selected stack must resolve its session component facts"
        );
        if frame == 0 {
            let ticker = world.resource::<client_interaction::movement::MovementTicker>();
            let (authority, tick) = (
                ticker.interaction_authority_identity(),
                ticker.completed_tick(),
            );
            let mut tracker = world.resource_mut::<client_interaction::melee::SwingTracker>();
            tracker.sync_ticks_for_item(authority, tick, &Default::default(), Some(authored_ticks));
            assert!(tracker.try_swing(tick, authored_ticks as i32));
        }
        world.run_system_cached(prepare_actor_render_frame).unwrap();
        let client = world.resource::<ClientWorld>();
        let rig = client
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .actor_rig(1)
            .unwrap();
        let counters = [frame.saturating_sub(1) as f32, frame as f32];
        assert_eq!(
            rig.hand.map(|hand| hand.attack_time),
            counters.map(|counter| counter / authored_ticks as f32),
            "final publication must retain the selected item's denominator on idle frame {frame}"
        );
        assert_eq!(
            rig.java.swing,
            counters.map(|counter| counter / actor_animation::ACTOR_SWING_TICKS as f32),
            "the independent Java denominator remains effect-based"
        );
    }
}

#[test]
fn server_swings_preserve_the_independent_java_restart_guard() {
    let mut world = custom_emotes::fixture();
    let (network, mut captured) = client_runtime::network::NetworkHandle::<
        crate::runtime::network::resource_packs::PackApplication,
    >::stub_capturing_packets();
    world.insert_resource(network);
    let authored_ticks = 19;
    select_authored_swing(&mut world, authored_ticks);
    world.init_resource::<client_interaction::movement::MovementTicker>();
    world.init_resource::<client_interaction::movement::LocalMovementEffectTimeline>();
    world.init_resource::<client_interaction::melee::SwingTracker>();
    world.run_system_cached(super::advance_actor_frame).unwrap();
    let mut java = gameplay::melee::SwingTracker::default();
    for tick in 1..=8_u64 {
        *world.resource_mut::<client_interaction::movement::MovementTicker>() = {
            let mut ticker = client_interaction::movement::MovementTicker::default();
            *ticker = gameplay::test_support::survival_mining::ticker_with_ticks(tick);
            ticker
        };
        let mut effects =
            world.resource_mut::<client_interaction::movement::LocalMovementEffectTimeline>();
        effects.begin_frame();
        MovementEffectSource::commit_successful_tick(&mut **effects);
        world
            .resource_mut::<Time<bevy::time::Real>>()
            .advance_by(Duration::from_millis(50));
        if matches!(tick, 1 | 5) {
            let mut client = world.resource_mut::<ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            stream
                .submit(
                    stream.committed_sequence() + 1,
                    protocol::WorldEvent::ItemActor(protocol::ItemActorEvent::Action(
                        protocol::ActorActionEvent {
                            actor_runtime_ids: Arc::from([1_u64]),
                            kind: protocol::ActorActionKind::SwingArm,
                            data: 0.0,
                            swing_source: None,
                        },
                    )),
                )
                .unwrap();
            java.try_swing(tick + 1, actor_animation::ACTOR_SWING_TICKS);
        }
        world
            .run_system_cached(crate::melee::server_swings::echo)
            .unwrap();
        world.run_system_cached(super::advance_actor_frame).unwrap();
        world.run_system_cached(prepare_actor_render_frame).unwrap();
        let packets = captured.drain();
        assert_eq!(packets.len(), usize::from(tick == 1));
        for packet in packets {
            assert_swing_echo(packet, "none");
        }
        let rig = world
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .actor_rig(1)
            .unwrap();
        assert_eq!(
            rig.java.swing,
            java.published_progress(tick).java,
            "server swing at tick {tick} must retain the Java restart guard"
        );
    }
}

/// Checks every serverbound swing field after encoding and decoding the captured packet.
fn assert_swing_echo(packet: protocol::Packet, source: &str) {
    let mut packets = protocol::decode_batch(protocol::encode(&packet).unwrap()).unwrap();
    assert_eq!(packets.len(), 1);
    let Some(protocol::WorldEvent::ItemActor(protocol::ItemActorEvent::Action(action))) =
        protocol::into_world_event(packets.remove(0), 0).unwrap()
    else {
        panic!("expected a swing echo");
    };
    assert_eq!(action.actor_runtime_ids.as_ref(), &[1]);
    assert_eq!(action.kind, protocol::ActorActionKind::SwingArm);
    assert_eq!(action.data, 0.0);
    assert_eq!(action.swing_source.as_deref(), Some(source));
}

#[test]
fn server_swing_echo_uses_inventory_authority_before_the_next_tick() {
    for starts_with_authored_item in [true, false] {
        let mut world = custom_emotes::fixture();
        select_authored_swing(&mut world, 19);
        let authored = world
            .resource::<client_state::PlayerRuntime>()
            .selected_backing_stack()
            .unwrap()
            .clone();
        let slot = |stack| {
            InventoryEvent::Slot(InventorySlotEvent {
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
            })
        };
        if !starts_with_authored_item {
            world
                .resource_mut::<client_state::PlayerRuntime>()
                .inventory
                .ledger_mut()
                .apply(&slot(protocol::NetworkItemStack::empty()));
        }
        let mut movement = client_interaction::movement::MovementTicker::default();
        *movement = gameplay::test_support::survival_mining::ticker_with_ticks(4);
        world.insert_resource(movement);
        world.init_resource::<client_interaction::movement::LocalMovementEffectTimeline>();
        world.run_system_cached(super::advance_actor_frame).unwrap();
        let movement = world.resource::<client_interaction::movement::MovementTicker>();
        let (authority, tick) = (
            movement.interaction_authority_identity(),
            movement.completed_tick(),
        );
        let mut swings = client_interaction::melee::SwingTracker::default();
        let duration = if starts_with_authored_item {
            19
        } else {
            actor_animation::ACTOR_SWING_TICKS
        };
        swings.sync_ticks_for_item(authority, tick, &Default::default(), Some(duration as u32));
        assert!(swings.try_swing(tick - 3, duration));
        swings.published_progress(tick);
        world.insert_resource(swings);
        let (network, mut captured) = client_runtime::network::NetworkHandle::<
            crate::runtime::network::resource_packs::PackApplication,
        >::stub_capturing_packets();
        world.insert_resource(network);
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .inventory
            .enqueue_inventory_event(
                1,
                1,
                slot(if starts_with_authored_item {
                    protocol::NetworkItemStack::empty()
                } else {
                    authored
                }),
            )
            .unwrap();
        {
            let mut client = world.resource_mut::<ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            stream
                .submit(
                    stream.committed_sequence() + 1,
                    protocol::WorldEvent::ItemActor(protocol::ItemActorEvent::Action(
                        protocol::ActorActionEvent {
                            actor_runtime_ids: Arc::from([1]),
                            kind: protocol::ActorActionKind::SwingArm,
                            data: 0.0,
                            swing_source: Some("event".into()),
                        },
                    )),
                )
                .unwrap();
        }
        let mut app = App::new();
        crate::game_frame::configure_client_frame_schedule(&mut app);
        crate::melee::server_swings::configure(&mut app);
        app.add_systems(
            Update,
            crate::ui_runtime::drain_inventory_authority
                .in_set(client_state::ClientFrameSet::UiAuthority),
        );
        app.add_systems(
            Update,
            (move |movement: Res<client_interaction::movement::MovementTicker>| {
                assert_eq!(movement.completed_tick(), tick);
                let packets = captured.drain();
                assert_eq!(packets.len(), usize::from(starts_with_authored_item));
                for packet in packets {
                    assert_swing_echo(packet, "event");
                }
            })
            .in_set(client_state::ClientFrameSet::SemanticFinalize),
        );
        app.world_mut()
            .resource_mut::<bevy::ecs::schedule::Schedules>()
            .get_mut(Update)
            .unwrap()
            .run(&mut world);
    }
}
