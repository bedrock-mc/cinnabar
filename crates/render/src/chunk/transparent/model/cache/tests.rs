use super::*;
use render_scene::ChunkUploadPriority;

struct Fixture {
    cache: TransparentModelPreparationCache,
    combined: Option<TransparentModelCandidateCache>,
    orders: TransparentModelDrawOrders,
    assets: ChunkTextureAssets,
    allocations: Vec<(TransparentModelAllocationIdentity, ChunkRenderInstance)>,
    templates: [ModelTemplate; 1],
    quads: [assets::ModelQuad; 3],
}

type BatchOutput = (Range<u32>, FaceOrderClass, Box<[[u32; 2]]>);

/// Builds three faces per allocation, including two equal-distance faces for stable ties.
fn fixture(count: usize) -> Fixture {
    let allocations = (0..count)
        .map(|index| {
            let key = SubChunkKey::new(0, index as i32, 0, 0);
            let base = index as u32 * 32;
            let identity = TransparentModelAllocationIdentity {
                entity: Entity::from_bits(index as u64 + 1),
                key,
                generation: 1,
                model_range: base..base + 4,
                draw_range: base + 4..base + 10,
            };
            let instance = ChunkRenderInstance {
                key,
                origin: chunk_origin(key),
                generation: identity.generation,
                cube_layout: CubeQuadLayout::default(),
                cube_quads: Arc::from([]),
                cube_lighting: Arc::from([]),
                model_refs: Arc::from([PackedModelRef::new(0, 0, 0, 0b111)]),
                model_lighting: Arc::from([meshing::PackedQuadLighting::new([0; 4])]),
                model_draw_refs: Arc::from([]),
                transparent_model_draw_refs: (0..3)
                    .map(|quad| PackedModelDrawRef::new(0, quad))
                    .collect(),
                liquid_quads: Arc::from([]),
                liquid_lighting: Arc::from([]),
                has_depth_liquid: false,
                has_transparent_liquid: false,
                depth_liquid_start: None,
                biome: meshing::PackedBiomeRecord::fallback(),
                tint_identity: ChunkBiomeTintIdentity::default(),
                priority: ChunkUploadPriority::new(0.0),
                token: None,
                publication_permit: None,
            };
            (identity, instance)
        })
        .collect();
    Fixture {
        cache: default(),
        combined: None,
        orders: default(),
        assets: ChunkTextureAssets::default(),
        allocations,
        templates: [ModelTemplate {
            quad_start: 0,
            quad_count: 3,
            flags: 0,
        }],
        quads: std::array::from_fn(|index| {
            let z = if index == 1 { 256 } else { 0 };
            assets::ModelQuad {
                positions: [[0, 0, z], [256, 0, z], [256, 256, z], [0, 256, z]],
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 0,
            }
        }),
    }
}

impl Fixture {
    /// Scans the current visibility sequence through the production identity cache.
    fn scan(&mut self) -> TransparentModelAddressIdentity {
        self.cache.begin_frame(&self.assets);
        for (identity, instance) in &self.allocations {
            self.cache.observe(identity.clone(), instance);
        }
        self.cache.finish_frame(&mut self.orders)
    }

    /// Resolves pages through the same combined-array reuse boundary as live preparation.
    fn candidates(
        &mut self,
        address: &TransparentModelAddressIdentity,
    ) -> Arc<[TransparentModelSortCandidate]> {
        self.cache
            .candidates(
                &mut self.combined,
                address,
                self.allocations
                    .iter()
                    .map(|(_, instance)| instance.transparent_model_draw_refs.len())
                    .sum(),
                &self.templates,
                &self.quads,
            )
            .unwrap()
    }

    /// Prepares identities and centroids without starting a worker or uploading geometry.
    fn prepare(
        &mut self,
    ) -> (
        TransparentModelAddressIdentity,
        Arc<[TransparentModelSortCandidate]>,
    ) {
        let address = self.scan();
        let candidates = self.candidates(&address);
        (address, candidates)
    }

    /// Reconstructs the uncached candidates in the original stable identity order.
    fn uncached_candidates(&self) -> Arc<[TransparentModelSortCandidate]> {
        let mut allocations = self.allocations.iter().collect::<Vec<_>>();
        allocations.sort_by_key(|(identity, _)| (identity.key, identity.draw_range.start));
        allocations
            .into_iter()
            .flat_map(|(identity, instance)| {
                instance.transparent_model_draw_refs.iter().enumerate().map(
                    move |(index, draw_ref)| {
                        let (centroid, words) = transparent_model_draw_candidate(
                            identity.key,
                            &instance.model_refs,
                            &instance.model_lighting,
                            *draw_ref,
                            &self.templates,
                            &self.quads,
                            identity.model_range.start / 4,
                        )
                        .unwrap();
                        TransparentModelSortCandidate {
                            entity: identity.entity,
                            key: identity.key,
                            draw_range: identity.draw_range.clone(),
                            stable_index: index as u32,
                            centroid,
                            words,
                        }
                    },
                )
            })
            .collect()
    }
}

