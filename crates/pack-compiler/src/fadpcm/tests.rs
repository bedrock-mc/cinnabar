use super::*;

fn block(coefficients: u32, shifts: u32, first: i16, second: i16, data: u8) -> Vec<u8> {
    let mut bytes = Vec::from(coefficients.to_le_bytes());
    bytes.extend(shifts.to_le_bytes());
    bytes.extend(first.to_le_bytes());
    bytes.extend(second.to_le_bytes());
    bytes.resize(140, data);
    bytes
}

fn bank(channels: u8, frames: u32, blocks: &[u8], rate: Option<u32>) -> Vec<u8> {
    let header_size = if rate.is_some() { 16_u32 } else { 8 };
    let mut bytes = Vec::from(*b"FSB5");
    for value in [1, 1, header_size, 0, blocks.len() as u32, 16, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.resize(60, 0);
    let mode = (u64::from(frames) << 34)
        | (u64::from(channels - 1) << 5)
        | (9 << 1)
        | u64::from(rate.is_some());
    bytes.extend(mode.to_le_bytes());
    if let Some(rate) = rate {
        bytes.extend(((2_u32 << 25) | (4 << 1)).to_le_bytes());
        bytes.extend(rate.to_le_bytes());
    }
    bytes.extend(blocks);
    bytes
}

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn signed_low_nibble_first_goldens_and_exact_metadata() {
    let mut encoded = block(0, 0, 0, 0, 0);
    for (index, byte) in encoded[12..].iter_mut().enumerate() {
        *byte = [0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe][index % 8];
    }
    let input = bank(1, 16, &encoded, None);
    let decoded = decode_fsb5_fadpcm(&input).unwrap();
    assert_eq!(decoded.channels(), 1);
    assert_eq!(decoded.sample_rate(), 48_000);
    assert_eq!(decoded.frames(), 16);
    assert_eq!(
        decoded.samples(),
        &[0, 1, 2, 3, 4, 5, 6, 7, -8, -7, -6, -5, -4, -3, -2, -1]
    );
    assert_eq!(decode_fsb5_fadpcm(&input).unwrap(), decoded);
}

#[test]
fn predictors_modulo_selectors_and_clamping_have_independent_goldens() {
    let expected = [0, 937, 1437, 1390, 1101, 0, 0];
    for selector in 0..16 {
        let decoded =
            decode_fsb5_fadpcm(&bank(1, 1, &block(selector, 0, 1000, 500, 0), None)).unwrap();
        assert_eq!(decoded.samples(), &[expected[selector as usize % 7]]);
    }
    let decoded = decode_fsb5_fadpcm(&bank(1, 8, &block(1, 0, 1024, 0, 0), None)).unwrap();
    assert_eq!(decoded.samples(), &[960, 900, 843, 790, 740, 693, 649, 608]);
    let decoded = decode_fsb5_fadpcm(&bank(1, 4, &block(2, 0, 1000, 500, 0), None)).unwrap();
    assert_eq!(decoded.samples(), &[1437, 1801, 2085, 2286]);
    let mut clipped = block(0, 15, 0, 0, 0);
    clipped[12] = 0x87;
    clipped[13] = 0x1f;
    assert_eq!(
        decode_fsb5_fadpcm(&bank(1, 4, &clipped, None))
            .unwrap()
            .samples(),
        &[32767, -32768, -32768, 32767]
    );
}

#[test]
fn stereo_interleave_partial_final_frame_and_history_reset() {
    let mut blocks = block(0, 0, 0, 0, 0x21);
    blocks.extend(block(0, 0, 0, 0, 0xfe));
    assert_eq!(
        decode_fsb5_fadpcm(&bank(2, 3, &blocks, Some(29_015)))
            .unwrap()
            .samples(),
        &[1, -2, 2, -1, 1, -2]
    );
    blocks.extend(block(0, 0, 0, 0, 0x33));
    blocks.extend(block(0, 0, 0, 0, 0xcc));
    let decoded = decode_fsb5_fadpcm(&bank(2, 257, &blocks, Some(29_015))).unwrap();
    assert_eq!(decoded.sample_rate(), 29_015);
    assert_eq!(decoded.samples().len(), 514);
    assert_eq!(&decoded.samples()[512..], &[3, -4]);
    let mut reset = block(1, 0, 1024, 0, 0);
    reset.extend(block(1, 0, 2048, 0, 0));
    let decoded = decode_fsb5_fadpcm(&bank(1, 257, &reset, None)).unwrap();
    assert_eq!(decoded.samples()[0], 960);
    assert_eq!(decoded.samples()[256], 1920);
}

#[test]
fn every_group_reads_its_own_predictor_and_shift_digit() {
    let selectors = 0x0000_0001;
    let shifts = 0x7654_3210;
    let decoded = decode_fsb5_fadpcm(&bank(
        1,
        256,
        &block(selectors, shifts, 1024, 0, 0x11),
        None,
    ))
    .unwrap();
    assert_eq!(decoded.samples()[0], 961);
    // Subsequent groups use the zero predictor rather than preceding history.
    for group in 1..8 {
        assert_eq!(decoded.samples()[group * 32], 1_i16 << group);
        assert_eq!(decoded.samples()[group * 32 + 31], 1_i16 << group);
    }
}

#[test]
fn rate_endpoints_and_decoded_size_ceiling_are_explicit() {
    for rate in [4000, 96_000] {
        assert_eq!(
            decode_fsb5_fadpcm(&bank(1, 1, &block(0, 0, 0, 0, 0), Some(rate)))
                .unwrap()
                .sample_rate(),
            rate
        );
    }
    let input = bank(2, (MAX_PCM_BYTES / 4 + 1) as u32, &[], None);
    assert_eq!(
        decode_fsb5_fadpcm(&input),
        Err(FadpcmDecodeError::DecodedTooLarge)
    );
    let mut bad = bank(1, 1, &block(0, 0, 0, 0, 0), None);
    bad[0] = b'x';
    assert!(matches!(
        decode_fsb5_fadpcm(&bad),
        Err(FadpcmDecodeError::Malformed(_))
    ));
}

#[test]
fn zero_block_and_only_zero_alignment_padding_are_valid() {
    let mut input = bank(1, 256, &block(0, 0, 0, 0, 0), None);
    let decoded = decode_fsb5_fadpcm(&input).unwrap();
    assert!(decoded.samples().iter().all(|sample| *sample == 0));
    input.resize(input.len() + 20, 0);
    word(&mut input, 20, 160);
    assert!(decode_fsb5_fadpcm(&input).is_ok());
    *input.last_mut().unwrap() = 1;
    assert!(decode_fsb5_fadpcm(&input).is_err());
    input.pop();
    word(&mut input, 20, 159);
    assert!(decode_fsb5_fadpcm(&input).is_err());
    assert!(decode_fsb5_fadpcm(&bank(1, 1, &vec![0; 280], None)).is_err());
}

#[test]
fn every_truncation_and_hostile_header_fails_closed() {
    let good = bank(1, 1, &block(0, 0, 0, 0, 0), Some(29_015));
    for length in 0..good.len() {
        assert!(
            decode_fsb5_fadpcm(&good[..length]).is_err(),
            "prefix {length}"
        );
    }
    for (offset, value) in [
        (4, 2),
        (8, 2),
        (12, u32::MAX),
        (16, u32::MAX),
        (20, u32::MAX),
        (24, 15),
        (28, 1),
        (32, 1),
        (72, 0),
        (72, 96_001),
    ] {
        let mut bad = good.clone();
        word(&mut bad, offset, value);
        assert!(
            decode_fsb5_fadpcm(&bad).is_err(),
            "offset {offset} value {value}"
        );
    }
    for mode in [
        0,
        (1_u64 << 34) | (2 << 5),
        (1_u64 << 34) | (1 << 7),
        (1_u64 << 34) | (15 << 1),
        0x3fff_ffff_u64 << 34,
    ] {
        let mut bad = bank(1, 1, &block(0, 0, 0, 0, 0), None);
        bad[60..68].copy_from_slice(&mode.to_le_bytes());
        assert!(decode_fsb5_fadpcm(&bad).is_err());
    }
    assert!(matches!(
        decode_fsb5_fadpcm(&vec![0; MAX_INPUT_BYTES + 1]),
        Err(FadpcmDecodeError::InputTooLarge)
    ));
}

#[test]
fn malformed_unknown_and_duplicate_chunks_are_rejected() {
    let good = bank(1, 1, &block(0, 0, 0, 0, 0), Some(29_015));
    for chunk in [
        (1_u32 << 25) | 8,
        (2 << 25) | 6,
        (2 << 25) | 8 | 1,
        (2 << 25) | (0xff_ffff << 1),
    ] {
        let mut bad = good.clone();
        word(&mut bad, 68, chunk);
        assert!(decode_fsb5_fadpcm(&bad).is_err());
    }
    let mut duplicate = good.clone();
    word(&mut duplicate, 68, (2 << 25) | 8 | 1);
    duplicate.splice(
        76..76,
        [(2_u32 << 25) | 8, 29_015]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    word(&mut duplicate, 12, 24);
    assert!(decode_fsb5_fadpcm(&duplicate).is_err());
}

#[test]
fn name_table_is_bounded_terminated_and_not_used_as_audio_data() {
    let mut named = bank(1, 1, &block(0, 0, 0, 0, 0), None);
    named.splice(68..68, [4, 0, 0, 0, b'a', 0, 0, 0]);
    word(&mut named, 16, 8);
    assert!(decode_fsb5_fadpcm(&named).is_ok());
    for (index, value) in [(68, 3), (68, 8), (74, b'x'), (75, 1)] {
        let mut bad = named.clone();
        bad[index] = value;
        assert!(decode_fsb5_fadpcm(&bad).is_err());
    }
    for length in [1, 257] {
        let mut names = Vec::from(4_u32.to_le_bytes());
        names.resize(4 + length, b'a');
        if length > 1 {
            names.push(0);
        }
        let mut bad = bank(1, 1, &block(0, 0, 0, 0, 0), None);
        word(&mut bad, 16, names.len() as u32);
        bad.splice(68..68, names);
        assert!(decode_fsb5_fadpcm(&bad).is_err());
    }
}
