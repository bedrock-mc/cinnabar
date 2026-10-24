use super::*;

/// Creates an empty section with either omitted or explicitly delivered block storage.
fn empty_section(stream: &mut WorldStream, key: SubChunkKey, stored: bool) {
    if stored {
        stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(stream.classifier.air_network_id()))
            .unwrap();
        stream.sync_resident(key);
    } else {
        stream.record_known_air(key);
    }
}

/// Delivers the render boundary's acknowledgement for a dispatched empty publication.
fn acknowledge_removal(stream: &mut WorldStream, change: WorldMeshChange) {
    let WorldMeshChange::Remove {
        key,
        generation,
        dirty_since,
        permit,
        ..
    } = change
    else {
        panic!("expected an empty publication");
    };
    if let Some(permit) = permit {
        let permit = permit.into_handoff().unwrap();
        let permit = permit.into_render_entity().unwrap();
        let permit = permit.into_gpu_prepared().unwrap();
        assert!(permit.retire());
    }
    stream.acknowledge_mesh_upload(key, generation, dirty_since, dirty_since);
    assert!(stream.is_mesh_clean(key));
}

/// First empty receipts cost one operation per section; retiring their settled column costs none.
#[test]
fn settled_empty_column_retention_spends_no_removal_operations() {
    for stored in [false, true] {
        for keep_survivor in [false, true] {
            let mut stream = light_scheduler::lit_stream(0);
            stream.submit(1, WorldEvent::ChunkRadiusUpdated(2)).unwrap();
            let column = ChunkKey::new(0, 0, 0);
            let range = stream.authority.dimension_range(0).unwrap();
            let keys = (0..range.sub_chunk_count)
                .map(|offset| {
                    SubChunkKey::from_chunk(column, range.base_sub_chunk_y + offset as i32)
                })
                .collect::<Vec<_>>();
            let now = Instant::now();
            for &key in &keys {
                empty_section(&mut stream, key, stored);
                stream.mark_dirty_exact(key, now);
            }
            stream.loaded_columns.insert(column);
            let survivor = SubChunkKey::new(0, 20, 0, 0);
            if keep_survivor {
                stream.record_known_air(survivor);
            }
            let allowance = PublicationAllowance::new(PublicationServiceConfig::DEFAULT);
            allowance.begin_frame(1, 0, 0, keys.len() * 2, keys.len() * 2);
            stream.set_publication_allowance(allowance.clone());
            let before = allowance.remaining_zero_byte_operations();
            stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, keys.len());
            let changes = stream.take_mesh_changes();
            assert_eq!(
                changes.len(),
                keys.len(),
                "first empty evidence must publish"
            );
            assert_eq!(
                allowance.remaining_zero_byte_operations(),
                before - keys.len()
            );
            for change in changes {
                acknowledge_removal(&mut stream, change);
            }
            let remaining = allowance.remaining_zero_byte_operations();
            let queued = stream.stats.publication_stages.mesh_changes_queued;

            assert!(stream.retain_local([320.5, 80.0, 0.5]));
            stream.dispatch_mesh_jobs_with_limits([320.5, 80.0, 0.5], 0, keys.len());

            assert_eq!(
                allowance.remaining_zero_byte_operations(),
                remaining,
                "settled GPU absence needs no second publication on eviction"
            );
            assert_eq!(stream.stats.publication_stages.mesh_changes_queued, queued);
            assert!(stream.take_mesh_changes().is_empty());
            assert!(!stream.loaded_columns.contains(&column));
            for key in keys {
                assert!(!stream.resident.contains(&key));
                assert!(!stream.known_air.contains(&key));
                assert!(!stream.applied_mesh_generations.contains_key(&key));
                assert!(!stream.withdrawn_meshes.contains(&key));
                assert!(stream.authority.terrain().sub_chunk(key).is_none());
                assert!(stream.revisions.dirty(key).is_none());
            }
            assert_eq!(stream.resident.contains(&survivor), keep_survivor);
        }
    }
}

