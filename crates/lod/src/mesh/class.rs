//! Mesh classes of every material id, resolved once from the caller's classes.

use crate::{
    classify::{BlockClass, MaterialClasses, voxel_class},
    mesh::Bucket,
    voxel::{FULL_COVERAGE, Light, MATERIAL_FIELD, MaterialId, Voxel, WATER_FIELD},
};

/// Empty space: shows every face in front of it.
pub(super) const AIR: u8 = 1 << 0;
/// Hides every face in front of it; its own faces go to the opaque bucket.
pub(super) const SOLID: u8 = 1 << 1;
/// Translucent water.
pub(super) const WATER: u8 = 1 << 2;
/// See-through blocks: drawn opaque, hide only faces of their own material.
pub(super) const SEE_THROUGH: u8 = 1 << 3;
/// Set with [`SEE_THROUGH`] on waterlogged see-through voxels, which hold water too.
pub(super) const WET: u8 = 1 << 4;
/// Every class bit, one per mask the mesher builds.
pub(super) const CLASS_BITS: u32 = 5;

const _: () = assert!(WET as u32 == 1 << (CLASS_BITS - 1));

/// The bucket of a face of a voxel of class `own` and word `own_word` against a front voxel
/// of class `front` and word `front_word`, or `None` when it does not show. Solid faces show
/// against anything but solid; water against air and dry see-through voxels, as it fills
/// waterlogged ones; see-through faces against anything but solid or the same material,
/// except that a waterlogged voxel shows its water's surface against air.
pub(super) fn face_bucket(own: u8, own_word: u32, front: u8, front_word: u32) -> Option<Bucket> {
    let shows = match own {
        SOLID => front != SOLID,
        WATER => front == AIR || front == SEE_THROUGH,
        _ if own & SEE_THROUGH != 0 => {
            if own & WET != 0 && front == AIR {
                return Some(Bucket::Translucent);
            }
            front != SOLID && MATERIAL_FIELD.read(own_word) != MATERIAL_FIELD.read(front_word)
        }
        _ => false,
    };
    shows.then_some(if own == WATER {
        Bucket::Translucent
    } else {
        Bucket::Opaque
    })
}

/// The mesh class of each material id, built once per pack and material registry and
/// shared by every mesh job.
#[derive(Clone)]
pub struct MeshClasses {
    flags: Box<[u8]>,
    /// The lowest water material, which colours the surface of waterlogged voxels.
    water: Option<MaterialId>,
}

impl std::fmt::Debug for MeshClasses {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MeshClasses")
            .field("entries", &self.flags.len())
            .finish()
    }
}

impl MeshClasses {
    /// Classes of the block ids below `material_count`, asked of `classes` once each as
    /// [`voxel_class`] reads them; air is empty, hidden and every id at or past the count
    /// are solid. Waterlogged see-through voxels hold water only when some id is water.
    /// Rebuild when the pack or the registry's length changes.
    #[must_use]
    pub fn new(classes: &impl MaterialClasses, material_count: u32) -> Self {
        let entries = MATERIAL_FIELD.max_value() as usize + 1;
        let mut flags = vec![SOLID; entries].into_boxed_slice();
        flags[MaterialId::AIR.raw() as usize] = AIR;
        let mut water = None;
        let last = material_count.min(MaterialId::LAST_BLOCK.raw() + 1);
        for raw in MaterialId::FIRST_BLOCK.raw()..last {
            let material = MaterialId::new(raw).expect("block ids fit the field");
            let voxel = Voxel::new(material, Light::DARK, FULL_COVERAGE, false);
            flags[raw as usize] = match voxel_class(classes, voxel) {
                BlockClass::Air | BlockClass::Opaque | BlockClass::Leaves => SOLID,
                BlockClass::Water => {
                    water.get_or_insert(material);
                    WATER
                }
                BlockClass::SeeThrough => SEE_THROUGH,
            };
        }
        Self { flags, water }
    }

    /// The class bits of a voxel word: one class, plus [`WET`] on waterlogged see-through
    /// voxels when a water material exists.
    pub(super) fn of_word(&self, word: u32) -> u8 {
        let class = self.flags[MATERIAL_FIELD.read(word) as usize];
        if class == SEE_THROUGH && WATER_FIELD.read(word) != 0 && self.water.is_some() {
            SEE_THROUGH | WET
        } else {
            class
        }
    }

    /// The payload of a face in `bucket` of a voxel of class `class` and word `word`: the
    /// word itself, with the water material on the water surface of a waterlogged voxel.
    pub(super) fn face_word(&self, class: u8, word: u32, bucket: Bucket) -> u32 {
        match self.water {
            Some(water) if class & WET != 0 && bucket == Bucket::Translucent => {
                (word & !MATERIAL_FIELD.mask()) | MATERIAL_FIELD.place(water.raw())
            }
            _ => word,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{blocks, fixture_classifier};

    #[test]
    fn classes_follow_the_distant_terrain_rules() {
        let classes = MeshClasses::new(&fixture_classifier(), blocks::SAND + 1);
        let word = |id: u32| {
            Voxel::new(
                MaterialId::new(id).expect("id"),
                Light::DARK,
                FULL_COVERAGE,
                false,
            )
            .bits()
        };
        assert_eq!(classes.of_word(Voxel::AIR.bits()), AIR);
        assert_eq!(classes.of_word(Voxel::HIDDEN.bits()), SOLID);
        assert_eq!(classes.of_word(word(blocks::STONE)), SOLID);
        assert_eq!(
            classes.of_word(word(blocks::LEAVES)),
            SOLID,
            "leaves are opaque"
        );
        assert_eq!(classes.of_word(word(blocks::WATER)), WATER);
        assert_eq!(classes.of_word(word(blocks::GLASS)), SEE_THROUGH);
        let wet_glass = Voxel::new(
            MaterialId::new(blocks::GLASS).expect("id"),
            Light::DARK,
            FULL_COVERAGE,
            true,
        )
        .bits();
        assert_eq!(classes.of_word(wet_glass), SEE_THROUGH | WET);
        let surface = classes.face_word(SEE_THROUGH | WET, wet_glass, Bucket::Translucent);
        assert_eq!(MATERIAL_FIELD.read(surface), blocks::WATER);
        assert_eq!(
            surface & !MATERIAL_FIELD.mask(),
            wet_glass & !MATERIAL_FIELD.mask()
        );
        assert_eq!(
            classes.face_word(SEE_THROUGH | WET, wet_glass, Bucket::Opaque),
            wet_glass
        );
        assert_eq!(
            classes.of_word(word(blocks::SAND + 1)),
            SOLID,
            "ids past the count are solid"
        );
    }
}
