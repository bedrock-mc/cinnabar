use std::sync::OnceLock;

use assets::{
    BlockFlags, BlockVisual, CompiledAssets, DIAGNOSTIC_MATERIAL, Material, NetworkIdMode,
    RuntimeAssets, TextureArray, TextureMip, encode_blob,
};
use bevy::{
    camera::primitives::Aabb,
    prelude::{App, MinimalPlugins, Vec3, Visibility},
};
use render::{
    BlockClassifier, ChunkRenderInstance, ChunkRenderQueue, ChunkRenderQueueLimits,
    ChunkTextureAssetIdentity, ChunkUploadPriority, DebugWorldPlugin, Face, MATERIAL_UV_REFLECT_U,
    MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270,
    Neighbourhood, TextureArrayLimits, TextureLimitError, greedy_texture_uv, mesh_sub_chunk,
    plan_texture_mip_uploads, texture_asset_needs_rebuild,
};
use world::{SubChunk, SubChunkKey};

const AIR: u32 = 12_530;

fn runtime_assets() -> &'static RuntimeAssets {
    static ASSETS: OnceLock<RuntimeAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let mut visuals = vec![
            BlockVisual {
                faces: [DIAGNOSTIC_MATERIAL; 6],
                flags: BlockFlags::empty(),
            };
            14
        ];
        for material_id in 1..14_u32 {
            visuals[material_id as usize] = BlockVisual {
                faces: [material_id; 6],
                flags: BlockFlags::CUBE_GEOMETRY,
            };
        }
        let compiled = CompiledAssets {
            visuals: visuals.into_boxed_slice(),
            hashed: Box::new([]),
            materials: vec![Material { layer: 0, flags: 0 }; 14].into_boxed_slice(),
            textures: TextureArray {
                layers: 1,
                mips: [16_u32, 8, 4, 2, 1]
                    .into_iter()
                    .map(|size| TextureMip {
                        size,
                        rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            },
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

#[test]
fn render_queue_enforces_item_and_byte_limits_without_losing_replacements() {
    let first = SubChunkKey::new(0, 0, 0, 0);
    let second = SubChunkKey::new(0, 1, 0, 0);
    let third = SubChunkKey::new(0, 2, 0, 0);
    let mut queue = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 2,
        max_bytes: 48,
    });

    queue
        .try_insert(first, solid_mesh(1), ChunkUploadPriority::new(0.0))
        .unwrap();
    queue
        .try_insert(
            second,
            render::ChunkMesh::default(),
            ChunkUploadPriority::new(1.0),
        )
        .unwrap();
    assert_eq!(queue.retained_len(), 2);
    assert_eq!(queue.pending_bytes(), 48);

    let rejected = queue
        .try_insert(third, solid_mesh(3), ChunkUploadPriority::new(2.0))
        .unwrap_err();
    assert_eq!(rejected.quad_count(), 6);
    assert_eq!(queue.retained_len(), 2);
    assert_eq!(queue.pending_bytes(), 48);

    queue
        .try_update(
            first,
            render::ChunkMesh::default(),
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    queue
        .try_update(second, solid_mesh(2), ChunkUploadPriority::new(1.0))
        .unwrap();
    let superseding = queue
        .try_update(first, solid_mesh(4), ChunkUploadPriority::new(0.0))
        .unwrap_err();
    assert_eq!(superseding.quad_count(), 6);
    assert_eq!(queue.pending_bytes(), 48);

    queue.try_remove(first).unwrap();
    assert_eq!(queue.retained_len(), 2, "removal remains losslessly queued");
    assert!(queue.try_remove(third).is_err());
}

#[test]
fn rejected_mesh_is_eventually_delivered_after_the_capped_queue_drains() {
    let first = SubChunkKey::new(0, 0, 0, 0);
    let second = SubChunkKey::new(0, 1, 0, 0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
            max_items: 1,
            max_bytes: 48,
        }))
        .add_plugins(DebugWorldPlugin::new(1));

    let rejected = {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_insert(first, solid_mesh(1), ChunkUploadPriority::new(0.0))
            .unwrap();
        queue
            .try_insert(second, solid_mesh(2), ChunkUploadPriority::new(1.0))
            .unwrap_err()
    };

    app.update();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(second, rejected, ChunkUploadPriority::new(1.0))
        .unwrap();
    app.update();

    let mut keys = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .iter(app.world())
        .map(ChunkRenderInstance::key)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(keys, [first, second]);
}

