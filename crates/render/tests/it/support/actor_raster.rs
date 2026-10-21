//! Shared actor geometry and shader raster fixture.
#![allow(
    dead_code,
    reason = "shared actor rasters serve different shader test targets"
)]

use super::{gpu_snapshot, render_api as render, shader_source};
use assets::EntityRenderMaterial;
use bevy::math::{Mat4, Vec3, Vec4};
use gpu_snapshot::{Draw, Gpu};
use render_model::ActorGpuInstance;
use render_model::{ActorRigVertex, EntityRigId};
use std::ops::Range;

/// Geometry and ordered instance ranges submitted to the production actor shader.
pub(crate) struct RasterBatch<'a> {
    pub vertices: &'a [ActorRigVertex],
    pub geometry: &'a [render_model::ActorRigGeometrySpan],
    pub instances: &'a [ActorGpuInstance],
    pub draws: &'a [(Range<u32>, Range<u32>)],
}

/// Renders an opaque actor batch with its exact per-instance geometry selection.
pub(crate) fn raster_batch(gpu: &Gpu, batch: RasterBatch<'_>, scene: RasterScene<'_>) -> Vec<u8> {
    raster_batch_in_scene(
        gpu,
        batch,
        Default::default(),
        scene,
        false,
        false,
        &[],
        Default::default(),
        None,
    )
}

/// Camera, actor transform and original texture inputs for a production shader raster.
pub(crate) struct RasterScene<'a> {
    pub clip_from_world: Mat4,
    pub world_from_actor: [[f32; 4]; 3],
    pub camera_position: Vec3,
    pub texture_size: [u32; 2],
    pub texels: &'a [[u8; 4]],
}

pub(crate) fn cube(size: [u32; 3], top: bool, bottom: bool) -> Vec<ActorRigVertex> {
    let face = |u| serde_json::json!({"uv":[u,0],"uv_size":[1,1]});
    let mut uv = serde_json::Map::new();
    if top {
        uv.insert("up".into(), face(0));
    }
    if bottom {
        uv.insert("down".into(), face(1));
    }
    let geometry = serde_json::json!({
        "format_version":"1.12.0",
        "minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture","texture_width":2,"texture_height":1},
            "bones":[{"name":"root","pivot":[0,0,0],"cubes":[{
                "origin":[-8,0,-8],"size":size,"uv":uv
            }]}]
        }]
    });
    let geometry = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        &geometry.to_string(),
    )
    .unwrap()
    .unwrap();
    render_model::skin_geometry(&geometry, EntityRigId(0))
        .unwrap()
        .vertices
        .to_vec()
}

pub(crate) fn source(gamma: bool) -> String {
    source_with_definitions(gamma, &[])
}

/// Compiles the production actor entry points with the selected fixture variants.
fn source_with_definitions(gamma: bool, definitions: &[&str]) -> String {
    let actor = include_str!("../../../src/actor.wesl")
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &render_model::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    let mut definitions = definitions.to_vec();
    if gamma {
        definitions.push("ACTOR_GAMMA_BLEND");
    }
    shader_source::standalone(&actor, &definitions)
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(21)")
        .replace("@group(3) @binding(0)", "@group(0) @binding(22)")
}

pub(crate) fn raster(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: EntityRenderMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
) -> Vec<u8> {
    raster_material(
        gpu,
        vertices,
        render_model::ActorMaterial {
            kind: material,
            ..Default::default()
        },
        below,
        texels,
    )
}

pub(crate) fn raster_material(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render_model::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
) -> Vec<u8> {
    raster_material_target(gpu, vertices, material, below, texels, false, false)
}

pub(crate) fn raster_material_target(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render_model::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
    srgb: bool,
    gamma: bool,
) -> Vec<u8> {
    raster_material_lighting(
        gpu,
        vertices,
        material,
        below,
        texels,
        srgb,
        gamma,
        render_model::pack_actor_light(15, 15),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "each knob is an independent shader input under test"
)]
pub(crate) fn raster_material_lighting(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render_model::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
    srgb: bool,
    gamma: bool,
    light: u32,
) -> Vec<u8> {
    raster_material_with_overlay(
        gpu, vertices, material, below, texels, srgb, gamma, light, 0, 0,
    )
}

/// Renders independent actor tint and overlay inputs with the fixed fixture camera.
#[allow(
    clippy::too_many_arguments,
    reason = "each knob is an independent shader input under test"
)]
pub(crate) fn raster_material_with_overlay(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render_model::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
    srgb: bool,
    gamma: bool,
    light: u32,
    overlay_rgba8: u32,
    tint_rgba8: u32,
) -> Vec<u8> {
    raster_material_with_state(
        gpu,
        vertices,
        material,
        below,
        texels,
        srgb,
        gamma,
        light,
        overlay_rgba8,
        tint_rgba8,
        &[],
        gpu_snapshot::RasterState::default(),
    )
}

