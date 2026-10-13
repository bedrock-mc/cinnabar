//! The 32-bit voxel word: the single source of its bit layout.
//!
//! `material 18 | block light 4 | sky light 4 | coverage 3 | water 1 | spare 2`, from the
//! least significant bit. Shader constants are generated from [`VOXEL_FIELDS`].

/// One named bit field of the voxel word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelField {
    pub name: &'static str,
    pub shift: u32,
    pub width: u32,
}

impl VoxelField {
    /// The field's bits in place.
    #[must_use]
    pub const fn mask(self) -> u32 {
        (((1_u64 << self.width) - 1) << self.shift) as u32
    }

    /// Largest value the field holds.
    #[must_use]
    pub const fn max_value(self) -> u32 {
        ((1_u64 << self.width) - 1) as u32
    }

    /// This field's value in `word`.
    const fn read(self, word: u32) -> u32 {
        (word & self.mask()) >> self.shift
    }

    /// `value` shifted into this field; bits past the width are dropped.
    const fn place(self, value: u32) -> u32 {
        (value << self.shift) & self.mask()
    }

    /// The field of `width` bits directly above this one.
    const fn after(self, name: &'static str, width: u32) -> Self {
        Self {
            name,
            shift: self.shift + self.width,
            width,
        }
    }
}

/// Store-local material id.
pub const MATERIAL_FIELD: VoxelField = VoxelField {
    name: "material",
    shift: 0,
    width: 18,
};
/// Block light, 0..=15.
pub const BLOCK_LIGHT_FIELD: VoxelField = MATERIAL_FIELD.after("block_light", 4);
/// Sky light, 0..=15.
pub const SKY_LIGHT_FIELD: VoxelField = BLOCK_LIGHT_FIELD.after("sky_light", 4);
/// Quantised fraction of the voxel's volume that is non-air, for shading only.
pub const COVERAGE_FIELD: VoxelField = SKY_LIGHT_FIELD.after("coverage", 3);
/// Set when the voxel holds or is covered by water.
pub const WATER_FIELD: VoxelField = COVERAGE_FIELD.after("water", 1);
/// Reserved; must be zero.
pub const SPARE_FIELD: VoxelField = WATER_FIELD.after("spare", 2);

/// Every field in bit order; together they cover the word exactly once.
pub const VOXEL_FIELDS: [VoxelField; 6] = [
    MATERIAL_FIELD,
    BLOCK_LIGHT_FIELD,
    SKY_LIGHT_FIELD,
    COVERAGE_FIELD,
    WATER_FIELD,
    SPARE_FIELD,
];

const _: () = {
    let mut covered = 0_u32;
    let mut index = 0;
    while index < VOXEL_FIELDS.len() {
        let mask = VOXEL_FIELDS[index].mask();
        assert!(covered & mask == 0, "voxel fields overlap");
        covered |= mask;
        index += 1;
    }
    assert!(
        covered == u32::MAX,
        "voxel fields must cover the whole word"
    );
};

/// Largest light nibble.
pub const MAX_LIGHT: u8 = 15;
/// Coverage of a voxel whose whole volume is non-air.
pub const FULL_COVERAGE: u8 = COVERAGE_FIELD.max_value() as u8;

/// A store-local material id. `0` is air and the largest id is [`MaterialId::HIDDEN`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialId(u32);

impl MaterialId {
    /// Air; never assigned to a block key.
    pub const AIR: Self = Self(0);
    /// Opaque voxel enclosed by opaque voxels; carries no appearance.
    pub const HIDDEN: Self = Self(MATERIAL_FIELD.max_value());
    /// First id a registry may assign to a block key.
    pub const FIRST_BLOCK: Self = Self(1);
    /// Last id a registry may assign to a block key.
    pub const LAST_BLOCK: Self = Self(MATERIAL_FIELD.max_value() - 1);

    /// Accepts any id that fits the material field.
    #[must_use]
    pub const fn new(raw: u32) -> Option<Self> {
        if raw <= MATERIAL_FIELD.max_value() {
            Some(Self(raw))
        } else {
            None
        }
    }

    /// The id as stored in the material field.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// True for ids a registry assigns to blocks, excluding air and hidden.
    #[must_use]
    pub const fn is_block(self) -> bool {
        self.0 >= Self::FIRST_BLOCK.0 && self.0 <= Self::LAST_BLOCK.0
    }
}

/// Sky and block light nibbles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Light {
    sky: u8,
    block: u8,
}

impl Light {
    /// No light.
    pub const DARK: Self = Self { sky: 0, block: 0 };
    /// Full sky light, no block light.
    pub const FULL_SKY: Self = Self {
        sky: MAX_LIGHT,
        block: 0,
    };

    /// Builds a light value; nibbles above [`MAX_LIGHT`] saturate.
    #[must_use]
    pub const fn new(sky: u8, block: u8) -> Self {
        Self {
            sky: if sky > MAX_LIGHT { MAX_LIGHT } else { sky },
            block: if block > MAX_LIGHT { MAX_LIGHT } else { block },
        }
    }

    /// Sky light, `0..=15`.
    #[must_use]
    pub const fn sky(self) -> u8 {
        self.sky
    }

    /// Block light, `0..=15`.
    #[must_use]
    pub const fn block(self) -> u8 {
        self.block
    }
}

/// One packed voxel word. Every value built here is canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Voxel(u32);

impl Voxel {
    /// Unlit air with no flags.
    pub const AIR: Self = Self(0);
    /// The one canonical word for an enclosed opaque voxel: no light, full coverage.
    pub const HIDDEN: Self = Self(
        MATERIAL_FIELD.place(MaterialId::HIDDEN.0) | COVERAGE_FIELD.place(FULL_COVERAGE as u32),
    );

