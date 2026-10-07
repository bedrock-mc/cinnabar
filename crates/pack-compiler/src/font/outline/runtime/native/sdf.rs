use super::super::super::{FontCompileError, RasterizedGlyph, invalid};

const SPREAD: usize = 4;
const GRADIENT_EPSILON: f32 = 0.001;

#[derive(Clone, Copy)]
struct EdgeDistance {
    squared: f32,
    vector: [f32; 2],
    seed: bool,
}

pub(super) fn extent(width: u32, height: u32) -> Option<[u32; 2]> {
    if width == 0 || height == 0 {
        return Some([0, 0]);
    }
    let padding = (SPREAD * 2) as u32;
    Some([width.checked_add(padding)?, height.checked_add(padding)?])
}

pub(super) fn glyph(mut glyph: RasterizedGlyph) -> Result<RasterizedGlyph, FontCompileError> {
    if glyph.width == 0 || glyph.height == 0 {
        return Ok(glyph);
    }
    let [width, height] = extent(glyph.width, glyph.height)
        .ok_or_else(|| invalid("distance-field extent exceeds bounds"))?;
    let (width, height) = (width as usize, height as usize);
    let mut coverage = vec![0; width * height];
    for row in 0..glyph.height as usize {
        let start = (row + SPREAD) * width + SPREAD;
        coverage[start..start + glyph.width as usize].copy_from_slice(
            &glyph.alpha[row * glyph.width as usize..(row + 1) * glyph.width as usize],
        );
    }
    glyph.alpha = distance_field(&coverage, width, height)?.into_boxed_slice();
    glyph.width = width as u32;
    glyph.height = height as u32;
    glyph.bearing[0] = glyph.bearing[0]
        .checked_sub(SPREAD as i16)
        .ok_or_else(|| invalid("distance-field bearing exceeds bounds"))?;
    glyph.bearing[1] = glyph.bearing[1]
        .checked_sub(SPREAD as i16)
        .ok_or_else(|| invalid("distance-field bearing exceeds bounds"))?;
    Ok(glyph)
}

fn distance_field(source: &[u8], width: usize, height: usize) -> Result<Vec<u8>, FontCompileError> {
    if width == 0 || height == 0 || source.len() != width * height {
        return Err(invalid("distance-field coverage dimensions differ"));
    }
    #[cfg(test)]
    FIELD_ALLOCATIONS.with(|count| count.set(count.get() + 1));
    let stride = width + 2;
    let rows = height + 2;
    let mut coverage = vec![0; stride * rows];
    for y in 0..height {
        coverage[(y + 1) * stride + 1..(y + 1) * stride + 1 + width]
            .copy_from_slice(&source[y * width..(y + 1) * width]);
    }
    let mut distances = vec![
        EdgeDistance {
            squared: 2_000_000.0,
            vector: [1000.0; 2],
            seed: false,
        };
        coverage.len()
    ];
    for y in 1..=height {
        for x in 1..=width {
            let index = y * stride + x;
            let neighbors = [
                index - 1,
                index + 1,
                index - stride - 1,
                index - stride,
                index - stride + 1,
                index + stride - 1,
                index + stride,
                index + stride + 1,
            ];
            let alpha = coverage[index];
            let edge = neighbors.iter().any(|&neighbor| match alpha {
                0 => coverage[neighbor] >= 128,
                1..=127 => coverage[neighbor] > 0,
                _ => coverage[neighbor] < 128,
            });
            if !edge {
                continue;
            }
            let alpha_at = |index: usize| f32::from(coverage[index]) / 255.0;
            let mut gradient = [
                alpha_at(index - stride + 1) - alpha_at(index - stride - 1)
                    + std::f32::consts::SQRT_2 * (alpha_at(index + 1) - alpha_at(index - 1))
                    + alpha_at(index + stride + 1)
                    - alpha_at(index + stride - 1),
                alpha_at(index + stride - 1) - alpha_at(index - stride - 1)
                    + std::f32::consts::SQRT_2
                        * (alpha_at(index + stride) - alpha_at(index - stride))
                    + alpha_at(index + stride + 1)
                    - alpha_at(index - stride + 1),
            ];
            if gradient.iter().any(|axis| axis.abs() > GRADIENT_EPSILON) {
                let length = (gradient[0] * gradient[0] + gradient[1] * gradient[1]).sqrt();
                gradient[0] /= length;
                gradient[1] /= length;
            }
            let distance = coverage_edge_distance(gradient, alpha_at(index));
            distances[index] = EdgeDistance {
                squared: distance * distance,
                vector: [gradient[0] * distance, gradient[1] * distance],
                seed: true,
            };
        }
    }
    for y in 1..=height {
        for x in 1..=width {
            relax(
                &mut distances,
                y * stride + x,
                stride,
                &[(-1, -1), (0, -1), (1, -1), (-1, 0)],
            );
        }
        for x in (1..=width).rev() {
            relax(&mut distances, y * stride + x, stride, &[(1, 0)]);
        }
    }
    for y in (1..=height).rev() {
        for x in (1..=width).rev() {
            relax(
                &mut distances,
                y * stride + x,
                stride,
                &[(1, 0), (-1, 1), (0, 1), (1, 1)],
            );
        }
        for x in 1..=width {
            relax(&mut distances, y * stride + x, stride, &[(-1, 0)]);
        }
    }
    let mut output = Vec::with_capacity(source.len());
    for y in 1..=height {
        for x in 1..=width {
            let index = y * stride + x;
            let magnitude = distances[index].squared.max(0.0).sqrt();
            let signed = if coverage[index] >= 128 {
                magnitude
            } else {
                -magnitude
            };
            output.push((128.0 + 32.0 * signed).clamp(0.0, 255.0) as u8);
        }
    }
    Ok(output)
}

