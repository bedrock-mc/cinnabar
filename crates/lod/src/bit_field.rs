//! Named bit fields of 32-bit words shared with shaders: the voxel word and the quad words.

/// One named bit field of a 32-bit word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitField {
    pub name: &'static str,
    pub shift: u32,
    pub width: u32,
}

impl BitField {
    /// The first field of a word, starting at bit 0.
    pub(crate) const fn first(name: &'static str, width: u32) -> Self {
        Self {
            name,
            shift: 0,
            width,
        }
    }

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
    pub(crate) const fn read(self, word: u32) -> u32 {
        (word & self.mask()) >> self.shift
    }

    /// `value` shifted into this field; bits past the width are dropped.
    pub(crate) const fn place(self, value: u32) -> u32 {
        (value << self.shift) & self.mask()
    }

    /// The field of `width` bits directly above this one.
    pub(crate) const fn after(self, name: &'static str, width: u32) -> Self {
        Self {
            name,
            shift: self.shift + self.width,
            width,
        }
    }
}

/// True when `fields` never overlap and together cover all 32 bits; layouts assert this at
/// compile time.
#[must_use]
pub const fn covers_word(fields: &[BitField]) -> bool {
    let mut covered = 0_u32;
    let mut index = 0;
    while index < fields.len() {
        let mask = fields[index].mask();
        if covered & mask != 0 {
            return false;
        }
        covered |= mask;
        index += 1;
    }
    covered == u32::MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_rejects_gaps_and_overlaps() {
        let low = BitField::first("low", 16);
        let high = low.after("high", 16);
        assert!(covers_word(&[low, high]));
        assert!(!covers_word(&[low]));
        assert!(!covers_word(&[low, high, BitField::first("again", 1)]));
        assert_eq!(high.read(high.place(0xbeef)), 0xbeef);
        assert_eq!(high.place(0x1_0001), 1 << 16, "bits past the width drop");
    }
}
