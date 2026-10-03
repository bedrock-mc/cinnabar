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
    while Instant::now() < deadline {
        // Exercise the production local worker/poll path. Unrelated border
        // meshes may still await their own neighbourhood; only the placed
        // cell's current generation establishes this contract.
        stream.poll(camera_position, usize::MAX);
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
            return changes;
        }
        std::thread::yield_now();
    }
    panic!("predicted mesh generation was not published before the bounded deadline");
}

/// Placement publishes a renderable mesh without any server acceptance event,
/// while a later authoritative air correction removes that prediction.
#[test]
fn predicted_placement_publishes_an_urgent_mesh_before_server_acceptance() {
    let mut stream = fixture();
    // Bootstrap the real mesh neighbourhood before input. A one-column fixture
    // still owes surrounding requested columns and must not bypass that gate.
    // No server event is admitted between prediction and its mesh publication.
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
