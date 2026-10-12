use super::*;

#[test]
fn covered_snow_cannot_accumulate_a_third_primary() {
    let snow = ResolvedPaletteEntry {
        network_value: 1,
        sequential_id: Some(1),
        kind: VisualKind::Model,
        variant: BLOCK_VISUAL_VARIANT_TOP_SNOW,
        ..ResolvedPaletteEntry::DIAGNOSTIC
    };
    let plant = ResolvedPaletteEntry {
        network_value: 2,
        sequential_id: Some(2),
        kind: VisualKind::Cross,
        ..ResolvedPaletteEntry::DIAGNOSTIC
    };
    for order in [[snow, plant], [plant, snow]] {
        let mut contributors = ResolvedContributors::default();
        for entry in order {
            contributors.push(entry);
        }
        assert_eq!(
            contributors.primary_network_value(),
            Some(snow.network_value)
        );
        assert_eq!(
            contributors.covered_plant.unwrap().network_value,
            plant.network_value
        );
        contributors.push(plant);
        assert_eq!(contributors.primary_network_value(), None);
        assert!(contributors.covered_plant.is_none());
        assert_eq!(
            contributors.diagnostic_network_value(),
            Some(plant.network_value)
        );
    }
}

/// A two-layer section: a three-bit layer, whose words hold ten indices, over a one-bit layer.
fn mixed_section() -> world::SubChunk {
    fn var_i32(out: &mut Vec<u8>, value: i32) {
        let mut encoded = ((value << 1) ^ (value >> 31)) as u32;
        loop {
            let byte = (encoded & 0x7f) as u8;
            encoded >>= 7;
            if encoded == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }
    fn layer(out: &mut Vec<u8>, bits: u32, palette: &[i32], mut index: impl FnMut(usize) -> u32) {
        out.push(((bits as u8) << 1) | 1);
        let per_word = 32 / bits as usize;
        for word in 0..4096_usize.div_ceil(per_word) {
            let mut packed = 0_u32;
            for slot in 0..per_word {
                let linear = word * per_word + slot;
                if linear < 4096 {
                    packed |= index(linear) << (slot as u32 * bits);
                }
            }
            out.extend(packed.to_le_bytes());
        }
        var_i32(out, palette.len() as i32);
        for &entry in palette {
            var_i32(out, entry);
        }
    }
    let mut seed = 0x9e37_79b9_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut bytes = vec![8, 2];
    layer(&mut bytes, 3, &[0, 1, 2, 3, 4], |_| next() % 5);
    layer(&mut bytes, 1, &[0, 2], |linear| u32::from(linear % 7 == 0));
    world::SubChunk::decode(&bytes, &world::RawBlockIds { air: 0 })
}

// The mesher decodes its section once; every voxel, and any coordinate outside the section, must
// answer exactly as the packed lookups do.
#[test]
fn decoded_voxels_answer_as_packed_lookups() {
    let section = mixed_section();
    let visuals = assets::RuntimeAssets::diagnostic();
    let facts = || {
        PaletteFacts::new(
            BlockClassifier::new(0),
            &visuals,
            NetworkIdMode::Sequential,
            &section,
        )
    };
    let packed = facts();
    let mut decoded = facts();
    decoded.decode_voxels();
    assert!(decoded.decoded.is_some());
    for x in 0..=SIDE {
        for y in 0..SIDE {
            for z in 0..SIDE {
                assert_eq!(
                    format!("{:?}", decoded.at(x, y, z)),
                    format!("{:?}", packed.at(x, y, z)),
                    "voxel {x},{y},{z}"
                );
                assert_eq!(
                    decoded.seasonal_shelters_at(x, y, z),
                    packed.seasonal_shelters_at(x, y, z)
                );
            }
        }
    }
}
