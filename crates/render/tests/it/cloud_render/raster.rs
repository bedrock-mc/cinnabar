use assets::{AtmosphereRole, AtmosphereTexture};
use bevy::math::{Mat4, Vec3};
use meshing::{
    CLOUD_CELL_BLOCKS, CLOUD_MASK_SIZE, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, CLOUD_WORLD_PERIOD,
    CloudFace,
    cloud_viewport::{CloudViewport, ViewportCloudQuad, mesh_cloud_viewport},
};

use super::gpu_snapshot::{Draw, Gpu, RasterState};

fn cube() -> Vec<ViewportCloudQuad> {
    let mut texture = AtmosphereTexture {
        role: AtmosphereRole::Clouds,
        source_path: "fixture/clouds.png".into(),
        source_bytes: 1,
        source_sha256: [1; 32],
        pixels_sha256: [2; 32],
        width: CLOUD_MASK_SIZE,
        height: CLOUD_MASK_SIZE,
        rgba8: vec![0; (CLOUD_MASK_SIZE * CLOUD_MASK_SIZE * 4) as usize].into_boxed_slice(),
    };
    texture.rgba8[..4].fill(255);
    mesh_cloud_viewport(
        &texture,
        CloudViewport::try_new([0.0; 2], 2, 1, false, true).unwrap(),
    )
    .unwrap()
}

fn draw(
    gpu: &Gpu,
    records: &[ViewportCloudQuad],
    eye: Vec3,
    cull_mode: Option<wgpu::Face>,
    repeat_at_equal_depth: bool,
) -> Vec<u8> {
    let centre = Vec3::new(
        CLOUD_CELL_BLOCKS * 0.5,
        (CLOUD_UNDERSIDE_Y + CLOUD_TOP_Y) * 0.5,
        CLOUD_CELL_BLOCKS * 0.5,
    );
    let matrix = Mat4::perspective_infinite_reverse_rh(1.2, 1.0, 0.1)
        * Mat4::look_at_rh(eye, centre, Vec3::Z);
    let view = gpu.buffer(
        &super::gpu_snapshot::view(matrix, eye),
        wgpu::BufferUsages::UNIFORM,
    );
    let frame = gpu.buffer(&[0.0; 32], wgpu::BufferUsages::UNIFORM);
    let colour = gpu.buffer(
        &[
            1.0,
            1.0,
            1.0,
            render::CLOUD_ALPHA,
            CLOUD_CELL_BLOCKS,
            CLOUD_UNDERSIDE_Y,
            CLOUD_TOP_Y,
            CLOUD_WORLD_PERIOD,
        ],
        wgpu::BufferUsages::UNIFORM,
    );
    let geometry = gpu.buffer(bytemuck::cast_slice(records), wgpu::BufferUsages::STORAGE);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: frame.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: geometry.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: colour.as_entire_binding(),
        },
    ];
    let vertices = 0..u32::try_from(records.len())
        .unwrap()
        .checked_mul(6)
        .unwrap();
    let mut draws = vec![Draw {
        fragment: "cloud_fragment",
        vertices: vertices.clone(),
        bindings: &bindings,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_depth: repeat_at_equal_depth,
    }];
    if repeat_at_equal_depth {
        draws.push(Draw {
            fragment: "cloud_fragment",
            vertices,
            bindings: &bindings,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_depth: false,
        });
    }
    gpu.render_with_state(
        &super::shader(),
        "cloud_vertex",
        &draws,
        RasterState {
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode,
                ..Default::default()
            },
            multisample: Default::default(),
            depth_compare: wgpu::CompareFunction::Greater,
            write_mask: wgpu::ColorWrites::RED | wgpu::ColorWrites::GREEN | wgpu::ColorWrites::BLUE,
        },
    )
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn production_cloud_box_culls_hidden_faces_at_each_angle_and_preserves_target_alpha() {
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    let all = cube();
    assert_eq!(all.len(), 6);
    let midpoint = CLOUD_CELL_BLOCKS * 0.5;
    for (label, eye, visible) in [
        (
            "below",
            Vec3::new(midpoint, CLOUD_UNDERSIDE_Y - 12.0, midpoint),
            vec![CloudFace::Down],
        ),
        (
            "above",
            Vec3::new(midpoint, CLOUD_TOP_Y + 12.0, midpoint),
            vec![CloudFace::Up],
        ),
        (
            "south-east",
            Vec3::new(
                CLOUD_CELL_BLOCKS * 2.5,
                CLOUD_UNDERSIDE_Y - 12.0,
                CLOUD_CELL_BLOCKS * 2.5,
            ),
            vec![CloudFace::Down, CloudFace::South, CloudFace::East],
        ),
        (
            "north-west",
            Vec3::new(
                -CLOUD_CELL_BLOCKS * 1.5,
                CLOUD_UNDERSIDE_Y - 12.0,
                -CLOUD_CELL_BLOCKS * 1.5,
            ),
            vec![CloudFace::Down, CloudFace::North, CloudFace::West],
        ),
    ] {
        let facing: Vec<_> = all
            .iter()
            .copied()
            .filter(|record| visible.iter().any(|face| record.face == *face as u32))
            .collect();
        let culled = draw(&gpu, &all, eye, Some(wgpu::Face::Back), false);
        let expected = draw(&gpu, &facing, eye, Some(wgpu::Face::Back), false);
        assert_eq!(culled, expected, "{label}: hidden face contributed pixels");
        assert!(culled.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let unculled = draw(&gpu, &all, eye, None, false);
        assert_ne!(culled, unculled, "{label}: culling was not exercised");
        // Writing a fixture surface once followed by the identical production
        // surface must not alpha-blend again at equal reverse-Z depth.
        let equal_depth = draw(&gpu, &facing, eye, Some(wgpu::Face::Back), true);
        assert_eq!(
            expected, equal_depth,
            "{label}: equal-depth draw was admitted"
        );
    }
}
