use std::io::Write as _;

use super::*;
use crate::{
    geometry::{Octant, SectionPos},
    section::ChildState,
    storage::{DenseSection, PackScratch},
    voxel::{FULL_COVERAGE, Light, MaterialId},
};

fn block(material: u32, sky: u8) -> Voxel {
    Voxel::new(
        MaterialId::new(material).expect("material"),
        Light::new(sky, material as u8 % 16),
        FULL_COVERAGE,
        material.is_multiple_of(5),
    )
}

/// A section with `materials` distinct block voxels in a scattered pattern and a few biomes.
fn mixed_section(pos: SectionPos, materials: u32, children: ChildMask) -> Section {
    let mut dense = DenseSection::filled(Voxel::air(Light::FULL_SKY), 3);
    for (index, voxel) in dense.voxels_mut().iter_mut().enumerate() {
        let pick = (index as u32).wrapping_mul(2_654_435_761) % (materials + 1);
        if pick > 0 {
            *voxel = block(pick, (index % 16) as u8);
        }
    }
    for x in 0..32 {
        dense.biomes_mut().set(x, x / 2, 100 + x as u32 % 7);
    }
    packed(pos, &dense, children)
}

fn packed(pos: SectionPos, dense: &DenseSection, children: ChildMask) -> Section {
    Section::from_dense(pos, dense, children, &mut PackScratch::default()).expect("valid mask")
}

fn pos(level: u8, x: i32, y: i32, z: i32) -> SectionPos {
    SectionPos::new(level, x, y, z).expect("pos")
}

fn some_children() -> ChildMask {
    let mut mask = ChildMask::UNKNOWN;
    mask.set(Octant::ALL[0], ChildState::NonEmpty);
    mask.set(Octant::ALL[3], ChildState::Empty);
    mask
}

#[test]
fn sections_round_trip_at_every_palette_width() {
    let mut codec = SectionCodec::new();
    let mut sections = vec![
        packed(
            pos(0, 0, 0, 0),
            &DenseSection::filled(Voxel::AIR, 0),
            ChildMask::UNKNOWN,
        ),
        packed(
            pos(6, -1, -128, 8_388_607),
            &DenseSection::filled(Voxel::HIDDEN, u32::MAX),
            some_children(),
        ),
    ];
    for (index, materials) in [1, 2, 15, 300, 5000].into_iter().enumerate() {
        let level = index as u8 % 4;
        let children = if level == 0 {
            ChildMask::UNKNOWN
        } else {
            some_children()
        };
        sections.push(mixed_section(
            pos(level, -3 - index as i32, 4, 9),
            materials,
            children,
        ));
    }
    let mut full = DenseSection::filled(Voxel::AIR, 0);
    for (index, voxel) in full.voxels_mut().iter_mut().enumerate() {
        *voxel = block(1 + (index as u32 >> 4), (index % 16) as u8);
    }
    for column in 0..MAX_BIOME_PALETTE {
        full.biomes_mut()
            .set(column % 32, column / 32, column as u32);
    }
    sections.push(packed(pos(1, 0, 0, 0), &full, some_children()));
    assert_eq!(
        sections.last().map(|s| s.voxels().palette().len()),
        Some(SECTION_VOXELS)
    );
    assert_eq!(
        sections.last().map(|s| s.biomes().palette().len()),
        Some(MAX_BIOME_PALETTE)
    );

    let mut record = Vec::new();
    for section in &sections {
        codec.encode_into(section, &mut record);
        let header = RecordHeader::read(&record).expect("valid header");
        assert_eq!(header.key, section.pos().key());
        assert_eq!(header.children, section.children());
        assert_eq!(record.len(), HEADER_LEN + header.body_len as usize);
        assert!(header.body_len as usize <= MAX_COMPRESSED_LEN);
        let decoded = codec.decode(&record).expect("valid record");
        assert_eq!(&decoded, section);
        assert_eq!(decoded.biomes().palette(), section.biomes().palette());
        assert_eq!(
            codec.decode_expecting(&record, section.pos().key()),
            Ok(decoded)
        );
    }
}

