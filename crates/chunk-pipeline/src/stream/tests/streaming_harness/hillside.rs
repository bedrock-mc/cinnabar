//! Terraced terrain streamed nearest-first must converge to a from-scratch light solve.
use std::collections::VecDeque;

use super::*;

const HILL_AIR: u32 = 0;
const HILL_STONE: u32 = 2;
const TOP: i32 = 128;
const BOTTOM: i32 = 64;
const CANOPY_Y: i32 = 100;

/// Stepped terraces crossing the sub-chunk 4/5 boundary, with cliffs where the steps wrap.
fn height(x: i32, z: i32) -> i32 {
    66 + (x.div_euclid(2) + z.div_euclid(3)).rem_euclid(28)
}

/// Opaque plates whose shade must be filled sideways across sub-chunk and column borders.
fn solid(x: i32, y: i32, z: i32) -> bool {
    y < height(x, z) || (y == CANOPY_Y && (x.div_euclid(24) + z.div_euclid(24)).rem_euclid(2) == 0)
}

fn hillside_payload(key: SubChunkKey) -> Vec<u8> {
    let mut words = [0_u32; 128];
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..16 {
                if solid(key.x * 16 + x, key.y * 16 + y, key.z * 16 + z) {
                    let linear = ((x << 8) | (z << 4) | y) as usize;
                    words[linear / 32] |= 1 << (linear % 32);
                }
            }
        }
    }
    let mut payload = vec![9, 1, key.y as i8 as u8, 3];
    payload.extend(words.iter().flat_map(|word| word.to_le_bytes()));
    payload.extend(zig_zag_i32(2));
    payload.extend(zig_zag_i32(HILL_AIR as i32));
    payload.extend(zig_zag_i32(HILL_STONE as i32));
    payload
}

fn uniform_payload(y: i32, runtime_id: u32) -> Vec<u8> {
    let mut payload = vec![9, 1, y as i8 as u8, 1];
    payload.extend(zig_zag_i32(runtime_id as i32));
    payload
}

fn hillside_harness() -> Harness {
    let mut harness = Harness::for_tests();
    harness.stream = WorldStream::new_with_assets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [8.0, 81.62, 8.0],
            world_spawn_position: [8, 80, 8],
            air_network_id: HILL_AIR,
            block_network_ids_are_hashes: false,
        },
        Arc::new(super::super::light_scheduler::light_test_assets()),
        [8.0, 81.62, 8.0],
        None,
    );
    harness.terrain = |_| false;
    harness.highest = 12;
    harness
}

fn insert_column_payloads(harness: &mut Harness, column: ChunkKey) {
    for y in -4..4 {
        let key = SubChunkKey::from_chunk(column, y);
        harness.payloads.insert(key, uniform_payload(y, HILL_STONE));
    }
    for y in 4..7 {
        let key = SubChunkKey::from_chunk(column, y);
        harness.payloads.insert(key, hillside_payload(key));
    }
}

/// Sky light of every air cell in the known columns, solved from nothing.
fn reference_sky(known: &BTreeSet<ChunkKey>) -> HashMap<[i32; 3], u8> {
    let mut light = HashMap::new();
    let mut queue = VecDeque::new();
    for column in known {
        for x in column.x * 16..column.x * 16 + 16 {
            for z in column.z * 16..column.z * 16 + 16 {
                let mut direct = true;
                for y in (BOTTOM..TOP).rev() {
                    if solid(x, y, z) {
                        direct = false;
                        continue;
                    }
                    light.insert([x, y, z], if direct { 15_u8 } else { 0 });
                    if direct {
                        queue.push_back([x, y, z]);
                    }
                }
            }
        }
    }
    while let Some(cell) = queue.pop_front() {
        let next_level = light[&cell].saturating_sub(1);
        for offset in [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ] {
            let next = [
                cell[0] + offset[0],
                cell[1] + offset[1],
                cell[2] + offset[2],
            ];
            if let Some(level) = light.get_mut(&next)
                && *level < next_level
            {
                *level = next_level;
                queue.push_back(next);
            }
        }
    }
    light
}

/// Steps until the stream drains, failing instead of hanging when work never settles.
fn settle(harness: &mut Harness) {
    harness.step_until(Harness::idle);
}