/// Missing, queued and superseded empty receipts still need a fresh eviction removal.
#[test]
fn eviction_preserves_unsettled_empty_removals() {
    for state in 0..4 {
        let mut stream = light_scheduler::lit_stream(0);
        let key = SubChunkKey::new(0, 0, 0, 0);
        empty_section(&mut stream, key, true);
        let now = Instant::now();
        let mut previous = None;
        if state != 0 {
            stream.mark_dirty_exact(key, now);
            stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);
            let change = stream.pop_mesh_change().unwrap();
            let WorldMeshChange::Remove {
                generation,
                dirty_since,
                ..
            } = &change
            else {
                panic!("first empty publication");
            };
            previous = Some((*generation, *dirty_since));
            if state >= 2 {
                acknowledge_removal(&mut stream, change);
                stream.mark_dirty_exact(key, now);
                if state == 3 {
                    stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);
                    assert!(stream.pop_mesh_change().is_some());
                }
            }
        }
        let allowance = PublicationAllowance::new(PublicationServiceConfig::DEFAULT);
        allowance.begin_frame(1, 0, 0, 1, 1);
        stream.set_publication_allowance(allowance.clone());

        stream.evict_column(key.chunk());
        let dirty = stream
            .revisions
            .dirty(key)
            .expect("eviction still owes absence");
        if let Some((generation, since)) = previous {
            assert!(dirty.revision > generation);
            stream.acknowledge_mesh_upload(key, generation, since, now);
            assert!(stream.revisions.is_current(key, dirty.revision));
        }
        stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);

        assert_eq!(allowance.remaining_zero_byte_operations(), 0);
        assert!(matches!(
            stream.pop_mesh_change(),
            Some(WorldMeshChange::Remove { key: removed, generation, .. })
                if removed == key && generation == dirty.revision
        ));
        assert!(stream.pop_mesh_change().is_none());
    }
}

/// A returning section needs a new first receipt, even after its prior settled absence retired.
#[test]
fn empty_retirement_does_not_reuse_receipts_after_reentry() {
    let mut stream = light_scheduler::lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    empty_section(&mut stream, key, false);
    let now = Instant::now();
    let old = stream.mark_dirty_exact(key, now);
    stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);
    let change = stream.pop_mesh_change().unwrap();
    acknowledge_removal(&mut stream, change);
    stream.evict_column(key.chunk());
    assert!(stream.revisions.dirty(key).is_none());

    empty_section(&mut stream, key, false);
    let current = stream.mark_dirty_exact(key, now);
    assert!(current > old);
    stream.acknowledge_mesh_upload(key, old, now, now);
    assert!(!stream.is_mesh_clean(key));
    stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);
    let change = stream
        .pop_mesh_change()
        .expect("re-entry needs its own receipt");
    assert!(
        matches!(&change, WorldMeshChange::Remove { generation, .. } if *generation == current)
    );
    acknowledge_removal(&mut stream, change);
}

/// Prepared geometry and a later conversion to air both keep their first removal publication.
#[test]
fn geometry_and_unacknowledged_air_conversion_still_publish_removals() {
    for convert_to_air in [false, true] {
        let mut stream = light_scheduler::lit_stream(1);
        let key = SubChunkKey::new(1, 0, 0, 0);
        stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(2))
            .unwrap();
        light_scheduler::install_current_light(&mut stream, key, 0, 15, true);
        stream.mark_dirty_exact(key, Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 1), 1);
        let mut completed = false;
        test_time::eventually("geometry completion", || {
            if let Ok(completion) = stream.mesh_rx.try_recv() {
                stream.accept_mesh_completion(completion);
                completed = true;
            }
            completed
        });
        let WorldMeshChange::Upsert {
            mesh,
            generation,
            dirty_since,
            ..
        } = stream.pop_mesh_change().expect("geometry publication")
        else {
            panic!("expected prepared geometry");
        };
        assert!(!mesh.is_empty());
        stream.acknowledge_mesh_upload(key, generation, dirty_since, dirty_since);
        if convert_to_air {
            empty_section(&mut stream, key, true);
            stream.mark_changed(key, Instant::now());
            stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1);
            assert!(matches!(
                stream.pop_mesh_change(),
                Some(WorldMeshChange::Remove { .. })
            ));
            assert!(!stream.is_mesh_clean(key));
        }

        stream.evict_column(key.chunk());
        stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1);

        assert!(
            matches!(stream.pop_mesh_change(), Some(WorldMeshChange::Remove { key: removed, .. }) if removed == key)
        );
    }
}

