//! The skinning kernel against an original one-triangle rig on a real device.

use super::layout::ACTOR_VERTEX_WORDS;
use super::skin::{
    SKIN_PARAMS_BYTES, SkinJob, SkinParams, output_layout, rig_layout, skin_shader_source,
};
use crate::gpu_snapshot::Gpu;
use crate::shaderpack::geometry::{BLOCK_ID, LIGHTMAP, NORMAL, POSITION, UV0};
use bevy::math::Vec3;
use render_model::{ACTOR_GPU_INSTANCE_WORDS, ACTOR_RIG_VERTEX_WORDS};
use wgpu::util::DeviceExt;

const CAMERA: Vec3 = Vec3::new(1.0, 2.0, 3.0);
const ACTOR: Vec3 = Vec3::new(10.0, 64.0, -5.0);
const ENTITY_ID: i32 = 42;
const BLOCK_LIGHT: u32 = 3;
const SKY_LIGHT: u32 = 12;

/// Rig-local corners of one triangle in the XY plane, facing +Z, with front and back UVs.
const CORNERS: [([f32; 3], [f32; 2], [f32; 2]); 3] = [
    ([0.0, 0.0, 0.0], [0.0, 0.0], [0.5, 0.0]),
    ([1.0, 0.0, 0.0], [0.25, 0.0], [0.75, 0.0]),
    ([0.0, 1.0, 0.0], [0.0, 0.25], [0.5, 0.25]),
];

fn layout(
    gpu: &Gpu,
    descriptor: &bevy::render::render_resource::BindGroupLayoutDescriptor,
) -> wgpu::BindGroupLayout {
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &descriptor.entries,
        })
}

/// Skins the fixture rig at `ACTOR` and returns the kernel's Iris vertex words.
fn skin(name: &str) -> Option<Vec<u32>> {
    skin_with_uv(name, [0., 0., 1., 1.])
}

/// Skins the fixture after applying an animated UV transform.
fn skin_with_uv(name: &str, uv: [f32; 4]) -> Option<Vec<u32>> {
    skin_fixture(name, uv, [0.0; 2], [1.0; 2], 0)
}

/// Skins a triangle placed in an atlas sprite, optionally as a glint job.
fn skin_fixture(
    name: &str,
    uv: [f32; 4],
    origin: [f32; 2],
    span: [f32; 2],
    flags: u32,
) -> Option<Vec<u32>> {
    skin_corners(name, uv, origin, span, flags, &CORNERS, false)
}

