use std::{
    fs,
    mem::size_of,
    path::Path,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use asset_compiler::compile_pack as compile_pack_with_lights;
use assets::{
    ANIMATION_FLAG_BLEND, Animation, AssetError, BlockFlags, BlockVisual, CompiledAssets,
    CompiledBiomeAssets, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_CUTOUT,
    MODEL_QUAD_FLAG_TWO_SIDED, Material, ModelStateField, NO_ANIMATION, NO_MODEL_TEMPLATE,
    NetworkIdMode, RegistryRecord, RuntimeAssets, TextureArray, TextureMip, TexturePage,
    TextureRef, VisualKind, encode_blob, read_registry,
};

fn compile_pack(root: &Path, records: &[RegistryRecord]) -> Result<CompiledAssets, AssetError> {
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap_or(0)
    ];
    compile_pack_with_lights(root, records, &lights)
}
use bevy::{
    camera::primitives::Aabb,
    prelude::{App, Mat4, MinimalPlugins, Quat, Vec3, Visibility},
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use meshing::{
    BlockClassifier, Face, Neighbourhood, PackedBiomeRecord, PackedQuad, mesh_sub_chunk,
};
use render::{
    AnimationFrameSample, ChunkAnimationClock, ChunkRenderInstance, ChunkRenderPlugin,
    ChunkRenderQueue, ChunkRenderQueueLimits, ChunkTextureAssetIdentity, ChunkUploadPriority,
    MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180,
    MATERIAL_UV_ROTATE_270, MAX_TRANSPARENT_DRAW_REFS, MAX_TRANSPARENT_VIEWS,
    PackedTransparentDrawRef, PresentedFrameAck, RenderViewCohort, TRANSPARENT_REF_BUFFER_BYTES,
    TRANSPARENT_REF_SLOT_BYTES, TextureArrayLimits, TextureLimitError, TexturePageBinding,
    TransparentAllocationIdentity, TransparentOrderedSnapshot, TransparentSortCandidate,
    TransparentSortError, TransparentSortJobGate, TransparentSortMetrics,
    TransparentSortMetricsSnapshot, TransparentSortResult, TransparentSortState,
    ViewSortGeneration, ViewSortKey, diagnostic_texture_page,
    direct_transparent_draw_args_for_test, greedy_texture_uv, mdi_transparent_draw_args_for_test,
    plan_texture_mip_uploads, plan_texture_page_bindings, select_animation_frames,
    sort_transparent_candidates_for_test, texture_asset_needs_rebuild,
};

const CHUNK_RENDERER_SOURCE: &str = concat!(
    include_str!("../src/chunk/transparent/sort.rs"),
    include_str!("../src/chunk/transparent/sort_state.rs"),
    include_str!("../src/chunk/transparent/retirement.rs"),
    include_str!("../src/chunk/transparent/model.rs"),
    include_str!("../src/chunk/biome_tints.rs"),
    include_str!("../src/chunk/textures.rs"),
    include_str!("../src/chunk/api.rs"),
    include_str!("../src/chunk/presentation/frame_probe.rs"),
    include_str!("../src/chunk/queue.rs"),
    include_str!("../src/chunk/plugin.rs"),
    include_str!("../src/chunk/extract.rs"),
    include_str!("../src/chunk/pipeline/layouts.rs"),
    include_str!("../src/chunk/pipeline/opaque.rs"),
    include_str!("../src/chunk/pipeline/model.rs"),
    include_str!("../src/chunk/pipeline/liquid.rs"),
    include_str!("../src/chunk/gpu/types.rs"),
    include_str!("../src/chunk/gpu/bind_groups.rs"),
    include_str!("../src/chunk/gpu/arena.rs"),
    include_str!("../src/chunk/gpu/upload.rs"),
    include_str!("../src/chunk/transparent/sort_prepare.rs"),
    include_str!("../src/chunk/gpu/layout.rs"),
    include_str!("../src/chunk/pipeline/commands.rs"),
    include_str!("../src/chunk/draw.rs"),
    include_str!("../src/chunk/transparent/liquid.rs"),
);
use world::{DecodedBiomeColumn, SubChunk, SubChunkKey};

const AIR: u32 = 12_530;

fn standalone_world_shader(source: &str) -> String {
    let lighting = include_str!("../src/lighting.wgsl").replacen(
        "#define_import_path cinnabar::lighting",
        "",
        1,
    );
    let biome_tint = include_str!("../src/biome_tint.wgsl").replacen(
        "#define_import_path cinnabar::biome_tint",
        "",
        1,
    );
    source
        .replacen(
            "#import bevy_render::view::View",
            "struct View { clip_from_world: mat4x4<f32>, world_position: vec3<f32>, }",
            1,
        )
        .replacen(
            "#import cinnabar::lighting::{light_ao_factor, light_brightness, lit_colour}",
            &lighting,
            1,
        )
        .replacen(
            "#import cinnabar::biome_tint::blended_biome_tint",
            &biome_tint,
            1,
        )
}

fn entry_points_use_binding(module: &naga::Module, stage: naga::ShaderStage, binding: u32) -> bool {
    let Some((handle, _)) = module.global_variables.iter().find(|(_, variable)| {
        variable
            .binding
            .as_ref()
            .is_some_and(|resource| resource.group == 0 && resource.binding == binding)
    }) else {
        return false;
    };
    let mut entries = module
        .entry_points
        .iter()
        .filter(|entry| entry.stage == stage)
        .peekable();
    entries.peek().is_some()
        && entries.all(|entry| function_uses_global(module, &entry.function, handle, 0))
}

fn function_uses_global(
    module: &naga::Module,
    function: &naga::Function,
    global: naga::Handle<naga::GlobalVariable>,
    depth: usize,
) -> bool {
    if function.expressions.iter().any(|(_, expression)| {
        matches!(expression, naga::Expression::GlobalVariable(handle) if *handle == global)
    }) {
        return true;
    }
    if depth >= 16 {
        return false;
    }
    let mut calls = Vec::new();
    collect_function_calls(&function.body, &mut calls);
    calls
        .into_iter()
        .any(|handle| function_uses_global(module, &module.functions[handle], global, depth + 1))
}

fn collect_function_calls(block: &naga::Block, calls: &mut Vec<naga::Handle<naga::Function>>) {
    for statement in block {
        match statement {
            naga::Statement::Call { function, .. } => calls.push(*function),
            naga::Statement::Block(block) => collect_function_calls(block, calls),
            naga::Statement::If { accept, reject, .. } => {
                collect_function_calls(accept, calls);
                collect_function_calls(reject, calls);
            }
            naga::Statement::Switch { cases, .. } => {
                for case in cases {
                    collect_function_calls(&case.body, calls);
                }
            }
            naga::Statement::Loop {
                body, continuing, ..
            } => {
                collect_function_calls(body, calls);
                collect_function_calls(continuing, calls);
            }
            _ => {}
        }
    }
}

fn entry_points_call_function(
    module: &naga::Module,
    stage: naga::ShaderStage,
    function_name: &str,
) -> bool {
    let Some((target, _)) = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some(function_name))
    else {
        return false;
    };
    let mut entries = module
        .entry_points
        .iter()
        .filter(|entry| entry.stage == stage)
        .peekable();
    entries.peek().is_some()
        && entries.all(|entry| function_calls(module, &entry.function, target, 0))
}

