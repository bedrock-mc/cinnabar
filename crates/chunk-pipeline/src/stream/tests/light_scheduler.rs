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
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

pub(super) fn lit_stream(dimension: i32) -> WorldStream {
    lit_stream_with_assets(dimension, light_test_assets())
}

/// Builds a synthetic lighting stream with the supplied runtime assets.
fn lit_stream_with_assets(dimension: i32, assets: RuntimeAssets) -> WorldStream {
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
        Arc::new(assets),
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
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: assets::VisualSupport::Exact,
        faces: [0; 6],
        flags,
        kind,
        contributor_role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
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
    settle_light_with_retired_wait(stream, camera, || {});
}

/// Reports an observed occupied retired slot before waiting for its worker to release it.
fn settle_light_with_retired_wait(
    stream: &mut WorldStream,
    camera: [f32; 3],
    mut on_retired_wait: impl FnMut(),
) {
    // Emitter-dense fields take hundreds of boundary exchanges to settle.
    for _ in 0..1024 {
        stream.dispatch_light_jobs(camera, usize::MAX);
        if stream.lighting.jobs.in_flight.is_empty() {
            // Retired jobs can still own slots after their in-flight entries are removed.
            // Wait for real worker progress instead of spending scheduler turns spinning.
            let deadline = Instant::now() + Duration::from_secs(5);
            while stream.lighting.running_jobs.load(Ordering::Acquire) != 0 {
                on_retired_wait();
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

pub(super) fn install_current_light(
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
mod column_propagation;
mod convergence;
mod dependency_lifecycle;
mod filter_dominance;
mod mesh_fairness;
mod mesh_light_dependencies;
mod provenance;
mod scheduling_priority;
mod sky_boundary;
mod worker_caps;

mod mesh_admission;
mod mesh_preparation;

mod backlog;
mod mixed_prefix;
mod mutation_summary;
mod noop_gates;
mod pending_coalescing;
mod resident_air;
mod startup_lanes;
mod transfer_priority;
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
    let (entered, waiting) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let observed = waiting.recv_timeout(Duration::from_secs(5));
        running.store(0, Ordering::Release);
        observed.expect("convergence must reach the occupied retired slot before release");
    });
    let mut entered = Some(entered);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settle_light_with_retired_wait(&mut stream, [8.0; 3], || {
            if let Some(entered) = entered.take() {
                entered.send(()).unwrap();
            }
        });
    }));
    let joined = worker.join();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    joined.unwrap();
    assert!(stream.light_is_current(key));
}