fn solid_mesh(runtime_id: u32) -> render::ChunkMesh {
    let mut encoded = vec![9, 1, 0, 1];
    encoded.extend(zig_zag_i32(runtime_id as i32));
    let sub_chunk = SubChunk::decode(&encoded).expect("uniform sub-chunk");
    mesh_sub_chunk(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub_chunk,
    )
}

#[test]
fn upload_budget_is_nearest_first_and_queue_supports_update_remove() {
    let near = SubChunkKey::new(0, 1, 2, 3);
    let far = SubChunkKey::new(0, 20, 2, 20);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(DebugWorldPlugin::new(1));

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_insert(far, solid_mesh(7), ChunkUploadPriority::new(100.0))
            .unwrap();
        queue
            .try_insert(near, solid_mesh(11), ChunkUploadPriority::new(1.0))
            .unwrap();
    }
    app.update();

    let rendered = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .iter(app.world())
        .map(|instance| {
            (
                instance.key(),
                instance.quad_count(),
                instance.quads()[0].material_id(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec![(near, 6, 11)]);
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<(&Aabb, &Visibility)>()
            .iter(app.world())
            .count(),
        1,
    );

    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&ChunkRenderInstance>()
            .iter(app.world())
            .count(),
        2
    );
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        0
    );

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_update(far, solid_mesh(13), ChunkUploadPriority::new(0.0))
            .unwrap();
        queue.try_remove(near).unwrap();
    }
    app.update();
    app.update();

    let rendered = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .iter(app.world())
        .map(|instance| {
            (
                instance.key(),
                instance.quad_count(),
                instance.quads()[0].material_id(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec![(far, 6, 13)]);
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        0
    );

    assert!(
        ChunkUploadPriority::from_camera(near, Vec3::splat(16.0))
            < ChunkUploadPriority::from_camera(far, Vec3::splat(16.0))
    );
}

#[test]
fn packed_chunk_shader_parses_and_validates() {
    let shader = include_str!("../src/chunk.wgsl").replacen(
        "#import bevy_render::view::View",
        "struct View { clip_from_world: mat4x4<f32>, }",
        1,
    );
    let module = naga::front::wgsl::parse_str(&shader).expect("parse packed chunk WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate packed chunk WGSL");

    for binding in 3..=5 {
        assert!(
            shader.contains(&format!("@group(0) @binding({binding})")),
            "packed chunk shader is missing global texture binding {binding}"
        );
    }
    assert!(shader.contains("textureSample(block_textures, block_sampler"));
    assert!(shader.contains("@interpolate(flat) layer: u32"));
    assert!(shader.contains("greedy_uv"));
    assert!(!shader.contains("debug_color"));
}

#[test]
fn greedy_uvs_match_every_face_and_repeat_once_per_block() {
    // Resource-pack PNGs and WGPU both treat v=0 as the top row. Vertical
    // faces must therefore assign v=0 to their upper-Y geometry corners.
    let vertical_standard = [[0.0, 1.0], [16.0, 1.0], [16.0, 0.0], [0.0, 0.0]];
    let vertical_transposed = [[0.0, 1.0], [0.0, 0.0], [16.0, 0.0], [16.0, 1.0]];
    let horizontal_standard = [[0.0, 0.0], [16.0, 0.0], [16.0, 1.0], [0.0, 1.0]];
    let horizontal_transposed = [[0.0, 0.0], [0.0, 1.0], [16.0, 1.0], [16.0, 0.0]];

    for face in [Face::NegativeX, Face::PositiveZ] {
        assert_eq!(
            std::array::from_fn(|corner| greedy_texture_uv(face, corner as u32, 16, 1, 0)),
            vertical_standard,
            "unexpected UV corners for {face:?}"
        );
    }
    for face in [Face::PositiveX, Face::NegativeZ] {
        assert_eq!(
            std::array::from_fn(|corner| greedy_texture_uv(face, corner as u32, 16, 1, 0)),
            vertical_transposed,
            "unexpected UV corners for {face:?}"
        );
    }
    assert_eq!(
        std::array::from_fn(|corner| {
            greedy_texture_uv(Face::NegativeY, corner as u32, 16, 1, 0)
        }),
        horizontal_standard
    );
    assert_eq!(
        std::array::from_fn(|corner| {
            greedy_texture_uv(Face::PositiveY, corner as u32, 16, 1, 0)
        }),
        horizontal_transposed
    );

    assert_eq!(
        std::array::from_fn(|corner| greedy_texture_uv(Face::PositiveZ, corner as u32, 1, 1, 0)),
        [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]
    );
    assert_eq!(
        std::array::from_fn(|corner| greedy_texture_uv(Face::PositiveZ, corner as u32, 16, 16, 0)),
        [[0.0, 16.0], [16.0, 16.0], [16.0, 0.0], [0.0, 0.0]]
    );
}

#[test]
fn material_uv_flags_rotate_and_reflect_greedy_coordinates() {
    let face = Face::PositiveZ;
    let base = greedy_texture_uv(face, 1, 4, 2, 0);
    assert_eq!(base, [4.0, 2.0]);
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_ROTATE_90),
        [2.0, 0.0]
    );
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_ROTATE_180),
        [0.0, 0.0]
    );
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_ROTATE_270),
        [0.0, 4.0]
    );
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_REFLECT_U),
        [0.0, 2.0]
    );
    assert_eq!(
        greedy_texture_uv(face, 1, 4, 2, MATERIAL_UV_REFLECT_V),
        [4.0, 0.0]
    );
}