fn function_calls(
    module: &naga::Module,
    function: &naga::Function,
    target: naga::Handle<naga::Function>,
    depth: usize,
) -> bool {
    if depth >= 16 {
        return false;
    }
    let mut calls = Vec::new();
    collect_function_calls(&function.body, &mut calls);
    calls.contains(&target)
        || calls
            .into_iter()
            .any(|handle| function_calls(module, &module.functions[handle], target, depth + 1))
}

fn runtime_assets() -> &'static RuntimeAssets {
    static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let mut visuals = vec![
            BlockVisual {
                faces: [DIAGNOSTIC_MATERIAL; 6],
                flags: BlockFlags::empty(),
                kind: VisualKind::Diagnostic,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            };
            14
        ];
        for material_id in 1..14_u32 {
            visuals[material_id as usize] = BlockVisual {
                faces: [material_id; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                contributor_role: assets::ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            };
        }
        let compiled = CompiledAssets {
            visuals: visuals.into_boxed_slice(),
            hashed: Box::new([]),
            materials: vec![
                Material {
                    texture: TextureRef::DIAGNOSTIC,
                    flags: 0,
                    animation: NO_ANIMATION
                };
                14
            ]
            .into_boxed_slice(),
            light_properties: vec![assets::LightProperties::default(); 14].into_boxed_slice(),
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
        let blob = encode_blob(&compiled).expect("encode synthetic plugin assets");
        RuntimeAssets::decode(&blob).expect("decode synthetic plugin assets")
    })
}

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn write_flowerbed_pack(root: &Path) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create FlowerBed fixture tree");
    fs::write(
        root.join("blocks.json"),
        r#"{"wildflowers":{"textures":"wildflowers"}}"#,
    )
    .expect("write FlowerBed blocks routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"wildflowers":{"textures":["textures/blocks/wildflowers","textures/blocks/wildflowers_stem"]}}}"#,
    )
    .expect("write FlowerBed terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write empty flipbook inventory");

    for (index, name) in ["wildflowers", "wildflowers_stem"].into_iter().enumerate() {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for (pixel_index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[
                20 + index as u8,
                80 + (pixel_index % 16) as u8,
                140,
                if pixel_index % 3 == 0 { 0 } else { 255 },
            ]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode FlowerBed fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write FlowerBed fixture PNG");
    }
}

