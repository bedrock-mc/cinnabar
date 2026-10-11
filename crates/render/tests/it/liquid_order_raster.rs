//! Flat water blends to the same pixels in any face order, drawn from its records or refs.
use crate::chunk_constants;
use crate::gpu_snapshot;
use crate::shader_source;

use bevy::math::Vec3;
use gpu_snapshot::{Draw, Gpu, RasterState};
use meshing::liquid::LIQUID_TOP_INSET_BIT;
use meshing::{Face, PackedLiquidQuad};

/// One liquid face at chunk-local `origin`, with every exposed top lowered as in a lake.
fn face(origin: [u8; 3], face: Face, heights: [u8; 4]) -> [u32; 4] {
    let mut words = PackedLiquidQuad::try_pack(origin, face, heights, 0, 0, [0; 2], false)
        .unwrap()
        .words();
    words[2] |= LIQUID_TOP_INSET_BIT;
    words
}

/// A level 4x4 water surface: one plane, one facing, one face per cell.
fn surface() -> Vec<[u32; 4]> {
    (0..4)
        .flat_map(|z| (0..4).map(move |x| face([x, 0, z], Face::PositiveY, [230; 4])))
        .collect()
}

/// Renders production liquid selection and blending; base_vertex zero uses sorted refs.
/// A direct draw passes (metadata index + 1) * 4 as base_vertex.
fn render(
    gpu: &Gpu,
    source: &str,
    records: &[[u32; 4]],
    refs: &[[u32; 2]],
    base_vertex: u32,
) -> Vec<u8> {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("white water texel"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
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
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&Default::default());
    let frame = render::AtmosphereFrame::default();
    let atmosphere = gpu.buffer(
        bytemuck::cast_slice(std::slice::from_ref(&frame)),
        wgpu::BufferUsages::UNIFORM,
    );
    let table = render::LightmapInputs::default()
        .build()
        .map(|_| [1.0_f32; 4]);
    let lightmap = gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM);
    let eye = Vec3::new(2.0, 3.0, -4.0);
    let projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_3,
        1.0,
        0.1,
    );
    let matrix =
        projection * glam::camera::rh::view::look_at_mat4(eye, Vec3::new(2.0, 0.5, 2.0), Vec3::Y);
    let view = gpu.buffer(
        &gpu_snapshot::view(matrix, eye),
        wgpu::BufferUsages::UNIFORM,
    );
    // Bindings cannot be empty, so unread buffers hold one zeroed entry.
    let non_empty = |words: Vec<u32>, entry: usize| {
        if words.is_empty() {
            vec![0; entry]
        } else {
            words
        }
    };
    let streams = gpu.words(&non_empty(records.concat(), 4), wgpu::BufferUsages::STORAGE);
    // The water's chunk, at metadata index 1, sits at the world origin. Index 0 lies out
    // of view, so a draw that misreads its metadata index misses the surface.
    let origins = gpu.words(
        &[160, 0, 160, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        wgpu::BufferUsages::STORAGE,
    );
    let refs_buffer = gpu.words(&non_empty(refs.concat(), 2), wgpu::BufferUsages::STORAGE);
    let draw = gpu.words(&[base_vertex, 0, 0, 0], wgpu::BufferUsages::UNIFORM);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: origins.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 13,
            resource: streams.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 14,
            resource: refs_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 15,
            resource: atmosphere.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
            resource: wgpu::BindingResource::TextureView(&atlas),
        },
        wgpu::BindGroupEntry {
            binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
            resource: wgpu::BindingResource::TextureView(&atlas),
        },
        wgpu::BindGroupEntry {
            binding: 20,
            resource: lightmap.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 24,
            resource: draw.as_entire_binding(),
        },
    ];
    let quads = records.len() as u32;
    gpu.render_with_state(
        source,
        "order_witness",
        &[Draw {
            fragment: "fragment",
            vertices: 0..quads * chunk_constants::STATIC_QUAD_INDICES.len() as u32,
            bindings: &bindings,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_depth: true,
        }],
        RasterState {
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                ..Default::default()
            },
            write_mask: wgpu::ColorWrites::RED | wgpu::ColorWrites::GREEN | wgpu::ColorWrites::BLUE,
            ..Default::default()
        },
    )
}

