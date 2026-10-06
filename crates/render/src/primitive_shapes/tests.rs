use super::*;
use render_api::primitive_shapes::PrimitiveShapeKind;
use render_model::primitive_shapes::{
    PrimitiveActor, PrimitiveInstance, PrimitiveMeshKey, PrimitiveState,
};

#[test]
fn vanilla_mesh_counts_and_unit_geometry() {
    for (kind, segments, count) in [
        (PrimitiveShapeKind::Line, 0, 2),
        (PrimitiveShapeKind::Box, 0, 24),
        (PrimitiveShapeKind::Circle, 20, 40),
        (PrimitiveShapeKind::Sphere, 20, 120),
        (PrimitiveShapeKind::Arrow, 4, 26),
        (PrimitiveShapeKind::Circle, 0, 2),
        (PrimitiveShapeKind::Sphere, 0, 6),
        (PrimitiveShapeKind::Circle, 255, 510),
    ] {
        let vertices = mesh::build(PrimitiveMeshKey { kind, segments });
        assert_eq!(vertices.len(), count);
        assert!(vertices.iter().flatten().all(|value| value.is_finite()));
        if kind == PrimitiveShapeKind::Box {
            for edge in vertices.chunks_exact(2) {
                assert_eq!(
                    (0..3)
                        .filter(|&axis| edge[0][axis] != edge[1][axis])
                        .count(),
                    1
                );
                assert!(
                    edge.iter()
                        .all(|point| point[..3].iter().all(|value| value.abs() == 0.5))
                );
            }
        }
    }
    let circle = mesh::build(PrimitiveMeshKey {
        kind: PrimitiveShapeKind::Circle,
        segments: 4,
    });
    assert!((circle[0][0] - 1.0).abs() < 0.0001);
    assert!((circle[1][2] + 1.0).abs() < 0.0001);
    assert_eq!(circle[0], circle[circle.len() - 1]);
}

/// Resolves production imports and feature branches before standalone shader validation.
fn source(definitions: &[&str]) -> String {
    let raw = shader(include_str!("shapes.wgsl"), "primitive test");
    let raw = raw.source.as_str().replace(
        "#import bevy_render::globals::Globals",
        "struct Globals { time:f32, delta_time:f32, frame_count:u32, }",
    );
    crate::shader_source::standalone(&raw, definitions)
}

