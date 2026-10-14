use std::{collections::BTreeMap, sync::Arc};

use world::ChunkKey;

use super::*;
use crate::{
    fixtures::{biome_column, blocks, fixture_classifier, terrain_block, terrain_column},
    geometry::MAX_LEVEL,
    sampler::{ColumnSnapshot, sample_column},
};

fn material(id: u32) -> MaterialId {
    MaterialId::new(id).expect("material")
}

fn sample(
    chunk_x: i32,
    chunk_z: i32,
    sub_chunks: &BTreeMap<i32, Arc<world::SubChunk>>,
) -> ColumnSample {
    let biomes = biome_column(0, &[]);
    let snapshot = ColumnSnapshot {
        chunk: ChunkKey::new(0, chunk_x, chunk_z),
        sub_chunks,
        biomes: &biomes,
        default_biome: 0,
        light: None,
    };
    sample_column(&snapshot, &mut fixture_classifier()).expect("column has a key")
}

#[test]
fn surfaces_skip_water_and_keep_leaves_and_glass() {
    let classes = fixture_classifier();
    let (chunk_x, chunk_z) = (-3, 2);
    let surface = ChunkSurface::from_sample(
        &sample(chunk_x, chunk_z, &terrain_column(chunk_x, chunk_z, -1..8)),
        &classes,
    );
    let mut kinds = [false; 3];
    for z in 0..CHUNK_SIDE {
        for x in 0..CHUNK_SIDE {
            let (bx, bz) = (chunk_x * 16 + x as i32, chunk_z * 16 + z as i32);
            let expected = (-16..128).rev().find_map(|y| {
                let (id, _) = terrain_block(bx, y, bz);
                (id != blocks::AIR && id != blocks::WATER).then(|| SurfaceColumn {
                    top_y: y,
                    material: material(id),
                })
            });
            assert_eq!(
                surface.get(x, z),
                expected.map(SurfaceColumn::signature),
                "column ({x}, {z})"
            );
            let id = expected.map(|column| column.material.raw());
            kinds[0] |= id == Some(blocks::SAND);
            kinds[1] |= id == Some(blocks::LEAVES);
            kinds[2] |= id == Some(blocks::GRASS);
        }
    }
    assert_eq!(
        kinds, [true; 3],
        "the chunk covers seabed, canopy and grass columns"
    );
}

#[test]
fn columns_without_solid_blocks_have_no_surface() {
    let classes = fixture_classifier();
    let water = crate::fixtures::uniform_sub_chunk(blocks::WATER);
    let column: BTreeMap<_, _> = [(-2, Arc::new(water))].into();
    let surface = ChunkSurface::from_sample(&sample(0, 0, &column), &classes);
    assert_eq!(surface, ChunkSurface::default());
    let stone = crate::fixtures::uniform_sub_chunk(blocks::STONE);
    let column: BTreeMap<_, _> = [(-3, Arc::new(stone))].into();
    let surface = ChunkSurface::from_sample(&sample(0, 0, &column), &classes);
    assert_eq!(
        surface.get(15, 15),
        Some(
            SurfaceColumn {
                top_y: -33,
                material: material(blocks::STONE)
            }
            .signature()
        )
    );
}

#[test]
fn signatures_are_stable_non_zero_and_round_trip() {
    let column = |top_y, id| {
        SurfaceColumn {
            top_y,
            material: material(id),
        }
        .signature()
    };
    // Pinned: signatures are persisted, so the mix must not change.
    assert_eq!(column(64, 1).bits(), PINNED_SIGNATURE);
    assert_eq!(ColumnSignature::from_bits(0), None);
    let mut seen = std::collections::HashSet::new();
    for top_y in -64..320 {
        for id in 1..64 {
            let signature = column(top_y, id);
            assert_ne!(signature.bits(), 0);
            assert_eq!(
                ColumnSignature::from_bits(signature.bits()),
                Some(signature)
            );
            seen.insert(signature);
        }
    }
    assert!(
        seen.len() * 100 > 384 * 63 * 80,
        "neighbouring surfaces rarely collide: {} distinct",
        seen.len()
    );
}

/// [`SurfaceColumn::signature`] of Y 64 in material 1.
const PINNED_SIGNATURE: u16 = 63_762;

