use super::*;

#[test]
fn queue_counts_every_stream_and_sidecar() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let cube = solid_mesh(1);
    let mesh = meshing::ChunkMesh::from_streams(
        cube.quads().to_vec(),
        vec![meshing::PackedModelRef::new(1, 2, 0, 1)],
        vec![meshing::PackedQuadLighting::new([0; 4])],
        vec![meshing::PackedModelDrawRef::new(0, 0)],
        vec![meshing::PackedLiquidQuad::try_from_words([4, 5, 6, 7]).unwrap()],
        vec![meshing::PackedQuadLighting::new([0; 4])],
        cube.connectivity(),
    )
    .with_transparent_model_draw_refs(vec![meshing::PackedModelDrawRef::new(0, 1)]);
    let expected = 6 * (size_of::<PackedQuad>() + size_of::<meshing::PackedQuadLighting>())
        + size_of::<meshing::PackedModelRef>()
        + size_of::<meshing::PackedQuadLighting>()
        + 2 * size_of::<meshing::PackedModelDrawRef>()
        + size_of::<meshing::PackedLiquidQuad>()
        + size_of::<meshing::PackedQuadLighting>();
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
fn cube_lighting_counts_toward_exact_queue_caps_and_survives_extraction() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let mesh = solid_mesh(1);
    let expected_lighting = mesh.cube_lighting().to_vec();
    let exact_bytes =
        6 * (size_of::<PackedQuad>() + size_of::<meshing::PackedQuadLighting>()) as u64;
    let mut too_small = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 1,
        max_bytes: exact_bytes - 1,
    });
    let mesh = too_small
        .try_insert(key, mesh, ChunkUploadPriority::new(0.0))
        .expect_err("cube lighting must participate in the CPU queue cap");
    assert_eq!(too_small.pending_bytes(), 0);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
            max_items: 1,
            max_bytes: exact_bytes,
        }))
        .add_plugins(ChunkRenderPlugin::new(1));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(key, mesh, ChunkUploadPriority::new(0.0))
        .expect("exact cube geometry plus CPU-light sidecar cap fits");
    assert_eq!(
        app.world().resource::<ChunkRenderQueue>().pending_bytes(),
        exact_bytes
    );
    app.update();

    let instance = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .single(app.world())
        .unwrap();
    assert_eq!(instance.cube_lighting(), expected_lighting);
    assert_eq!(instance.cube_lighting().len(), instance.quads().len());
}

#[test]
fn opaque_and_model_streams_share_one_subchunk_visibility_component() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let cube = solid_mesh(1);
    let mesh = meshing::ChunkMesh::from_streams(
        cube.quads().to_vec(),
        vec![meshing::PackedModelRef::new(1, 0, 0, 1)],
        vec![meshing::PackedQuadLighting::new([0; 4])],
        vec![meshing::PackedModelDrawRef::new(0, 0)],
        vec![],
        vec![],
        cube.connectivity(),
    );
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(1));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(key, mesh, ChunkUploadPriority::new(0.0))
        .unwrap();

    app.update();

    {
        let mut query = app
            .world_mut()
            .query::<(&ChunkRenderInstance, &mut Visibility)>();
        let (instance, mut visibility) = query.single_mut(app.world_mut()).unwrap();
        assert_eq!(instance.key(), key);
        assert!(!instance.quads().is_empty());
        assert!(!instance.model_refs().is_empty());
        *visibility = Visibility::Hidden;
    }

    let (instance, visibility) = app
        .world_mut()
        .query::<(&ChunkRenderInstance, &Visibility)>()
        .single(app.world())
        .unwrap();
    assert_eq!(instance.key(), key);
    assert_eq!(*visibility, Visibility::Hidden);
    assert!(!instance.quads().is_empty());
    assert!(!instance.model_refs().is_empty());
}

