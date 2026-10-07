//! Production diffuse transport on resident walls, roofs and cached input.
use super::super::frame::EnhancedFrameGpu;
use super::*;
use wgpu::util::DeviceExt;

const QUERY: &str = r#"
#import cinnabar::enhanced_indirect::spatial_indirect
#import cinnabar::enhanced_indirect_trace::grid_segment_visibility
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,4>;
@compute @workgroup_size(1) fn sample_field(){
    results[0]=spatial_indirect(vec3(6.0,5.6,6.0),vec3(0.0,1.0,0.0),1.0,1.0);
    results[1]=spatial_indirect(vec3(11.0,10.05,6.0),vec3(0.0,1.0,0.0),1.0,1.0);
    results[2]=vec4(grid_segment_visibility(vec3(3.5,6.5,6.5),vec3(6.5,6.5,6.5)),
        spatial_indirect(vec3(6.0,6.0,6.0),vec3(1.0,0.0,0.0),1.0,1.0).rgb);
    results[3]=spatial_indirect(vec3(6.0,5.6,6.0),vec3(0.0,1.0,0.0),0.0,1.0);
}
"#;

#[test]
fn update_order_is_complete_and_admits_camera_neighbors_first() {
    let order = probe_update_order();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert!(
        sorted.iter().copied().eq(0..PROBE_COUNT),
        "each cached probe is updated exactly once per cycle"
    );
    for &index in &order[..8] {
        let cell = [
            index % PROBE_SIZE[0],
            (index / PROBE_SIZE[0]) % PROBE_SIZE[1],
            index / (PROBE_SIZE[0] * PROBE_SIZE[1]),
        ];
        for axis in 0..3 {
            assert!(
                (PROBE_SIZE[axis] / 2 - 1..=PROBE_SIZE[axis] / 2).contains(&cell[axis]),
                "visible center probes precede far corners"
            );
        }
    }
}

#[test]
fn scroll_updates_exposed_probes_first_and_retains_overlap() {
    for shift in [IVec3::X, -IVec3::X, IVec3::Y, -IVec3::Z] {
        let mut order = probe_update_order();
        let origin = shift * PROBE_SPACING as i32;
        order_scrolled_probes(&mut order, Some(IVec3::ZERO), origin);
        let exposed = order
            .iter()
            .filter(|&&index| {
                let old_cell = probe_cell(index) + shift;
                !old_cell.cmpge(IVec3::ZERO).all()
                    || !old_cell
                        .cmplt(IVec3::from_array(PROBE_SIZE.map(|v| v as i32)))
                        .all()
            })
            .count();
        assert!(exposed > 0 && exposed < PROBE_COUNT as usize);
        for &index in &order[..exposed] {
            let old_cell = probe_cell(index) + shift;
            assert!(
                !old_cell.cmpge(IVec3::ZERO).all()
                    || !old_cell
                        .cmplt(IVec3::from_array(PROBE_SIZE.map(|v| v as i32)))
                        .all()
            );
        }
        assert!(grids_overlap(IVec3::ZERO, origin));
        assert!(!grids_overlap(
            IVec3::ZERO,
            IVec3::from_array(GRID_SIZE.map(|v| v as i32))
        ));
        let mut sorted = order;
        sorted.sort_unstable();
        assert!(sorted.into_iter().eq(0..PROBE_COUNT));
    }
}

