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
    /// A client actor lifecycle request, paired with packet audio by the echo ledger.
    pub client_generated: bool,
    pub actor_synchronization: Option<actor_animation::ActorLifetimeId>,
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
            client_generated: committed.client_generated,
            actor_synchronization: committed.actor_synchronization,
            event: committed.event,
        });
    }
}