fn coverage_edge_distance(gradient: [f32; 2], alpha: f32) -> f32 {
    let mut axes = [gradient[0].abs(), gradient[1].abs()];
    if axes.iter().any(|&axis| axis <= GRADIENT_EPSILON) {
        return 0.5 - alpha;
    }
    if axes[0] < axes[1] {
        axes.swap(0, 1);
    }
    let [major, minor] = axes;
    let weighted = alpha * major;
    if weighted < minor * 0.5 {
        (major + minor) * 0.5 - (2.0 * major * minor * alpha).sqrt()
    } else if weighted < major - minor * 0.5 {
        (0.5 - alpha) * major
    } else {
        (2.0 * major * minor * (1.0 - alpha)).sqrt() - (major + minor) * 0.5
    }
}

fn relax(
    distances: &mut [EdgeDistance],
    index: usize,
    stride: usize,
    neighbors: &[(isize, isize)],
) {
    if distances[index].seed {
        return;
    }
    for &(x, y) in neighbors {
        let candidate = distances[(index as isize + x + y * stride as isize) as usize];
        let x = x as f32;
        let y = y as f32;
        let squared = candidate.squared
            + 2.0 * (candidate.vector[0] * x + candidate.vector[1] * y)
            + x * x
            + y * y;
        if squared < distances[index].squared {
            distances[index] = EdgeDistance {
                squared,
                vector: [candidate.vector[0] + x, candidate.vector[1] + y],
                seed: false,
            };
        }
    }
}

#[cfg(test)]
std::thread_local! {
    pub(super) static FIELD_ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_edge_retains_signed_half_pixel_distance_and_saturates() {
        let width = 24;
        let height = 16;
        let coverage = (0..height)
            .flat_map(|_| (0..width).map(|x| if x >= 12 { 255 } else { 0 }))
            .collect::<Vec<_>>();
        let field = distance_field(&coverage, width, height).unwrap();
        let row = &field[8 * width..9 * width];
        assert_eq!(&row[9..15], &[48, 80, 112, 144, 176, 208]);
        assert_eq!(row[4], 0);
        assert_eq!(row[19], 255);
    }

    #[test]
    fn partial_coverage_moves_the_edge_instead_of_thresholding_it() {
        for (alpha, encoded) in [(64, 120), (127, 127), (128, 128), (191, 135)] {
            let width = 15;
            let height = 15;
            let coverage = (0..height)
                .flat_map(|_| {
                    (0..width).map(|x| match x {
                        0..=6 => 0,
                        7 => alpha,
                        _ => 255,
                    })
                })
                .collect::<Vec<_>>();
            let field = distance_field(&coverage, width, height).unwrap();
            assert_eq!(field[7 * width + 7], encoded);
        }
    }

    #[test]
    fn padding_keeps_source_ink_in_place_and_blank_glyphs_allocate_no_field() {
        let source = RasterizedGlyph {
            codepoint: 'A',
            width: 2,
            height: 2,
            bearing: [3, -5],
            advance_64: 640,
            alpha: vec![255; 4].into_boxed_slice(),
        };
        let field = glyph(source).unwrap();
        assert_eq!((field.width, field.height), (10, 10));
        assert_eq!(field.bearing, [-1, -9]);
        assert_eq!(field.advance_64, 640);
        assert!(field.alpha[4 * 10 + 4] >= 128);
        assert_eq!(field.alpha[0], 0);
        let blank = glyph(RasterizedGlyph {
            codepoint: ' ',
            width: 0,
            height: 0,
            bearing: [0, 0],
            advance_64: 320,
            alpha: Box::default(),
        })
        .unwrap();
        assert!(blank.alpha.is_empty());
        assert_eq!(blank.advance_64, 320);
    }
}
