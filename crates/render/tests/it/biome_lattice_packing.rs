//! Pixel inputs from the compact lattice must match the original full-width record exactly.

use std::sync::Arc;

use meshing::{PackedBiomeRecord, biome_lattice::*};
use wgpu::util::DeviceExt;
use world::{BiomeStorage, DecodedBiomeColumn, RawBiomeIds};

use crate::{gpu_snapshot::Gpu, shader_source};

/// Encodes a one-bit biome palette in the same X-Z-Y order used by terrain bench fixtures.
fn storage(kind: usize) -> Arc<BiomeStorage> {
    let mut payload = vec![3];
    for word in 0..128 {
        let packed = (0..32).fold(0_u32, |bits, bit| {
            let index = word * 32 + bit;
            let (x, y, z) = (index >> 8, index & 15, index >> 4 & 15);
            let second = match kind {
                0 => false,
                1 => true,
                2 => y >= 4 + (x * 3 + z * 5) % 8,
                3 => (x * 7 + y * 3 + z * 5) % 23 < 2,
                _ => (x / 5 + z / 7) % 2 == 1,
            };
            bits | (u32::from(second) << bit)
        });
        payload.extend_from_slice(&packed.to_le_bytes());
    }
    payload.extend([4, 2, 4]); // Two entries, biome IDs 1 and 2.
    DecodedBiomeColumn::decode(0, 1, &payload, &RawBiomeIds { default_biome: 0 })
        .storage(0)
        .unwrap()
}

/// Reconstructs the former nine-word points by counting source samples independently.
fn original_words(record: &PackedBiomeRecord) -> Vec<u32> {
    let wide_magic = PackedBiomeRecord::from_storage(&storage(0), |_| u32::MAX).words()[0];
    let mut words = record.words()[..DESCRIPTOR_WORDS].to_vec();
    words[0] = wide_magic;
    if record.uniform_tint_index().is_some() {
        words.extend_from_slice(&record.words()[DESCRIPTOR_WORDS..]);
        return words;
    }
    let old_payload = DESCRIPTOR_WORDS + LATTICE_WORDS;
    let new_payload = DESCRIPTOR_WORDS + WIDE_LATTICE_WORDS;
    for relative in &mut words[2..DESCRIPTOR_WORDS] {
        if *relative != 0 {
            *relative += (new_payload - old_payload) as u32;
        }
    }
    words.resize(new_payload, 0);
    for x in 0..LATTICE_SIDE {
        for y in 0..LATTICE_SIDE {
            for z in 0..LATTICE_SIDE {
                let position = [x, y, z].map(|v| (v as i32 - 1) * BIOME_LATTICE_STEP);
                let mut counts: Vec<(u32, u32)> = Vec::new();
                for dz in -1..=1 {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let tint = record
                                .tint_index_at([
                                    position[0] + dx * BIOME_LATTICE_STEP,
                                    position[1] + dy * BIOME_LATTICE_STEP,
                                    position[2] + dz * BIOME_LATTICE_STEP,
                                ])
                                .unwrap();
                            if let Some((_, count)) =
                                counts.iter_mut().find(|(value, _)| *value == tint)
                            {
                                *count += 1;
                            } else {
                                counts.push((tint, 1));
                            }
                        }
                    }
                }
                counts.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
                let point = DESCRIPTOR_WORDS + lattice_index(position) * WIDE_LATTICE_POINT_WORDS;
                words[point] = counts.len().min(LATTICE_BIOME_LIMIT) as u32;
                for (index, &(tint, count)) in counts.iter().take(LATTICE_BIOME_LIMIT).enumerate() {
                    words[point + 1 + index] = tint;
                    words[point + 1 + LATTICE_BIOME_LIMIT + index] =
                        (count as f32 / LATTICE_STENCIL_SAMPLES as f32).to_bits();
                }
            }
        }
    }
    words.extend_from_slice(&record.words()[old_payload..]);
    words
}

/// Matches the production tint-buffer layout, with distinct values in every domain.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Tint {
    words: [u32; 8],
    seasonal: [[f32; 4]; assets::seasonal_foliage::SEASONAL_FOLIAGE_COUNT],
}

/// Three distinct tint rows exercise RGB10, RGB8, alpha, and seasonal foliage.
fn tints() -> [Tint; 3] {
    std::array::from_fn(|i| Tint {
        words: [
            (170 + i as u32 * 231)
                | ((813 - i as u32 * 193) << 10)
                | ((191 + i as u32 * 137) << 20),
            0x19e8_4a81 ^ (i as u32 * 0x0071_2118),
            0x2191_8753 ^ (i as u32 * 0x0031_5022),
            0x3491_8042 ^ (i as u32 * 0x0009_2011),
            0x1859_517a ^ (i as u32 * 0x0071_1323),
            0x0089_4722 ^ (i as u32 * 0x0011_1851),
            assets::BIOME_TINT_FLAG_SEASONAL_FOLIAGE,
            (0.3 + i as f32 * 0.2).to_bits(),
        ],
        seasonal: std::array::from_fn(|cell| {
            [0.2 + i as f32 * 0.3, 0.8 - cell as f32 * 0.01, 0.4, 1.0]
        }),
    })
}

