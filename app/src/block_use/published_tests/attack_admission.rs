//! Cached actor points retain the ordinary attack admission gates.

use super::attack_hit_position_tests::{
    actor_pick_bounds, attack, attack_fixture, unpublished_attack_fixture,
};
use super::*;
use client_presentation::aim_assist::{AimAssistFrame, AimAssistTarget, TargetKind};
use protocol::test_support::transactions::observe_transaction;

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
        .run_system_cached(crate::survival_mining::produce_survival_mining)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_melee)
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
    world.resource_mut::<AimAssistFrame>().target = Some(AimAssistTarget {
        kind: TargetKind::Actor(99),
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
        .run_system_cached(crate::melee::produce_melee)
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
        "a deferred actor press must not break the block behind it"
    );
    assert!(captured.drain().is_empty(), "the uncached press must wait");
    press(&mut world, semantic_input::InputMode::KeyboardMouse);
    produce(&mut world);
    assert_eq!(
        actor_attacks(&mut captured).len(),
        1,
        "the same deferred press resolves from its published actor pick"
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
            .run_system_cached(crate::melee::produce_melee)
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
            "native use waits for the deferred attack; the explicit override permits it"
        );
    }
}

#[test]
fn attack_admission_expired_uncached_press_still_does_not_mine_through_an_actor() {
    let (mut world, mut captured) = unpublished_attack_fixture(7.5);
    world.init_resource::<client_interaction::survival_mining::SurvivalMiningRuntime>();
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    press(&mut world, semantic_input::InputMode::KeyboardMouse);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(
            gameplay::interaction_authority::MAX_PENDING_INTERACTION_MILLIS + 1,
        ));
    world.insert_resource(crate::melee::MeleeTarget::default());
    press(&mut world, semantic_input::InputMode::KeyboardMouse);
    produce(&mut world);
    assert_eq!(
        world
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        0
    );
    assert!(captured.drain().is_empty());
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
    world.insert_resource(crate::modding::interaction::ModInteraction {
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
