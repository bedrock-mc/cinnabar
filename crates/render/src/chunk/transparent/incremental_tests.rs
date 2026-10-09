//! Incremental per-sub-chunk ordering against the former global sort as the oracle.
use super::*;

/// The former global sort input, kept only as this oracle's vocabulary.
struct OracleCandidate {
    key: SubChunkKey,
    local_quad_index: u32,
    liquid_record_index: u32,
    metadata_index: u32,
    subchunk_center: [f32; 3],
    quad_centroid: [f32; 3],
}

/// The former whole-view sort: sub-chunks by view depth, faces by the shared metric.
fn oracle_sort(
    view_from_world: Mat4,
    candidates: &[OracleCandidate],
) -> Vec<PackedTransparentDrawRef> {
    let metric = TransparentFaceMetric::new(view_from_world.inverse().transform_point3(Vec3::ZERO));
    let quad_depths = candidates
        .iter()
        .map(|candidate| metric.distance(candidate.key, Vec3::from_array(candidate.quad_centroid)))
        .collect::<Vec<_>>();
    let mut grouped = BTreeMap::<SubChunkKey, Vec<usize>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        grouped.entry(candidate.key).or_default().push(index);
    }
    let mut groups = grouped
        .into_iter()
        .map(|(key, indices)| {
            let center = Vec3::from_array(candidates[indices[0]].subchunk_center);
            let depth = view_from_world.transform_point3(center).z;
            (depth, key, indices)
        })
        .collect::<Vec<_>>();
    groups.sort_by(|(left_depth, left_key, _), (right_depth, right_key, _)| {
        left_depth
            .total_cmp(right_depth)
            .then_with(|| left_key.cmp(right_key))
    });
    let mut refs = Vec::with_capacity(candidates.len());
    for (_depth, _key, mut group) in groups {
        group.sort_by(|&left, &right| {
            let left_candidate = &candidates[left];
            let right_candidate = &candidates[right];
            quad_depths[right]
                .total_cmp(&quad_depths[left])
                .then_with(|| left_candidate.key.cmp(&right_candidate.key))
                .then_with(|| {
                    left_candidate
                        .local_quad_index
                        .cmp(&right_candidate.local_quad_index)
                })
        });
        refs.extend(group.into_iter().map(|index| {
            let candidate = &candidates[index];
            PackedTransparentDrawRef::new(candidate.liquid_record_index, candidate.metadata_index)
        }));
    }
    refs
}

struct Scene {
    groups: Vec<Arc<TransparentGroupInput>>,
    candidates: Vec<OracleCandidate>,
}

/// Water tops, sides and coplanar diagonal ties across a 5x2x5 block of sub-chunks.
fn scene() -> Scene {
    let mut seed = 0x2545_f491_u32;
    let mut next = move |bound: u32| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed % bound
    };
    let mut keys = Vec::new();
    for x in -2..=2 {
        for y in 3..=4 {
            for z in -2..=2 {
                keys.push(SubChunkKey::new(0, x, y, z));
            }
        }
    }
    let (mut groups, mut candidates, mut record) = (Vec::new(), Vec::new(), 4_u32);
    for (metadata, key) in keys.into_iter().enumerate() {
        let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
        let centroids = (0..96)
            .map(|_| {
                let (a, b) = (next(16) as f32, next(16) as f32);
                origin
                    + match next(4) {
                        0 => Vec3::new(a + 0.5, 15.0 - meshing::liquid::LIQUID_FACE_INSET, b + 0.5),
                        1 => Vec3::new(a, b + 0.5, next(16) as f32 + 0.5),
                        2 => Vec3::new(a + 0.5, b + 0.5, 15.0 - a),
                        _ => Vec3::new(15.0 - b, a + 0.5, b + 0.5),
                    }
            })
            .collect::<Box<[_]>>();
        let identity = TransparentAllocationIdentity::new(
            key,
            1,
            record * 4..(record + centroids.len() as u32) * 4,
            0..4,
            metadata as u32,
        );
        for (local, centroid) in centroids.iter().enumerate() {
            candidates.push(OracleCandidate {
                key,
                local_quad_index: local as u32,
                liquid_record_index: record + local as u32,
                metadata_index: metadata as u32,
                subchunk_center: (origin + Vec3::splat(8.0)).to_array(),
                quad_centroid: centroid.to_array(),
            });
        }
        record += centroids.len() as u32;
        groups.push(Arc::new(TransparentGroupInput {
            identity,
            tint_identity: ChunkBiomeTintIdentity::default(),
            centroids,
            tint_colors: Box::new([]),
        }));
    }
    Scene { groups, candidates }
}

