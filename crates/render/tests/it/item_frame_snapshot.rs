use bevy::math::Vec3;
use render_model::{
    BLOCK_ENTITY_VERTEX_WORDS, BlockEntityKind, BlockEntityScene, BlockEntitySubmission,
    BlockEntityVertex, BlockSelectionFrame, BlockSelectionTarget, CrackShape,
    ITEM_FRAME_PANEL_FRONT_PIXELS, ITEM_FRAME_ROTATION_STEP_DEGREES, ItemFrameModel,
    MIN_DYNAMIC_ATLAS_WIDTH, SceneClock, item_frame_item_transform,
};

use crate::{
    gpu_snapshot::{self, Draw, Gpu},
    shader_source,
};

#[derive(Clone, Copy)]
struct FrameCase {
    glow: bool,
    item: bool,
    support: bool,
    light: f32,
}

impl Default for FrameCase {
    fn default() -> Self {
        Self {
            glow: false,
            item: true,
            support: true,
            light: 1.0,
        }
    }
}

/// Creates a brown frame and an asymmetric item entirely from original diagnostic pixels.
fn artwork() -> (assets::RuntimeBlockEntityAssets, Vec<u8>) {
    let width = MIN_DYNAMIC_ATLAS_WIDTH as usize;
    let mut atlas = vec![0; width * 16 * 4];
    let mut item = vec![0; 16 * 16 * 4];
    for y in 0..16 {
        for x in 0..16 {
            let border = !(2..14).contains(&x) || !(2..14).contains(&y);
            let color = if border {
                [110, 57, 22, 255]
            } else {
                [193, 130, 57, 255]
            };
            atlas[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&color);
            let wood = if y % 4 == 0 {
                [210, 155, 65, 255]
            } else {
                [240, 200, 100, 255]
            };
            atlas[(y * width + x + 32) * 4..(y * width + x + 33) * 4].copy_from_slice(&wood);
            atlas[(y * width + x + 48) * 4..(y * width + x + 49) * 4]
                .copy_from_slice(&[24, 220, 255, 255]);
            let color = if (5..11).contains(&x) && (2..8).contains(&y) {
                [32, 255, 32, 255]
            } else if (6..10).contains(&x) && (8..14).contains(&y) {
                [255, 32, 255, 255]
            } else {
                [0; 4]
            };
            item[(y * 16 + x) * 4..(y * 16 + x + 1) * 4].copy_from_slice(&color);
            atlas[(y * width + x + 16) * 4..(y * width + x + 17) * 4].copy_from_slice(&color);
        }
    }
    let mut placements: Vec<_> = assets::block_entity_geometry::ITEM_FRAME_TEXTURES
        .into_iter()
        .zip([32, 0, 48])
        .map(|(name, x)| assets::BlockEntityPlacement {
            name: name.into(),
            x,
            y: 0,
            width: 16,
            height: 16,
        })
        .collect();
    placements.sort_by(|a, b| a.name.cmp(&b.name));
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        MIN_DYNAMIC_ATLAS_WIDTH,
        16,
        &atlas,
        &placements,
    )
    .unwrap();
    (
        assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap(),
        item,
    )
}

/// Places the production frame and sprite meshes beside the block that supports the requested face.
fn geometry(
    outward: u8,
    support_normal: Vec3,
    rotation: u8,
    case: FrameCase,
) -> (Vec<BlockEntityVertex>, Vec<u8>, [u32; 2], u32) {
    let (assets, item) = artwork();
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets);
    let frame = scene.update(
        SceneClock::default(),
        &[],
        &[BlockEntitySubmission {
            block: [0; 3],
            light: case.light.into(),
            kind: BlockEntityKind::ItemFrame(ItemFrameModel {
                glow: case.glow,
                outward,
                map: None,
            }),
        }],
    );
    let atlas = frame.atlas.as_ref().unwrap();
    let mut pixels = atlas.static_rgba8.to_vec();
    pixels.extend_from_slice(&frame.dynamic_rgba8);
    let mut vertices = frame.solid.to_vec();
    let rows = item_frame_item_transform(
        [0; 3],
        outward,
        f32::from(rotation) * ITEM_FRAME_ROTATION_STEP_DEGREES,
    );
    let transform = |point: [f32; 3], w: f32| {
        rows.map(|row| Vec3::new(row[0], row[1], row[2]).dot(Vec3::from_array(point)) + row[3] * w)
    };
    for vertex in
        render_model::extruded_sprite_vertices(16, 16, &item, [0.0, 0.0, 1.0, 1.0]).unwrap()
    {
        if !case.item {
            break;
        }
        vertices.push(BlockEntityVertex {
            position: transform(vertex.position, 1.0),
            normal: transform(vertex.normal, 0.0),
            uv: [
                (16.0 + vertex.uv[0] * 16.0) / atlas.size[0] as f32,
                vertex.uv[1] * 16.0 / atlas.size[1] as f32,
            ],
            color: [1.0; 4],
            ..Default::default()
        });
    }
    let textured_end = vertices.len() as u32;
    let mut support = BlockSelectionFrame::default();
    support.update(
        Some(&BlockSelectionTarget {
            block: (-support_normal).to_array().map(|v| v as i32),
            bounds: [
                (-support_normal).to_array(),
                (Vec3::ONE - support_normal).to_array(),
            ],
            shape: CrackShape::Cube,
        }),
        false,
    );
    vertices.extend(support.highlight.iter().map(|vertex| BlockEntityVertex {
        color: [0.58, 0.67, 0.75, 1.0],
        ..*vertex
    }));
    if !case.support {
        vertices.truncate(textured_end as usize);
    }
    (vertices, pixels, atlas.size, textured_end)
}

