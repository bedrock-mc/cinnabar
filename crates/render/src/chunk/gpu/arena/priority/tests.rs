use super::*;
use bevy::math::FloatOrd;

/// Runs the previous full-sort policy as an independent ordering oracle.
fn previous_plan(
    mut candidates: Vec<GpuUpdateCandidate>,
    camera: Vec3,
    tint: ChunkBiomeTintIdentity,
    fairness: &GpuUpdateFairness,
) -> Vec<Entity> {
    candidates.retain(|candidate| chunk_tint_identity_is_active(candidate.tint_identity, tint));
    candidates.sort_by_cached_key(|candidate| {
        let age = fairness.wait_age(candidate.entity);
        (
            Reverse(candidate.priority.is_urgent() || fairness.is_urgent(candidate.entity)),
            Reverse(if age >= GPU_UPDATE_STARVED_FRAMES {
                age
            } else {
                0
            }),
            Reverse(age >= GPU_UPDATE_OVERDUE_FRAMES),
            FloatOrd(ChunkUploadPriority::from_camera(candidate.key, camera).distance_squared()),
            candidate.key,
        )
    });
    candidates
        .into_iter()
        .map(|candidate| candidate.entity)
        .collect()
}

/// Creates reproducible shuffled distances with no local game fixtures.
fn candidates(world: &mut World, count: usize) -> Vec<GpuUpdateCandidate> {
    (0..count)
        .map(|index| GpuUpdateCandidate {
            entity: world.spawn_empty().id(),
            key: SubChunkKey::new(0, ((index * 4051) % count) as i32, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            priority: ChunkUploadPriority::new(0.0),
        })
        .collect()
}

#[test]
fn retained_gpu_order_matches_full_sort_across_movement_aging_and_replacement() {
    let mut world = World::new();
    let mut active = candidates(&mut world, 513);
    let tint = ChunkBiomeTintIdentity::default();
    let mut fairness = GpuUpdateFairness::default();
    // Duplicate keys must keep input order, including when that order reverses.
    active[1].key = active[0].key;
    for frame in 0..125 {
        let camera = match frame % 8 {
            0..=3 => Vec3::ZERO,
            4 => Vec3::new(frame as f32 * 16.0, 8.0, 8.0),
            5 => Vec3::splat(f32::NAN),
            _ => Vec3::splat(f32::INFINITY),
        };
        active.reverse();
        active[5].priority = if frame % 3 == 0 {
            ChunkUploadPriority::urgent()
        } else {
            ChunkUploadPriority::new(0.0)
        };
        active[7].tint_identity = if frame % 2 == 0 {
            tint
        } else {
            ChunkBiomeTintIdentity::new(1, 0)
        };
        active[9].key = SubChunkKey::new(0, frame, 2, -3);
        active[9].generation += 1;
        let expected = previous_plan(active.clone(), camera, tint, &fairness);
        let selected =
            plan_gpu_chunk_updates(active.clone(), &HashMap::new(), camera, tint, &fairness);
        assert_eq!(selected, expected, "frame {frame}");
        let urgent = active
            .iter()
            .filter(|candidate| candidate.priority.is_urgent())
            .map(|candidate| candidate.entity)
            .collect::<Vec<_>>();
        // Most waiters cross both age thresholds while near work keeps succeeding.
        fairness.finish_frame(&selected, &selected[..1], &urgent);
    }
    fairness.reset();
    assert_eq!(
        plan_gpu_chunk_updates(active.clone(), &HashMap::new(), Vec3::ZERO, tint, &fairness),
        previous_plan(active, Vec3::ZERO, tint, &fairness)
    );
}

#[test]
fn retained_gpu_starved_ages_preserve_order_and_saturated_ties() {
    let mut world = World::new();
    let active = candidates(&mut world, 5);
    let tint = ChunkBiomeTintIdentity::default();
    let mut fairness = GpuUpdateFairness::default();
    for (candidate, age) in active.iter().zip([
        u32::MAX,
        u32::MAX,
        u32::MAX - 1,
        GPU_UPDATE_STARVED_FRAMES,
        GPU_UPDATE_STARVED_FRAMES + 1,
    ]) {
        fairness.wait_ages.insert(candidate.entity, age);
    }
    for _ in 0..4 {
        let expected = previous_plan(active.clone(), Vec3::ZERO, tint, &fairness);
        let selected =
            plan_gpu_chunk_updates(active.clone(), &HashMap::new(), Vec3::ZERO, tint, &fairness);
        assert_eq!(selected, expected);
        fairness.finish_frame(&selected, &[], &[]);
    }
}

#[test]
fn retained_gpu_selection_reuses_distances_at_fixed_backlogs() {
    for size in [512, 4_096, 65_536] {
        let mut world = World::new();
        let mut active = candidates(&mut world, size);
        let tint = ChunkBiomeTintIdentity::default();
        let mut fairness = GpuUpdateFairness::default();
        for frame in 0..=GPU_UPDATE_STARVED_FRAMES + 1 {
            let selected = plan_gpu_chunk_updates(
                active.clone(),
                &HashMap::new(),
                Vec3::ZERO,
                tint,
                &fairness,
            );
            assert_eq!(
                selected,
                previous_plan(active.clone(), Vec3::ZERO, tint, &fairness)
            );
            let evaluated = fairness.priorities.lock().unwrap().distance_evaluations;
            assert_eq!(evaluated, if frame == 0 { size } else { 0 });
            fairness.finish_frame(&selected, &[], &[]);
        }
        eprintln!(
            "GPU selection backlog={size}: previous distances/frame={size}, retained warm distances/frame=0"
        );
        active[0].key = SubChunkKey::new(0, -1, 0, 0);
        plan_gpu_chunk_updates(active.clone(), &HashMap::new(), Vec3::ZERO, tint, &fairness);
        assert_eq!(fairness.priorities.lock().unwrap().distance_evaluations, 1);
        let camera = Vec3::splat(16.0);
        let selected =
            plan_gpu_chunk_updates(active.clone(), &HashMap::new(), camera, tint, &fairness);
        assert_eq!(selected, previous_plan(active, camera, tint, &fairness));
        assert_eq!(
            fairness.priorities.lock().unwrap().distance_evaluations,
            size
        );
        plan_gpu_chunk_updates(Vec::new(), &HashMap::new(), camera, tint, &fairness);
        assert_eq!(
            fairness.priorities.lock().unwrap().order.ordered().count(),
            0
        );
    }
}
