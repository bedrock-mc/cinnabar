use std::{
    mem::size_of,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use assets::{
    ANIMATION_FLAG_BLEND, Animation, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets,
    DIAGNOSTIC_MATERIAL, Material, NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets,
    TextureArray, TextureMip, TexturePage, TextureRef, VisualKind, encode_blob,
};
use bevy::{
    camera::primitives::Aabb,
    prelude::{App, MinimalPlugins, Vec3, Visibility},
};
use render::{
    AnimationFrameSample, BlockClassifier, ChunkAnimationClock, ChunkRenderInstance,
    ChunkRenderQueue, ChunkRenderQueueLimits, ChunkTextureAssetIdentity, ChunkUploadPriority,
    DebugWorldPlugin, Face, MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90,
    MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270, Neighbourhood, PackedBiomeRecord, PackedQuad,
    PresentedFrameAck, RenderViewCohort, TextureArrayLimits, TextureLimitError, TexturePageBinding,
    diagnostic_texture_page, greedy_texture_uv, mesh_sub_chunk, plan_texture_mip_uploads,
    plan_texture_page_bindings, select_animation_frames, texture_asset_needs_rebuild,
};
use world::{DecodedBiomeColumn, SubChunk, SubChunkKey};

const AIR: u32 = 12_530;

#[test]
fn task7_streams_share_one_physical_buffer_with_binding_headroom() {
    const CURRENT_VERTEX_STORAGE_BINDINGS: usize = 6;
    const MODEL_TEMPLATE_BINDINGS: usize = 1;
    let plugin = include_str!("../src/plugin.rs");
    let legacy_task7_buffers = [
        "model_buffer: Buffer",
        "model_lighting_buffer: Buffer",
        "liquid_buffer: Buffer",
        "liquid_lighting_buffer: Buffer",
    ]
    .into_iter()
    .filter(|field| plugin.contains(field))
    .count();
    let task7_physical_buffers = usize::from(plugin.contains("geometry_stream_buffer: Buffer"));

    assert_eq!(legacy_task7_buffers, 0);
    assert_eq!(
        task7_physical_buffers, 1,
        "the four logical Task 7 streams must share one physical storage buffer"
    );
    assert!(
        CURRENT_VERTEX_STORAGE_BINDINGS + task7_physical_buffers + MODEL_TEMPLATE_BINDINGS <= 8,
        "the projected vertex-stage storage bindings must fit the common/minimum limit"
    );
}

#[test]
fn crossed_model_pipeline_is_two_sided_and_uses_shared_bounded_bindings() {
    let plugin = include_str!("../src/plugin.rs");
    let shader = include_str!("../src/model.wgsl");
    assert!(plugin.contains("load_internal_asset!(app, MODEL_SHADER_HANDLE, \"model.wgsl\""));
    assert!(plugin.contains("\"packed model pipeline\""));
    assert!(plugin.contains("model_descriptor.primitive.cull_mode = None"));
    assert!(plugin.contains("resource: arena.geometry_stream_buffer.as_entire_binding()"));
    assert!(plugin.contains("resource: texture_assets.model_template_buffer.as_entire_binding()"));
    assert!(plugin.contains("MODEL_TEMPLATE_BINDING_BUDGET: u32 = 8"));
    assert!(plugin.contains("MODEL_VERTEX_STORAGE_BINDINGS: u32 = 8"));
    assert!(plugin.contains("MODEL_VERTEX_STORAGE_BINDINGS <= MODEL_TEMPLATE_BINDING_BUDGET"));
    assert!(shader.contains("@binding(12) var<storage, read> model_templates: array<u32>"));
    assert!(shader.contains("@binding(13) var<storage, read> geometry_streams: array<u32>"));
    assert!(shader.contains("visible_quad_mask"));
    assert!(shader.contains("lighting_base_index"));
    assert!(shader.contains("block_light"));
    assert!(shader.contains("sky_light"));
    assert!(shader.contains("safe_quad_index"));
    let zero_guard = shader
        .find("if (quad_count == 0u)")
        .expect("zero-quad templates require an early invisible return");
    assert!(zero_guard < shader.find("template_quad_base").unwrap());
    assert!(zero_guard < shader.find("let light_word").unwrap());
    assert!(shader.contains("sampled.a < 0.5"));
    assert!(shader.contains("let quad_flags = model_templates[template_quad_base + 11u]"));
    assert!(shader.contains("@builtin(front_facing) front_facing: bool"));
    assert!(shader.contains("if (!front_facing && in.two_sided == 0u) { discard; }"));
    assert!(!shader.contains("face_light"));
}

#[test]
fn crossed_model_shader_parses_validates_and_has_one_shared_binding_shape() {
    let shader = include_str!("../src/model.wgsl").replacen(
        "#import bevy_render::view::View",
        "struct View { clip_from_world: mat4x4<f32>, }",
        1,
    );
    let module = naga::front::wgsl::parse_str(&shader).expect("parse packed model WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate packed model WGSL");
    assert_eq!(shader.matches("@group(0) @binding(").count(), 14);
    for binding in 0..=13 {
        assert!(shader.contains(&format!("@group(0) @binding({binding})")));
    }
}

#[test]
fn crossed_model_direct_and_mdi_commands_have_identical_output_addressing() {
    let source = include_str!("../src/plugin.rs");
    assert!(source.contains("fn model_direct_draw_command("));
    assert!(source.contains("fn model_mdi_draw_command("));
    assert!(source.contains("model_draw_command(allocation, direct_stream_addresses(allocation))"));
    assert!(source.contains("model_draw_command(allocation, mdi_stream_addresses(allocation))"));
}

#[test]
fn queue_counts_every_stream_and_sidecar() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let cube = solid_mesh(1);
    let mesh = render::ChunkMesh::from_streams(
        cube.quads().to_vec(),
        vec![render::PackedModelRef::new(1, 2, 3, u32::MAX)],
        vec![render::PackedQuadLighting::new([0; 4])],
        vec![render::PackedLiquidQuad::new([4, 5, 6, 7])],
        vec![render::PackedQuadLighting::new([0; 4])],
        cube.connectivity(),
    );
    let expected = 6 * size_of::<PackedQuad>()
        + size_of::<render::PackedModelRef>()
        + size_of::<render::PackedQuadLighting>()
        + size_of::<render::PackedLiquidQuad>()
        + size_of::<render::PackedQuadLighting>();
    let mut queue = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 1,
        max_bytes: expected as u64,
    });

    queue
        .try_insert(key, mesh, ChunkUploadPriority::new(0.0))
        .expect("every geometry stream fits at its exact combined byte count");
    assert_eq!(queue.pending_bytes(), expected as u64);
}