/// Renders the real block-entity shader with a depth-writing support block and item geometry.
fn render_face(
    gpu: &Gpu,
    source: &str,
    outward: u8,
    basis: (Vec3, Vec3),
    oblique: bool,
    rotation: u8,
    case: FrameCase,
) -> Vec<u8> {
    let (normal, up) = basis;
    let (vertices, pixels, size, textured_end) = geometry(outward, normal, rotation, case);
    let center = Vec3::splat(0.5) - normal * (ITEM_FRAME_PANEL_FRONT_PIXELS / 16.0);
    let eye = center
        + normal * 2.3
        + if oblique {
            up.cross(normal) * 1.2 + up * 0.55
        } else {
            Vec3::ZERO
        };
    let matrix = glam::camera::rh::proj::directx::perspective_infinite_reverse(0.7, 1.0, 0.1)
        * glam::camera::rh::view::look_at_mat4(eye, center, up);
    let view = gpu.buffer(
        &gpu_snapshot::view(matrix, eye),
        wgpu::BufferUsages::UNIFORM,
    );
    let mesh = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("original item frame diagnostic atlas"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        texture.size(),
    );
    let texture = texture.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let lightmap = gpu.lightmap(&[1.0; 256 * 4]);
    let atmosphere = gpu.buffer(
        &crate::gpu_snapshot::atmosphere(),
        wgpu::BufferUsages::UNIFORM,
    );
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: mesh.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(&texture),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: lightmap.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: atmosphere.as_entire_binding(),
        },
    ];
    gpu.render(
        source,
        "block_entity_vertex",
        &[
            Draw {
                fragment: "block_entity_overlay",
                vertices: textured_end..vertices.len() as u32,
                bindings: &bindings,
                blend: None,
                write_depth: true,
            },
            Draw {
                fragment: "block_entity_solid",
                vertices: 0..textured_end,
                bindings: &bindings,
                blend: None,
                write_depth: true,
            },
        ],
    )
}

/// Returns the visible green marker's count and pixel centroid.
fn green_centroid(pixels: &[u8]) -> (usize, [f32; 2]) {
    let side = gpu_snapshot::SNAPSHOT_SIDE as usize;
    let mut count = 0;
    let mut sum = [0.0; 2];
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if pixel[1] > 200 && pixel[0] < 60 && pixel[2] < 60 {
            count += 1;
            sum[0] += (index % side) as f32;
            sum[1] += (index / side) as f32;
        }
    }
    (count, sum.map(|v| v / count.max(1) as f32))
}

