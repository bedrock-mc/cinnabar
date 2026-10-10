//! Sorted and direct water draws and model draws through the shared transparent pipeline
//! produce the pixels of the liquid and model programs it composes, drawn in the same order
//! through their own pipelines.
use crate::{chunk_constants, gpu_snapshot, material_shader, shader_source, solid_terrain_raster};
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::Gpu;
use meshing::liquid::TRANSPARENT_WATER_DRAW_FLAG;
use meshing::{Face, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef};
use render::PackedTransparentDrawRef;

const SIDE: u32 = 128;
const WATER_QUADS: u32 = 2;
const MODEL_QUADS: u32 = 2;
/// Model draw refs start after the liquid records, so their instances start here.
const MODEL_INSTANCE_BASE: u32 = WATER_QUADS * 2;

/// A world quad: its family and the instance range its draw submits.
#[derive(Clone, Copy)]
enum Quad {
    Water(u32),
    Model(u32),
}

struct Fixture {
    group: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    indices: wgpu::Buffer,
}

impl Fixture {
    fn new(gpu: &Gpu) -> Self {
        let storage = wgpu::BufferUsages::STORAGE;
        let uniform = wgpu::BufferUsages::UNIFORM;
        // Metadata slot 0 at the world origin; both families read it through base vertex 0.
        let origins = gpu.words(&[0, 0, 0, 0, 0, 0, 0, 0], storage);
        // Material 0 textures water, material 1 the models; neither animates.
        let materials = gpu.words(
            &[
                [0, 0, assets::NO_ANIMATION, 0, 0, 0],
                [
                    0,
                    assets::MATERIAL_FLAG_ISOTROPIC,
                    assets::NO_ANIMATION,
                    0,
                    0,
                    0,
                ],
            ]
            .concat(),
            storage,
        );
        // Liquid records, then model draw refs, model refs and lighting records.
        let draw_word = WATER_QUADS as usize * 4;
        let ref_word = draw_word + MODEL_QUADS as usize * 2;
        let light_word = ref_word + MODEL_QUADS as usize * 4;
        let mut geometry = Vec::new();
        for quad in 0..WATER_QUADS as u8 {
            let lighting = (light_word / 2) as u32 + u32::from(quad);
            geometry.extend(
                PackedLiquidQuad::try_pack(
                    [quad, 0, quad],
                    Face::PositiveY,
                    [255; 4],
                    0,
                    lighting,
                    [0, 0],
                    false,
                )
                .unwrap()
                .words(),
            );
        }
        for quad in 0..MODEL_QUADS {
            geometry.extend(PackedModelDrawRef::new((ref_word / 4) as u32 + quad, 0).words());
        }
        for quad in 0..MODEL_QUADS {
            let lighting = (light_word / 2) as u32 + WATER_QUADS + quad;
            geometry.extend(PackedModelRef::new(0, quad, lighting, 1).words());
        }
        geometry.extend(solid_terrain_raster::lighting_words(
            (WATER_QUADS + MODEL_QUADS) as usize,
        ));
        let geometry = gpu.words(&geometry, storage);
        // Refs list the water records in reverse, so a ref slot and the record it names differ,
        // as they do in a sorted snapshot.
        let refs = (0..WATER_QUADS)
            .rev()
            .flat_map(|quad| bytemuck::cast::<_, [u32; 2]>(PackedTransparentDrawRef::new(quad, 0)))
            .collect::<Vec<_>>();
        let refs = gpu.words(&refs, storage);
        // One template per model quad: a near-top face under the water of the same block.
        let mut templates = vec![MODEL_QUADS];
        for quad in 0..MODEL_QUADS {
            templates.extend([quad, 1, 0]);
        }
        for quad in 0..MODEL_QUADS as i16 {
            let offset = quad * 256;
            let positions: [i16; 12] = [
                offset,
                240,
                offset,
                offset,
                240,
                offset + 256,
                offset + 256,
                240,
                offset + 256,
                offset + 256,
                240,
                offset,
            ];
            templates.extend(
                positions
                    .chunks_exact(2)
                    .map(|p| u32::from(p[0] as u16) | (u32::from(p[1] as u16) << 16)),
            );
            let uvs: [u16; 8] = [0, 0, 0, 4096, 4096, 4096, 4096, 0];
            templates.extend(
                uvs.chunks_exact(2)
                    .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16)),
            );
            templates.extend([1, 0]);
        }
        let templates = gpu.words(&templates, storage);
        let empty = gpu.words(&[0; 64], storage);
        // Biome tint 0 is the fallback: a blue water tint at half opacity.
        let mut tint = [0.0_f32; 8 + assets::SEASONAL_FOLIAGE_COUNT * 4];
        tint[5] = f32::from_bits(0xff_80_40);
        tint[7] = 0.5;
        let tints = gpu.buffer(&tint, storage);
        let query_tables = gpu.words(&meshing::biome_lattice::query_table_words(), uniform);
        let animations = gpu.words(&[0; 8], storage);
        let frames = gpu.words(&[0; 4], storage);
        let clock = gpu.words(&[0; 4], uniform);
        let frame = render::AtmosphereFrame::default();
        let atmosphere = gpu.buffer(bytemuck::cast_slice(std::slice::from_ref(&frame)), uniform);
        let lightmap = gpu.buffer(
            bytemuck::cast_slice(&render::LightmapInputs::default().build()),
            uniform,
        );
        let eye = Vec3::new(1.0, 4.0, 1.0);
        let world_from_view = Mat4::look_at_rh(eye, Vec3::new(1.0, 0.0, 1.0), Vec3::Z);
        let clip_from_world =
            Mat4::perspective_infinite_reverse_rh(1.0, 1.0, 0.05) * world_from_view;
        let view = gpu.buffer(&gpu_snapshot::view(clip_from_world, eye), uniform);
        let atlas = solid_terrain_raster::pattern_texture(gpu);
        let sampler = gpu.device.create_sampler(&Default::default());

        let buffer = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let read = wgpu::BufferBindingType::Storage { read_only: true };
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("transparent terrain fixture"),
                entries: &[
                    buffer(0, wgpu::BufferBindingType::Uniform),
                    buffer(2, read),
                    buffer(3, read),
                    texture(4),
                    texture(5),
                    sampler_entry(6),
                    buffer(7, read),
                    buffer(8, read),
                    buffer(9, read),
                    buffer(10, read),
                    buffer(11, wgpu::BufferBindingType::Uniform),
                    buffer(12, read),
                    buffer(13, read),
                    buffer(14, read),
                    buffer(15, wgpu::BufferBindingType::Uniform),
                    texture(material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0]),
                    texture(material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1]),
                    sampler_entry(material_shader::NATIVE_LEAF_SAMPLER_BINDING),
                    buffer(
                        material_shader::BIOME_QUERY_TABLES_BINDING,
                        wgpu::BufferBindingType::Uniform,
                    ),
                    buffer(20, wgpu::BufferBindingType::Uniform),
                ],
            });
        let entries = [
            (0, view.as_entire_binding()),
            (2, origins.as_entire_binding()),
            (3, materials.as_entire_binding()),
            (4, wgpu::BindingResource::TextureView(&atlas)),
            (5, wgpu::BindingResource::TextureView(&atlas)),
            (6, wgpu::BindingResource::Sampler(&sampler)),
            (7, empty.as_entire_binding()),
            (8, tints.as_entire_binding()),
            (9, animations.as_entire_binding()),
            (10, frames.as_entire_binding()),
            (11, clock.as_entire_binding()),
            (12, templates.as_entire_binding()),
            (13, geometry.as_entire_binding()),
            (14, refs.as_entire_binding()),
            (15, atmosphere.as_entire_binding()),
            (
                material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                wgpu::BindingResource::TextureView(&atlas),
            ),
            (
                material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                wgpu::BindingResource::TextureView(&atlas),
            ),
            (
                material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                wgpu::BindingResource::Sampler(&sampler),
            ),
            (
                material_shader::BIOME_QUERY_TABLES_BINDING,
                query_tables.as_entire_binding(),
            ),
            (20, lightmap.as_entire_binding()),
        ]
        .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &entries,
        });
        Self {
            group,
            layout,
            indices: gpu.words(
                &chunk_constants::STATIC_QUAD_INDICES,
                wgpu::BufferUsages::INDEX,
            ),
        }
    }

    /// The production blend, depth and cull state over `source`'s entry points.
    fn pipeline(
        &self,
        gpu: &Gpu,
        source: &str,
        (vertex, fragment): (&str, &str),
        front_face: wgpu::FrontFace,
    ) -> wgpu::RenderPipeline {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&self.layout],
                push_constant_ranges: &[],
            });
        gpu.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    front_face,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::GreaterEqual,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::RED
                            | wgpu::ColorWrites::GREEN
                            | wgpu::ColorWrites::BLUE,
                    })],
                }),
                multiview: None,
                cache: None,
            })
    }

    /// Draws `quads` in order, each through the pipeline, base vertex and first instance
    /// `select` picks, and reads the pixels.
    fn render<'p>(
        &self,
        gpu: &Gpu,
        quads: &[Quad],
        select: impl Fn(Quad) -> (&'p wgpu::RenderPipeline, i32, u32),
    ) -> Vec<u8> {
        let size = wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        };
        let texture = |format, usage| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let colour = texture(
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let colour_view = colour.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &colour_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.12,
                            g: 0.18,
                            b: 0.25,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            for &quad in quads {
                let (pipeline, base_vertex, first_instance) = select(quad);
                pass.set_pipeline(pipeline);
                pass.draw_indexed(0..6, base_vertex, first_instance..first_instance + 1);
            }
        }
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIDE) * u64::from(SIDE) * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            colour.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIDE * 4),
                    rows_per_image: Some(SIDE),
                },
            },
            size,
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
        readback.slice(..).get_mapped_range().to_vec()
    }
}

