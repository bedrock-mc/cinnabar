//! Attack field sampling stays distinct from the retained target's hit point.

use super::attack_hit_position_tests::{attack, attack_fixture};
use super::*;
use gameplay::movement::MovementEffectSource;
use protocol::test_support::transactions::observe_transaction;

/// Drains completed inputs through the same guarded session queue as gameplay.
fn flush_movement(world: &mut World, limit: usize) {
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        let network = world.resource::<NetworkHandle<PackApplication>>();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut movement,
            limit,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| network.send_physics_packet(identity, packet, guard),
        )
        .unwrap();
    });
}

/// Records distinct completed poses and their successful ticks in one frame.
fn catch_up(world: &mut World, count: usize) {
    world
        .resource_mut::<LocalMovementEffectTimeline>()
        .begin_frame();
    for offset in 0..count {
        let mut sample = gameplay::test_support::survival_mining::completed(102 + offset as u64);
        sample.position = [4.5 + (offset + 1) as f32 * 0.25, 2.620_01, 8.5];
        world
            .resource_mut::<MovementTicker>()
            .enqueue_completed_physics(sample)
            .unwrap();
        world
            .resource_mut::<LocalMovementEffectTimeline>()
            .commit_successful_tick();
    }
}

#[test]
fn attack_fields_use_the_pose_before_catchup_and_precede_its_inputs() {
    for ticks in [1, 3] {
        let (mut world, mut captured) = attack_fixture(7.5);
        catch_up(&mut world, ticks);
        attack(&mut world);
        flush_movement(&mut world, gameplay::movement::OUTBOX_CAPACITY);
        let packets = captured.drain();
        let tx = packets
            .first()
            .and_then(observe_transaction)
            .expect("attack before catch-up inputs");
        assert_eq!(tx.actor_use().unwrap().from_position, [4.5, 2.620_01, 8.5]);
        assert_eq!(
            packets[1].kind(),
            protocol::swing_arm_packet(42, protocol::SwingSource::Attack).kind()
        );
        let inputs: Vec<_> = packets
            .iter()
            .filter_map(protocol::player_auth_input_trace_sample)
            .collect();
        assert_eq!(inputs.len(), ticks);
        for (offset, input) in inputs.iter().enumerate() {
            assert_eq!(input.tick, 102 + offset as u64);
            assert_eq!(input.position[0], 4.5 + (offset + 1) as f32 * 0.25);
        }
    }
}

#[test]
fn attack_fields_do_not_overtake_movement_from_an_older_frame() {
    let (mut world, mut captured) = attack_fixture(7.5);
    catch_up(&mut world, 3);
    world
        .resource_mut::<LocalMovementEffectTimeline>()
        .begin_frame();
    attack(&mut world);
    assert!(captured.drain().is_empty());
    flush_movement(&mut world, 1);
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    let first = captured.drain();
    assert_eq!(first.len(), 1);
    assert!(protocol::player_auth_input_trace_sample(&first[0]).is_some());
    flush_movement(&mut world, gameplay::movement::OUTBOX_CAPACITY);
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    let packets = captured.drain();
    let tx = packets.iter().find_map(observe_transaction).unwrap();
    assert_eq!(tx.actor_use().unwrap().from_position, [5.25, 2.620_01, 8.5]);
    assert_eq!(packets.len(), 4);
    assert!(protocol::player_auth_input_trace_sample(&packets[1]).is_some());
    assert!(observe_transaction(&packets[2]).is_some());
}

#[test]
fn attack_fields_wait_for_a_previously_held_release() {
    let (mut world, mut captured) = attack_fixture(7.5);
    catch_up(&mut world, 1);
    world
        .resource_mut::<MovementTicker>()
        .hold_release_after_tick(
            102,
            vec![protocol::swing_arm_packet(
                42,
                protocol::SwingSource::ThrowItem,
            )],
        );
    attack(&mut world);
    assert!(captured.drain().is_empty());
}

#[test]
fn attack_fields_sample_the_current_slot_and_full_held_descriptor() {
    let (mut world, mut captured) = attack_fixture(7.5);
    item_use_tests::hold_snowball(&mut world);
    let stack = world
        .resource::<client_state::PlayerRuntime>()
        .inventory
        .ledger()
        .displayed_stack(0)
        .unwrap()
        .clone();
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        ui.inventory_ledger_mut(&mut player)
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(0),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    slot: 3,
                },
                stack,
                storage_item: None,
            }));
        player.inventory.set_local_selected_slot(3);
    });
    attack(&mut world);
    let packets = captured.drain();
    let encoded = protocol::encode(&packets[0]).unwrap();
    let decoded = protocol::decode_batch(encoded).unwrap();
    let tx = decoded.iter().find_map(observe_transaction).unwrap();
    let actor = tx.actor_use().unwrap();
    assert_eq!(actor.action, Some(protocol::ActorUseAction::Attack));
    assert_eq!((actor.runtime_id, actor.slot), (99, 3));
    assert_eq!(
        (actor.item.network_id, actor.item.count, actor.item.metadata),
        (300, 16, 0)
    );
    assert_eq!(actor.item.stack_network_id, None);
    assert_eq!(actor.item.block_runtime_id, 0);
    assert_eq!(tx.legacy_request_id, 0);
    assert!(tx.legacy_set_slots.is_none());
    assert!(tx.actions.is_empty());
    let player = world.resource::<client_state::PlayerRuntime>();
    let selected = player.inventory.ledger().displayed_stack(3).unwrap();
    assert_eq!(selected.stack_network_id, 5);
}

#[test]
fn attack_fields_keep_the_network_anchor_when_the_visual_eye_lowers() {
    for eye_height in [protocol::PLAYER_NETWORK_OFFSET, 1.270_01, 0.4] {
        let (mut world, mut captured) = attack_fixture(7.5);
        let origin = world.resource::<InteractionOriginSnapshot>();
        let ray = origin.outbound_ray().unwrap();
        let feet = Vec3::new(4.5, 1.0, 8.5);
        let eye = feet + Vec3::Y * eye_height;
        let mut carrier = client_presentation::local_player::LocalPlayerFrameCarrier::default();
        carrier
            .publish(client_presentation::local_player::LocalPlayerFrameSample {
                session_generation: ray.session_generation(),
                actor_session_id: ray.actor_session_id(),
                fifo_sequence: ray.fifo_sequence(),
                physics_tick: ray.physics_tick(),
                perspective: ray.perspective(),
                world_collision_identity: ray.world_collision_identity().clone(),
                pose: Transform::from_translation(eye),
                eye,
                feet,
                rotation: Quat::IDENTITY,
            })
            .unwrap();
        world
            .resource_mut::<InteractionOriginSnapshot>()
            .publish_from_local_player_frame(&carrier);
        attack(&mut world);
        let packets = captured.drain();
        let tx = packets.iter().find_map(observe_transaction).unwrap();
        assert_eq!(tx.actor_use().unwrap().from_position, [4.5, 2.620_01, 8.5]);
    }
}
