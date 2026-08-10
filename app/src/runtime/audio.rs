use bevy::prelude::Message;
use client_world::WorldStream;

/// App-facing audio transport seam. Playback and sound resolution intentionally
/// live downstream of this packet-preserving ingress message.
#[derive(Debug, Clone, PartialEq, Message)]
pub(crate) struct SequencedAudioEvent {
    pub(crate) sequence: u64,
    pub(crate) event: protocol::AudioEvent,
}

pub(crate) fn drain_committed_audio(
    stream: &mut WorldStream,
    mut forward: impl FnMut(SequencedAudioEvent),
) {
    for committed in stream.take_committed_audio() {
        forward(SequencedAudioEvent {
            sequence: committed.sequence,
            event: committed.event,
        });
    }
}