fn flowerbed_runtime_assets() -> &'static RuntimeAssets {
    static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let directory = tempfile::tempdir().expect("create FlowerBed render fixture");
        write_flowerbed_pack(directory.path());
        let generated = read_registry(include_bytes!("../../assets/data/block-registry-v1001.bin"))
            .expect("decode committed FlowerBed registry");
        let records = [0_u32, 3, 7]
            .into_iter()
            .enumerate()
            .map(|(sequential_id, growth)| {
                let mut record = generated
                    .iter()
                    .find(|record| {
                        record.name.as_ref() == "minecraft:wildflowers"
                            && record.model_state.get(ModelStateField::Growth) == Some(growth)
                            && record.model_state.get(ModelStateField::Orientation) == Some(2)
                    })
                    .unwrap_or_else(|| panic!("missing generated growth={growth} FlowerBed record"))
                    .clone();
                record.sequential_id = sequential_id as u32;
                record.network_hash = 50_000 + sequential_id as u32;
                record
            })
            .collect::<Vec<_>>();
        let compiled = compile_pack(directory.path(), &records)
            .expect("compile real FlowerBed render fixture through assets");
        let blob = encode_blob(&compiled).expect("encode compiled FlowerBed render fixture");
        RuntimeAssets::decode(&blob).expect("decode compiled FlowerBed render fixture")
    })
}

fn flowerbed_sub_chunk(placements: &[([u8; 3], usize)]) -> SubChunk {
    let bits_per_index = 2_usize;
    let values_per_word = 32 / bits_per_index;
    let mut words = vec![0_u32; 4096_usize.div_ceil(values_per_word)];
    for &([x, y, z], runtime_id) in placements {
        assert!(x < 16 && y < 16 && z < 16);
        assert!(runtime_id < 3);
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        let shift = (linear % values_per_word) * bits_per_index;
        words[linear / values_per_word] |= (runtime_id as u32 + 1) << shift;
    }

    let mut encoded = vec![9, 1, 0, 5];
    for word in words {
        encoded.extend_from_slice(&word.to_le_bytes());
    }
    encoded.extend(zig_zag_i32(4));
    encoded.extend(zig_zag_i32(AIR as i32));
    encoded.extend(zig_zag_i32(0));
    encoded.extend(zig_zag_i32(1));
    encoded.extend(zig_zag_i32(2));
    SubChunk::decode(&encoded).expect("decode packed FlowerBed subchunk")
}

#[path = "plugin/contracts.rs"]
mod contracts;
#[path = "plugin/queue.rs"]
mod queue;
#[path = "plugin/textures.rs"]
mod textures;
