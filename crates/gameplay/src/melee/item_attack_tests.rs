use super::*;
use protocol::wire::valentine::bedrock::version::v1_26_51::{
    EnumsItemUseInventoryTransactionActionType as Action,
    EnumsItemUseInventoryTransactionClientCooldownState as Cooldown,
    InventoryTransactionPacketTransaction, ItemUseInventoryTransaction, McpePacketData,
};

/// Supplies verified selected-stack identity and a sampled item-directed attack.
fn press(tick: u64) -> PressContext {
    let stack = protocol::NetworkItemStack::empty();
    PressContext {
        tick,
        player_position: [10.0, 50.0, -3.0],
        input_mode: PlayerInputMode::Mouse,
        local_runtime_id: 1,
        selection: Some(FrozenMiningSelection {
            slot: 4,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        }),
        swing_duration: 19,
        now_millis: tick * 50,
        item_attack: Some(ItemAttackPress {
            direction: [0.0, 0.0, -1.0],
            cooldown: Some(protocol::ItemAttackCooldown {
                category: "test:shared".into(),
                ticks: 19,
            }),
        }),
    }
}

/// Extracts the item transaction instead of accepting an actor attack as equivalent.
fn transaction(outcome: &MeleeOutcome) -> &ItemUseInventoryTransaction {
    let McpePacketData::InventoryTransactionPacket(packet) = &outcome.packets.last().unwrap().data
    else {
        panic!("item attack transaction missing");
    };
    let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction) =
        &packet.transaction
    else {
        panic!("item attack incorrectly routed through actor use");
    };
    transaction
}

#[test]
fn piercing_attacks_report_aim_for_air_actor_and_block_targets_without_mining() {
    for crosshair in [
        Crosshair::Miss,
        Crosshair::Block,
        Crosshair::Actor(ActorHit {
            runtime_id: 8,
            distance: 2.0,
            point: [10.0, 51.0, -5.0],
        }),
    ] {
        let mut runtime = MeleeRuntime::default();
        runtime.observe_input(true, true);
        let outcome = runtime.resolve(crosshair, &press(100), &mut SwingTracker::default());
        let transaction = transaction(&outcome);
        assert_eq!(transaction.action_type, Action::Useasattack);
        assert_eq!(transaction.slot, 4);
        assert_eq!(
            [
                transaction.from_position.x,
                transaction.from_position.y,
                transaction.from_position.z
            ],
            [10.0, 50.0, -3.0]
        );
        assert_eq!(
            [
                transaction.click_position.x,
                transaction.click_position.y,
                transaction.click_position.z
            ],
            [10.0, 50.0, -4.0]
        );
        assert_eq!(transaction.client_cooldown_state, Cooldown::Off);
        assert!(protocol::is_aim_assist_rotation_action(
            outcome.packets.last().unwrap()
        ));
        assert!(!outcome.missed_swing);
        assert!(runtime.actor_in_front(), "piercing attacks veto mining");
    }
    let mut runtime = MeleeRuntime::default();
    runtime.observe_input(true, true);
    let block = runtime.resolve(Crosshair::Block, &press(100), &mut SwingTracker::default());
    assert_eq!(transaction(&block).action_type, Action::Useasattack);
    runtime.observe_input(false, true);
    assert_eq!(
        runtime.observe_attack_target(Crosshair::Block, true),
        Crosshair::Miss
    );
    assert!(
        runtime.actor_in_front(),
        "holding attack still suppresses mining"
    );
    runtime.observe_attack_target(Crosshair::Block, false);
    assert!(
        !runtime.actor_in_front(),
        "an ordinary item restores mining"
    );
}

#[test]
fn shared_attack_cooldown_expires_on_its_tick_and_full_queue_does_not_start_it() {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.observe_input(true, true);
    resolve_and_send(
        &mut runtime,
        &mut swings,
        Crosshair::Miss,
        &press(100),
        5000,
        |_| Err(BatchSendError::Full),
    );
    assert!(runtime.attack_cooldowns.is_empty());
    assert_eq!(swings.take_started(), None);
    resolve_and_send(
        &mut runtime,
        &mut swings,
        Crosshair::Miss,
        &press(100),
        5001,
        |packets| {
            assert_eq!(packets.len(), 2);
            Ok(())
        },
    );
    assert_eq!(runtime.attack_cooldowns.len(), 1);
    for (tick, expected, packet_count) in [(118, Cooldown::On, 1), (119, Cooldown::Off, 2)] {
        runtime.observe_input(true, true);
        let outcome = runtime.resolve(Crosshair::Miss, &press(tick), &mut swings);
        assert_eq!(transaction(&outcome).client_cooldown_state, expected);
        assert_eq!(outcome.packets.len(), packet_count);
    }
    runtime.synchronize((1, 1));
    runtime.synchronize((1, 2));
    assert_eq!(
        runtime.attack_cooldowns.len(),
        1,
        "teleport keeps item category timers"
    );
    runtime.synchronize((2, 1));
    assert!(runtime.attack_cooldowns.is_empty());
}

#[test]
fn invalid_item_attack_aim_neither_swings_nor_consumes_a_cooldown() {
    let mut runtime = MeleeRuntime::default();
    runtime.observe_input(true, true);
    let mut input = press(100);
    input.item_attack.as_mut().unwrap().direction[0] = f32::NAN;
    let outcome = runtime.resolve(Crosshair::Miss, &input, &mut SwingTracker::default());
    assert!(outcome.packets.is_empty());
    assert!(runtime.attack_cooldowns.is_empty());
}
