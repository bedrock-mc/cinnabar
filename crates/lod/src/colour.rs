//! Flat colours of distant terrain faces: one per material and face, and biome tints per
//! tint class. Callers fill them from their runtime assets; the face colour word is laid
//! out once here, like the voxel word, so shader constants can be generated from it.
//!
//! A face's shaded colour is `colour × mix(1, tint, weight)`, in linear light
//! ([`FaceColour::shade`]): `colour` is the linear average of the face's texture, `tint`
//! the biome colour of the face's [`TintClass`] in the voxel column's biome, and `weight`
//! the share of the texture the tint covers (1 for tinted faces, the overlay's mean weight
//! for overlay-masked faces such as grass sides).

use crate::{
    bit_field::{BitField, covers_word},
    geometry::Face,
    voxel::{MATERIAL_FIELD, MaterialId},
};

/// Which biome colour tints a face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TintClass {
    /// Not tinted.
    None,
    Grass,
    Foliage,
    Birch,
    Evergreen,
    DryFoliage,
    Water,
}

impl TintClass {
    /// Every class in index order.
    pub const ALL: [Self; 7] = [
        Self::None,
        Self::Grass,
        Self::Foliage,
        Self::Birch,
        Self::Evergreen,
        Self::DryFoliage,
        Self::Water,
    ];
    /// Number of classes.
    pub const COUNT: usize = Self::ALL.len();

    /// Index in `0..COUNT`.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The class at `index`; `None` past the last.
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Self> {
        if index < Self::COUNT {
            Some(Self::ALL[index])
        } else {
            None
        }
    }
}

/// Red of the sRGB-encoded average colour.
pub const COLOUR_RED_FIELD: BitField = BitField::first("red", 8);
/// Green of the sRGB-encoded average colour.
pub const COLOUR_GREEN_FIELD: BitField = COLOUR_RED_FIELD.after("green", 8);
/// Blue of the sRGB-encoded average colour.
pub const COLOUR_BLUE_FIELD: BitField = COLOUR_GREEN_FIELD.after("blue", 8);
/// [`TintClass::index`].
pub const COLOUR_TINT_FIELD: BitField = COLOUR_BLUE_FIELD.after("tint", 3);
/// Share of the face the tint covers, in steps of `1 / max_value`.
pub const COLOUR_TINT_WEIGHT_FIELD: BitField = COLOUR_TINT_FIELD.after("tint_weight", 5);

/// Every field of the face colour word in bit order; together they cover the word once.
pub const FACE_COLOUR_FIELDS: [BitField; 5] = [
    COLOUR_RED_FIELD,
    COLOUR_GREEN_FIELD,
    COLOUR_BLUE_FIELD,
    COLOUR_TINT_FIELD,
    COLOUR_TINT_WEIGHT_FIELD,
];

const _: () = assert!(
    covers_word(&FACE_COLOUR_FIELDS),
    "face colour fields must cover the word exactly once"
);
const _: () = assert!(TintClass::COUNT <= COLOUR_TINT_FIELD.max_value() as usize + 1);

/// One face's flat colour word: see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct FaceColour(u32);

impl FaceColour {
    /// The colour of a face whose texture is unknown: a neutral grey.
    pub const UNKNOWN: Self = Self::from_srgb([128, 128, 128], TintClass::None, 0);
    /// The colour of exposed hidden voxels, which carry no material: the grey of stone, as
    /// most of them are underground rock.
    pub const HIDDEN: Self = Self::from_srgb([125, 125, 125], TintClass::None, 0);

    /// A colour from sRGB bytes, `tint` and a tint weight in `0..=max` of its field.
    #[must_use]
    pub const fn from_srgb(rgb: [u8; 3], tint: TintClass, weight: u32) -> Self {
        Self(
            COLOUR_RED_FIELD.place(rgb[0] as u32)
                | COLOUR_GREEN_FIELD.place(rgb[1] as u32)
                | COLOUR_BLUE_FIELD.place(rgb[2] as u32)
                | COLOUR_TINT_FIELD.place(tint as u32)
                | COLOUR_TINT_WEIGHT_FIELD.place(weight),
        )
    }

    /// A colour from a linear average, sRGB-encoded so dark colours keep their precision,
    /// with `tint` covering `weight` (clamped to `0..=1`) of the face. An untinted face
    /// keeps weight 0.
    #[must_use]
    pub fn from_linear(linear: [f32; 3], tint: TintClass, weight: f32) -> Self {
        let max = COLOUR_TINT_WEIGHT_FIELD.max_value();
        let weight = if tint == TintClass::None {
            0
        } else {
            (weight.clamp(0.0, 1.0) * max as f32).round() as u32
        };
        Self::from_srgb(linear.map(encode_srgb), tint, weight)
    }

