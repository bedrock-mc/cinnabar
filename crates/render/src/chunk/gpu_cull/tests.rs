use super::model::{
    ARGS_WORDS, CullCamera, CullPhase, CullRecord, CullStream, args_region, args_words,
    reference_args, slot_enabled,
};
use super::slots::{CullSlots, cull_record, quad_bounds};
use super::*;

const SIDE: i32 = world::SUB_CHUNK_SIDE as i32;

fn allocation(layout: CubeQuadLayout, metadata_index: u32) -> GpuChunkAllocation {
    GpuChunkAllocation {
        key: SubChunkKey::new(0, 1, 4, -1),
        generation: 1,
        tint_identity: ChunkBiomeTintIdentity::default(),
        quad_range: 100..112,
        cube_layout: layout,
        cube_lighting_range: Some(200..224),
        model_range: Some(224..232),
        model_lighting_range: Some(232..240),
        model_draw_range: Some(240..250),
        transparent_model_draw_range: None,
        liquid_range: Some(250..270),
        liquid_lighting_range: Some(270..280),
        has_depth_liquid: true,
        has_transparent_liquid: false,
        depth_liquid_range: Some(5..9),
        order_independent_liquid: false,
        metadata_index,
    }
}

#[test]
fn displaced_model_cull_record_keeps_outer_geometry_visible() {
    use bevy::{camera::primitives::Aabb, math::Vec3A};
    let mut allocation = allocation(CubeQuadLayout::default(), 0);
    allocation.key = SubChunkKey::new(0, 0, 0, 0);
    let [low, high] = cull_record(&allocation, None).bounds();
    let low = Vec3A::from_array(low.map(|value| value as f32));
    let high = Vec3A::from_array(high.map(|value| value as f32));
    let aabb = Aabb {
        center: (low + high) * 0.5,
        half_extents: (high - low) * 0.5,
    };
    let frustum = Frustum(bevy::shape::ViewFrustum::from_clip_from_world(
        &glam::camera::rh::proj::directx::orthographic(-1.4, -1.25, 0.0, 16.0, -16.0, 16.0),
    ));
    assert!(
        frustum.intersects_obb_identity(&aabb),
        "GPU culling retains component displacement beyond a model overhang"
    );
}

fn cpu_args(allocation: &GpuChunkAllocation, camera: [f64; 3]) -> [Vec<[u32; 5]>; 4] {
    let words = |draw: DrawIndexedIndirectArgs| {
        [
            draw.index_count,
            draw.instance_count,
            draw.first_index,
            draw.base_vertex as u32,
            draw.first_instance,
        ]
    };
    [
        solid_indirect_commands(allocation, Some(camera))
            .into_iter()
            .flatten()
            .map(words)
            .collect(),
        cutout_indirect_command(allocation)
            .map(words)
            .into_iter()
            .collect(),
        model_mdi_draw_command(allocation)
            .map(words)
            .into_iter()
            .collect(),
        depth_liquid_mdi_draw_command(allocation)
            .map(words)
            .into_iter()
            .collect(),
    ]
}

/// A record drawn through the cull kernels issues exactly the CPU path's indirect commands.
#[test]
fn records_reproduce_every_cpu_indirect_command_from_every_side() {
    let index_counts = [
        STATIC_QUAD_INDICES.len() as u32,
        STATIC_QUAD_INDICES.len() as u32,
        MODEL_INDEX_COUNT,
        STATIC_QUAD_INDICES.len() as u32,
    ];
    let layouts = [
        CubeQuadLayout::from_solid_counts([1, 1, 2, 2, 1, 1]),
        CubeQuadLayout::from_solid_counts([2, 0, 0, 3, 1, 0]),
        CubeQuadLayout::from_solid_counts([2; 6]),
        CubeQuadLayout::default(),
        CubeQuadLayout::from_solid_counts([3; 6]),
    ];
    let origin = chunk_origin(SubChunkKey::new(0, 1, 4, -1)).map(f64::from);
    let offsets = [-7.5, 0.0, 0.25, 8.0, 15.0, 16.0, 16.5, 40.0];
    for layout in layouts {
        let allocation = allocation(layout, 3);
        let record = cull_record(&allocation, None);
        for x in offsets {
            for y in offsets {
                for z in offsets {
                    let camera = [origin[0] + x, origin[1] + y, origin[2] + z];
                    let gpu = reference_args(
                        &[
                            CullRecord::default(),
                            CullRecord::default(),
                            CullRecord::default(),
                            record,
                        ],
                        CullCamera::new(Some(camera)),
                        index_counts,
                        |_| true,
                    );
                    assert_eq!(
                        gpu,
                        cpu_args(&allocation, camera),
                        "{layout:?} at {camera:?}"
                    );
                }
            }
        }
    }
    let mut invalid = allocation(CubeQuadLayout::default(), 3);
    invalid.cube_lighting_range = None;
    invalid.model_range = None;
    invalid.has_depth_liquid = false;
    assert!(!cull_record(&invalid, None).is_live());
}