#[derive(Default)]
struct Incremental {
    base: Option<TransparentLayoutBase>,
}

impl Incremental {
    /// Sorts like a worker job against the last slot, returning every group's refs in key
    /// order and the classes of the groups it sorted again.
    fn sort(
        &mut self,
        scene: &Scene,
        camera: Vec3,
    ) -> (Vec<PackedTransparentDrawRef>, Vec<FaceOrderClass>) {
        let allocations = scene
            .groups
            .iter()
            .map(|group| group.identity.clone())
            .collect::<Arc<[_]>>();
        let output = plan_transparent_slot(
            camera,
            &allocations,
            &scene.groups,
            self.base.as_ref(),
            usize::MAX,
        );
        let previous = self.base.as_ref().map(|base| &base.layout);
        let mut refs = Vec::new();
        let mut resorted = Vec::new();
        for (group, class) in output
            .layout
            .groups
            .iter()
            .zip(output.layout.classes.iter())
        {
            refs.extend_from_slice(
                &output.refs[group.ref_range.start as usize..group.ref_range.end as usize],
            );
            let kept = previous.is_some_and(|layout| {
                layout
                    .groups
                    .iter()
                    .zip(layout.classes.iter())
                    .any(|(old, old_class)| old.key == group.key && old_class == class)
            });
            if !kept {
                resorted.push(class.expect("worker groups carry their class"));
            }
        }
        self.base = Some(TransparentLayoutBase {
            refs: output.refs,
            allocations,
            layout: output.layout,
        });
        (refs, resorted)
    }
}

fn by_metadata(refs: &[PackedTransparentDrawRef]) -> BTreeMap<u32, Vec<PackedTransparentDrawRef>> {
    let mut groups = BTreeMap::<u32, Vec<_>>::new();
    for draw_ref in refs {
        groups
            .entry(draw_ref.metadata_index())
            .or_default()
            .push(*draw_ref);
    }
    groups
}

fn camera_path() -> Vec<Vec3> {
    let mut path = Vec::new();
    for step in 0..140 {
        let t = step as f32;
        path.push(Vec3::new(
            -20.3 + t * 0.37,
            63.4 + (t * 0.3).sin() * 2.0,
            7.9,
        ));
    }
    for step in 0..60 {
        let t = step as f32;
        path.push(Vec3::new(31.0 - t * 0.53, 70.2 - t * 0.29, 31.0 - t * 0.61));
    }
    for step in 0..40 {
        path.push(Vec3::new(0.01, 40.0 + step as f32 * 1.13, -0.01));
    }
    for jump in [
        Vec3::new(-100.0, 200.0, 50.0),
        Vec3::new(15.99, 64.0, 15.99),
        Vec3::new(16.01, 64.0, 16.01),
        Vec3::new(-31.9, 47.5, 47.9),
        Vec3::new(8.0, 300.0, 8.0),
    ] {
        path.push(jump);
    }
    path
}

#[test]
fn incremental_order_matches_full_sort_across_camera_moves() {
    let scene = scene();
    let mut incremental = Incremental::default();
    for (index, camera) in camera_path().into_iter().enumerate() {
        let rotation = Quat::from_rotation_y(index as f32 * 0.7) * Quat::from_rotation_x(0.4);
        let view_from_world = Mat4::from_rotation_translation(rotation, camera).inverse();
        let oracle_camera = view_from_world.inverse().transform_point3(Vec3::ZERO);
        let oracle = oracle_sort(view_from_world, &scene.candidates);
        let (refs, _) = incremental.sort(&scene, oracle_camera);
        assert_eq!(by_metadata(&refs), by_metadata(&oracle), "camera {camera}");
        let layout = refs
            .chunk_by(|left, right| left.metadata_index() == right.metadata_index())
            .map(|run| run[0].metadata_index())
            .collect::<Vec<_>>();
        assert!(layout.is_sorted(), "groups keep the key-ordered layout");
    }
}

