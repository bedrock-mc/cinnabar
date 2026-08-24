use std::sync::Arc;

use assets::{AudioAlternative, AudioDefinition, RuntimeAudioCatalog, encode_audio_catalog};
use bevy::prelude::{
    App, IntoScheduleConfigs, IntoSystemSet, MessageWriter, ResMut, Resource, SystemSet, Update,
};
use protocol::{AudioEvent, PlayAudioEvent, StopAudioEvent};

use super::*;
use crate::runtime::audio::drain_committed_audio;
use crate::{
    app::{configure_client_frame_schedule, configure_client_production_frame_systems},
    runtime::{
        audio::SequencedAudioEvent,
        world::{ClientWorld, reconcile_world_stream_before_physics},
    },
    session_audio::{SessionAudio, SessionAudioCatalog, drain_sequenced_audio_into_session},
};

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

fn fixture_alternative(name: &str, weight: u16) -> AudioAlternative {
    AudioAlternative {
        object_form: true,
        name: name.into(),
        weight,
        volume: None,
        pitch: None,
        is_3d: None,
        stream: None,
        load_on_low_memory: None,
    }
}

fn fixture_catalog() -> RuntimeAudioCatalog {
    let definitions = vec![AudioDefinition {
        identifier: "random.orb".into(),
        category: None,
        subtitle: None,
        min_distance: None,
        max_distance: None,
        volume: None,
        pitch: None,
        use_legacy_max_distance: None,
        alternatives: vec![fixture_alternative("sounds/orb", 1)].into_boxed_slice(),
    }];
    let bytes = encode_audio_catalog([0x33; 32], [0x44; 32], &definitions).unwrap();
    RuntimeAudioCatalog::decode(&bytes).unwrap()
}

fn sequenced_play(sequence: u64, name: &str) -> SequencedAudioEvent {
    SequencedAudioEvent {
        sequence,
        event: AudioEvent::Play(PlayAudioEvent {
            name: Arc::from(name),
            position: [0, 64, 0],
            volume: 1.0,
            pitch: 1.0,
            loop_count: 0,
            server_sound_handle: None,
        }),
    }
}

fn sequenced_stop(sequence: u64) -> SequencedAudioEvent {
    SequencedAudioEvent {
        sequence,
        event: AudioEvent::Stop(StopAudioEvent {
            name: Arc::from("random.orb"),
            stop_all_sounds: false,
            stop_music_legacy: false,
        }),
    }
}

fn connected_client_world() -> ClientWorld {
    ClientWorld {
        stream: Some(WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        })),
        ..ClientWorld::default()
    }
}

#[derive(Resource)]
struct PendingAudio(Vec<SequencedAudioEvent>);

fn write_pending_audio(
    mut pending: ResMut<PendingAudio>,
    mut writer: MessageWriter<SequencedAudioEvent>,
) {
    for event in pending.0.drain(..) {
        writer.write(event);
    }
}

#[test]
fn session_audio_reader_consumes_each_sequenced_event_exactly_once() {
    let mut app = App::new();
    app.add_message::<SequencedAudioEvent>()
        .init_resource::<crate::environment::WorldClock>()
        .init_resource::<SessionAudio>()
        .insert_resource(SessionAudioCatalog(Some(Arc::new(fixture_catalog()))))
        .insert_resource(connected_client_world())
        .insert_resource(PendingAudio(vec![
            sequenced_play(1, "random.orb"),
            sequenced_stop(2),
        ]))
        .add_systems(
            Update,
            (write_pending_audio, drain_sequenced_audio_into_session).chain(),
        );

    app.update();

    let session = app.world().resource::<SessionAudio>();
    assert_eq!(session.len(), 2, "both events resolved into outcomes");
    assert_eq!(session.admitted_total(), 2);
    assert_eq!(session.unknown_definition_total(), 0);

    // A later frame without new writes must neither duplicate nor accumulate.
    app.update();
    let session = app.world().resource::<SessionAudio>();
    assert_eq!(session.len(), 2);
    assert_eq!(session.admitted_total(), 2);
}

#[test]
fn production_schedule_reads_session_audio_after_the_world_stream_writer() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    let schedules = app.world().resource::<bevy::ecs::schedule::Schedules>();
    let graph = schedules
        .get(Update)
        .expect("production Update schedule")
        .graph();

    let reader = system_node(
        graph,
        drain_sequenced_audio_into_session,
        "drain_sequenced_audio_into_session",
    );
    // Bevy 0.18 stores a fn-to-fn `.after(target)` constraint as an edge from
    // the target's set-level node to the dependent's system node.
    let writer_set = bevy::ecs::schedule::NodeId::Set(
        graph
            .system_sets
            .get_key(IntoSystemSet::into_system_set(reconcile_world_stream_before_physics).intern())
            .expect("writer set key"),
    );
    assert!(
        graph.dependency().graph().contains_edge(writer_set, reader),
        "the audio reader must run after the world-stream writer"
    );
}

fn system_node<M>(
    graph: &bevy::ecs::schedule::ScheduleGraph,
    system: impl IntoSystemSet<M>,
    label: &str,
) -> bevy::ecs::schedule::NodeId {
    let key = graph
        .system_sets
        .get_key(system.into_system_set().intern())
        .unwrap_or_else(|| panic!("missing {label}"));
    let parent = bevy::ecs::schedule::NodeId::Set(key);
    graph
        .systems
        .iter()
        .find_map(|(key, _, _)| {
            let child = bevy::ecs::schedule::NodeId::System(key);
            graph
                .hierarchy()
                .graph()
                .contains_edge(parent, child)
                .then_some(child)
        })
        .unwrap_or_else(|| panic!("missing {label}"))
}
