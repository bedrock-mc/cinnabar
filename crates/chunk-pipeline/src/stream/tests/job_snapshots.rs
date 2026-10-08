use std::{hint::black_box, sync::Barrier};

use super::{
    light_scheduler::{install_current_light, lit_stream},
    *,
};

const SIDE: i32 = world::SUB_CHUNK_SIDE as i32;

/// Installs resident neighbours, packed light, biomes, and a higher seasonal column section.
fn fixture() -> (WorldStream, SubChunkKey) {
    let mut stream = lit_stream(1);
    let center = SubChunkKey::new(1, 0, 0, 0);
    for key in center
        .mesh_neighbourhood_dependents()
        .chain([SubChunkKey::new(1, 0, 3, 0)])
    {
        let section = if key == center {
            cave_test_slab(1)
        } else {
            uniform_sub_chunk(3)
        };
        stream.authority.commit_sub_chunk(key, section).unwrap();
        install_current_light(&mut stream, key, 7, 0, false);
        let revision = stream.lighting.store.light(key).unwrap().generation();
        let mut light = SubChunkLight::uniform(7, 0, revision).unwrap();
        light.set(LightChannel::Block, 1, 2, 3, 9).unwrap();
        stream.lighting.store.insert_resident(key, light);
    }
    for x in -1..=1 {
        for z in -1..=1 {
            stream.authority.commit_biome_column(
                ChunkKey::new(1, x, z),
                DecodedBiomeColumn::decode(-1, 5, &[1, 2, 255, 255, 255, 255], &RAW_BIOMES),
            );
        }
    }
    (stream, center)
}

/// Captures exactly the handles consumed by a production mesh worker.
fn capture_mesh(stream: &WorldStream, key: SubChunkKey) -> MeshSnapshot {
    stream.mesh_snapshot(
        key,
        stream.authority.terrain().sub_chunk(key).unwrap(),
        stream.mesh_light_halo(key).unwrap(),
    )
}

#[test]
fn unchanged_job_captures_allocate_nothing_and_share_every_section_payload() {
    let (stream, key) = fixture();
    let before = allocation_count::thread_allocations();
    for _ in 0..64 {
        black_box((
            stream.light_block_snapshot(key),
            stream.light_prior_snapshot(key),
            capture_mesh(&stream, key),
        ));
    }
    assert_eq!(allocation_count::thread_allocations() - before, 0);

    let blocks = stream.light_block_snapshot(key);
    let prior = stream.light_prior_snapshot(key);
    let mesh = capture_mesh(&stream, key);
    for sample_key in key.mesh_dependents() {
        let SnapshotBlock::Resident(captured) = blocks.blocks.get(&sample_key).unwrap() else {
            panic!("resident fixture");
        };
        assert!(Arc::ptr_eq(
            captured,
            &stream.authority.terrain().sub_chunk(sample_key).unwrap()
        ));
        assert!(Arc::ptr_eq(
            prior.light.light(sample_key).unwrap(),
            stream.lighting.store.light(sample_key).unwrap()
        ));
    }
    assert!(Arc::ptr_eq(
        &mesh.center,
        &stream.authority.terrain().sub_chunk(key).unwrap()
    ));
    assert!(Arc::ptr_eq(
        mesh.column.as_ref().unwrap(),
        &stream
            .authority
            .terrain()
            .chunk(key.chunk())
            .unwrap()
            .shared_sub_chunks()
    ));
    for [dx, dy, dz] in
        (-1_i8..=1).flat_map(|x| (-1_i8..=1).flat_map(move |y| (-1_i8..=1).map(move |z| [x, y, z])))
    {
        let sample_key = SubChunkKey::new(
            key.dimension,
            key.x + i32::from(dx),
            key.y + i32::from(dy),
            key.z + i32::from(dz),
        );
        if [dx, dy, dz] != [0; 3] {
            assert!(Arc::ptr_eq(
                mesh.adjacent[mesh_offset_index([dx, dy, dz])]
                    .as_ref()
                    .unwrap(),
                &stream.authority.terrain().sub_chunk(sample_key).unwrap()
            ));
        }
        assert!(Arc::ptr_eq(
            &mesh.light_halo.slots[mesh_offset_index([dx, dy, dz])]
                .as_ref()
                .unwrap()
                .light,
            stream.lighting.store.light(sample_key).unwrap()
        ));
        let biome_slot = ::meshing::biome_volume_index(dx, dy, dz).unwrap();
        assert!(Arc::ptr_eq(
            mesh.biomes[biome_slot].as_ref().unwrap(),
            &stream
                .authority
                .terrain()
                .biome_storage(sample_key)
                .unwrap()
        ));
    }
}

