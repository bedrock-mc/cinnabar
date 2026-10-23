//! Measures and paints formatted glyph ink before atlas placement.

use super::*;

pub(super) struct LineLayout {
    pub(super) layout: Arc<TextLayout>,
    pub(super) size: [u32; 2],
    pub(super) top: i32,
    pub(super) advance: u32,
    pub(super) ink_y: [i32; 2],
}

/// Measures width-bounded ink before any atlas placement or pixel allocation.
pub(super) fn measure_line(
    text: &str,
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
) -> Option<LineLayout> {
    #[cfg(feature = "tracy")]
    let _zone = tracing::info_span!("nametag.measure").entered();
    let layout = layouts
        .layout(TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64: MAX_LINE_TEXELS * 64,
            line_height_64: TEXT_LINE_HEIGHT_64,
            baseline_64: TEXT_BASELINE_64,
            scale: UiScale::default(),
            font,
            wrap: TextWrap {
                max_lines: Some(1),
                allow_visual_overflow: true,
                ..Default::default()
            },
        })
        .ok()?;
    let advance = layout.size_64()[0].div_ceil(64).clamp(1, MAX_LINE_TEXELS);
    let glyphs = || layout.glyphs().iter().filter(|glyph| glyph.line == 0);
    let ink = || {
        glyphs().filter(|glyph| {
            glyph.bounds_64[2] > glyph.bounds_64[0] && glyph.bounds_64[3] > glyph.bounds_64[1]
        })
    };
    let ink_top = ink()
        .map(|glyph| glyph.bounds_64[1].div_euclid(64))
        .min()
        .unwrap_or(0);
    let ink_bottom = ink()
        .map(|glyph| (glyph.bounds_64[3] + 63).div_euclid(64))
        .max()
        .unwrap_or(0);
    let top = glyphs()
        .map(|glyph| glyph.bounds_64[1].div_euclid(64))
        .min()
        .unwrap_or(0)
        .min(0);
    let bottom = glyphs()
        .map(|glyph| (glyph.bounds_64[3] + 63).div_euclid(64))
        .max()
        .unwrap_or(0)
        .max(TEXT_LINE_HEIGHT_64.div_ceil(64) as i32);
    let right = glyphs()
        .map(|glyph| {
            let extra = if glyph.style.bold
                && glyph.bounds_64[2] > glyph.bounds_64[0]
                && glyph.bounds_64[3] > glyph.bounds_64[1]
            {
                TEXT_BOLD_OFFSET_64 as i32
            } else {
                0
            };
            let shear = if glyph.style.italic {
                ui::TEXT_ITALIC_SHEAR_PX * layout.key().effect_scale()
            } else {
                0.0
            };
            ((glyph.bounds_64[2] + extra) as f32 / 64.0 + shear).ceil() as i32
        })
        .max()
        .unwrap_or(0)
        .max(advance as i32);
    // Clip oversized ink to one atlas cell before allocating its pixels.
    let (width, height) = (
        (right as u32).min(MAX_LINE_TEXELS),
        ((bottom - top) as u32).min(NAMETAG_ATLAS_SIDE),
    );
    Some(LineLayout {
        layout,
        size: [width, height],
        top,
        advance,
        ink_y: [ink_top, ink_bottom],
    })
}