#[test]
fn allocated_but_undrawn_target_is_not_exact_presented_evidence() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let acknowledgement = PresentedFrameAck {
        cohort: RenderViewCohort::new(0, [65, 65], 16),
        frame_sequence: 1,
        allocation_manifest: Arc::from([(key, 7)]),
        visible_allocation_manifest: Arc::from([(key, 7)]),
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
        transparent_sort_generation: 0,
        model_witness: None,
    };

    assert!(!acknowledgement.is_exact());
}

#[test]
fn render_queue_enforces_item_and_byte_limits_without_losing_replacements() {
    let first = SubChunkKey::new(0, 0, 0, 0);
    let second = SubChunkKey::new(0, 1, 0, 0);
    let third = SubChunkKey::new(0, 2, 0, 0);
    let mut queue = ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
        max_items: 2,
        max_bytes: 96,
    });

    queue
        .try_insert(first, solid_mesh(1), ChunkUploadPriority::new(0.0))
        .unwrap();
    queue
        .try_insert(
            second,
            meshing::ChunkMesh::default(),
            ChunkUploadPriority::new(1.0),
        )
        .unwrap();
    assert_eq!(queue.retained_len(), 2);
    assert_eq!(queue.pending_bytes(), 96);

    let rejected = queue
        .try_insert(third, solid_mesh(3), ChunkUploadPriority::new(2.0))
        .unwrap_err();
    assert_eq!(rejected.quad_count(), 6);
    assert_eq!(queue.retained_len(), 2);
    assert_eq!(queue.pending_bytes(), 96);

    queue
        .try_update(
            first,
            meshing::ChunkMesh::default(),
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
    assert_eq!(queue.pending_bytes(), 96);

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
            max_bytes: 96,
        }))
        .add_plugins(ChunkRenderPlugin::new(1));

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

