use super::*;
use render_api::primitive_shapes::PrimitiveShapeKind;
use render_model::primitive_shapes::PrimitiveState;

/// Specializes the production descriptor exactly as a gamma-corrected text pass.
fn material(mode: u8) -> RenderPipelineDescriptor {
    let mut result = descriptor();
    ShapeSpecializer
        .specialize(
            Key {
                msaa: Msaa::Off,
                hdr: false,
                gamma: true,
                mode,
            },
            &mut result,
        )
        .unwrap();
    result
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
        let material = material(mode);
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
            &super::super::tests::source(&definitions),
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
