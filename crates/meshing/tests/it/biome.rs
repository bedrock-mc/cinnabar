use std::sync::Arc;

use meshing::{
    BIOME_NEIGHBOUR_SLOT_COUNT, MAX_PACKED_BIOME_RECORD_WORDS, PackedBiomeRecord,
    biome_neighbour_index, biome_volume_index,
};
use world::{DecodedBiomeColumn, RawBiomeIds};

const BIOMES: RawBiomeIds = RawBiomeIds { default_biome: 0 };

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

#[test]
fn uniform_record_remaps_only_the_palette() {
    let mut payload = vec![1];
    payload.extend(zig_zag_i32(42));
    let storage = DecodedBiomeColumn::decode(-4, 1, &payload, &BIOMES)
        .storage(-4)
        .unwrap();

    let record = PackedBiomeRecord::from_storage(&storage, |id| id + 1000);

    assert_eq!(
        &record.words()[record.words()[6] as usize..],
        &[1 << 8, 1042]
    );
    assert_eq!(record.bits_per_index(), 0);
    assert_eq!(record.palette_len(), 1);
    assert_eq!(record.tint_index(15, 15, 15), Some(1042));
}

#[test]
fn packed_record_preserves_bedrock_words_and_xzy_lookup() {
    let mut payload = vec![3]; // Network palette, one bit per index.
    let mut packed = vec![0_u32; 128];
    let linear = (1_usize << 8) | (3_usize << 4) | 2;
    packed[linear / 32] |= 1 << (linear % 32);
    for word in &packed {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend(zig_zag_i32(2));
    payload.extend(zig_zag_i32(7));
    payload.extend(zig_zag_i32(9));
    let storage = DecodedBiomeColumn::decode(0, 1, &payload, &BIOMES)
        .storage(0)
        .unwrap();

    let record = PackedBiomeRecord::from_storage(&storage, |id| id * 10);

    assert_eq!(record.bits_per_index(), 1);
    assert_eq!(record.palette_len(), 2);
    assert_eq!(
        &record.words()[record.words()[6] as usize + 1..record.words()[6] as usize + 129],
        packed.as_slice()
    );
    assert_eq!(
        &record.words()[record.words()[6] as usize + 129..],
        &[70, 90]
    );
    assert_eq!(record.tint_index(1, 2, 3), Some(90));
    assert_eq!(record.tint_index(1, 3, 2), Some(70));
}

#[test]
fn fallback_record_is_a_valid_uniform_palette() {
    let record = PackedBiomeRecord::fallback();

    assert_eq!(&record.words()[record.words()[6] as usize..], &[1 << 8, 0]);
    assert_eq!(record.tint_index(0, 0, 0), Some(0));
    assert_eq!(record.tint_index(16, 0, 0), None);
}

fn uniform_storage(id: i32) -> Arc<world::BiomeStorage> {
    let mut payload = vec![1];
    payload.extend(zig_zag_i32(id));
    DecodedBiomeColumn::decode(0, 1, &payload, &BIOMES)
        .storage(0)
        .unwrap()
}

#[test]
fn neighbourhood_record_samples_all_cross_chunk_slots_without_flattening() {
    let center = uniform_storage(10);
    let east = uniform_storage(20);
    let north_west = uniform_storage(30);
    let mut halo: [Option<Arc<world::BiomeStorage>>; BIOME_NEIGHBOUR_SLOT_COUNT] =
        std::array::from_fn(|_| None);
    halo[biome_neighbour_index(0, 0).unwrap()] = Some(center);
    halo[biome_neighbour_index(1, 0).unwrap()] = Some(east);
    halo[biome_neighbour_index(-1, -1).unwrap()] = Some(north_west);

    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id + 1_000);

    assert_eq!(record.tint_index_at([15, 7, 8]), Some(1_010));
    assert_eq!(record.tint_index_at([16, 7, 8]), Some(1_020));
    assert_eq!(record.tint_index_at([-1, 7, -1]), Some(1_030));
}

