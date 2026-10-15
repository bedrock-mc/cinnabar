//! Physical release eligibility is independent of movement FIFO admission.

use super::respawn::{charging_world, use_button};
use super::*;
use client_interaction::item_use::ItemUseRuntime;

#[path = "backpressure/retained_fence.rs"]
mod retained_fence;

#[path = "backpressure/release_facing.rs"]
mod release_facing;

struct Dry;

impl sim::CollisionWorld for Dry {
    /// Keeps the liquid-free control unconstrained by terrain.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// Publishes a successful fixed tick with the requested current wetness.
fn advance(world: &mut World, wet: bool) -> gameplay::movement::PhysicsMovementSample {
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    let sample = {
        let mut physics = world.resource_mut::<LocalPhysicsController>();
        let frame = if wet {
            physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &Water,
            )
        } else {
            physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &Dry,
            )
        };
        assert!(frame.blocked.is_none());
        assert_eq!(physics.in_water(), wet);
        frame.samples.into_iter().next().unwrap()
    };
    world
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(sample.clone())
        .unwrap();
    sample
}

/// Runs both real producer phases while leaving the movement FIFO undrained.
fn produce(world: &mut World) {
    world
        .run_system_cached(crate::item_use::prepare_item_release)
        .unwrap();
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
}

/// Refuses movement admission at the real FIFO boundary without publishing any input.
fn refuse_fifo(world: &mut World) {
    let mut movement = world.resource_mut::<MovementTicker>();
    let before = movement.pending_snapshots();
    assert!(!before.is_empty());
    let result = gameplay::movement::flush_player_auth_inputs(
        &mut movement,
        gameplay::movement::OUTBOX_CAPACITY,
        Some(gameplay::test_support::survival_mining::evidence()),
        |_, _| Err::<(), ()>(()),
    );
    assert!(result.is_err());
    assert_eq!(movement.pending_snapshots(), before);
}

/// Starts an ordinary release at the same completed tick with or without queued movement.
fn release_edge(charge: u64, wet: bool, blocked: bool) -> (World, client_session::CapturedPackets) {
    let (mut world, mut captured) = charging_world(charge - 1);
    assert!(
        world
            .resource::<client_presentation::aim_assist::AimAssistFrame>()
            .interaction_direction()
            .is_none()
    );
    advance(&mut world, wet);
    if blocked {
        refuse_fifo(&mut world);
    } else {
        flush_and_acknowledge(&mut world);
    }
    captured.drain();
    use_button(&mut world, false);
    world
        .resource_mut::<LocalViewPose>()
        .set_rotation(Quat::from_rotation_y(90_f32.to_radians()));
    produce(&mut world);
    assert!(captured.drain().is_empty());
    assert_eq!(world.resource::<ItemUseRuntime>().is_using(), blocked);
    assert_eq!(
        world.resource::<MovementTicker>().has_held_release(),
        !blocked
    );
    assert!(
        world
            .resource::<LocalPhysicsController>()
            .state()
            .unwrap()
            .spin_attack_ticks
            .is_none()
    );
    (world, captured)
}

/// Drains the old FIFO, verifies release-facing order, and observes only the original launch.
fn finish(world: &mut World, captured: &mut client_session::CapturedPackets, wet: bool) -> bool {
    flush_and_acknowledge(world);
    produce(world);
    assert!(world.resource::<MovementTicker>().has_held_release());
    let facing = advance(world, wet);
    assert!(!facing.processed.spin_started);
    flush_and_acknowledge(world);
    produce(world);
    let packets = captured.drain();
    let inputs: Vec<_> = packets
        .iter()
        .enumerate()
        .filter_map(|(index, packet)| {
            protocol::player_auth_input_trace_sample(packet).map(|sample| (index, sample))
        })
        .collect();
    let (facing_index, input) = inputs
        .iter()
        .find(|(_, sample)| sample.tick == facing.tick)
        .unwrap();
    assert!((input.yaw - 90.0).abs() < 1e-3);
    assert!(!input.flag_names.contains(&"StartSpinAttack"));
    let releases: Vec<_> = packets
        .iter()
        .enumerate()
        .filter_map(|(index, packet)| {
            protocol::test_support::transactions::observe_transaction(packet)
                .is_some_and(|transaction| transaction.release().is_some())
                .then_some(index)
        })
        .collect();
    assert_eq!(releases.len(), 1);
    assert!(*facing_index < releases[0]);
    assert!(!world.resource::<MovementTicker>().has_held_release());
    advance(world, wet).processed.spin_started
}

