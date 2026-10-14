//! Mesher tests: fixtures and random sections against the scalar reference, exterior
//! culling, crack border lists, clusters and translucent classes.

mod cache;
mod cracks;
mod culling;
mod layout;
mod oracle;

use std::collections::HashMap;

use crate::{
    classify::TableClassifier,
    fixtures::{blocks, fixture_classifier},
    geometry::{Face, SECTION_SIDE, SectionPos},
    mesh::{
        Bucket, ExteriorCulling, MeshList,
        class::MeshClasses,
        input::{FaceSlabs, MeshInput},
        mesher::Mesher,
        output::SectionMesh,
        quad::Quad,
        reference,
    },
    section::{ChildMask, Section},
    storage::{DenseSection, PackScratch},
    voxel::{FULL_COVERAGE, Light, MaterialId, Voxel},
};

/// Material ids the fixture classifier knows, plus one it does not (classed solid).
const MATERIAL_COUNT: u32 = blocks::SAND + 1;
const UNKNOWN_MATERIAL: u32 = MATERIAL_COUNT + 3;

/// A voxel of fixture block `id` with `light`.
fn block(id: u32, light: Light) -> Voxel {
    Voxel::new(
        MaterialId::new(id).expect("fixture id"),
        light,
        FULL_COVERAGE,
        id == blocks::WATER,
    )
}

/// Voxels of every level as a function of the level and absolute level coordinates.
type Generator<'a> = dyn Fn(u8, [i64; 3]) -> Voxel + 'a;

/// Sections built from a generator, on demand.
struct World<'a> {
    generate: &'a Generator<'a>,
    sections: HashMap<SectionPos, Section>,
    scratch: PackScratch,
}

impl<'a> World<'a> {
    /// An empty world drawing its voxels from `generate`.
    fn new(generate: &'a Generator<'a>) -> Self {
        Self {
            generate,
            sections: HashMap::new(),
            scratch: PackScratch::default(),
        }
    }

    /// Builds `pos` from the generator if it is not built yet.
    fn build(&mut self, pos: SectionPos) {
        if self.sections.contains_key(&pos) {
            return;
        }
        let side = SECTION_SIDE as i64;
        let origin = [pos.x(), pos.y(), pos.z()].map(|value| i64::from(value) * side);
        let mut dense = DenseSection::filled(Voxel::AIR, 0);
        for y in 0..SECTION_SIDE {
            for z in 0..SECTION_SIDE {
                for x in 0..SECTION_SIDE {
                    let at = [
                        origin[0] + x as i64,
                        origin[1] + y as i64,
                        origin[2] + z as i64,
                    ];
                    dense.set(x, y, z, (self.generate)(pos.level(), at));
                }
            }
        }
        let section = Section::from_dense(pos, &dense, ChildMask::UNKNOWN, &mut self.scratch)
            .expect("unknown child masks fit every level");
        self.sections.insert(pos, section);
    }

    /// Builds `pos` and every section its mesh reads for which `known` holds.
    fn build_around(&mut self, pos: SectionPos, known: impl Fn(SectionPos) -> bool) {
        self.build(pos);
        for face in Face::ALL {
            let slabs = FaceSlabs::of(pos, face);
            for neighbour in [slabs.same, slabs.coarser]
                .into_iter()
                .chain(slabs.finer)
                .flatten()
            {
                if known(neighbour) {
                    self.build(neighbour);
                }
            }
        }
    }

    /// The mesh input of built section `pos` with every built section it reads.
    fn input(&self, pos: SectionPos) -> MeshInput<'_> {
        MeshInput::from_lookup(&self.sections[&pos], |at| self.sections.get(&at))
    }
}

/// The fixture blocks' classes.
fn classifier() -> TableClassifier {
    fixture_classifier()
}

/// The fixture blocks' mesh classes, with unknown ids solid.
fn mesh_classes() -> MeshClasses {
    MeshClasses::new(&classifier(), MATERIAL_COUNT)
}

/// Every quad of `mesh` with its list and bucket, sorted.
fn listed(mesh: &SectionMesh) -> Vec<(MeshList, Bucket, Quad)> {
    let mut out = Vec::new();
    for list in MeshList::ALL {
        for bucket in Bucket::ALL {
            for face in Face::ALL {
                for &quad in mesh.group(list, bucket, face) {
                    assert_eq!(quad.face(), face, "quads sit in their face's group");
                    out.push((list, bucket, quad));
                }
            }
        }
    }
    out.sort();
    out
}

/// Meshes `pos` with the mesher and the reference, asserts both give the same quads and
/// returns the mesh.
fn mesh_checked(
    world: &World<'_>,
    pos: SectionPos,
    mesher: &mut Mesher,
    culling: ExteriorCulling,
) -> SectionMesh {
    let input = world.input(pos);
    let mesh = mesher.mesh(&input, &mesh_classes(), culling);
    let classes = reference::Classes {
        classes: &classifier(),
        water: MaterialId::new(blocks::WATER),
    };
    let expected = reference::mesh(&input, classes, culling);
    let actual = listed(&mesh);
    if actual != expected {
        let missing: Vec<_> = expected
            .iter()
            .filter(|q| !actual.contains(q))
            .take(8)
            .collect();
        let extra: Vec<_> = actual
            .iter()
            .filter(|q| !expected.contains(q))
            .take(8)
            .collect();
        panic!(
            "{pos:?} {culling:?}: {} quads, reference {}; missing {missing:?}; extra {extra:?}",
            actual.len(),
            expected.len()
        );
    }
    mesh
}

/// Deterministic xorshift for random fixtures.
struct Rng(u64);

impl Rng {
    /// The next pseudo-random word.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A pseudo-random value in `0..bound`.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

/// A stable hash of a level and absolute position, for generators.
fn hash(level: u8, at: [i64; 3], seed: u64) -> u64 {
    let mut state = seed ^ u64::from(level).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for value in at {
        state = (state ^ value as u64).wrapping_mul(0xff51_afd7_ed55_8ccd);
        state ^= state >> 33;
    }
    state
}

/// The section position, which must be representable.
fn pos(level: u8, x: i32, y: i32, z: i32) -> SectionPos {
    SectionPos::new(level, x, y, z).expect("representable")
}