#[test]
fn allocated_but_undrawn_target_is_not_exact_presented_evidence() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let acknowledgement = PresentedFrameAck {
        cohort: RenderViewCohort::new(0, [65, 65], 16),
        frame_sequence: 1,
        allocation_manifest: Arc::from([(key, 7)]),
        drawn_manifest: Arc::from([]),
        view_generation: 1,
        render_ready_at: now,
        present_returned_at: now + Duration::from_millis(1),
        gpu_completed_at: now + Duration::from_millis(2),
        missing_target_instances: 0,
        unexpected_target_instances: 0,
        source_instances: 0,
        foreign_instances: 0,
        stale_generation_instances: 0,
        orphan_allocations: 0,
    };

    assert!(!acknowledgement.is_exact());
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

    assert_eq!(shader.matches("@group(0) @binding(").count(), 12);
    for binding in 0..=11 {
        assert!(
            shader.contains(&format!("@group(0) @binding({binding})")),
            "packed chunk shader is missing global texture binding {binding}"
        );
    }
    assert_eq!(shader.matches("textureSample(").count(), 0);
    assert_eq!(shader.matches("textureSampleGrad(").count(), 2);
    assert!(shader.contains("fn sample_texture_ref("));
    assert!(shader.contains("texture_ref >> 31u"));
    assert!(shader.contains("texture_ref & 0x7ffu"));
    assert!(shader.contains("let uv_dx = dpdx(in.uv);"));
    assert!(shader.contains("let uv_dy = dpdy(in.uv);"));
    assert!(shader.contains("if (in.frame_blend > 0.0)"));
    assert!(shader.contains("mix(current_sample, next_sample, in.frame_blend)"));
    assert!(shader.contains("@interpolate(flat) current_texture: u32"));
    assert!(shader.contains("@interpolate(flat) next_texture: u32"));
    assert!(shader.contains("@interpolate(flat) frame_blend: f32"));
    assert!(shader.contains("@location(3) @interpolate(flat) material_flags: u32"));
    assert!(shader.contains("@interpolate(flat) material_flags: u32"));
    assert_eq!(
        shader
            .matches("out.material_flags = material.flags;")
            .count(),
        1
    );
    assert!(shader.contains("material_flags & (1u << 8u)"));
    assert!(shader.contains("sampled.a < 0.5"));
    assert_eq!(shader.matches("discard").count(), 1);
    assert!(shader.contains("material_flags & 0x30u"));
    assert!(shader.contains("material_flags & (1u << 6u)"));
    assert!(shader.contains("mix(sampled.rgb, tinted, sampled.a)"));
    assert!(shader.contains("in.biome_record,"));
    assert!(shader.contains("if ((in.material_flags & (1u << 8u)) != 0u && sampled.a < 0.5) {"));
    assert!(shader.contains("greedy_uv"));
    assert!(shader.contains("var<storage, read> biome_records: array<u32>"));
    assert!(shader.contains("var<storage, read> biome_tints: array<BiomeTintGpu>"));
    assert!(shader.contains("out.biome_record = u32(chunk_origin.value.w);"));
    assert!(shader.contains("local_position - normal * 0.001"));
    assert!(shader.contains("(coordinate.x << 8u) | (coordinate.z << 4u) | coordinate.y"));
    assert!(shader.contains("fn packed_biome_tint_index"));
    assert!(shader.contains("fn unpack_linear_rgb10"));
    assert!(shader.contains("if (tint_kind == 0x10u)"));
    assert!(shader.contains("if (tint_kind == 0x30u)"));
    assert!(shader.contains("switch material_flags & 0x600u"));
    assert!(shader.contains("case 0x200u"));
    assert!(shader.contains("case 0x400u"));
    assert!(shader.contains("case 0x600u"));
    assert!(shader.contains("clock.tick / animation.ticks_per_frame"));
    assert!(shader.contains("clock.tick % animation.ticks_per_frame"));
    assert!(shader.contains("(current_index + 1u) % animation.frame_count"));
    assert!(shader.contains("animation.flags & 1u"));
    assert!(shader.contains("material.animation == 0xffffffffu"));
    assert!(shader.contains("var block_textures_page_0: texture_2d_array<f32>"));
    assert!(shader.contains("var block_textures_page_1: texture_2d_array<f32>"));
    assert!(shader.contains("var<storage, read> animations: array<AnimationGpu>"));
    assert!(shader.contains("var<storage, read> animation_frames: array<u32>"));
    assert!(shader.contains("var<uniform> clock: AnimationClockGpu"));
    assert!(!shader.contains("debug_color"));
}

