//! Line building for a [`TextLayout`]: word wrap at the last fitting space,
//! vanilla's chop of a word wider than the line, per-line alignment, line
//! padding and `...` truncation at a line limit.

use assets::{CompiledFontCatalog, GlyphMetrics};

use super::{
    FIXED_POINT_DENOMINATOR, GlyphQuad, MAX_GLYPHS_PER_LAYOUT, MAX_WRAP_LINES,
    REPLACEMENT_CODEPOINT, TEXT_BOLD_OFFSET_64, TextError, TextLayout, TextLayoutKey,
    TextLayoutRequest, TextLineAlign, TextStyle, WordChop, invisible,
    parse::parse_bedrock_text_with_style,
};

pub(super) fn build_layout(
    id: u64,
    key: TextLayoutKey,
    request: TextLayoutRequest<'_>,
) -> Result<TextLayout, TextError> {
    let spans = parse_bedrock_text_with_style(
        request.text,
        crate::UiLimits::MAX_TEXT_BYTES,
        request.style,
    )?;
    let characters: Vec<(char, TextStyle)> = spans
        .iter()
        .flat_map(|span| {
            span.text
                .chars()
                .map(move |codepoint| (codepoint, span.style))
        })
        .collect();
    let glyph_count = characters.iter().filter(|(c, _)| *c != '\n').count();
    if glyph_count > MAX_GLYPHS_PER_LAYOUT {
        return Err(TextError::GlyphLimitExceeded {
            actual: glyph_count,
            limit: MAX_GLYPHS_PER_LAYOUT,
        });
    }
    let scale_1024 = i64::from(key.scale_1024);
    let line_height_64 = scale_metric(i64::from(request.line_height_64), scale_1024)?;
    let mut lines = Lines {
        request,
        scale_1024,
        line_height_64,
        pitch_64: (line_height_64 + i64::from(request.wrap.line_padding_64)).max(1),
        baseline_64: scale_metric(i64::from(request.baseline_64), scale_1024)?,
        glyphs: Vec::with_capacity(glyph_count),
        marks: Vec::with_capacity(glyph_count),
        widths: Vec::new(),
        line: 0,
        line_start: 0,
        min_64: 0,
        max_64: 0,
        x_64: 0,
        ellipsized: false,
    };
    let width_64 = u64::from(request.width_64);
    let mut space: Option<WrapPoint> = None;
    let mut index = 0usize;
    while let Some(&(codepoint, style)) = characters.get(index) {
        let source = index;
        index += 1;
        if codepoint == '\n' {
            if !lines.break_line()? {
                break;
            }
            space = None;
            continue;
        }
        if invisible::is_invisible(codepoint) {
            continue;
        }
        let glyph = lines.glyph(codepoint, style)?;
        let mut candidate = lines.candidate(&glyph)?;
        if lines.glyphs.len() > lines.line_start && candidate.width_64 > width_64 {
            if let Some(point) = space.take().filter(|point| point.glyphs > lines.line_start) {
                // Drop the space and the partial word; the word restarts the next line.
                lines.truncate(point.glyphs);
                index = point.resume;
                if !lines.break_line()? {
                    break;
                }
                continue;
            }
            if request.wrap.chop != WordChop::Glyph {
                // A space that overflows ends the line and is consumed.
                if codepoint != ' ' {
                    index = lines.chop(source)?;
                }
                if !lines.break_line()? {
                    break;
                }
                continue;
            }
            if !lines.break_line()? {
                break;
            }
            candidate = lines.candidate(&glyph)?;
        }
        // Only a legacy glyph break refuses a glyph wider than the whole line;
        // a vanilla label lets it overflow.
        if candidate.width_64 > width_64 && request.wrap.chop == WordChop::Glyph {
            return Err(TextError::VisualWidthExceeded {
                actual_64: candidate.width_64,
                limit_64: width_64,
            });
        }
        if codepoint == ' ' {
            space = Some(WrapPoint {
                glyphs: lines.glyphs.len(),
                resume: index,
            });
        }
        lines.push(glyph, style, candidate, source)?;
    }
    lines.finish_line()?;
    lines.into_layout(id, key)
}

/// Where a line may break: the space's glyph index and the character after it.
struct WrapPoint {
    glyphs: usize,
    resume: usize,
}

/// A glyph's line extent and pen after it, plus the character it came from.
#[derive(Clone, Copy)]
struct Mark {
    source: usize,
    min_64: i64,
    max_64: i64,
    pen_64: i64,
}

