//! Columns past the full-detail radius keep their blocks but skip lighting and meshing.

use super::*;

fn stream() -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [8.0, 0.0, 8.0],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.set_far_terrain_enabled(true);
    stream
}

fn meshing(stream: &WorldStream, x: i32) -> bool {
    stream
        .mesh_jobs
        .pending
        .keys()
        .any(|key| key.chunk() == ChunkKey::new(0, x, 0))
}

/// A far column is stored and summarised but not meshed until the radius reaches it, and a
/// shrinking radius withdraws it again.
#[test]
fn far_columns_mesh_only_inside_the_full_detail_radius() {
    let mut stream = stream();
    stream.set_full_detail_radius(Some(1));
    for (sequence, x) in [(1, 0), (2, 5)] {
        stream
            .submit(sequence, inline_block_entity_event(x, 7, Vec::new()))
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    assert!(meshing(&stream, 0), "the near column meshes");
    assert!(!meshing(&stream, 5), "the far column waits");
    assert!(
        stream
            .authority
            .terrain()
            .chunk(ChunkKey::new(0, 5, 0))
            .is_some(),
        "its blocks are kept"
    );

    stream.set_full_detail_radius(Some(6));
    assert!(meshing(&stream, 5), "a wider radius promotes it");

    stream.mesh_jobs.pending.clear();
    stream.set_full_detail_radius(Some(1));
    assert!(meshing(&stream, 5), "a narrower one queues its removal");
    assert!(!stream.wants_mesh(ChunkKey::new(0, 5, 0)));
}

/// Without a radius every column meshes, as before far terrain.
#[test]
fn no_radius_meshes_everything() {
    let mut stream = stream();
    stream
        .submit(1, inline_block_entity_event(5, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(meshing(&stream, 5));
    assert_eq!(stream.full_detail_radius(), None);
}

fn drawn(stream: &WorldStream, x: i32) -> bool {
    stream
        .near_drawn_columns()
        .1
        .any(|column| column == ChunkKey::new(0, x, 0))
}

/// Uploads every mesh waiting in `column`.
fn upload(stream: &mut WorldStream, x: i32) {
    let column = ChunkKey::new(0, x, 0);
    let keys: Vec<SubChunkKey> = stream
        .resident
        .column(column)
        .chain(stream.known_air.column(column))
        .copied()
        .collect();
    let now = Instant::now();
    for key in keys {
        if let Some((revision, since)) = stream
            .revisions
            .dirty(key)
            .map(|dirty| (dirty.revision, dirty.since))
        {
            stream.acknowledge_mesh_upload(key, revision, since, now);
        }
    }
}

/// Limiting detail after running without a limit withdraws meshes already past the radius.
#[test]
fn a_new_limit_demotes_columns_meshed_without_one() {
    let mut stream = stream();
    for (sequence, x) in [(1, 0), (2, 5)] {
        stream
            .submit(sequence, inline_block_entity_event(x, 7, Vec::new()))
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    upload(&mut stream, 5);
    assert!(drawn(&stream, 5));

    stream.set_full_detail_radius(Some(1));
    assert!(!stream.wants_mesh(ChunkKey::new(0, 5, 0)));
    assert!(!drawn(&stream, 5), "far terrain takes the column back");
    assert!(meshing(&stream, 5), "its meshes are withdrawn");
    assert!(stream.wants_mesh(ChunkKey::new(0, 0, 0)));
}

/// A promoted column stays with far terrain until its new meshes upload.
#[test]
fn promotion_hands_over_only_after_upload() {
    let mut stream = stream();
    stream.set_full_detail_radius(Some(1));
    stream
        .submit(1, inline_block_entity_event(5, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    upload(&mut stream, 5);
    assert!(!drawn(&stream, 5), "past the radius");

    stream.set_full_detail_radius(Some(6));
    assert!(!drawn(&stream, 5), "promoted but not yet meshed");
    upload(&mut stream, 5);
    assert!(drawn(&stream, 5));
}

/// A block change past full detail re-summarises the column for far terrain.
#[test]
fn changes_past_full_detail_refresh_the_summary() {
    let mut stream = stream();
    stream.set_full_detail_radius(Some(1));
    stream
        .submit(1, inline_block_entity_event(5, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.dispatch_far_summaries();
    assert!(stream.far_terrain.pending.is_empty());
    let key = *stream
        .resident
        .column(ChunkKey::new(0, 5, 0))
        .next()
        .expect("a resident section");
    stream.mark_dirty_exact(key, Instant::now());
    assert!(stream.far_terrain.pending.contains(&ChunkKey::new(0, 5, 0)));
}

/// Lifting the limit meshes columns that are still arriving, not only complete ones.
#[test]
fn lifting_the_limit_meshes_partial_columns() {
    let mut stream = stream();
    stream.set_full_detail_radius(Some(1));
    let key = SubChunkKey::from_chunk(ChunkKey::new(0, 7, 0), 0);
    stream.resident.insert(key);
    assert!(!stream.loaded_columns.contains(&ChunkKey::new(0, 7, 0)));

    stream.set_full_detail_radius(None);
    assert!(meshing(&stream, 7));
}
