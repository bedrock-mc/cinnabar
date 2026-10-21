use super::*;

/// Batch retirement preserves overlap and snapshots while invalidating removed authority.
#[test]
fn batch_eviction_preserves_overlap_and_snapshot() {
    let mut harness = Harness::for_tests();
    let removed = SubChunkKey::new(0, 0, 4, 0);
    let retained = SubChunkKey::new(0, 1, 4, 0);
    for key in [removed, retained] {
        harness
            .stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(STONE))
            .unwrap();
        harness.stream.sync_resident(key);
    }
    let snapshot = harness
        .stream
        .authority
        .terrain()
        .sub_chunk(removed)
        .unwrap();
    let revision = harness
        .stream
        .authority
        .terrain()
        .collision_revision(retained.chunk());
    harness
        .stream
        .evict_columns(BTreeSet::from([removed.chunk()]));
    assert!(
        harness
            .stream
            .authority
            .terrain()
            .sub_chunk(removed)
            .is_none()
    );
    assert!(
        harness
            .stream
            .authority
            .terrain()
            .collision_revision(removed.chunk())
            .is_none()
    );
    assert_eq!(
        harness
            .stream
            .authority
            .terrain()
            .collision_revision(retained.chunk()),
        revision
    );
    assert!(harness.stream.resident.contains(&retained));
    assert!(harness.stream.mesh_jobs.pending.contains_key(&removed));
    assert!(harness.stream.mesh_jobs.pending.contains_key(&retained));
    assert_eq!(snapshot.runtime_id(0, 0, 0, 0), Some(STONE));
}

/// Lending the stream to its between-frames service converges, across a request-mode join and
/// a disjoint teleport, to exactly the meshes frame-only polling presents, never presenting a
/// darker or differently shaped intermediate mesh.
#[test]
fn between_frames_service_converges_to_the_frame_polled_view() {
    let converge = |harness: &mut Harness| {
        for (center, teleport) in [
            (ChunkKey::new(0, 0, 0), false),
            (ChunkKey::new(0, 125, 137), true),
        ] {
            harness.send_view(center, teleport);
            let report = harness.run();
            assert!(harness.idle());
            assert_eq!((report.dark_meshes, report.geometry_meshes), (0, 0));
        }
        harness.presented.clone()
    };
    let frame_polled = converge(&mut Harness::for_tests());
    let mut serviced = Harness::for_tests().with_service();
    assert_eq!(converge(&mut serviced), frame_polled);
    let work = serviced.service_work;
    assert!(
        work.decoded_results > 0 && work.light_results > 0 && work.mesh_results > 0,
        "the service carried decode, light and mesh work: {work:?}"
    );
}

/// Reports complete initial-load and disjoint-teleport convergence through real stream polling.
#[test]
#[ignore = "offline streaming timing comparison"]
fn mesh_stall_stream_timing() {
    let mut harness = Harness::new(8, 1);
    for (phase, center, teleport) in [
        ("initial", ChunkKey::new(0, 0, 0), false),
        ("teleport", ChunkKey::new(0, 125, 137), true),
    ] {
        harness.send_view(center, teleport);
        let started = Instant::now();
        let report = harness.run();
        println!(
            "mesh_stall_stream {phase} drain_ms={} {report:?}",
            started.elapsed().as_millis()
        );
        assert!(harness.idle(), "{phase} did not drain");
        assert_eq!((report.dark_meshes, report.geometry_meshes), (0, 0));
    }
}

/// Bursty replies overlap repeated lighting waves and a disjoint view replacement.
#[test]
#[ignore = "offline burst and lighting saturation timing"]
fn mesh_stall_burst_timing() {
    let mut harness = Harness::new(48, 4);
    for (phase, center, teleport) in [
        ("initial", ChunkKey::new(0, 0, 0), false),
        ("teleport", ChunkKey::new(0, 125, 137), true),
    ] {
        harness.send_view(center, teleport);
        harness.frame_work_times.clear();
        harness.peak_light_jobs = 0;
        let started = Instant::now();
        let mut waves = 0;
        let mut polls = 0;
        while waves < 3 || !harness.idle() {
            if waves < 3 && polls % 16 == 15 && harness.stream.resident.len() > 1_000 {
                let sources = harness
                    .stream
                    .resident
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>();
                harness.stream.mark_light_changed_sources(sources);
                waves += 1;
            }
            harness.step();
            polls += 1;
            assert!(started.elapsed() < Duration::from_secs(60), "burst stalled");
        }
        assert!(
            harness
                .stream
                .resident
                .iter()
                .all(|key| harness.stream.light_is_current(*key))
        );
        harness.frame_work_times.sort_unstable();
        let times = &harness.frame_work_times;
        println!(
            "mesh_stall_burst {phase} drain_ms={} slots={} waves={waves} peak_light={} frame_p99_us={} frame_max_us={}",
            started.elapsed().as_millis(),
            harness.stream.resident.len(),
            harness.peak_light_jobs,
            times[times.len() * 99 / 100].as_micros(),
            times.last().unwrap().as_micros()
        );
        assert!(
            *times.last().unwrap() < BACKLOG_FRAME_LIMIT,
            "{phase} backlog frame exceeded {BACKLOG_FRAME_LIMIT:?}"
        );
        assert!(
            harness.peak_light_jobs
                >= 99.min(
                    initial_light_job_cap()
                        * vanilla_dimension_range(0).unwrap().sub_chunk_count as usize
                )
        );
    }
}
