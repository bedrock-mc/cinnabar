use super::*;
use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;

#[test]
fn custom_emotes_keep_the_owned_identity_off_the_bedrock_packet_channel() {
    let mut h = Harness::new();
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    h.app.insert_resource(network);
    h.play();
    assert!(h.runtime().emotes().playback().is_some());
    assert!(
        captured.drain().is_empty(),
        "custom clips use the authenticated Cinnabar API"
    );
    h.play();
    assert!(captured.drain().is_empty());
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
