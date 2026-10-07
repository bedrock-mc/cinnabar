use std::sync::Arc;

use chunk_pipeline::WorldStream;
use protocol::{DimensionHeightDiagnostic, WorldBootstrap, WorldEvent};

use super::leaf_column_exposed;

#[test]
fn leaf_exposure_uses_the_admitted_session_roof() {
    for (dimension, name, wire_dimension, minimum_y, roof) in [
        (0, "minecraft:overworld", 3, 0, 256),
        (1000, "example:raised", 1000, 256, 512),
    ] {
        let mut stream = WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        });
        stream
            .submit(
                1,
                WorldEvent::DimensionHeights(vec![DimensionHeightDiagnostic {
                    name: Arc::from(name),
                    dimension: wire_dimension,
                    minimum_y,
                    height_range: roof - minimum_y,
                    generator: 1,
                }]),
            )
            .unwrap();
        assert!(
            leaf_column_exposed(&stream, roof - 1, |y| y >= roof),
            "resident air below the session roof is exposed in {name}"
        );
        assert!(!leaf_column_exposed(&stream, roof - 1, |_| true));
        assert!(!leaf_column_exposed(&stream, roof, |_| false));
    }
}