    /// Builds a voxel. Air drops coverage and water; hidden yields [`Voxel::HIDDEN`].
    #[must_use]
    pub const fn new(material: MaterialId, light: Light, coverage: u8, water: bool) -> Self {
        if material.0 == MaterialId::HIDDEN.0 {
            return Self::HIDDEN;
        }
        if material.0 == MaterialId::AIR.0 {
            return Self::air(light);
        }
        let coverage = if coverage > FULL_COVERAGE {
            FULL_COVERAGE
        } else {
            coverage
        };
        Self(
            MATERIAL_FIELD.place(material.0)
                | BLOCK_LIGHT_FIELD.place(light.block as u32)
                | SKY_LIGHT_FIELD.place(light.sky as u32)
                | COVERAGE_FIELD.place(coverage as u32)
                | WATER_FIELD.place(water as u32),
        )
    }

    /// Air carrying `light`.
    #[must_use]
    pub const fn air(light: Light) -> Self {
        Self(BLOCK_LIGHT_FIELD.place(light.block as u32) | SKY_LIGHT_FIELD.place(light.sky as u32))
    }

    /// Accepts a stored word only when it is canonical.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Option<Self> {
        let candidate = Self(bits);
        let material = candidate.material();
        let canonical = Self::new(
            material,
            candidate.light(),
            candidate.coverage(),
            candidate.has_water(),
        );
        if SPARE_FIELD.read(bits) == 0 && canonical.0 == bits {
            Some(candidate)
        } else {
            None
        }
    }

    /// The raw 32-bit word.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// The material field.
    #[must_use]
    pub const fn material(self) -> MaterialId {
        MaterialId(MATERIAL_FIELD.read(self.0))
    }

    /// The sky and block light fields.
    #[must_use]
    pub const fn light(self) -> Light {
        Light {
            sky: SKY_LIGHT_FIELD.read(self.0) as u8,
            block: BLOCK_LIGHT_FIELD.read(self.0) as u8,
        }
    }

    /// Coverage in `0..=FULL_COVERAGE`.
    #[must_use]
    pub const fn coverage(self) -> u8 {
        COVERAGE_FIELD.read(self.0) as u8
    }

    /// True when the voxel is water or waterlogged.
    #[must_use]
    pub const fn has_water(self) -> bool {
        WATER_FIELD.read(self.0) != 0
    }

    /// True when the material is air, whatever the light.
    #[must_use]
    pub const fn is_air(self) -> bool {
        self.material().0 == MaterialId::AIR.0
    }

    /// True for the canonical hidden word.
    #[must_use]
    pub const fn is_hidden(self) -> bool {
        self.0 == Self::HIDDEN.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_round_trip_at_their_extremes() {
        let material = MaterialId::LAST_BLOCK;
        let voxel = Voxel::new(material, Light::new(15, 15), FULL_COVERAGE, true);
        assert_eq!(voxel.material(), material);
        assert_eq!(voxel.light(), Light::new(15, 15));
        assert_eq!(voxel.coverage(), FULL_COVERAGE);
        assert!(voxel.has_water());
        assert_eq!(Voxel::from_bits(voxel.bits()), Some(voxel));
        let low = Voxel::new(MaterialId::FIRST_BLOCK, Light::new(0, 9), 0, false);
        assert_eq!(low.light(), Light::new(0, 9));
        assert_eq!(low.coverage(), 0);
        assert!(!low.has_water());
    }

    #[test]
    fn layout_matches_the_documented_widths() {
        let widths: Vec<_> = VOXEL_FIELDS
            .iter()
            .map(|field| (field.name, field.width))
            .collect();
        assert_eq!(
            widths,
            [
                ("material", 18),
                ("block_light", 4),
                ("sky_light", 4),
                ("coverage", 3),
                ("water", 1),
                ("spare", 2),
            ]
        );
    }

    #[test]
    fn air_and_hidden_are_canonical() {
        assert_eq!(Voxel::AIR.bits(), 0);
        assert!(Voxel::AIR.is_air());
        assert!(Voxel::HIDDEN.is_hidden());
        assert_eq!(Voxel::HIDDEN.light(), Light::DARK);
        assert_eq!(
            Voxel::new(MaterialId::HIDDEN, Light::FULL_SKY, 1, true),
            Voxel::HIDDEN
        );
        let air = Voxel::new(MaterialId::AIR, Light::new(12, 3), 5, true);
        assert_eq!(air, Voxel::air(Light::new(12, 3)));
        assert_eq!(air.coverage(), 0);
        assert!(!air.has_water());
    }

    #[test]
    fn non_canonical_words_are_rejected() {
        assert_eq!(Voxel::from_bits(SPARE_FIELD.mask()), None);
        assert_eq!(
            Voxel::from_bits(Voxel::HIDDEN.bits() | SKY_LIGHT_FIELD.mask()),
            None
        );
        assert_eq!(
            Voxel::from_bits(WATER_FIELD.mask()),
            None,
            "air never carries water"
        );
        assert_eq!(Voxel::from_bits(Voxel::HIDDEN.bits()), Some(Voxel::HIDDEN));
    }

    #[test]
    fn light_and_material_ids_saturate_or_reject() {
        assert_eq!(Light::new(200, 16), Light::new(15, 15));
        assert_eq!(MaterialId::new(MATERIAL_FIELD.max_value() + 1), None);
        assert!(!MaterialId::AIR.is_block());
        assert!(!MaterialId::HIDDEN.is_block());
        assert!(MaterialId::LAST_BLOCK.is_block());
    }
}
