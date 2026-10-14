//! Small distant terrain meshes for the renderer's tests, meshed by the real mesher from
//! fixture blocks.

use std::sync::Arc;

use lod::{
    fixtures::{blocks, fixture_classifier},
    geometry::{SECTION_SIDE, SectionPos},
    mesh::{
        ExteriorCulling, class::MeshClasses, input::MeshInput, mesher::Mesher, output::SectionMesh,
    },
    section::{ChildMask, Section},
    storage::{DenseSection, PackScratch},
    voxel::{FULL_COVERAGE, Light, MaterialId, Voxel},
};

/// A voxel of fixture block `id`, lit by full sky.
pub(crate) fn block(id: u32) -> Voxel {
    Voxel::new(
        MaterialId::new(id).expect("fixture id"),
        Light::FULL_SKY,
        FULL_COVERAGE,
        id == blocks::WATER,
    )
}

/// The section at `pos` whose voxel `(x, y, z)` is `voxel(x, y, z)` and whose voxel column
/// `(x, z)` lies in raw biome `biome(x, z)`.
pub(crate) fn section_of(
    pos: SectionPos,
    voxel: impl Fn(usize, usize, usize) -> Option<u32>,
    biome: impl Fn(usize, usize) -> u32,
) -> Section {
    let mut dense = DenseSection::filled(Voxel::air(Light::FULL_SKY), 1);
    for z in 0..SECTION_SIDE {
        for x in 0..SECTION_SIDE {
            assert!(dense.biomes_mut().set(x, z, biome(x, z)));
            for y in 0..SECTION_SIDE {
                if let Some(id) = voxel(x, y, z) {
                    dense.set(x, y, z, block(id));
                }
            }
        }
    }
    Section::from_dense(pos, &dense, ChildMask::UNKNOWN, &mut PackScratch::default())
        .expect("a valid section")
}

/// The mesh of `section` beside the known sections `around`; other neighbours are unknown.
pub(crate) fn mesh_beside(section: &Section, around: &[Section]) -> Arc<SectionMesh> {
    let classes = MeshClasses::new(&fixture_classifier(), blocks::SAND + 1);
    let input = MeshInput::from_lookup(section, |pos| {
        around.iter().find(|neighbour| neighbour.pos() == pos)
    });
    Arc::new(Mesher::new().mesh(&input, &classes, ExteriorCulling::SectionLocal))
}

/// The mesh of the section at `pos` whose voxel `(x, y, z)` is `voxel(x, y, z)`, with unknown
/// neighbours, in biome 1.
pub(crate) fn mesh(
    pos: SectionPos,
    voxel: impl Fn(usize, usize, usize) -> Option<u32>,
) -> Arc<SectionMesh> {
    mesh_beside(&section_of(pos, voxel, |_, _| 1), &[])
}

/// Ground of stone below `height` with water above it up to `water` (exclusive).
pub(crate) fn ground(pos: SectionPos, height: usize, water: usize) -> Arc<SectionMesh> {
    mesh(pos, |_, y, _| {
        if y < height {
            Some(blocks::STONE)
        } else if y < water {
            Some(blocks::WATER)
        } else {
            None
        }
    })
}