struct LegacySnapshot {
    key: SubChunkKey,
    blocks: BTreeMap<SubChunkKey, Arc<SubChunk>>,
    light_blocks: BTreeMap<SubChunkKey, Arc<SubChunk>>,
    light: BTreeMap<SubChunkKey, Arc<SubChunkLight>>,
    direct: BTreeMap<SubChunkKey, StoredDirectSky>,
    trusted: BTreeSet<SubChunkKey>,
    column_above: Vec<(i32, Arc<SubChunk>)>,
    assets: Arc<RuntimeAssets>,
    classifier: BlockClassifier,
    mode: NetworkIdMode,
}

impl LegacySnapshot {
    /// Materializes the former map and vector capture directly from world state.
    fn capture(stream: &WorldStream, key: SubChunkKey) -> Self {
        Self {
            key,
            blocks: key
                .mesh_neighbourhood_dependents()
                .filter_map(|key| {
                    stream
                        .authority
                        .terrain()
                        .sub_chunk(key)
                        .map(|value| (key, value))
                })
                .collect(),
            light_blocks: key
                .mesh_dependents()
                .filter_map(|key| {
                    stream
                        .authority
                        .terrain()
                        .sub_chunk(key)
                        .map(|value| (key, value))
                })
                .collect(),
            light: key
                .mesh_neighbourhood_dependents()
                .filter_map(|key| {
                    stream
                        .lighting
                        .store
                        .light(key)
                        .map(|value| (key, Arc::clone(value)))
                })
                .collect(),
            direct: key
                .mesh_dependents()
                .filter_map(|key| {
                    stream
                        .lighting
                        .direct_sky
                        .get(&key)
                        .cloned()
                        .map(|value| (key, value))
                })
                .collect(),
            trusted: key
                .mesh_dependents()
                .filter(|sample| *sample != key && stream.light_is_current(*sample))
                .collect(),
            column_above: stream
                .authority
                .terrain()
                .chunk(key.chunk())
                .unwrap()
                .sub_chunks()
                .filter_map(|(y, value)| {
                    y.checked_sub(key.y)
                        .filter(|offset| *offset >= 2)
                        .map(|offset| (offset, value))
                })
                .collect(),
            assets: Arc::clone(stream.authority.runtime_assets()),
            classifier: stream.classifier,
            mode: stream.network_id_mode(),
        }
    }

    /// Builds the old borrowed neighbourhood from individually collected section handles.
    fn mesh(&self) -> ChunkMesh {
        let mut neighbourhood = MeshNeighbourhood::new(&self.blocks[&self.key])
            .with_block_origin([self.key.x * SIDE, self.key.y * SIDE, self.key.z * SIDE]);
        for (&key, section) in &self.blocks {
            if key != self.key {
                assert!(neighbourhood.insert(
                    [
                        (key.x - self.key.x) as i8,
                        (key.y - self.key.y) as i8,
                        (key.z - self.key.z) as i8
                    ],
                    section
                ));
            }
        }
        for (offset, section) in &self.column_above {
            assert!(neighbourhood.insert_column_above(*offset, section));
        }
        ::meshing::mesh_sub_chunk_in_neighbourhood_with_lighting(
            &self.classifier,
            &self.assets,
            self.mode,
            &neighbourhood,
            &|local: [i32; 3]| {
                let position = BlockPos::new(
                    self.key.x * SIDE + local[0],
                    self.key.y * SIDE + local[1],
                    self.key.z * SIDE + local[2],
                );
                ::meshing::MeshLightSample::try_new(
                    self.read_light(self.key.dimension, position, LightChannel::Block),
                    self.read_light(self.key.dimension, position, LightChannel::Sky),
                )
                .unwrap()
            },
        )
    }
}

