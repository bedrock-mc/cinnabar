use super::*;
use chunk_pipeline::WorldStream;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use std::sync::Arc;

#[derive(Default, Resource)]
struct Heard(Vec<SequencedAudioEvent>);

fn advance_actor_tick(mut world: ResMut<ClientWorld>) {
    world
        .stream
        .as_mut()
        .unwrap()
        .advance_actor_interpolation_frame(1);
}

fn listen(mut events: MessageReader<SequencedAudioEvent>, mut heard: ResMut<Heard>) {
    heard.0.extend(events.read().cloned());
}

#[test]
fn synchronized_audio_is_delivered_in_the_actor_completion_frame_once() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 17,
                runtime_id: 7,
                kind: ActorKind::Entity {
                    identifier: "minecraft:ender_dragon".into(),
                },
                position: [1.0, 64.0, 3.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::Audio(protocol::AudioEvent::Level(protocol::LevelAudioEvent {
                sound_event: "death".into(),
                position: [1.0, 65.0, 3.0],
                data: -1,
                actor_identifier: "minecraft:ender_dragon".into(),
                is_baby: false,
                is_global: false,
                actor_unique_id: 17,
                fire_at_position: Some([1.0, 64.0, 3.0]),
            })),
        )
        .unwrap();
    assert_eq!(stream.committed_sequence(), 2);
    assert_eq!(stream.authority().actor(7).unwrap().unique_id, 17);
    assert_eq!(stream.authority().actor(7).unwrap().status.age_ticks, 0);
    assert!(stream.take_committed_audio().is_empty());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<SequencedAudioEvent>()
        .init_resource::<ClientWorld>()
        .init_resource::<Heard>()
        .configure_sets(
            Update,
            (
                crate::app::ClientFrameSet::WorldPublication,
                crate::app::ClientFrameSet::ActorPreparation,
            )
                .chain(),
        )
        .add_systems(
            Update,
            advance_actor_tick.in_set(crate::app::ClientFrameSet::ActorPreparation),
        )
        .add_systems(
            Update,
            listen
                .after(crate::app::ClientFrameSet::ActorPreparation)
                .after(drain_actor_audio),
        );
    app.world_mut().resource_mut::<ClientWorld>().stream = Some(stream);
    configure(&mut app);
    app.update();
    assert_eq!(
        app.world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .actor(7)
            .unwrap()
            .status
            .age_ticks,
        1
    );
    let heard = &app.world().resource::<Heard>().0;
    assert_eq!(
        heard.len(),
        1,
        "the tick release reaches this frame's sound consumer"
    );
    assert_eq!(heard[0].sequence, 2);
    assert_eq!(heard[0].actor_synchronization.unwrap().runtime_id, 7);
    app.update();
    assert_eq!(app.world().resource::<Heard>().0.len(), 1);
}