/// Captures the observable batch output, including stable tie order and camera classes.
fn batch_output(
    camera: Vec3,
    candidates: Arc<[TransparentModelSortCandidate]>,
) -> Vec<BatchOutput> {
    sort_transparent_model_candidates(camera, candidates)
        .into_iter()
        .map(|batch| (batch.draw_range, batch.class, batch.words))
        .collect()
}

/// Creates a worker key using only the supplied camera position and retained inputs.
fn sort_key(address: TransparentModelAddressIdentity, camera: Vec3) -> TransparentModelSortKey {
    TransparentModelSortKey {
        view_entity: Entity::from_bits(100),
        order_camera: TransparentFaceMetric::new(camera)
            .order_camera(address.allocations.iter().map(|identity| identity.key)),
        address,
    }
}

#[test]
fn unchanged_frames_reuse_identity_and_centroid_arrays_without_builds() {
    let mut f = fixture(3);
    let (original, candidates) = f.prepare();
    for _ in 0..8 {
        let (address, reused) = f.prepare();
        assert_eq!(address, original);
        assert!(Arc::ptr_eq(&address.allocations, &original.allocations));
        assert!(Arc::ptr_eq(&reused, &candidates));
    }
    assert_eq!(
        f.cache.counts,
        PreparationCounts {
            addresses: 1,
            pages: 3,
            centroids: 9,
        }
    );
}

#[test]
fn changing_one_allocation_rebuilds_only_its_centroid_page() {
    // Each identity field and mesh stream must independently invalidate its page.
    for change in 0..8 {
        let mut f = fixture(3);
        let (previous, _) = f.prepare();
        let untouched = f.allocations[0].0.entity;
        let retained = Arc::clone(f.cache.pages[&untouched].centroids.as_ref().unwrap());
        let (identity, instance) = &mut f.allocations[1];
        match change {
            0 => {
                identity.generation += 1;
                instance.generation += 1;
            }
            1 => identity.model_range = 320..324,
            2 => identity.draw_range = 324..330,
            3 => identity.entity = Entity::from_bits(99),
            4 => {
                identity.key.x += 5;
                instance.key = identity.key;
            }
            5 => instance.model_refs = instance.model_refs.to_vec().into(),
            6 => instance.model_lighting = instance.model_lighting.to_vec().into(),
            7 => {
                instance.transparent_model_draw_refs =
                    instance.transparent_model_draw_refs.to_vec().into();
            }
            _ => unreachable!(),
        }
        let (address, cached) = f.prepare();
        assert_ne!(address, previous, "input {change}");
        assert!(Arc::ptr_eq(
            f.cache.pages[&untouched].centroids.as_ref().unwrap(),
            &retained,
        ));
        assert_eq!(f.cache.pages.len(), 3);
        assert_eq!(
            f.cache.counts,
            PreparationCounts {
                addresses: 2,
                pages: 4,
                centroids: 12,
            },
            "input {change}",
        );
        assert_eq!(
            batch_output(Vec3::ZERO, cached),
            batch_output(Vec3::ZERO, f.uncached_candidates()),
        );
    }
}

#[test]
fn removal_and_reinsertion_at_the_same_address_evict_the_old_mesh() {
    let mut f = fixture(2);
    let (original, _) = f.prepare();
    let (identity, instance) = f.allocations.remove(0);
    let source = Arc::downgrade(&instance.model_refs);
    let replacement = instance.clone();
    drop(instance);
    let retained = Arc::clone(
        f.cache.pages[&f.allocations[0].0.entity]
            .centroids
            .as_ref()
            .unwrap(),
    );
    f.prepare();
    assert!(!f.cache.pages.contains_key(&identity.entity));
    assert_eq!(f.cache.counts.pages, 2, "removal builds no centroid pages");
    let mut replacement = replacement;
    replacement.model_refs = Arc::from([PackedModelRef::new(1, 0, 0, 0b111)]);
    assert!(source.upgrade().is_none());
    f.allocations.insert(0, (identity, replacement));
    let (reinserted, _) = f.prepare();
    assert_eq!(reinserted.allocations, original.allocations);
    assert_ne!(
        reinserted, original,
        "old work must not match reused addresses"
    );
    assert_eq!(f.cache.counts.pages, 3);
    assert_eq!(f.cache.counts.centroids, 9);
    assert!(Arc::ptr_eq(
        f.cache.pages[&f.allocations[1].0.entity]
            .centroids
            .as_ref()
            .unwrap(),
        &retained,
    ));
}