struct Glyph {
    codepoint: char,
    resolved: char,
    metrics: GlyphMetrics,
    draw_size_64: Option<[u32; 2]>,
    advance_64: i64,
    bold_offset_64: i64,
}

struct LineCandidate {
    bounds_64: [i32; 4],
    pen_end_64: i64,
    min_64: i64,
    max_64: i64,
    width_64: u64,
}

struct Lines<'a> {
    request: TextLayoutRequest<'a>,
    scale_1024: i64,
    line_height_64: i64,
    pitch_64: i64,
    baseline_64: i64,
    glyphs: Vec<GlyphQuad>,
    marks: Vec<Mark>,
    /// Finished lines' widths, for alignment.
    widths: Vec<i64>,
    line: usize,
    line_start: usize,
    min_64: i64,
    max_64: i64,
    x_64: i64,
    ellipsized: bool,
}

impl Lines<'_> {
    fn glyph(&self, codepoint: char, style: TextStyle) -> Result<Glyph, TextError> {
        let (resolved, metrics) = resolve_glyph(self.request.font, codepoint)?;
        let bold_offset_64 = if style.bold {
            i64::from(TEXT_BOLD_OFFSET_64)
        } else {
            0
        };
        let advance_64 = i64::from(metrics.advance_64);
        let advance_64 = if advance_64 > 0 {
            advance_64 + bold_offset_64
        } else {
            advance_64
        };
        Ok(Glyph {
            codepoint,
            resolved,
            metrics,
            draw_size_64: self.request.font.draw_size_64(resolved),
            advance_64: scale_metric(advance_64, self.scale_1024)?,
            bold_offset_64: scale_metric(bold_offset_64, self.scale_1024)?,
        })
    }

    fn candidate(&self, glyph: &Glyph) -> Result<LineCandidate, TextError> {
        let bounds_64 = glyph_bounds(
            glyph.metrics,
            glyph.draw_size_64,
            self.x_64,
            self.line,
            self.pitch_64,
            self.baseline_64,
            self.scale_1024,
        )?;
        let pen_end_64 = self
            .x_64
            .checked_add(glyph.advance_64)
            .ok_or(TextError::FixedPointOverflow)?;
        let min_64 = self.min_64.min(i64::from(bounds_64[0])).min(pen_end_64);
        let ink_right_64 = i64::from(bounds_64[2])
            + if bounds_64[2] > bounds_64[0] && bounds_64[3] > bounds_64[1] {
                glyph.bold_offset_64
            } else {
                0
            };
        let max_64 = self.max_64.max(ink_right_64).max(pen_end_64);
        let width_64 = u64::try_from(max_64 - min_64).map_err(|_| TextError::FixedPointOverflow)?;
        Ok(LineCandidate {
            bounds_64,
            pen_end_64,
            min_64,
            max_64,
            width_64,
        })
    }

    fn push(
        &mut self,
        glyph: Glyph,
        style: TextStyle,
        candidate: LineCandidate,
        source: usize,
    ) -> Result<(), TextError> {
        self.glyphs.push(GlyphQuad {
            codepoint: glyph.codepoint,
            resolved_codepoint: glyph.resolved,
            page: glyph.metrics.page,
            uv: glyph.metrics.uv,
            bounds_64: candidate.bounds_64,
            line: u16::try_from(self.line).map_err(|_| TextError::FixedPointOverflow)?,
            style,
        });
        self.marks.push(Mark {
            source,
            min_64: candidate.min_64,
            max_64: candidate.max_64,
            pen_64: candidate.pen_end_64,
        });
        self.x_64 = candidate.pen_end_64;
        self.min_64 = candidate.min_64;
        self.max_64 = candidate.max_64;
        Ok(())
    }

    /// Keep the first `len` glyphs, restoring the line extent after them.
    fn truncate(&mut self, len: usize) {
        self.glyphs.truncate(len);
        self.marks.truncate(len);
        match self.marks.last().filter(|_| len > self.line_start) {
            Some(mark) => {
                self.min_64 = mark.min_64;
                self.max_64 = mark.max_64;
                self.x_64 = mark.pen_64;
            }
            None => {
                self.min_64 = 0;
                self.max_64 = 0;
                self.x_64 = 0;
            }
        }
    }

    /// Chop the word filling this line so it plus a `-` fits, keeping at least
    /// one glyph; appends the hyphen unless hidden. Returns where to resume.
    fn chop(&mut self, current: usize) -> Result<usize, TextError> {
        let style = |lines: &Self| {
            lines
                .glyphs
                .last()
                .map(|glyph| glyph.style)
                .unwrap_or_default()
        };
        let mut hyphen = self.glyph('-', style(self))?;
        let width_64 = u64::from(self.request.width_64);
        let mut resume = current;
        while self.glyphs.len() - self.line_start >= 2
            && self.candidate(&hyphen)?.width_64 > width_64
        {
            resume = self.marks[self.glyphs.len() - 1].source;
            self.truncate(self.glyphs.len() - 1);
            hyphen = self.glyph('-', style(self))?;
        }
        if self.request.wrap.chop == WordChop::Hyphen {
            let style = style(self);
            let candidate = self.candidate(&hyphen)?;
            self.push(hyphen, style, candidate, usize::MAX)?;
        }
        Ok(resume)
    }

    /// End the line; `false` when the line limit stops the text here, after
    /// ending the last line in `...`.
    fn break_line(&mut self) -> Result<bool, TextError> {
        if self
            .request
            .wrap
            .max_lines
            .is_some_and(|max| self.line + 1 >= usize::from(max))
        {
            self.ellipsize()?;
            return Ok(false);
        }
        self.finish_line()?;
        let next = self
            .line
            .checked_add(1)
            .ok_or(TextError::FixedPointOverflow)?;
        if next + 1 > MAX_WRAP_LINES {
            return Err(TextError::WrapLineLimitExceeded {
                actual: next + 1,
                limit: MAX_WRAP_LINES,
            });
        }
        self.line = next;
        self.line_start = self.glyphs.len();
        self.min_64 = 0;
        self.max_64 = 0;
        self.x_64 = 0;
        Ok(true)
    }

    /// Vanilla removes the saved newline, then visible glyphs only until `...` fits.
    fn ellipsize(&mut self) -> Result<(), TextError> {
        self.ellipsized = true;
        let width_64 = u64::from(self.request.width_64);
        let style = self
            .glyphs
            .last()
            .map(|glyph| glyph.style)
            .unwrap_or_default();
        loop {
            let kept = self.glyphs.len();
            let mut fits = true;
            for _ in 0..3 {
                let dot = self.glyph('.', style)?;
                let candidate = self.candidate(&dot)?;
                fits &= candidate.width_64 <= width_64;
                self.push(dot, style, candidate, usize::MAX)?;
            }
            if fits || kept == self.line_start {
                return Ok(());
            }
            self.truncate(kept - 1);
        }
    }

    /// Normalise the line's glyphs to start at zero and record its width.
    fn finish_line(&mut self) -> Result<(), TextError> {
        let shift_64 = -self.min_64;
        for glyph in &mut self.glyphs[self.line_start..] {
            glyph.bounds_64[0] = checked_i32(i64::from(glyph.bounds_64[0]) + shift_64)?;
            glyph.bounds_64[2] = checked_i32(i64::from(glyph.bounds_64[2]) + shift_64)?;
        }
        self.widths.push(self.max_64 - self.min_64);
        Ok(())
    }

    fn into_layout(mut self, id: u64, key: TextLayoutKey) -> Result<TextLayout, TextError> {
        let line_count = self.widths.len();
        let factor = match self.request.wrap.align {
            TextLineAlign::Left => 0,
            TextLineAlign::Center => 1,
            TextLineAlign::Right => 2,
        };
        let mut maximum_width_64 = 0i64;
        let offsets: Vec<i64> = self
            .widths
            .iter()
            .map(|width| {
                let offset = if factor == 0 {
                    0
                } else {
                    (i64::from(self.request.width_64) - width).max(0) * factor / 2
                };
                maximum_width_64 = maximum_width_64.max(offset + width);
                offset
            })
            .collect();
        for glyph in &mut self.glyphs {
            let offset = offsets[usize::from(glyph.line)];
            glyph.bounds_64[0] = checked_i32(i64::from(glyph.bounds_64[0]) + offset)?;
            glyph.bounds_64[2] = checked_i32(i64::from(glyph.bounds_64[2]) + offset)?;
        }
        let count = i64::try_from(line_count).map_err(|_| TextError::FixedPointOverflow)?;
        let nominal_height_64 =
            count * self.line_height_64 + (count - 1) * (self.pitch_64 - self.line_height_64);
        let height_64 = normalize_vertical_bounds(&mut self.glyphs, nominal_height_64)?;
        Ok(TextLayout {
            id,
            key,
            glyphs: self.glyphs.into_boxed_slice(),
            source_indices: self
                .marks
                .into_iter()
                .map(|mark| (mark.source != usize::MAX).then_some(mark.source))
                .collect(),
            line_count: u16::try_from(line_count).map_err(|_| TextError::FixedPointOverflow)?,
            size_64: [checked_u32(maximum_width_64)?, checked_u32(height_64)?],
            ellipsized: self.ellipsized,
            linear_sampling: self.request.font.linear_sampling(),
        })
    }
}

