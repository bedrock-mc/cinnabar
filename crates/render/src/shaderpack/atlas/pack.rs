//! Gap-free packing of power-of-two square sprites into one 2D texture.
//!
//! Sprites are placed largest first along a Z-order curve measured in units of the
//! current sprite side. Because every side is a power of two and sides only shrink,
//! each sprite lands on a multiple of its own side and the placed area never leaves
//! holes, so a set fits exactly when its total area fits the chosen extent.

/// A sprite's square at atlas mip 0, aligned to its own side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SpriteRect {
    pub x: u32,
    pub y: u32,
    pub side: u32,
}

impl SpriteRect {
    /// The same square at atlas mip `level`; whole texels while `side >> level` is non-zero.
    pub const fn at_mip(self, level: u32) -> Self {
        Self {
            x: self.x >> level,
            y: self.y >> level,
            side: self.side >> level,
        }
    }
}

/// Extent of a packed texture and each input sprite's square, in input order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackedAtlas {
    pub width: u32,
    pub height: u32,
    pub rects: Vec<SpriteRect>,
}

/// Packs `(key, side)` sprites, ordering placement by side (largest first) then key, into
/// the smallest `width × height` with `width ∈ {height, 2·height}` that holds their area.
///
/// Returns `None` when a side is not a power of two or the extent would exceed
/// `max_dimension`. Equal inputs always produce equal placements.
pub(crate) fn pack_sprites(sprites: &[(u32, u32)], max_dimension: u32) -> Option<PackedAtlas> {
    if sprites.iter().any(|&(_, side)| !side.is_power_of_two()) {
        return None;
    }
    let largest = sprites.iter().map(|&(_, side)| side).max()?;
    let area = sprites
        .iter()
        .map(|&(_, side)| u64::from(side) * u64::from(side))
        .sum::<u64>();
    let (mut width, mut height) = (largest, largest);
    while u64::from(width) * u64::from(height) < area {
        if width == height {
            width = width.checked_mul(2)?;
        } else {
            height *= 2;
        }
    }
    if width > max_dimension {
        return None;
    }
    let mut order = (0..sprites.len()).collect::<Vec<_>>();
    order.sort_unstable_by_key(|&index| (std::cmp::Reverse(sprites[index].1), sprites[index].0));
    let mut rects = vec![
        SpriteRect {
            x: 0,
            y: 0,
            side: 0
        };
        sprites.len()
    ];
    let mut cursor = 0_u64;
    let mut current_side = largest;
    for index in order {
        let side = sprites[index].1;
        if side < current_side {
            let ratio = u64::from(current_side / side);
            cursor *= ratio * ratio;
            current_side = side;
        }
        let per_square = u64::from(height / side).pow(2);
        let square = u32::try_from(cursor / per_square).ok()?;
        let (column, row) = deinterleave(cursor % per_square);
        debug_assert!(square < width / height, "packed area fits the extent");
        rects[index] = SpriteRect {
            x: square * height + column * side,
            y: row * side,
            side,
        };
        cursor += 1;
    }
    Some(PackedAtlas {
        width,
        height,
        rects,
    })
}

/// Splits a Z-order index into its even (column) and odd (row) bits.
fn deinterleave(index: u64) -> (u32, u32) {
    let compact = |mut bits: u64| {
        bits &= 0x5555_5555_5555_5555;
        bits = (bits | (bits >> 1)) & 0x3333_3333_3333_3333;
        bits = (bits | (bits >> 2)) & 0x0f0f_0f0f_0f0f_0f0f;
        bits = (bits | (bits >> 4)) & 0x00ff_00ff_00ff_00ff;
        bits = (bits | (bits >> 8)) & 0x0000_ffff_0000_ffff;
        bits = (bits | (bits >> 16)) & 0x0000_0000_ffff_ffff;
        bits as u32
    };
    (compact(index), compact(index >> 1))
}
