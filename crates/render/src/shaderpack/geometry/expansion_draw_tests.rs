//! A translated fixture program drawing the expanded fixture section, from wide and from
//! packed vertex streams.

use super::expansion_tests::{
    ANCHOR, ATLAS_SIZE, Expanded, GRASS_BLOCK_ID, GRASS_RECT, SECTION_ORIGIN, SPRITE_RGBA,
    expand_fixture, expand_fixture_in,
};
use super::vertex::{CONSTANT_STREAM_WORDS, TerrainLayout, draw_origin};
use super::{DRAW_ORIGIN_VERTEX_BUFFER, LIGHTMAP_TEXTURE_MATRIX, vertex_buffer_layouts};
use crate::gpu_snapshot::Gpu;
use bevy::math::{Mat4, Vec3};
use wgpu::util::DeviceExt;

// --- Translated fixture program ----------------------------------------------------------------

const TARGET_SIDE: u32 = 64;

fn translate_fixture() -> shaderpack::ProgramTranslation {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/shaderpacks/terrain_probe");
    let pack = shaderpack::ShaderPack::open(&path).expect("fixture pack opens");
    let translation = pack.translate(&shaderpack::TranslateConfig::default());
    let program = translation
        .programs
        .into_iter()
        .find(|program| program.program.name == "gbuffers_terrain")
        .expect("fixture defines gbuffers_terrain");
    assert!(
        program.is_ok(),
        "{:?}",
        program.all_diagnostics().collect::<Vec<_>>()
    );
    program
}

fn uniform_words(block: &shaderpack::UniformBlock, model_view: Mat4, projection: Mat4) -> Vec<u8> {
    let mut bytes = vec![0_u8; block.size as usize];
    let mut put = |offset: u32, matrix: Mat4| {
        let start = offset as usize;
        bytes[start..start + 64].copy_from_slice(bytemuck::cast_slice(&matrix.to_cols_array()));
    };
    for member in &block.members {
        match member.name.as_str() {
            "iris_ModelViewMat" => put(member.offset, model_view),
            "iris_ProjMat" => put(member.offset, projection),
            "iris_TextureMat" => {
                let (count, stride) = member.array.unwrap_or((1, 64));
                for index in 0..count {
                    let matrix = if index == 1 {
                        LIGHTMAP_TEXTURE_MATRIX
                    } else {
                        Mat4::IDENTITY
                    };
                    put(member.offset + index * stride, matrix);
                }
            }
            _ => {}
        }
    }
    bytes
}

