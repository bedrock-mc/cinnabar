use std::{
    mem::size_of,
    sync::{Arc, OnceLock},
};

use crate::VisibilityKeyDigest;
use assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, Material, NO_ANIMATION,
    NO_MODEL_TEMPLATE, NetworkIdMode, TextureMip, TexturePage, TextureRef, VisualKind, encode_blob,
};
use bevy::{
    prelude::*,
    render::render_resource::{DownlevelFlags, DrawIndexedIndirectArgs, WgpuFeatures},
};
use world::SubChunk;

use super::*;

fn target_expectation(
    now: Instant,
    manifest: impl IntoIterator<Item = (SubChunkKey, u64)>,
) -> TargetRenderExpectation {
    TargetRenderExpectation {
        cohort: RenderViewCohort::new(0, [65, 65], 16),
        source_cohort: Some(RenderViewCohort::new(0, [0, 0], 16)),
        target_keys: None,
        manifest: Arc::from(manifest.into_iter().collect::<Vec<_>>()),
        view_generation: 1,
        render_ready_at: now,
    }
}

fn opaque_runtime_assets() -> &'static RuntimeAssets {
    static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let compiled = CompiledAssets {
            visuals: vec![
                BlockVisual {
                    faces: [0; 6],
                    flags: BlockFlags::AIR,
                    kind: VisualKind::Invisible,
                    contributor_role: assets::ContributorRole::Air,
                    model_template: NO_MODEL_TEMPLATE,
                    animation: NO_ANIMATION,
                    variant: 0,
                },
                BlockVisual {
                    faces: [1; 6],
                    flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                    kind: VisualKind::Cube,
                    contributor_role: assets::ContributorRole::Primary,
                    model_template: NO_MODEL_TEMPLATE,
                    animation: NO_ANIMATION,
                    variant: 0,
                },
            ]
            .into_boxed_slice(),
            light_properties: vec![assets::LightProperties::default(); 2].into_boxed_slice(),
            hashed: Box::new([]),
            materials: vec![
                Material {
                    texture: TextureRef::DIAGNOSTIC,
                    flags: 0,
                    animation: NO_ANIMATION
                };
                2
            ]
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
        let blob = encode_blob(&compiled).expect("encode opaque plugin test assets");
        RuntimeAssets::decode(&blob).expect("decode opaque plugin test assets")
    })
}

fn solid_test_mesh() -> ChunkMesh {
    let sub_chunk = SubChunk::decode(&[9, 1, 0, 1, 2]).expect("uniform test sub-chunk");
    meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        opaque_runtime_assets(),
        NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &sub_chunk,
    )
}

fn committed_transparent_state(
    key: &ViewSortKey,
    refs: Vec<PackedTransparentDrawRef>,
) -> TransparentSortState {
    let mut state = TransparentSortState::with_upload_cap(64);
    let generation = state.request(key);
    assert_eq!(
        state.complete(TransparentSortResult::new(generation, key.clone(), refs).unwrap()),
        Ok(false)
    );
    assert!(state.acknowledge_upload());
    state
}

fn retirement_test_allocation() -> ArenaAllocation {
    ArenaAllocation {
        generation: 7,
        tint_identity: ChunkBiomeTintIdentity::new(2, 2),
        cube_range: Some(0..2),
        cube_lighting_range: Some(20..24),
        model_range: None,
        model_lighting_range: None,
        model_draw_range: None,
        transparent_model_draw_range: None,
        liquid_range: Some(8..16),
        liquid_lighting_range: Some(16..20),
        quad_capacity: 2,
        geometry_stream_range: Some(0..24),
        geometry_stream_capacity: 30,
        biome_range: 2..6,
        biome_capacity: 4,
        gpu: GpuChunkAllocation {
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 7,
            tint_identity: ChunkBiomeTintIdentity::new(2, 2),
            quad_range: 0..2,
            cube_lighting_range: Some(20..24),
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: Some(8..16),
            liquid_lighting_range: Some(16..20),
            has_depth_liquid: false,
            has_transparent_liquid: true,
            depth_liquid_range: None,
            metadata_index: 3,
        },
    }
}

#[path = "gpu/tests.rs"]
mod gpu;
#[path = "gpu/model_tests.rs"]
mod gpu_models;
#[path = "gpu/queue_tests.rs"]
mod gpu_queue;
#[path = "presentation/tests.rs"]
mod presentation;
#[path = "presentation/command_tests.rs"]
mod presentation_commands;
#[path = "presentation/model_witness_tests.rs"]
mod presentation_model_witness;
#[path = "transparent/tests.rs"]
mod transparent;