#[test]
fn every_horizontal_boundary_and_corner_uses_its_exact_neighbour_slot() {
    let halo = std::array::from_fn(|slot| Some(uniform_storage(slot as i32 + 1)));
    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id);

    for dz in -1_i8..=1 {
        for dx in -1_i8..=1 {
            let coordinate = [
                match dx {
                    -1 => -1,
                    0 => 0,
                    _ => 16,
                },
                8,
                match dz {
                    -1 => -1,
                    0 => 0,
                    _ => 16,
                },
            ];
            let slot = biome_neighbour_index(dx, dz).unwrap();
            assert_eq!(record.tint_index_at(coordinate), Some(slot as u32 + 1));
        }
    }
}

#[test]
fn missing_neighbour_uses_the_vanilla_zero_biome_sample() {
    let mut payload = vec![3];
    let mut packed = vec![0_u32; 128];
    for z in 0..16_usize {
        for y in 0..16_usize {
            let linear = (15 << 8) | (z << 4) | y;
            packed[linear / 32] |= 1 << (linear % 32);
        }
    }
    for word in packed {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend(zig_zag_i32(2));
    payload.extend(zig_zag_i32(7));
    payload.extend(zig_zag_i32(9));
    let center = DecodedBiomeColumn::decode(0, 1, &payload, &BIOMES)
        .storage(0)
        .unwrap();
    let mut halo = std::array::from_fn(|_| None);
    halo[biome_neighbour_index(0, 0).unwrap()] = Some(center);

    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id * 10);

    assert_eq!(record.tint_index_at([-1, 4, 8]), Some(0));
    assert_eq!(record.tint_index_at([16, 4, 8]), Some(0));
}

#[test]
fn identical_neighbour_payloads_are_deduplicated_and_uniform_fast_path_is_recorded() {
    let storage = uniform_storage(42);
    let halo = std::array::from_fn(|_| Some(Arc::clone(&storage)));

    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id + 1_000);

    assert_eq!(
        record.words().len(),
        meshing::biome_lattice::DESCRIPTOR_WORDS + 2
    );
    assert_eq!(record.uniform_tint_index(), Some(1_042));
    assert_eq!(record.byte_len(), record.words().len() as u64 * 4);
    assert!(record.words().len() <= MAX_PACKED_BIOME_RECORD_WORDS);
}

#[test]
fn vertical_biome_neighbour_is_not_rejected() {
    let record = PackedBiomeRecord::from_storage(&uniform_storage(7), |id| id);
    assert_eq!(record.tint_index_at([8, 16, 8]), Some(0));
}

/// Resolves a binary palette to the second biome's contribution.
fn boundary_weight(record: &PackedBiomeRecord, coordinate: [i32; 3]) -> f32 {
    record
        .blend_samples(coordinate)
        .unwrap()
        .iter()
        .filter(|s| s.tint_index == 2)
        .map(|s| s.weight)
        .sum()
}

/// Constructs a complete volume with a planar biome boundary at world X=16.
fn boundary_record() -> PackedBiomeRecord {
    let halo = std::array::from_fn(|slot| Some(uniform_storage(if slot % 3 == 2 { 2 } else { 1 })));
    PackedBiomeRecord::from_neighbourhood(&halo, |id| id)
}

#[test]
fn two_biome_boundary_matches_vanilla_lattice_and_distance_weights() {
    let record = boundary_record();
    let expected = [
        0.0, 0.06575897, 0.13483617, 0.24291475, 0.33333334, 0.3744327, 0.4681695, 0.5762481,
    ];
    for (x, value) in (8..16).zip(expected) {
        assert!(
            (boundary_weight(&record, [x, 8, 8]) - value).abs() < 0.00001,
            "column {x}"
        );
    }
    let mut west =
        std::array::from_fn(|slot| Some(uniform_storage(if slot % 3 == 0 { 1 } else { 2 })));
    let east = PackedBiomeRecord::from_neighbourhood(&west, |id| id);
    assert!((boundary_weight(&east, [0, 8, 8]) - 2.0 / 3.0).abs() < 0.00001);
    west[biome_neighbour_index(-1, 0).unwrap()] = None;
    let missing = PackedBiomeRecord::from_neighbourhood(&west, |id| id);
    assert_ne!(missing, east);
}