/// Composes the production liquid shader with the indexed order-witness entry point.
fn witness_source() -> String {
    let indices = chunk_constants::STATIC_QUAD_INDICES;
    let index_source = format!(
        "const RASTER_INDICES: array<u32, {}> = array({});",
        indices.len(),
        indices.map(|index| format!("{index}u")).join(", ")
    );
    format!(
        "{}\n{index_source}\n{WITNESS}",
        shader_source::standalone(
            include_str!("../../src/liquid.wesl"),
            &["NATIVE_GAMMA_BLEND"]
        )
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
}

/// The water chunk's metadata index; a direct draw passes it plus one, times four, as its
/// base vertex.
const METADATA_INDEX: u32 = 1;
const DIRECT_BASE_VERTEX: u32 = (METADATA_INDEX + 1) * 4;

/// Refs to `records` in `order`, all in the water chunk.
fn refs(order: impl IntoIterator<Item = usize>) -> Vec<[u32; 2]> {
    order
        .into_iter()
        .map(|record| [record as u32, METADATA_INDEX])
        .collect()
}

#[test]
fn flat_water_draws_the_same_pixels_from_records_as_from_any_sorted_order() {
    let Some(gpu) = Gpu::for_fixture("liquid_order_raster") else {
        return;
    };
    let source = witness_source();
    let records = surface();
    let count = records.len();
    let direct = render(&gpu, &source, &records, &[], DIRECT_BASE_VERTEX);
    let background = render(&gpu, &source, &[], &[], 0);
    assert!(direct != background, "the surface drew nothing");
    gpu_snapshot::save("liquid_order_flat", &direct);
    for order in [
        refs(0..count),
        refs((0..count).rev()),
        refs((0..count).map(|index| (index * 5) % count)),
    ] {
        let sorted = render(&gpu, &source, &records, &order, 0);
        assert!(
            direct == sorted,
            "a flat surface's pixels depend on its face order"
        );
    }

    // A wall behind the surface's front rows overlaps it on screen, so order matters there:
    // the same comparison sees the difference, which is why such water stays sorted.
    let mut shore = records.clone();
    shore.push(face([1, 0, 2], Face::NegativeZ, [0, 230, 230, 0]));
    let wall_last = render(&gpu, &source, &shore, &refs(0..shore.len()), 0);
    let wall_first = render(
        &gpu,
        &source,
        &shore,
        &refs(std::iter::once(count).chain(0..count)),
        0,
    );
    assert!(
        wall_last != wall_first,
        "the witness cannot see blend order"
    );
}

// Reaches production liquid geometry with indexed-draw vertex and instance selection.
// Each record gets a distinct color so pixel comparisons expose blend-order changes.
const WITNESS: &str = r#"
@group(0) @binding(24) var<uniform> raster_draw: vec4<u32>;
/// Draws production liquid geometry with per-record colors that expose blend order.
@vertex fn order_witness(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = RASTER_INDICES[index % 6u];
    let draw_ref = liquid_draw_ref(raster_draw.x + corner, index / 6u);
    let word = draw_ref.liquid_record_index * 4u;
    let local = liquid_corner(
        geometry_streams[word],
        geometry_streams[word + 1u],
        corner,
        geometry_streams[word + 2u],
    );
    let world = vec3<f32>(chunk_origins[draw_ref.metadata_index].value.xyz) + local;
    let shade = f32(draw_ref.liquid_record_index % 7u) / 7.0;
    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
    out.world_position = world;
    out.uv = vec2(0.5);
    out.current_texture = 0u;
    out.next_texture = 0u;
    out.frame_blend = 0.0;
    out.water_tint = vec4(0.2 + 0.6 * shade, 0.9 - 0.6 * shade, 0.5, 0.5);
    out.lighting = vec3(1.0);
    out.native_light_levels = vec2(0.0);
    out.native_face_shade = 1.0;
    out.depth_write_route = 0u;
    out.two_sided = 1u;
    return out;
}
"#;