/// Runs either production conversion entry with explicit authored corners.
#[allow(clippy::too_many_arguments, reason = "explicit kernel fixture inputs")]
fn skin_corners(
    name: &str,
    uv: [f32; 4],
    origin: [f32; 2],
    span: [f32; 2],
    flags: u32,
    corners: &[([f32; 3], [f32; 2], [f32; 2])],
    blocks: bool,
) -> Option<Vec<u32>> {
    let gpu = Gpu::for_fixture(name)?;
    let mut instance = vec![0_u32; ACTOR_GPU_INSTANCE_WORDS];
    // `world_from_actor` rows: a translation to ACTOR.
    for (row, translation) in ACTOR.to_array().into_iter().enumerate() {
        instance[row * 4 + row] = 1.0_f32.to_bits();
        instance[row * 4 + 3] = translation.to_bits();
    }
    // Bone bases 0, span 0, partial tick 1, no uv offset and unit uv scale.
    instance[16] = 1.0_f32.to_bits();
    for (index, value) in uv.into_iter().enumerate() {
        instance[20 + index] = value.to_bits();
    }
    let glint = render_model::ActorGlint::item(0.0, span).parameters();
    let glint_base = std::mem::offset_of!(render_model::ActorGpuInstance, glint) / 4;
    for (index, value) in glint.into_iter().enumerate() {
        instance[glint_base + index] = value.to_bits();
    }
    let mut vertices = vec![0_u32; corners.len() * ACTOR_RIG_VERTEX_WORDS];
    for (index, (position, uv, back_uv)) in corners.iter().copied().enumerate() {
        let base = index * ACTOR_RIG_VERTEX_WORDS;
        let floats = [
            position[0],
            position[1],
            position[2],
            0.0,
            0.0,
            1.0,
            origin[0] + uv[0] * span[0],
            origin[1] + uv[1] * span[1],
            origin[0] + back_uv[0] * span[0],
            origin[1] + back_uv[1] * span[1],
        ];
        for (offset, value) in floats.into_iter().enumerate() {
            vertices[base + offset] = value.to_bits();
        }
        vertices[base + 10] = 0;
        for (axis, value) in origin.into_iter().enumerate() {
            vertices[base + ACTOR_RIG_VERTEX_WORDS - 2 + axis] = value.to_bits();
        }
    }
    let identity_bone: Vec<u32> = [
        [1.0_f32, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]
    .into_iter()
    .flatten()
    .map(f32::to_bits)
    .collect();
    let job = SkinJob {
        instance: 0,
        first_output: 0,
        vertex_count: corners.len() as u32,
        flags,
        light: BLOCK_LIGHT | (SKY_LIGHT << 4),
        colour: u32::MAX,
        block_id: ENTITY_ID,
        first_output_high: 0,
    };
    let params = SkinParams {
        camera: [CAMERA.x, CAMERA.y, CAMERA.z, 0.0],
        counts: if blocks {
            [corners.len() as u32, 0, 0, 0]
        } else {
            [1, 0, corners.len() as u32, 0]
        },
    };
    let storage = |words: &[u32]| {
        gpu.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(words),
                usage: wgpu::BufferUsages::STORAGE,
            })
    };
    let sources = [
        storage(&instance),
        storage(&vertices),
        storage(&[0, corners.len() as u32]),
        storage(&identity_bone),
        storage(&identity_bone),
    ];
    let jobs = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&job),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let output_bytes = u64::from(job.output_vertices() * ACTOR_VERTEX_WORDS * 4);
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let uniform = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    assert_eq!(std::mem::size_of::<SkinParams>() as u64, SKIN_PARAMS_BYTES);

    let source = crate::shader_source::composed_prepared_with(
        &skin_shader_source(include_str!("skin.wesl")),
        &[],
        Vec::new(),
    );
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let rig_layout = layout(
        &gpu,
        &if blocks {
            super::skin::block_entity_layout()
        } else {
            rig_layout()
        },
    );
    let output_layout = layout(&gpu, &output_layout());
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&rig_layout), Some(&output_layout)],
            immediate_size: 0,
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(if blocks {
                "convert_block_entities"
            } else {
                "skin_rig"
            }),
            compilation_options: Default::default(),
            cache: None,
        });
    let block_vertices: Vec<_> = corners
        .iter()
        .map(|(position, uv, _)| render_model::BlockEntityVertex {
            position: *position,
            uv: *uv,
            color: [1.0; 4],
            normal: [0.0, 0.0, 1.0],
            actor_light: 0,
        })
        .collect();
    let block_buffer = storage(bytemuck::cast_slice(&block_vertices));
    let source_entries: Vec<_> = if blocks {
        vec![wgpu::BindGroupEntry {
            binding: 5,
            resource: block_buffer.as_entire_binding(),
        }]
    } else {
        sources
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect()
    };
    let source_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &rig_layout,
        entries: &source_entries,
    });
    let output_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &output_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: jobs.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
        ],
    });
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &source_group, &[]);
        pass.set_bind_group(1, &output_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output_bytes);
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = readback
        .slice(..)
        .get_mapped_range()
        .expect("readback buffer is mapped")
        .to_vec();
    Some(bytemuck::cast_slice(&bytes).to_vec())
}

fn half(bits: u32) -> f32 {
    half_to_f32(bits as u16)
}