#[test]
fn vertical_boundary_uses_the_same_kernel() {
    let halo = std::array::from_fn(|slot| Some(uniform_storage(if slot >= 18 { 2 } else { 1 })));
    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id);
    assert!((boundary_weight(&record, [8, 12, 8]) - 1.0 / 3.0).abs() < 0.00001);
    assert!((boundary_weight(&record, [8, 15, 8]) - 0.5762481).abs() < 0.00001);
    assert_eq!(record.tint_index_at([8, 16, 8]), Some(2));
    assert_eq!(biome_volume_index(0, 1, 0), Some(22));
}

#[test]
fn negative_cache_cells_preserve_vanilla_truncation_and_distance_ties() {
    let points = meshing::biome_lattice::nearest_lattice_points([1, 8, 8]);
    assert_eq!(
        points.map(|point| point.0),
        [
            [0, 8, 8],
            [4, 8, 8],
            [0, 4, 8],
            [0, 8, 4],
            [0, 8, 12],
            [0, 12, 8],
            [4, 4, 8],
            [4, 8, 4],
        ]
    );
    assert!((points.iter().map(|point| point.1).sum::<f32>() - 1.0).abs() < 0.00001);
}

#[test]
fn offline_biome_boundary_gallery() {
    let default_directory = tempfile::tempdir().unwrap();
    let directory = std::env::var_os("CINNABAR_BIOME_GALLERY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| default_directory.path().to_path_buf());
    std::fs::create_dir_all(&directory).unwrap();
    let west = boundary_record();
    let halo = std::array::from_fn(|slot| Some(uniform_storage(if slot % 3 == 0 { 1 } else { 2 })));
    let east = PackedBiomeRecord::from_neighbourhood(&halo, |id| id);
    for after in [false, true] {
        let weights = (0..32 * 16)
            .map(|index| {
                let column = index % 32;
                let z = index / 32;
                if after {
                    boundary_weight(if column < 16 { &west } else { &east }, [column % 16, 8, z])
                } else {
                    let base = column / 4 * 4;
                    let t = (column % 4) as f32 / 4.0;
                    let taps = [(1.0 - t) / 3.0, 1.0 / 3.0, 1.0 / 3.0, t / 3.0];
                    (0..4)
                        .filter(|&tap| base + (tap as i32 - 1) * 4 >= 16)
                        .map(|tap| taps[tap])
                        .sum()
                }
            })
            .collect::<Vec<_>>();
        let mut image = image::RgbImage::new(1024, 512);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let weight = weights[((y / 32) * 32 + x / 32) as usize];
            let checker = if (x / 8 + y / 8) % 2 == 0 { 1.0 } else { 0.92 };
            let a = [160.0, 190.0, 80.0];
            let b = [80.0, 150.0, 130.0];
            *pixel = image::Rgb(std::array::from_fn(|i| {
                ((a[i] * (1.0 - weight) + b[i] * weight) * checker) as u8
            }));
        }
        image
            .save(directory.join(if after { "after.png" } else { "before.png" }))
            .unwrap();
    }
}

/// Encodes a planar boundary inside one subchunk without expanding stored biomes.
fn split_storage() -> Arc<world::BiomeStorage> {
    let mut payload = vec![3];
    for word in 0..128 {
        payload.extend_from_slice(&(if word >= 64 { u32::MAX } else { 0_u32 }).to_le_bytes());
    }
    payload.extend(zig_zag_i32(2));
    payload.extend(zig_zag_i32(1));
    payload.extend(zig_zag_i32(2));
    DecodedBiomeColumn::decode(0, 1, &payload, &BIOMES)
        .storage(0)
        .unwrap()
}