fn uniform_biome_record(tint_index: u32) -> PackedBiomeRecord {
    let mut encoded = vec![1];
    encoded.extend(zig_zag_i32(42));
    let storage = DecodedBiomeColumn::decode(0, 1, &encoded)
        .expect("uniform biome column")
        .storage(0)
        .expect("uniform biome storage");
    PackedBiomeRecord::from_storage(&storage, |_| tint_index)
}

#[test]
fn render_queue_counts_and_extracts_non_fallback_biome_records() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mesh = solid_mesh(1);
    let biome = uniform_biome_record(7);
    let expected_bytes = 6 * size_of::<PackedQuad>() as u64 + biome.byte_len();
    let mut queue = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 1,
        max_bytes: expected_bytes - 1,
    });

    let (rejected_mesh, rejected_biome) = queue
        .try_insert_with_biome(key, mesh, biome.clone(), ChunkUploadPriority::new(0.0))
        .expect_err("biome bytes must participate in queue limits");
    assert_eq!(rejected_mesh.quad_count(), 6);
    assert_eq!(rejected_biome, biome);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
            max_items: 1,
            max_bytes: expected_bytes,
        }))
        .add_plugins(DebugWorldPlugin::new(1));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert_with_biome(
            key,
            rejected_mesh,
            rejected_biome,
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    assert_eq!(
        app.world().resource::<ChunkRenderQueue>().pending_bytes(),
        expected_bytes
    );

    app.update();
    let instance = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .single(app.world())
        .unwrap();
    assert_eq!(instance.biome_record(), &biome);
}