/// Splits world coordinates with the same negative-coordinate contract as section storage.
fn split(dimension: i32, position: BlockPos) -> (SubChunkKey, [u8; 3]) {
    (
        SubChunkKey::new(
            dimension,
            position.x.div_euclid(SIDE),
            position.y.div_euclid(SIDE),
            position.z.div_euclid(SIDE),
        ),
        [
            position.x.rem_euclid(SIDE) as u8,
            position.y.rem_euclid(SIDE) as u8,
            position.z.rem_euclid(SIDE) as u8,
        ],
    )
}

impl LightBlockAccess for LegacySnapshot {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        let (key, local) = split(self.key.dimension, position);
        self.light_blocks
            .get(&key)
            .map_or(LightBlockSample::Unknown, |section| {
                sample_resident_light(section, local, self.classifier, |id| {
                    let properties = self.assets.resolve(self.mode, id).light_properties();
                    SolverLightProperties::new(properties.emission(), properties.filter()).unwrap()
                })
            })
    }
}

impl LightReadAccess for LegacySnapshot {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        let (key, [x, y, z]) = split(dimension, position);
        self.light
            .get(&key)
            .and_then(|light| light.get(channel, x, y, z))
            .unwrap_or(0)
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        let (key, [x, y, z]) = split(dimension, position);
        self.direct.get(&key).is_some_and(|direct| {
            self.light
                .get(&key)
                .is_some_and(|light| direct.light_revision == light.generation())
                && direct.mask.get(x, y, z)
        })
    }

    fn boundary_light(
        &self,
        dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        let (key, _) = split(dimension, position);
        if !self.trusted.contains(&key) {
            return if self.light.contains_key(&key) {
                BoundaryLightSample::untrusted()
            } else {
                BoundaryLightSample::unknown()
            };
        }
        BoundaryLightSample::trusted(
            self.read_light(dimension, position, channel),
            channel == LightChannel::Sky && self.has_direct_sky_provenance(dimension, position),
        )
        .unwrap()
    }
}

#[test]
fn shared_inputs_match_map_and_vector_snapshot_results() {
    let (stream, key) = fixture();
    let legacy = LegacySnapshot::capture(&stream, key);
    let mesh = capture_mesh(&stream, key);
    assert_eq!(
        mesh.mesh(
            stream.classifier,
            stream.runtime_assets(),
            stream.network_id_mode()
        ),
        legacy.mesh()
    );
    let mut blocks = stream.light_block_snapshot(key);
    blocks.resolve_palette_light();
    let prior = stream.light_prior_snapshot(key);
    let bounds = LightBounds::new(
        key.dimension,
        BlockPos::new(0, 0, 0),
        BlockPos::new(SIDE - 1, SIDE - 1, SIDE - 1),
    )
    .unwrap();
    let expected = solve_light(
        &legacy,
        &legacy,
        bounds,
        9,
        DimensionLightProfile::Nether,
        LIGHT_SOLVE_LIMITS,
    )
    .unwrap();
    let actual = solve_light(
        &blocks,
        &prior,
        bounds,
        9,
        blocks.profile,
        LIGHT_SOLVE_LIMITS,
    )
    .unwrap();
    assert_eq!(actual.sub_chunks(), expected.sub_chunks());
    assert_eq!(
        DirectSkyMask::from_output(&actual, key),
        DirectSkyMask::from_output(&expected, key)
    );
    assert_eq!(actual.stats(), expected.stats());
}