/// A deterministic varied world: heights 60–75 and four materials, per `seed`.
fn world_column(seed: u32, bx: i32, bz: i32) -> SurfaceColumn {
    // Neighbouring columns share smooth terrain: height from coarse cells.
    let height = mix(seed, bx.div_euclid(4), bz.div_euclid(4)) % 16;
    SurfaceColumn {
        top_y: 60 + height as i32,
        material: material(1 + mix(seed, bx, bz) % 4),
    }
}

/// [`world_column`] over a whole region.
fn world_region(seed: u32, pos: RegionPos) -> RegionSignature {
    region(pos, |bx, bz| Some(world_column(seed, bx, bz)))
}

/// The region at `pos` with every column's surface from `surface(block_x, block_z)`.
fn region(pos: RegionPos, surface: impl Fn(i32, i32) -> Option<SurfaceColumn>) -> RegionSignature {
    let mut signature = RegionSignature::new(pos);
    for local_z in 0..REGION_SIDE_CHUNKS {
        for local_x in 0..REGION_SIDE_CHUNKS {
            let chunk_x = pos.x * REGION_SIDE_CHUNKS + local_x;
            let chunk_z = pos.z * REGION_SIDE_CHUNKS + local_z;
            let mut chunk = ChunkSurface::default();
            for z in 0..CHUNK_SIDE {
                for x in 0..CHUNK_SIDE {
                    let (bx, bz) = (chunk_x * 16 + x as i32, chunk_z * 16 + z as i32);
                    chunk.set(x, z, surface(bx, bz).map(SurfaceColumn::signature));
                }
            }
            assert!(signature.record(chunk_x, chunk_z, chunk));
        }
    }
    signature
}

/// The same world with `percent` of its columns rebuilt 1–5 blocks higher in another
/// material, as the epoch measurements edited theirs.
fn edited(seed: u32, pos: RegionPos, percent: u32) -> RegionSignature {
    region(pos, |bx, bz| {
        let hash = mix(seed ^ 0xed17, bx, bz);
        let column = world_column(seed, bx, bz);
        Some(if hash % 100 < percent {
            SurfaceColumn {
                top_y: column.top_y + 1 + (hash / 100 % 5) as i32,
                material: material(100 + hash % 50),
            }
        } else {
            column
        })
    })
}

fn flat(pos: RegionPos, top_y: i32, id: u32) -> RegionSignature {
    region(pos, |_, _| {
        Some(SurfaceColumn {
            top_y,
            material: material(id),
        })
    })
}

fn flat_chunk(top_y: i32, id: u32) -> ChunkSurface {
    let mut chunk = ChunkSurface::default();
    let signature = SurfaceColumn {
        top_y,
        material: material(id),
    }
    .signature();
    for z in 0..CHUNK_SIDE {
        for x in 0..CHUNK_SIDE {
            chunk.set(x, z, Some(signature));
        }
    }
    chunk
}

fn mix(seed: u32, x: i32, z: i32) -> u32 {
    let mut value =
        seed ^ (x as u32).wrapping_mul(0x9e37_79b9) ^ (z as u32).wrapping_mul(0x85eb_ca6b);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ value >> 16
}

#[test]
fn region_comparisons_follow_the_threshold_and_abstain_rules() {
    let pos = RegionPos { x: 2, z: -3 };
    let world = world_region(1, pos);
    for seed in 0..64 {
        assert_eq!(
            compare_region(&world, &world, seed),
            RegionComparison::Match {
                matching: SAMPLED_COLUMNS as u32
            }
        );
        assert!(matches!(
            compare_region(&world, &edited(1, pos, 2), seed),
            RegionComparison::Match { .. }
        ));
        assert!(matches!(
            compare_region(&world, &world_region(2, pos), seed),
            RegionComparison::Mismatch { .. }
        ));
        assert_eq!(
            compare_region(&flat(pos, -61, 3), &flat(pos, 4, 3), seed),
            RegionComparison::Uniform,
            "flat maps never vote, even against each other"
        );
        assert_eq!(
            compare_region(&world, &flat(pos, -61, 3), seed),
            RegionComparison::Uniform,
            "one uniform side is enough to abstain"
        );
    }
}

#[test]
fn regions_vote_only_with_enough_overlap() {
    let pos = RegionPos { x: -1, z: 0 };
    // The first `count` columns of the same world, in region row order.
    let first = |count: usize| {
        region(pos, |bx, bz| {
            let local = (bz.rem_euclid(64) * 64 + bx.rem_euclid(64)) as usize;
            (local < count).then(|| world_column(4, bx, bz))
        })
    };
    let world = world_region(4, pos);
    assert_eq!(
        compare_region(&world, &first(MIN_OVERLAPPING_COLUMNS - 1), 9),
        RegionComparison::TooFewColumns
    );
    assert_eq!(
        compare_region(&RegionSignature::new(pos), &world, 9),
        RegionComparison::TooFewColumns
    );
    assert_eq!(
        compare_region(&world, &first(MIN_OVERLAPPING_COLUMNS), 9),
        RegionComparison::Match {
            matching: SAMPLED_COLUMNS as u32
        }
    );
}