#[test]
fn encoding_is_deterministic_and_compresses() {
    let section = mixed_section(pos(2, 1, 1, 1), 15, some_children());
    let first = SectionCodec::new().encode(&section);
    let mut codec = SectionCodec::new();
    let _ = codec.encode(&mixed_section(pos(0, 0, 0, 0), 300, ChildMask::UNKNOWN));
    assert_eq!(
        codec.encode(&section),
        first,
        "reused state changes nothing"
    );
    let raw = body_len(section.voxels().palette().len(), 7);
    assert!(first.len() < raw, "{} >= {raw}", first.len());
}

#[test]
fn every_flipped_bit_or_truncation_is_rejected() {
    let section = mixed_section(pos(3, -7, 2, 5), 15, some_children());
    let mut codec = SectionCodec::new();
    let record = codec.encode(&section);
    for byte in 0..record.len() {
        for bit in [0, 3, 7] {
            let mut corrupt = record.clone();
            corrupt[byte] ^= 1 << bit;
            assert!(
                codec.decode(&corrupt).is_err(),
                "flip of bit {bit} in byte {byte} was accepted"
            );
        }
    }
    for len in 0..record.len() {
        assert!(codec.decode(&record[..len]).is_err(), "truncated to {len}");
    }
    assert_eq!(
        codec.decode(&record[..HEADER_LEN - 1]),
        Err(CodecError::Truncated(25))
    );
    let mut longer = record.clone();
    longer.push(0);
    assert_eq!(
        codec.decode(&longer),
        Err(CodecError::BodyLength {
            declared: (record.len() - HEADER_LEN) as u32,
            actual: record.len() - HEADER_LEN + 1
        })
    );
    assert_eq!(codec.decode(&record), Ok(section), "the codec recovers");
}

#[test]
fn records_for_another_key_are_rejected() {
    let section = mixed_section(pos(1, 2, 3, 4), 2, some_children());
    let mut codec = SectionCodec::new();
    let record = codec.encode(&section);
    let other = pos(1, 2, 3, 5).key();
    assert_eq!(
        codec.decode_expecting(&record, other),
        Err(CodecError::KeyMismatch {
            expected: other,
            found: section.pos().key()
        })
    );
}

/// Header fields of a forged record.
struct Forged {
    key: u64,
    palette_len: u16,
    children: u16,
}

impl Forged {
    fn new(level: u8, palette_len: u16) -> Self {
        Self {
            key: pos(level, 0, 0, 0).key().bits(),
            palette_len,
            children: 0,
        }
    }

    /// Signs a record whose body is the given compressed bytes.
    fn with_compressed(&self, compressed: &[u8]) -> Vec<u8> {
        let mut record = SECTION_MAGIC.to_vec();
        record.extend_from_slice(&SECTION_FORMAT_VERSION.to_le_bytes());
        record.extend_from_slice(&self.key.to_be_bytes());
        record.extend_from_slice(&self.palette_len.to_le_bytes());
        record.extend_from_slice(&self.children.to_le_bytes());
        record.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        let crc = record_crc(&record, compressed);
        record.extend_from_slice(&crc.to_le_bytes());
        record.extend_from_slice(compressed);
        record
    }

    /// Signs a record whose body deflates `body`.
    fn with_body(&self, body: &[u8]) -> Vec<u8> {
        self.with_compressed(&deflate(body))
    }
}

fn deflate(body: &[u8]) -> Vec<u8> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), Compression::new(DEFLATE_LEVEL));
    encoder.write_all(body).expect("in-memory write");
    encoder.finish().expect("in-memory deflate")
}

/// A uniform body: one palette word, no index words, one biome.
fn uniform_body(voxel_bits: u32) -> Vec<u8> {
    let mut body = voxel_bits.to_le_bytes().to_vec();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&[0; SECTION_COLUMNS]);
    body
}

