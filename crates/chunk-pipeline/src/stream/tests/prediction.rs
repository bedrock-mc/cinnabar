use super::*;

fn column_payload() -> Vec<u8> {
    let mut payload = vec![9, 1, (-4_i8) as u8, 1, 0];
    payload.extend(biome_payload(0, 1));
    payload
}

fn fixture() -> WorldStream {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 2,
            block_network_ids_are_hashes: false,
        },
        Arc::new(non_default_air_runtime_assets()),
        [0.0; 3],
        None,
    );
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: column_payload(),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream
}

fn server_update(stream: &mut WorldStream, sequence: u64, position: [i32; 3], network_id: u32) {
    stream
        .submit(
            sequence,
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position,
                layer: 0,
                network_id,
            }]),
        )
        .unwrap();
}

fn block(stream: &WorldStream, [x, y, z]: [i32; 3]) -> Option<u32> {
    stream
        .collision_store()
        .sub_chunk(SubChunkKey::new(
            0,
            x.div_euclid(16),
            y.div_euclid(16),
            z.div_euclid(16),
        ))
        .and_then(|sub_chunk| {
            sub_chunk.runtime_id(
                0,
                x.rem_euclid(16) as u8,
                y.rem_euclid(16) as u8,
                z.rem_euclid(16) as u8,
            )
        })
}

fn publish_local_mesh_work(
    stream: &mut WorldStream,
    camera_position: [f32; 3],
    target: SubChunkKey,
    generation: u64,
) -> Vec<WorldMeshChange> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut changes = Vec::new();
    let mut polls = 0;
    let mut mesh_dispatches = 0;
    let mut mesh_results = 0;
    let mut decode_results = 0;
    while Instant::now() < deadline {
        // Exercise the production local worker/poll path. Unrelated border
        // meshes may still await their own neighbourhood; only the placed
        // cell's current generation establishes this contract.
        let work = stream.poll(camera_position, usize::MAX);
        polls += 1;
        mesh_dispatches += work.mesh_jobs_dispatched;
        mesh_results += work.mesh_results;
        decode_results += work.decoded_results;
        changes.extend(stream.take_mesh_changes());
        if changes.iter().any(|change| match change {
            WorldMeshChange::Upsert {
                key,
                generation: actual,
                ..
            }
            | WorldMeshChange::Remove {
                key,
                generation: actual,
                ..
            } => *key == target && *actual == generation,
        }) {
            eprintln!(
                "local mesh publication: polls={polls}, mesh_dispatches={mesh_dispatches}, mesh_results={mesh_results}, decode_results={decode_results}"
            );
            return changes;
        }
        std::thread::yield_now();
    }
    panic!("predicted mesh generation was not published before the bounded deadline");
}

/// A server-selected alternative state replaces a prediction without acceptance bookkeeping.
#[test]
fn a_different_server_state_replaces_a_predicted_state() {
    let mut stream = fixture();
    let position = [3, 200, 3];
    assert!(stream.predict_block(position, 0, 1));
    assert_eq!(block(&stream, position), Some(1));
    server_update(&mut stream, 2, position, 0);
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(block(&stream, position), Some(0));
}

/// Exercises real non-cube model publication when a local world carrier is available.
#[test]
fn placed_family_models_publish_urgent_meshes_without_server_acceptance() {
    let Some(path) = std::env::var_os("CINNABAR_TEST_WORLD_CARRIER") else {
        eprintln!(
            "missing fixture: CINNABAR_TEST_WORLD_CARRIER; skipping real placement model publication"
        );
        return;
    };
    let assets = Arc::new(RuntimeAssets::decode(&std::fs::read(path).unwrap()).unwrap());
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let air = assets.air_network_id(NetworkIdMode::Sequential).unwrap();
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: air,
            block_network_ids_are_hashes: false,
        },
        assets,
        [0.0; 3],
        None,
    );
    for (index, (x, z)) in (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| (x, z)))
        .enumerate()
    {
        stream
            .submit(
                index as u64 + 1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x,
                    z,
                    mode: LevelChunkMode::Inline { count: 1 },
                    payload: column_payload(),
                }),
            )
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    for name in [
        "minecraft:oak_log",
        "minecraft:oak_slab",
        "minecraft:trapdoor",
        "minecraft:hopper",
        "minecraft:torch",
        "minecraft:stone_button",
        "minecraft:lantern",
        "minecraft:white_carpet",
        "minecraft:oak_fence",
        "minecraft:glass_pane",
        "minecraft:snow_layer",
    ] {
        let record = records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap();
        let position: [i32; 3] = [3, 200, 3];
        let key = SubChunkKey::new(0, 0, position[1].div_euclid(16), 0);
        assert!(stream.predict_block(position, 0, record.sequential_id));
        assert_eq!(block(&stream, position), Some(record.sequential_id));
        let generation = stream.revisions.dirty(key).unwrap().revision;
        let changes = publish_local_mesh_work(
            &mut stream,
            position.map(|axis| axis as f32),
            key,
            generation,
        );
        assert!(
            changes.iter().any(|change| matches!(change,
            WorldMeshChange::Upsert { key: changed, generation: actual, urgent: true, .. }
                if *changed == key && *actual == generation)),
            "{name}"
        );
        eprintln!("predicted family model: {name}; server acceptance events=0");
    }
}