/// Draws the fixture section's solid faces with the translated `gbuffers_terrain` program:
/// from the expanded vertices. Returns both colour targets' pixels.
fn draw_fixture(expanded: &Expanded) -> Vec<Vec<u8>> {
    let program = translate_fixture();
    let reflection = program.reflection.as_ref().expect("reflection");
    let gpu = &expanded.gpu;
    let device = &gpu.device;
    let module = |stage: shaderpack::StageKind| {
        let compiled = program
            .stages
            .iter()
            .find(|translated| translated.stage == stage)
            .and_then(|translated| translated.result.as_ref().ok())
            .expect("stage compiled");
        let wgsl = naga::back::wgsl::write_string(
            &compiled.module,
            &compiled.info,
            naga::back::wgsl::WriterFlags::empty(),
        )
        .expect("stage writes as WGSL");
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        })
    };
    let entry = |stage| {
        reflection
            .entry_points
            .iter()
            .find(|(kind, _)| *kind == stage)
            .map(|(_, name)| name.clone())
            .expect("entry point")
    };
    let vertex_module = module(shaderpack::StageKind::Vertex);
    let fragment_module = module(shaderpack::StageKind::Fragment);

    // Group 0: the loose uniforms; group 1: the shared sampler and the atlas.
    let uniforms = reflection.uniforms.as_ref().expect("program uses uniforms");
    let eye = Vec3::new(19.0, 20.0, 3.5);
    let model_view =
        glam::camera::rh::view::look_at_mat4(eye, Vec3::new(19.0, 0.0, 3.5), Vec3::NEG_Z);
    let projection =
        glam::camera::rh::proj::directx::orthographic(-2.0, 2.0, -2.0, 2.0, 0.1, 100.0);
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &uniform_words(uniforms, model_view, projection),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let group0_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: uniforms.binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let gtexture = reflection
        .textures
        .iter()
        .find(|texture| texture.name == "gtexture")
        .expect("program samples gtexture");
    // The atlas's sampler class (repeat/nearest) has its own binding.
    let sampler_binding = reflection
        .samplers
        .iter()
        .find(|sampler| !sampler.comparison)
        .expect("program samples with a filtering sampler")
        .binding;
    let group1_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: sampler_binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: gtexture.binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let mut atlas_pixels = vec![0_u8; (ATLAS_SIZE[0] * ATLAS_SIZE[1] * 4) as usize];
    for y in GRASS_RECT[1]..GRASS_RECT[1] + GRASS_RECT[3] {
        for x in GRASS_RECT[0]..GRASS_RECT[0] + GRASS_RECT[2] {
            let texel = ((y * ATLAS_SIZE[0] + x) * 4) as usize;
            atlas_pixels[texel..texel + 4].copy_from_slice(&SPRITE_RGBA);
        }
    }
    let atlas = device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: ATLAS_SIZE[0],
                height: ATLAS_SIZE[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &atlas_pixels,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let atlas_view = atlas.create_view(&Default::default());
    let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &group0_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: uniforms.binding,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });
    let group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &group1_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: sampler_binding,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: gtexture.binding,
                resource: wgpu::BindingResource::TextureView(&atlas_view),
            },
        ],
    });

    let layouts = vertex_buffer_layouts(&reflection.vertex_inputs, &expanded.layout)
        .expect("terrain supplies every input");
    let buffers: Vec<_> = layouts
        .iter()
        .map(|layout| {
            Some(wgpu::VertexBufferLayout {
                array_stride: layout.array_stride,
                step_mode: layout.step_mode,
                attributes: &layout.attributes,
            })
        })
        .collect();
    let targets: Vec<_> = reflection
        .color_outputs
        .iter()
        .map(|_| {
            Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        })
        .collect();
    assert_eq!(targets.len(), 2, "DRAWBUFFERS:01");
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&group0_layout), Some(&group1_layout)],
        immediate_size: 0,
    });
    let vertex_entry = entry(shaderpack::StageKind::Vertex);
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &vertex_module,
            entry_point: Some(&vertex_entry),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        // The translator flips clip-space Y, mirroring the counter-clockwise front faces.
        primitive: wgpu::PrimitiveState {
            front_face: wgpu::FrontFace::Cw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fragment_module,
            entry_point: Some(&entry(shaderpack::StageKind::Fragment)),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    });

    let texture = |format| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: TARGET_SIDE,
                height: TARGET_SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    };
    let colortex = [
        texture(wgpu::TextureFormat::Rgba8Unorm),
        texture(wgpu::TextureFormat::Rgba8Unorm),
    ];
    let depth = texture(wgpu::TextureFormat::Depth32Float);
    let views: Vec<_> = colortex
        .iter()
        .map(|texture| texture.create_view(&Default::default()))
        .collect();
    let depth_view = depth.create_view(&Default::default());
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&super::draw::face_indices(expanded.faces)),
        usage: wgpu::BufferUsages::INDEX,
    });
    let constants = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&CONSTANT_STREAM_WORDS),
        usage: wgpu::BufferUsages::VERTEX,
    });
    // The section's draw origin in slot 0, which packed draws select as their first instance.
    let origins = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&draw_origin(SECTION_ORIGIN, ANCHOR)),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let attachments: Vec<_> = views
            .iter()
            .map(|view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })
            })
            .collect();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
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
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group0, &[]);
        pass.set_bind_group(1, &group1, &[]);
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        let solid = &expanded.categories[0];
        pass.set_vertex_buffer(super::GEOMETRY_VERTEX_BUFFER, expanded.vertices.slice(..));
        pass.set_vertex_buffer(super::CONSTANT_VERTEX_BUFFER, constants.slice(..));
        if expanded.layout.packed() {
            pass.set_vertex_buffer(DRAW_ORIGIN_VERTEX_BUFFER, origins.slice(..));
        }
        pass.draw_indexed(
            0..(solid.end - solid.start) * 6,
            (solid.start * 4) as i32,
            0..1,
        );
    }
    gpu.queue.submit([encoder.finish()]);
    colortex
        .iter()
        .map(|texture| read_target(gpu, texture))
        .collect()
}