fn normalize_vertical_bounds(
    glyphs: &mut [GlyphQuad],
    nominal_height_64: i64,
) -> Result<i64, TextError> {
    let min_64 = glyphs
        .iter()
        .map(|glyph| i64::from(glyph.bounds_64[1]))
        .fold(0, i64::min);
    let max_64 = glyphs
        .iter()
        .map(|glyph| i64::from(glyph.bounds_64[3]))
        .fold(nominal_height_64, i64::max);
    let shift_64 = -min_64;
    for glyph in glyphs {
        glyph.bounds_64[1] = checked_i32(i64::from(glyph.bounds_64[1]) + shift_64)?;
        glyph.bounds_64[3] = checked_i32(i64::from(glyph.bounds_64[3]) + shift_64)?;
    }
    Ok(max_64 - min_64)
}

fn resolve_glyph(
    font: &CompiledFontCatalog,
    codepoint: char,
) -> Result<(char, GlyphMetrics), TextError> {
    if let Some(metrics) = font.glyph(codepoint) {
        return Ok((codepoint, *metrics));
    }
    font.glyph(REPLACEMENT_CODEPOINT)
        .copied()
        .map(|metrics| (REPLACEMENT_CODEPOINT, metrics))
        .ok_or(TextError::MissingReplacementGlyph)
}