#[test]
fn split_eye_selects_the_same_faces_as_the_f64_eye() {
    let origin = [-32, 64, 48];
    let values = [
        -40.0, -32.0, -31.999, -17.0, -16.5, -16.0, 0.0, 47.0, 48.0, 48.25, 64.0, 64.5, 80.0,
    ];
    for x in values {
        for y in values.map(|value| value + 64.0) {
            for z in values.map(|value| value + 32.0) {
                let eye = [x, y, z];
                let expected = (0..6)
                    .filter(|&face| {
                        meshing::sub_chunk_facing_faces(origin, eye).contains(Face::ALL[face])
                    })
                    .fold(0_u8, |mask, face| mask | 1 << (Face::ALL[face] as u8));
                assert_eq!(
                    CullCamera::new(Some(eye)).facing(origin),
                    expected,
                    "{eye:?}"
                );
            }
        }
    }
    assert_eq!(CullCamera::new(None).facing(origin), 0x3f);
    assert_eq!(
        CullCamera::new(Some([f64::NAN, 0.0, 0.0])).facing(origin),
        0x3f
    );
}

#[test]
fn quad_bounds_contain_the_rasterised_quad() {
    for face in Face::ALL {
        let quad = PackedQuad::new([3, 14, 0], face, 4, 2, 0);
        let [low, high] = quad_bounds(&quad);
        assert_eq!(low, [3, 14, 0]);
        // The +Y plane sits one block above the origin; extents clamp to the sub-chunk.
        assert!(high[1] >= 15 && high.iter().all(|&value| value <= SIDE));
        assert!(high[0] >= 7 && high[2] >= 4);
    }
}

#[test]
fn args_regions_are_disjoint_and_fit_the_buffer() {
    let capacity = 1024;
    let mut regions = CullPhase::ALL
        .into_iter()
        .flat_map(|phase| {
            CullStream::ALL.map(|stream| {
                let start = args_region(capacity, phase, stream);
                (
                    start,
                    start + stream.draws_per_record() * capacity * ARGS_WORDS,
                )
            })
        })
        .collect::<Vec<_>>();
    regions.sort_unstable();
    assert!(regions.windows(2).all(|pair| pair[0].1 == pair[1].0));
    assert_eq!(regions[0].0, 0);
    assert_eq!(u64::from(regions.last().unwrap().1), args_words(capacity));
}

/// Every count-capable backend, DX12 included, consumes compacted counts; the rest use cleared
/// fixed regions.
#[test]
fn indirect_devices_select_the_best_gpu_cull_submission() {
    let count = WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT | WgpuFeatures::INDIRECT_FIRST_INSTANCE;
    let fixed = WgpuFeatures::INDIRECT_FIRST_INSTANCE;
    let compute = DownlevelFlags::COMPUTE_SHADERS;
    let mdi = ChunkDrawMode::MultiDrawIndirect;
    assert_eq!(
        gpu_cull_submission(mdi, count, compute, false),
        Some(GpuCullSubmission::Count)
    );
    assert_eq!(
        gpu_cull_submission(mdi, fixed, compute, false),
        Some(GpuCullSubmission::Fixed)
    );
    assert!(gpu_cull_supported(mdi, fixed, compute, false));
    assert_eq!(
        gpu_cull_submission(mdi, WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT, compute, false),
        None
    );
    assert_eq!(gpu_cull_submission(mdi, count, compute, true), None);
    assert_eq!(
        gpu_cull_submission(ChunkDrawMode::Direct, count, compute, false),
        None
    );
    assert_eq!(
        gpu_cull_submission(mdi, count, DownlevelFlags::empty(), false),
        None
    );
}