#[test]
fn boundary_inside_a_chunk_has_the_same_gradient_as_a_chunk_edge() {
    let halo = std::array::from_fn(|slot| {
        Some(match slot % 3 {
            0 => uniform_storage(1),
            1 => split_storage(),
            _ => uniform_storage(2),
        })
    });
    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id);
    for (x, expected) in [(4, 1.0 / 3.0), (7, 0.5762481), (8, 2.0 / 3.0), (12, 1.0)] {
        assert!(
            (boundary_weight(&record, [x, 8, 8]) - expected).abs() < 0.00001,
            "column {x}"
        );
    }
}

#[test]
fn review_render_uniform_sentinel_tint_keeps_a_valid_blending_record() {
    let storage = DecodedBiomeColumn::decode(-4, 1, &[1, 0], &BIOMES)
        .storage(-4)
        .unwrap();
    let halo = std::array::from_fn(|_| Some(Arc::clone(&storage)));
    let record = PackedBiomeRecord::from_neighbourhood(&halo, |_| u32::MAX);
    let samples = record.blend_samples([8, 8, 8]).unwrap();
    let total: f32 = samples.iter().map(|sample| sample.weight).sum();
    assert!((total - 1.0).abs() < 1e-5);
    assert!(
        samples
            .iter()
            .filter(|sample| sample.weight > 0.0)
            .all(|sample| sample.tint_index == u32::MAX)
    );
}

/// A one-bit storage whose cells alternate between `a` and `b` in coarse slabs along X and Z.
fn striped_storage(a: i32, b: i32) -> Arc<world::BiomeStorage> {
    let mut payload = vec![3]; // Network palette, one bit per index.
    let mut packed = vec![0_u32; 128];
    for linear in 0..4096_usize {
        let (x, z) = (linear >> 8, (linear >> 4) & 15);
        if (x / 5 + z / 7) % 2 == 1 {
            packed[linear / 32] |= 1 << (linear % 32);
        }
    }
    for word in &packed {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend(zig_zag_i32(2));
    payload.extend(zig_zag_i32(a));
    payload.extend(zig_zag_i32(b));
    DecodedBiomeColumn::decode(0, 1, &payload, &BIOMES)
        .storage(0)
        .unwrap()
}

/// Every lattice point holds the four most frequent tints of its 27 samples, in first-seen order
/// among equal counts, exactly as counted from `tint_index_at`.
#[test]
fn lattice_points_count_the_tints_their_samples_read() {
    use meshing::biome_lattice::{
        BIOME_LATTICE_STEP, LATTICE_BIOME_LIMIT, LATTICE_SIDE, LATTICE_STENCIL_SAMPLES,
    };
    let halo: [Option<Arc<world::BiomeStorage>>; BIOME_NEIGHBOUR_SLOT_COUNT] =
        std::array::from_fn(|slot| match slot % 5 {
            0 => None,
            1 => Some(striped_storage(3, 11)),
            2 => Some(uniform_storage(5)),
            3 => Some(striped_storage(11, 7)),
            _ => Some(uniform_storage(3)),
        });
    let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id + 100);
    assert_eq!(record.uniform_tint_index(), None);
    for x in 0..LATTICE_SIDE {
        for y in 0..LATTICE_SIDE {
            for z in 0..LATTICE_SIDE {
                let mut counts: Vec<(u32, u32)> = Vec::new();
                for dz in 0..3 {
                    for dy in 0..3 {
                        for dx in 0..3 {
                            let sample = [x + dx, y + dy, z + dz]
                                .map(|v| (v as i32 - 2) * BIOME_LATTICE_STEP);
                            let tint = record.tint_index_at(sample).unwrap_or(0);
                            match counts.iter_mut().find(|(seen, _)| *seen == tint) {
                                Some((_, count)) => *count += 1,
                                None => counts.push((tint, 1)),
                            }
                        }
                    }
                }
                counts.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
                let position = [x, y, z].map(|v| (v as i32 - 1) * BIOME_LATTICE_STEP);
                let samples = record.lattice_samples(position);
                assert_eq!(
                    samples.iter().filter(|sample| sample.weight > 0.0).count(),
                    counts.len().min(LATTICE_BIOME_LIMIT)
                );
                for (sample, &(tint, count)) in
                    samples.iter().zip(counts.iter().take(LATTICE_BIOME_LIMIT))
                {
                    assert_eq!(sample.tint_index, tint, "point {position:?}");
                    assert_eq!(
                        sample.weight.to_bits(),
                        (count as f32 / LATTICE_STENCIL_SAMPLES as f32).to_bits()
                    );
                }
            }
        }
    }
}

