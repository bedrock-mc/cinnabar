use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

use assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, ContributorRole, LightProperties,
    Material, NO_ANIMATION, NO_MODEL_TEMPLATE, RuntimeAssets, TextureArray, TextureMip,
    TexturePage, TextureRef, VisualKind, encode_blob,
};
use protocol::WorldBootstrap;
use world::{
    BlockPos, BlockUpdate, BoundaryLightSample, DecodedLevelChunk, LightBlockAccess, LightChannel,
    LightReadAccess, LightSolveError, SubChunkKey, SubChunkLight,
};

use super::*;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    })
}

fn lit_stream(dimension: i32) -> WorldStream {
    WorldStream::new_with_assets(
        WorldBootstrap {
            dimension,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(light_test_assets()),
        [0.0, 80.0, 0.0],
        None,
    )
}

fn light_test_assets() -> RuntimeAssets {
    let visuals = [
        (BlockFlags::AIR, VisualKind::Invisible, ContributorRole::Air),
        (
            BlockFlags::CUBE_GEOMETRY,
            VisualKind::Cube,
            ContributorRole::Primary,
        ),
        (
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            VisualKind::Cube,
            ContributorRole::Primary,
        ),
        (
            BlockFlags::CUBE_GEOMETRY,
            VisualKind::Cube,
            ContributorRole::Primary,
        ),
    ]
    .map(|(flags, kind, contributor_role)| BlockVisual {
        faces: [0; 6],
        flags,
        kind,
        contributor_role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    });
    let compiled = CompiledAssets {
        visuals: visuals.into(),
        light_properties: vec![
            LightProperties::new(0, 0).unwrap(),
            LightProperties::new(15, 0).unwrap(),
            LightProperties::new(0, 15).unwrap(),
            LightProperties::new(0, 0).unwrap(),
        ]
        .into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        }]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 1,
            mips: [16_u32, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })]
        .into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

fn complete_one_light(stream: &mut WorldStream, camera: [f32; 3]) {
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 1);
    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("light worker completion");
    stream.accept_light_completion(completion);
}

fn settle_light(stream: &mut WorldStream, camera: [f32; 3]) {
    for _ in 0..128 {
        stream.dispatch_light_jobs(camera, usize::MAX);
        if stream.pending_light.is_empty() && stream.in_flight_light.is_empty() {
            return;
        }
        let completion = stream
            .light_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("light convergence made no bounded progress");
        stream.accept_light_completion(completion);
    }
    panic!("light convergence exceeded the bounded test iteration limit");
}

fn install_current_light(
    stream: &mut WorldStream,
    key: SubChunkKey,
    block: u8,
    sky: u8,
    direct: bool,
) {
    let resident_blocks = stream.store.sub_chunk(key).is_some();
    if resident_blocks {
        stream.resident.insert(key);
        stream.known_air.remove(&key);
    } else {
        stream.record_known_air(key);
    }
    stream.next_block_generation = stream.next_block_generation.wrapping_add(1).max(1);
    let block_generation = stream.next_block_generation;
    let light_revision = block_generation.wrapping_add(10_000);
    stream.block_generations.insert(key, block_generation);
    let light = SubChunkLight::uniform(block, sky, light_revision).unwrap();
    if resident_blocks {
        stream.light_store.insert_resident(key, light);
    } else {
        stream.light_store.insert_known_air(key, light);
    }
    stream.light_ownership.insert(
        key,
        LightOwnership {
            block_generation,
            light_revision,
        },
    );
    stream.direct_sky.insert(
        key,
        StoredDirectSky {
            light_revision,
            mask: Arc::new(DirectSkyMask::Uniform(direct)),
        },
    );
    stream.light_revisions.entries.remove(&key);
    stream.pending_light.remove(&key);
}

fn synthetic_light_completion(
    stream: &mut WorldStream,
    key: SubChunkKey,
    direct_sky: DirectSkyMask,
    light_levels_changed: bool,
    direct_sky_changed: bool,
    changed_faces: [bool; 6],
) -> LightCompletion {
    let revision = stream.mark_light_dirty_exact(key).unwrap();
    let identity = LightJobIdentity {
        revision,
        block_generation: stream.block_generations[&key],
        previous_light_generation: stream
            .light_store
            .light(key)
            .map(|light| light.generation()),
    };
    stream.pending_light.remove(&key);
    stream.in_flight_light.insert(key, identity);
    LightCompletion {
        key,
        identity,
        result: Ok(SolvedLightJob {
            replacement: stream.light_store.light(key).unwrap().as_ref().clone(),
            direct_sky: Arc::new(direct_sky),
            used_uniform_fast_path: false,
            light_levels_changed,
            direct_sky_changed,
            changed_faces,
        }),
        queue_wait: Duration::ZERO,
        duration: Duration::from_millis(3),
    }
}

mod cases_01;
mod cases_02;
