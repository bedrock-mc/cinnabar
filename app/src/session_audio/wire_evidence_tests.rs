use crate::session_audio::{AudioOutcome, SessionAudio};
use acceptance::audio_wire::WireEvidence;
use client_presentation::audio_ingress::SequencedAudioEvent;
use std::sync::Arc;

fn event(session: u64, sequence: u64) -> SequencedAudioEvent {
    SequencedAudioEvent {
        origin_stream_session_id: session,
        dimension: 0,
        dimension_epoch: 0,
        actor_synchronization: None,
        sequence,
        event: protocol::AudioEvent::Play(protocol::PlayAudioEvent {
            name: Arc::from("game.player.attack.critical"),
            position: [9, -17, 25],
            volume: 1.0,
            pitch: 1.0,
            loop_count: -1,
            server_sound_handle: Some(123456789),
        }),
    }
}

fn stream() -> chunk_pipeline::WorldStream {
    chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

fn forwarded(stream: &mut chunk_pipeline::WorldStream, sequence: u64) -> Vec<SequencedAudioEvent> {
    stream
        .submit(
            sequence,
            protocol::WorldEvent::Audio(event(0, sequence).event),
        )
        .unwrap();
    let mut events = Vec::new();
    client_presentation::audio_ingress::drain_committed_audio(stream, |event| events.push(event));
    events
}

fn app(stream: chunk_pipeline::WorldStream, enabled: bool) -> bevy::prelude::App {
    let mut app = bevy::prelude::App::new();
    app.add_message::<SequencedAudioEvent>()
        .init_resource::<crate::environment::WorldClock>()
        .insert_resource(crate::runtime::world::ClientWorld {
            stream: Some(stream),
            ..crate::runtime::world::ClientWorld::default()
        })
        .insert_resource(crate::session_audio::SessionAudioCatalog(None))
        .insert_resource(SessionAudio::default())
        .insert_resource(WireEvidence::new(enabled))
        .add_systems(
            bevy::prelude::Update,
            crate::session_audio::drain_sequenced_audio_into_session,
        );
    app
}

fn write(app: &mut bevy::prelude::App, events: impl IntoIterator<Item = SequencedAudioEvent>) {
    let mut messages = app
        .world_mut()
        .resource_mut::<bevy::ecs::message::Messages<SequencedAudioEvent>>();
    for event in events {
        messages.write(event);
    }
}

#[test]
fn production_forwarding_rejects_old_buffered_stream_after_fifo_restart() {
    let mut old = stream();
    let old_id = old.authority().actor_session_id();
    let old_events = forwarded(&mut old, 1);
    assert_eq!(old_events[0].origin_stream_session_id, old_id);
    let mut app = app(old, true);
    write(&mut app, old_events);
    app.update();
    assert_eq!(
        app.world()
            .resource::<WireEvidence>()
            .observed_sequences()
            .len(),
        1
    );
    let old_buffered = {
        let mut world = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        forwarded(world.stream.as_mut().unwrap(), 2)
    };
    let mut replacement = stream();
    let new_id = replacement.authority().actor_session_id();
    assert_ne!(old_id, new_id);
    let new_events = forwarded(&mut replacement, 1);
    app.world_mut()
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream = Some(replacement);
    write(&mut app, old_buffered.into_iter().chain(new_events));
    app.update();
    let audio = app.world().resource::<SessionAudio>();
    let rows = app.world().resource::<WireEvidence>().observed_sequences();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, new_id);
    assert_eq!(rows[0].1, 1);
    assert_eq!(
        audio.catalog_unavailable_total(),
        2,
        "replaced-stream events cannot reach catalog resolution"
    );
    let mut events = Vec::new();
    {
        let mut world = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        for sequence in 2..=4 {
            events.extend(forwarded(world.stream.as_mut().unwrap(), sequence));
        }
    }
    write(&mut app, events);
    app.update();
    assert_eq!(
        app.world()
            .resource::<WireEvidence>()
            .observed_sequences()
            .len(),
        4
    );
    app.world_mut()
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream = None;
    write(&mut app, [event(new_id, 5)]);
    app.update();
    assert!(
        app.world()
            .resource::<WireEvidence>()
            .observed_sequences()
            .is_empty()
    );
    assert!(app.world().resource::<SessionAudio>().is_empty());
}

#[test]
fn production_evidence_precedes_missing_catalog_without_changing_resolution() {
    let mut origin = stream();
    let events = forwarded(&mut origin, 1);
    let mut comparison = stream();
    let comparison_events = forwarded(&mut comparison, 1);
    let mut enabled = app(origin, true);
    let mut disabled = app(comparison, false);
    write(&mut enabled, events);
    write(&mut disabled, comparison_events);
    enabled.update();
    disabled.update();
    let enabled_rows = enabled
        .world()
        .resource::<WireEvidence>()
        .observed_sequences();
    let disabled_rows = disabled
        .world()
        .resource::<WireEvidence>()
        .observed_sequences();
    let enabled = enabled.world().resource::<SessionAudio>();
    let disabled = disabled.world().resource::<SessionAudio>();
    assert_eq!(
        enabled.iter().collect::<Vec<_>>(),
        disabled.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        enabled.catalog_unavailable_total(),
        disabled.catalog_unavailable_total()
    );
    assert!(
        enabled
            .iter()
            .all(|outcome| matches!(outcome, AudioOutcome::Skipped { .. }))
    );
    assert_eq!(enabled_rows.len(), 1);
    assert!(disabled_rows.is_empty());
}
