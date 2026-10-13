//! Section records and file corruption helpers shared by this crate's tests.

use std::{fs::OpenOptions, path::Path};

use lod::{
    codec::SectionCodec,
    geometry::{SectionKey, SectionPos},
    section::{ChildMask, Section},
    storage::{DenseSection, PackScratch},
    voxel::{FULL_COVERAGE, Light, MaterialId, Voxel},
};

/// The key of the section at `(level, x, y, z)`.
pub(crate) fn key(level: u8, x: i32, y: i32, z: i32) -> SectionKey {
    SectionPos::new(level, x, y, z)
        .expect("test position")
        .key()
}

/// A valid record for `key` whose contents differ with `seed`: up to `materials` block
/// materials scattered through air.
pub(crate) fn record(key: SectionKey, seed: u32, materials: u32) -> Vec<u8> {
    let mut dense = DenseSection::filled(Voxel::air(Light::FULL_SKY), seed % 7);
    for (index, voxel) in dense.voxels_mut().iter_mut().enumerate() {
        let pick = (index as u32 ^ seed).wrapping_mul(2_654_435_761) >> 16;
        let material = pick % (materials + 1);
        if material > 0 {
            *voxel = Voxel::new(
                MaterialId::new(material).expect("block material"),
                Light::new((pick % 16) as u8, 0),
                FULL_COVERAGE,
                false,
            );
        }
    }
    let section = Section::from_dense(
        key.pos(),
        &dense,
        ChildMask::UNKNOWN,
        &mut PackScratch::default(),
    )
    .expect("unknown children are valid at every level");
    SectionCodec::new().encode(&section)
}

/// XORs the byte at `offset` of the file at `path` with `0x5a`.
pub(crate) fn flip_byte(path: &Path, offset: u64) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("open for corruption");
    let mut byte = [0];
    crate::positional::read_exact_at(&file, &mut byte, offset).expect("read byte");
    byte[0] ^= 0x5a;
    crate::positional::write_all_at(&file, &byte, offset).expect("write byte");
}

/// Overwrites `len` bytes at `offset` of the file at `path` with zeros.
pub(crate) fn zero_bytes(path: &Path, offset: u64, len: usize) {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for corruption");
    crate::positional::write_all_at(&file, &vec![0; len], offset).expect("zero bytes");
}

/// Truncates the file at `path` to `len` bytes.
pub(crate) fn truncate(path: &Path, len: u64) {
    OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for truncation")
        .set_len(len)
        .expect("truncate");
}

/// A varied chunk surface of world `seed`: heights and materials from a hash of each
/// block column, so different seeds disagree on almost every column.
pub(crate) fn surface(seed: u32, chunk_x: i32, chunk_z: i32) -> lod::epoch::ChunkSurface {
    let mut surface = lod::epoch::ChunkSurface::default();
    for z in 0..16 {
        for x in 0..16 {
            let (bx, bz) = (chunk_x * 16 + x as i32, chunk_z * 16 + z as i32);
            let hash = (seed ^ (bx as u32).wrapping_mul(0x9e37_79b9)).wrapping_mul(0x85eb_ca6b)
                ^ (bz as u32).wrapping_mul(0xc2b2_ae35);
            let hash = hash ^ hash >> 15;
            let column = lod::epoch::SurfaceColumn {
                top_y: 60 + (hash % 16) as i32,
                material: MaterialId::new(1 + hash / 16 % 8).expect("block material"),
            };
            surface.set(x, z, Some(column.signature()));
        }
    }
    surface
}

/// A flat chunk surface at `top_y`, as on lobby and void maps.
pub(crate) fn flat_surface(top_y: i32) -> lod::epoch::ChunkSurface {
    let mut surface = lod::epoch::ChunkSurface::default();
    let column = lod::epoch::SurfaceColumn {
        top_y,
        material: MaterialId::new(1).expect("block material"),
    };
    for z in 0..16 {
        for x in 0..16 {
            surface.set(x, z, Some(column.signature()));
        }
    }
    surface
}