/// Selects the same verified item in another slot to invalidate the old accepted use.
fn select_other_slot(world: &mut World) {
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let mut stack = player.selected_stack().unwrap().clone();
        stack.stack_network_id += 1;
        ui.inventory_ledger_mut(&mut player)
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(0),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    slot: 1,
                },
                stack,
                storage_item: None,
            }));
        player.inventory.set_local_selected_slot(1);
    });
}

#[test]
fn ordinary_riptide_fifo_backpressure_keeps_original_charge_and_wetness() {
    for charge in [
        gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1,
        gameplay::item_use::RIPTIDE_CHARGE_TICKS,
    ] {
        for (wet_edge, wet_later) in [(true, true), (true, false), (false, true), (false, false)] {
            let (mut reference, mut reference_packets) = release_edge(charge, wet_edge, false);
            let expected = finish(&mut reference, &mut reference_packets, wet_later);
            assert_eq!(
                expected,
                wet_edge && charge == gameplay::item_use::RIPTIDE_CHARGE_TICKS
            );
            let (mut world, mut captured) = release_edge(charge, wet_edge, true);
            let slowdown = world.resource::<ItemUseRuntime>().movement_modifier();
            assert!(slowdown.is_some());
            for _ in 0..3 {
                assert!(!advance(&mut world, wet_later).processed.spin_started);
                refuse_fifo(&mut world);
                produce(&mut world);
                assert_eq!(
                    world.resource::<ItemUseRuntime>().movement_modifier(),
                    slowdown
                );
                assert!(!world.resource::<MovementTicker>().has_held_release());
                assert!(captured.drain().is_empty());
            }
            assert_eq!(finish(&mut world, &mut captured, wet_later), expected);
        }
    }
}

#[test]
fn ordinary_riptide_selection_change_cancels_a_release_while_fifo_waits() {
    for switch_back in [false, true] {
        let (mut world, mut captured) =
            release_edge(gameplay::item_use::RIPTIDE_CHARGE_TICKS, true, true);
        select_other_slot(&mut world);
        produce(&mut world);
        assert!(!world.resource::<ItemUseRuntime>().is_using());
        if switch_back {
            world
                .resource_mut::<client_state::PlayerRuntime>()
                .inventory
                .set_local_selected_slot(0);
        }
        for _ in 0..3 {
            assert!(!advance(&mut world, true).processed.spin_started);
            produce(&mut world);
            assert!(!world.resource::<MovementTicker>().has_held_release());
            assert!(captured.drain().is_empty());
        }
        flush_and_acknowledge(&mut world);
        produce(&mut world);
        assert!(
            captured
                .drain()
                .iter()
                .all(|packet| protocol::player_auth_input_trace_sample(packet).is_some())
        );
        assert!(!advance(&mut world, true).processed.spin_started);
    }
}

#[test]
fn ordinary_riptide_repress_does_not_upgrade_an_owed_early_release() {
    let (mut world, mut captured) =
        release_edge(gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1, true, true);
    use_button(&mut world, true);
    for _ in 0..3 {
        assert!(!advance(&mut world, true).processed.spin_started);
        produce(&mut world);
        assert!(world.resource::<ItemUseRuntime>().is_using());
        assert!(captured.drain().is_empty());
    }
    assert!(!finish(&mut world, &mut captured, true));
}

#[test]
fn ordinary_riptide_new_release_is_observed_while_an_earlier_release_waits_for_facing() {
    let (mut world, mut captured) =
        release_edge(gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1, true, true);
    use_button(&mut world, true);
    for _ in 0..3 {
        advance(&mut world, true);
        produce(&mut world);
    }
    flush_and_acknowledge(&mut world);
    captured.drain();
    produce(&mut world);
    assert!(world.resource::<MovementTicker>().has_held_release());
    assert!(
        world.resource::<ItemUseRuntime>().is_using(),
        "the repress begins a fresh use behind the old release"
    );
    use_button(&mut world, false);
    produce(&mut world);
    assert!(captured.drain().is_empty());
    for _ in 0..=gameplay::item_use::RIPTIDE_CHARGE_TICKS {
        assert!(!advance(&mut world, true).processed.spin_started);
        produce(&mut world);
        assert!(captured.drain().is_empty());
    }
    flush_and_acknowledge(&mut world);
    produce(&mut world);
    let packets = captured.drain();
    assert_eq!(
        packets
            .iter()
            .filter(|packet| {
                protocol::test_support::transactions::observe_transaction(packet)
                    .is_some_and(|transaction| transaction.release().is_some())
            })
            .count(),
        1,
        "only the older release can leave at its facing boundary"
    );
    assert!(!finish(&mut world, &mut captured, true));
}