#[test]
fn slot_table_tracks_moves_removals_cave_visibility_and_tint() {
    let entity = |index: u32| Entity::from_raw_u32(index + 1).unwrap();
    let live = cull_record(&allocation(CubeQuadLayout::default(), 0), None);
    let tint = ChunkBiomeTintIdentity::default();
    let mut hidden = HashSet::new();
    let mut table = CullSlots::default();
    table.set_tint(tint, &hidden);
    table.update(entity(0), 2, tint, live, &hidden);
    table.update(entity(1), 0, tint, live, &hidden);
    assert_eq!(table.slot_count(), 3);
    assert!(slot_enabled(table.enabled(), 2) && slot_enabled(table.enabled(), 0));
    assert_eq!(table.take_dirty(), [0, 1, 2]);

    // A replacement allocation moves the entity; the old slot is cleared and re-uploaded.
    table.update(entity(0), 1, tint, live, &hidden);
    assert!(!slot_enabled(table.enabled(), 2) && !table.records()[2].is_live());
    table.trim();
    assert_eq!(table.slot_count(), 2);
    assert_eq!(table.take_dirty(), [1]);

    hidden.insert(entity(1));
    table.refresh_entity(entity(1), &hidden);
    assert!(!slot_enabled(table.enabled(), 0));
    hidden.clear();
    table.refresh_entity(entity(1), &hidden);
    assert!(slot_enabled(table.enabled(), 0));

    let stale = ChunkBiomeTints::with_revision(Arc::from([]), 9).table_identity();
    table.set_tint(stale, &hidden);
    assert!(!slot_enabled(table.enabled(), 0) && !slot_enabled(table.enabled(), 1));

    // Regrowing past a trimmed slot re-uploads it, so no stale GPU record survives.
    table.set_tint(tint, &hidden);
    table.remove(entity(0));
    table.trim();
    table.take_dirty();
    table.update(entity(2), 3, tint, live, &hidden);
    assert_eq!(table.take_dirty(), [1, 2, 3]);
}