/// IEEE half-precision bits to `f32` for the lightmap values the kernel writes.
fn half_to_f32(bits: u16) -> f32 {
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f32::from(bits & 0x3ff);
    let magnitude = if exponent == 0 {
        mantissa * 2f32.powi(-24)
    } else {
        (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15)
    };
    if bits & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

/// An entity triangle lands camera-relative with Java's entity attributes, and its
/// two-sided back face reverses the winding, flips the normal and samples the back UVs.
#[test]
fn a_skinned_actor_triangle_writes_iris_entity_vertices_and_its_back_face() {
    let Some(words) = skin("shader-pack actor skinning") else {
        return;
    };
    let vertex = |index: usize| &words[index * ACTOR_VERTEX_WORDS as usize..];
    let position = |index: usize| {
        let v = vertex(index);
        let at = POSITION.word as usize;
        Vec3::new(
            f32::from_bits(v[at]),
            f32::from_bits(v[at + 1]),
            f32::from_bits(v[at + 2]),
        )
    };
    let uv = |index: usize| {
        let at = UV0.word as usize;
        [
            f32::from_bits(vertex(index)[at]),
            f32::from_bits(vertex(index)[at + 1]),
        ]
    };
    let normal = |index: usize| {
        vertex(index)[NORMAL.word as usize]
            .to_le_bytes()
            .map(|b| b as i8)
    };
    let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3;
    for (index, (local, front_uv, _)) in CORNERS.into_iter().enumerate() {
        let expected = Vec3::from_array(local) + ACTOR - CAMERA;
        assert!(
            position(index).abs_diff_eq(expected, 1e-4),
            "corner {index}: {} vs {expected}",
            position(index)
        );
        assert!(close(uv(index), front_uv), "front uv {index}");
        assert_eq!(normal(index), [0, 0, 127, 0]);
        let v = vertex(index);
        let ids = v[BLOCK_ID.word as usize];
        assert_eq!((ids & 0xffff) as u16 as i16, ENTITY_ID as i16);
        assert_eq!(
            (ids >> 16) as u16 as i16,
            -1,
            "entities read Iris's BLOCK_RENDER_TYPE in mc_Entity.y"
        );
        let light = v[LIGHTMAP.word as usize];
        assert_eq!(
            [half(light & 0xffff), half(light >> 16)],
            [(BLOCK_LIGHT * 16) as f32, (SKY_LIGHT * 16) as f32],
            "raw Java lightmap coordinates"
        );
    }
    // Back face: corners 0, 2, 1 with the back UV map and the reversed normal.
    for (slot, corner) in [0_usize, 2, 1].into_iter().enumerate() {
        let back = 3 + slot;
        let (local, _, back_uv) = CORNERS[corner];
        assert!(
            position(back).abs_diff_eq(Vec3::from_array(local) + ACTOR - CAMERA, 1e-4),
            "back corner {slot}"
        );
        assert!(close(uv(back), back_uv), "back uv {slot}");
        assert_eq!(normal(back), [0, 0, -127, 0]);
    }
}

#[test]
fn animated_uvs_keep_their_fractional_and_repeating_coordinates() {
    let Some(words) = skin_with_uv("shader-pack repeating actor UVs", [-2.5, 3.25, 2., 4.]) else {
        return;
    };
    for (index, (_, uv, _)) in CORNERS.iter().enumerate() {
        let base = index * ACTOR_VERTEX_WORDS as usize + UV0.word as usize;
        assert_eq!(f32::from_bits(words[base]), -2.5 + uv[0] * 2.);
        assert_eq!(f32::from_bits(words[base + 1]), 3.25 + uv[1] * 4.);
    }
}

#[test]
fn item_glint_coordinates_are_independent_of_atlas_placement_and_dimensions() {
    for (origin, span) in [([0.0; 2], [1.0; 2]), ([0.375, 0.625], [0.25, 0.125])] {
        let Some(words) = skin_fixture(
            "shader-pack sprite-local glint UVs",
            [0., 0., 1., 1.],
            origin,
            span,
            super::skin::JOB_GLINT,
        ) else {
            return;
        };
        for (index, (_, front, _)) in CORNERS.into_iter().enumerate() {
            let base = index * ACTOR_VERTEX_WORDS as usize + UV0.word as usize;
            for (axis, expected) in front.into_iter().enumerate() {
                assert!((f32::from_bits(words[base + axis]) - expected).abs() < 1e-6);
            }
        }
        for (slot, corner) in [0, 2, 1].into_iter().enumerate() {
            let base = (slot + 3) * ACTOR_VERTEX_WORDS as usize + UV0.word as usize;
            for (axis, expected) in CORNERS[corner].2.into_iter().enumerate() {
                assert!((f32::from_bits(words[base + axis]) - expected).abs() < 1e-6);
            }
        }
    }
}

/// A single glinted instance whose textures are enough to exercise draw planning.
struct GlintSource<'a> {
    data: render_model::ActorGpuInstance,
    texture: &'a bevy::render::render_resource::Texture,
}

