//! Original fallback artwork for styled Latin mathematical letters and digits.

use super::*;

#[derive(Clone, Copy, Default)]
pub(super) struct Style {
    bold: bool,
    italic: bool,
}

/// Enumerates regular, bold and italic Latin alphabets and digits with Unicode's base letters.
pub(super) fn variants() -> impl Iterator<Item = (char, char, Style)> {
    let alphabets = [
        (0x1d400, true, false),
        (0x1d434, false, true),
        (0x1d468, true, true),
        (0x1d5a0, false, false),
        (0x1d5d4, true, false),
        (0x1d608, false, true),
        (0x1d63c, true, true),
        (0x1d670, false, false),
    ]
    .into_iter()
    .flat_map(|(start, bold, italic)| {
        ('A'..='Z')
            .chain('a'..='z')
            .enumerate()
            .filter_map(move |(index, base)| {
                let codepoint = char::from_u32(start + index as u32)?;
                // The italic h is encoded separately as U+210E; this slot is unassigned.
                (codepoint != '\u{1d455}').then_some((codepoint, base, Style { bold, italic }))
            })
    });
    let digits = [
        (0x1d7ce, true),
        (0x1d7e2, false),
        (0x1d7ec, true),
        (0x1d7f6, false),
    ]
    .into_iter()
    .flat_map(|(start, bold)| {
        ('0'..='9').enumerate().map(move |(index, base)| {
            (
                char::from_u32(start + index as u32).unwrap(),
                base,
                Style {
                    bold,
                    italic: false,
                },
            )
        })
    });
    alphabets.chain(digits)
}

impl Style {
    /// Additional raster allocation, admitted alongside the unstyled source metrics.
    pub(super) fn charge(self, metrics: &fontdue::Metrics) -> usize {
        let added = self.expansion(metrics.height as u32) as usize;
        if added == 0 {
            0
        } else {
            (metrics.width + added) * metrics.height
        }
    }

    /// Maximum extra columns from one-texel emboldening and a stepped italic lean.
    fn expansion(self, height: u32) -> u32 {
        u32::from(self.bold)
            + if self.italic {
                height.saturating_sub(1) / 4
            } else {
                0
            }
    }

    /// Derives binary, grid-aligned artwork while retaining the source glyph's baseline.
    pub(super) fn apply(self, glyph: &mut RasterizedGlyph) -> Result<(), FontCompileError> {
        let added = self.expansion(glyph.height);
        if added == 0 || glyph.alpha.iter().all(|&alpha| alpha == 0) {
            return Ok(());
        }
        let advance = if self.bold && glyph.advance_64 > 0 {
            glyph
                .advance_64
                .checked_add(64)
                .ok_or_else(|| metric_error(glyph.codepoint, "styled advance"))?
        } else {
            glyph.advance_64
        };
        let width = glyph.width + added;
        let mut alpha = vec![0; (width * glyph.height) as usize];
        for y in 0..glyph.height {
            let lean = if self.italic {
                (glyph.height - 1 - y) / 4
            } else {
                0
            };
            for x in 0..glyph.width {
                let value = glyph.alpha[(y * glyph.width + x) as usize];
                for bold in 0..=u32::from(self.bold) {
                    let target = &mut alpha[(y * width + x + lean + bold) as usize];
                    *target = (*target).max(value);
                }
            }
        }
        glyph.width = width;
        glyph.alpha = alpha.into_boxed_slice();
        glyph.advance_64 = advance;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_letters_keep_the_baseline_and_solid_source_pixels() {
        for (target, base, expected_width, expected_advance) in [
            ('𝑩', 'B', 4, 256),
            ('𝗦', 'S', 3, 256),
            ('𝟭', '1', 3, 256),
            ('𝙰', 'A', 2, 192),
        ] {
            let (_, letter, style) = variants().find(|(cp, _, _)| *cp == target).unwrap();
            assert_eq!(letter, base);
            let mut glyph = RasterizedGlyph {
                codepoint: target,
                width: 2,
                height: 5,
                bearing: [0, -5],
                advance_64: 192,
                alpha: vec![255; 10].into_boxed_slice(),
            };
            style.apply(&mut glyph).unwrap();
            assert_eq!(glyph.bearing, [0, -5]);
            assert_eq!(glyph.width, expected_width);
            assert_eq!(glyph.advance_64, expected_advance);
            assert!(glyph.alpha.iter().all(|&alpha| alpha == 0 || alpha == 255));
            assert!(glyph.alpha.contains(&255));
        }
    }
}
