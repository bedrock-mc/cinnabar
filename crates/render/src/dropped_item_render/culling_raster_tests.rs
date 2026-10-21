use super::*;
use crate::gpu_snapshot::{self, Draw, Gpu, RasterState};
use bevy::math::{Mat4, Vec3};
use render_model::dropped_item::cube_mesh;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Invokes the production vertex shader with the builders' unchanged vertex data.
const VERTEX: &str = r#"
@group(0) @binding(22) var<storage, read> mesh: array<u32>;
@vertex fn fixture_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let base = index * ITEM_MESH_VERTEX_WORDS;
    var input: VertexInput;
    input.position = vec3(bitcast<f32>(mesh[base]), bitcast<f32>(mesh[base+1u]), bitcast<f32>(mesh[base+2u]));
    input.uv = vec2(bitcast<f32>(mesh[base+3u]), bitcast<f32>(mesh[base+4u]));
    input.normal = vec3(bitcast<f32>(mesh[base+5u]), bitcast<f32>(mesh[base+6u]), bitcast<f32>(mesh[base+7u]));
    input.layer = mesh[base+8u];
    input.color = unpack4x8unorm(mesh[base+9u]);
    input.glint_scale = vec2(bitcast<f32>(mesh[base+10u]), bitcast<f32>(mesh[base+11u]));
    input.row_0 = vec4(1.0,0.0,0.0,0.0);
    input.row_1 = vec4(0.0,1.0,0.0,0.0);
    input.row_2 = vec4(0.0,0.0,1.0,0.0);
    input.packed = vec4(0u,15u,15u,0u);
    input.glint = vec4(0.0);
    return item_vertex(input);
}
"#;

/// Draws one model through the real item shader and requested cull/coverage mode.
fn raster(gpu: &Gpu, model: &DroppedItemModel, eye: Vec3, samples: u32, cull: bool) -> Vec<u8> {
    let side = 32;
    let mut atlas = vec![255; side * side * 4];
    let vertices = build_model(&mut atlas, model, side as u32).unwrap();
    let mesh = gpu.words(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("owned dropped item culling fixture"),
            size: wgpu::Extent3d {
                width: side as u32,
                height: side as u32,
                depth_or_array_layers: (atlas.len() / (side * side * 4)) as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &atlas,
    );
    let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..default()
    });
    let sampler = gpu
        .device
        .create_sampler(&wgpu::SamplerDescriptor::default());
    let (matrix, eye) = camera(model, eye);
    let draw = ItemDraw {
        vertices: 0..vertices.len() as u32,
        instance: 0,
        cull_back: model.can_cull_back_faces(),
        bounds: DroppedItemBounds::from_mesh(&vertices).and_then(|bounds| {
            bounds.transformed(render_model::dropped_item::dropped_item_transform(
                [0.0; 3], 0.0, 1.0,
            ))
        }),
    };
    let cull =
        cull && samples == 1 && draw.culls_in_view(DroppedItemCullView::new(matrix, eye).as_ref());
    let view = gpu.buffer(
        &gpu_snapshot::view(matrix, eye),
        wgpu::BufferUsages::UNIFORM,
    );
    let light = gpu.lightmap(&[1.0; 256 * 4]);
    // Keep the nearby witness outside the distance-fog interval so its colour proves visibility.
    let mut atmosphere = [0.0; 32];
    atmosphere[19] = 1000.0;
    atmosphere[20] = 1001.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let environment = gpu.buffer(&[1.0_f32, 0.0, 0.0, 1.0], wgpu::BufferUsages::UNIFORM);
    let glint = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("unused dropped item glint"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&default());
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(&atlas),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: environment.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::TextureView(&glint),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 20,
            resource: light.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 21,
            resource: atmosphere.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 22,
            resource: mesh.as_entire_binding(),
        },
    ];
    let mut source = crate::shader_source::standalone(
        include_str!("../dropped_item.wesl"),
        if samples > 1 {
            &[crate::alpha_coverage::SHADER_DEF]
        } else {
            &[]
        },
    )
    .replace("@vertex\nfn item_vertex", "fn item_vertex")
    .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
    .replace("@group(1) @binding(1)", "@group(0) @binding(21)");
    source.push_str(&VERTEX.replace(
        "ITEM_MESH_VERTEX_WORDS",
        &(ITEM_MESH_VERTEX_BYTES / size_of::<u32>()).to_string(),
    ));
    gpu.render_srgb_with_state(
        &source,
        "fixture_vertex",
        &[Draw {
            fragment: "item_fragment",
            vertices: 0..vertices.len() as u32,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }],
        RasterState {
            primitive: wgpu::PrimitiveState {
                cull_mode: cull.then_some(wgpu::Face::Back),
                ..default()
            },
            multisample: wgpu::MultisampleState {
                count: samples,
                alpha_to_coverage_enabled: samples > 1,
                ..default()
            },
            ..default()
        },
    )
}

/// Builds the same reverse-Z camera for pixel rendering and per-view eligibility.
fn camera(model: &DroppedItemModel, eye: Vec3) -> (Mat4, Vec3) {
    let center = if matches!(model, DroppedItemModel::NativeSprite(_)) {
        Vec3::new(0.0, 0.5, 0.0)
    } else {
        Vec3::ZERO
    };
    let eye = center + eye;
    let up = if (eye - center).normalize().dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let matrix = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        0.65,
        1.0,
        render_api::CAMERA_NEAR_PLANE_BLOCKS,
    ) * glam::camera::rh::view::look_at_mat4(eye, center, up);
    (matrix, eye)
}

