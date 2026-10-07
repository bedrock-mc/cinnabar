use super::*;

fn origins(
    block: [i32; 3],
    neighbors: [BlockFlags; BlockFace::ALL.len()],
    below_is_campfire: bool,
) -> (Vec<[f32; 3]>, AmbientRandom) {
    let mut random = AmbientRandom::new(5489);
    let mut positions = Vec::new();
    emit_fire_smoke(
        block,
        neighbors,
        below_is_campfire,
        &mut random,
        |position| positions.push(position),
    );
    (positions, random)
}

#[test]
fn native_smoke_random_float_rounding_consumes_one_mt_word_per_coordinate() {
    // Independently rounded witnesses for vanilla's random float's double
    // intermediate, rather than a float-first conversion or a 24-bit truncation.
    let mut random = AmbientRandom::new(5489);
    for expected in [
        0x3f50_91bb,
        0x3e0a_ba7c,
        0x3f67_e1fb,
        0x3f55_c31f,
        0x3e02_08d5,
    ] {
        assert_eq!(random.unit().to_bits(), expected);
    }
    assert_eq!(random.next(), 4_161_255_391);
}

#[test]
fn supported_fire_emits_three_native_origins_in_upper_half_with_no_extra_chance_roll() {
    for below in [BlockFlags::FIRE_TOP_SUPPORT, BlockFlags::FIRE_FLAMMABLE] {
        let mut neighbors = [BlockFlags::FIRE_FLAMMABLE; BlockFace::ALL.len()];
        neighbors[BlockFace::Down as usize] = below;
        let (positions, mut random) = origins([0, 0, 0], neighbors, false);
        assert_eq!(positions.len(), 3, "native ordinary smoke count");
        assert_eq!(positions[0][0].to_bits(), 0x3f50_91bb);
        assert_eq!(positions[0][2].to_bits(), 0x3f67_e1fb);
        let expected_y = f32::from_bits(0x3e0a_ba7c) * 0.5 + 0.5;
        assert_eq!(positions[0][1], expected_y);
        for [x, y, z] in positions {
            assert!((0.0..=1.0).contains(&x));
            assert!((0.5..=1.0).contains(&y));
            assert!((0.0..=1.0).contains(&z));
        }
        // Three XYZ draws consume exactly nine words, and no probability roll.
        assert_eq!(random.next(), 1_323_567_403);
    }
}

#[test]
fn native_may_place_makes_valid_wall_attached_fire_use_the_three_smoke_branch() {
    for face in ATTACHMENT_ORDER {
        let mut neighbors = [BlockFlags::empty(); BlockFace::ALL.len()];
        neighbors[face as usize] = BlockFlags::FIRE_FLAMMABLE;
        let (positions, _) = origins([-20, -60, 40], neighbors, false);
        assert_eq!(positions.len(), 3, "{face:?}: native mayPlace succeeds");
        assert!(
            positions
                .iter()
                .all(|position| (-59.5..=-59.0).contains(&position[1]))
        );
    }
}

#[test]
fn rejected_campfire_placement_emits_two_smoke_origins_per_combustible_attachment() {
    let mut neighbors = [BlockFlags::FIRE_FLAMMABLE; BlockFace::ALL.len()];
    neighbors[BlockFace::Down as usize] = BlockFlags::empty();
    let (positions, _) = origins([0; 3], neighbors, true);
    assert_eq!(positions.len(), 10);
    // Current callback emits west, east, north, south, above, two per face.
    for (face, pair) in ATTACHMENT_ORDER.into_iter().zip(positions.chunks_exact(2)) {
        for position in pair {
            assert!(
                position
                    .iter()
                    .all(|component| (0.0..=1.0).contains(component))
            );
            let (axis, band) = match face {
                BlockFace::West => (0, 0.0..=0.1),
                BlockFace::East => (0, 0.9..=1.0),
                BlockFace::North => (2, 0.0..=0.1),
                BlockFace::South => (2, 0.9..=1.0),
                BlockFace::Up => (1, 0.9..=1.0),
                BlockFace::Down => unreachable!(),
            };
            assert!(band.contains(&position[axis]), "{face:?}: {position:?}");
        }
    }
    assert_eq!(positions[0][0], f32::from_bits(0x3f50_91bb) * 0.1);
    assert_eq!(positions[0][1].to_bits(), 0x3e0a_ba7c);
}

#[test]
fn air_unsupported_fire_has_no_smoke_and_does_not_consume_randomness() {
    let (positions, mut random) =
        origins([0; 3], [BlockFlags::empty(); BlockFace::ALL.len()], false);
    assert!(positions.is_empty());
    assert_eq!(random.next(), 3_499_211_612);
}

#[test]
fn native_support_short_circuits_campfire_rejection_before_attachment_emission() {
    let mut neighbors = [BlockFlags::FIRE_FLAMMABLE; BlockFace::ALL.len()];
    neighbors[BlockFace::Down as usize] = BlockFlags::FIRE_TOP_SUPPORT;
    assert_eq!(origins([0; 3], neighbors, true).0.len(), 3);
}