#[test]
fn the_agreement_threshold_is_inclusive() {
    let half = SAMPLED_COLUMNS * MIN_AGREEMENT_PERCENT / 100;
    assert_eq!(
        agreement(half),
        RegionComparison::Match {
            matching: half as u32
        }
    );
    assert_eq!(
        agreement(half - 1),
        RegionComparison::Mismatch {
            matching: half as u32 - 1
        }
    );
}

#[test]
fn draws_are_reproducible_per_seed_and_vary_between_seeds() {
    let pos = RegionPos { x: 0, z: 0 };
    let stored = world_region(6, pos);
    // Half the columns differ, so each draw lands near the threshold.
    let fresh = region(pos, |bx, bz| {
        let column = world_column(6, bx, bz);
        Some(SurfaceColumn {
            top_y: column.top_y + i32::from(mix(8, bx, bz).is_multiple_of(2)),
            ..column
        })
    });
    let (mut matches, mut mismatches) = (0, 0);
    for seed in 0..64 {
        let comparison = compare_region(&stored, &fresh, seed);
        assert_eq!(comparison, compare_region(&stored, &fresh, seed));
        match comparison {
            RegionComparison::Match { .. } => matches += 1,
            RegionComparison::Mismatch { .. } => mismatches += 1,
            other => panic!("varied regions vote: {other:?}"),
        }
    }
    assert!(matches > 0 && mismatches > 0, "{matches} / {mismatches}");
}

#[test]
#[should_panic(expected = "signatures of different regions")]
fn comparing_different_regions_panics() {
    let _ = compare_region(
        &RegionSignature::new(RegionPos { x: 0, z: 0 }),
        &RegionSignature::new(RegionPos { x: 1, z: 0 }),
        0,
    );
}

#[test]
fn votes_merge_and_split_at_their_margins_and_then_stay_decided() {
    assert_eq!(decide(11, 0), EpochDecision::Merge);
    assert_eq!(decide(10, 0), EpochDecision::Candidate);
    assert_eq!(decide(13, 1), EpochDecision::Merge);
    assert_eq!(decide(12, 1), EpochDecision::Candidate);
    assert_eq!(decide(0, 101), EpochDecision::Split);
    assert_eq!(decide(0, 100), EpochDecision::Candidate);
    assert_eq!(decide(1, 103), EpochDecision::Split);
    assert_eq!(decide(1, 102), EpochDecision::Candidate);
    assert_eq!(decide(u32::MAX, u32::MAX), EpochDecision::Candidate);

    let mut vote = EpochVote::new();
    for _ in 0..10 {
        assert_eq!(
            vote.record(RegionComparison::Match { matching: 32 }),
            EpochDecision::Candidate
        );
        vote.record(RegionComparison::Uniform);
        vote.record(RegionComparison::TooFewColumns);
    }
    assert_eq!((vote.matching(), vote.mismatching()), (10, 0));
    assert_eq!(
        vote.record(RegionComparison::Match { matching: 20 }),
        EpochDecision::Merge
    );
    assert_eq!(
        vote.add(0, 1000),
        EpochDecision::Merge,
        "the first decision is final"
    );
    assert_eq!(vote.decision(), EpochDecision::Merge);

    let mut split = EpochVote::new();
    assert_eq!(split.add(5, 110), EpochDecision::Candidate);
    assert_eq!(split.add(0, 1), EpochDecision::Split);
}

/// Replays a session over `regions` regions against stored signatures.
fn session(
    stored: impl Fn(RegionPos) -> RegionSignature,
    fresh: impl Fn(RegionPos) -> RegionSignature,
    regions: i32,
) -> EpochDecision {
    let mut vote = EpochVote::new();
    for index in 0..regions {
        let pos = RegionPos {
            x: index % 11 - 5,
            z: index / 11 - 5,
        };
        vote.record(compare_region(&stored(pos), &fresh(pos), index as u64));
    }
    vote.decision()
}

