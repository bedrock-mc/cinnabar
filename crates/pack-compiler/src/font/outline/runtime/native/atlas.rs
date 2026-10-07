use assets::{GlyphMetrics, MAX_FONT_PAGES};

use super::{FontCompileError, RasterizedGlyph, invalid};

pub(super) struct Atlas {
    pub(super) glyphs: Vec<GlyphMetrics>,
    pub(super) pages: Vec<Box<[u8]>>,
    pub(super) side: u32,
}

pub(super) fn pack_pages(
    source: Vec<RasterizedGlyph>,
    maximum_side: u32,
    adaptive: bool,
) -> Result<Atlas, FontCompileError> {
    pack_pages_bounded(source, maximum_side, adaptive, MAX_FONT_PAGES)
}

pub(super) fn pack_pages_bounded(
    mut source: Vec<RasterizedGlyph>,
    maximum_side: u32,
    adaptive: bool,
    maximum_pages: usize,
) -> Result<Atlas, FontCompileError> {
    if maximum_pages == 0 || maximum_pages > MAX_FONT_PAGES {
        return Err(invalid("native glyph page budget exceeds bounds"));
    }
    for glyph in &mut source {
        if glyph.width == 0 || glyph.height == 0 {
            glyph.width = 1;
            glyph.height = 1;
            glyph.alpha = vec![0].into_boxed_slice();
        }
    }
    let mut side = if adaptive { 256 } else { maximum_side };
    while side < maximum_side && admitted(&source, side) != source.len() {
        side *= 2;
    }
    let mut ranges = Vec::new();
    let mut first = 0;
    while first < source.len() {
        if ranges.len() == maximum_pages {
            return Err(invalid("native glyph pages exceed their page budget"));
        }
        let count = admitted(&source[first..], side);
        if count == 0 {
            return Err(FontCompileError::OutlineAtlasFull { side });
        }
        ranges.push(first..first + count);
        first += count;
    }
    let mut glyphs = Vec::with_capacity(source.len());
    let mut pages = Vec::with_capacity(ranges.len());
    for range in ranges {
        #[cfg(test)]
        PACKED_PAGES.with(|count| count.set(count.get() + 1));
        let (mut part, mut pixels) = super::pack(&source[range], side)?;
        for glyph in &mut part {
            glyph.page = pages.len() as u16;
        }
        for texel in pixels.chunks_exact_mut(4) {
            texel[..3].fill(255);
        }
        glyphs.extend(part);
        pages.push(pixels);
    }
    Ok(Atlas {
        glyphs,
        pages,
        side,
    })
}

fn admitted(glyphs: &[RasterizedGlyph], side: u32) -> usize {
    let padding = super::ATLAS_PADDING;
    let mut x = padding;
    let mut y = padding;
    let mut row_height = 0;
    for (index, glyph) in glyphs.iter().enumerate() {
        if glyph.width + padding * 2 > side || glyph.height + padding * 2 > side {
            return index;
        }
        if x + glyph.width + padding > side {
            x = padding;
            y += row_height + padding;
            row_height = 0;
        }
        if y + glyph.height + padding > side {
            return index;
        }
        x += glyph.width + padding;
        row_height = row_height.max(glyph.height);
    }
    glyphs.len()
}

#[cfg(test)]
std::thread_local! {
    static PACKED_PAGES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(codepoint: char, width: u32, height: u32, alpha: u8) -> RasterizedGlyph {
        RasterizedGlyph {
            codepoint,
            width,
            height,
            bearing: [1, 2],
            advance_64: 160,
            alpha: vec![alpha; width as usize * height as usize].into_boxed_slice(),
        }
    }

    #[test]
    fn oversized_sets_preserve_every_glyphs_texels_across_pages() {
        let source = [('A', 31), ('B', 107), ('C', 221)]
            .into_iter()
            .map(|(ch, alpha)| glyph(ch, 254, 254, alpha))
            .collect();
        let atlas = pack_pages(source, 256, false).unwrap();
        assert!(atlas.pages.len() > 1);
        for (metrics, expected) in atlas.glyphs.iter().zip([31, 107, 221]) {
            assert_eq!(metrics.bearing, [1, 2]);
            assert_eq!(metrics.advance_64, 160);
            let pixels = &atlas.pages[usize::from(metrics.page)];
            for y in metrics.uv[1]..metrics.uv[3] {
                for x in metrics.uv[0]..metrics.uv[2] {
                    let index = (u32::from(y) * atlas.side + u32::from(x)) as usize * 4;
                    assert_eq!(&pixels[index..index + 4], &[255, 255, 255, expected]);
                }
            }
        }
    }

    #[test]
    fn exceeded_page_budget_rejects_before_allocating_any_page() {
        let source = || {
            ['A', 'B', 'C']
                .into_iter()
                .map(|ch| glyph(ch, 254, 254, 255))
                .collect()
        };
        PACKED_PAGES.with(|count| count.set(0));
        assert!(pack_pages_bounded(source(), 256, false, 2).is_err());
        assert_eq!(PACKED_PAGES.with(std::cell::Cell::get), 0);
        let admitted = pack_pages_bounded(source(), 256, false, 3).unwrap();
        assert_eq!(admitted.pages.len(), 3);
        assert_eq!(PACKED_PAGES.with(std::cell::Cell::get), 3);
    }

    #[test]
    fn empty_and_collapsed_ink_get_transparent_tiles_with_source_advances() {
        let atlas = pack_pages(
            vec![glyph(' ', 0, 0, 255), glyph('A', 0, 3, 255)],
            1024,
            true,
        )
        .unwrap();
        assert_eq!(atlas.side, 256);
        for metrics in atlas.glyphs {
            assert_eq!(metrics.advance_64, 160);
            assert!(metrics.uv[2] > metrics.uv[0] && metrics.uv[3] > metrics.uv[1]);
            let pixels = &atlas.pages[usize::from(metrics.page)];
            let index =
                (u32::from(metrics.uv[1]) * atlas.side + u32::from(metrics.uv[0])) as usize * 4;
            assert_eq!(&pixels[index..index + 4], &[255, 255, 255, 0]);
        }
    }
}
