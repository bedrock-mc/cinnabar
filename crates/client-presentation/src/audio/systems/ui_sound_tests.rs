//! Menu sounds survive teardown of the gameplay audio binding.

use bevy::prelude::*;

use super::{IngestState, UiSoundCue, ingest_audio_events};
use crate::{
    audio::{AudioEngine, AudioSettings},
    audio_ingress::SequencedAudioEvent,
    observations::WorldObservation,
};

/// Exercises a menu frame immediately after the previous world disappears.
fn after_disconnect(
    messages: MessageReader<SequencedAudioEvent>,
    cues: MessageReader<UiSoundCue>,
    engine: ResMut<AudioEngine>,
    mut state: Local<IngestState>,
) {
    state.stream = 1;
    ingest_audio_events(
        messages,
        cues,
        WorldObservation::default(),
        None,
        engine,
        state,
    );
}

#[test]
fn interface_click_is_not_discarded_by_world_teardown() {
    let mut app = App::new();
    app.add_message::<SequencedAudioEvent>()
        .add_message::<UiSoundCue>()
        .insert_resource(AudioEngine::new(None))
        .add_systems(Update, after_disconnect);
    app.world_mut()
        .write_message(UiSoundCue(client_ui::sound_requests::UI_CLICK));
    app.update();
    let mut engine = app.world_mut().resource_mut::<AudioEngine>();
    engine.pump(None, 0.0, &AudioSettings::default());
    assert_eq!(
        engine.stats.no_bank, 1,
        "the interface request reaches playback even without a world"
    );
    engine.pump(None, 0.0, &AudioSettings::default());
    assert_eq!(engine.stats.no_bank, 1, "the request is consumed once");
}
