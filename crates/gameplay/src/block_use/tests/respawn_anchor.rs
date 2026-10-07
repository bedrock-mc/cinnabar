use protocol::wire::valentine::bedrock::version::v1_26_51::{
    EnumsItemUseInventoryTransactionActionType, EnumsItemUseInventoryTransactionPredictedResult,
    EnumsItemUseInventoryTransactionTriggerType, InventoryTransactionPacketTransaction,
    McpePacketData,
};

use super::{
    GameModeCapabilities, ItemUseTrigger, LocalUse, PlayerGameMode, UseSurroundings, network_item,
    surroundings, toggled_states, use_packets, verified,
};
use protocol::NetworkItemStack;

/// The clicked anchor and held glowstone facts, without any installed assets.
fn anchor(charge: i32, glowstone: bool) -> UseSurroundings {
    UseSurroundings {
        clicked_canonical_state: Some(format!(
            r#"{{"respawn_anchor_charge":{{"type":"int","value":{charge}}}}}"#
        )),
        held_block_identifier: glowstone.then(|| "minecraft:glowstone".to_owned()),
        ..surroundings("minecraft:respawn_anchor", "minecraft:air")
    }
}

/// Charging below full and activation at full never predict adjacent glowstone.
#[test]
fn glowstone_uses_anchor_at_every_charge_without_a_dimension_gate() {
    let glowstone = verified(network_item(2, 77));
    let caps = GameModeCapabilities::for_mode(PlayerGameMode::Survival);
    for charge in 0..=4 {
        let around = anchor(charge, true);
        assert_eq!(
            LocalUse::resolve(&glowstone, [2, 63, 0], 1, &around, &caps),
            LocalUse::Interact,
            "charge {charge}: server charges in either dimension, then activates at full"
        );
        assert_eq!(
            toggled_states(
                around.clicked_identifier.as_deref().unwrap(),
                around.clicked_canonical_state.as_deref().unwrap(),
            ),
            None,
            "charge and explosion are server-authoritative; no local block mutation"
        );
    }
    let sneaking = UseSurroundings {
        sneaking: true,
        ..anchor(0, true)
    };
    assert_eq!(
        LocalUse::resolve(&glowstone, [2, 63, 0], 1, &sneaking, &caps),
        LocalUse::Place,
        "sneaking with an item bypasses block use"
    );
}

/// Glowstone dust cannot charge; an uncharged anchor permits the usual item fallback.
#[test]
fn uncharged_anchor_does_not_consume_other_block_items() {
    let block = verified(network_item(3, 78));
    let caps = GameModeCapabilities::for_mode(PlayerGameMode::Survival);
    let around = UseSurroundings {
        held_block_identifier: Some("minecraft:stone".to_owned()),
        ..anchor(0, false)
    };
    assert_eq!(
        LocalUse::resolve(&block, [2, 63, 0], 1, &around, &caps),
        LocalUse::Place
    );
    let dust = verified(network_item(4, 0));
    assert_eq!(
        LocalUse::resolve(&dust, [2, 63, 0], 1, &anchor(0, false), &caps),
        LocalUse::Nothing
    );
    assert_eq!(
        LocalUse::resolve(&block, [2, 63, 0], 1, &anchor(1, false), &caps),
        LocalUse::Interact,
        "a charged anchor activates before block placement"
    );
    let empty = verified(NetworkItemStack::empty());
    assert_eq!(
        LocalUse::resolve(&empty, [2, 63, 0], 1, &anchor(0, false), &caps),
        LocalUse::Nothing
    );
    let charged = UseSurroundings {
        clicked_canonical_state: Some(r#"{"respawn_anchor_charge":4}"#.to_owned()),
        sneaking: true,
        ..anchor(4, false)
    };
    assert_eq!(
        LocalUse::resolve(&empty, [2, 63, 0], 1, &charged, &caps),
        LocalUse::Interact,
        "sneaking with an empty hand still activates a charged anchor"
    );
}

/// Anchor charging uses the ordinary click-block transaction with success prediction,
/// the clicked anchor runtime id and the untouched held glowstone descriptor.
#[test]
fn anchor_use_sends_click_block_success_with_the_clicked_anchor() {
    let glowstone = verified(network_item(2, 77));
    let caps = GameModeCapabilities::for_mode(PlayerGameMode::Survival);
    let observed = crate::interaction_authority::FrozenBlockObservation::fixture(
        [2, 63, 0],
        1,
        glowstone.clone(),
    );
    for charge in 0..=4 {
        let outcome = LocalUse::resolve(&glowstone, [2, 63, 0], 1, &anchor(charge, true), &caps);
        for (trigger, expected_trigger) in [
            (
                ItemUseTrigger::PlayerInput,
                EnumsItemUseInventoryTransactionTriggerType::Playerinput,
            ),
            (
                ItemUseTrigger::SimulationTick,
                EnumsItemUseInventoryTransactionTriggerType::Simulationtick,
            ),
        ] {
            let packets = use_packets(
                (&observed, 3_219),
                [0.5, 65.62, 0.5],
                trigger,
                outcome,
                None,
                None,
                42,
                |_| true,
                101,
            );
            assert_eq!(packets.len(), 2, "successful interaction swings first");
            let McpePacketData::InventoryTransactionPacket(packet) = &packets[1].data else {
                panic!("click-block transaction");
            };
            let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction) =
                &packet.transaction
            else {
                panic!("item use transaction");
            };
            assert_eq!(
                transaction.action_type,
                EnumsItemUseInventoryTransactionActionType::Place
            );
            assert_eq!(transaction.trigger_type, expected_trigger);
            assert_eq!(
                transaction.client_interact_prediction,
                EnumsItemUseInventoryTransactionPredictedResult::Success
            );
            assert_eq!(transaction.target_block_id, 3_219);
            assert_eq!(transaction.position.x, observed.target.position[0]);
            assert_eq!(i32::from(transaction.item.id), glowstone.network_id());
            assert_eq!(transaction.item.stacksize, glowstone.count());
            assert_eq!(transaction.item.block_runtime_id, 77);
            assert!(transaction.actions.actions.is_empty());
        }
    }
}
