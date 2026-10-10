use bevy::prelude::{App, First, Last, MinimalPlugins, ResMut, Resource};
use chunk_pipeline::WorldStream;
use protocol::WorldBootstrap;

use super::*;

#[derive(Resource, Default)]
struct Observed(Vec<bool>);

fn observe(client_world: Res<ClientWorld>, mut observed: ResMut<Observed>) {
    observed.0.push(client_world.stream.is_some());
}

fn serviced_app(slot: Option<WorldServiceSlot>) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ClientWorld {
            stream: Some(WorldStream::new(WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [8.0, 80.0, 8.0],
                world_spawn_position: [8, 64, 8],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            })),
            ..ClientWorld::default()
        })
        .init_resource::<LocalViewPose>()
        .insert_resource(ChunkUploadBudget::new(64, 64 * 1024 * 1024))
        .init_resource::<WorldStreamFramePoll>()
        .init_resource::<Observed>();
    if let Some(slot) = slot {
        app.insert_resource(slot);
    }
    configure_world_service(&mut app, Last);
    app.add_systems(First, observe).add_systems(Last, observe);
    app
}

/// Every frame system from `First` to `Last` finds the stream at home; it is lent out only
/// between frames and is back before the next `First`.
#[test]
fn frame_systems_always_find_the_stream_while_the_service_holds_it_between_frames() {
    let service = WorldServiceSlot {
        service: WorldStreamService::spawn().unwrap(),
    };
    let mut app = serviced_app(Some(service));
    for _ in 0..3 {
        app.update();
        assert!(
            app.world().resource::<ClientWorld>().stream.is_none(),
            "the service holds the stream between frames"
        );
        assert!(
            app.world()
                .resource::<WorldServiceSlot>()
                .service
                .is_servicing()
        );
    }
    assert_eq!(app.world().resource::<Observed>().0, vec![true; 6]);
}

/// Without the production service the stream never leaves the frame thread.
#[test]
fn without_a_service_the_stream_stays_home_between_frames() {
    let mut app = serviced_app(None);
    app.update();
    assert!(app.world().resource::<ClientWorld>().stream.is_some());
}