/// Each phase submits only the draws its enabled records can emit, never one per slot and stream.
#[test]
fn fixed_draw_counts_cover_exactly_what_enabled_records_can_emit() {
    let entity = |index: u32| Entity::from_raw_u32(index + 1).unwrap();
    let tint = ChunkBiomeTintIdentity::default();
    let every_stream = cull_record(
        &allocation(CubeQuadLayout::from_solid_counts([1, 1, 2, 2, 1, 1]), 0),
        None,
    );
    let mut solid_only = allocation(CubeQuadLayout::from_solid_counts([2, 0, 0, 3, 1, 0]), 2);
    solid_only.model_draw_range = None;
    solid_only.has_depth_liquid = false;
    let solid_only = cull_record(&solid_only, None);
    assert_eq!(every_stream.max_draws(), [3, 1, 1, 1]);
    assert_eq!(solid_only.max_draws(), [3, 1, 0, 0]);

    let mut hidden = HashSet::from([entity(3)]);
    let mut table = CullSlots::default();
    table.set_tint(tint, &hidden);
    table.update(entity(0), 0, tint, every_stream, &hidden);
    table.update(entity(2), 2, tint, solid_only, &hidden);
    table.update(entity(3), 3, tint, every_stream, &hidden);
    // Four slots used to submit [12, 4, 4, 4]; the empty slot and the cave-hidden one add nothing.
    assert_eq!(table.draw_bounds(), [6, 2, 1, 1]);

    let index_counts = [6, 6, 6, 6];
    let offsets = [-7.5, 0.0, 0.25, 8.0, 15.0, 16.5, 40.0];
    for x in offsets {
        for y in offsets {
            for z in offsets {
                let camera = CullCamera::new(Some([x, 64.0 + y, z - 16.0]));
                let args = reference_args(table.records(), camera, index_counts, |slot| {
                    slot_enabled(table.enabled(), slot)
                });
                for (stream, args) in args.iter().enumerate() {
                    assert!(args.len() as u32 <= table.draw_bounds()[stream]);
                }
            }
        }
    }

    let recount = |table: &CullSlots| {
        let mut bounds = [0; 4];
        for (slot, record) in table.records().iter().enumerate() {
            if slot_enabled(table.enabled(), slot) {
                for (bound, draws) in bounds.iter_mut().zip(record.max_draws()) {
                    *bound += draws;
                }
            }
        }
        bounds
    };
    hidden.clear();
    table.refresh_entity(entity(3), &hidden);
    assert_eq!(table.draw_bounds(), [9, 3, 2, 2]);
    table.update(entity(2), 5, tint, every_stream, &hidden);
    assert_eq!(table.draw_bounds(), [9, 3, 3, 3]);
    table.remove(entity(0));
    table.trim();
    assert_eq!(table.draw_bounds(), [6, 2, 2, 2]);
    assert_eq!(table.draw_bounds(), recount(&table));
    let stale = ChunkBiomeTints::with_revision(Arc::from([]), 9).table_identity();
    table.set_tint(stale, &hidden);
    assert_eq!(table.draw_bounds(), [0; 4]);
    table.set_tint(tint, &hidden);
    assert_eq!(table.draw_bounds(), recount(&table));
}

