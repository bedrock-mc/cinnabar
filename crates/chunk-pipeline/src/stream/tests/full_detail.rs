//! Columns past the full-detail radius keep their blocks but skip lighting and meshing.

use super::*;

mod cancellations;
mod handoffs;
mod transitions;

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

/// Whether non-air data has pending geometry or a withdrawal in this column.
fn meshing(stream: &WorldStream, x: i32) -> bool {
    stream.mesh_jobs.pending.keys().any(|key| {
        key.chunk() == ChunkKey::new(0, x, 0)
            && stream.terrain_removal_kind(*key) != render_api::TerrainRemovalKind::Empty
    })
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

/// Dequeues and acknowledges pending section publications, returning the acknowledged keys.
fn upload(stream: &mut WorldStream, x: i32) -> Vec<SubChunkKey> {
    let column = ChunkKey::new(0, x, 0);
    let keys: Vec<SubChunkKey> = stream
        .resident
        .column(column)
        .chain(stream.known_air.column(column))
        .copied()
        .collect();
    let now = Instant::now();
    let mut acknowledged = Vec::new();
    for key in keys {
        if let Some((revision, since)) = stream
            .revisions
            .dirty(key)
            .map(|dirty| (dirty.revision, dirty.since))
        {
            stream.mesh_jobs.pending.remove(&key);
            stream.acknowledge_mesh_upload(key, revision, since, now);
            acknowledged.push(key);
        }
    }
    acknowledged
}

/// Limiting detail after running without a limit hands meshes already past the radius to far
/// terrain, withdrawing them only once its summary is delivered and far terrain draws it.
#[test]
fn a_new_limit_hands_columns_meshed_without_one_to_far_terrain() {
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
    assert!(drawn(&stream, 5), "kept until far terrain covers it");
    let five = ChunkKey::new(0, 5, 0);
    test_time::eventually("the hand-over summary", || {
        stream.dispatch_far_summaries();
        stream.drain_far_summaries(&mut Vec::new());
        stream.far_handoffs().any(|column| column == five)
    });
    stream.mesh_jobs.pending.clear();
    stream.finish_far_handoff(five);
    assert!(!drawn(&stream, 5), "far terrain takes the column back");
    assert!(!stream.wants_mesh(five));
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
    stream.mark_changed_sources([key], Instant::now());
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

/// Detail maintenance yields at the deadline and a changed destination replaces its cursor.
#[test]
fn interrupted_detail_scan_converges_on_the_latest_player_grid() {
    let mut stream = stream();
    stream.set_full_detail_radius(Some(16));
    stream.between_frames_service = true;
    stream.local_player_chunk = Some(ChunkKey::new(0, 100, 0));
    stream.frame_deadline = Some(Instant::now());
    let first = stream.poll([1600.0, 0.0, 0.0], 0);
    assert!(first.maintenance_items > 0);
    assert!(
        stream.has_pending_full_detail(),
        "a teleport resumes across polls"
    );

    stream.local_player_chunk = Some(ChunkKey::new(0, -100, 0));
    stream.service_yield = Some(Arc::new(AtomicBool::new(true)));
    let before = stream.maintenance_items;
    stream.poll([-1600.0, 0.0, 0.0], 0);
    assert_eq!(
        stream.maintenance_items, before,
        "reclaim runs no new maintenance"
    );
    stream.service_yield = None;
    test_time::eventually("the replacement detail grid", || {
        stream.poll([-1600.0, 0.0, 0.0], 0);
        !stream.has_pending_full_detail()
    });
    assert!(stream.wants_mesh(ChunkKey::new(0, -100, 0)));
    assert!(!stream.wants_mesh(ChunkKey::new(0, 100, 0)));
    assert!(!stream.wants_mesh(ChunkKey::new(0, 0, 0)));
}

/// A paused radius change preserves drawn columns until their far summaries take over.
#[test]
fn serviced_radius_changes_preserve_the_far_handoff() {
    let mut stream = stream();
    stream
        .submit(1, inline_block_entity_event(5, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    upload(&mut stream, 5);
    stream.between_frames_service = true;
    stream.set_full_detail_radius(Some(1));
    stream.frame_deadline = Some(Instant::now());
    stream.poll([0.0; 3], 0);
    assert!(
        drawn(&stream, 5),
        "near coverage survives a paused demotion"
    );
    test_time::eventually("the detail limit", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(drawn(&stream, 5));
    let five = ChunkKey::new(0, 5, 0);
    test_time::eventually("the delivered far summary", || {
        stream.dispatch_far_summaries();
        stream.drain_far_summaries(&mut Vec::new());
        stream.far_handoffs().any(|column| column == five)
    });
    stream.finish_far_handoff(five);
    assert!(!drawn(&stream, 5));
    assert!(!stream.wants_mesh(five));
    stream.set_full_detail_radius(None);
    test_time::eventually("restoring full detail", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(stream.wants_mesh(five));
    assert!(meshing(&stream, 5));
}

/// An initial limit preserves old membership while paused, including resends and live edits.
#[test]
fn paused_initial_limit_keeps_existing_columns_active() {
    let mut stream = stream();
    stream
        .submit(1, inline_block_entity_event(2, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.between_frames_service = true;
    stream.set_full_detail_radius(Some(1));
    assert!(stream.wants_mesh(ChunkKey::new(0, 2, 0)));
    stream
        .submit(2, inline_block_entity_event(3, 7, Vec::new()))
        .unwrap();
    stream
        .submit(3, inline_block_entity_event(2, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(
        stream.wants_mesh(ChunkKey::new(0, 2, 0)),
        "resends keep old membership"
    );
    assert!(
        !stream.wants_mesh(ChunkKey::new(0, 3, 0)),
        "new arrivals use the base radius"
    );
    stream.mesh_jobs.pending.clear();
    let keys: Vec<_> = stream
        .resident
        .column(ChunkKey::new(0, 2, 0))
        .copied()
        .collect();
    stream.mark_changed_sources(keys, Instant::now());
    assert!(
        meshing(&stream, 2),
        "an edit still invalidates the near mesh"
    );
    test_time::eventually("the initial detail limit", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(stream.wants_mesh(ChunkKey::new(0, 2, 0)));
    assert!(!stream.wants_mesh(ChunkKey::new(0, 3, 0)));
}

/// Replacing an unfinished unlimited transition uses current residency, not an old disk.
#[test]
fn interrupted_unlimited_transition_does_not_promote_new_outer_columns() {
    let mut stream = stream();
    stream.set_full_detail_radius(Some(4));
    stream.between_frames_service = true;
    stream.set_full_detail_radius(None);
    stream.set_full_detail_radius(Some(1));
    stream
        .submit(1, inline_block_entity_event(2, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(!stream.wants_mesh(ChunkKey::new(0, 2, 0)));
    test_time::eventually("the replacement radius", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(!stream.wants_mesh(ChunkKey::new(0, 2, 0)));
}

/// A dimension change during the first limit retires already visited old memberships too.
#[test]
fn initial_detail_scan_drops_membership_from_a_retired_dimension() {
    let mut stream = stream();
    stream
        .submit(1, inline_block_entity_event(5, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.between_frames_service = true;
    stream.set_full_detail_radius(Some(16));
    stream.frame_deadline = Some(Instant::now());
    stream.poll([0.0; 3], 0);
    stream
        .submit(
            2,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    test_time::eventually("detail after a dimension change", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(!stream.wants_mesh(ChunkKey::new(0, 5, 0)));
    assert!(stream.wants_mesh(ChunkKey::new(1, 0, 0)));
}

/// Radius changes, mode and render-distance changes (Auto crossing its threshold), block
/// changes, uploads, resends and evictions in a scrambled order never leave a column that had
/// a drawer without one: near terrain draws it once its meshes are published, and far terrain
/// from a delivered summary while shown.
#[test]
fn covered_columns_always_keep_a_drawer() {
    const COLUMNS: i32 = 8;
    let mut stream = stream();
    let mut seed = 0x9e37_79b9_u64;
    let mut next = move |bound: u64| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) % bound
    };
    let mut sequence = 0;
    let mut load = |stream: &mut WorldStream, x: i32| {
        sequence += 1;
        stream
            .submit(sequence, inline_block_entity_event(x, 7, Vec::new()))
            .unwrap();
        complete_pending_decode_jobs(stream);
    };
    for x in 0..COLUMNS {
        load(&mut stream, x);
    }
    // The Far Terrain setting and the requested render distance decide it as the app does,
    // so Auto crosses its threshold back and forth as the render distance changes.
    use render_api::far_terrain::{DeviceTier, FarTerrainMode};
    let tier = DeviceTier::Mid;
    let mut mode = FarTerrainMode::On;
    let mut render = 64;
    let mut far_on = true;
    let mut far_summaries: HashSet<ChunkKey> = HashSet::new();
    let mut covered: HashSet<ChunkKey> = HashSet::new();
    // Whether each section's last upload drew geometry or withdrew it. Near terrain draws these
    // meshes whether or not far terrain tracks the column as drawn.
    let mut on_screen: HashMap<SubChunkKey, bool> = HashMap::new();
    for step in 0..600 {
        let x = next(COLUMNS as u64) as i32;
        let column = ChunkKey::new(0, x, 0);
        match next(8) {
            0 => {
                let radius = [Some(1), Some(3), Some(6), None][next(4) as usize];
                stream.set_full_detail_radius(radius.filter(|_| far_on));
            }
            1 => {
                if next(2) == 0 {
                    mode = FarTerrainMode::ALL[next(3) as usize];
                } else {
                    render = [16, tier.auto_full_detail_chunks(), 64][next(3) as usize];
                }
                // As the app does: turning off lifts the limit at once, but summaries and the
                // far scene stay until near terrain draws every loaded column.
                let wanted = mode.enabled(render, tier);
                if wanted != far_on {
                    far_on = wanted;
                    if far_on {
                        stream.set_far_terrain_enabled(true);
                    } else {
                        stream.set_full_detail_radius(None);
                    }
                }
            }
            2 => {
                let key = stream.resident.column(column).next().copied();
                if let Some(key) = key {
                    stream.mark_changed_sources([key], Instant::now());
                }
            }
            3 | 4 => {
                let wanted = stream.wants_mesh(column);
                for key in upload(&mut stream, x) {
                    on_screen.insert(key, wanted);
                }
            }
            5 => {
                stream.dispatch_far_summaries();
                let mut delivered = Vec::new();
                stream.drain_far_summaries(&mut delivered);
                far_summaries.extend(delivered.iter().map(|summary| summary.column));
                let ready: Vec<ChunkKey> = stream
                    .far_handoffs()
                    .filter(|column| far_summaries.contains(column))
                    .collect();
                for column in ready {
                    stream.finish_far_handoff(column);
                }
            }
            6 => load(&mut stream, x),
            _ => {
                stream.evict_column(column);
                far_summaries.remove(&column);
                covered.remove(&column);
                on_screen.retain(|key, _| key.chunk() != column);
            }
        }
        if !far_on && stream.far_terrain_enabled() && stream.all_loaded_near_drawn() {
            stream.set_far_terrain_enabled(false);
        }
        let far_shown = stream.far_terrain_enabled();
        if !far_shown {
            far_summaries.clear();
        }
        for x in 0..COLUMNS {
            let column = ChunkKey::new(0, x, 0);
            if !stream.loaded_columns.contains(&column)
                || stream.resident.column(column).next().is_none()
            {
                covered.remove(&column);
                continue;
            }
            let near = stream
                .resident
                .column(column)
                .chain(stream.known_air.column(column))
                .all(|key| on_screen.get(key).copied().unwrap_or(false));
            assert!(
                near || !drawn(&stream, x),
                "far terrain hides under {column:?} without its meshes at step {step}"
            );
            let far = far_shown && far_summaries.contains(&column);
            if near || far {
                covered.insert(column);
            } else {
                assert!(
                    !covered.contains(&column),
                    "{column:?} lost its drawer at step {step}"
                );
            }
        }
    }
    assert!(!covered.is_empty());
}

/// A hand-over completes only on a summary issued after the column left full detail.
#[test]
fn hand_over_waits_for_a_summary_taken_after_demotion() {
    let mut stream = stream();
    stream
        .submit(1, inline_block_entity_event(5, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.dispatch_far_summaries();
    upload(&mut stream, 5);
    stream.set_full_detail_radius(Some(1));
    let five = ChunkKey::new(0, 5, 0);
    let mut delivered = Vec::new();
    test_time::eventually("the earlier summary", || {
        stream.drain_far_summaries(&mut delivered);
        !delivered.is_empty()
    });
    assert_eq!(
        stream.far_handoffs().count(),
        0,
        "taken before the demotion"
    );
    test_time::eventually("the hand-over summary", || {
        stream.dispatch_far_summaries();
        stream.drain_far_summaries(&mut Vec::new());
        stream.far_handoffs().any(|column| column == five)
    });
}

/// Evicting a column mid hand-over still withdraws its meshes.
#[test]
fn evicting_mid_hand_over_withdraws_meshes() {
    let mut stream = stream();
    stream
        .submit(1, inline_block_entity_event(5, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    upload(&mut stream, 5);
    stream.set_full_detail_radius(Some(1));
    assert!(drawn(&stream, 5));
    stream.mesh_jobs.pending.clear();
    stream.evict_column(ChunkKey::new(0, 5, 0));
    assert!(meshing(&stream, 5), "its removal is queued");
    assert!(!drawn(&stream, 5));
}

/// An interrupted restoration still refreshes far residents admitted by a wider radius.
#[test]
fn interrupted_restoration_refreshes_pending_promotions() {
    let radius = 64;
    let far = ChunkKey::new(0, 40, 0);
    let mut stream = stream();
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(radius))
        .unwrap();
    stream.set_full_detail_radius(Some(16));
    stream
        .submit(2, inline_block_entity_event(far.x, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(
        stream.resident.column(far).next().is_some(),
        "the far column is retained"
    );
    assert!(!meshing(&stream, far.x));
    stream.between_frames_service = true;
    stream.set_full_detail_radius(None);
    stream.service_yield = Some(Arc::new(AtomicBool::new(true)));
    stream.poll([0.0; 3], 0);
    assert!(
        !meshing(&stream, far.x),
        "restoration has not reached the far column"
    );
    stream.set_full_detail_radius(Some(radius));
    stream.service_yield = None;
    test_time::eventually("the wider replacement radius", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(stream.wants_mesh(far));
    assert!(
        meshing(&stream, far.x),
        "pending promotion must queue its mesh"
    );
}

/// Meshes published ahead of a restoration scan keep coverage until far terrain takes over.
#[test]
fn interrupted_restoration_hands_off_newly_published_outer_meshes() {
    for arriving in [false, true] {
        let far = ChunkKey::new(0, 40, 0);
        let mut stream = stream();
        stream
            .submit(1, WorldEvent::ChunkRadiusUpdated(64))
            .unwrap();
        stream.set_full_detail_radius(Some(16));
        if !arriving {
            stream
                .submit(2, inline_block_entity_event(far.x, 7, Vec::new()))
                .unwrap();
            complete_pending_decode_jobs(&mut stream);
        }
        stream.between_frames_service = true;
        stream.set_full_detail_radius(None);
        if arriving {
            stream
                .submit(2, inline_block_entity_event(far.x, 7, Vec::new()))
                .unwrap();
            complete_pending_decode_jobs(&mut stream);
        } else {
            let keys: Vec<_> = stream.resident.column(far).copied().collect();
            assert!(!keys.is_empty(), "retained far data");
            stream.mark_changed_sources(keys, Instant::now());
        }
        assert!(
            stream.has_pending_full_detail(),
            "restoration has not completed"
        );
        assert!(!upload(&mut stream, far.x).is_empty());
        assert!(
            drawn(&stream, far.x),
            "unlimited detail published the near mesh"
        );
        stream.set_full_detail_radius(Some(16));
        test_time::eventually("the interrupted restoration demotion", || {
            stream.poll([0.0; 3], 0);
            !stream.has_pending_full_detail()
        });
        assert!(stream.wants_mesh(far), "a drawn outer mesh needs a handoff");
        assert!(
            drawn(&stream, far.x),
            "near coverage stays until the far summary arrives"
        );
        test_time::eventually("the far coverage for an interrupted restoration", || {
            stream.dispatch_far_summaries();
            stream.drain_far_summaries(&mut Vec::new());
            stream.far_handoffs().any(|column| column == far)
        });
        stream.mesh_jobs.pending.clear();
        stream.finish_far_handoff(far);
        assert!(!drawn(&stream, far.x));
        assert!(!stream.wants_mesh(far));
        assert!(
            meshing(&stream, far.x),
            "far coverage releases the near geometry"
        );
    }
}

/// A near mesh published during restoration remains live inside the replacement limit's band.
#[test]
fn interrupted_restoration_keeps_drawn_hysteresis_columns_active() {
    let far = ChunkKey::new(0, 5, 0);
    let mut stream = stream();
    stream.set_full_detail_radius(Some(1));
    stream
        .submit(1, inline_block_entity_event(far.x, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.between_frames_service = true;
    stream.set_full_detail_radius(None);
    let keys: Vec<_> = stream.resident.column(far).copied().collect();
    let key = *keys.first().expect("retained far data");
    stream.mark_changed_sources(keys, Instant::now());
    assert!(!upload(&mut stream, far.x).is_empty());
    assert!(drawn(&stream, far.x));
    stream.set_full_detail_radius(Some(3));
    test_time::eventually("the replacement detail band", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(
        stream.wants_mesh(far),
        "drawn geometry in the band remains live"
    );
    assert!(drawn(&stream, far.x));
    stream.mesh_jobs.pending.clear();
    stream.mark_changed_sources([key], Instant::now());
    assert!(
        meshing(&stream, far.x),
        "later block updates still reach its mesh"
    );
}

/// Restoring a drawn handoff inside a new limit refreshes it and cancels the old demotion.
#[test]
fn interrupted_restoration_repromotes_a_drawn_handoff() {
    let far = ChunkKey::new(0, 5, 0);
    let mut stream = stream();
    stream
        .submit(1, inline_block_entity_event(far.x, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    upload(&mut stream, far.x);
    stream.set_full_detail_radius(Some(1));
    assert!(
        stream.wants_mesh(far),
        "near geometry waits for far coverage"
    );
    stream.mesh_jobs.pending.clear();
    stream.between_frames_service = true;
    stream.set_full_detail_radius(None);
    stream.set_full_detail_radius(Some(6));
    test_time::eventually("the renewed near membership", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(stream.wants_mesh(far));
    assert!(drawn(&stream, far.x));
    test_time::eventually("the old demotion summary", || {
        stream.dispatch_far_summaries();
        let mut delivered = Vec::new();
        stream.drain_far_summaries(&mut delivered);
        delivered.iter().any(|summary| summary.column == far)
    });
    assert!(
        !stream.far_handoffs().any(|column| column == far),
        "the earlier demotion is cancelled"
    );
    assert!(
        meshing(&stream, far.x),
        "renewed detail refreshes its lighting and mesh"
    );
}

/// Complete eviction cannot make new hysteresis-band arrivals inherit old membership.
#[test]
fn complete_eviction_preserves_pending_membership_epoch() {
    let mut stream = stream();
    let column = ChunkKey::new(0, 2, 0);
    let key = SubChunkKey::from_chunk(column, 0);
    stream.resident.insert(key);
    stream.between_frames_service = true;
    stream.set_full_detail_radius(Some(1));
    assert!(stream.wants_mesh(column));
    stream.evict_columns(BTreeSet::from([column]));
    assert!(stream.resident.is_empty());
    stream.resident.insert(key);
    assert!(
        !stream.wants_mesh(column),
        "a new arrival is outside the base radius"
    );
    test_time::eventually("membership after full eviction", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(!stream.wants_mesh(column));
}

/// A column already demoted by an unfinished first limit still needs a later promotion.
#[test]
fn interrupted_initial_limit_restoration_refreshes_demoted_columns() {
    let radius = 64;
    let far = ChunkKey::new(0, 40, 0);
    let mut stream = stream();
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(radius))
        .unwrap();
    stream
        .submit(2, inline_block_entity_event(far.x, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(
        stream.resident.column(far).next().is_some(),
        "the far column is retained"
    );
    stream.between_frames_service = true;
    stream.set_full_detail_radius(Some(16));
    stream.frame_deadline = Some(Instant::now());
    stream.poll([0.0; 3], 0);
    assert!(stream.has_pending_full_detail());
    assert!(!stream.wants_mesh(far));
    stream.mesh_jobs.pending.clear();
    stream.set_full_detail_radius(None);
    stream.set_full_detail_radius(Some(radius));
    test_time::eventually("the replacement of an unfinished first limit", || {
        stream.poll([0.0; 3], 0);
        !stream.has_pending_full_detail()
    });
    assert!(meshing(&stream, far.x));
}
