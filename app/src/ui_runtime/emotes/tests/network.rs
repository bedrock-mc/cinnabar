use super::*;
use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;

#[test]
fn each_emote_start_sends_once_on_its_selection_frame() {
    let mut h = Harness::new();
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    h.app.insert_resource(network);
    h.play();
    let packets = captured.drain();
    assert_eq!(packets.len(), 1);
    let packet = protocol::test_support::packets::emote(&packets[0])
        .expect("selection must send an emote start immediately");
    let emote = h.runtime().emotes().playback().unwrap().emote;
    assert_eq!(packet.actor_runtime_id, 42);
    assert_eq!(packet.emote_id, emote.id());
    assert_eq!(
        f64::from(packet.length_ticks),
        (emote.duration_seconds() / simulation_clock::TICK_DURATION.as_secs_f64()).ceil()
    );
    assert_eq!(packet.flags, 0);
    h.app.update();
    assert!(captured.drain().is_empty());
    // Even a restart at the same clock value is a distinct start, not a pose refresh.
    h.play();
    assert_eq!(captured.drain().len(), 1);
}

#[test]
fn closing_the_wheel_without_a_selection_sends_no_emote() {
    let mut h = Harness::new();
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    h.app.insert_resource(network);
    h.press(KeyCode::KeyB);
    h.press(KeyCode::Escape);
    assert!(h.runtime().emotes().playback().is_none());
    assert!(captured.drain().is_empty());
}