/// Release timing: `cargo test --release -p render --lib gpu_cull_cpu_stage_bench -- --ignored --nocapture`.
#[test]
#[ignore = "offline CPU stage timing fixture"]
fn gpu_cull_cpu_stage_bench() {
    use bevy::render::render_phase::ViewRangefinder3d;
    use std::time::Instant;

    // A 12-chunk radius: 25 x 25 columns of 24 sub-chunks.
    let mut world = World::new();
    let mut entities = Vec::new();
    for x in -12..=12 {
        for z in -12..=12 {
            for y in -4..20 {
                let index = entities.len() as u32;
                let mut allocation =
                    allocation(CubeQuadLayout::from_solid_counts([1, 1, 2, 2, 1, 1]), index);
                allocation.key = SubChunkKey::new(0, x, y, z);
                entities.push(world.spawn(allocation).id());
            }
        }
    }
    let eye = Vec3::new(8.0, 72.0, 8.0);
    let world_from_view =
        Transform::from_translation(eye).looking_to(Vec3::new(1.0, -0.2, 0.4), Vec3::Y);
    let clip_from_world =
        glam::camera::rh::proj::directx::perspective_infinite_reverse(1.2, 16.0 / 9.0, 0.05)
            * world_from_view.to_matrix().inverse();
    let frustum = bevy::camera::primitives::Frustum(
        bevy::shape::ViewFrustum::from_clip_from_world(&clip_from_world),
    );
    let mut query = world.query::<&GpuChunkAllocation>();
    query.update_archetypes(&world);
    let visible = entities
        .iter()
        .copied()
        .filter(|&entity| {
            let origin =
                chunk_origin(query.get_manual(&world, entity).unwrap().key).map(|v| v as f32 + 8.0);
            let aabb = bevy::camera::primitives::Aabb {
                center: Vec3A::from_array(origin),
                half_extents: Vec3A::splat(8.0),
            };
            frustum.intersects_obb_identity(&aabb)
        })
        .map(|entity| (entity, MainEntity::from(entity)))
        .collect::<Vec<_>>();
    let rangefinder = ViewRangefinder3d::from_world_from_view(&world_from_view.compute_affine());
    let probe = ActiveFrameProbe::default();
    let tint = ChunkBiomeTintIdentity::default();
    let median = |mut samples: Vec<f64>| {
        samples.sort_by(f64::total_cmp);
        samples[samples.len() / 2]
    };

    // Before: the CPU indirect path's queue planning and indirect rebuild, every frame.
    let mut uploaded = Vec::new();
    let (mut queue, mut prepare, mut draws) = (Vec::new(), Vec::new(), 0);
    for _ in 0..200 {
        let scope = probe.scope();
        let started = Instant::now();
        let sorted = sorted_visible_entities(visible.iter().copied())
            .into_iter()
            .filter(|(entity, _)| {
                query
                    .get_manual(&world, *entity)
                    .ok()
                    .is_some_and(|allocation| {
                        drawable_allocation_identity(&scope, *entity, allocation, tint)
                            .is_some_and(|identity| scope.record_visible(*entity, identity))
                    })
            })
            .collect::<Vec<_>>();
        let cubes = front_to_back_cube_entities(
            sorted
                .iter()
                .map(|(entity, _)| (*entity, query.get_manual(&world, *entity).unwrap().key)),
            &rangefinder,
        );
        let solids = cubes.clone();
        let models = sorted.iter().map(|(entity, _)| *entity).collect::<Vec<_>>();
        let liquids = models.clone();
        queue.push(started.elapsed().as_secs_f64() * 1e3);
        let started = Instant::now();
        let resident = |list: &[Entity]| {
            list.iter()
                .filter_map(|&entity| {
                    query
                        .get_manual(&world, entity)
                        .ok()
                        .map(|item| (entity, item))
                })
                .collect::<Vec<_>>()
        };
        let mut commands = Vec::new();
        commands.extend(
            pipeline::solid::prepare_solid_indirect_batch_draws(
                resident(&solids),
                Some(eye.as_dvec3().to_array()),
                &scope,
                tint,
            )
            .0,
        );
        commands.extend(prepare_indirect_batch_draws(resident(&cubes), &scope, tint).0);
        commands.extend(prepare_model_indirect_batch_draws(resident(&models), &scope, tint).0);
        commands
            .extend(prepare_depth_liquid_indirect_batch_draws(resident(&liquids), &scope, tint).0);
        let bytes: &[u8] = bytemuck::cast_slice(&commands);
        if uploaded != bytes {
            uploaded = bytes.to_vec();
        }
        prepare.push(started.elapsed().as_secs_f64() * 1e3);
        draws = commands.len();
    }

    // After: the record table's steady frame, and a streaming frame that replaces 64 records.
    let hidden = HashSet::new();
    let mut table = CullSlots::default();
    table.set_tint(tint, &hidden);
    for &entity in &entities {
        let allocation = query.get_manual(&world, entity).unwrap();
        table.update(
            entity,
            allocation.metadata_index,
            tint,
            cull_record(allocation, None),
            &hidden,
        );
    }
    table.take_dirty();
    let (mut steady, mut streaming) = (Vec::new(), Vec::new());
    for frame in 0..200 {
        let started = Instant::now();
        table.set_tint(tint, &hidden);
        table.trim();
        let dirty = table.take_dirty();
        std::hint::black_box((dirty, table.take_enabled_dirty()));
        steady.push(started.elapsed().as_secs_f64() * 1e3);
        let started = Instant::now();
        for &entity in entities.iter().skip(frame * 64 % entities.len()).take(64) {
            let allocation = query.get_manual(&world, entity).unwrap();
            table.update(
                entity,
                allocation.metadata_index,
                tint,
                cull_record(allocation, None),
                &hidden,
            );
        }
        table.trim();
        std::hint::black_box(table.take_dirty());
        streaming.push(started.elapsed().as_secs_f64() * 1e3);
    }
    eprintln!(
        "gpu cull cpu bench: {} resident, {} frustum-visible, {draws} indirect commands; \
         before opaque_batch_planning={:.3}ms indirect_preparation={:.3}ms; \
         after steady={:.4}ms streaming_64={:.4}ms (medians)",
        entities.len(),
        visible.len(),
        median(queue),
        median(prepare),
        median(steady),
        median(streaming),
    );
}
