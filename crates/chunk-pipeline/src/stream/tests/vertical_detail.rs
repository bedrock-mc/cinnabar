//! Vertical limits change mesh lifetime without changing world or light storage.

use super::light_scheduler::{install_current_light, lit_stream};
use super::*;

/// Installs independent solid sections with current light and no pending lighting work.
fn stacked_stream(heights: impl IntoIterator<Item = i32>) -> WorldStream {
    let mut stream = lit_stream(1);
    for y in heights {
        let key = SubChunkKey::new(1, 0, y, 0);
        stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    stream
}

/// Drains zero-byte changes using the same acknowledgement path as the renderer.
fn withdraw(stream: &mut WorldStream) -> Vec<SubChunkKey> {
    stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, usize::MAX);
    let mut removed = Vec::new();
    while let Some(change) = stream.pop_mesh_change() {
        match change {
            WorldMeshChange::Remove {
                key,
                generation,
                dirty_since,
                ..
            } => {
                stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
                removed.push(key);
            }
            WorldMeshChange::Upsert { .. } => panic!("withdrawal must not build geometry"),
        }
    }
    removed
}

#[test]
fn unlimited_is_default_and_a_limit_preserves_blocks_and_light() {
    let mut stream = stacked_stream(-4..=8);
    let outside = SubChunkKey::new(1, 0, 8, 0);
    let blocks = stream.authority.terrain().sub_chunk(outside).unwrap();
    let light = stream.lighting.store.light(outside).unwrap().clone();
    assert!(stream.mesh_requires_geometry(outside));
    stream.set_vertical_render_distance(2);
    stream.reevaluate_vertical_detail([0.0; 3]);
    for y in -4..=8 {
        assert_eq!(
            stream.mesh_requires_geometry(SubChunkKey::new(1, 0, y, 0)),
            (-2..=2).contains(&y)
        );
    }
    assert!(Arc::ptr_eq(
        &blocks,
        &stream.authority.terrain().sub_chunk(outside).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &light,
        stream.lighting.store.light(outside).unwrap()
    ));
    assert!(withdraw(&mut stream).contains(&outside));
}

#[test]
fn height_crossing_restores_new_sections_and_withdraws_old_ones() {
    let mut stream = stacked_stream(-4..=8);
    stream.set_vertical_render_distance(1);
    stream.reevaluate_vertical_detail([0.0; 3]);
    withdraw(&mut stream);
    stream.reevaluate_vertical_detail([0.0, 64.0, 0.0]);
    for y in -4..=8 {
        let key = SubChunkKey::new(1, 0, y, 0);
        assert_eq!(stream.mesh_requires_geometry(key), (3..=5).contains(&y));
        if (3..=5).contains(&y) {
            assert!(
                stream.mesh_jobs.pending.contains_key(&key),
                "entering terrain must remesh"
            );
        }
    }
    assert!(withdraw(&mut stream).contains(&SubChunkKey::new(1, 0, 0, 0)));
}

#[test]
fn disabling_the_limit_restores_every_suppressed_section() {
    let mut stream = stacked_stream(-4..=8);
    stream.set_vertical_render_distance(1);
    stream.reevaluate_vertical_detail([0.0; 3]);
    withdraw(&mut stream);
    stream.set_vertical_render_distance(render_api::terrain_distance::VERTICAL_DISTANCE_UNLIMITED);
    stream.reevaluate_vertical_detail([0.0; 3]);
    for key in stream.resident.iter() {
        assert!(stream.mesh_requires_geometry(*key));
        assert!(stream.mesh_jobs.pending.contains_key(key));
    }
}

