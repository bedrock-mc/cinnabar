use crate::player_runtime::PlayerRuntime;
use std::sync::Arc;

use assets::{AudioAlternative, AudioDefinition, RuntimeAudioCatalog, encode_audio_catalog};
use bevy::prelude::{
    App, AppExit, IntoScheduleConfigs, IntoSystemSet, MessageWriter, ResMut, Resource, SystemSet,
    Update,
};
use protocol::{AudioEvent, PlayAudioEvent, StopAudioEvent};

use super::*;
use crate::{
    app::{
        ClientBlobCacheOwner, configure_acceptance_finish_system, configure_client_frame_schedule,
        configure_client_production_frame_systems,
    },
    menu::{MenuAction, MenuRuntime},
    runtime::{
        network::{NetworkHandle, ResourcePackAdmissionState},
        world::{ClientWorld, TransferNotice, reconcile_world_stream_before_physics},
    },
    session::{drive_session, follow_server_transfer, recover_session_failure},
    session_audio::{SessionAudio, SessionAudioCatalog, drain_sequenced_audio_into_session},
};
use client_presentation::audio_ingress::SequencedAudioEvent;
use client_presentation::audio_ingress::drain_committed_audio;
use client_ui::ui_runtime::UiRuntime;

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
    assert!(
        forwarded
            .iter()
            .all(|event| event.origin_stream_session_id == stream.authority().actor_session_id())
    );
    assert_eq!(stream.stats().committed_audio_events, 0);

    drain_committed_audio(&mut stream, |event| forwarded.push(event));
    assert_eq!(
        forwarded.len(),
        2,
        "drained audio must not accumulate in the app"
    );
}