impl super::RigSource for GlintSource<'_> {
    /// Planning does not read storage buffers.
    fn buffers(&self) -> super::RigBuffers<'_> {
        unreachable!("draw planning does not bind buffers")
    }

    /// Visits the fixture instance with its artwork texture.
    fn for_each_instance(&self, visit: &mut dyn FnMut(super::RigInstance<'_>)) {
        visit(super::RigInstance {
            index: 0,
            data: &self.data,
            vertex_count: 3,
            texture: super::LayerTexture {
                texture: self.texture,
                layer: 0,
                compose: super::LayerCompose::Plain,
            },
            runtime_id: None,
            camera: true,
            light: [15; 2],
            blend: None,
            held: None,
        });
    }

    /// Uses the fixture texture for both glint variants.
    fn glint(&self, _item: bool) -> Option<&bevy::render::render_resource::Texture> {
        Some(self.texture)
    }
}

#[test]
fn glint_draws_honor_strength_and_the_configured_clock() {
    use super::view::ViewActors;
    use crate::shaderpack::draws::DrawKind;
    let (device, _) = wgpu::Device::noop(&Default::default());
    let texture = bevy::render::render_resource::Texture::from(device.create_texture(
        &wgpu::TextureDescriptor {
            label: None,
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
        },
    ));
    for hand in [false, true] {
        for strength in [0.0, 0.05, 0.25, 1.0] {
            let source = GlintSource {
                data: render_model::ActorGpuInstance {
                    material: assets::EntityRenderMaterial::Glint as u32,
                    glint: render_model::ActorGlint {
                        time_seconds: 37.0,
                        speed: 0.0,
                        strength,
                        ..Default::default()
                    }
                    .parameters(),
                    ..Default::default()
                },
                texture: &texture,
            };
            let mut actors = ViewActors::default();
            actors.plan_rig(
                &source,
                hand,
                &super::ActorSources::default(),
                &|_| true,
                &mut 0,
            );
            let glints: Vec<_> = actors
                .draws
                .iter()
                .filter(|draw| matches!(draw.kind, DrawKind::ArmorGlint | DrawKind::HandGlint))
                .collect();
            assert_eq!(glints.len(), usize::from(strength > 0.0));
            if let Some(draw) = glints.first() {
                assert_eq!(
                    draw.values.texture_matrix.transform_point3(Vec3::ZERO),
                    Vec3::ZERO
                );
                let job = actors
                    .jobs
                    .iter()
                    .find(|job| job.output_start() == draw.vertices.start)
                    .unwrap();
                let [red, green, blue, alpha] = job.colour.to_le_bytes();
                let test = draw.kind.alpha_test("gbuffers_basic", &[]).unwrap();
                assert_eq!(test.function, shaderpack::CompareFunction::Greater);
                assert!(f32::from(alpha) / 255.0 > test.reference);
                assert_eq!(
                    [red, green, blue],
                    render_model::ActorGlint::TINT_RGB
                        .map(|value| (value * strength * 255.0).round() as u8)
                );
                assert_eq!(alpha, u8::MAX);
            }
        }
    }
}

