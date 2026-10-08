use bevy::prelude::*;

use crate::runtime::world::ClientWorld;
use client_presentation::audio_ingress::SequencedAudioEvent;

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        drain_actor_audio
            .after(crate::app::ClientFrameSet::ActorPreparation)
            .before(crate::named_audio::drain_live_named_audio),
    );
}

fn drain_actor_audio(
    mut world: ResMut<ClientWorld>,
    mut messages: MessageWriter<SequencedAudioEvent>,
) {
    if let Some(stream) = world.stream.as_mut() {
        client_presentation::audio_ingress::drain_committed_audio(stream, |event| {
            messages.write(event);
        });
    }
}

#[cfg(test)]
#[path = "synchronized_tests.rs"]
mod tests;