fn glyph_bounds(
    metrics: GlyphMetrics,
    draw_size_64: Option<[u32; 2]>,
    x_64: i64,
    line: usize,
    pitch_64: i64,
    baseline_64: i64,
    scale_1024: i64,
) -> Result<[i32; 4], TextError> {
    let bearing = |value: i16| {
        i64::from(value)
            .checked_mul(FIXED_POINT_DENOMINATOR)
            .ok_or(TextError::FixedPointOverflow)
            .and_then(|value| scale_metric(value, scale_1024))
    };
    let bearing_x_64 = bearing(metrics.bearing[0])?;
    let bearing_y_64 = bearing(metrics.bearing[1])?;
    let [texel_width_64, texel_height_64] = draw_size_64.unwrap_or([
        u32::from(metrics.uv[2].saturating_sub(metrics.uv[0])) * FIXED_POINT_DENOMINATOR as u32,
        u32::from(metrics.uv[3].saturating_sub(metrics.uv[1])) * FIXED_POINT_DENOMINATOR as u32,
    ]);
    let width_64 = scale_metric(i64::from(texel_width_64), scale_1024)?;
    let height_64 = scale_metric(i64::from(texel_height_64), scale_1024)?;
    let line_y_64 = i64::try_from(line)
        .ok()
        .and_then(|line| line.checked_mul(pitch_64))
        .ok_or(TextError::FixedPointOverflow)?;
    let left = x_64
        .checked_add(bearing_x_64)
        .ok_or(TextError::FixedPointOverflow)?;
    // Bearings point up from the baseline, so the baseline offset is what puts
    // the glyph inside the line box rather than above its origin.
    let top = line_y_64
        .checked_add(baseline_64)
        .and_then(|top| top.checked_add(bearing_y_64))
        .ok_or(TextError::FixedPointOverflow)?;
    Ok([
        checked_i32(left)?,
        checked_i32(top)?,
        checked_i32(left + width_64)?,
        checked_i32(top + height_64)?,
    ])
}

fn scale_metric(value: i64, scale_1024: i64) -> Result<i64, TextError> {
    value
        .checked_mul(scale_1024)
        .and_then(|scaled| scaled.checked_div(crate::UiScale::SCALE_DENOMINATOR))
        .ok_or(TextError::FixedPointOverflow)
}

fn checked_i32(value: i64) -> Result<i32, TextError> {
    i32::try_from(value).map_err(|_| TextError::FixedPointOverflow)
}

fn checked_u32(value: i64) -> Result<u32, TextError> {
    u32::try_from(value).map_err(|_| TextError::FixedPointOverflow)
}
