//! Terrain blends biome tints per fragment, so the blender receives interpolated positions
//! that leave the record's sub-chunk: at face edges, and in derivative helper lanes that
//! extrapolate past the horizon. Those must tint like the block that owns the face and never
//! address words outside the record.
use crate::shader_source;

use std::{
    future::Future,
    task::{Context, Poll, Waker},
};
use wgpu::util::DeviceExt;
use world::{DecodedBiomeColumn, RawBiomeIds};

/// Unrelated arena words around the record; a lookup that leaves it reads these.
const FOREIGN_WORD: u32 = 64;
const FOREIGN_WORDS: usize = 1_024;

/// Positions outside the sub-chunk, each with the in-chunk position of its nearest block.
const OUTSIDE: [([f32; 3], [f32; 3]); 5] = [
    ([-1.0e9, 4.5, 8.5], [0.5, 4.5, 8.5]),
    ([1.0e9, 4.5, 8.5], [15.5, 4.5, 8.5]),
    ([8.5, -1.0e9, 8.5], [8.5, 0.5, 8.5]),
    ([8.5, 4.5, 1.0e9], [8.5, 4.5, 15.5]),
    ([20.5, 20.5, -4.5], [15.5, 15.5, 0.5]),
];

/// Polls the adapter/device futures without adding another executor dependency.
fn finish<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}

/// A centre-only record: absent neighbours sample biome zero, so the lattice path runs.
fn edge_record() -> meshing::PackedBiomeRecord {
    // Uniform network palette holding biome 1 (zig-zag encoded as 2).
    let storage = DecodedBiomeColumn::decode(0, 1, &[1, 2], &RawBiomeIds { default_biome: 0 })
        .storage(0)
        .expect("one uniform biome storage");
    let record = meshing::PackedBiomeRecord::from_storage(&storage, |id| id);
    assert_eq!(
        record.uniform_tint_index(),
        None,
        "fixture must blend the lattice"
    );
    record
}

/// Matches the packed tint header followed by the production seasonal palette cells.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TintRow {
    packed: [u32; 8],
    seasonal_foliage: [[f32; 4]; assets::SEASONAL_FOLIAGE_COUNT],
}

/// Grass tint rows for tint indices 0 (red) and 1 (green), laid out as `BiomeTintGpu`.
fn tint_table() -> [TintRow; 2] {
    [0x3ff, 0x3ff << 10].map(|grass| TintRow {
        packed: [grass, 0, 0, 0, 0, 0, 0, 0],
        seasonal_foliage: [[0.0; 4]; assets::SEASONAL_FOLIAGE_COUNT],
    })
}

#[test]
fn positions_outside_the_sub_chunk_tint_as_their_nearest_block() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        // FXC's unoptimized debug shader exceeds its temporary-register limit for the
        // generated biome table. Exercise optimized shaders, as release builds do,
        // while retaining backend validation and every GPU bounds assertion.
        flags: wgpu::InstanceFlags::debugging() & !wgpu::InstanceFlags::DEBUG,
        ..Default::default()
    });
    let adapter = match finish(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) {
        Ok(adapter) => adapter,
        Err(error @ wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("skipping biome tint bounds: missing native GPU adapter fixture ({error})");
            return;
        }
        Err(error) => panic!("biome tint bounds: GPU fixture adapter request failed: {error}"),
    };
    if adapter.get_info().backend == wgpu::Backend::Noop {
        eprintln!("skipping biome tint bounds: missing native GPU adapter fixture (Noop adapter)");
        return;
    }
    eprintln!("biome tint bounds GPU fixture: {:?}", adapter.get_info());
    let (device, queue) =
        finish(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let source = shader_source::composed(
        &format!(
            "#import cinnabar::biome_tint::blended_biome_tint
@group(0) @binding(0) var<storage, read> queries: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> colours: array<vec4<f32>>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    colours[id.x] = blended_biome_tint(0x10u, 0u, {FOREIGN_WORDS}u, queries[id.x].xyz, vec3(0.0));
}}"
        ),
        &[],
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("biome tint bounds"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let mut words = vec![FOREIGN_WORD; FOREIGN_WORDS];
    words.extend_from_slice(edge_record().words());
    words.extend(std::iter::repeat_n(FOREIGN_WORD, FOREIGN_WORDS));
    let queries: Vec<[f32; 4]> = OUTSIDE
        .iter()
        .flat_map(|&(outside, inside)| [outside, inside])
        .map(|[x, y, z]| [x, y, z, 0.0])
        .collect();
    let storage = |contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents,
            usage: wgpu::BufferUsages::STORAGE | usage,
        })
    };
    let records = storage(bytemuck::cast_slice(&words), wgpu::BufferUsages::empty());
    let tints = storage(
        bytemuck::cast_slice(&tint_table()),
        wgpu::BufferUsages::empty(),
    );
    let query_tables = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("biome query tables"),
        contents: bytemuck::cast_slice(&meshing::biome_lattice::query_table_words()),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let inputs = storage(bytemuck::cast_slice(&queries), wgpu::BufferUsages::empty());
    let result_bytes = (queries.len() * size_of::<[f32; 4]>()) as u64;
    let outputs = storage(
        &vec![0_u8; result_bytes as usize],
        wgpu::BufferUsages::COPY_SRC,
    );
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            (0, &inputs),
            (1, &outputs),
            (7, &records),
            (8, &tints),
            (
                crate::material_shader::BIOME_QUERY_TABLES_BINDING,
                &query_tables,
            ),
        ]
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding,
            resource: buffer.as_entire_binding(),
        }),
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(queries.len() as u32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&outputs, 0, &readback, 0, result_bytes);
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let colours: Vec<[f32; 4]> =
        bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();

    for (pair, &(outside, inside)) in colours.chunks_exact(2).zip(&OUTSIDE) {
        assert!(
            pair[1][1] > 0.0,
            "{inside:?} must blend the record's own biome: {:?}",
            pair[1]
        );
        assert_eq!(
            pair[0], pair[1],
            "{outside:?} must tint like the block at {inside:?}"
        );
    }
}
