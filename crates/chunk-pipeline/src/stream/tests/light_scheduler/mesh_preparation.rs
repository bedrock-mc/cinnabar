use super::*;

/// Saturated service leaves geometry untouched and resumes with the nearest ready section.
#[test]
fn unavailable_mesh_capacity_skips_geometry_classification() {
    for limit in ["budget", "workers", "output"] {
        let mut stream = lit_stream(1);
        let near = SubChunkKey::new(1, 0, 0, 0);
        let far = SubChunkKey::new(1, 8, 0, 0);
        for key in [far, near] {
            stream
                .authority
                .commit_sub_chunk(key, super::uniform_sub_chunk(2))
                .unwrap();
            install_current_light(&mut stream, key, 0, 0, false);
            stream.mark_dirty_exact(key, Instant::now());
        }
        let budget = usize::from(limit != "budget");
        if limit == "workers" {
            stream.admitted_mesh_jobs.store(
                super::super::super::meshing::admission::mesh_worker_cap(),
                Ordering::Release,
            );
        }
        if limit == "output" {
            for x in 0..MAX_PENDING_MESH_CHANGES {
                stream.mesh_changes.push(WorldMeshChange::Remove {
                    key: SubChunkKey::new(1, x as i32, 8, 0),
                    generation: 0,
                    dirty_since: Instant::now(),
                    urgent: false,
                    permit: None,
                });
            }
        }
        let classifications = &super::super::super::meshing::GEOMETRY_CLASSIFICATIONS;
        classifications.set(0);
        assert_eq!(
            stream.dispatch_mesh_jobs_with_limits([8.0; 3], budget, 1),
            0
        );
        assert_eq!(
            classifications.get(),
            0,
            "{limit}: no geometry can be admitted"
        );
        assert!(stream.mesh_jobs.pending.contains_key(&near));
        assert!(stream.mesh_jobs.pending.contains_key(&far));
        stream.admitted_mesh_jobs.store(0, Ordering::Release);
        drop(stream.mesh_changes.drain());
        assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(completion.key, near, "{limit}: resume by current priority");
        stream.accept_mesh_completion(completion);
    }
}

/// Reserved urgent work still dispatches while ordinary workers are occupied.
#[test]
fn saturated_regular_meshes_keep_urgent_admission() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact_with_priority(key, Instant::now(), true);
    let cap = super::super::super::meshing::admission::mesh_worker_cap();
    stream.admitted_mesh_jobs.store(cap, Ordering::Release);
    assert_eq!(stream.dispatch_mesh_jobs_reserving([8.0; 3], 1, 1, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(completion.key, key);
    stream.accept_mesh_completion(completion);
    assert_eq!(stream.admitted_mesh_jobs.load(Ordering::Acquire), cap);
    stream.admitted_mesh_jobs.store(0, Ordering::Release);
}