/// A settled air receipt cannot bypass eviction removal while an older geometry job is retained.
#[test]
fn empty_retirement_keeps_removal_with_inflight_geometry() {
    let mut stream = light_scheduler::lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(2))
        .unwrap();
    light_scheduler::install_current_light(&mut stream, key, 0, 15, true);
    stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 1), 1);
    let mut old_completion = None;
    test_time::eventually("retained geometry completion", || {
        old_completion = stream.mesh_rx.try_recv().ok();
        old_completion.is_some()
    });
    empty_section(&mut stream, key, true);
    stream.mark_changed(key, Instant::now());
    stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1);
    let change = stream.pop_mesh_change().unwrap();
    acknowledge_removal(&mut stream, change);
    assert!(stream.mesh_jobs.in_flight.contains_key(&key));

    stream.evict_column(key.chunk());
    let removal = stream
        .revisions
        .dirty(key)
        .expect("inflight work needs removal");
    stream.accept_mesh_completion(old_completion.unwrap());
    assert!(stream.pop_mesh_change().is_none());
    assert!(stream.revisions.is_current(key, removal.revision));
    stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1);

    assert!(matches!(
        stream.pop_mesh_change(),
        Some(WorldMeshChange::Remove { key: removed, generation, .. })
            if removed == key && generation == removal.revision
    ));
}

/// Skipping a redundant publication still invalidates meshes that sampled the retired source.
#[test]
fn settled_empty_eviction_still_invalidates_surviving_neighbours() {
    let mut stream = light_scheduler::lit_stream(0);
    let removed = SubChunkKey::new(0, 0, 0, 0);
    let neighbour = SubChunkKey::new(0, 1, 0, 0);
    empty_section(&mut stream, removed, true);
    stream
        .authority
        .commit_sub_chunk(neighbour, uniform_sub_chunk(2))
        .unwrap();
    stream.sync_resident(neighbour);
    stream.mark_dirty_exact(removed, Instant::now());
    stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);
    let change = stream.pop_mesh_change().unwrap();
    acknowledge_removal(&mut stream, change);
    assert!(stream.revisions.dirty(neighbour).is_none());

    stream.evict_column(removed.chunk());

    assert!(stream.revisions.dirty(removed).is_none());
    assert!(stream.revisions.dirty(neighbour).is_some());
    assert!(stream.resident.contains(&neighbour));
    assert!(stream.authority.terrain().sub_chunk(removed).is_none());
}

/// Changed lighting cannot change acknowledged air geometry, but first empty evidence still publishes.
#[test]
fn changed_light_preserves_settled_empty_publication() {
    for acknowledged in [false, true] {
        let mut stream = light_scheduler::lit_stream(0);
        let key = SubChunkKey::new(0, 0, 0, 0);
        empty_section(&mut stream, key, true);
        let now = Instant::now();
        if acknowledged {
            stream.mark_dirty_exact(key, now);
            stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);
            let changes = stream.take_mesh_changes();
            assert_eq!(changes.len(), 1);
            for change in changes {
                acknowledge_removal(&mut stream, change);
            }
        }
        let queued = stream.stats.publication_stages.mesh_changes_queued;
        stream.mark_changed_light_mesh_dependents(key, [true; 6], now, false);
        stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);
        let changes = stream.take_mesh_changes();
        assert_eq!(changes.len(), usize::from(!acknowledged));
        assert_eq!(
            stream.stats.publication_stages.mesh_changes_queued - queued,
            u64::from(!acknowledged),
        );
        if acknowledged {
            assert!(stream.is_mesh_clean(key));
        }
    }
}