#[test]
fn changed_target_restores_sections_withdrawn_by_an_interrupted_scan() {
    let mut stream = stacked_stream(-4..=8);
    stream.set_vertical_render_distance(1);
    let key = SubChunkKey::new(1, 0, 8, 0);
    stream.reconcile_vertical_section(key, Instant::now());
    withdraw(&mut stream);
    stream.set_vertical_render_distance(render_api::terrain_distance::VERTICAL_DISTANCE_UNLIMITED);
    stream.reevaluate_vertical_detail([0.0; 3]);
    assert!(stream.mesh_requires_geometry(key));
    assert!(stream.mesh_jobs.pending.contains_key(&key));
}

#[test]
fn obsolete_worker_result_cannot_publish_outside_the_new_band() {
    let mut stream = stacked_stream([0]);
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let mut completion = None;
    test_time::eventually("vertical-distance mesh completion", || {
        completion = stream.mesh_rx.try_recv().ok();
        completion.is_some()
    });
    stream.set_vertical_render_distance(1);
    // Move the admission target before the resumable withdrawal reaches the section.
    stream.service_yield = Some(Arc::new(AtomicBool::new(true)));
    stream.reevaluate_vertical_detail([0.0, 128.0, 0.0]);
    stream.accept_mesh_completion(completion.unwrap());
    assert!(
        !stream
            .mesh_changes
            .iter()
            .any(|change| matches!(change, WorldMeshChange::Upsert { .. }))
    );
}

#[test]
fn negative_height_uses_floor_division_and_includes_boundary_sections() {
    let mut stream = stacked_stream(-4..=4);
    stream.set_vertical_render_distance(1);
    stream.reevaluate_vertical_detail([0.0, -0.25, 0.0]);
    for y in -4..=4 {
        assert_eq!(
            stream.mesh_requires_geometry(SubChunkKey::new(1, 0, y, 0)),
            (-2..=0).contains(&y)
        );
    }
}

#[test]
fn new_terrain_outside_the_band_never_dispatches_a_mesh_worker() {
    let mut stream = lit_stream(1);
    stream.set_vertical_render_distance(1);
    stream.reevaluate_vertical_detail([0.0; 3]);
    let key = SubChunkKey::new(1, 0, 6, 0);
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    assert!(withdraw(&mut stream).contains(&key));
    stream.reevaluate_vertical_detail([0.0, 96.0, 0.0]);
    assert!(stream.mesh_requires_geometry(key));
    assert_eq!(stream.dispatch_mesh_jobs([0.0, 96.0, 0.0], 1), 1);
}

#[test]
fn evicted_sections_can_return_with_a_different_vertical_target() {
    let mut stream = stacked_stream([6]);
    let key = SubChunkKey::new(1, 0, 6, 0);
    stream.set_vertical_render_distance(1);
    stream.reevaluate_vertical_detail([0.0; 3]);
    withdraw(&mut stream);
    stream.evict_column(key.chunk());
    stream.reevaluate_vertical_detail([0.0, 96.0, 0.0]);
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    assert!(stream.mesh_requires_geometry(key));
    assert_eq!(stream.dispatch_mesh_jobs([0.0, 96.0, 0.0], 1), 1);
}

#[test]
fn restoring_unlimited_height_reaches_every_column() {
    let mut stream = lit_stream(1);
    let keys: Vec<_> = [-1, 0, 1]
        .into_iter()
        .flat_map(|x| {
            [-1, 0, 1].into_iter().flat_map(move |z| {
                [-4, 0, 6]
                    .into_iter()
                    .map(move |y| SubChunkKey::new(1, x, y, z))
            })
        })
        .collect();
    for &key in &keys {
        stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    stream.set_vertical_render_distance(1);
    stream.reevaluate_vertical_detail([0.0; 3]);
    assert_eq!(withdraw(&mut stream).len(), 18);
    stream.set_vertical_render_distance(render_api::terrain_distance::VERTICAL_DISTANCE_UNLIMITED);
    stream.reevaluate_vertical_detail([0.0; 3]);
    for key in keys {
        assert!(stream.mesh_requires_geometry(key));
        assert!(stream.mesh_jobs.pending.contains_key(&key));
    }
}