#[test]
fn all_shader_material_variants_validate() {
    for definitions in [
        &[][..],
        &["TEXT_GLYPH"][..],
        &["TEXT_DEPTH"][..],
        &["TEXT_DEPTH", "TEXT_GLYPH", "TEXT_ALPHA_TEST"][..],
        &["SHAPE_GAMMA"][..],
        &["SHAPE_GAMMA", "TEXT_GLYPH"][..],
        &["SHAPE_GAMMA", "TEXT_DEPTH"][..],
        &["SHAPE_GAMMA", "TEXT_DEPTH", "TEXT_GLYPH", "TEXT_ALPHA_TEST"][..],
    ] {
        let source = source(definitions);
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
fn gpu_visibility_handles_expiry_dimension_removal_attachment_and_hour_wrap() {
    use bevy::math::{Mat4, Vec3};
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("primitive visibility") else {
        return;
    };
    let mut instances = vec![PrimitiveState::new(PrimitiveShapeKind::Line).instance(u32::MAX); 12];
    instances[0].lifetime[0] = 3601.0;
    instances[1].lifetime[0] = 3600.0;
    instances[2].meta[1] = 1;
    instances[3].meta[0] = 0;
    instances[4].meta[2] = 0;
    instances[5].meta[2] = 1;
    instances[6].data[3] = 2.0;
    instances[6].transform[3] = [20.0, 0.0, 0.0, 1.0];
    instances[7].data[3] = 0.0;
    instances[8].data[3] = 2.0;
    instances[8].transform[3] = [2.0, 0.0, 0.0, 1.0];
    instances[9].transform[3] = [100.0, 0.0, 0.0, 1.0];
    instances[10] = PrimitiveState::new(PrimitiveShapeKind::Box).instance(u32::MAX);
    instances[10].data[3] = 1.0;
    instances[10].transform = Mat4::from_scale_rotation_translation(
        Vec3::splat(2.0),
        bevy::math::Quat::IDENTITY,
        Vec3::ONE,
    )
    .to_cols_array_2d();
    instances[11].meta[2] = 2;
    instances[11].data[3] = 2.0;
    let view = gpu.buffer(
        &crate::gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let shape = gpu.words(
        bytemuck::cast_slice::<PrimitiveInstance, u32>(&instances),
        wgpu::BufferUsages::STORAGE,
    );
    let frame = gpu.words(
        &[3600.0_f32.to_bits(), 0, 100.0_f32.to_bits(), 0],
        wgpu::BufferUsages::UNIFORM,
    );
    let globals = gpu.words(&[0, 0, 0, 0], wgpu::BufferUsages::UNIFORM);
    let actors = gpu.words(
        bytemuck::cast_slice(&[
            PrimitiveActor {
                position: [0.0; 3],
                valid: 1,
            },
            PrimitiveActor::default(),
            PrimitiveActor {
                position: [2.0, 0.0, 0.0],
                valid: 1,
            },
        ]),
        wgpu::BufferUsages::STORAGE,
    );
    let output = gpu.words(
        &[0; 12],
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let source = format!(
        "{}\n@group(0) @binding(8) var<storage,read_write> results:array<u32>;\n@compute @workgroup_size(1) fn check(@builtin(global_invocation_id) id:vec3<u32>) {{results[id.x]=u32(visible(shapes[id.x],anchor(shapes[id.x])));}}",
        source(&[])
    );
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("check"),
            compilation_options: default(),
            cache: None,
        });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: shape.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: frame.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: actors.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&default());
    {
        let mut pass = encoder.begin_compute_pass(&default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(12, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 48);
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    assert_eq!(
        bytemuck::cast_slice::<u8, u32>(&readback.slice(..).get_mapped_range()),
        [1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0]
    );
}

/// Creates one line patch used to measure real renderer work rather than mirrored estimates.
fn line_update(id: u64, x: f32) -> render_api::primitive_shapes::PrimitiveShapeChange {
    use render_api::primitive_shapes::*;
    PrimitiveShapeChange::Upsert(PrimitiveShapeUpdate {
        network_id: id,
        kind: PrimitiveShapeKind::Line,
        location: Some([x, 0.0, 0.0]),
        rotation: None,
        scale: None,
        color: None,
        total_time_left: None,
        maximum_render_distance: None,
        dimension: None,
        attached_actor: None,
        data: PrimitiveShapeData::Line {
            end: [x + 1.0, 0.0, 0.0],
        },
    })
}

#[test]
fn retained_renderer_has_zero_steady_work_and_changed_slot_bounded_churn() {
    use bevy::{
        ecs::system::RunSystemOnce,
        render::renderer::{RenderDevice, RenderQueue, WgpuWrapper},
    };
    use render_api::primitive_shapes::PrimitiveShapesEvent;
    let Some(native) = crate::gpu_snapshot::Gpu::for_fixture("retained primitive uploads") else {
        return;
    };
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(native.device));
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(native.queue))));
    let scene = PrimitiveShapesScene::default();
    scene.store.lock().unwrap().apply(PrimitiveShapesEvent {
        changes: (0..1000).map(|id| line_update(id, id as f32)).collect(),
        skipped_entries: 0,
    });
    world.insert_resource(scene);
    world.run_system_once(gpu::init).unwrap();
    let mut prepare = IntoSystem::into_system(gpu::prepare);
    prepare.initialize(&mut world);
    prepare.run((), &mut world).unwrap();
    let first = world.resource::<gpu::ShapeGpu>().work;
    assert_eq!(first.bytes, 1000 * gpu::INSTANCE_BYTES);
    assert_eq!(first.mesh_rebuilds, 1);
    let allocated = crate::alloc_count::thread_allocations();
    for _ in 0..10 {
        prepare.run((), &mut world).unwrap();
    }
    assert_eq!(crate::alloc_count::thread_allocations() - allocated, 0);
    assert_eq!(world.resource::<gpu::ShapeGpu>().work, first);
    world
        .resource::<PrimitiveShapesScene>()
        .store
        .lock()
        .unwrap()
        .apply(PrimitiveShapesEvent {
            changes: (0..100)
                .map(|id| line_update(id, id as f32 + 0.5))
                .collect(),
            skipped_entries: 0,
        });
    prepare.run((), &mut world).unwrap();
    let changed = world.resource::<gpu::ShapeGpu>().work;
    assert_eq!(changed.bytes - first.bytes, 100 * gpu::INSTANCE_BYTES);
    assert_eq!(changed.uploads - first.uploads, 1);
    assert_eq!(changed.mesh_rebuilds, first.mesh_rebuilds);
    world.resource_mut::<PrimitiveShapesScene>().store =
        Arc::new(Mutex::new(PrimitiveShapeStore::default()));
    prepare.run((), &mut world).unwrap();
    assert!(world.resource::<gpu::ShapeGpu>().batches.is_empty());
}