    /// The raw word.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// The sRGB-encoded average colour.
    #[must_use]
    pub const fn srgb(self) -> [u8; 3] {
        [
            COLOUR_RED_FIELD.read(self.0) as u8,
            COLOUR_GREEN_FIELD.read(self.0) as u8,
            COLOUR_BLUE_FIELD.read(self.0) as u8,
        ]
    }

    /// The average colour in linear light.
    #[must_use]
    pub fn linear(self) -> [f32; 3] {
        self.srgb().map(decode_srgb)
    }

    /// The tint class.
    #[must_use]
    pub fn tint(self) -> TintClass {
        TintClass::from_index(COLOUR_TINT_FIELD.read(self.0) as usize).unwrap_or(TintClass::None)
    }

    /// The share of the face the tint covers, `0..=1`.
    #[must_use]
    pub fn tint_weight(self) -> f32 {
        COLOUR_TINT_WEIGHT_FIELD.read(self.0) as f32 / COLOUR_TINT_WEIGHT_FIELD.max_value() as f32
    }

    /// The face's colour under the linear biome `tint` of its class: what shaders compute.
    #[must_use]
    pub fn shade(self, tint: [f32; 3]) -> [f32; 3] {
        let weight = self.tint_weight();
        let colour = self.linear();
        std::array::from_fn(|channel| colour[channel] * (1.0 + (tint[channel] - 1.0) * weight))
    }
}

/// Face colours of every material id below [`Self::len`], by [`Face::index`]: a dense
/// table shaders index with the quad payload's material field. Ids at or past the length
/// read [`FaceColour::UNKNOWN`]; the hidden id reads [`FaceColour::HIDDEN`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialColours {
    faces: Vec<[FaceColour; 6]>,
}

impl Default for MaterialColours {
    fn default() -> Self {
        Self::new()
    }
}

impl MaterialColours {
    /// A table holding only air, which is never drawn.
    #[must_use]
    pub fn new() -> Self {
        Self {
            faces: vec![[FaceColour::UNKNOWN; 6]],
        }
    }

    /// Entries, air included: the next material id [`Self::push`] fills.
    #[must_use]
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// True when only air is listed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.len() <= MaterialId::FIRST_BLOCK.raw() as usize
    }

    /// Appends the faces of the next material id; false, adding nothing, past the last
    /// block id the material field holds.
    pub fn push(&mut self, faces: [FaceColour; 6]) -> bool {
        if self.faces.len() > MaterialId::LAST_BLOCK.raw() as usize {
            return false;
        }
        self.faces.push(faces);
        true
    }

    /// The colour of `material`'s `face`.
    #[must_use]
    pub fn get(&self, material: MaterialId, face: Face) -> FaceColour {
        if material == MaterialId::HIDDEN {
            return FaceColour::HIDDEN;
        }
        self.faces
            .get(material.raw() as usize)
            .map_or(FaceColour::UNKNOWN, |faces| faces[face.index()])
    }

    /// Every entry by material id, for upload.
    #[must_use]
    pub fn entries(&self) -> &[[FaceColour; 6]] {
        &self.faces
    }
}

/// Bits of the material field the table is indexed by, shared with shaders.
pub const MATERIAL_INDEX_BITS: u32 = MATERIAL_FIELD.width;

/// Linear biome tints by tint class for every biome a session knows, and raw biome ids to
/// their records; unknown ids read the first record, the fallback biome.
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeTints {
    by_raw_id: Box<[u32]>,
    records: Box<[[[f32; 3]; TintClass::COUNT]]>,
}

impl BiomeTints {
    /// Tints from `records`, at least one, with `by_raw_id` naming each raw biome id's
    /// record; [`TintClass::None`] entries are forced to white.
    ///
    /// # Panics
    ///
    /// When `records` is empty or `by_raw_id` names a record past its end.
    #[must_use]
    pub fn new(by_raw_id: Box<[u32]>, mut records: Box<[[[f32; 3]; TintClass::COUNT]]>) -> Self {
        assert!(!records.is_empty(), "a fallback biome record");
        assert!(
            by_raw_id
                .iter()
                .all(|&record| (record as usize) < records.len()),
            "raw ids name existing records"
        );
        for record in records.iter_mut() {
            record[TintClass::None.index()] = [1.0; 3];
        }
        Self { by_raw_id, records }
    }