/// Applies deterministic palette edits, preserving the same authority and invalidation path as packets.
fn edit_after_dispatch(stream: &mut WorldStream, center: SubChunkKey) {
    let mut random = 0x8ac7_4901_u32;
    for index in 0..32 {
        random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let key = if index % 3 == 0 {
            center
        } else {
            SubChunkKey::new(
                center.dimension,
                (random % 3) as i32 - 1,
                0,
                ((random >> 5) % 3) as i32 - 1,
            )
        };
        let update = if index == 0 {
            BlockUpdate::new(8, 0, 0, 0, 2)
        } else {
            BlockUpdate::new(
                (random & 15) as u8,
                ((random >> 4) & 15) as u8,
                ((random >> 8) & 15) as u8,
                0,
                (random >> 12) % 3,
            )
        };
        let mutation = ChunkStore::prepare_sub_chunk_blocks(
            key,
            stream.authority.terrain().sub_chunk(key).as_deref(),
            &[update],
            0,
        )
        .unwrap();
        assert!(stream.commit_block_mutations(vec![mutation]));
    }
    let light_generation = stream.lighting.store.light(center).unwrap().generation();
    stream.lighting.store.insert_resident(
        center,
        SubChunkLight::uniform(2, 0, light_generation + 1).unwrap(),
    );
    stream.authority.commit_biome_column(
        center.chunk(),
        DecodedBiomeColumn::decode(-1, 5, &[1, 4, 255, 255, 255, 255], &RAW_BIOMES),
    );
    let roof = SubChunkKey::new(center.dimension, center.x, center.y + 3, center.z);
    stream
        .authority
        .commit_sub_chunk(roof, uniform_sub_chunk(2))
        .unwrap();
    stream.mark_changed(roof, Instant::now());
}

