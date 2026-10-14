//! Greedy rectangle merging on 32×32 face bit rows, and the bit transpose that turns X-run
//! masks into in-plane rows.

use crate::{geometry::SECTION_SIDE, mesh::quad::MAX_QUAD_SIDE};

/// One face plane: bit `u` of row `v` is a face at in-plane `[u, v]`.
pub(super) type Rows = [u32; SECTION_SIDE];

const _: () = assert!(SECTION_SIDE == u32::BITS as usize);

/// Merges the faces of `rows` into rectangles of equal `key`, consuming the rows, and
/// calls `emit(u, v, width, height, key)` for each. Scans rows from `v = 0` and runs from the
/// lowest `u`: a run widens along `u` while keys match, then grows along `v` while the
/// whole span has faces of the same key, each side capped at [`MAX_QUAD_SIDE`].
pub(super) fn merge(
    rows: &mut Rows,
    key: impl Fn(usize, usize) -> u32,
    mut emit: impl FnMut(usize, usize, usize, usize, u32),
) {
    for v in 0..SECTION_SIDE {
        while rows[v] != 0 {
            let row = rows[v];
            let u = row.trailing_zeros() as usize;
            let first = key(u, v);
            let run = ((row >> u).trailing_ones() as usize).min(MAX_QUAD_SIDE);
            let mut width = 1;
            while width < run && key(u + width, v) == first {
                width += 1;
            }
            let span = (u32::MAX >> (u32::BITS as usize - width)) << u;
            let mut height = 1;
            'grow: while v + height < SECTION_SIDE && height < MAX_QUAD_SIDE {
                if rows[v + height] & span != span {
                    break;
                }
                for offset in 0..width {
                    if key(u + offset, v + height) != first {
                        break 'grow;
                    }
                }
                height += 1;
            }
            for row in &mut rows[v..v + height] {
                *row &= !span;
            }
            emit(u, v, width, height, first);
        }
    }
}

/// Transposes a 32×32 bit matrix in place: bit `c` of row `r` moves to bit `r` of row `c`.
pub(super) fn transpose(rows: &mut Rows) {
    let mut step = 16;
    let mut mask = 0x0000_ffff_u32;
    while step != 0 {
        let mut row = 0;
        while row < SECTION_SIDE {
            let swap = ((rows[row] >> step) ^ rows[row + step]) & mask;
            rows[row] ^= swap << step;
            rows[row + step] ^= swap;
            row = (row + step + 1) & !step;
        }
        step >>= 1;
        mask ^= mask << step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transpose_matches_the_definition() {
        let mut state = 0x1234_5678_u32;
        let original: Rows = std::array::from_fn(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        });
        let mut rows = original;
        transpose(&mut rows);
        for (r, original_row) in original.iter().enumerate() {
            for (c, row) in rows.iter().enumerate() {
                assert_eq!(row >> r & 1, original_row >> c & 1, "({r}, {c})");
            }
        }
    }

    #[test]
    fn merging_caps_sides_and_splits_keys() {
        let mut rows = [u32::MAX; SECTION_SIDE];
        let mut quads = Vec::new();
        merge(
            &mut rows,
            |u, _| u32::from(u >= 20),
            |u, v, w, h, k| quads.push((u, v, w, h, k)),
        );
        assert_eq!(rows, [0; SECTION_SIDE]);
        assert!(quads.contains(&(0, 0, 16, 16, 0)));
        assert!(
            quads.contains(&(16, 0, 4, 16, 0)),
            "the key change ends the run"
        );
        assert!(quads.contains(&(20, 0, 12, 16, 1)));
        let area: usize = quads.iter().map(|q| q.2 * q.3).sum();
        assert_eq!(area, SECTION_SIDE * SECTION_SIDE);
    }
}