#[test]
fn packed_lattice_tints_are_byte_identical_on_bench_terrains_and_biome_seams() {
    let Some(gpu) = Gpu::for_fixture("biome lattice packing oracle") else {
        return;
    };
    let fixtures: Vec<_> = (0..5)
        .map(|kind| {
            let center = storage(kind);
            let halo = std::array::from_fn(|slot| {
                if kind == 4 && slot % 5 == 0 {
                    None
                } else {
                    Some(if slot % 3 == 2 {
                        storage(1)
                    } else {
                        center.clone()
                    })
                }
            });
            PackedBiomeRecord::from_neighbourhood(&halo, |id| id)
        })
        .chain([PackedBiomeRecord::fallback()])
        .collect();
    let domains = [
        (0x10_u32, 0),
        (0x20, 0),
        (0x20, assets::MATERIAL_FLAG_BIRCH_FOLIAGE),
        (0x20, assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE),
        (0x20, assets::MATERIAL_FLAG_DRY_FOLIAGE),
        (0x30, 0),
        (0x20, assets::MATERIAL_FLAG_SEASONAL_FOLIAGE),
        (
            0x20,
            assets::MATERIAL_FLAG_SEASONAL_FOLIAGE | assets::MATERIAL_FLAG_EXPOSED_FOLIAGE,
        ),
    ];
    let mut records = Vec::new();
    let mut queries = Vec::new();
    for fixture in &fixtures {
        let compact = records.len() as u32;
        records.extend_from_slice(fixture.words());
        let original = records.len() as u32;
        records.extend(original_words(fixture));
        for &(kind, flags) in &domains {
            for cell in 0..4096_u32 {
                queries.push([compact, original, cell, kind]);
                queries.push([flags, 0, 0, 0]);
            }
        }
    }
    let source = shader_source::composed("import render::biome_tint::blended_biome_tint;
@group(0) @binding(0) var<storage, read> queries: array<vec4<u32>>;
@group(0) @binding(1) var<storage, read_write> colours: array<vec4<u32>>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= arrayLength(&queries) / 2u) { return; }
    let query = queries[id.x * 2u];
    let flags = queries[id.x * 2u + 1u].x;
    let position = vec3<f32>(vec3(query.z / 256u, query.z / 16u % 16u, query.z % 16u)) + vec3(0.5);
    colours[id.x * 2u] = bitcast<vec4<u32>>(blended_biome_tint(query.w, flags, query.x, position, vec3(-32.0, 16.0, -16.0)));
    colours[id.x * 2u + 1u] = bitcast<vec4<u32>>(blended_biome_tint(query.w, flags, query.y, position, vec3(-32.0, 16.0, -16.0)));
}", &[]);
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("biome packing oracle"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let storage_usage = wgpu::BufferUsages::STORAGE;
    let record_buffer = gpu.words(&records, storage_usage);
    let tint_buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&tints()),
            usage: storage_usage,
        });
    let query_buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&queries),
            usage: storage_usage,
        });
    let tables = gpu.words(
        &crate::material_shader::world_uniform_words(
            &[0; crate::material_shader::WORLD_CLOCK_WORDS],
        ),
        wgpu::BufferUsages::UNIFORM,
    );
    let result_bytes = (queries.len() * size_of::<[u32; 4]>()) as u64;
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_bytes,
        usage: storage_usage | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            (0, &query_buffer),
            (1, &output),
            (7, &record_buffer),
            (8, &tint_buffer),
            (crate::material_shader::BIOME_QUERY_TABLES_BINDING, &tables),
        ]
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding,
            resource: buffer.as_entire_binding(),
        }),
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((queries.len() as u32 / 2).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, result_bytes);
    gpu.queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let mapped = readback.slice(..).get_mapped_range().unwrap();
    let colours: &[[u32; 4]] = bytemuck::cast_slice(&mapped);
    assert!(
        colours
            .iter()
            .flatten()
            .all(|&bits| f32::from_bits(bits).is_finite()),
        "oracle must produce finite colours"
    );
    assert!(colours.iter().any(|colour| colour[0] != 0));
    for (query, pair) in colours.as_chunks::<2>().0.iter().enumerate() {
        assert_eq!(
            pair[0],
            pair[1],
            "biome query {query}: {:?}",
            queries[query * 2]
        );
    }
    eprintln!(
        "BIOME_PIXEL_ORACLE queries={} differing_components=0 backend={:?}",
        queries.len() / 2,
        gpu.backend
    );
}