fn solid_mesh(runtime_id: u32) -> meshing::ChunkMesh {
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
        .add_plugins(ChunkRenderPlugin::new(1));

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
    let shader = standalone_world_shader(include_str!("../../src/chunk.wgsl"));
    let module = naga::front::wgsl::parse_str(&shader).expect("parse packed chunk WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate packed chunk WGSL");

    assert_eq!(shader.matches("@group(0) @binding(").count(), 14);
    for binding in 0..=11 {
        assert!(
            shader.contains(&format!("@group(0) @binding({binding})")),
            "packed chunk shader is missing global texture binding {binding}"
        );
    }
    assert!(shader.contains("@group(0) @binding(15)"));
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

#[test]
fn world_shaders_share_light_curve_channels_and_fragment_only_daylight() {
    let plugin = CHUNK_RENDERER_SOURCE.replace("\r\n", "\n");
    let lighting = include_str!("../../src/lighting.wgsl");
    assert_eq!(
        lighting
            .matches("const LIGHT_CURVE: array<f32, 16>")
            .count(),
        1
    );
    assert_eq!(lighting.matches("fn lit_colour(").count(), 1);
    assert_eq!(
        lighting
            .matches("const PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR: f32 = 0.2;")
            .count(),
        1,
        "the conservative floor remains explicitly provisional until native visual tuning"
    );
    assert_eq!(
        lighting
            .matches("const PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR: f32 = 0.04;")
            .count(),
        1,
        "light level zero must retain an explicit, independently tunable ambient floor"
    );
    assert!(lighting.contains("let block_contribution = vec3("));
    assert!(lighting.contains(
        "let effective_daylight = max(clamp(daylight, 0.0, 1.0), PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR);"
    ));
    assert!(lighting.contains("let sky_contribution = vec3("));
    assert!(lighting.contains("let channel_light = max(block_contribution, sky_contribution);"));
    assert!(lighting.contains("vec3(PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR)"));
    assert!(lighting.contains("vec3(1.0)"));
    assert!(lighting.contains("channel_light"));
    assert!(lighting.contains("return colour * combined * clamp(ao_factor, 0.0, 1.0)"));
    assert!(lighting.contains("fn light_brightness(level: u32)"));
    assert!(lighting.contains("fn light_ao_factor(level: u32)"));
    for shader in [
        include_str!("../../src/chunk.wgsl"),
        include_str!("../../src/model.wgsl"),
        include_str!("../../src/liquid.wgsl"),
    ] {
        assert!(shader.contains(
            "#import cinnabar::lighting::{light_ao_factor, light_brightness, lit_colour}"
        ));
        assert!(!shader.contains("const LIGHT_CURVE: array<f32, 16>"));
        assert!(!shader.contains("fn lit_colour("));
        assert!(shader.contains("block_light"));
        assert!(shader.contains("sky_light"));
        assert!(shader.contains("ambient_occlusion"));
        let vertex = shader.split("@fragment").next().unwrap();
        assert!(!vertex.contains("atmosphere.sun_direction_daylight.w"));
        let fragment = shader.split("@fragment").nth(1).unwrap();
        assert!(fragment.contains("atmosphere.sun_direction_daylight.w"));

        let standalone = standalone_world_shader(shader);
        let module = naga::front::wgsl::parse_str(&standalone).expect("parse world shader");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("validate world shader");
        assert!(entry_points_use_binding(
            &module,
            naga::ShaderStage::Vertex,
            13
        ));
        assert!(!entry_points_use_binding(
            &module,
            naga::ShaderStage::Vertex,
            15
        ));
        assert!(entry_points_use_binding(
            &module,
            naga::ShaderStage::Fragment,
            15
        ));
        assert!(entry_points_call_function(
            &module,
            naga::ShaderStage::Vertex,
            "light_brightness"
        ));
        assert!(!entry_points_call_function(
            &module,
            naga::ShaderStage::Fragment,
            "light_brightness"
        ));
    }
    assert!(plugin.contains("binding: 13,\n                    visibility: ShaderStages::VERTEX"));
    assert!(
        plugin.contains("binding: 15,\n                    visibility: ShaderStages::FRAGMENT")
    );
    assert!(!plugin.contains("binding: 15,\n                    visibility: ShaderStages::VERTEX"));
}

#[test]
fn chunk_shader_reads_cube_light_from_expanded_origin_without_changing_bindings() {
    let shader = include_str!("../../src/chunk.wgsl");
    assert!(shader.contains("struct ChunkOrigin"));
    assert!(shader.contains("cube_bases: vec4<u32>"));
    assert!(shader.contains("@binding(13) var<storage, read> geometry_streams: array<u32>"));
    assert!(shader.contains("let local_quad_index = instance_index - chunk_origin.cube_bases.x"));
    assert!(shader.contains("chunk_origin.cube_bases.y + local_quad_index"));
    assert_eq!(
        standalone_world_shader(shader)
            .matches("@group(0) @binding(")
            .count(),
        14
    );
    assert_eq!(std::mem::size_of::<PackedQuad>(), 8);
    assert_eq!(std::mem::size_of::<meshing::PackedQuadLighting>(), 8);
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
    let expected_bytes = 6
        * (size_of::<PackedQuad>() + size_of::<meshing::PackedQuadLighting>()) as u64
        + biome.byte_len();
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
        .add_plugins(ChunkRenderPlugin::new(1));
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
    assert_eq!(instance.generation(), 1);
}

#[test]
fn render_queue_carries_biome_tint_revision_to_the_instance() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(1));
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
fn packed_chunk_pipeline_family_shares_one_opaque_depth_writing_phase() {
    let plugin = CHUNK_RENDERER_SOURCE;

    assert_eq!(
        plugin
            .matches("let descriptor = RenderPipelineDescriptor {")
            .count(),
        1
    );
    assert_eq!(plugin.matches(".add_render_command::<Opaque3d").count(), 6);
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
    assert_eq!(plugin.matches("binding: ").count(), 32);
    for binding in 0..=15 {
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
    assert_eq!(
        plugin
            .matches(".add_render_command::<Transparent3d")
            .count(),
        3
    );
    assert_eq!(size_of::<Material>(), 12);
    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(
        plugin
            .matches("pass.set_bind_group(0, bind_group, &[view_offset.offset]);")
            .count(),
        9,
        "cube/opaque-model/transparent-model/transparent-liquid/depth-liquid direct and MDI share the global bind group"
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
