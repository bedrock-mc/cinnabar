use bevy::prelude::Message;
use chunk_pipeline::WorldStream;

/// App-facing audio transport seam. Playback and sound resolution intentionally
/// live downstream of this packet-preserving ingress message.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct SequencedAudioEvent {
    /// Immutable local WorldStream lifetime, not a sampled clock or account identity.
    pub origin_stream_session_id: u64,
    pub sequence: u64,
    pub dimension: i32,
    pub dimension_epoch: u64,
    pub actor_synchronization: Option<client_world::ActorLifetimeId>,
    pub event: protocol::AudioEvent,
}

/// Forwards each committed audio envelope once, preserving its origin and transport identity.
pub fn drain_committed_audio(
    stream: &mut WorldStream,
    mut forward: impl FnMut(SequencedAudioEvent),
) {
    let origin_stream_session_id = stream.authority().actor_session_id();
    for committed in stream.take_committed_audio() {
        forward(SequencedAudioEvent {
            origin_stream_session_id,
            sequence: committed.sequence,
            dimension: committed.dimension,
            dimension_epoch: committed.dimension_epoch,
            actor_synchronization: committed.actor_synchronization,
            event: committed.event,
        });
    }
}
