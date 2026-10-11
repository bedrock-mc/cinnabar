use client_presentation::named_audio::AudioDevice;
use client_presentation::named_audio::test_support::sample;
use std::sync::Arc;
use {
    crate::named_audio::{SequencedAudioEvent, drain_live_named_audio},
    client_presentation::named_audio::NamedAudio,
};

fn play(sequence: u64) -> SequencedAudioEvent {
    SequencedAudioEvent {
        origin_stream_session_id: 1,
        sequence,
        dimension: 0,
        dimension_epoch: 0,
        actor_synchronization: None,
        event: protocol::AudioEvent::Play(protocol::PlayAudioEvent {
            name: Arc::from("ambient.underwater.loop"),
            position: [0; 3],
            volume: 1.0,
            pitch: 1.0,
            loop_count: -1,
            server_sound_handle: None,
        }),
    }
}
fn stop(sequence: u64, all: bool) -> SequencedAudioEvent {
    SequencedAudioEvent {
        origin_stream_session_id: 1,
        sequence,
        dimension: 0,
        dimension_epoch: 0,
        actor_synchronization: None,
        event: protocol::AudioEvent::Stop(protocol::StopAudioEvent {
            name: Arc::from("ambient.underwater.loop"),
            stop_all_sounds: all,
            stop_music_legacy: false,
        }),
    }
}

// Independently generated PCM is decoded through the real validated carrier.
// These composed tests prove routing/ownership, not authentic audible content.
struct EmptyAudioWorld;
impl sim::CollisionWorld for EmptyAudioWorld {
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}
fn forward_live_audio(
    mut world: bevy::prelude::ResMut<crate::runtime::world::ClientWorld>,
    mut messages: bevy::prelude::MessageWriter<SequencedAudioEvent>,
) {
    if let Some(stream) = world.stream.as_mut() {
        client_presentation::audio_ingress::drain_committed_audio(stream, |event| {
            messages.write(event);
        });
    }
}
fn composed_app() -> (bevy::prelude::App, rodio::dynamic_mixer::DynamicMixer<f32>) {
    use bevy::prelude::*;
    use client_presentation::local_player_camera_receipt::{
        CameraPublicationAttempt, begin_camera_publication_attempt,
    };
    use {
        crate::{
            environment::WorldClock,
            local_player::{publish_local_player_frame, resolve_camera_pose},
            movement::{LocalPhysicsController, PhysicsCollisionRegistries},
            runtime::world::ClientWorld,
        },
        client_presentation::{
            camera::{CameraSettingsAuthority, FlyCamera},
            local_player::{CameraPose, LocalPlayerFrameCarrier, LocalViewPose},
        },
    };
    let mut world = ClientWorld::new(Arc::new(assets::RuntimeAssets::diagnostic()));
    world.stream = Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    }));
    let collisions = PhysicsCollisionRegistries::bind_coherent_assets(
        assets::pinned_block_registry_bytes(),
        include_bytes!("../../../crates/assets/data/block-physics-v2193.bin"),
        std::path::Path::new("fixture.preg"),
        std::path::Path::new("fixture.mcbea"),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 64.0, 0.0], 0, false);
    physics.advance(
        std::time::Duration::from_millis(50),
        sim::MovementInput::default(),
        &EmptyAudioWorld,
    );
    assert!(physics.last_world_identity().is_some());
    let (device, mixer) = AudioDevice::memory_mixer();
    let mut app = App::new();
    app.add_message::<SequencedAudioEvent>()
        .insert_resource(world)
        .insert_resource(collisions)
        .insert_resource(physics)
        .init_resource::<WorldClock>()
        .init_resource::<CameraPose>()
        .insert_resource(LocalViewPose::new(
            Vec3::new(0.0, 65.62, 0.0),
            Quat::IDENTITY,
        ))
        .init_resource::<CameraSettingsAuthority>()
        .init_resource::<LocalPlayerFrameCarrier>()
        .init_resource::<CameraPublicationAttempt>()
        .insert_resource(NamedAudio::new(Some(Arc::new(sample()))))
        .insert_non_send(device)
        .add_systems(
            Update,
            (
                forward_live_audio,
                begin_camera_publication_attempt,
                resolve_camera_pose,
                publish_local_player_frame,
                drain_live_named_audio,
            )
                .chain(),
        );
    app.world_mut()
        .spawn((FlyCamera::default(), Transform::default()));
    (app, mixer)
}
fn commit(app: &mut bevy::prelude::App, sequence: u64, event: protocol::WorldEvent) {
    app.world_mut()
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(sequence, event)
        .unwrap();
}
fn live_play() -> protocol::WorldEvent {
    let protocol::AudioEvent::Play(mut event) = play(1).event else {
        unreachable!()
    };
    event.position = [0, 512, 0];
    protocol::WorldEvent::Audio(protocol::AudioEvent::Play(event))
}
fn live_dimension(dimension: i32) -> protocol::WorldEvent {
    protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
        dimension,
        position: [0.0, 64.0, 0.0],
        ..Default::default()
    })
}