#[test]
fn live_playback_reader_consumes_commit_epoch_fences_without_diagnostic_ring() {
    let mut world = connected_client_world();
    let stream = world.stream.as_mut().unwrap();
    stream.submit(1, audio_event("first")).unwrap();
    stream
        .submit(
            2,
            WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    stream.submit(3, audio_event("middle")).unwrap();
    stream
        .submit(
            4,
            WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                dimension: 0,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    stream.submit(5, audio_event("last")).unwrap();
    let mut events = Vec::new();
    drain_committed_audio(stream, |event| events.push(event));
    assert_eq!(
        events
            .iter()
            .map(|event| (event.dimension, event.dimension_epoch))
            .collect::<Vec<_>>(),
        vec![(0, 0), (1, 2), (0, 4)]
    );
    let mut app = App::new();
    app.add_message::<SequencedAudioEvent>()
        .insert_resource(world)
        .init_resource::<crate::environment::WorldClock>()
        .init_resource::<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>()
        .init_resource::<crate::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<crate::local_player::CameraPose>()
        .init_resource::<crate::movement::LocalPhysicsController>()
        .init_resource::<crate::named_audio::NamedAudio>()
        .add_systems(Update, crate::named_audio::drain_live_named_audio);
    for event in events {
        app.world_mut()
            .resource_mut::<bevy::ecs::message::Messages<SequencedAudioEvent>>()
            .write(event);
    }
    app.update();
    let stats = app
        .world()
        .resource::<crate::named_audio::NamedAudio>()
        .stats();
    assert_eq!((stats.stale, stats.unavailable), (2, 1));
    app.update();
    assert_eq!(
        app.world()
            .resource::<crate::named_audio::NamedAudio>()
            .stats()
            .stale,
        2
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

/// Builds a play message for the fixture stream that produced it.
fn sequenced_play(origin_stream_session_id: u64, sequence: u64, name: &str) -> SequencedAudioEvent {
    SequencedAudioEvent {
        origin_stream_session_id,
        dimension: 0,
        dimension_epoch: 0,
        actor_synchronization: None,
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

/// Builds a stop message for the fixture stream that produced it.
fn sequenced_stop(origin_stream_session_id: u64, sequence: u64) -> SequencedAudioEvent {
    SequencedAudioEvent {
        origin_stream_session_id,
        dimension: 0,
        dimension_epoch: 0,
        actor_synchronization: None,
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
    // Create an earlier session even when this test runs alone in nextest.
    let previous_world = connected_client_world();
    let previous_session = previous_world
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    let world = connected_client_world();
    let stream_session = world
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    assert_ne!(stream_session, previous_session);
    let mut app = App::new();
    app.add_message::<SequencedAudioEvent>()
        .init_resource::<crate::environment::WorldClock>()
        .init_resource::<SessionAudio>()
        .insert_resource(SessionAudioCatalog(Some(Arc::new(fixture_catalog()))))
        .insert_resource(world)
        .insert_resource(PendingAudio(vec![
            sequenced_play(stream_session, 1, "random.orb"),
            sequenced_stop(stream_session, 2),
            sequenced_play(previous_session, 3, "random.orb"),
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
fn production_audio_reader_clears_disconnect_state_and_drops_stale_messages() {
    let world = connected_client_world();
    let stream_session = world
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    let mut app = App::new();
    app.add_message::<SequencedAudioEvent>()
        .init_resource::<crate::environment::WorldClock>()
        .init_resource::<SessionAudio>()
        .insert_resource(SessionAudioCatalog(Some(Arc::new(fixture_catalog()))))
        .insert_resource(world)
        .insert_resource(PendingAudio(vec![sequenced_play(
            stream_session,
            1,
            "random.orb",
        )]))
        .add_systems(
            Update,
            (write_pending_audio, drain_sequenced_audio_into_session).chain(),
        );

    app.update();
    assert_eq!(app.world().resource::<SessionAudio>().len(), 1);

    app.world_mut().resource_mut::<ClientWorld>().stream = None;
    app.world_mut()
        .resource_mut::<PendingAudio>()
        .0
        .push(sequenced_play(stream_session, 2, "random.orb"));
    app.update();

    let disconnected = app.world().resource::<SessionAudio>();
    assert!(
        disconnected.is_empty(),
        "disconnect must clear old outcomes"
    );
    assert_eq!(disconnected.resets(), 1);

    app.update();
    assert_eq!(
        app.world().resource::<SessionAudio>().resets(),
        1,
        "repeated disconnected frames must remain idempotent"
    );

    app.world_mut().resource_mut::<ClientWorld>().stream = connected_client_world().stream;
    app.update();

    let replacement = app.world().resource::<SessionAudio>();
    assert!(
        replacement.is_empty(),
        "audio written after teardown must not replay in the replacement session"
    );
    assert_eq!(
        replacement.resets(),
        1,
        "replacement binding is not a second reset"
    );
    assert_eq!(replacement.admitted_total(), 1);
}

fn retained_session_audio() -> SessionAudio {
    let mut audio = SessionAudio::default();
    audio.admit(1, 0, [sequenced_stop(1, 1)], None);
    audio
}

fn add_audio_teardown_resources(app: &mut App, client_world: ClientWorld, menu: MenuRuntime) {
    app.add_message::<SequencedAudioEvent>()
        .init_resource::<crate::environment::WorldClock>()
        .insert_resource(SessionAudioCatalog(None))
        .insert_resource(retained_session_audio())
        .insert_resource(client_world)
        .insert_resource(menu)
        .insert_resource(crate::session::SessionController::default())
        .insert_resource(NetworkHandle::disconnected())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(UiRuntime::new(1))
        .insert_resource(PlayerRuntime::new(1))
        .insert_resource(crate::movement::MovementTicker::default())
        .insert_resource(crate::movement::LocalPhysicsController::default())
        .insert_resource(crate::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(crate::local_player::InteractionOriginSnapshot::default());
}

#[test]
fn menu_disconnect_clears_audio_in_its_production_frame() {
    let mut app = App::new();
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
    menu.activate(MenuAction::PauseDisconnect);
    add_audio_teardown_resources(&mut app, connected_client_world(), menu);
    app.add_message::<AppExit>()
        .insert_resource(ClientBlobCacheOwner::default())
        .add_systems(
            Update,
            (
                drive_session,
                drain_sequenced_audio_into_session.after(drive_session),
            ),
        );

    app.update();

    assert!(app.world().resource::<ClientWorld>().stream.is_none());
    assert!(app.world().resource::<SessionAudio>().is_empty());
    assert_eq!(app.world().resource::<SessionAudio>().resets(), 1);
}

#[test]
fn failure_recovery_clears_audio_in_its_production_frame() {
    let mut app = App::new();
    let mut client_world = connected_client_world();
    client_world.fatal_error = Some("session failed".to_owned());
    add_audio_teardown_resources(
        &mut app,
        client_world,
        MenuRuntime::new(true, 2, "Player".to_owned()),
    );
    app.add_systems(
        Update,
        (
            recover_session_failure,
            drain_sequenced_audio_into_session.after(recover_session_failure),
        ),
    );

    app.update();

    assert!(app.world().resource::<ClientWorld>().stream.is_none());
    assert!(app.world().resource::<SessionAudio>().is_empty());
    assert_eq!(app.world().resource::<SessionAudio>().resets(), 1);
}

#[test]
fn rejected_transfer_clears_audio_in_its_production_frame() {
    let mut app = App::new();
    let mut client_world = connected_client_world();
    client_world.transfer_notice = Some(TransferNotice {
        host: " ".to_owned(),
        port: 19132,
    });
    add_audio_teardown_resources(
        &mut app,
        client_world,
        MenuRuntime::new(true, 2, "Player".to_owned()),
    );
    app.insert_resource(ClientBlobCacheOwner::default())
        .add_systems(
            Update,
            (
                follow_server_transfer,
                drain_sequenced_audio_into_session.after(follow_server_transfer),
            ),
        );

    app.update();

    assert!(app.world().resource::<ClientWorld>().stream.is_none());
    assert!(app.world().resource::<SessionAudio>().is_empty());
    assert_eq!(app.world().resource::<SessionAudio>().resets(), 1);
}

#[test]
fn production_schedule_reads_session_audio_after_the_world_stream_writer() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    configure_acceptance_finish_system(&mut app);
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
    for (teardown, label) in [
        (
            IntoSystemSet::into_system_set(drive_session).intern(),
            "menu disconnect",
        ),
        (
            IntoSystemSet::into_system_set(follow_server_transfer).intern(),
            "server transfer",
        ),
        (
            IntoSystemSet::into_system_set(recover_session_failure).intern(),
            "failure recovery",
        ),
    ] {
        let teardown = bevy::ecs::schedule::NodeId::Set(
            graph
                .system_sets
                .get_key(teardown)
                .expect("teardown set key"),
        );
        assert!(
            graph.dependency().graph().contains_edge(teardown, reader),
            "the audio reader must run after {label}"
        );
    }
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
