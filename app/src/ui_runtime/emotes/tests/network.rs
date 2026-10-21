use super::*;
use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;

#[test]
fn custom_emotes_keep_the_owned_identity_and_state_off_bedrock() {
    let _catalog = cosmetics::test_support::playback_catalog();
    let mut h = Harness::new();
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    h.app.insert_resource(network);
    let mut movement = client_interaction::movement::MovementTicker::default();
    movement.reset(
        1,
        100,
        gameplay::test_support::survival_mining::completed(101).position,
    );
    movement.set_source(gameplay::movement::MovementSource::Physics);
    h.app.insert_resource(movement);
    for tick in [101, 102] {
        h.play();
        assert!(h.runtime().emotes().playback().is_some());
        assert!(
            captured.drain().is_empty(),
            "custom clips use the authenticated Cinnabar API"
        );
        let mut movement = h
            .app
            .world_mut()
            .resource_mut::<client_interaction::movement::MovementTicker>();
        movement
            .enqueue_completed_physics(gameplay::test_support::survival_mining::completed(tick))
            .unwrap();
        let snapshot = *movement.pending_snapshots().last().unwrap();
        assert_eq!(snapshot.tick, tick);
        let packet = protocol::player_auth_input(snapshot).unwrap();
        let decoded = protocol::decode_batch(protocol::encode(&packet).unwrap()).unwrap();
        let trace = protocol::player_auth_input_trace_sample(&decoded[0]).unwrap();
        assert!(
            !trace.flag_names.contains(&"Emoting"),
            "render-only custom playback must not announce a native emote"
        );
    }
}

#[test]
fn closing_the_wheel_without_a_selection_sends_no_emote() {
    let _catalog = cosmetics::test_support::playback_catalog();
    let mut h = Harness::new();
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    h.app.insert_resource(network);
    h.press(KeyCode::KeyB);
    h.press(KeyCode::Escape);
    assert!(h.runtime().emotes().playback().is_none());
    assert!(captured.drain().is_empty());
}