#[test]
fn actual_committed_ingress_camera_writer_publication_and_mixer_submit_admit_once() {
    let (mut app, mut mixer) = composed_app();
    commit(&mut app, 1, live_play());
    app.update();
    let proof = app
        .world()
        .resource::<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>()
        .published()
        .unwrap();
    assert_eq!(proof.owner.sequence, 1);
    let audio = app.world().resource::<NamedAudio>();
    assert_eq!(
        (
            audio.stats().accepted,
            audio.stats().submitted,
            audio.stats().missing_camera
        ),
        (1, 1, 0)
    );
    assert_eq!(audio.occupied_permits(), 1);
    assert!(mixer.by_ref().take(8).any(|value| value > 0.0));
    app.update();
    assert_eq!(
        app.world().resource::<NamedAudio>().stats().submitted,
        1,
        "live reader does not replay diagnostics"
    );
}

#[test]
fn delayed_actual_old_epoch_stop_cannot_cancel_current_epoch_submitted_voice() {
    let (mut app, mut mixer) = composed_app();
    commit(
        &mut app,
        1,
        protocol::WorldEvent::Audio(stop(1, true).event),
    );
    let mut held = Vec::new();
    {
        let mut world = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        client_presentation::audio_ingress::drain_committed_audio(
            world.stream.as_mut().unwrap(),
            |event| held.push(event),
        );
    }
    assert_eq!((held[0].dimension, held[0].dimension_epoch), (0, 0));
    commit(&mut app, 2, live_dimension(1));
    commit(&mut app, 3, live_dimension(0));
    commit(&mut app, 4, live_play());
    app.update();
    let proof = app
        .world()
        .resource::<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>()
        .published()
        .unwrap();
    assert_eq!((proof.owner.dimension, proof.owner.epoch), (0, 3));
    assert_eq!(app.world().resource::<NamedAudio>().stats().submitted, 1);
    assert!(mixer.by_ref().take(8).any(|value| value > 0.0));
    // Delayed delivery uses the actual producer envelope, not a forged epoch.
    app.world_mut().write_message(held.pop().unwrap());
    app.update();
    let audio = app.world().resource::<NamedAudio>();
    assert_eq!(
        (
            audio.stats().stale,
            audio.stats().stopped,
            audio.occupied_permits()
        ),
        (1, 0, 1)
    );
    assert!(mixer.by_ref().take(8).any(|value| value > 0.0));
    commit(
        &mut app,
        5,
        protocol::WorldEvent::Audio(stop(5, false).event),
    );
    app.update();
    assert_eq!(app.world().resource::<NamedAudio>().stats().stopped, 1);
    assert_eq!(app.world().resource::<NamedAudio>().occupied_permits(), 1);
    for _ in 0..1024 {
        mixer.next();
        if app.world().resource::<NamedAudio>().occupied_permits() == 0 {
            break;
        }
    }
    assert_eq!(app.world().resource::<NamedAudio>().occupied_permits(), 0);
}

#[test]
fn every_committed_camera_family_cancels_live_voice_without_reminting_or_regrant() {
    for event in [
        protocol::CameraEvent::Switch(protocol::CameraSwitchEvent {
            camera_unique_id: 1,
            target_player_unique_id: 1,
        }),
        protocol::CameraEvent::Instruction(Box::default()),
        protocol::CameraEvent::Instruction(Box::new(protocol::CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        })),
        protocol::CameraEvent::Instruction(Box::new(protocol::CameraInstructionEvent {
            clear: Some(false),
            ..Default::default()
        })),
        protocol::CameraEvent::Shake(protocol::CameraShakeEvent {
            intensity: 0.1,
            duration_seconds: 0.1,
            shake_type: protocol::CameraShakeType::Positional,
            action: protocol::CameraShakeAction::Add,
        }),
        protocol::CameraEvent::Shake(protocol::CameraShakeEvent {
            intensity: 0.0,
            duration_seconds: 0.0,
            shake_type: protocol::CameraShakeType::Rotational,
            action: protocol::CameraShakeAction::Stop,
        }),
    ] {
        let (mut app, mut mixer) = composed_app();
        commit(&mut app, 1, live_play());
        app.update();
        assert_eq!(app.world().resource::<NamedAudio>().stats().submitted, 1);
        assert!(mixer.by_ref().take(8).any(|value| value > 0.0));
        commit(&mut app, 2, protocol::WorldEvent::Camera(event));
        app.update();
        let audio = app.world().resource::<NamedAudio>();
        assert_eq!(
            audio.occupied_permits(),
            1,
            "cancellation is not backend retirement"
        );
        assert_eq!(audio.retained_controls().0, 1);
        assert!(audio.retained_controls().1);
        for _ in 0..1024 {
            mixer.next();
            if app.world().resource::<NamedAudio>().occupied_permits() == 0 {
                break;
            }
        }
        assert_eq!(app.world().resource::<NamedAudio>().occupied_permits(), 0);
        app.world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .begin_timed_session();
        commit(&mut app, 3, live_play());
        app.update();
        let audio = app.world().resource::<NamedAudio>();
        assert_eq!(
            (audio.stats().submitted, audio.stats().missing_camera),
            (1, 1)
        );
        assert_eq!(audio.occupied_permits(), 0);
    }
}
