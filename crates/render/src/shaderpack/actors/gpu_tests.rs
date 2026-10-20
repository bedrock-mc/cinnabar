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
    let mut vertices = vec![0_u32; CORNERS.len() * ACTOR_RIG_VERTEX_WORDS];
    for (index, (position, uv, back_uv)) in CORNERS.into_iter().enumerate() {
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
        vertex_count: 3,
        flags,
        light: BLOCK_LIGHT | (SKY_LIGHT << 4),
        colour: u32::MAX,
        block_id: ENTITY_ID,
        first_output_high: 0,
    };
    let params = SkinParams {
        camera: [CAMERA.x, CAMERA.y, CAMERA.z, 0.0],
        counts: [1, 0, 3, 0],
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
        storage(&[0, 3]),
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
    let rig_layout = layout(&gpu, &rig_layout());
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
            entry_point: Some("skin_rig"),
            compilation_options: Default::default(),
            cache: None,
        });
    let source_entries: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: buffer.as_entire_binding(),
        })
        .collect();
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
        for strength in [0.0, 0.25, 1.0] {
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
                assert_eq!(
                    job.colour.to_le_bytes(),
                    [(strength * 255.0).round() as u8; 4]
                );
            }
        }
    }
}