    /// The record index of raw biome `raw`.
    #[must_use]
    pub fn record_of(&self, raw: u32) -> u32 {
        self.by_raw_id.get(raw as usize).copied().unwrap_or(0)
    }

    /// The linear tint of `class` in raw biome `raw`.
    #[must_use]
    pub fn tint(&self, raw: u32, class: TintClass) -> [f32; 3] {
        self.records[self.record_of(raw) as usize][class.index()]
    }

    /// Record indices by raw biome id, for upload.
    #[must_use]
    pub fn by_raw_id(&self) -> &[u32] {
        &self.by_raw_id
    }

    /// Records by index, for upload.
    #[must_use]
    pub fn records(&self) -> &[[[f32; 3]; TintClass::COUNT]] {
        &self.records
    }
}

/// The sRGB byte of a linear channel.
fn encode_srgb(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let srgb = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
}

/// The linear channel of an sRGB byte.
#[must_use]
pub fn decode_srgb(byte: u8) -> f32 {
    let srgb = f32::from(byte) / 255.0;
    if srgb <= 0.040_45 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_documented_widths() {
        let widths: Vec<_> = FACE_COLOUR_FIELDS
            .iter()
            .map(|field| (field.name, field.width))
            .collect();
        assert_eq!(
            widths,
            [
                ("red", 8),
                ("green", 8),
                ("blue", 8),
                ("tint", 3),
                ("tint_weight", 5)
            ]
        );
    }

    #[test]
    fn colours_round_trip_through_srgb_bytes() {
        for byte in [0_u8, 1, 10, 54, 128, 200, 255] {
            assert_eq!(encode_srgb(decode_srgb(byte)), byte);
        }
        let colour = FaceColour::from_linear([0.2, 0.5, 0.01], TintClass::Grass, 0.5);
        let linear = colour.linear();
        for (got, want) in linear.iter().zip([0.2, 0.5, 0.01]) {
            assert!((got - want).abs() < 0.005, "{linear:?}");
        }
        assert_eq!(colour.tint(), TintClass::Grass);
        assert!((colour.tint_weight() - 0.5).abs() <= 0.5 / 31.0);
        let plain = FaceColour::from_linear([0.3; 3], TintClass::None, 1.0);
        assert_eq!(plain.tint_weight(), 0.0, "untinted faces carry no weight");
    }

    #[test]
    fn shading_tints_only_the_weighted_share() {
        let tint = [0.2, 0.6, 0.1];
        let full = FaceColour::from_linear([0.5; 3], TintClass::Grass, 1.0);
        let none = FaceColour::from_linear([0.5; 3], TintClass::None, 1.0);
        let base = full.linear();
        let shaded = full.shade(tint);
        for channel in 0..3 {
            assert!((shaded[channel] - base[channel] * tint[channel]).abs() < 1e-6);
        }
        assert_eq!(none.shade(tint), none.linear());
    }

    #[test]
    fn tables_answer_hidden_and_unlisted_materials() {
        let mut table = MaterialColours::new();
        assert!(table.is_empty());
        let stone = [FaceColour::from_srgb([90, 90, 90], TintClass::None, 0); 6];
        assert!(table.push(stone));
        let stone_id = MaterialId::FIRST_BLOCK;
        assert_eq!(table.get(stone_id, Face::PosY), stone[3]);
        assert_eq!(
            table.get(MaterialId::HIDDEN, Face::NegX),
            FaceColour::HIDDEN
        );
        let unlisted = MaterialId::new(9).expect("id");
        assert_eq!(table.get(unlisted, Face::NegX), FaceColour::UNKNOWN);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn biome_tints_fall_back_to_the_first_record_and_never_tint_untinted_faces() {
        let mut fallback = [[0.5; 3]; TintClass::COUNT];
        fallback[TintClass::Grass.index()] = [0.1, 0.9, 0.1];
        let mut plains = fallback;
        plains[TintClass::Water.index()] = [0.0, 0.2, 0.8];
        let tints = BiomeTints::new(Box::new([0, 1, 0]), Box::new([fallback, plains]));
        assert_eq!(tints.tint(1, TintClass::Water), [0.0, 0.2, 0.8]);
        assert_eq!(tints.tint(99, TintClass::Grass), [0.1, 0.9, 0.1]);
        assert_eq!(tints.tint(1, TintClass::None), [1.0; 3]);
    }
}
