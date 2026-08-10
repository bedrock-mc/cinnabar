use std::sync::Arc;

use protocol::{AudioEvent, PlayAudioEvent};

use super::*;
use crate::runtime::audio::drain_committed_audio;

fn audio_event(name: &str) -> WorldEvent {
    WorldEvent::Audio(AudioEvent::Play(PlayAudioEvent {
        name: Arc::from(name),
        position: [4, -5, 6],
        volume: 1.5,
        pitch: -2.0,
        loop_count: 17,
        server_sound_handle: Some(91),
    }))
}

#[test]
fn app_audio_seam_drains_each_committed_event_once_in_the_same_call() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, audio_event("first")).unwrap();
    stream.submit(2, audio_event("second")).unwrap();

    let mut forwarded = Vec::new();
    drain_committed_audio(&mut stream, |event| forwarded.push(event));
    assert_eq!(forwarded.len(), 2);
    assert_eq!((forwarded[0].sequence, forwarded[1].sequence), (1, 2));
    assert_eq!(stream.stats().committed_audio_events, 0);

    drain_committed_audio(&mut stream, |event| forwarded.push(event));
    assert_eq!(
        forwarded.len(),
        2,
        "drained audio must not accumulate in the app"
    );
}
