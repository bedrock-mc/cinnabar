//! Faint untextured death rays through the production vertex and fragment entry points.
use crate::{gpu_snapshot, shader_safety, shader_source};

use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use render::BlockEntityVertex;

fn source() -> String {
    let shader = shader_safety::from_block_entity_wesl(
        include_str!("../../src/block_entity/block_entity.wesl"),
        "dragon-death-rays.wesl",
        render::BLOCK_ENTITY_VERTEX_WORDS,
        render::BLOCK_SELECTION_VERTICES_PER_EDGE,
    );
    let bevy::shader::Source::Wesl(source) = shader.source else {
        unreachable!()
    };
    shader_source::standalone(&source, &[])
        .replace("@group(1) @binding(0)", "@group(0) @binding(4)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(5)")
}

fn triangle(depth: f32, color: [f32; 4]) -> [BlockEntityVertex; 3] {
    [[-1.0, -1.0, depth], [3.0, -1.0, depth], [-1.0, 3.0, depth]].map(|position| {
        BlockEntityVertex {
            position,
            color,
            normal: Vec3::Y.to_array(),
            ..Default::default()
        }
    })
}

fn raster(gpu: &Gpu, colors: &[[f32; 4]]) -> Vec<u8> {
    let vertices = colors
        .iter()
        .enumerate()
        .flat_map(|(index, &color)| triangle(if index == 0 { 0.8 } else { 0.2 }, color))
        .collect::<Vec<_>>();
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let vertices = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
    let lightmap = gpu.buffer(&[1.0; 256 * 4], wgpu::BufferUsages::UNIFORM);
    let mut atmosphere = [0.0; 32];
    atmosphere[19] = 999.0;
    atmosphere[20] = 1000.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: vertices.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: lightmap.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: atmosphere.as_entire_binding(),
        },
    ];
    let draws = (0..colors.len())
        .map(|index| Draw {
            fragment: "block_entity_additive",
            vertices: index as u32 * 3..index as u32 * 3 + 3,
            bindings: &bindings,
            blend: Some(render::DRAGON_DEATH_BLEND),
            write_depth: false,
        })
        .collect::<Vec<_>>();
    gpu.render(&source(), "block_entity_vertex", &draws)
}

#[test]
fn fading_rays_keep_sub_cutoff_alpha_and_add_without_writing_depth() {
    let Some(gpu) =
        Gpu::for_fixture("fading_rays_keep_sub_cutoff_alpha_and_add_without_writing_depth")
    else {
        return;
    };
    let sample =
        (SNAPSHOT_SIDE as usize / 2 * SNAPSHOT_SIDE as usize + SNAPSHOT_SIDE as usize / 2) * 4;
    let transparent = raster(&gpu, &[[1.0, 0.0, 1.0, 0.0]]);
    let faint = raster(&gpu, &[[1.0, 1.0, 1.0, 0.01]]);
    for channel in 0..3 {
        assert!(
            faint[sample + channel] >= transparent[sample + channel] + 2,
            "fading alpha must contribute below the ordinary overlay cutoff"
        );
    }
    let behind = raster(&gpu, &[[1.0, 1.0, 1.0, 0.01], [1.0, 0.0, 0.0, 0.05]]);
    assert!(
        behind[sample] >= faint[sample] + 11,
        "a front ray cannot occlude an additive ray behind it"
    );
    assert_eq!(behind[sample + 1], faint[sample + 1]);
    assert_eq!(behind[sample + 2], faint[sample + 2]);
    gpu_snapshot::save("dragon-death-ray-faint", &faint);
    gpu_snapshot::save("dragon-death-ray-additive", &behind);
}