/// Placement publishes a renderable mesh without any server acceptance event,
/// while a later authoritative air correction removes that prediction.
#[test]
fn predicted_placement_publishes_an_urgent_mesh_before_server_acceptance() {
    // Bootstrap the real mesh neighbourhood before input. A one-column fixture
    // still owes surrounding requested columns and must not bypass that gate.
    // No server event is admitted between prediction and its mesh publication.
    let (mut stream, next_sequence) = loaded_neighbourhood();
    let position: [i32; 3] = [3, 200, 3];
    let key = SubChunkKey::new(0, 0, position[1].div_euclid(16), 0);
    let camera_position = position.map(|axis| axis as f32);
    let cube = 1;
    let air = stream.air_block_id();
    assert!(stream.known_air.contains(&key));
    let collision_before = stream.collision_store().collision_revision(key.chunk());

    assert!(stream.predict_block(position, 0, cube));
    assert_eq!(block(&stream, position), Some(cube));
    assert_ne!(
        stream.collision_store().collision_revision(key.chunk()),
        collision_before,
        "placement changes collision authority immediately"
    );
    let predicted_generation = stream.revisions.dirty(key).unwrap().revision;

    // Only local worker completions are processed here: there is no following
    // server event that could accept or repeat the placed block.
    let changes = publish_local_mesh_work(&mut stream, camera_position, key, predicted_generation);
    let (mesh, dirty_since) = changes
        .iter()
        .find_map(|change| match change {
            WorldMeshChange::Upsert {
                key: changed,
                mesh,
                generation,
                dirty_since,
                urgent,
                ..
            } if *changed == key && *generation == predicted_generation => {
                assert!(
                    *urgent,
                    "predicted placement bypasses ordinary mesh priority"
                );
                Some((mesh, *dirty_since))
            }
            _ => None,
        })
        .expect("local prediction publishes its own render mesh");
    assert!(!mesh.cube_quads().is_empty());
    stream.acknowledge_mesh_upload(key, predicted_generation, dirty_since, Instant::now());
    assert!(stream.is_mesh_clean(key));

    server_update(&mut stream, next_sequence, position, air);
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        block(&stream, position),
        None,
        "the cell is authoritative known air"
    );
    assert!(stream.known_air.contains(&key));
    let correction_generation = stream.revisions.dirty(key).unwrap().revision;
    assert_ne!(correction_generation, predicted_generation);
    let correction =
        publish_local_mesh_work(&mut stream, camera_position, key, correction_generation);
    assert!(
        correction.iter().any(|change| matches!(
            change,
            WorldMeshChange::Remove {
                key: removed,
                generation,
                urgent: true,
                ..
            } if *removed == key && *generation == correction_generation
        )),
        "the authoritative correction removes the predicted render mesh"
    );
}

/// A prediction lands at once and the server's later word replaces it.
#[test]
fn a_prediction_commits_immediately_and_a_server_update_overrides_it() {
    let mut stream = fixture();
    let air = stream.air_block_id();
    assert!(stream.predict_block([3, -64, 3], 0, air));
    assert_eq!(block(&stream, [3, -64, 3]), Some(air));
    server_update(&mut stream, 2, [3, -64, 3], 0);
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        block(&stream, [3, -64, 3]),
        Some(0),
        "the server rolls it back"
    );
}

#[test]
fn a_prediction_outside_authoritative_data_is_refused() {
    let mut stream = fixture();
    let air = stream.air_block_id();
    assert!(!stream.predict_block([40, -64, 3], 0, air));
    assert!(!stream.predict_block([3, 400, 3], 0, air));
    assert!(
        stream.predict_block([3, 200, 3], 0, 1),
        "a loaded column is known air"
    );
}