#[test]
fn native_probe_coordinates_and_radiance_updates_are_world_stable() {
    let source = r#"
#import cinnabar::enhanced_indirect_trace::{probe_storage_index,probe_radiance_mix,GI_HISTORY_SECONDS}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,3>;
@compute @workgroup_size(1) fn regression(){
    let size=vec3(4u);
    let previous_origin=vec3(-4,0,-4);
    let moved_origin=vec3(-3,0,-4);
    let cell=vec3(2,1,3);
    results[0]=vec4(f32(probe_storage_index(previous_origin+cell,size)),
        f32(probe_storage_index(moved_origin+cell-vec3(1,0,0),size)),
        f32(probe_storage_index(vec3(-1,0,-1),size)),0.0);
    results[1]=vec4(probe_radiance_mix(7.0,7.0,1.0),
        probe_radiance_mix(7.0+GI_HISTORY_SECONDS*0.5,7.0,1.0),
        probe_radiance_mix(7.0+GI_HISTORY_SECONDS,7.0,1.0),
        probe_radiance_mix(7.0,7.0,0.0));
    results[2]=vec4(f32(probe_storage_index(vec3(0),size)),
        f32(probe_storage_index(vec3(4,0,0),size)),probe_radiance_mix(0.0,100.0,1.0),0.0);
}
"#;
    let Some(values) = super::super::post_regressions::execute(source, 3) else {
        return;
    };
    assert_eq!(
        values[0][0], values[0][1],
        "walking reuses the same world probe slot"
    );
    assert_eq!(
        values[0][2], 51.0,
        "negative world coordinates wrap consistently"
    );
    assert_eq!(
        values[2][0], values[2][1],
        "newly exposed probes alias only departed cells"
    );
    assert_eq!(
        values[2][2], 1.0,
        "clock wrapping cannot revive completed history"
    );
    for (actual, expected) in values[1].iter().zip([0.0, 0.5, 1.0, 1.0]) {
        assert!(
            (actual - expected).abs() < 0.00001,
            "probe history interpolates by elapsed seconds"
        );
    }
}

fn cells() -> Vec<[u32; 4]> {
    let count = 16 * 16 * 16;
    let probes = 4 * 4 * 4;
    let mut data = vec![[0; 4]; HEADER_WORDS + count + probes * PROBE_WORDS];
    data[0] = [0, 0, 0, 1f32.to_bits()];
    data[1] = [16, 16, 16, (HEADER_WORDS + count) as u32];
    data[2] = [4, 4, 4, 4f32.to_bits()];
    data[3] = [1, 1, PROBE_WORDS as u32, HEADER_WORDS as u32];
    for cell in &mut data[HEADER_WORDS..HEADER_WORDS + count] {
        cell[0] = 1;
    }
    data
}
fn solid(data: &mut [[u32; 4]], p: [usize; 3], colour: [u8; 3]) {
    data[HEADER_WORDS + (p[2] * 16 + p[1]) * 16 + p[0]] = [
        3,
        u32::from(colour[0]) | u32::from(colour[1]) << 8 | u32::from(colour[2]) << 16,
        1f32.to_bits(),
        1f32.to_bits(),
    ];
}

