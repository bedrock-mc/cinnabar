use super::*;

#[test]
fn an_instant_mining_press_retains_its_press_and_completion_swings() {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_snowball(&mut world);
    world.init_resource::<crate::melee::MeleeTarget>();
    world.init_resource::<client_interaction::survival_mining::SurvivalMiningRuntime>();
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        let network = world.resource::<NetworkHandle<PackApplication>>();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut movement,
            gameplay::movement::OUTBOX_CAPACITY,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| network.send_physics_packet(identity, packet, guard),
        )
        .unwrap();
    });
    captured.drain();
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
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    assert_eq!(
        world
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        1,
        "the press must complete its instant break"
    );
    let swing = protocol::swing_arm_packet(42, protocol::SwingSource::Mine).kind();
    let count = captured
        .drain()
        .iter()
        .filter(|packet| packet.kind() == swing)
        .count();
    assert_eq!(count, 2, "the press and its instant completion each swing");
}
