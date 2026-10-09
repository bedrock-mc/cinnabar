use super::*;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    })
}

/// Asserts that loading readiness reads the same counts the full diagnostic status reports.
fn assert_progress_matches_status(stream: &WorldStream, target: ViewCohort) -> CohortProgress {
    let progress = stream.cohort_progress(target);
    let status = stream.cohort_status(target);
    assert_eq!(progress, CohortProgress::from(status));
    assert_eq!(progress.target_is_complete(), status.target_is_complete());
    progress
}

// Loading checks readiness without the resident scan yet decides it exactly as the full status.
#[test]
fn cohort_progress_agrees_with_the_full_status() {
    let explicit = ViewCohort::from_publisher(0, [0, 64, 0], 16);
    let mut stream = stream();
    stream.publisher.cohort = Some(explicit);
    stream.publisher.required_columns = ViewCohort {
        publisher_geometry: None,
        ..explicit
    }
    .classifier_columns();
    stream.loaded_columns = stream.publisher.required_columns.clone();
    stream.loaded_columns.insert(ChunkKey::new(0, 9, 9));
    assert!(assert_progress_matches_status(&stream, explicit).target_is_complete());
    stream.loaded_columns.remove(&ChunkKey::new(0, 1, 0));
    let partial = assert_progress_matches_status(&stream, explicit);
    assert!(!partial.target_is_complete());
    assert_eq!(partial.loaded_target + 1, partial.expected);

    let classified = ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: 1,
        publisher_geometry: None,
    };
    stream.publisher.cohort = Some(classified);
    stream.loaded_columns = classified.classifier_columns();
    stream.loaded_columns.insert(ChunkKey::new(0, 2, 0));
    assert!(assert_progress_matches_status(&stream, classified).target_is_complete());
    let other = ViewCohort {
        center: [5, 5],
        ..classified
    };
    let uncommitted = assert_progress_matches_status(&stream, other);
    assert_eq!((uncommitted.expected, uncommitted.loaded_target), (0, 0));
}