#[test]
fn every_mount_shows_a_centered_upright_item_in_front_of_its_support() {
    let Some(gpu) = Gpu::for_fixture("six item frame support faces") else {
        return;
    };
    eprintln!("item frame snapshot backend: {:?}", gpu.backend);
    let source = shader_source::standalone(
        include_str!("../../src/block_entity/block_entity.wesl"),
        &[],
    )
    .replace(
        "BLOCK_ENTITY_VERTEX_WORDS",
        &format!("{BLOCK_ENTITY_VERTEX_WORDS}u"),
    )
    .replace(
        "BLOCK_SELECTION_VERTICES_PER_EDGE",
        &format!("{}u", render_model::BLOCK_SELECTION_VERTICES_PER_EDGE),
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(5)")
    .replace("@group(1) @binding(1)", "@group(0) @binding(6)");
    let faces = [
        ("down", Vec3::NEG_Y, Vec3::Z),
        ("up", Vec3::Y, Vec3::NEG_Z),
        ("north", Vec3::NEG_Z, Vec3::Y),
        ("south", Vec3::Z, Vec3::Y),
        ("west", Vec3::NEG_X, Vec3::Y),
        ("east", Vec3::X, Vec3::Y),
    ];
    // Diagnostic replay may supply admission results recorded by an older client build.
    let facings: [u8; 6] = std::env::var("CINNABAR_REVIEW_FRAME_FACINGS")
        .map(|value| serde_json::from_str(&value).unwrap())
        .unwrap_or([0, 1, 2, 3, 4, 5]);
    let mut coverage = Vec::new();
    for (index, (name, normal, up)) in faces.into_iter().enumerate() {
        for oblique in [false, true] {
            let pixels = render_face(
                &gpu,
                &source,
                facings[index],
                (normal, up),
                oblique,
                0,
                FrameCase::default(),
            );
            gpu_snapshot::save(
                &format!(
                    "item-frame-{name}-{}",
                    if oblique { "side" } else { "front" }
                ),
                &pixels,
            );
            if !oblique {
                let (count, center) = green_centroid(&pixels);
                coverage.push((name, count, center));
            }
        }
    }
    let mut rotations = Vec::new();
    for index in [0, 2] {
        let (name, normal, up) = faces[index];
        for step in 0..8 {
            let pixels = render_face(
                &gpu,
                &source,
                facings[index],
                (normal, up),
                false,
                step,
                FrameCase::default(),
            );
            gpu_snapshot::save(&format!("item-frame-{name}-rotation-{step}"), &pixels);
            let (count, center) = green_centroid(&pixels);
            let midpoint = (gpu_snapshot::SNAPSHOT_SIDE as f32 - 1.0) / 2.0;
            let offset = center.map(|v| v - midpoint);
            let angle = (f32::from(step) * ITEM_FRAME_ROTATION_STEP_DEGREES).to_radians();
            let forward = offset[0] * angle.sin() - offset[1] * angle.cos();
            let sideways = offset[0] * angle.cos() + offset[1] * angle.sin();
            rotations.push((name, step, count, forward, sideways));
        }
    }
    let midpoint = (gpu_snapshot::SNAPSHOT_SIDE as f32 - 1.0) / 2.0;
    for (name, count, [x, y]) in coverage {
        assert!(
            count > 100,
            "{name}: item must remain visible in front of its support ({count})"
        );
        assert!((x - midpoint).abs() < 2.0, "{name}: centered item ({x})");
        assert!(
            y < midpoint - 2.5,
            "{name}: item's green top must remain upright ({y})"
        );
    }
    for (name, step, count, forward, sideways) in rotations {
        assert!(
            count > 100,
            "{name} rotation {step}: item must stay visible ({count})"
        );
        assert!(
            forward > 8.0 && sideways.abs() < 2.0,
            "{name} rotation {step}: item must turn clockwise about its center ({forward}, {sideways})"
        );
    }
}

#[test]
fn frame_artwork_has_a_wooden_rim_and_a_distinct_backing() {
    let Some(gpu) = Gpu::for_fixture("item frame artwork") else {
        return;
    };
    eprintln!("frame artwork backend: {:?}", gpu.backend);
    let source = shader_source::standalone(
        include_str!("../../src/block_entity/block_entity.wesl"),
        &[],
    )
    .replace(
        "BLOCK_ENTITY_VERTEX_WORDS",
        &format!("{BLOCK_ENTITY_VERTEX_WORDS}u"),
    )
    .replace(
        "BLOCK_SELECTION_VERTICES_PER_EDGE",
        &format!("{}u", render_model::BLOCK_SELECTION_VERTICES_PER_EDGE),
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(5)")
    .replace("@group(1) @binding(1)", "@group(0) @binding(6)");
    let faces = [
        ("down", Vec3::NEG_Y, Vec3::Z),
        ("up", Vec3::Y, Vec3::NEG_Z),
        ("north", Vec3::NEG_Z, Vec3::Y),
        ("south", Vec3::Z, Vec3::Y),
        ("west", Vec3::NEG_X, Vec3::Y),
        ("east", Vec3::X, Vec3::Y),
    ];
    let mut rim_counts = Vec::new();
    for glow in [false, true] {
        let case = FrameCase {
            glow,
            item: false,
            support: false,
            light: 1.0,
        };
        let name = if glow { "glow" } else { "normal" };
        for (outward, (face, normal, up)) in faces.into_iter().enumerate() {
            for oblique in [false, true] {
                let pixels =
                    render_face(&gpu, &source, outward as u8, (normal, up), oblique, 0, case);
                gpu_snapshot::save(
                    &format!(
                        "frame-artwork-{name}-{face}-{}",
                        if oblique { "side" } else { "front" }
                    ),
                    &pixels,
                );
                if !oblique && outward == 2 {
                    let count = pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .filter(|p| p[0] > 160 && p[1] > 100 && p[2] < 100)
                        .count();
                    rim_counts.push((name, count));
                }
            }
        }
        let pixels = render_face(&gpu, &source, 2, (Vec3::Z, Vec3::Y), true, 0, case);
        gpu_snapshot::save(&format!("frame-artwork-{name}-back"), &pixels);
        let dark = FrameCase { light: 0.1, ..case };
        let pixels = render_face(&gpu, &source, 2, (Vec3::NEG_Z, Vec3::Y), false, 0, dark);
        gpu_snapshot::save(&format!("frame-artwork-{name}-dark"), &pixels);
        let center = gpu_snapshot::SNAPSHOT_SIDE as usize / 2;
        let pixel = &pixels[(center * gpu_snapshot::SNAPSHOT_SIDE as usize + center) * 4..][..4];
        if glow {
            assert!(
                pixel[1] > 150 && pixel[2] > 180,
                "glow backing retains light in darkness: {pixel:?}"
            );
        } else {
            assert!(
                pixel[0] < 50,
                "ordinary backing retains world light: {pixel:?}"
            );
        }
    }
    for (name, count) in rim_counts {
        assert!(
            count > 100,
            "{name}: distinct wooden rim must be visible ({count})"
        );
    }
}