/// Renders the production actor vertex and fragment entry points under explicit raster state.
#[allow(
    clippy::too_many_arguments,
    reason = "each knob is an independent shader input under test"
)]
pub(crate) fn raster_material_with_state(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render_model::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
    srgb: bool,
    gamma: bool,
    light: u32,
    overlay_rgba8: u32,
    tint_rgba8: u32,
    definitions: &[&str],
    state: gpu_snapshot::RasterState,
) -> Vec<u8> {
    let clip = Mat4::from_cols(
        Vec4::new(2.0, 0.0, 0.0, 0.0),
        Vec4::ZERO,
        Vec4::new(0.0, if below { 2.0 } else { -2.0 }, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 0.5, 1.0),
    );
    raster_material_in_scene(
        gpu,
        vertices,
        material,
        RasterScene {
            clip_from_world: clip,
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            camera_position: Vec3::ZERO,
            texture_size: [2, 1],
            texels: &texels,
        },
        srgb,
        gamma,
        light,
        overlay_rgba8,
        tint_rgba8,
        definitions,
        state,
    )
}

/// Renders arbitrary charts and projections through the production actor entry points.
#[allow(
    clippy::too_many_arguments,
    reason = "each knob is an independent shader input under test"
)]
pub(crate) fn raster_material_in_scene(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render_model::ActorMaterial,
    scene: RasterScene<'_>,
    srgb: bool,
    gamma: bool,
    light: u32,
    overlay_rgba8: u32,
    tint_rgba8: u32,
    definitions: &[&str],
    mut state: gpu_snapshot::RasterState,
) -> Vec<u8> {
    assert_eq!(
        scene.texels.len(),
        (scene.texture_size[0] * scene.texture_size[1]) as usize
    );
    let companion = state.actor_companion.take().map(|material| {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("actor mask companion fixture"),
            size: wgpu::Extent3d {
                width: gpu_snapshot::SNAPSHOT_SIDE,
                height: gpu_snapshot::SNAPSHOT_SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: state.multisample.count,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        raster_material_in_scene(
            gpu,
            vertices,
            material,
            RasterScene { ..scene },
            srgb,
            gamma,
            light,
            overlay_rgba8,
            tint_rgba8,
            &[],
            gpu_snapshot::RasterState {
                depth_target: Some(texture.clone()),
                multisample: wgpu::MultisampleState {
                    alpha_to_coverage_enabled: false,
                    ..state.multisample
                },
                ..Default::default()
            },
        );
        texture.create_view(&Default::default())
    });
    let mut definitions = definitions.to_vec();
    if companion.is_some() {
        definitions.push("MASK_DISSOLVE");
        if state.multisample.count > 1 {
            definitions.push("MASK_DISSOLVE_MSAA");
        }
    }
    let instance = ActorGpuInstance {
        world_from_actor: scene.world_from_actor,
        partial_tick: 1.0,
        uv_anim: render_model::IDENTITY_UV_ANIM,
        light,
        overlay_rgba8,
        tint: tint_rgba8,
        multitexture_layers: [u32::MAX; 2],
        material: material.gpu_word(),
        dissolve_multiplier: material.dissolve_multiplier,
        light_color_multiplier: material.light_color_multiplier,
        glint: material.glint.parameters(),
        ..Default::default()
    };
    raster_batch_in_scene(
        gpu,
        RasterBatch {
            vertices,
            geometry: &[render_model::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: vertices.len() as u32,
            }],
            instances: &[instance],
            draws: &[(0..vertices.len() as u32, 0..1)],
        },
        material,
        scene,
        srgb,
        gamma,
        &definitions,
        state,
        companion,
    )
}

