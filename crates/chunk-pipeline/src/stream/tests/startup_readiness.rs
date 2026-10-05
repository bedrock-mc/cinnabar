use super::*;
use client_world::PublisherViewGeometry;

#[path = "allocation_count.rs"]
mod allocation_count;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: RAW_IDS.air,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn startup_readiness_matches_committed_cohort_with_foreign_and_missing_columns() {
    let mut stream = stream();
    let explicit = ViewCohort::from_publisher(0, [0, 64, 0], 16);
    let targets = [
        explicit,
        ViewCohort {
            publisher_geometry: None,
            ..explicit
        },
        ViewCohort {
            dimension: 7,
            center: [i32::MAX, i32::MIN],
            radius: 2,
            publisher_geometry: None,
        },
        ViewCohort {
            radius: -1,
            publisher_geometry: None,
            ..explicit
        },
    ];
    for target in targets {
        stream.publisher.cohort = Some(target);
        stream.publisher.required_columns.clear();
        stream.loaded_columns.clear();
        assert!(!stream.startup_view_complete());
        assert_eq!(
            stream.startup_view_complete(),
            stream.cohort_status(target).target_is_complete()
        );
        stream.publisher.required_columns = target.classifier_columns();
        for key in stream.publisher.required_columns.clone() {
            stream.loaded_columns.insert(key);
            assert_eq!(
                stream.startup_view_complete(),
                stream.cohort_status(target).target_is_complete()
            );
        }
        assert!(stream.startup_view_complete());
        stream.loaded_columns.insert(ChunkKey::new(-7, 77, 77));
        stream.resident.insert(SubChunkKey::new(-7, 77, 0, 77));
        stream.known_air.insert(SubChunkKey::new(-7, 77, 1, 77));
        assert!(stream.startup_view_complete());
        assert!(!stream.cohort_status(target).is_exact());
        let missing = *stream.publisher.required_columns.first().unwrap();
        stream.loaded_columns.remove(&missing);
        assert!(!stream.startup_view_complete());
        stream.loaded_columns.insert(missing);
        assert!(stream.startup_view_complete());
    }
}

#[test]
fn unchanged_startup_readiness_has_no_heap_work_for_either_cohort_kind() {
    let mut stream = stream();
    for publisher_geometry in [
        Some(PublisherViewGeometry {
            center_blocks: [0, 0],
            radius_blocks: 16,
        }),
        None,
    ] {
        let target = ViewCohort {
            dimension: 0,
            center: [0, 0],
            radius: 1,
            publisher_geometry,
        };
        stream.publisher.cohort = Some(target);
        stream.publisher.required_columns = target.classifier_columns();
        stream.loaded_columns = stream.publisher.required_columns.clone();
        stream.known_air = (-128..128).map(|x| SubChunkKey::new(0, x, 8, 0)).collect();
        let before = allocation_count::thread_allocations();
        for _ in 0..8 {
            assert!(std::hint::black_box(&stream).startup_view_complete());
        }
        assert_eq!(allocation_count::thread_allocations() - before, 0);
        let before = allocation_count::thread_allocations();
        assert!(stream.cohort_status(target).target_is_complete());
        assert!(allocation_count::thread_allocations() > before);
    }
}