#[test]
fn retained_mesh_pointers_keep_their_sources_alive_until_eviction() {
    let mut f = fixture(1);
    f.prepare();
    let source = Arc::downgrade(&f.allocations[0].1.model_refs);
    f.allocations.clear();
    assert!(source.upgrade().is_some());
    f.prepare();
    assert!(source.upgrade().is_none());
}

#[test]
fn authored_changes_retain_geometry_but_release_authored_maps() {
    let mut f = fixture(1);
    let array = f.assets.assets().texture_array().clone();
    let maps = Arc::new(
        render_scene::EnhancedTextureAssets::new(
            [array.clone(), array.clone()],
            [array.clone(), array.clone()],
            [array.clone(), array],
            vec![u32::MAX; render_scene::EnhancedTextureAssets::REFERENCE_BASE_WORDS]
                .into_boxed_slice(),
        )
        .unwrap(),
    );
    let source = Arc::downgrade(&maps);
    f.assets = f.assets.with_updated_enhanced(Some(maps));
    let (authored, candidates) = f.prepare();
    let counts = f.cache.counts;
    f.assets = f.assets.with_updated_enhanced(None);
    let (detached, retained) = f.prepare();
    assert_eq!(authored, detached);
    assert!(Arc::ptr_eq(&candidates, &retained));
    assert_eq!(f.cache.counts, counts);
    assert!(
        source.upgrade().is_none(),
        "geometry retains only its carrier"
    );
}

#[test]
fn asset_revisions_and_asset_pointer_changes_rebuild_all_pages() {
    let mut f = fixture(2);
    let (old, _) = f.prepare();
    let asset_source = Arc::downgrade(f.assets.assets());
    f.assets.revision += 1;
    f.quads[0].positions = [[512, 0, 0]; 4];
    let (revised, revised_candidates) = f.prepare();
    assert_ne!(old, revised);
    assert_eq!(f.cache.counts.pages, 4);
    assert_eq!(f.cache.counts.centroids, 12);
    assert_eq!(revised_candidates[0].centroid, Vec3::new(2.0, 0.0, 0.0));
    f.assets =
        ChunkTextureAssets::with_revision(Arc::new(RuntimeAssets::diagnostic()), f.assets.revision);
    let (replaced, _) = f.prepare();
    assert_ne!(revised, replaced);
    assert_eq!(f.cache.counts.pages, 6);
    assert_eq!(f.cache.counts.centroids, 18);
    assert!(
        asset_source.upgrade().is_some(),
        "old worker keys retain their asset pointer"
    );
    drop(old);
    drop(revised);
    assert!(asset_source.upgrade().is_none());
}

#[test]
fn camera_changes_reuse_centroids_and_preserve_stable_sorted_output() {
    let mut f = fixture(1);
    let (address, candidates) = f.prepare();
    let counts = f.cache.counts;
    let before = Vec3::ZERO;
    let after = Vec3::new(0.0, 0.0, 2.0);
    assert_ne!(
        sort_key(address.clone(), before),
        sort_key(address.clone(), after)
    );
    assert_eq!(
        batch_output(before, Arc::clone(&candidates))[0].2.as_ref(),
        [[0, 1], [0, 0], [0, 2]]
    );
    assert_eq!(
        batch_output(after, Arc::clone(&candidates))[0].2.as_ref(),
        [[0, 0], [0, 2], [0, 1]]
    );
    for camera in [before, after, Vec3::splat(200.0)] {
        let (reused_address, reused) = f.prepare();
        assert!(Arc::ptr_eq(
            &reused_address.allocations,
            &address.allocations
        ));
        assert!(Arc::ptr_eq(&reused, &candidates));
        assert_eq!(
            batch_output(camera, reused),
            batch_output(camera, f.uncached_candidates())
        );
    }
    assert_eq!(f.cache.counts, counts);
}

#[test]
fn visibility_reordering_preserves_stable_identity_ties_and_duplicates() {
    let mut f = fixture(3);
    let (first_identity, _) = f.allocations[0].clone();
    for (identity, instance) in &mut f.allocations {
        identity.key = first_identity.key;
        identity.draw_range = first_identity.draw_range.clone();
        instance.key = first_identity.key;
    }
    f.prepare();
    f.allocations.reverse();
    f.allocations.push(f.allocations[0].clone());
    let address = f.scan();
    assert_eq!(
        address
            .allocations
            .iter()
            .map(|identity| identity.entity)
            .collect::<Vec<_>>(),
        f.allocations
            .iter()
            .map(|(identity, _)| identity.entity)
            .collect::<Vec<_>>(),
    );
    f.candidates(&address);
    assert_eq!(
        f.cache.counts.pages, 3,
        "membership order does not rebuild centroids"
    );
    assert_eq!(f.cache.counts.centroids, 9);
}