/// A server batch received before the prediction must not erase it on commit.
#[test]
fn a_prediction_survives_an_in_flight_batch_received_before_it() {
    let mut stream = fixture();
    let air = stream.air_block_id();
    server_update(&mut stream, 2, [5, -64, 5], 1);
    assert!(stream.predict_block([3, -64, 3], 0, air));
    assert!(stream.predict_block([5, -64, 5], 0, air));
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(block(&stream, [3, -64, 3]), Some(air));
    assert_eq!(block(&stream, [5, -64, 5]), Some(air));
    server_update(&mut stream, 3, [5, -64, 5], 1);
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(block(&stream, [5, -64, 5]), Some(1), "a later batch wins");
}

fn loaded_neighbourhood() -> (WorldStream, u64) {
    let mut stream = fixture();
    let mut next_sequence = 2;
    for x in -1..=1 {
        for z in -1..=1 {
            if x == 0 && z == 0 {
                continue;
            }
            stream
                .submit(
                    next_sequence,
                    WorldEvent::LevelChunk(LevelChunkEvent {
                        dimension: 0,
                        x,
                        z,
                        mode: LevelChunkMode::Inline { count: 1 },
                        payload: column_payload(),
                    }),
                )
                .unwrap();
            next_sequence += 1;
        }
    }
    complete_pending_decode_jobs(&mut stream);
    (stream, next_sequence)
}

fn poll_until_queued(
    stream: &mut WorldStream,
    camera_position: [f32; 3],
    queued: impl Fn(&WorldMeshChange) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !stream.mesh_changes.iter().any(&queued) {
        assert!(Instant::now() < deadline, "the mesh change was not queued");
        stream.poll(camera_position, usize::MAX);
        std::thread::yield_now();
    }
}

/// Drains the queue like the renderer (last write wins per key, each acknowledged) and
/// returns the generation and upsert flag `key` ends with.
fn present_queued_changes(stream: &mut WorldStream, key: SubChunkKey) -> Option<(u64, bool)> {
    let mut last = None;
    while let Some(change) = stream.pop_mesh_change() {
        let (changed, generation, dirty_since, is_upsert) = match &change {
            WorldMeshChange::Upsert {
                key,
                generation,
                dirty_since,
                ..
            } => (*key, *generation, *dirty_since, true),
            WorldMeshChange::Remove {
                key,
                generation,
                dirty_since,
                ..
            } => (*key, *generation, *dirty_since, false),
        };
        stream.acknowledge_mesh_upload(changed, generation, dirty_since, Instant::now());
        if changed == key {
            last = Some((generation, is_upsert));
        }
    }
    last
}

/// Leaves a known-air removal queued, then places a block there and returns its generation.
fn place_behind_queued_removal(invalidate_tints: bool) -> (WorldStream, SubChunkKey, u64) {
    let (mut stream, next_sequence) = loaded_neighbourhood();
    let position: [i32; 3] = [3, 200, 3];
    let key = SubChunkKey::new(0, 0, position[1].div_euclid(16), 0);
    let camera_position = position.map(|axis| axis as f32);
    assert!(stream.known_air.contains(&key));
    poll_until_queued(
        &mut stream,
        camera_position,
        |change| matches!(change, WorldMeshChange::Remove { key: removed, .. } if *removed == key),
    );

    server_update(&mut stream, next_sequence, position, 1);
    complete_pending_decode_jobs(&mut stream);
    if invalidate_tints {
        stream.invalidate_resident_biome_tints(Instant::now());
    }
    let generation = stream.revisions.dirty(key).unwrap().revision;
    poll_until_queued(&mut stream, camera_position, |change| {
        matches!(change, WorldMeshChange::Upsert { key: changed, generation: g, .. }
            if *changed == key && *g == generation)
    });
    let stages = stream.stats.phase2_stages;
    assert_eq!(
        stages.mesh_changes_queued - stages.mesh_changes_dequeued,
        stream.mesh_changes.len() as u64,
        "a superseded change leaves the queue accounting balanced"
    );
    (stream, key, generation)
}

/// An urgent remesh must not be overtaken by an older removal still queued for the key.
#[test]
fn urgent_upsert_is_not_overtaken_by_an_older_queued_removal() {
    let (mut stream, key, generation) = place_behind_queued_removal(false);
    assert_eq!(
        present_queued_changes(&mut stream, key),
        Some((generation, true))
    );
    assert!(stream.is_mesh_clean(key));
}

