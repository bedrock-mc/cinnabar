//! Borrows session identity for the presentation audio lane.
use bevy::prelude::*;
use client_audio::audio_ingress::SequencedAudioEvent;
use client_audio::session_audio::{SessionAudio, SessionAudioCatalog};

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drain_sequenced_audio_into_session(
    messages: MessageReader<SequencedAudioEvent>,
    clock: Res<client_presentation::environment::WorldClock>,
    client_world: Res<crate::runtime::world::ClientWorld>,
    catalog: Res<SessionAudioCatalog>,
    #[cfg(feature = "acceptance")] mut wire_messages: MessageReader<SequencedAudioEvent>,
    #[cfg(feature = "acceptance")] mut wire: Option<ResMut<acceptance::audio_wire::WireEvidence>>,
    session: ResMut<SessionAudio>,
) {
    #[cfg(feature = "acceptance")]
    if let Some(wire) = wire.as_mut() {
        let stream_id = client_world
            .stream
            .as_ref()
            .map(|stream| stream.authority().actor_session_id());
        wire.bind(stream_id);
        if let Some(stream_id) = stream_id {
            for event in wire_messages.read() {
                if event.origin_stream_session_id == stream_id {
                    wire.emit(stream_id, event);
                }
            }
        } else {
            wire_messages.clear();
        }
    }
    client_audio::session_audio::drain_sequenced_audio_into_session(
        messages,
        client_audio::observations::SessionObservation(clock.session_generation()),
        client_audio::observations::WorldObservation {
            stream: client_world.stream.as_ref(),
        },
        catalog,
        session,
    );
}

#[cfg(all(test, feature = "acceptance"))]
#[path = "session_audio/wire_evidence_tests.rs"]
mod wire_evidence_tests;