#[test]
fn adapter_limits_reject_oversized_texture_arrays() {
    assert_eq!(
        TextureArrayLimits {
            max_layers: 4,
            max_dimension_2d: 16,
        }
        .validate(5, 16),
        Err(TextureLimitError::Layers {
            requested: 5,
            supported: 4,
        })
    );
    assert_eq!(
        TextureArrayLimits {
            max_layers: 4,
            max_dimension_2d: 8,
        }
        .validate(4, 16),
        Err(TextureLimitError::Dimension {
            requested: 16,
            supported: 8,
        })
    );
}

#[test]
fn mip_upload_plan_preserves_exact_layer_offsets_and_row_padding() {
    let plans = plan_texture_mip_uploads(runtime_assets().texture_array(), 256)
        .expect("plan synthetic texture upload");
    assert_eq!(plans.len(), 5);
    assert_eq!(
        plans
            .iter()
            .map(|plan| (
                plan.mip_level,
                plan.size,
                plan.bytes_per_row,
                plan.rows_per_image,
                plan.layer_source_offsets.as_ref(),
                plan.layer_staging_offsets.as_ref(),
                plan.staging_bytes,
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, 16, 256, 16, &[0][..], &[0][..], 4096),
            (1, 8, 256, 8, &[0][..], &[0][..], 2048),
            (2, 4, 256, 4, &[0][..], &[0][..], 1024),
            (3, 2, 256, 2, &[0][..], &[0][..], 512),
            (4, 1, 256, 1, &[0][..], &[0][..], 256),
        ]
    );
    assert_eq!(runtime_assets().materials().len(), 14);

    let two_layers = TextureArray {
        layers: 2,
        mips: vec![TextureMip {
            size: 2,
            rgba8: vec![0; 2 * 2 * 4 * 2].into_boxed_slice(),
        }]
        .into_boxed_slice(),
    };
    let plan = plan_texture_mip_uploads(&two_layers, 256)
        .expect("plan two-layer texture upload")
        .remove(0);
    assert_eq!(plan.layer_source_offsets.as_ref(), [0, 16]);
    assert_eq!(plan.layer_staging_offsets.as_ref(), [0, 512]);
    assert_eq!(plan.staging_bytes, 1024);
}

#[test]
fn global_texture_bind_group_rebuilds_only_for_new_asset_identity_or_revision() {
    let current = ChunkTextureAssetIdentity::for_test(0x1000, 7);
    assert!(!texture_asset_needs_rebuild(Some(current), current));
    assert!(texture_asset_needs_rebuild(
        Some(current),
        ChunkTextureAssetIdentity::for_test(0x2000, 7)
    ));
    assert!(texture_asset_needs_rebuild(
        Some(current),
        ChunkTextureAssetIdentity::for_test(0x1000, 8)
    ));
    assert!(texture_asset_needs_rebuild(None, current));
}
