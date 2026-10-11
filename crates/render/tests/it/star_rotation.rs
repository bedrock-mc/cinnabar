use crate::gpu_snapshot;
use crate::shader_source;

#[test]
fn star_vertex_stage_reads_the_celestial_frame() {
    let source = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let vertex = module
        .entry_points
        .iter()
        .find(|entry| entry.name == "atmosphere_vertex")
        .unwrap();
    let reads_time = vertex.function.expressions.iter().any(|(_, expression)| {
        matches!(expression, naga::Expression::GlobalVariable(handle)
            if module.global_variables[*handle].name.as_deref() == Some("atmosphere"))
    });
    assert!(
        reads_time,
        "star projection must read celestial time rather than stay fixed in world space"
    );
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn celestial_time_moves_the_star_mesh_on_the_gpu() {
    use bevy::math::{Mat4, Vec3};
    use gpu_snapshot::{Draw, Gpu};
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    let mut source = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    source.push_str("\n@fragment fn star_probe(input: VertexOutput) -> @location(0) vec4<f32> { return vec4(vec3(input.star_alpha), 1.0); }");
    let uniform = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let stars = [
        -0.44, 0.16, 0.56, 1.0, -0.36, 0.16, 0.64, 1.0, -0.36, 0.24, 0.64, 1.0, -0.44, 0.16, 0.56,
        1.0, -0.36, 0.24, 0.64, 1.0, -0.44, 0.24, 0.56, 1.0,
    ];
    let stars = gpu.buffer(&stars, wgpu::BufferUsages::STORAGE);
    let atmosphere = gpu.buffer(
        &[0.0; 32],
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    );
    let mut centers = Vec::new();
    for (name, time) in [("stars-before", 0.0_f32), ("stars-after", 0.25_f32)] {
        gpu.queue
            .write_buffer(&atmosphere, 29 * 4, bytemuck::bytes_of(&time));
        let bindings = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: atmosphere.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: stars.as_entire_binding(),
            },
        ];
        let pixels = gpu.render(
            &source,
            "atmosphere_vertex",
            &[Draw {
                fragment: "star_probe",
                vertices: 3..9,
                bindings: &bindings,
                blend: None,
                write_depth: false,
            }],
        );
        gpu_snapshot::save(name, &pixels);
        let lit = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, p)| p[0] == 255)
            .map(|(index, _)| [index % 256, index / 256])
            .collect::<Vec<_>>();
        assert!(!lit.is_empty(), "the star must render");
        centers.push(std::array::from_fn::<_, 2, _>(|axis| {
            lit.iter().map(|point| point[axis]).sum::<usize>() as f32 / lit.len() as f32
        }));
    }
    assert!(
        (centers[1][0] - 102.0).abs() < 2.0 && (centers[1][1] - 179.0).abs() < 2.0,
        "a quarter turn must rotate the star around native +Z: {centers:?}"
    );
}