#[test]
fn primitive_text_material_modes_keep_depth_background_and_facing() {
    use crate::gpu_snapshot::{Draw, Gpu, RasterState, SNAPSHOT_SIDE};
    use render_model::primitive_shapes::PrimitiveTextRecord;
    let Some(gpu) = Gpu::for_fixture("primitive text facing") else {
        return;
    };
    let mut shape = PrimitiveState::new(PrimitiveShapeKind::Text).instance(u32::MAX);
    shape.color = [1.0, 0.0, 0.0, 1.0];
    shape.transform[3][2] = 0.5;
    let view = gpu.buffer(
        &crate::gpu_snapshot::view(Mat4::IDENTITY, Vec3::new(0.0, 0.0, 5.0)),
        wgpu::BufferUsages::UNIFORM,
    );
    let shapes = gpu.words(bytemuck::cast_slice(&[shape]), wgpu::BufferUsages::STORAGE);
    let frame = gpu.words(&[0, 0, 100.0_f32.to_bits(), 0], wgpu::BufferUsages::UNIFORM);
    let globals = gpu.words(&[0; 4], wgpu::BufferUsages::UNIFORM);
    let actors = gpu.words(&[0; 4], wgpu::BufferUsages::STORAGE);
    let atlas = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        atlas.as_image_copy(),
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        atlas.size(),
    );
    let atlas_view = atlas.create_view(&default());
    let sampler = gpu.device.create_sampler(&default());
    for (mode, flags, glyph, shown) in [
        (1, 0, false, true),
        (1, 1, false, false),
        (1, 0, true, false),
        (2, 0, true, true),
        (2, 1, true, false),
        (2, 0, false, false),
        (3, 1, false, true),
        (3, 0, false, false),
        (3, 1, true, false),
        (4, 1, true, true),
        (4, 0, true, false),
        (4, 1, false, false),
        (2, 8, true, false),
        (2, 8 | 2, true, false),
        (2, 8 | 4, true, true),
        (1, 8, false, false),
        (1, 8 | 4, false, false),
        (1, 8 | 2, false, true),
    ] {
        let material = pipeline::test_material(mode);
        let fragment = material.fragment.as_ref().unwrap();
        // `#ifdef` checks registered names, including definitions whose boolean value is false.
        let definitions: Vec<&str> = material
            .vertex
            .shader_defs
            .iter()
            .chain(&fragment.shader_defs)
            .map(|value| match value {
                bevy::shader::ShaderDefVal::Bool(name, _)
                | bevy::shader::ShaderDefVal::Int(name, _)
                | bevy::shader::ShaderDefVal::UInt(name, _) => name.as_str(),
            })
            .collect();
        let depth = material.depth_stencil.as_ref().unwrap();
        let record = PrimitiveTextRecord {
            rect: [-20.0, -20.0, 20.0, 20.0],
            uv: if glyph {
                [0.0, 0.0, 1.0, 1.0]
            } else {
                [0.0, 0.0, -1.0, -1.0]
            },
            color: if glyph {
                [1.0; 4]
            } else {
                [0.0, 0.0, 1.0, 1.0]
            },
            meta: [0, 1, flags, 0],
        };
        let text = gpu.words(bytemuck::cast_slice(&[record]), wgpu::BufferUsages::STORAGE);
        let bindings = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: shapes.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: frame.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: actors.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: text.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&atlas_view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ];
        let pixels = gpu.render_with_state(
            &source(&definitions),
            "text_vertex",
            &[Draw {
                fragment: "shape_fragment",
                vertices: 0..6,
                bindings: &bindings,
                blend: fragment.targets[0].as_ref().unwrap().blend,
                write_depth: depth.depth_write_enabled,
            }],
            RasterState {
                primitive: material.primitive,
                depth_compare: depth.depth_compare,
                ..default()
            },
        );
        let center = ((SNAPSHOT_SIDE / 2) * SNAPSHOT_SIDE + SNAPSHOT_SIDE / 2) as usize * 4;
        let color = if glyph {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        assert_eq!(
            pixels[center..center + 4] == color,
            shown,
            "mode {mode}, flags {flags}, glyph {glyph}, pixel {:?}",
            &pixels[center..center + 4]
        );
    }
}