#[test]
fn far_groups_reuse_their_order_within_a_direction_class() {
    let scene = scene();
    let mut incremental = Incremental::default();
    // Camera blocks 4..=11 of one chunk keep every class except the near chunk's.
    let start = Vec3::new(4.2, 68.5, 4.2);
    let (_, warm) = incremental.sort(&scene, start);
    assert_eq!(warm.len(), scene.groups.len());
    for step in 1..40 {
        let camera = start + Vec3::new(step as f32 * 0.17, step as f32 * 0.05, step as f32 * 0.11);
        let (_, resorted) = incremental.sort(&scene, camera);
        assert!(
            resorted
                .iter()
                .all(|class| matches!(class, FaceOrderClass::Near(_))),
            "only the near sub-chunk re-sorts at {camera}"
        );
        assert_eq!(resorted.len(), 1);
    }
    // Far above the scene nothing is near, so motion inside the chunk sorts nothing.
    let high = Vec3::new(4.5, 200.0, 4.5);
    incremental.sort(&scene, high);
    for step in 1..20 {
        let (_, resorted) = incremental.sort(&scene, high + Vec3::splat(step as f32 * 0.3));
        assert!(resorted.is_empty());
    }
}

#[test]
fn near_groups_resort_only_on_quantized_motion_and_stay_bounded() {
    let scene = scene();
    let mut incremental = Incremental::default();
    // A chunk corner puts the near interval across two chunks on every axis.
    let corner = Vec3::new(15.9, 63.9, 15.9);
    incremental.sort(&scene, corner);
    let (_, jitter) = incremental.sort(&scene, corner + Vec3::splat(0.001));
    assert!(jitter.is_empty(), "sub-quantum motion reuses every order");
    let (_, moved) = incremental.sort(&scene, corner + Vec3::new(0.0, 0.0, -0.25));
    assert!(!moved.is_empty() && moved.len() <= 8);
    assert!(
        moved
            .iter()
            .all(|class| matches!(class, FaceOrderClass::Near(_)))
    );
}

#[test]
fn unchanged_orders_upload_nothing_and_near_motion_patches_only_its_group() {
    let scene = scene();
    let manifest = scene
        .groups
        .iter()
        .map(|group| group.identity.clone())
        .collect::<Vec<_>>();
    let key = |camera: Vec3| {
        ViewSortKey::try_new(
            camera.to_array(),
            manifest.clone(),
            ChunkTextureAssetIdentity::new(1, 1),
            ChunkBiomeTintIdentity::default(),
        )
        .unwrap()
    };
    let mut state =
        TransparentSortState::with_upload_cap(DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME);
    let commit = |camera: Vec3, state: &mut TransparentSortState| {
        let key = key(camera);
        let generation = state.request(&key);
        let output = plan_transparent_slot(
            camera,
            &key.sorted_allocations,
            &scene.groups,
            state.base_for(&key).as_ref(),
            DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME,
        );
        let result = TransparentSortResult::planned(generation, key, output).unwrap();
        let committed = state.complete(result).unwrap();
        if !committed {
            while state.next_upload_batch().is_some() {
                state.acknowledge_upload();
            }
        }
        state.take_patch()
    };
    let start = Vec3::new(4.2, 68.0, 4.2);
    commit(start, &mut state);
    let slot = state.committed().unwrap().buffer_slot();

    let near = scene
        .groups
        .iter()
        .position(|group| group.identity.key == SubChunkKey::new(0, 0, 4, 0))
        .unwrap();
    let near_start = scene.groups[..near]
        .iter()
        .map(|group| group.centroids.len())
        .sum::<usize>();
    let near_range = near_start..near_start + scene.groups[near].centroids.len();
    let patch = commit(start + Vec3::new(2.5, 0.0, 3.0), &mut state);
    assert!(!patch.is_empty());
    assert!(
        patch
            .iter()
            .all(|span| near_range.start <= span.start && span.end <= near_range.end),
        "only the re-sorted near group is written"
    );
    assert_eq!(state.committed().unwrap().buffer_slot(), slot);

    let high = Vec3::new(4.5, 200.0, 4.5);
    commit(high, &mut state);
    assert!(commit(high + Vec3::new(3.0, 1.0, 2.0), &mut state).is_empty());
    assert!(state.next_upload_batch().is_none());
}
