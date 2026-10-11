use super::*;
use assets::RuntimeAssets;

#[test]
fn fog_samples_outside_nether_height_keep_the_dimension_biome() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let target: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("assets/bedrock-target.json")).unwrap())
            .unwrap();
    let path = root.join(target["artifacts"]["world_assets"].as_str().unwrap());
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "SKIP fog_samples_outside_nether_height_keep_the_dimension_biome: missing {}",
            path.display()
        );
        return;
    };
    let assets = RuntimeAssets::decode(&bytes).unwrap();
    let stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 1,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.5, 129.0, 0.5],
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let bindings = FogBindings::for_profiles(
        &assets.biome_assets().rules,
        &[assets::BiomeVisualProfile {
            biome_identifier: "minecraft:hell".into(),
            fog_identifier: "test:fog".into(),
            atmosphere_identifier: "test:atmosphere".into(),
            lighting_identifier: "test:lighting".into(),
            sky_rgb8: None,
        }],
        &[assets::FogProfile {
            identifier: "test:fog".into(),
            distances: Box::new([]),
        }],
    );
    for height in [129.0, -1.0] {
        assert_eq!(
            fog_biome_samples(&stream, [0.5, height, 0.5], &bindings),
            [Some(FogProfileIndex(0)); render_model::PRECIPITATION_SAMPLE_OFFSETS.len()]
        );
    }
}

#[test]
fn weather_and_fog_use_one_floored_native_lattice() {
    let origin = [-1.25, 63.875, 2.5];
    let positions = sample_positions(origin);
    for (actual, offset) in positions.iter().zip(PRECIPITATION_SAMPLE_OFFSETS) {
        assert_eq!(
            *actual,
            std::array::from_fn(|axis| origin[axis].floor() + offset[axis] as f32)
        );
    }
    assert_eq!(positions.len(), PRECIPITATION_SAMPLE_OFFSETS.len());
}

#[test]
fn missing_cells_never_get_renormalized_or_assigned_fallback_precipitation() {
    assert_eq!(count_precipitation_samples([None, None]), None);
    assert_eq!(count_precipitation_samples([Some(false), None]), Some(0));
    assert_eq!(
        count_precipitation_samples([Some(true), None, Some(false)]),
        Some(1)
    );
}
