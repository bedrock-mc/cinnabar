//! Controlled device readiness through the real frame pump.

use super::*;
use bevy::ecs::system::SystemState;
use bevy::prelude::World;

/// Runs the audio frame against the world's engine, settings and optional device.
fn frame(world: &mut World) {
    let mut state = SystemState::<(
        Res<Time>,
        Res<AudioSettings>,
        ResMut<AudioEngine>,
        Option<NonSendMut<AudioDevice>>,
    )>::new(world);
    let (time, settings, engine, device) = state.get_mut(world).expect("audio frame resources");
    pump_audio(
        time,
        super::super::engine::Listener {
            position: [0.0; 3],
            right: [1.0, 0.0, 0.0],
        },
        settings,
        engine,
        device,
    );
    state.apply(world);
}

/// Builds a frame fixture with a decoded, nonspatial sound and an unopened device.
fn audio_world(device: AudioDevice) -> World {
    let mut world = World::new();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(AudioSettings::default());
    world.insert_resource(super::super::engine::tests::engine(&[("first", "ui")]));
    world.insert_non_send(device);
    world
}

#[test]
fn opening_frames_retain_sources_until_a_transport_is_ready() {
    let (device, release, owner) = AudioDevice::delayed_open();
    let mut world = audio_world(device);
    let (ready, mut reference_mixer) = AudioDevice::memory_mixer();
    let mut reference = audio_world(ready);
    for count in 1..=4 {
        world
            .resource_mut::<AudioEngine>()
            .enqueue(SoundRequest::new("first"));
        frame(&mut world);
        reference
            .resource_mut::<AudioEngine>()
            .enqueue(SoundRequest::new("first"));
        frame(&mut reference);
        assert_eq!(world.resource::<AudioEngine>().stats.started, count);
        assert_eq!(world.resource::<AudioEngine>().stats.backend_failed, 0);
    }
    let mut mixer = world.non_send_mut::<AudioDevice>().start_capture();
    frame(&mut world);
    assert_eq!(world.resource::<AudioEngine>().stats.started, 4);
    let expected: Vec<_> = reference_mixer.by_ref().take(64).collect();
    let actual: Vec<_> = mixer.by_ref().take(expected.len()).collect();
    assert!(actual.iter().any(|sample| *sample > 0.0));
    assert_eq!(actual, expected, "opening must not consume or drop samples");
    frame(&mut world);
    assert_eq!(
        world.resource::<AudioEngine>().stats.started,
        4,
        "submit once"
    );
    assert_eq!(
        mixer.take(64).collect::<Vec<_>>(),
        reference_mixer.take(64).collect::<Vec<_>>(),
        "pending sources must be submitted once"
    );
    drop(release);
    owner.join().unwrap();
}

#[test]
fn stops_and_session_resets_cancel_staged_sources() {
    let (device, release, owner) = AudioDevice::delayed_open();
    let mut world = audio_world(device);
    for reset in [false, true] {
        world
            .resource_mut::<AudioEngine>()
            .enqueue(SoundRequest::new("first"));
        frame(&mut world);
        let mut engine = world.resource_mut::<AudioEngine>();
        if reset {
            engine.stop_all();
        } else {
            engine.stop_named("first");
        }
    }
    let mut mixer = world.non_send_mut::<AudioDevice>().start_capture();
    frame(&mut world);
    assert_eq!(world.resource::<AudioEngine>().stats.started, 2);
    assert_eq!(mixer.next(), None);
    drop(release);
    owner.join().unwrap();
}

#[test]
fn a_failed_open_retires_staged_sources_without_retrying_them() {
    let (device, release, owner) = AudioDevice::delayed_open();
    let mut world = audio_world(device);
    world
        .resource_mut::<AudioEngine>()
        .enqueue(SoundRequest::new("first"));
    frame(&mut world);
    drop(release);
    owner.join().unwrap();
    frame(&mut world);
    assert_eq!(world.resource::<AudioEngine>().stats.started, 1);
    assert_eq!(world.resource::<AudioEngine>().stats.backend_failed, 1);
    frame(&mut world);
    assert_eq!(world.resource::<AudioEngine>().stats.backend_failed, 1);
}