/// A scalar-lit top face keeps the outward normal of the source mesh builder.
#[test]
fn scalar_lit_block_entity_top_faces_keep_outward_normals() {
    use super::skin::block_entity_layout;
    use render_model::block_entity::mesh::{Layer, MeshBuilder};
    let Some(gpu) = Gpu::for_fixture("shader-pack scalar-lit block-entity normals") else {
        return;
    };
    let mut mesh = MeshBuilder::new([16; 2]);
    mesh.quad(
        Layer::Solid,
        [[0., 1., 0.], [1., 1., 0.], [1., 1., 1.], [0., 1., 1.]],
        [0., 0., 1., 1.],
        [1.; 4],
    );
    assert!(mesh.solid.iter().all(|vertex| vertex.normal == [0.; 3]));
    let storage = |contents: &[u8]| {
        gpu.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            })
    };
    let vertices = storage(bytemuck::cast_slice(&mesh.solid));
    let jobs = storage(bytemuck::bytes_of(&SkinJob::default()));
    let output_bytes = mesh.solid.len() as u64 * u64::from(ACTOR_VERTEX_WORDS) * 4;
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let params = SkinParams {
        counts: [mesh.solid.len() as u32, 0, u32::MAX, 0],
        ..Default::default()
    };
    let uniform = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let source = crate::shader_source::composed_prepared_with(
        &skin_shader_source(include_str!("skin.wesl")),
        &[],
        Vec::new(),
    );
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let source_layout = layout(&gpu, &block_entity_layout());
    let output_layout = layout(&gpu, &output_layout());
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&source_layout), Some(&output_layout)],
            immediate_size: 0,
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("convert_block_entities"),
            compilation_options: Default::default(),
            cache: None,
        });
    let sources = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &source_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 5,
            resource: vertices.as_entire_binding(),
        }],
    });
    let bindings = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &output_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: jobs.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
        ],
    });
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &sources, &[]);
        pass.set_bind_group(1, &bindings, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output_bytes);
    gpu.queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    receive.recv().unwrap().unwrap();
    let bytes = readback.slice(..).get_mapped_range().unwrap();
    let words: &[u32] = bytemuck::cast_slice(&bytes);
    for vertex in words.as_chunks::<{ ACTOR_VERTEX_WORDS as usize }>().0 {
        assert_eq!(vertex[NORMAL.word as usize].to_le_bytes(), [0, 127, 0, 0]);
    }
}

#[test]
fn ordinary_actor_draws_use_the_no_item_sentinel() {
    let (device, _) = wgpu::Device::noop(&Default::default());
    let texture = bevy::render::render_resource::Texture::from(device.create_texture(
        &wgpu::TextureDescriptor {
            label: None,
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
        },
    ));
    let source = GlintSource {
        data: Default::default(),
        texture: &texture,
    };
    let mut actors = super::view::ViewActors::default();
    actors.plan_rig(
        &source,
        false,
        &super::ActorSources::default(),
        &|_| true,
        &mut 0,
    );
    assert!(!actors.draws.is_empty());
    assert!(actors.draws.iter().all(|draw| draw.values.item_id == 0));
}

#[test]
fn failed_actor_stream_growth_keeps_the_previous_allocation() {
    use std::cell::Cell;
    use std::rc::Rc;
    let (device, _) = wgpu::Device::noop(&Default::default());
    let mut stream = super::view::Growable::default();
    let usage = wgpu::BufferUsages::STORAGE;
    let _ = stream.ensure(&device, "actor stream fixture", 16, usage);
    let previous = stream.buffer.as_ref().unwrap().clone();
    let called = Rc::new(Cell::new(false));
    let injected = Rc::clone(&called);
    let _failure = crate::gpu_allocation::inject(move |label, _| {
        if label == "actor stream fixture" {
            injected.set(true);
            true
        } else {
            false
        }
    });
    let _ = stream.ensure(&device, "actor stream fixture", 32, usage);
    assert!(
        called.get(),
        "actor stream growth must use a recoverable error scope"
    );
    assert_eq!(stream.buffer.as_ref(), Some(&previous));
}