/// Biome tint invalidation keeps queued removals; they must still yield to the newer mesh.
#[test]
fn tint_invalidation_keeps_a_queued_removal_behind_the_newer_mesh() {
    let (mut stream, key, generation) = place_behind_queued_removal(true);
    assert_eq!(
        present_queued_changes(&mut stream, key),
        Some((generation, true))
    );
    assert!(stream.is_mesh_clean(key));
}

/// A newer urgent removal must not be overtaken by an older urgent mesh, leaving a ghost block.
#[test]
fn urgent_removal_is_not_overtaken_by_an_older_queued_upsert() {
    let (mut stream, next_sequence) = loaded_neighbourhood();
    let position: [i32; 3] = [3, 200, 3];
    let key = SubChunkKey::new(0, 0, position[1].div_euclid(16), 0);
    let camera_position = position.map(|axis| axis as f32);
    assert!(stream.predict_block(position, 0, 1));
    let predicted = stream.revisions.dirty(key).unwrap().revision;
    poll_until_queued(&mut stream, camera_position, |change| {
        matches!(change, WorldMeshChange::Upsert { key: changed, generation, .. }
            if *changed == key && *generation == predicted)
    });

    let air = stream.air_block_id();
    server_update(&mut stream, next_sequence, position, air);
    complete_pending_decode_jobs(&mut stream);
    let corrected = stream.revisions.dirty(key).unwrap().revision;
    poll_until_queued(&mut stream, camera_position, |change| {
        matches!(change, WorldMeshChange::Remove { key: removed, generation, .. }
            if *removed == key && *generation == corrected)
    });

    assert_eq!(
        present_queued_changes(&mut stream, key),
        Some((corrected, false))
    );
    assert!(stream.is_mesh_clean(key));
}

/// A missing second cell leaves the valid first cell untouched.
#[test]
fn paired_prediction_rejects_every_cell_when_one_is_unloaded() {
    let mut stream = fixture();
    let first = [3, 200, 3];
    let original = block(&stream, first);
    assert!(!stream.predict_blocks(&[(first, 0, 1), ([40, 200, 3], 0, 1)]));
    assert_eq!(block(&stream, first), original);
}

/// A late handoff can publish completed prediction workers without admitting another server batch.
#[test]
fn prediction_workers_publish_without_a_second_world_poll() {
    let (mut stream, _) = loaded_neighbourhood();
    let position: [i32; 3] = [3, 200, 3];
    let key = SubChunkKey::new(0, 0, position[1].div_euclid(16), 0);
    assert!(stream.predict_block(position, 0, 1));
    let generation = stream.prediction_generation(position).unwrap().1;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        stream.poll_prediction_jobs(position.map(|coordinate| coordinate as f32), 2);
        if stream.take_mesh_changes().into_iter().any(|change| {
            matches!(change,
            WorldMeshChange::Upsert {key: actual, generation: revision, urgent: true, ..}
                if actual == key && revision == generation)
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "prediction mesh did not reach the late handoff"
        );
        std::thread::yield_now();
    }
}

