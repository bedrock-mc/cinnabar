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
    world.resource_scope(|world, mut ui: Mut<client_ui::ui_runtime::UiRuntime>| {
        let mut player = world.resource_mut::<crate::player_runtime::PlayerRuntime>();
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
    });
}

#[test]
fn final_publication_preserves_authored_swing_ticks_without_attack_input() {
    let authored_ticks = 19;
    let mut world = custom_emotes::fixture();
    select_authored_swing(&mut world, authored_ticks);
    world.init_resource::<crate::movement::MovementTicker>();
    world.init_resource::<crate::movement::LocalMovementEffectTimeline>();
    world.init_resource::<crate::melee::SwingTracker>();
    assert!(!world.contains_resource::<crate::semantic_controls::SemanticInputSnapshot>());

    for frame in 0..4_u64 {
        *world.resource_mut::<crate::movement::MovementTicker>() = {
            let mut ticker = crate::movement::MovementTicker::default();
            *ticker = gameplay::test_support::survival_mining::ticker_with_ticks(frame + 1);
            ticker
        };
        let mut effects = world.resource_mut::<crate::movement::LocalMovementEffectTimeline>();
        effects.begin_frame();
        MovementEffectSource::commit_successful_tick(&mut **effects);
        world
            .resource_mut::<Time<bevy::time::Real>>()
            .advance_by(Duration::from_millis(50));
        world.run_system_cached(super::advance_actor_frame).unwrap();
        assert_eq!(
            crate::melee::selected_attack_timing(
                world.resource::<crate::player_runtime::PlayerRuntime>(),
                world.resource::<ClientWorld>(),
            )
            .and_then(|timing| timing.swing_duration_ticks),
            Some(authored_ticks),
            "the selected stack must resolve its session component facts"
        );
        if frame == 0 {
            let ticker = world.resource::<crate::movement::MovementTicker>();
            let (authority, tick) = (
                ticker.interaction_authority_identity(),
                ticker.completed_tick(),
            );
            let mut tracker = world.resource_mut::<crate::melee::SwingTracker>();
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
            counters.map(|counter| counter / client_world::ACTOR_SWING_TICKS as f32),
            "the independent Java denominator remains effect-based"
        );
    }
}