fn mismatches(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}

#[test]
fn shared_transparent_pipeline_draws_water_and_models_like_their_own_pipelines() {
    // Both programs' storage buffers together exceed the default per-stage limit, which the
    // renderer raises to the shared layout's count.
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "shared transparent pipeline",
        wgpu::Features::empty(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: render::required_vertex_storage_buffers(),
            ..Default::default()
        },
    ) else {
        return;
    };
    let fixture = Fixture::new(&gpu);
    let lightmap =
        |source: String| source.replace("@group(1) @binding(0)", "@group(0) @binding(20)");
    let defs = ["NATIVE_GAMMA_BLEND"];
    // Each family's own program, as its dedicated pipeline ran it before the merge.
    let liquid = lightmap(format!(
        "{}\n{LIQUID_REFERENCE}",
        shader_source::standalone(include_str!("../../src/liquid.wgsl"), &defs)
    ));
    let model = lightmap(format!(
        "{}\n{MODEL_REFERENCE}",
        shader_source::standalone(include_str!("../../src/model.wgsl"), &defs)
    ));
    let shared = lightmap(shader_source::composed(
        include_str!("../../src/transparent_terrain.wgsl"),
        &defs,
    ));
    let liquid = fixture.pipeline(
        &gpu,
        &liquid,
        ("reference_vertex", "fragment"),
        wgpu::FrontFace::Cw,
    );
    let model = fixture.pipeline(
        &gpu,
        &model,
        ("reference_vertex", "fragment_blend"),
        wgpu::FrontFace::Ccw,
    );
    let shared = fixture.pipeline(&gpu, &shared, ("vertex", "fragment"), wgpu::FrontFace::Ccw);

    // Back to front: each block's model face, then the water face above it.
    let order = [
        Quad::Model(0),
        Quad::Water(0),
        Quad::Model(1),
        Quad::Water(1),
    ];
    // A water record's sorted ref slot; refs list the records in reverse.
    let slot = |record: u32| WATER_QUADS - 1 - record;
    let expected = fixture.render(&gpu, &order, |quad| match quad {
        Quad::Water(index) => (&liquid, 0, slot(index)),
        Quad::Model(index) => (&model, 0, MODEL_INSTANCE_BASE + index),
    });
    let actual = fixture.render(&gpu, &order, |quad| match quad {
        Quad::Water(index) => (&shared, 0, slot(index) | TRANSPARENT_WATER_DRAW_FLAG),
        Quad::Model(index) => (&shared, 0, MODEL_INSTANCE_BASE + index),
    });
    // Direct water names its record and passes metadata index 0 plus one as base vertex 4.
    let direct = fixture.render(&gpu, &order, |quad| match quad {
        Quad::Water(index) => (&shared, 4, index | TRANSPARENT_WATER_DRAW_FLAG),
        Quad::Model(index) => (&shared, 0, MODEL_INSTANCE_BASE + index),
    });
    let models_only = fixture.render(&gpu, &order[..1], |_| (&model, 0, MODEL_INSTANCE_BASE));
    gpu_snapshot::save("transparent_terrain_expected", &expected);
    gpu_snapshot::save("transparent_terrain_actual", &actual);
    gpu_snapshot::save("transparent_terrain_direct", &direct);
    let background = &expected[..4];
    assert!(
        expected.chunks_exact(4).any(|pixel| pixel != background),
        "the fixture draws something"
    );
    assert_ne!(
        mismatches(&expected, &models_only),
        0,
        "water must blend over the models"
    );
    assert_eq!(mismatches(&expected, &actual), 0, "sorted refs");
    assert_eq!(mismatches(&expected, &direct), 0, "direct records");
}

const LIQUID_REFERENCE: &str = r#"
@vertex fn reference_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    return vertex_for_ref(transparent_refs[instance_index], vertex_index);
}
"#;

const MODEL_REFERENCE: &str = r#"
@vertex fn reference_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    return model_vertex(vertex_index, instance_index);
}
"#;
