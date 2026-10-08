use super::*;
use bevy::prelude::{App, Update};
use chunk_pipeline::WorldStream;
use protocol::{BiomeDefinitionEvent, BiomeDefinitionsEvent, WorldBootstrap, WorldEvent};
use std::sync::Arc;

#[test]
fn seasonal_scheduled_palette_update_reaches_render_resource_with_same_mesh_identity() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from([BiomeDefinitionEvent {
                    biome_id: Some(7),
                    name: "example:cold_season".into(),
                    temperature: -0.5,
                    downfall: 0.8,
                    snow_foliage: 0.0,
                    max_snow_accumulation: Some(0.5),
                    map_water_color: 0xff44_6688,
                }]),
            }),
        )
        .unwrap();
    let before = stream.resolved_biome_tints_snapshot();
    let table = stream.biome_tint_identity();
    let active = ChunkBiomeTints::from_resolved_with_identity(&before, table);
    let client = ClientWorld {
        stream: Some(stream),
        ..ClientWorld::default()
    };
    let mut app = App::new();
    app.insert_resource(client)
        .insert_resource(active)
        .insert_resource(WeatherTickFrame {
            rain: vec![[1.0, 1.0]; world::TICKS_PER_SECOND as usize],
            dimension: 0,
            weather_cycle_enabled: true,
        })
        .add_systems(Update, update_seasonal_foliage);
    // Bounded fixture frames; stop when the native palette refresh publishes.
    for _ in 0..world::TICKS_PER_SECOND {
        app.update();
        let snapshot = app
            .world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .resolved_biome_tints_snapshot();
        if !Arc::ptr_eq(&snapshot, &before) {
            break;
        }
    }
    let stream = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap();
    let after = stream.resolved_biome_tints_snapshot();
    let active = app.world().resource::<ChunkBiomeTints>();
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(active.table_identity(), table);
    assert_eq!(stream.biome_tint_identity(), table);
    assert_eq!(after.raw_id_to_dense, before.raw_id_to_dense);
    let dense = after.dense_index(7) as usize;
    let exposed = assets::SEASONAL_FOLIAGE_EXPOSED_OFFSET;
    let colour = after.records[dense].seasonal_foliage[exposed];
    assert_eq!(
        active.entries()[dense].seasonal_foliage[exposed],
        [colour[0], colour[1], colour[2]]
    );
    assert_ne!(
        after.records[dense].seasonal_foliage[exposed],
        before.records[dense].seasonal_foliage[exposed]
    );
}
