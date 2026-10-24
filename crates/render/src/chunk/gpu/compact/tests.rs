use super::*;
use crate::gpu_snapshot::{Draw, Gpu};

#[test]
fn four_storage_buffers_render_every_packed_table_and_later_updates() {
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "compact terrain tables",
        wgpu::Features::empty(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: 4,
            ..Default::default()
        },
    ) else {
        return;
    };
    let device = RenderDevice::from(gpu.device.clone());
    let queue = RenderQueue::new(gpu.queue.clone());
    let sources: Vec<_> = (1..=TRANSPARENT_REFS_BINDING)
        .filter(|binding| matches!(binding, 1 | 2 | 3 | 7 | 8 | 9 | 10 | 12 | 13 | 14))
        .map(|binding| {
            let words = match binding {
                2 => vec![2, 0, 0, 0, 0, 0, 0, 0],
                3 => vec![3, 0, 0, 0, 0, 0],
                8 => {
                    let mut words = vec![0; size_of::<BiomeTintGpu>() / 4];
                    words[0] = 8;
                    words
                }
                9 => vec![9, 0, 0, 0],
                14 => vec![14, 0],
                _ => vec![binding],
            };
            (
                binding,
                gpu.words(
                    &words,
                    BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                ),
            )
        })
        .collect();
    let mut entries: Vec<_> = sources
        .iter()
        .map(|(binding, buffer)| BindGroupEntry {
            binding: *binding,
            resource: buffer.as_entire_binding(),
        })
        .collect();
    let mut compact = CompactTerrain::default();
    assert!(compact.prepare(&device, &queue, &mut entries));
    entries
        .iter_mut()
        .find(|entry| entry.binding == 2)
        .unwrap()
        .resource = compact.binding();
    assert_eq!(
        entries.len(),
        crate::minimum_vertex_storage_buffers() as usize
    );
    let source = crate::shader_source::composed(
        r#"
import render::chunk_bindings::{chunk_origins, animations, animation_frames, model_templates, transparent_refs, cube_quads, geometry_streams};
import render::material::materials;
import render::biome_tint::{tint_grass, biome_records};
struct Output { @builtin(position) position: vec4<f32>, @location(0) value: f32 }
@vertex fn vertex(@builtin(vertex_index) index: u32) -> Output {
    var out: Output;
    out.position = vec4(vec2(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - vec2(1.0), 0.5, 1.0);
    let total = u32(chunk_origins(0u).value.x) + materials(0u).texture + tint_grass(0u) + animations(0u).frame_start + animation_frames(0u) + model_templates(0u) + transparent_refs(0u).liquid_record_index + cube_quads[0] + geometry_streams[0] + biome_records[0];
    out.value = f32(total) / 255.0;
    return out;
}
@fragment fn fragment(in: Output) -> @location(0) vec4<f32> { return vec4(in.value, 0.0, 0.0, 1.0); }
"#,
        &["PACKED_TERRAIN"],
    );
    // Preparation must not hold a reference across the next graph update.
    drop(entries);
    for (origin, expected) in [(2u32, 79u8), (22u32, 99u8)] {
        gpu.queue.write_buffer(
            &sources.iter().find(|(binding, _)| *binding == 2).unwrap().1,
            0,
            bytemuck::bytes_of(&origin),
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        compact.encode(&mut encoder);
        gpu.queue.submit([encoder.finish()]);
        let entries: Vec<_> = sources
            .iter()
            .filter(|(binding, _)| matches!(binding, 1 | 2 | 7 | 13))
            .map(|(binding, buffer)| BindGroupEntry {
                binding: *binding,
                resource: if *binding == 2 {
                    compact.binding()
                } else {
                    buffer.as_entire_binding()
                },
            })
            .collect();
        let pixels = gpu.render(
            &source,
            "vertex",
            &[Draw {
                fragment: "fragment",
                vertices: 0..3,
                bindings: &entries,
                blend: None,
                write_depth: true,
            }],
        );
        assert_eq!(
            &pixels[pixels.len() / 2..pixels.len() / 2 + 4],
            &[expected, 0, 0, 255]
        );
    }
}