#[test]
fn render_queue_carries_biome_tint_revision_to_the_instance() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(DebugWorldPlugin::new(1));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert_with_biome_revision(
            key,
            solid_mesh(1),
            uniform_biome_record(7),
            9,
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();

    app.update();
    let instance = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .single(app.world())
        .unwrap();
    assert_eq!(instance.tint_revision(), 9);
}

#[test]
fn packed_chunk_pipeline_family_remains_one_opaque_depth_writing_phase() {
    let plugin = include_str!("../src/plugin.rs");

    assert_eq!(
        plugin
            .matches("let descriptor = RenderPipelineDescriptor {")
            .count(),
        1
    );
    assert_eq!(plugin.matches(".add_render_command::<Opaque3d").count(), 4);
    assert_eq!(plugin.matches("BindGroupLayoutDescriptor::new(").count(), 1);
    assert_eq!(
        plugin.matches("render_device.create_bind_group(").count(),
        1
    );
    assert_eq!(plugin.matches("render_device.create_texture(").count(), 1);
    assert_eq!(plugin.matches("render_device.create_sampler(").count(), 1);
    assert!(plugin.contains("layout: vec![bind_group_layout.clone()]"));
    assert!(plugin.contains("blend: None"));
    assert!(plugin.contains("depth_write_enabled: true"));
    assert_eq!(plugin.matches("binding: ").count(), 28);
    for binding in 0..=13 {
        assert_eq!(
            plugin.matches(&format!("binding: {binding},")).count(),
            2,
            "packed chunk pipeline changed binding {binding}"
        );
    }
    assert_eq!(
        plugin
            .matches("resource: texture_assets.material_buffer.as_entire_binding()")
            .count(),
        1
    );
    assert_eq!(
        plugin
            .matches("BindingResource::TextureView(&texture_assets.views[")
            .count(),
        2
    );
    assert_eq!(
        plugin
            .matches("BindingResource::Sampler(&texture_assets.sampler)")
            .count(),
        1
    );
    assert!(!plugin.contains("AlphaMask3d"));
    assert!(!plugin.contains("Transparent3d"));
    assert_eq!(size_of::<Material>(), 12);
    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(
        plugin
            .matches("pass.set_bind_group(0, bind_group, &[view_offset.offset]);")
            .count(),
        4,
        "cube/model direct and MDI must share the same global bind group"
    );
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

fn texture_ref(page: u32, layer: u32) -> TextureRef {
    TextureRef::new(page, layer).expect("valid synthetic texture reference")
}

fn animation(flags: u32) -> Animation {
    Animation {
        frame_start: 0,
        frame_count: 3,
        ticks_per_frame: 2,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 1,
        flags,
    }
}

#[test]
fn animation_selects_current_next_cross_page_wrap_and_non_blended_frames() {
    let frames = [texture_ref(0, 7), texture_ref(0, 8), texture_ref(1, 3)];
    let material = Material {
        texture: frames[0],
        flags: 0,
        animation: 0,
    };
    let sample = |tick, partial_tick, flags| {
        select_animation_frames(
            material,
            &[animation(flags)],
            &frames,
            ChunkAnimationClock::from_parts(tick, partial_tick),
        )
    };

    assert_eq!(
        sample(0, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.0)
    );
    assert_eq!(
        sample(1, 0.5, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.75)
    );
    assert_eq!(
        sample(2, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[1], frames[2], 0.0)
    );
    assert_eq!(
        sample(5, 0.5, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[2], frames[0], 0.75)
    );
    assert_eq!(
        sample(6, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.0)
    );
    assert_eq!(
        sample(3, 0.9, 0),
        AnimationFrameSample::new(frames[1], frames[1], 0.0)
    );

    let static_material = Material {
        texture: texture_ref(1, 4),
        flags: 0,
        animation: NO_ANIMATION,
    };
    assert_eq!(
        select_animation_frames(
            static_material,
            &[],
            &[],
            ChunkAnimationClock::from_parts(123, 0.75),
        ),
        AnimationFrameSample::new(static_material.texture, static_material.texture, 0.0)
    );

    let one_frame = Animation {
        frame_count: 1,
        ..animation(ANIMATION_FLAG_BLEND)
    };
    assert_eq!(
        select_animation_frames(
            material,
            &[one_frame],
            &frames[..1],
            ChunkAnimationClock::from_parts(1, 0.5),
        ),
        AnimationFrameSample::new(frames[0], frames[0], 0.0)
    );
}

#[test]
fn one_and_two_page_assets_have_one_stable_shared_binding_shape() {
    assert_eq!(
        plan_texture_page_bindings(1),
        Some([
            TexturePageBinding::Asset(0),
            TexturePageBinding::DiagnosticFallback,
        ])
    );
    assert_eq!(
        plan_texture_page_bindings(2),
        Some([TexturePageBinding::Asset(0), TexturePageBinding::Asset(1)])
    );
    assert_eq!(plan_texture_page_bindings(0), None);
    assert_eq!(plan_texture_page_bindings(3), None);
}

#[test]
fn one_page_fallback_is_a_real_one_layer_copy_of_diagnostic_mips() {
    let source = TextureArray {
        layers: 2,
        mips: [4_u32, 2, 1]
            .into_iter()
            .map(|size| {
                let layer_bytes = size as usize * size as usize * 4;
                let mut rgba8 = vec![0x11; layer_bytes];
                rgba8.extend(vec![0x22; layer_bytes]);
                TextureMip {
                    size,
                    rgba8: rgba8.into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };

    let fallback = diagnostic_texture_page(&source).expect("extract diagnostic page");
    assert_eq!(fallback.layers, 1);
    assert_eq!(fallback.mips.len(), source.mips.len());
    for (source_mip, fallback_mip) in source.mips.iter().zip(&fallback.mips) {
        let layer_bytes = source_mip.size as usize * source_mip.size as usize * 4;
        assert_eq!(
            fallback_mip.rgba8.as_ref(),
            &source_mip.rgba8[..layer_bytes]
        );
    }
}

#[test]
fn animation_clock_updates_do_not_rebuild_or_reupload_texture_assets() {
    let identity = ChunkTextureAssetIdentity::for_test(0x1000, 7);
    let mut current = None;
    let mut immutable_uploads = 0;
    let mut clock_bytes = 0;
    for frame in 0..120_u32 {
        if texture_asset_needs_rebuild(current, identity) {
            immutable_uploads += 1;
            current = Some(identity);
        }
        let clock = ChunkAnimationClock::from_elapsed_seconds(f64::from(frame) / 60.0);
        assert!(clock.partial_tick() >= 0.0 && clock.partial_tick() < 1.0);
        clock_bytes += size_of::<ChunkAnimationClock>();
    }
    assert_eq!(immutable_uploads, 1);
    assert_eq!(clock_bytes, 120 * 16);

    let plugin = include_str!("../src/plugin.rs");
    assert_eq!(plugin.matches("render_queue.write_texture(").count(), 1);
    assert_eq!(
        plugin.matches("render_queue.write_buffer(").count(),
        6,
        "one shared writer covers all new immutable geometry streams"
    );
    assert!(plugin.contains("render_queue.write_buffer(&gpu_clock.buffer"));
}

#[test]
fn asset_revision_replacement_is_atomic_and_retains_the_previous_prepared_set_on_failure() {
    let plugin = include_str!("../src/plugin.rs");
    let start = plugin
        .find("fn prepare_chunk_texture_assets(")
        .expect("texture preparation system");
    let end = plugin[start..]
        .find("\nfn storage_table_fits(")
        .map(|offset| start + offset)
        .expect("end of texture preparation system");
    let prepare = &plugin[start..end];

    assert!(
        !prepare.contains("gpu_assets.prepared = None"),
        "a rejected new revision must retain the previous complete GPU asset set"
    );
    let second_page = prepare
        .find("let (texture_1, view_1, padded_1) = upload_texture_page(")
        .expect("second page is prepared before publication");
    let publish = prepare
        .find("gpu_assets.prepared = Some(PreparedChunkTextureAssets {")
        .expect("complete revision publication");
    assert!(second_page < publish);
    assert!(prepare.contains("material.texture.raw()"));
    assert!(prepare.contains(".animations()"));
    assert!(prepare.contains(".animation_frames()"));
    assert!(prepare.contains("_textures: [texture_0, texture_1]"));
    assert!(prepare.contains("views: [view_0, view_1]"));
}