#[test]
fn forged_records_reach_each_typed_rejection() {
    let mut codec = SectionCodec::new();
    let valid = Forged::new(0, 1).with_body(&uniform_body(Voxel::HIDDEN.bits()));
    let decoded = codec.decode(&valid).expect("forged record is valid");
    assert_eq!(decoded.voxels().uniform_voxel(), Some(Voxel::HIDDEN));
    assert_eq!(decoded.biomes().get(31, 31), 9);

    let non_canonical = Voxel::HIDDEN.bits() | 1 << 20;
    assert_eq!(
        codec.decode(&Forged::new(0, 1).with_body(&uniform_body(non_canonical))),
        Err(CodecError::Voxel {
            index: 0,
            bits: non_canonical
        })
    );
    assert_eq!(
        codec.decode(&Forged::new(0, 0).with_body(&uniform_body(0))),
        Err(CodecError::PaletteLength(0))
    );
    let mut level0_children = Forged::new(0, 1);
    level0_children.children = some_children().to_bits();
    assert_eq!(
        codec.decode(&level0_children.with_body(&uniform_body(0))),
        Err(CodecError::LevelZeroChildren(some_children().to_bits()))
    );
    let mut bad_mask = Forged::new(2, 1);
    bad_mask.children = 0x0100;
    assert_eq!(
        codec.decode(&bad_mask.with_body(&uniform_body(0))),
        Err(CodecError::Storage(StorageError::ChildMask(0x0100)))
    );
    let mut bad_key = Forged::new(0, 1);
    bad_key.key = 0xf000_0000_0000_0000;
    assert_eq!(
        codec.decode(&bad_key.with_body(&uniform_body(0))),
        Err(CodecError::Key(KeyError::Level(15)))
    );

    let mut long = uniform_body(0);
    long.push(0);
    assert!(matches!(
        codec.decode(&Forged::new(0, 1).with_body(&long)),
        Err(CodecError::BodySize { .. })
    ));
    let short = &uniform_body(0)[..100];
    assert!(matches!(
        codec.decode(&Forged::new(0, 1).with_body(short)),
        Err(CodecError::BodySize { .. })
    ));
    let mut zero_biomes = uniform_body(0);
    zero_biomes[4..6].copy_from_slice(&0_u16.to_le_bytes());
    assert!(matches!(
        codec.decode(&Forged::new(0, 1).with_body(&zero_biomes)),
        Err(CodecError::BodySize { .. })
    ));
    let mut bad_cell = uniform_body(0);
    *bad_cell.last_mut().expect("cells") = 1;
    assert_eq!(
        codec.decode(&Forged::new(0, 1).with_body(&bad_cell)),
        Err(CodecError::Storage(StorageError::BiomeIndex {
            cell: SECTION_COLUMNS - 1,
            index: 1,
            palette_len: 1
        }))
    );
}

#[test]
fn packed_parts_are_validated_after_inflating() {
    let mut codec = SectionCodec::new();
    let air = Voxel::AIR.bits().to_le_bytes();
    let mut duplicate = [air, air].concat();
    duplicate.extend(std::iter::repeat_n(0, packed_word_count(1) * 8));
    duplicate.extend_from_slice(&uniform_body(0)[4..]);
    assert_eq!(
        codec.decode(&Forged::new(0, 2).with_body(&duplicate)),
        Err(CodecError::Storage(StorageError::DuplicatePalette(0)))
    );
}

#[test]
fn malformed_streams_and_inflation_bombs_are_rejected() {
    let mut codec = SectionCodec::new();
    let body = deflate(&uniform_body(0));
    let mut trailing = body.clone();
    trailing.extend_from_slice(&[1, 2, 3]);
    assert_eq!(
        codec.decode(&Forged::new(0, 1).with_compressed(&trailing)),
        Err(CodecError::Deflate)
    );
    assert_eq!(
        codec.decode(&Forged::new(0, 1).with_compressed(&body[..body.len() - 2])),
        Err(CodecError::Deflate)
    );
    assert_eq!(
        codec.decode(&Forged::new(0, 1).with_compressed(&[0xff; 32])),
        Err(CodecError::Deflate)
    );
    let bomb = deflate(&vec![0; 64 << 20]);
    assert!(bomb.len() < MAX_COMPRESSED_LEN);
    let limit = body_len(1, MAX_BIOME_PALETTE);
    let result = codec.decode(&Forged::new(0, 1).with_compressed(&bomb));
    assert!(
        matches!(result, Err(CodecError::BodySize { expected, actual }) if expected == limit && actual > limit),
        "{result:?}"
    );
    assert!(
        codec.body.capacity() < 2 * MAX_BODY_LEN,
        "decode stays bounded"
    );
    let oversized = vec![0; MAX_COMPRESSED_LEN + 1];
    assert!(matches!(
        codec.decode(&Forged::new(0, 1).with_compressed(&oversized)),
        Err(CodecError::BodyLength { .. })
    ));
}