/// Produces a colored binary-alpha sprite with a central hole and distinct edge texels.
fn sprite() -> render_model::DroppedItemSprite {
    let mut pixels: Vec<u8> = (0..16)
        .flat_map(|i| [40 + i * 10, 200 - i * 5, 80, 255])
        .collect();
    for i in [5, 6, 9, 10] {
        pixels[i * 4 + 3] = 0;
    }
    render_model::DroppedItemSprite {
        width: 4,
        height: 4,
        rgba8: pixels.into(),
    }
}

#[test]
fn item_culling_preserves_pixels_from_faces_edges_and_corners() {
    let Some(gpu) = Gpu::for_fixture("dropped item culling pixels") else {
        return;
    };
    let sprite = sprite();
    let cube = render_model::DroppedItemCube {
        tile: 1,
        faces: std::array::from_fn(|i| Arc::from([60 + i as u8 * 25, 150, 70, 255])),
        tints: [render_model::OPAQUE_WHITE; 6],
    };
    let quads: Vec<_> = cube_mesh([0; 6], [render_model::OPAQUE_WHITE; 6], 1, 1)
        .unwrap()
        .as_chunks::<6>()
        .0
        .iter()
        .map(|face| {
            let order = if face[1].position == face[5].position {
                [0, 4, 1, 2]
            } else {
                [0, 1, 2, 5]
            };
            assets::ModelQuad {
                positions: order.map(|i| face[i].position.map(|p| ((p + 0.5) * 256.0) as i16)),
                uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
                material: 0,
                flags: 0,
            }
        })
        .collect();
    let block = render_model::DroppedItemBlock {
        materials: Arc::from([(
            render_model::DroppedItemSprite {
                width: 1,
                height: 1,
                rgba8: Arc::from([70, 180, 90, 255]),
            },
            render_model::OPAQUE_WHITE,
        )]),
        quads: quads.into(),
        rotation: 1,
    };
    let mut opaque_sprite = sprite.clone();
    for pixel in Arc::make_mut(&mut opaque_sprite.rgba8)
        .as_chunks_mut::<4>()
        .0
    {
        pixel[3] = 255;
    }
    let models = [
        (DroppedItemModel::Sprite(sprite.clone()), false),
        (DroppedItemModel::Sprite(opaque_sprite), true),
        (DroppedItemModel::NativeSprite(sprite), true),
        (DroppedItemModel::Cube(cube), true),
        (DroppedItemModel::Block(block), true),
    ];
    for (index, (model, can_cull)) in models.iter().enumerate() {
        assert_eq!(model.can_cull_back_faces(), *can_cull);
        for samples in [1, 4] {
            for (angle, eye) in [
                Vec3::X,
                -Vec3::X,
                Vec3::Y,
                -Vec3::Y,
                Vec3::Z,
                -Vec3::Z,
                Vec3::new(1.0, 0.3, 1.0),
                Vec3::new(-1.0, 0.3, -1.0),
                Vec3::new(0.3, -1.0, 0.4),
            ]
            .into_iter()
            .enumerate()
            {
                let before = raster(&gpu, model, eye.normalize() * 2.5, samples, false);
                // Multisampled passes retain rear faces for their partial edge coverage.
                let after = raster(&gpu, model, eye.normalize() * 2.5, samples, samples == 1);
                assert!(
                    before.as_chunks::<4>().0.iter().any(|p| p != &before[..4]),
                    "empty fixture {index}/{angle}/{samples}"
                );
                let differences = before.iter().zip(&after).filter(|(a, b)| a != b).count();
                assert_eq!(
                    differences, 0,
                    "model {index}, angle {angle}, samples {samples}"
                );
                if angle == 6 && samples == 1 && matches!(model, DroppedItemModel::Cube(_)) {
                    gpu_snapshot::save("dropped-item-culling-before", &before);
                    gpu_snapshot::save("dropped-item-culling-after", &after);
                }
            }
        }
    }
}

#[test]
fn camera_inside_and_near_plane_clipped_cube_keeps_visible_rear_pixels() {
    let Some(gpu) = Gpu::for_fixture("dropped item camera intersection pixels") else {
        return;
    };
    let model = DroppedItemModel::Cube(render_model::DroppedItemCube {
        tile: 1,
        faces: std::array::from_fn(|_| Arc::from([80, 180, 40, 255])),
        tints: [render_model::OPAQUE_WHITE; 6],
    });
    let near = render_api::CAMERA_NEAR_PLANE_BLOCKS;
    for axis in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
        for distance in [0.25, 0.5, 0.5 + near * 0.8] {
            let eye = axis * distance;
            let before = raster(&gpu, &model, eye, 1, false);
            let after = raster(&gpu, &model, eye, 1, true);
            assert!(
                before
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[1] > pixel[2]),
                "rear surface was not visible at {eye:?}"
            );
            assert_eq!(
                before, after,
                "camera or near-plane intersection at {eye:?}"
            );
        }
    }
}