#[test]
fn actor_quad_midpoints_match_across_both_triangles_and_back_faces() {
    let corners = [
        ([0., 0., 0.], [0., 0.], [0.5, 0.]),
        ([1., 0., 0.], [0.25, 0.], [0.75, 0.]),
        ([1., 1., 0.], [0.3, 0.25], [0.8, 0.25]),
        ([0., 1., 0.], [0., 0.2], [0.5, 0.2]),
    ];
    let triangles = [0, 1, 2, 0, 2, 3].map(|index| corners[index]);
    for blocks in [false, true] {
        let Some(words) = skin_corners(
            "shader-pack quad centres",
            [0., 0., 1., 1.],
            [0.; 2],
            [1.; 2],
            0,
            &triangles,
            blocks,
        ) else {
            return;
        };
        let front: [f32; 2] =
            std::array::from_fn(|axis| corners.iter().map(|c| c.1[axis]).sum::<f32>() / 4.0);
        let back: [f32; 2] =
            std::array::from_fn(|axis| corners.iter().map(|c| c.2[axis]).sum::<f32>() / 4.0);
        for index in 0..if blocks { 6 } else { 12 } {
            let base =
                index * ACTOR_VERTEX_WORDS as usize + super::layout::ACTOR_MID_UV_WORD as usize;
            let expected = if index < 6 { front } else { back };
            for axis in 0..2 {
                assert!(
                    (f32::from_bits(words[base + axis]) - expected[axis]).abs() < 1e-6,
                    "blocks={blocks}, vertex={index}, axis={axis}"
                );
            }
        }
    }
}

#[test]
fn actor_kernel_workgroup_sizes_match_host_dispatch() {
    let source = crate::shader_source::composed_prepared_with(
        &skin_shader_source(include_str!("skin.wesl")),
        &[],
        Vec::new(),
    );
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    assert_eq!(module.entry_points.len(), 2);
    for entry in &module.entry_points {
        assert_eq!(
            entry.workgroup_size,
            [super::skin::SKIN_WORKGROUP_SIZE, 1, 1]
        );
    }
}

#[test]
fn actor_stream_failure_keeps_world_replacement_and_native_actors() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions {
                enable: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let gpu = crate::shaderpack::pipelines::GpuContext {
        device: device.clone(),
        queue: queue.clone(),
        adapter,
    };
    let pack = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
        (
            "gbuffers_basic.vsh",
            "#version 120\nvoid main() { gl_Position = ftransform(); }",
        ),
        (
            "gbuffers_basic.fsh",
            "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }",
        ),
    ]));
    let pack = crate::shaderpack::loader::build(
        &pack,
        "actor allocation",
        "",
        &Default::default(),
        None,
        &gpu,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let mut state =
        crate::shaderpack::passes::ViewState::new(&device, std::sync::Arc::new(pack), [4, 4])
            .unwrap();
    assert!(state.geometry_targets_ready());
    let layout = |descriptor: bevy::render::render_resource::BindGroupLayoutDescriptor| {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &descriptor.entries,
        })
    };
    let rig_layout =
        bevy::render::render_resource::BindGroupLayout::from(layout(super::skin::rig_layout()));
    let block_layout = bevy::render::render_resource::BindGroupLayout::from(layout(
        super::skin::block_entity_layout(),
    ));
    let output_layout =
        bevy::render::render_resource::BindGroupLayout::from(layout(super::skin::output_layout()));
    let compose_layout =
        bevy::render::render_resource::BindGroupLayout::from(layout(super::compose::layout()));
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let pipeline = bevy::render::render_resource::ComputePipeline::from(pipeline);
    let texture = bevy::render::render_resource::Texture::from(device.create_texture(
        &wgpu::TextureDescriptor {
            label: None,
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
        },
    ));
    let source = GlintSource {
        data: Default::default(),
        texture: &texture,
    };
    let sources = super::ActorSources {
        actors: Some(&source),
        kernels: Some(super::skin::SkinKernels {
            rig: &pipeline,
            block_entities: &pipeline,
            compose: &pipeline,
            rig_layout: &rig_layout,
            block_entity_layout: &block_layout,
            output_layout: &output_layout,
            compose_layout: &compose_layout,
        }),
        ..Default::default()
    };
    let _failure = crate::gpu_allocation::inject(|label, _| label == "shader-pack actor vertices");
    let mut encoder = device.create_command_encoder(&Default::default());
    super::prepare(&device, &queue, &mut encoder, &mut state, &sources);
    assert!(state.actors.draws.is_empty());
    assert!(
        state.geometry_targets_ready() && !state.actors.replaces_actors,
        "actor allocation failure must keep terrain replacement and native actors"
    );
    super::prepare(&device, &queue, &mut encoder, &mut state, &sources);
    assert!(
        state.geometry_targets_ready()
            && state.actors.allocation_failed
            && !state.actors.replaces_actors,
        "actor failure stays native until view reconstruction"
    );
}
