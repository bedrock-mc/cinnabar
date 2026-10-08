//! Production overlay vertices must stay in front of two-sided leaf depth from either view.
use crate::{gpu_snapshot::Gpu, shader_source};
use render::{
    BlockEntityScene, BlockSelectionFrame, BlockSelectionTarget, CrackInstance, SceneClock,
};

#[test]
fn bamboo_leaf_overlays_survive_front_and_reverse_depth_without_duplicate_quads() {
    let Some(gpu) = Gpu::for_fixture("bamboo two-sided overlay depth") else {
        return;
    };
    let (records, assets) = super::block_selection_native::fixture();
    let record = records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:bamboo"
                && record.canonical_state.contains("small_leaves")
        })
        .unwrap();
    let visual = assets.resolve(assets::NetworkIdMode::Sequential, record.sequential_id);
    let template = &assets.model_templates()[visual.model_template().unwrap() as usize];
    let leaf = &assets.model_quads()[template.quad_start as usize + assets::BlockFace::ALL.len()];
    assert_ne!(leaf.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
    let plane_z = f32::from(leaf.positions[0][2]) / 256.0
        + meshing::bamboo::quad_offset(
            meshing::bamboo::transform_for_template(template.flags, visual.variant(), [0; 3]),
            assets::BlockFace::ALL.len() as u32,
        )[2];
    let shape = render::crack_shape_from_template(
        &assets,
        visual.model_template().unwrap(),
        visual.variant(),
        [0; 3],
    )
    .unwrap();
    let mut selection = BlockSelectionFrame::default();
    selection.update(
        Some(&BlockSelectionTarget {
            block: [0; 3],
            bounds: [[0.0; 3], [1.0; 3]],
            shape: shape.clone(),
        }),
        false,
    );
    let atlas = assets::encode_block_entity_catalog(
        b"{}",
        16,
        16,
        &[255; 16 * 16 * 4],
        &[assets::BlockEntityPlacement {
            name: "textures/environment/destroy_stage_0".into(),
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        }],
    )
    .unwrap();
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&atlas).unwrap());
    let cracks = scene
        .update(
            SceneClock::default(),
            &[CrackInstance {
                block: [0; 3],
                stage: 0,
                shape,
            }],
            &[],
        )
        .crack
        .clone();
    let expected_vertices = template.quad_count as usize * 6;
    assert_eq!(
        selection.highlight.len(),
        expected_vertices,
        "each leaf is blended once"
    );
    assert_eq!(cracks.len(), expected_vertices, "each leaf is blended once");
    let vertices: Vec<_> = selection
        .highlight
        .iter()
        .chain(cracks.iter())
        .copied()
        .collect();
    let source = shader_source::standalone(
        include_str!("../../src/block_entity/block_entity.wgsl"),
        &[],
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(6)")
    .replace(
        "BLOCK_SELECTION_VERTICES_PER_EDGE",
        &format!("{}u", render::BLOCK_SELECTION_VERTICES_PER_EDGE),
    )
    .replace(
        "BLOCK_ENTITY_VERTEX_WORDS",
        &format!("{}u", render::BLOCK_ENTITY_VERTEX_WORDS),
    )
    .replace(
        "@vertex\nfn block_overlay_vertex(@builtin(vertex_index) vertex_index: u32)",
        "fn overlay_vertex(vertex_index: u32)",
    );
    let source = format!(
        "{source}\n@group(0) @binding(5) var<storage,read_write> results: array<vec4<f32>>;\n@compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) id: vec3<u32>) {{ results[id.x] = vec4(overlay_vertex({}u + id.x * {}u).world_position,1.0); }}",
        assets::BlockFace::ALL.len() * 6,
        expected_vertices
    );
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("witness"),
            compilation_options: Default::default(),
            cache: None,
        });
    let words = gpu.words(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
    let lightmap = gpu.buffer(
        bytemuck::cast_slice(&render::LightmapInputs::default().build()),
        wgpu::BufferUsages::UNIFORM,
    );
    let output = gpu.words(
        &[0; 8],
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 32,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    for eye_z in [-2.0, 2.0] {
        let mut view = [0.0; 104];
        view[96..99].copy_from_slice(&[0.5, 0.5, eye_z]);
        let view = gpu.buffer(&view, wgpu::BufferUsages::UNIFORM);
        let entries = [
            (0, view.as_entire_binding()),
            (1, words.as_entire_binding()),
            (5, output.as_entire_binding()),
            (6, lightmap.as_entire_binding()),
        ]
        .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(2, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 32);
        gpu.queue.submit([encoder.finish()]);
        readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        let positions: Vec<[f32; 4]> =
            bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
        readback.unmap();
        for (path, position) in ["highlight", "crack"].into_iter().zip(positions) {
            assert!(
                (position[2] - plane_z) * (eye_z - plane_z) > 0.0,
                "{path} eye_z={eye_z}: overlay must be on viewer side of leaf depth; {position:?} plane={plane_z}"
            );
        }
    }
}