#[test]
fn compact_lattice_preserves_all_stencil_fraction_bits_and_full_width_indices() {
    use meshing::biome_lattice::{LATTICE_POINT_WORDS, WIDE_LATTICE_POINT_WORDS};
    let halo = std::array::from_fn(|slot| Some(uniform_storage(slot as i32 + 1)));
    for (offset, words) in [(0, LATTICE_POINT_WORDS), (65_536, WIDE_LATTICE_POINT_WORDS)] {
        let record = PackedBiomeRecord::from_neighbourhood(&halo, |id| id + offset);
        assert_eq!(record.lattice_point_words(), words);
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    let points = meshing::biome_lattice::nearest_lattice_points([x, y, z]);
                    let samples = record.blend_samples([x, y, z]).unwrap();
                    for (point, (position, weight)) in points.into_iter().enumerate() {
                        let mut counts: Vec<(u32, u32)> = Vec::new();
                        for dz in -1..=1 {
                            for dy in -1..=1 {
                                for dx in -1..=1 {
                                    let tint = record
                                        .tint_index_at([
                                            position[0]
                                                + dx * meshing::biome_lattice::BIOME_LATTICE_STEP,
                                            position[1]
                                                + dy * meshing::biome_lattice::BIOME_LATTICE_STEP,
                                            position[2]
                                                + dz * meshing::biome_lattice::BIOME_LATTICE_STEP,
                                        ])
                                        .unwrap();
                                    if let Some((_, count)) =
                                        counts.iter_mut().find(|(value, _)| *value == tint)
                                    {
                                        *count += 1;
                                    } else {
                                        counts.push((tint, 1));
                                    }
                                }
                            }
                        }
                        counts.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
                        for (i, sample) in samples[point * 4..point * 4 + 4].iter().enumerate() {
                            let expected = counts.get(i).map_or((0, 0.0), |&(tint, count)| {
                                (
                                    tint,
                                    weight
                                        * (count as f32
                                            / meshing::biome_lattice::LATTICE_STENCIL_SAMPLES
                                                as f32),
                                )
                            });
                            assert_eq!(sample.tint_index, expected.0);
                            assert_eq!(
                                sample.weight.to_bits(),
                                expected.1.to_bits(),
                                "query {:?} point {position:?}",
                                [x, y, z]
                            );
                        }
                    }
                }
            }
        }
    }
    let fractions = meshing::biome_lattice::lattice_fraction_words();
    for (count, bits) in fractions.into_iter().enumerate() {
        assert_eq!(
            bits,
            (count as f32 / meshing::biome_lattice::LATTICE_STENCIL_SAMPLES as f32).to_bits()
        );
    }
    // The largest compact index must fit both halves of each tint word.
    let edge = PackedBiomeRecord::from_neighbourhood(&halo, |id| u32::from(u16::MAX) + 1 - id);
    assert_eq!(edge.lattice_point_words(), LATTICE_POINT_WORDS);
    assert_eq!(edge.tint_index_at([-1, 8, -1]), Some(u32::from(u16::MAX)));
    assert!(
        edge.blend_samples([0, 8, 0])
            .unwrap()
            .iter()
            .any(|sample| sample.tint_index == u32::from(u16::MAX) && sample.weight > 0.0)
    );
    for sample in edge
        .blend_samples([15, 15, 15])
        .unwrap()
        .into_iter()
        .filter(|sample| sample.weight > 0.0)
    {
        assert!(sample.tint_index >= u32::from(u16::MAX) - BIOME_NEIGHBOUR_SLOT_COUNT as u32);
    }
}