#[test]
fn replacing_mesh_or_assets_invalidates_uploaded_classes_without_losing_witnesses() {
    for change_assets in [false, true] {
        let mut f = fixture(2);
        let (address, candidates) = f.prepare();
        let camera = Vec3::ZERO;
        for batch in sort_transparent_model_candidates(camera, candidates) {
            f.orders.publish(&address, batch);
        }
        let changed = f.allocations[0].0.clone();
        let untouched = f.allocations[1].0.clone();
        let revision = f.orders.get(&changed).unwrap().revision;
        if change_assets {
            f.assets.revision += 1;
        } else {
            f.allocations[0].1.model_refs = f.allocations[0].1.model_refs.to_vec().into();
        }
        f.scan();
        assert_eq!(f.orders.class(&changed), None);
        assert_eq!(f.orders.get(&changed).unwrap().revision, revision);
        assert_eq!(f.orders.class(&untouched).is_none(), change_assets);
    }
}

#[test]
fn reappearing_pages_cannot_reuse_classes_from_an_evicted_mesh() {
    let mut f = fixture(1);
    let (address, candidates) = f.prepare();
    for batch in sort_transparent_model_candidates(Vec3::ZERO, candidates) {
        f.orders.publish(&address, batch);
    }
    let (identity, mut instance) = f.allocations.remove(0);
    f.prepare();
    assert!(f.orders.class(&identity).is_some());
    instance.model_refs = Arc::from([PackedModelRef::new(1, 0, 0, 0b111)]);
    f.allocations.push((identity.clone(), instance));
    f.scan();
    assert_eq!(f.orders.class(&identity), None);
}

#[test]
fn stale_in_flight_results_cannot_stage_after_inputs_are_replaced() {
    for change in 0..5 {
        let mut f = fixture(1);
        let (address, candidates) = f.prepare();
        let old_key = sort_key(address, Vec3::ZERO);
        let old_batches = sort_transparent_model_candidates(Vec3::ZERO, candidates);
        match change {
            0 => f.allocations[0].0.generation += 1,
            1 => {
                f.allocations[0].1.model_refs = f.allocations[0].1.model_refs.to_vec().into();
            }
            2 => f.assets.revision += 1,
            3 => {
                let removed = f.allocations.remove(0);
                f.prepare();
                f.allocations.push(removed);
            }
            4 => f.cache.clear(),
            _ => unreachable!(),
        }
        let (current_address, current_candidates) = f.prepare();
        let current = sort_key(current_address, Vec3::ZERO);
        let (old_generation, new_generation) = (ViewSortGeneration(1), ViewSortGeneration(2));
        let mut runtime = TransparentModelSortRuntime::default();
        runtime.gate.in_flight = Some(old_generation);
        runtime.gate.pending = Some((
            new_generation,
            TransparentModelSortWork {
                generation: new_generation,
                key: current.clone(),
                camera_position: Vec3::ZERO,
                candidates: Arc::clone(&current_candidates),
            },
        ));
        let next = stage_transparent_model_result(
            &mut runtime,
            TransparentModelWorkerResult {
                generation: old_generation,
                key: old_key,
                batches: old_batches,
            },
            &current,
        )
        .unwrap();
        assert_eq!(next.0, new_generation);
        assert_eq!(next.1.key, current);
        assert!(
            runtime.staged.is_none(),
            "replacement {change} rejects old work"
        );
        assert!(runtime.committed.is_none());
        assert!(
            stage_transparent_model_result(
                &mut runtime,
                TransparentModelWorkerResult {
                    generation: new_generation,
                    key: current.clone(),
                    batches: sort_transparent_model_candidates(Vec3::ZERO, current_candidates),
                },
                &current,
            )
            .is_none()
        );
        assert_eq!(runtime.staged.as_ref().unwrap().key, current);
    }
}

#[test]
fn camera_only_changes_still_accept_in_flight_work_for_the_same_inputs() {
    let mut f = fixture(1);
    let (address, candidates) = f.prepare();
    let old_key = sort_key(address.clone(), Vec3::ZERO);
    let current = sort_key(address, Vec3::new(0.0, 0.0, 2.0));
    let mut runtime = TransparentModelSortRuntime::default();
    assert!(
        stage_transparent_model_result(
            &mut runtime,
            TransparentModelWorkerResult {
                generation: ViewSortGeneration(1),
                key: old_key.clone(),
                batches: sort_transparent_model_candidates(Vec3::ZERO, candidates),
            },
            &current,
        )
        .is_none()
    );
    assert_eq!(runtime.staged.as_ref().unwrap().key, old_key);
}
