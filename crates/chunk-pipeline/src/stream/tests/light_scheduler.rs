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
        local_player_unique_id: 1,
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
            local_player_unique_id: 1,
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

pub(super) fn light_test_assets() -> RuntimeAssets {
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
        support: assets::VisualSupport::Exact,
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
            ..assets::Material::unvaried()
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
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

fn complete_one_light(stream: &mut WorldStream, camera: [f32; 3]) {
    let dispatched = stream.dispatch_light_jobs(camera, 1);
    assert!(dispatched > 0);
    for _ in 0..dispatched {
        let completion = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(2))
            .expect("light worker completion");
        stream.accept_light_completion(completion);
    }
}

/// Advances bounded scheduler turns until test lighting is current and its workers are idle.
pub(super) fn settle_light(stream: &mut WorldStream, camera: [f32; 3]) {
    for _ in 0..128 {
        stream.dispatch_light_jobs(camera, usize::MAX);
        if stream.lighting.jobs.in_flight.is_empty() {
            // Retired jobs can still own slots after their in-flight entries are removed.
            // Wait for real worker progress instead of spending scheduler turns spinning.
            let deadline = Instant::now() + Duration::from_secs(5);
            while stream.lighting.running_jobs.load(Ordering::Acquire) != 0 {
                assert!(
                    Instant::now() < deadline,
                    "retired light workers did not release their slots"
                );
                std::thread::yield_now();
            }
            if stream.lighting.jobs.pending.is_empty() {
                return;
            }
            // A finished scan round can defer ready work until the next turn.
            continue;
        }
        let completion = stream
            .lighting
            .rx
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
    let resident_blocks = stream.authority.terrain().sub_chunk(key).is_some();
    if resident_blocks {
        stream.resident.insert(key);
        stream.known_air.remove(&key);
    } else {
        stream.record_known_air(key);
    }
    stream.lighting.next_block_generation =
        stream.lighting.next_block_generation.wrapping_add(1).max(1);
    let block_generation = stream.lighting.next_block_generation;
    let light_revision = block_generation.wrapping_add(10_000);
    stream
        .lighting
        .block_generations
        .insert(key, block_generation);
    let light = SubChunkLight::uniform(block, sky, light_revision).unwrap();
    if resident_blocks {
        stream.lighting.store.insert_resident(key, light);
    } else {
        stream.lighting.store.insert_known_air(key, light);
    }
    stream.lighting.ownership.insert(
        key,
        LightOwnership {
            block_generation,
            light_revision,
        },
    );
    stream.lighting.direct_sky.insert(
        key,
        StoredDirectSky {
            light_revision,
            mask: Arc::new(DirectSkyMask::Uniform(direct)),
        },
    );
    stream.lighting.revisions.entries.remove(&key);
    stream.lighting.jobs.pending.remove(&key);
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
        block_generation: stream.lighting.block_generations[&key],
        previous_light_generation: stream
            .lighting
            .store
            .light(key)
            .map(|light| light.generation()),
        batch_id: 0,
        urgent: false,
    };
    stream.lighting.jobs.pending.remove(&key);
    stream.lighting.jobs.in_flight.insert(key, identity);
    LightCompletion {
        key,
        identity,
        result: Ok(SolvedLightJob {
            replacement: stream.lighting.store.light(key).unwrap().as_ref().clone(),
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

mod air_fixed_point;
mod boundary_dominance;
mod cases_01;
mod cases_02;
mod filter_dominance;
mod sky_boundary;

mod mesh_admission;

mod backlog;
mod mixed_prefix;
mod mutation_summary;
mod pending_coalescing;
mod resident_air;
mod startup_lanes;
mod uniform_air;

/// Retired workers without tracked completions must release their slots before convergence retries.
#[test]
fn settle_light_waits_for_retired_worker_slots() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_light_dirty_exact(key).unwrap();
    let running = Arc::clone(&stream.lighting.running_jobs);
    running.store(effective_light_job_cap(), Ordering::Release);
    assert!(stream.lighting.jobs.in_flight.is_empty());
    assert_eq!(stream.dispatch_light_jobs([8.0; 3], usize::MAX), 0);
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        running.store(0, Ordering::Release);
    });
    settle_light(&mut stream, [8.0; 3]);
    worker.join().unwrap();
    assert!(stream.light_is_current(key));
}