#[test]
fn native_spatial_light_preserves_shelter_colour_and_wall_visibility() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native Vulkan adapter for resident-grid indirect light");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("spatial light fixture device");
    let update_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production spatial light update"),
        source: wgpu::ShaderSource::Wgsl(
            crate::shader_source::composed(
                include_str!("indirect_compute.wgsl"),
                &["INDIRECT_COMPUTE"],
            )
            .into(),
        ),
    });
    let query_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production spatial interpolation"),
        source: wgpu::ShaderSource::Wgsl(crate::shader_source::composed(QUERY, &[]).into()),
    });
    let pipeline = |shader: &wgpu::ShaderModule, entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: None,
            module: shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let update = pipeline(&update_shader, "update_spatial_irradiance");
    let query = pipeline(&query_shader, "sample_field");
    let environment = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("spatial transport cached sky"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 15,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let environment_view = environment.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let cloud = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("disabled cloud fixture"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let cloud_view = cloud.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let storage = |label, data: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: data,
            usage,
        })
    };
    let local = storage(
        "empty spatial local lamps",
        &vec![0; super::super::local_lights::LOCAL_LIGHT_BUFFER_BYTES],
        wgpu::BufferUsages::STORAGE,
    );
    let range = storage(
        "whole fixture probe range",
        bytemuck::cast_slice(&[0u32, 64, 1, 0]),
        wgpu::BufferUsages::UNIFORM,
    );
    let order = storage(
        "fixture irradiance update order",
        bytemuck::cast_slice(&(0u32..64).collect::<Vec<_>>()),
        wgpu::BufferUsages::STORAGE,
    );
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spatial receiver results"),
        size: 64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spatial receiver readback"),
        size: 64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let result_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("spatial results"),
        layout: &query.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 31,
            resource: output.as_entire_binding(),
        }],
    });
    let empty_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("empty spatial group"),
        layout: &query.get_bind_group_layout(1),
        entries: &[],
    });
    let mut measured = Vec::new();
    for mode in 0..7 {
        let mut data = cells();
        if mode == 0 {
            for z in 2..10 {
                for y in 2..10 {
                    for x in 2..10 {
                        if x == 2 || x == 9 || y == 2 || y == 9 || z == 2 || z == 9 {
                            solid(&mut data, [x, y, z], [128; 3]);
                        }
                    }
                }
            }
        } else if mode <= 2 || mode == 4 {
            for z in 0..16 {
                for y in 0..16 {
                    solid(&mut data, [8, y, z], [255, 8, 2]);
                    if mode == 2 {
                        solid(&mut data, [4, y, z], [0; 3]);
                    }
                }
            }
            if mode == 4 {
                for z in 0..16 {
                    for y in 0..16 {
                        for x in 0..2 {
                            data[HEADER_WORDS + (z * 16 + y) * 16 + x] = [0; 4];
                        }
                    }
                }
            }
        } else if mode == 3 {
            for z in 8..16 {
                for y in 0..16 {
                    for x in 0..16 {
                        data[HEADER_WORDS + (z * 16 + y) * 16 + x] = [0; 4];
                    }
                }
            }
        }
        if mode >= 5 {
            for index in 0..64 {
                let cell = IVec3::new(index % 4, (index / 4) % 4, index / 16);
                let point = cell.as_vec3() * 4.0
                    + Vec3::splat(2.5)
                    + if mode == 6 {
                        Vec3::new(16.0, 0.0, 0.0)
                    } else {
                        Vec3::ZERO
                    };
                let base = HEADER_WORDS + 16 * 16 * 16 + index as usize * PROBE_WORDS;
                data[base] = [point.x.to_bits(), point.y.to_bits(), point.z.to_bits(), 1];
                for face in 0..6 {
                    data[base + 1 + face] = [
                        0.5f32.to_bits(),
                        0.5f32.to_bits(),
                        0.5f32.to_bits(),
                        80f32.to_bits(),
                    ];
                    data[base + 7 + face] = [6400f32.to_bits(), 0, 0, 0];
                }
            }
        }
        let field = storage(
            "resident surfaces and writable probes",
            bytemuck::cast_slice(&data),
            wgpu::BufferUsages::STORAGE,
        );
        let sky = if mode == 0 || mode == 3 {
            [255u8; 4]
        } else {
            [0, 0, 0, 255]
        };
        queue.write_texture(
            environment.as_image_copy(),
            bytemuck::cast_slice(&[sky; 15]),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 15,
            },
        );
        let mut frame: EnhancedFrameGpu = bytemuck::Zeroable::zeroed();
        frame.light_direction = (-Vec3::X).extend(1.0);
        frame.light_colour = Vec4::splat(if mode == 0 || mode == 3 { 0.0 } else { 3.2 });
        let frame_buffer = storage(
            "spatial physical source frame",
            bytemuck::bytes_of(&frame),
            wgpu::BufferUsages::UNIFORM,
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("production transport inputs"),
            layout: &update.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&environment_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: range.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: local.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&cloud_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: order.as_entire_binding(),
                },
            ],
        });
        let query_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("production spatial field sampling"),
            layout: &query.get_bind_group_layout(2),
            entries: &[wgpu::BindGroupEntry {
                binding: 12,
                resource: field.as_entire_binding(),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        if mode < 5 {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&update);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(64, 1, 1);
        }
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&query);
            pass.set_bind_group(0, &result_group, &[]);
            pass.set_bind_group(1, &empty_group, &[]);
            pass.set_bind_group(2, &query_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 64);
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |value| send.send(value).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receive.recv().unwrap().unwrap();
        let mapped = readback.slice(..).get_mapped_range();
        measured.push(bytemuck::cast_slice::<u8, [f32; 4]>(&mapped).to_vec());
        drop(mapped);
        readback.unmap();
    }
    assert!(
        measured[0][0][3] > 0.9,
        "sheltered probes remain valid: {:?}",
        measured[0]
    );
    assert!(
        measured[0][0][..3].iter().all(|v| *v < 0.02),
        "closed room receives no unoccluded sky: {:?}",
        measured[0]
    );
    assert!(
        measured[0][1][0] > 0.5,
        "exposed receiver retains sky irradiance: {:?}",
        measured[0]
    );
    let lit = measured[1][2];
    let shaded = measured[2][2];
    assert!(
        lit[1] > 0.05 && lit[1] > lit[2] * 10.0 && lit[1] > lit[3] * 10.0,
        "authored red wall produces coloured bounce: {lit:?}"
    );
    assert!(
        shaded[1] < lit[1] * 0.1,
        "occluded wall cannot bounce direct sun: {shaded:?} versus {lit:?}"
    );
    assert_eq!(
        shaded[0], 0.0,
        "interpolation never crosses the opaque wall"
    );
    assert!(
        measured[3][0][0] > 0.8,
        "missing upper/side residency preserves open sky fallback: {:?}",
        measured[3]
    );
    assert!(
        measured[3][3][..3].iter().all(|v| v.abs() < 0.000001),
        "unknown directions contribute no coloured bounce in a closed-sky receiver: {:?}",
        measured[3]
    );
    assert!(
        measured[4][2][1] > lit[1] * 0.5,
        "known sunlit walls retain bounded bounce across missing far residency: {:?}",
        measured[4]
    );
    assert!(
        (measured[5][0][0] - 0.5).abs() < 0.00001,
        "overlapping initialized world probes remain usable: {:?}",
        measured[5]
    );
    assert_eq!(
        measured[6][0], [0.0; 4],
        "aliased departed probes are rejected before rewriting their slot"
    );

    let render_device = RenderDevice::from(device);
    let render_queue = RenderQueue(std::sync::Arc::new(
        bevy::render::renderer::WgpuWrapper::new(queue),
    ));
    let mut view = IndirectGridGpu::new(&render_device);
    let geometry = IndirectGeometry::default();
    let coverage = crate::chunk::ChunkResidentCoverage::default();
    let textures = ChunkTextureAssets::default();
    let tints = crate::ChunkBiomeTints::default();
    view.prepare(
        &render_queue,
        &geometry,
        &coverage,
        &textures,
        &tints,
        Vec3::ZERO,
        Some(0),
        1,
    );
    for _ in 0..PROBE_COUNT.div_ceil(UPDATE_PROBES) {
        view.submitted.store(true, Ordering::Relaxed);
        view.prepare(
            &render_queue,
            &geometry,
            &coverage,
            &textures,
            &tints,
            Vec3::ZERO,
            Some(0),
            1,
        );
    }
    let counts = (view.rebuilds, view.uploads);
    let pointers = (view.words.as_ptr(), view.colors.as_ptr());
    assert!(!view.pending());
    for _ in 0..3 {
        assert!(!view.prepare(
            &render_queue,
            &geometry,
            &coverage,
            &textures,
            &tints,
            Vec3::splat(0.05),
            Some(0),
            1
        ));
    }
    assert_eq!(
        (view.rebuilds, view.uploads),
        counts,
        "unchanged spatial inputs upload no data"
    );
    assert_eq!(
        (view.words.as_ptr(), view.colors.as_ptr()),
        pointers,
        "warm grid retains all storage"
    );
    assert!(view.prepare(
        &render_queue,
        &geometry,
        &coverage,
        &textures,
        &tints,
        Vec3::ZERO,
        Some(0),
        2
    ));
    assert_eq!(
        view.rebuilds, counts.0,
        "lighting refresh never rebuilds occupancy"
    );
    let epoch = view.epoch;
    let buffers = (view.words.as_ptr(), view.colors.as_ptr());
    assert!(view.prepare(
        &render_queue,
        &geometry,
        &coverage,
        &textures,
        &tints,
        Vec3::new(4.1, 0.0, 0.0),
        Some(0),
        2
    ));
    assert_eq!(
        view.epoch, epoch,
        "walking preserves overlapping GPU probe history"
    );
    assert_eq!(buffers, (view.words.as_ptr(), view.colors.as_ptr()));
    view.prepare(
        &render_queue,
        &geometry,
        &coverage,
        &textures,
        &tints,
        Vec3::new(400.0, 0.0, 0.0),
        Some(0),
        2,
    );
    assert_ne!(
        view.epoch, epoch,
        "teleports invalidate unrelated probe history"
    );
}