/// Rasterizes admitted ink with the glyph colors and active formatting palette.
pub(super) fn rasterize_line<'p>(
    measured: &LineLayout,
    pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    palette: &ui::FormattingPalette,
    effects: ui::TextEffects<'_>,
) -> Vec<u8> {
    #[cfg(feature = "tracy")]
    let _zone = tracing::info_span!("nametag.raster").entered();
    let [width, height] = measured.size;
    let top = measured.top;
    let glyphs = || {
        measured
            .layout
            .glyphs()
            .iter()
            .filter(|glyph| glyph.line == 0)
    };
    let mut canvas = vec![0u8; (width * height * 4) as usize];
    for (index, glyph) in glyphs().enumerate() {
        let (page, uv) = effects.glyph_raster(glyph, index, measured.layout.id());
        let Some(page) = pages(usize::from(page)) else {
            continue;
        };
        let tint = if glyph.rendering == assets::FontRendering::Coverage
            && assets::is_private_use_glyph(glyph.resolved_codepoint)
        {
            [255; 3]
        } else {
            palette.rgb(glyph.style.color).unwrap_or([255; 3])
        };
        // Sheet glyphs are drawn scaled into their bounds, so sample the source nearest-texel.
        let mut bounds = glyph.bounds_64.map(|value| value as f32 / 64.0);
        bounds[1] -= top as f32;
        bounds[3] -= top as f32;
        let shear = if glyph.style.italic {
            ui::TEXT_ITALIC_SHEAR_PX * measured.layout.key().effect_scale()
        } else {
            0.0
        };
        rasterize_glyph(
            &mut canvas,
            [width, height],
            page,
            bounds,
            uv,
            tint,
            false,
            shear,
        );
        if glyph.style.bold {
            let offset = TEXT_BOLD_OFFSET_64 as f32 / 64.0;
            bounds[0] += offset;
            bounds[2] += offset;
            rasterize_glyph(
                &mut canvas,
                [width, height],
                page,
                bounds,
                uv,
                tint,
                true,
                shear,
            );
        }
    }
    canvas
}

/// Samples one glyph into its clipped destination bounds.
#[allow(clippy::too_many_arguments)]
fn rasterize_glyph(
    canvas: &mut [u8],
    [width, height]: [u32; 2],
    page: GlyphPage<'_>,
    bounds: [f32; 4],
    uv: [u16; 4],
    tint: [u8; 3],
    composite: bool,
    shear: f32,
) {
    let [u0, v0, u1, v1] = uv.map(f32::from);
    let (source_width, source_height) = (u1 - u0, v1 - v0);
    let (dest_width, dest_height) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
    if source_width <= 0.0 || source_height <= 0.0 || dest_width <= 0.0 || dest_height <= 0.0 {
        return;
    }
    for dy in bounds[1].floor().max(0.0) as u32..(bounds[3].ceil() as u32).min(height) {
        let fy = (dy as f32 + 0.5 - bounds[1]) / dest_height;
        let lean = shear * (1.0 - fy);
        for dx in (bounds[0] + lean).floor().max(0.0) as u32
            ..((bounds[2] + lean).ceil() as u32).min(width)
        {
            let fx = (dx as f32 + 0.5 - bounds[0] - lean) / dest_width;
            if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
                continue;
            }
            let sx = (u0 + fx * source_width) as u32;
            let sy = (v0 + fy * source_height) as u32;
            if sx >= page.width || sy >= page.height {
                continue;
            }
            let Some(texel) = page.texel((sy * page.width + sx) as usize) else {
                continue;
            };
            if texel[3] == 0 {
                continue;
            }
            let target = ((dy * width + dx) * 4) as usize;
            if composite {
                blend_glyph_texel(&mut canvas[target..target + 4], texel, tint);
            } else {
                for channel in 0..3 {
                    canvas[target + channel] =
                        (u16::from(texel[channel]) * u16::from(tint[channel]) / 255) as u8;
                }
                canvas[target + 3] = texel[3];
            }
        }
    }
}

/// Composites straight-alpha glyph coverage without losing partially covered ink.
fn blend_glyph_texel(target: &mut [u8], texel: [u8; 4], tint: [u8; 3]) {
    let source_alpha = u32::from(texel[3]);
    let target_alpha = u32::from(target[3]);
    let remaining = 255 - source_alpha;
    let alpha = source_alpha * 255 + target_alpha * remaining;
    for channel in 0..3 {
        let source = u32::from(texel[channel]) * u32::from(tint[channel]) / 255;
        let color =
            source * source_alpha * 255 + u32::from(target[channel]) * target_alpha * remaining;
        target[channel] = ((color + alpha / 2) / alpha) as u8;
    }
    target[3] = ((alpha + 127) / 255) as u8;
}