#[test]
fn edits_while_jobs_wait_preserve_inputs_and_requeue_stale_completions() {
    let (mut stream, key) = fixture();
    let legacy = LegacySnapshot::capture(&stream, key);
    let expected_mesh = legacy.mesh();
    let mesh = capture_mesh(&stream, key);
    let light_revision = stream.mark_light_dirty_exact(key).unwrap();
    let identity = LightJobIdentity {
        revision: light_revision,
        block_generation: stream.lighting.block_generations[&key],
        previous_light_generation: stream
            .lighting
            .store
            .light(key)
            .map(|light| light.generation()),
        batch_id: 0,
        urgent: false,
    };
    stream.lighting.jobs.pending.remove(&key);
    stream.lighting.jobs.in_flight.insert(key, identity);
    let bounds = LightBounds::new(
        key.dimension,
        BlockPos::new(0, 0, 0),
        BlockPos::new(SIDE - 1, SIDE - 1, SIDE - 1),
    )
    .unwrap();
    let expected_light = solve_light(
        &legacy,
        &legacy,
        bounds,
        identity.revision,
        DimensionLightProfile::Nether,
        LIGHT_SOLVE_LIMITS,
    )
    .unwrap();
    let job = PreparedLightJob {
        key,
        identity,
        blocks: stream.light_block_snapshot(key),
        prior: stream.light_prior_snapshot(key),
        bounds,
        queued_at: Instant::now(),
    };
    let revision = stream.mark_dirty_exact(key, Instant::now());
    stream.mesh_jobs.pending.remove(&key);
    stream.mesh_jobs.in_flight.insert(key, revision);
    let classifier = stream.classifier;
    let assets = Arc::clone(stream.authority.runtime_assets());
    let mode = stream.network_id_mode();
    let tint_identity = stream.biome_tint_identity();
    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = Arc::clone(&barrier);
    let worker = std::thread::spawn(move || {
        worker_barrier.wait();
        let output = mesh.mesh(classifier, &assets, mode);
        let light = solve_prepared_light_job(job).unwrap();
        (mesh, output, light)
    });
    edit_after_dispatch(&mut stream, key);
    let newest_light = Arc::clone(stream.lighting.store.light(key).unwrap());
    let current_revision = stream.mesh_jobs.pending[&key].revision;
    let current_light_revision = stream.lighting.jobs.pending[&key].revision;
    barrier.wait();
    let (snapshot, mesh, light) = worker.join().unwrap();
    assert_eq!(mesh, expected_mesh);
    assert_eq!(
        &light.replacement,
        expected_light.sub_chunks()[&key].as_ref()
    );
    assert_eq!(
        *light.direct_sky,
        DirectSkyMask::from_output(&expected_light, key)
    );
    assert!(!Arc::ptr_eq(
        &snapshot.center,
        &stream.authority.terrain().sub_chunk(key).unwrap()
    ));
    let captured_roof = snapshot
        .neighbourhood()
        .seasonal_column()
        .find(|(offset, _)| *offset == 3)
        .unwrap()
        .1
        .runtime_id(0, 0, 0, 0);
    assert_eq!(captured_roof, Some(3));
    let biome_slot = ::meshing::biome_volume_index(0, 0, 0).unwrap();
    assert_eq!(
        snapshot.biomes[biome_slot]
            .as_ref()
            .unwrap()
            .biome_id(0, 0, 0),
        Some(1)
    );
    assert_eq!(
        stream
            .authority
            .terrain()
            .biome_storage(key)
            .unwrap()
            .biome_id(0, 0, 0),
        Some(2)
    );
    assert_eq!(snapshot.light_halo.sample_channels([1, 2, 3]), [9, 0]);
    assert_eq!(newest_light.get(LightChannel::Block, 1, 2, 3), Some(2));
    stream.accept_light_completion(LightCompletion {
        key,
        identity,
        result: Ok(light),
        queue_wait: Duration::ZERO,
        duration: Duration::ZERO,
    });
    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
        key,
        revision,
        source: snapshot.center,
        biome_sources: snapshot.biomes,
        biome: PackedBiomeRecord::fallback(),
        tint_identity,
        mesh,
        dependency_mask: MeshDependencyMask::default(),
        light_halo: snapshot.light_halo,
        queue_wait: Duration::ZERO,
        dispatch_wait: Duration::ZERO,
        duration: Duration::ZERO,
        urgent: false,
    });
    assert_eq!(stream.stats().stale_light_jobs, 1);
    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.take_mesh_changes().is_empty());
    assert!(Arc::ptr_eq(
        stream.lighting.store.light(key).unwrap(),
        &newest_light
    ));
    assert_eq!(stream.mesh_jobs.pending[&key].revision, current_revision);
    assert_eq!(
        stream.lighting.jobs.pending[&key].revision,
        current_light_revision
    );
    assert!(!stream.mesh_jobs.in_flight.contains_key(&key));
    assert!(!stream.lighting.jobs.in_flight.contains_key(&key));
}

#[test]
fn batched_dispatch_allocations_are_bounded_by_jobs_and_queue_growth() {
    const JOBS: usize = 64;
    let (stream, key) = fixture();
    let pool = &*workers::WORKERS;
    let (done, completed) = crossbeam_channel::bounded(JOBS);
    let before = allocation_count::thread_allocations();
    let mut batch = pool.batch(workers::Lane::Mesh);
    for _ in 0..JOBS {
        let inputs = (
            stream.light_block_snapshot(key),
            stream.light_prior_snapshot(key),
            capture_mesh(&stream, key),
        );
        let done = done.clone();
        batch.spawn(move || {
            drop(black_box(inputs));
            let _ = done.send(());
        });
    }
    drop(batch);
    let allocations = allocation_count::thread_allocations() - before;
    assert!(
        allocations <= JOBS as u64 + 16,
        "{allocations} allocations for {JOBS} jobs"
    );
    for _ in 0..JOBS {
        completed.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