#[test]
fn sessions_merge_same_worlds_split_different_ones_and_never_merge_flat_maps() {
    let regions = 121;
    let same = session(|pos| world_region(1, pos), |pos| edited(1, pos, 2), regions);
    assert_eq!(same, EpochDecision::Merge);
    let different = session(
        |pos| world_region(1, pos),
        |pos| world_region(2, pos),
        regions,
    );
    assert_eq!(different, EpochDecision::Split);
    let lobbies = session(|pos| flat(pos, -61, 3), |pos| flat(pos, -61, 3), regions);
    assert_eq!(
        lobbies,
        EpochDecision::Candidate,
        "identical flat maps abstain"
    );
    let void = session(RegionSignature::new, RegionSignature::new, regions);
    assert_eq!(void, EpochDecision::Candidate, "void arenas abstain");
}

#[test]
fn region_signatures_hold_only_their_own_chunks() {
    let pos = RegionPos::of_chunk(-1, 4);
    assert_eq!(pos, RegionPos { x: -1, z: 1 });
    let mut signature = RegionSignature::new(pos);
    assert_eq!(signature.pos(), pos);
    assert!(signature.chunk(-1, 4).is_none());
    assert!(signature.record(-4, 7, flat_chunk(1, 1)));
    assert!(!signature.record(0, 4, flat_chunk(1, 1)));
    assert!(!signature.record(-1, 8, flat_chunk(1, 1)));
    assert_eq!(signature.chunk(-4, 7), Some(&flat_chunk(1, 1)));
    assert!(signature.chunk(0, 4).is_none());
    assert!(signature.record(-4, 7, flat_chunk(2, 1)));
    assert_eq!(signature.chunk(-4, 7), Some(&flat_chunk(2, 1)));
}

#[test]
fn signatures_round_trip_through_their_persisted_form() {
    let pos = RegionPos { x: -2, z: 5 };
    let mut partial = world_region(3, pos);
    // Leave one chunk unrecorded and one recorded without any surface.
    partial.chunks[3] = None;
    partial.chunks[9] = Some(ChunkSurface::default());
    for signature in [RegionSignature::new(pos), world_region(3, pos), partial] {
        let mut bytes = Vec::new();
        signature.encode_into(&mut bytes);
        assert!(bytes.len() <= MAX_ENCODED_SIGNATURE_BYTES);
        assert_eq!(RegionSignature::decode(pos, &bytes), Ok(signature));
    }
}

#[test]
fn malformed_persisted_signatures_are_rejected() {
    let pos = RegionPos { x: 0, z: 0 };
    let mut bytes = Vec::new();
    world_region(1, pos).encode_into(&mut bytes);
    assert_eq!(
        RegionSignature::decode(pos, &bytes[..1]),
        Err(SignatureError::Truncated(1))
    );
    assert_eq!(
        RegionSignature::decode(pos, &bytes[..bytes.len() - 1]),
        Err(SignatureError::Length {
            expected: MAX_ENCODED_SIGNATURE_BYTES,
            actual: MAX_ENCODED_SIGNATURE_BYTES - 1
        })
    );
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(matches!(
        RegionSignature::decode(pos, &extra),
        Err(SignatureError::Length { .. })
    ));
}

#[test]
fn merging_keeps_older_chunks_the_newer_signature_lacks() {
    let pos = RegionPos { x: 1, z: 1 };
    let mut stored = world_region(1, pos);
    let mut newer = RegionSignature::new(pos);
    assert!(newer.record(4, 4, flat_chunk(9, 9)));
    stored.merge_from(&newer);
    assert_eq!(stored.chunk(4, 4), Some(&flat_chunk(9, 9)));
    assert_eq!(stored.chunk(5, 4), world_region(1, pos).chunk(5, 4));
    let recorded: Vec<_> = newer.recorded_chunks().map(|(x, z, _)| (x, z)).collect();
    assert_eq!(recorded, [(4, 4)]);
    assert!(!newer.is_empty());
    assert!(RegionSignature::new(pos).is_empty());
}

#[test]
fn regions_name_the_section_columns_that_overlap_them() {
    let region = RegionPos { x: -1, z: 2 };
    // Blocks -64..0 and 128..192.
    assert_eq!(region.section_columns(0), [-2..=-1, 4..=5]);
    assert_eq!(region.section_columns(1), [-1..=-1, 2..=2]);
    assert_eq!(region.section_columns(2), [-1..=-1, 1..=1]);
    assert_eq!(region.section_columns(MAX_LEVEL), [-1..=-1, 0..=0]);
}