fn assert_matches_reference(harness: &Harness) {
    let known = harness
        .stream
        .resident
        .iter()
        .map(|key| key.chunk())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|column| {
            (-4..20).all(|y| {
                harness
                    .stream
                    .light_source_is_known(SubChunkKey::from_chunk(*column, y))
            })
        })
        .collect::<BTreeSet<_>>();
    assert!(!known.is_empty());
    let mut wrong = reference_sky(&known)
        .into_iter()
        .filter_map(|(cell, expected)| {
            let solved = harness.stream.solved_light_at(cell.map(|v| v as f32 + 0.5));
            (solved.map(|(_, sky)| sky) != Some(expected)).then_some((cell, expected, solved))
        })
        .collect::<Vec<_>>();
    wrong.sort_unstable();
    assert!(
        wrong.is_empty(),
        "{} air cells differ from a from-scratch solve, first (cell, expected, solved) {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );

    let mut stale = Vec::new();
    for (key, presented) in &harness.presented {
        let center = harness.stream.authority.terrain().sub_chunk(*key).unwrap();
        let halo = harness.stream.mesh_light_halo(*key).unwrap();
        let fresh = harness.stream.mesh_snapshot(*key, center, halo).mesh(
            harness.stream.classifier,
            harness.stream.authority.runtime_assets(),
            harness.stream.authority.network_id_mode(),
        );
        if fresh.cube_lighting() != presented.cube_lighting() {
            stale.push(*key);
        }
    }
    stale.sort_unstable();
    assert!(
        stale.is_empty(),
        "{} presented meshes differ from current light, first {:?}",
        stale.len(),
        &stale[..stale.len().min(8)]
    );
}

/// Late neighbours, shade filled across borders, walking and unanswered slots all converge.
#[test]
fn streamed_shaded_terraces_converge_to_a_from_scratch_solve() {
    let mut harness = hillside_harness();
    let mut center = ChunkKey::new(0, 0, 0);
    for column in spiral(center, RADIUS) {
        insert_column_payloads(&mut harness, column);
        if column.x == 5 {
            harness
                .payloads
                .insert(SubChunkKey::from_chunk(column, 7), Vec::new());
        }
    }
    harness.send_view(center, false);
    settle(&mut harness);
    assert_matches_reference(&harness);

    for _ in 0..4 {
        let previous = spiral(center, RADIUS);
        center = ChunkKey::new(0, center.x + 1, center.z);
        let block = [center.x * 16 + 8, 80, center.z * 16 + 8];
        harness.camera = [block[0] as f32, 81.62, block[2] as f32];
        harness
            .wire
            .push_back(WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: block,
                radius_blocks: (RADIUS * 16) as u32,
            }));
        for column in spiral(center, RADIUS) {
            if !previous.contains(&column) {
                insert_column_payloads(&mut harness, column);
                harness.push_column(column);
            }
        }
        settle(&mut harness);
    }
    assert_matches_reference(&harness);
}

/// A slot whose requests end without data lights as air instead of shading the column below.
#[test]
fn unanswered_sub_chunk_lights_as_air() {
    let mut harness = hillside_harness();
    let center = ChunkKey::new(0, 2, 0);
    let unanswered = SubChunkKey::from_chunk(center, 7);
    let block = [center.x * 16 + 8, 80, center.z * 16 + 8];
    harness
        .wire
        .push_back(WorldEvent::PublisherUpdate(PublisherUpdateEvent {
            center: block,
            radius_blocks: 16,
        }));
    for column in spiral(center, 1) {
        insert_column_payloads(&mut harness, column);
        harness.push_column(column);
    }
    harness.payloads.insert(unanswered, Vec::new());
    settle(&mut harness);

    assert_eq!(harness.stream.stats().sub_chunk_retry_exhaustions, 1);
    let ground = [block[0], height(block[0], block[2]), block[2]];
    assert_eq!(
        harness
            .stream
            .solved_light_at(ground.map(|v| v as f32 + 0.5)),
        Some((0, 15))
    );
    assert!(
        harness
            .presented
            .contains_key(&SubChunkKey::from_chunk(center, 5))
    );
    assert_matches_reference(&harness);
}