#[test]
fn translated_gbuffers_program_draws_expanded_terrain() {
    let Some(expanded) = expand_fixture("shader-pack terrain draw") else {
        return;
    };
    let pixels = draw_fixture(&expanded);
    let pixel = |target: usize, x: u32, y: u32| {
        let index = ((y * TARGET_SIDE + x) * 4) as usize;
        [
            pixels[target][index],
            pixels[target][index + 1],
            pixels[target][index + 2],
            pixels[target][index + 3],
        ]
    };
    // Block (2,5,3)'s top centre sits on its faces' shared diagonal, between AO 1.0 and 0.8.
    // Rows are stored bottom-up (the translator flips clip-space Y), so it lands on row 31.
    let albedo = pixel(0, 24, 31);
    let expected = SPRITE_RGBA.map(|channel| (f32::from(channel) * 0.9).round() as u8);
    assert!(
        albedo[..3]
            .iter()
            .zip(&expected)
            .all(|(a, e)| a.abs_diff(*e) <= 3)
            && albedo[3] == 255,
        "albedo {albedo:?} vs {expected:?}"
    );
    // Lightmap (48,240)..(96,232) midway, through gl_TextureMatrix[1]: (72 + 8) / 256, (236 + 8) / 256.
    let data = pixel(1, 24, 31);
    let lightmap = [80.0 / 256.0 * 255.0, 244.0 / 256.0 * 255.0];
    assert!(
        (f32::from(data[0]) - lightmap[0]).abs() <= 2.0
            && (f32::from(data[1]) - lightmap[1]).abs() <= 2.0,
        "lightmap {data:?} vs {lightmap:?}"
    );
    assert_eq!(
        data[2], GRASS_BLOCK_ID as u8,
        "mc_Entity.x reaches the program"
    );
    assert_eq!(pixel(0, 4, 4), [0; 4], "nothing draws outside the faces");
}

#[test]
fn packed_terrain_draws_the_pixels_wide_terrain_draws() {
    let Some(wide) = expand_fixture("shader-pack wide terrain draw") else {
        return;
    };
    let program = translate_fixture();
    let reflection = program.reflection.as_ref().expect("reflection");
    let layout = TerrainLayout::compact(&reflection.vertex_inputs);
    assert!(
        layout.stride() < TerrainLayout::WIDE.stride(),
        "the fixture program reads fewer fields than the wide stream holds"
    );
    let Some(packed) = expand_fixture_in("shader-pack packed terrain draw", layout) else {
        return;
    };
    assert_eq!(
        packed.words.len() as u64 * 4,
        u64::from(packed.faces) * 4 * layout.stride()
    );
    let wide_pixels = draw_fixture(&wide);
    let packed_pixels = draw_fixture(&packed);
    for (target, (packed, reference)) in packed_pixels.iter().zip(&wide_pixels).enumerate() {
        let drawn = reference.chunks(4).filter(|texel| texel[3] != 0).count();
        assert!(drawn > 0, "colortex{target} draws the section");
        assert!(
            packed == reference,
            "colortex{target}: packed and wide terrain differ"
        );
    }
}

fn read_target(gpu: &Gpu, texture: &wgpu::Texture) -> Vec<u8> {
    let bytes = u64::from(TARGET_SIDE * TARGET_SIDE * 4);
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TARGET_SIDE * 4),
                rows_per_image: Some(TARGET_SIDE),
            },
        },
        wgpu::Extent3d {
            width: TARGET_SIDE,
            height: TARGET_SIDE,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    readback
        .slice(..)
        .get_mapped_range()
        .expect("readback buffer is mapped")
        .to_vec()
}
