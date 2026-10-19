use super::*;

#[test]
fn warm_actor_history_uploads_do_not_stage_unchanged_bytes() {
    let (app, _) = crate::queue_review_support::app();
    let device = app.world().resource::<RenderDevice>();
    let queue = app.world().resource::<RenderQueue>();
    let mut storage = MotionBuffer::new(device, 64).unwrap();
    let bytes = [3u8; 64];
    storage.upload(device, &bytes, |buffer, bytes| {
        queue.write_buffer(buffer, 0, bytes)
    });
    let before = test_alloc::thread_allocations();
    storage.upload(device, &bytes, |buffer, bytes| {
        queue.write_buffer(buffer, 0, bytes)
    });
    assert_eq!(
        test_alloc::thread_allocations(),
        before,
        "unchanged history must not allocate staging work"
    );
}

#[test]
fn empty_actor_views_prewarm_motion_for_later_visible_materials() {
    use bevy::prelude::Mut;
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        pipeline
            .prewarm(cache, Msaa::Sample4, false, false)
            .unwrap();
        let material =
            assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
                alpha_test: true,
                cull: true,
                depth_write: true,
                ..Default::default()
            }));
        let key = ActorPipelineKey {
            msaa: Msaa::Sample4,
            hdr: false,
            enhanced: false,
            material,
        };
        assert!(
            pipeline.motion_variant(key).is_some(),
            "motion pipeline must exist before the first actor becomes visible"
        );
    });
}

/// Composes the production velocity shader with its shared Rust storage strides.
pub(super) fn shader_source(defines: &[&str]) -> String {
    let source = crate::material_shader::source(include_str!("../../actor.wesl"))
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &render_model::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    crate::shader_source::standalone_prepared(&source, defines)
}

#[test]
fn actor_velocity_shaders_validate_with_and_without_multisample_cutout_coverage() {
    for defines in [
        &["ACTOR_MOTION"][..],
        &["ACTOR_MOTION", "ALPHA_TO_COVERAGE"][..],
    ] {
        let wgsl = shader_source(defines);
        let module = naga::front::wgsl::parse_str(&wgsl)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&wgsl)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

/// Selects enabled boolean definitions from a production shader stage.
fn shader_definition_names(values: &[bevy::shader::ShaderDefVal]) -> Vec<&str> {
    values
        .iter()
        .filter_map(|value| {
            if let bevy::shader::ShaderDefVal::Bool(name, true) = value {
                Some(name.as_ref())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn production_motion_fragment_fits_fifteen_variable_device_interface_limits() {
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_inter_stage_shader_variables: 15,
            ..Default::default()
        },
        ..Default::default()
    });
    let layouts: Vec<_> = [
        actor_bind_group_layout(),
        crate::lighting::layout(),
        layout(),
    ]
    .into_iter()
    .map(|descriptor| {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("production actor motion binding contract"),
            entries: &descriptor.entries,
        })
    })
    .collect();
    let slots: Vec<_> = layouts.iter().map(Some).collect();
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("production actor motion pipeline layout"),
        bind_group_layouts: &slots,
        ..Default::default()
    });
    for msaa in [Msaa::Off, Msaa::Sample4] {
        let material =
            assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
                alpha_test: true,
                ..Default::default()
            }));
        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
        MotionSpecializer
            .specialize(
                ActorPipelineKey {
                    msaa,
                    hdr: false,
                    enhanced: false,
                    material,
                },
                &mut descriptor,
            )
            .unwrap();
        let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("production actor motion vertex specialization"),
            source: wgpu::ShaderSource::Wgsl(
                shader_source(&shader_definition_names(&descriptor.vertex.shader_defs)).into(),
            ),
        });
        let fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("production actor motion fragment specialization"),
            source: wgpu::ShaderSource::Wgsl(
                shader_source(&shader_definition_names(
                    &descriptor.fragment.as_ref().unwrap().shader_defs,
                ))
                .into(),
            ),
        });
        let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fifteen-variable actor motion limits"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("actor_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("actor_motion_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: MOTION_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: msaa.samples(),
                alpha_to_coverage_enabled: msaa.samples() > 1,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
    }
}

#[test]
fn stationary_actor_publications_keep_both_motion_buffers_without_uploads() {
    use bevy::math::{Mat4, UVec4};
    let (device, _) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let camera = CameraHistory::new(
        Mat4::IDENTITY,
        Mat4::IDENTITY,
        UVec4::new(0, 0, 640, 320),
        0,
    );
    let mut history = ActorHistory::default();
    let mut instances = MotionBuffer::new(&device, size_of::<MotionInstance>()).unwrap();
    let mut bones = MotionBuffer::new(&device, render_model::ACTOR_BONE_MATRIX_BYTES).unwrap();
    let mut frame = crate::motion_blur::actor_history::tests::frame(Vec3::ZERO, [0.0, 2.0, 0.25]);
    for generation in 0..4 {
        frame.frame_generation = generation;
        assert!(!history.advance(&frame, camera, camera, generation != 0));
        let mut uploaded = 0;
        instances.upload(
            &device,
            bytemuck::cast_slice(&history.instances),
            |_, bytes| uploaded += bytes.len(),
        );
        bones.upload(&device, bytemuck::cast_slice(&history.bones), |_, bytes| {
            uploaded += bytes.len()
        });
        if generation < 2 {
            assert!(
                uploaded > 0,
                "first history frames populate the GPU buffers"
            );
        } else {
            assert_eq!(
                uploaded, 0,
                "unchanged actor publications must not stage GPU writes"
            );
        }
    }
}

#[test]
fn actor_velocity_preserves_cutout_culling_and_reads_depth_without_changing_it() {
    let material =
        assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            cull: true,
            depth_write: true,
            ..Default::default()
        }));
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    MotionSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Sample4,
                hdr: false,
                enhanced: false,
                material,
            },
            &mut descriptor,
        )
        .unwrap();
    assert_eq!(
        descriptor.primitive.cull_mode,
        Some(bevy::render::render_resource::Face::Back)
    );
    assert_eq!(descriptor.multisample.count, 4);
    assert!(descriptor.multisample.alpha_to_coverage_enabled);
    let depth = descriptor.depth_stencil.unwrap();
    assert_eq!(depth.depth_compare, Some(CompareFunction::Equal));
    assert_eq!(depth.depth_write_enabled, Some(false));
    assert!(eligible(material));
    assert!(!eligible(assets::EntityRenderMaterial::Default.word(Some(
        assets::EntityRenderMaterialState {
            blend: true,
            depth_write: false,
            ..Default::default()
        }
    ))));
}
