//! The power-of-two box reduction every bounded actor-art allocation applies past its limit.

/// Box-filters whole RGBA8 layers by powers of two until their longest side fits limit.
/// Averages partial edge blocks; returns None for zero sizes/limit or artwork that already fits.
pub fn fit_rgba_within(
    width: u16,
    height: u16,
    rgba8: &[u8],
    limit: u32,
) -> Option<([u16; 2], Vec<u8>)> {
    let longest = u32::from(width.max(height));
    let (width, height) = (usize::from(width), usize::from(height));
    let layer = width * height * 4;
    if longest <= limit || limit == 0 || layer == 0 {
        return None;
    }
    let factor = longest.div_ceil(limit).next_power_of_two() as usize;
    let (out_width, out_height) = (width.div_ceil(factor), height.div_ceil(factor));
    let mut out = Vec::with_capacity(out_width * out_height * 4 * (rgba8.len() / layer));
    for layer in rgba8.chunks_exact(layer) {
        for y in 0..out_height {
            for x in 0..out_width {
                let mut sum = [0u32; 4];
                let mut count = 0;
                for source_y in y * factor..((y + 1) * factor).min(height) {
                    for source_x in x * factor..((x + 1) * factor).min(width) {
                        let at = (source_y * width + source_x) * 4;
                        for (total, value) in sum.iter_mut().zip(&layer[at..at + 4]) {
                            *total += u32::from(*value);
                        }
                        count += 1;
                    }
                }
                out.extend(sum.map(|total| (total / count) as u8));
            }
        }
    }
    Some(([out_width as u16, out_height as u16], out))
}

#[cfg(test)]
mod tests {
    use super::fit_rgba_within;

    #[test]
    fn oversized_art_halves_by_powers_of_two_and_averages_odd_edges() {
        let pixels = (0..3u8 * 2)
            .flat_map(|texel| [texel * 10, 0, 0, 255])
            .collect::<Vec<_>>();
        let ([width, height], reduced) = fit_rgba_within(3, 2, &pixels, 2).unwrap();
        assert_eq!([width, height], [2, 1]);
        // Reds 0, 10, 30 and 40 average to 20; the odd column's 20 and 50 average to 35.
        assert_eq!(reduced, [20, 0, 0, 255, 35, 0, 0, 255]);
        let ([width, height], _) =
            fit_rgba_within(1024, 1024, &vec![9; 1024 * 1024 * 4], 512).unwrap();
        assert_eq!([width, height], [512, 512]);
        let ([width, height], _) = fit_rgba_within(654, 576, &vec![9; 654 * 576 * 4], 512).unwrap();
        assert_eq!([width, height], [327, 288]);
    }

    #[test]
    fn fitting_or_empty_art_is_left_alone() {
        assert!(fit_rgba_within(512, 512, &vec![0; 512 * 512 * 4], 512).is_none());
        assert!(fit_rgba_within(4, 4, &[0; 64], 0).is_none());
        assert!(fit_rgba_within(0, 4, &[], 2).is_none());
    }
}