/// Both cells become readable together, and a server rollback replaces the whole pair.
#[test]
fn predicted_door_pair_commits_and_server_correction_rolls_back_both_cells() {
    let mut stream = fixture();
    let cells = [[3, 207, 3], [3, 208, 3]];
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let halves = [0, 1].map(|upper| {
        records
            .iter()
            .find(|record| {
                if record.name.as_ref() != "minecraft:wooden_door" {
                    return false;
                }
                let state: serde_json::Value =
                    serde_json::from_str(&record.canonical_state).unwrap();
                state["upper_block_bit"]["value"] == upper
                    && state["minecraft:cardinal_direction"]["value"] == "west"
                    && state["open_bit"]["value"] == 0
                    && state["door_hinge_bit"]["value"] == 0
            })
            .unwrap()
            .sequential_id
    });
    assert!(stream.predict_blocks(&[(cells[0], 0, halves[0]), (cells[1], 0, halves[1])]));
    assert_eq!(block(&stream, cells[0]), Some(halves[0]));
    assert_eq!(block(&stream, cells[1]), Some(halves[1]));
    let air = stream.air_block_id();
    stream
        .submit(
            2,
            WorldEvent::BlockUpdates(
                cells
                    .into_iter()
                    .map(|position| BlockUpdateEvent {
                        dimension: 0,
                        position,
                        layer: 0,
                        network_id: air,
                    })
                    .collect(),
            ),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    for cell in cells {
        let key = SubChunkKey::new(0, 0, cell[1].div_euclid(16), 0);
        assert!(stream.collision_store().is_sub_chunk_loaded(key));
        assert_eq!(block(&stream, cell).unwrap_or(air), air);
    }
}

/// The late worker handoff leaves authoritative corrections for the ordered world poll.
#[test]
fn prediction_handoff_does_not_commit_queued_server_corrections() {
    let mut stream = fixture();
    let position = [0, -64, 0];
    assert!(stream.predict_block(position, 0, 0));
    stream.submit(2, worker_block_batch(0, 1)).unwrap();
    let job = stream.pending_decode.pop_front().unwrap();
    stream.decode_tx.send(job.job.run(Instant::now())).unwrap();
    stream.poll_prediction_jobs(position.map(|coordinate| coordinate as f32), 2);
    assert_eq!(block(&stream, position), Some(0));
    stream.poll(position.map(|coordinate| coordinate as f32), usize::MAX);
    assert_eq!(block(&stream, position), Some(1));
}

/// Settles startup work around the fixture's air section so only a new change remains.
fn settled_neighbourhood() -> (WorldStream, u64, [i32; 3], SubChunkKey, [f32; 3]) {
    let (mut stream, next_sequence) = loaded_neighbourhood();
    let position: [i32; 3] = [3, 200, 3];
    let key = SubChunkKey::new(0, 0, position[1].div_euclid(16), 0);
    let camera_position = position.map(|axis| axis as f32);
    let deadline = Instant::now() + Duration::from_secs(20);
    // Border sections may wait on columns the fixture never sends; settled means nothing
    // is in flight and a poll finds nothing more to start.
    loop {
        let report = stream.poll(camera_position, usize::MAX);
        present_queued_changes(&mut stream, key);
        let idle = stream.lighting.jobs.in_flight.is_empty()
            && stream.mesh_jobs.in_flight.is_empty()
            && stream.mesh_changes.is_empty()
            && report.light_jobs_dispatched == 0
            && report.mesh_jobs_dispatched == 0
            && report.light_results == 0
            && report.mesh_results == 0;
        if idle {
            break;
        }
        assert!(Instant::now() < deadline, "fixture work did not settle");
        std::thread::yield_now();
    }
    (stream, next_sequence, position, key, camera_position)
}

/// Waits for an urgent worker result; false once no urgent work is in flight.
fn await_urgent_result(stream: &WorldStream) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let urgent_in_flight = !stream.urgent_mesh_in_flight.is_empty()
            || stream
                .lighting
                .jobs
                .in_flight
                .values()
                .any(|identity| identity.urgent);
        if !urgent_in_flight {
            return false;
        }
        if !stream.lighting.rx.is_empty() || !stream.mesh_rx.is_empty() {
            return true;
        }
        assert!(Instant::now() < deadline, "urgent work did not finish");
        std::thread::yield_now();
    }
}

/// Counts polls until `key`'s `generation` pops for presentation, servicing urgent results
/// between polls the way the frame does before world publication.
fn polls_until_published(
    stream: &mut WorldStream,
    camera_position: [f32; 3],
    key: SubChunkKey,
    generation: u64,
) -> usize {
    let mut polls = 0;
    loop {
        if present_queued_changes(stream, key).is_some_and(|(published, _)| published == generation)
        {
            return polls;
        }
        if await_urgent_result(stream) {
            stream.service_urgent_work();
        } else {
            assert!(polls < 8, "the change never published");
            stream.poll(camera_position, usize::MAX);
            polls += 1;
        }
    }
}

/// A single server block update commits on arrival and its mesh publishes within one poll.
#[test]
fn a_single_block_update_publishes_its_mesh_within_one_poll() {
    let (mut stream, next_sequence, position, key, camera_position) = settled_neighbourhood();
    server_update(&mut stream, next_sequence, position, 1);
    assert_eq!(stream.order.blocking_block_updates(), None);
    assert!(
        stream.pending_decode.is_empty(),
        "a one-section batch skips the worker hop"
    );
    assert_eq!(block(&stream, position), Some(1));
    let generation = stream.revisions.dirty(key).unwrap().revision;
    let polls = polls_until_published(&mut stream, camera_position, key, generation);
    assert!(polls <= 1, "published after {polls} polls");
}