/// Binds shared artwork and independent geometry, bones and draw ranges for an actor batch.
#[allow(
    clippy::too_many_arguments,
    reason = "preserves the existing independent raster inputs"
)]
fn raster_batch_in_scene(
    gpu: &Gpu,
    batch: RasterBatch<'_>,
    material: render_model::ActorMaterial,
    scene: RasterScene<'_>,
    srgb: bool,
    gamma: bool,
    definitions: &[&str],
    mut state: gpu_snapshot::RasterState,
    companion: Option<wgpu::TextureView>,
) -> Vec<u8> {
    let view = gpu.buffer(
        &gpu_snapshot::view(scene.clip_from_world, scene.camera_position),
        wgpu::BufferUsages::UNIFORM,
    );
    let affine: [[f32; 4]; 3] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let instance = gpu.buffer(
        bytemuck::cast_slice(batch.instances),
        wgpu::BufferUsages::STORAGE,
    );
    let mesh = gpu.buffer(
        bytemuck::cast_slice(batch.vertices),
        wgpu::BufferUsages::STORAGE,
    );
    let spans = gpu.buffer(
        bytemuck::cast_slice(batch.geometry),
        wgpu::BufferUsages::STORAGE,
    );
    let bones = gpu.buffer(bytemuck::cast_slice(&affine), wgpu::BufferUsages::STORAGE);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("opposing cube UV witness"),
        size: wgpu::Extent3d {
            width: scene.texture_size[0],
            height: scene.texture_size[1],
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
        texture.as_image_copy(),
        bytemuck::cast_slice(scene.texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(scene.texture_size[0] * 4),
            rows_per_image: Some(scene.texture_size[1]),
        },
        texture.size(),
    );
    let skin = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Nearest,
        mag_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let class = gpu.buffer(&[0.0; 4], wgpu::BufferUsages::UNIFORM);
    let lightmap = gpu.lightmap(&[1.0; 256 * 4]);
    let mut atmosphere = [0.0; 32];
    atmosphere[19] = 1000.0;
    atmosphere[20] = 1001.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: instance.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: mesh.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: spans.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: bones.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: bones.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: wgpu::BindingResource::TextureView(&skin),
        },
        wgpu::BindGroupEntry {
            binding: 7,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 8,
            resource: class.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 9,
            resource: wgpu::BindingResource::TextureView(&skin),
        },
        wgpu::BindGroupEntry {
            binding: 10,
            resource: wgpu::BindingResource::TextureView(&skin),
        },
        wgpu::BindGroupEntry {
            binding: 11,
            resource: wgpu::BindingResource::TextureView(&skin),
        },
        wgpu::BindGroupEntry {
            binding: 20,
            resource: lightmap.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 21,
            resource: atmosphere.as_entire_binding(),
        },
    ];
    let glint_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut bindings = bindings.to_vec();
    bindings.push(wgpu::BindGroupEntry {
        binding: 12,
        resource: wgpu::BindingResource::TextureView(&glint_view),
    });
    bindings.push(wgpu::BindGroupEntry {
        binding: 13,
        resource: wgpu::BindingResource::Sampler(&sampler),
    });
    bindings.push(wgpu::BindGroupEntry {
        binding: 14,
        resource: wgpu::BindingResource::TextureView(&glint_view),
    });
    if let Some(view) = &companion {
        bindings.push(wgpu::BindGroupEntry {
            binding: 22,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    let draws: Vec<_> = batch
        .draws
        .iter()
        .map(|(vertices, _)| Draw {
            fragment: state.actor_fragment.unwrap_or("actor_fragment"),
            vertices: vertices.clone(),
            bindings: &bindings,
            blend: material.state.and_then(render::actor_material_blend_state),
            write_depth: state.actor_fragment.is_none()
                && material.state.is_none_or(|state| state.depth_write),
        })
        .collect();
    state.instance_ranges = batch
        .draws
        .iter()
        .map(|(_, instances)| instances.clone())
        .collect();
    // Automatic stage inference treats a dimensions-only vertex texture as unfilterable.
    let entries: Vec<_> = bindings
        .iter()
        .map(|entry| wgpu::BindGroupLayoutEntry {
            binding: entry.binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: match &entry.resource {
                wgpu::BindingResource::Buffer(binding) => wgpu::BindingType::Buffer {
                    ty: if binding.buffer.usage().contains(wgpu::BufferUsages::UNIFORM) {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage { read_only: true }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                wgpu::BindingResource::Sampler(_) => {
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
                }
                wgpu::BindingResource::TextureView(view) => wgpu::BindingType::Texture {
                    sample_type: if entry.binding == 22 {
                        wgpu::TextureSampleType::Depth
                    } else {
                        wgpu::TextureSampleType::Float { filterable: true }
                    },
                    view_dimension: if std::ptr::eq(*view, &skin) {
                        wgpu::TextureViewDimension::D2Array
                    } else {
                        wgpu::TextureViewDimension::D2
                    },
                    multisampled: entry.binding == 22 && state.multisample.count > 1,
                },
                _ => panic!("unsupported actor fixture binding"),
            },
            count: None,
        })
        .collect();
    let layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("actor fixture binding contract"),
            entries: &entries,
        });
    let state = gpu_snapshot::RasterState {
        layout: Some(
            gpu.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("actor fixture pipeline"),
                    bind_group_layouts: &[Some(&layout)],
                    ..Default::default()
                }),
        ),
        ..state
    };
    if srgb {
        gpu.render_srgb_with_state(
            &source_with_definitions(gamma, definitions),
            "actor_vertex",
            &draws,
            state,
        )
    } else {
        gpu.render_with_state(
            &source_with_definitions(gamma, definitions),
            "actor_vertex",
            &draws,
            state,
        )
    }
}
