use std::sync::Arc;

use assets::{
    BiomeRule, BlobProvenance, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets,
    ContributorRole, LightProperties, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    Material, NO_ANIMATION, RuntimeAssets, TextureRef, TintSource, encode_blob,
};
use chunk_pipeline::WorldStream;
use protocol::{LevelChunkEvent, LevelChunkMode, WorldBootstrap, WorldEvent};

/// Supplies different dimension and published-column colours without local game assets.
fn stream(dimension: i32) -> WorldStream {
    let diagnostic = RuntimeAssets::diagnostic();
    let mut biomes = CompiledBiomeAssets::diagnostic();
    biomes.rules = [0, 1, 2]
        .into_iter()
        .map(client_world::ingestion::default_biome_id)
        .chain(std::iter::once(42))
        .map(|id| BiomeRule {
            id,
            name: format!("test:biome_{id}").into(),
            flags: 0,
            grass: TintSource::direct(0x20_8040 + id),
            foliage: TintSource::direct(0x40_6020 + id),
            dry_foliage: TintSource::direct(0x80_6040 + id),
            water: TintSource::direct(0x20_4080 + id),
            temperature_bits: 0.8_f32.to_bits(),
            downfall_bits: 0.4_f32.to_bits(),
        })
        .collect();
    let compiled = CompiledAssets {
        visuals: vec![BlockVisual::diagnostic(
            BlockFlags::empty(),
            ContributorRole::Primary,
        )]
        .into(),
        light_properties: vec![LightProperties::OPAQUE_DARK].into(),
        hashed: Box::new([]),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..Material::unvaried()
        }]
        .into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: diagnostic.texture_pages().to_vec().into(),
        biomes,
        provenance: BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    let assets = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    WorldStream::new_with_assets(
        WorldBootstrap {
            dimension,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets),
        [0.0; 3],
        None,
    )
}

/// Expected gamma tint from the same resolved table that published biome IDs address.
fn tint(stream: &WorldStream, raw: u32, flags: u32) -> [f32; 4] {
    let table = stream.resolved_biome_tints_snapshot();
    particles::tiles::material_tint(&table.records[table.dense_index(raw) as usize], flags)
}

#[test]
fn grass_and_foliage_particles_use_dimension_colour_before_biome_publication() {
    for dimension in [0, 1, 2] {
        let stream = stream(dimension);
        let raw = client_world::ingestion::default_biome_id(dimension);
        for flags in [MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_FOLIAGE_TINT] {
            let expected = tint(&stream, raw, flags);
            assert_ne!(expected, [1.0; 4]);
            assert_eq!(stream.camera_biome_id([0.5; 3]), None);
            assert_eq!(super::biome_tint(&stream, flags, [0; 3]), expected);
        }
        assert_eq!(super::biome_tint(&stream, 0, [0; 3]), [1.0; 4]);
    }
}

#[test]
fn published_column_replaces_initial_particle_colour_without_waiting_for_a_mesh() {
    let mut stream = stream(0);
    let fallback = client_world::ingestion::default_biome_id(0);
    let block = [3, -63, 5];
    let center = block.map(|value| value as f32 + 0.5);
    let flags = MATERIAL_FLAG_GRASS_TINT;
    let before = super::biome_tint(&stream, flags, block);
    assert_eq!(before, tint(&stream, fallback, flags));
    let mut payload = vec![1, 84]; // Uniform raw biome 42, followed by inherited vertical slots.
    payload.extend(std::iter::repeat_n(
        0xff,
        protocol::vanilla_dimension_range(0)
            .unwrap()
            .sub_chunk_count
            - 1,
    ));
    payload.push(0);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 0 },
                payload,
            }),
        )
        .unwrap();
    test_time::eventually("particle biome column publication", || {
        stream.poll(center, 0);
        stream.committed_sequence() >= 1
    });
    assert_eq!(stream.camera_biome_id(center), Some(42));
    let after = super::biome_tint(&stream, flags, block);
    assert_eq!(after, tint(&stream, 42, flags));
    assert_ne!(before, after);
    assert_eq!(
        super::biome_tint(&stream, flags, [16, -63, 5]),
        before,
        "a neighbouring unpublished column still uses the dimension colour"
    );
}
