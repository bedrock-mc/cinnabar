//! Keeps every session icon on the bounded session page by halving the largest icons first.
//!
//! The base game grows its item atlas until every tile fits and only then scales tiles down, so
//! it never leaves an item without its icon. The session page has a smaller side limit, so the
//! largest icons, which already exceed any on-screen slot, give up resolution first.

use super::{GUTTER, SessionIcon};

/// Largest side an icon is halved down to; smaller icons keep their pixels.
const MIN_SHRUNK_SIDE: u32 = 16;

/// Halvings per icon that let the tallest-first shelf layout fit a `side` page. Icons flagged in
/// `fixed` keep their size. When even the smallest permitted sizes overflow, the result still
/// shrinks as far as allowed and the packer leaves out what does not fit.
pub(super) fn halvings(sizes: &[[u32; 2]], fixed: &[bool], side: u32) -> Vec<u32> {
    let mut shifts = vec![0; sizes.len()];
    loop {
        let scaled = scaled_sizes(sizes, &shifts);
        if shelf_fits(&scaled, side) {
            return shifts;
        }
        let largest = scaled
            .iter()
            .zip(fixed)
            .filter(|(_, fixed)| !**fixed)
            .map(|(size, _)| size[0].max(size[1]))
            .max()
            .unwrap_or(0);
        if largest <= MIN_SHRUNK_SIDE {
            return shifts;
        }
        for ((size, shift), fixed) in scaled.iter().zip(&mut shifts).zip(fixed) {
            if !fixed && size[0].max(size[1]) == largest {
                *shift += 1;
            }
        }
    }
}

/// Each icon's size after its halvings, rounding partial texels up.
pub(super) fn scaled_sizes(sizes: &[[u32; 2]], shifts: &[u32]) -> Vec<[u32; 2]> {
    sizes
        .iter()
        .zip(shifts)
        .map(|(size, &shift)| size.map(|extent| scaled(extent, shift)))
        .collect()
}

fn scaled(extent: u32, shift: u32) -> u32 {
    extent.div_ceil(1 << shift.min(31)).max(1)
}

/// Whether tallest-first shelves of the gutter-padded sizes fit a square `side` page.
pub(super) fn shelf_fits(sizes: &[[u32; 2]], side: u32) -> bool {
    let mut ordered = sizes.to_vec();
    ordered.sort_by_key(|size| std::cmp::Reverse(size[1]));
    let (mut x, mut y, mut row) = (0, 0, 0);
    for [width, height] in ordered {
        let (width, height) = (width + GUTTER * 2, height + GUTTER * 2);
        if width > side {
            return false;
        }
        if x + width > side {
            x = 0;
            y += row;
            row = 0;
        }
        x += width;
        row = row.max(height);
    }
    y + row <= side
}

/// Averages each `2^shift` square of straight-alpha texels, weighting colour by coverage so
/// transparent texels do not darken edges.
pub(super) fn shrink(icon: &SessionIcon, shift: u32) -> ([u32; 2], Vec<u8>) {
    let [width, height] = [icon.width, icon.height].map(|extent| scaled(extent, shift));
    let step = 1u32 << shift.min(31);
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for sy in y * step..((y + 1) * step).min(icon.height) {
                for sx in x * step..((x + 1) * step).min(icon.width) {
                    let at = ((sy * icon.width + sx) * 4) as usize;
                    let [r, g, b, a] = [0, 1, 2, 3].map(|c| u64::from(icon.rgba8[at + c]));
                    sum[0] += r * a;
                    sum[1] += g * a;
                    sum[2] += b * a;
                    sum[3] += a;
                    count += 1;
                }
            }
            let color = |channel: u64| {
                if sum[3] == 0 {
                    0
                } else {
                    (channel / sum[3]) as u8
                }
            };
            out.extend([
                color(sum[0]),
                color(sum[1]),
                color(sum[2]),
                (sum[3] / count.max(1)) as u8,
            ]);
        }
    }
    ([width, height], out)
}
