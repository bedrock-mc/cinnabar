//! `centerDepthSmooth` readers and the GPU sampler over a depth buffer an entity drew into.

use super::*;
use crate::gpu_snapshot::Gpu;
use shaderpack::{StageMask, UniformMember};

fn block(members: &[(&str, u32, ValueType, Option<(u32, u32)>)]) -> UniformBlock {
    UniformBlock {
        name: "uniforms".into(),
        group: 0,
        binding: 0,
        size: 64,
        members: members
            .iter()
            .map(|&(name, offset, ty, array)| UniformMember {
                name: name.into(),
                offset,
                ty,
                array,
            })
            .collect(),
        stages: StageMask::default(),
    }
}

#[test]
fn readers_split_at_the_sample_and_skip_geometry_and_non_float_declarations() {
    let float = ValueType::Scalar(ScalarType::Float);
    let reads = block(&[("viewWidth", 0, float, None), (UNIFORM, 12, float, None)]);
    let as_int = block(&[(UNIFORM, 4, ValueType::Scalar(ScalarType::Int), None)]);
    let as_array = block(&[(UNIFORM, 0, float, Some((2, 16)))]);
    let unrelated = block(&[("viewWidth", 0, float, None)]);
    let readers = CenterDepthReaders::plan([
        (ProgramGroup::ShadowComposite, Some(&reads)),
        (ProgramGroup::Prepare, Some(&reads)),
        // A geometry program has no full-screen block; Iris gives gbuffers no value.
        (ProgramGroup::Gbuffers, None),
        (ProgramGroup::Deferred, Some(&reads)),
        (ProgramGroup::Composite, Some(&as_int)),
        (ProgramGroup::Composite, Some(&as_array)),
        (ProgramGroup::Composite, Some(&unrelated)),
        (ProgramGroup::Composite, Some(&reads)),
        (ProgramGroup::Final, Some(&reads)),
    ]);
    let at = |program| Reader {
        program,
        offset: 12,
    };
    assert_eq!(readers.before, [at(0), at(1)]);
    assert_eq!(readers.after, [at(3), at(7), at(8)]);
    assert!(!readers.is_empty());
    assert!(CenterDepthReaders::plan([(ProgramGroup::Final, Some(&unrelated))]).is_empty());
}

/// Side of the fixture depth buffer; its centre texel is (2, 2) as Iris's nearest
/// `vec2(0.5)` reads it.
const SIDE: u32 = 4;

/// Offset of `centerDepthSmooth` in the fixture's uniform block.
const OFFSET: u64 = 8;

#[test]
fn gpu_entity_depth_at_the_centre_drives_the_smoothed_value() {
    let Some(gpu) = Gpu::for_fixture("shader-pack centerDepthSmooth sampler") else {
        return;
    };
    let device = &gpu.device;
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("centre depth fixture"),
        size: wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = depth.create_view(&Default::default());
    // Stands in for an entity's fragments: `entity` at the centre texel, terrain at 0.5
    // everywhere else, so only the centre can move the value.
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("centre entity fixture"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
@group(0) @binding(0) var<uniform> centre: vec4<f32>;
@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @builtin(frag_depth) f32 {
    return select(0.5, centre.x, all(vec2<u32>(p.xy) == vec2(2u)));
}
"#
            .into(),
        ),
    });
    let fill = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("centre entity fixture"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        multiview_mask: None,
        cache: None,
    });
    let centre = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("centre entity depth"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let fill_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("centre entity fixture"),
        layout: &fill.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: centre.as_entire_binding(),
        }],
    });
    let host = super::super::host_pipelines::host_pipelines(
        device,
        &[None; super::super::plan::MAX_COLOR_BUFFERS as usize],
        wgpu::TextureFormat::R32Float,
        true,
        [false; 3],
        &[],
        &[],
        Some(DepthSource::Geometry),
    );
    let sampler = CenterDepth::new(device, host.center_depth.as_ref().unwrap(), Some(&view));
    // A full-screen program's uniform block, with the CPU's placeholder already packed.
    let uniforms = [Some(device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("composite uniforms"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    }))];
    let readers = [Reader {
        program: 0,
        offset: OFFSET,
    }];
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("centerDepthSmooth readback"),
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let uniform = uniforms[0].as_ref().unwrap();
    let frame = |entity: f32, share: f32| -> f32 {
        gpu.queue
            .write_buffer(&centre, 0, bytemuck::bytes_of(&[entity, 0.0, 0.0, 0.0]));
        gpu.queue.write_buffer(uniform, 0, &[0; 16]);
        sampler.write_share(&gpu.queue, share);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("centre entity fixture"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&fill);
            pass.set_bind_group(0, &fill_group, &[]);
            pass.draw(0..3, 0..1);
        }
        sampler.sample_geometry(&mut encoder, &readers, &uniforms);
        encoder.copy_buffer_to_buffer(uniform, 0, &readback, 0, 16);
        gpu.queue.submit([encoder.finish()]);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let value = {
            let mapped = readback.slice(..).get_mapped_range().unwrap();
            let words: &[f32] = bytemuck::cast_slice(&mapped);
            assert_eq!(words[0], 0.0, "other members keep their packed values");
            words[OFFSET as usize / 4]
        };
        readback.unmap();
        value
    };
    // The first frame takes the entity's depth outright.
    let first = frame(0.25, 1.0);
    assert!((first - 0.25).abs() < 1e-6, "{first}");
    // The entity leaves the centre (the sky reads 1): half the gap closes per half-life.
    let second = frame(1.0, 0.5);
    assert!((second - 0.625).abs() < 1e-6, "{second}");
    // An entity walking in front of the centre pulls focus back towards it.
    let third = frame(0.125, 0.5);
    assert!((third - 0.375).abs() < 1e-6, "{third}");
}