/// A prediction dispatches its urgent work at once, before any poll.
#[test]
fn a_prediction_dispatches_its_work_without_a_poll() {
    let (mut stream, _, position, key, camera_position) = settled_neighbourhood();
    let stages = stream.stats.phase2_stages;
    assert!(stream.predict_block(position, 0, 1));
    let dispatched = stream.stats.phase2_stages;
    assert!(
        dispatched.light_jobs_dispatched + dispatched.mesh_jobs_dispatched
            > stages.light_jobs_dispatched + stages.mesh_jobs_dispatched
    );
    let generation = stream.revisions.dirty(key).unwrap().revision;
    assert_eq!(
        polls_until_published(&mut stream, camera_position, key, generation),
        0
    );
}

/// Ingress decode starts on submit rather than at the next poll.
#[test]
fn ingress_decode_is_in_flight_before_any_poll() {
    let mut stream = fixture();
    stream
        .submit(
            2,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 1,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: column_payload(),
            }),
        )
        .unwrap();
    stream.dispatch_ingress_decode();
    assert!(stream.pending_decode.is_empty());
    assert_eq!(stream.in_flight_decode_jobs, 1);
    let completion = stream
        .decode_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    stream.accept_decode_completion(completion);
    stream.apply_ready();
    assert_eq!(stream.committed_sequence(), 2);
}

/// The urgent pass leaves a non-urgent staged backlog to the poll, yet the urgent mesh still
/// publishes in the same frame.
#[test]
fn urgent_pass_leaves_a_staged_backlog_for_the_poll() {
    let (mut stream, _, position, key, camera_position) = settled_neighbourhood();
    let source = Arc::new(uniform_sub_chunk(1));
    let biome_sources: super::BiomeNeighbourhood = std::array::from_fn(|_| None);
    let backlog = super::MAX_STAGED_MESH_COMPLETIONS;
    for index in 0..backlog {
        let biome =
            super::pack_biome_record(&biome_sources, stream.authority.resolved_biome_tints());
        let mesh = ChunkMesh::default();
        stream.staged_mesh_bytes += chunk_publication_byte_len(&mesh, &biome);
        stream.staged_mesh_completions.push_back(MeshCompletion {
            output_permit: None,
            _job_permit: None,
            key: SubChunkKey::new(0, 1_000 + index as i32, 0, 0),
            revision: 0,
            source: Arc::clone(&source),
            biome_sources: biome_sources.clone(),
            biome,
            tint_identity: stream.biome_tint_identity(),
            mesh,
            dependency_mask: MeshDependencyMask::default(),
            light_halo: Default::default(),
            queue_wait: Duration::ZERO,
            dispatch_wait: Duration::ZERO,
            duration: Duration::ZERO,
            urgent: false,
        });
    }
    assert!(stream.predict_block(position, 0, 1));
    let generation = stream.revisions.dirty(key).unwrap().revision;
    assert_eq!(
        polls_until_published(&mut stream, camera_position, key, generation),
        0
    );
    assert_eq!(stream.staged_mesh_completions.len(), backlog);
}

/// A prediction outside a decoding server batch is not replayed over newer server terrain
/// that committed past that batch.
#[test]
fn a_prediction_outside_a_decoding_batch_yields_to_newer_server_terrain() {
    let mut stream = fixture();
    let column = |x| {
        WorldEvent::LevelChunk(LevelChunkEvent {
            dimension: 0,
            x,
            z: 0,
            mode: LevelChunkMode::Inline { count: 1 },
            payload: column_payload(),
        })
    };
    stream.submit(2, column(1)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.submit(3, worker_block_batch(0, 1)).unwrap();
    assert_eq!(stream.order.blocking_block_updates(), Some(3));
    let cell = [19, -64, 3];
    let air = stream.air_block_id();
    assert!(stream.predict_block(cell, 0, air));
    stream.submit(4, column(1)).unwrap();
    let index = stream
        .pending_decode
        .iter()
        .position(|queued| matches!(queued.job, super::DecodeJob::InlineLevelChunk { .. }))
        .expect("the newer column is queued");
    let job = stream.pending_decode.remove(index).unwrap();
    complete_decode_job(&mut stream, job);
    stream.apply_ready();
    assert_eq!(
        block(&stream, cell),
        Some(0),
        "the server column replaced it"
    );
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        block(&stream, cell),
        Some(0),
        "finishing the batch keeps it"
    );
}
