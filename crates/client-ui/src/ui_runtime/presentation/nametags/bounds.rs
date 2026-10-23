//! Presentation bounds keep off-screen labels out of the shared resource budgets.

use super::super::nametag_atlas::{LineMetrics, line_metrics};
use super::*;
use glam::{Mat4, Quat};

#[cfg(test)]
mod tests;

/// Retains local bounds independently of the shared glyph-layout LRU.
#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct BoundsCache {
    entries: HashMap<u64, CachedBounds>,
    font: Option<assets::FontCatalogIdentity>,
    style: Option<ui::mod_hud::NametagStyle>,
}

struct CachedBounds {
    lines: Arc<[Arc<str>]>,
    badge: bool,
    counter: Option<Arc<ui::mod_hud::CounterAnnotation>>,
    measured: Option<MeasuredBounds>,
    seen: bool,
}

#[derive(Clone, Copy)]
struct MeasuredBounds {
    rect: [f32; 4],
    scale: f32,
    lift: f32,
}

impl BoundsCache {
    /// Starts candidate selection without discarding unchanged measurements.
    pub(in crate::ui_runtime::presentation) fn begin_frame(&mut self) {
        for entry in self.entries.values_mut() {
            entry.seen = false;
        }
    }

    /// Releases departed candidates and keeps spare buckets proportional to the live set.
    pub(in crate::ui_runtime::presentation) fn finish_frame(&mut self) {
        self.entries.retain(|_, entry| entry.seen);
        if self.entries.capacity() > self.entries.len().saturating_mul(4) {
            self.entries.shrink_to(self.entries.len().saturating_mul(2));
        }
    }

    /// Projects cached local bounds using the current camera and actor pose.
    pub(in crate::ui_runtime::presentation) fn intersects_view(
        &mut self,
        anchor: &NametagAnchor,
        clip_from_world: Mat4,
        rotation: Quat,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        style: Option<&ui::mod_hud::NametagStyle>,
    ) -> bool {
        if self.font != Some(font.identity()) || self.style.as_ref() != style {
            self.entries.clear();
            self.font = Some(font.identity());
            self.style = style.cloned();
        }
        if style.is_some_and(|style| style.hide) || anchor.lines.is_empty() {
            return false;
        }
        let cached = self.entries.get_mut(&anchor.runtime_id).filter(|cached| {
            cached.lines == anchor.lines
                && cached.badge == anchor.cinnabar_badge
                && cached.counter == anchor.counter
        });
        // The first plate contains the ordinary label's unlifted anchor. An on-screen
        // anchor therefore intersects the conservative hull without measuring its text.
        if !anchor.sneaking
            && render_model::visibility_bounds::points_intersect_view(
                clip_from_world,
                [anchor.position],
            )
        {
            if let Some(cached) = cached {
                cached.seen = true;
            }
            return true;
        }
        let measured = if let Some(cached) = cached {
            cached.seen = true;
            cached.measured
        } else {
            let measured = measure_bounds(anchor, font, layouts, style);
            self.entries.insert(
                anchor.runtime_id,
                CachedBounds {
                    lines: Arc::clone(&anchor.lines),
                    badge: anchor.cinnabar_badge,
                    counter: anchor.counter.clone(),
                    measured,
                    seen: true,
                },
            );
            measured
        };
        measured.is_none_or(|bounds| bounds.intersects_view(anchor, clip_from_world, rotation))
    }
}

/// Measures local ink, plates and extensions under every admitted line prefix.
fn measure_bounds(
    anchor: &NametagAnchor,
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
    style: Option<&ui::mod_hud::NametagStyle>,
) -> Option<MeasuredBounds> {
    let name = line_metrics(&anchor.lines[0], font, layouts)?;
    let mut runs: [Option<LineMetrics>; 2] = [None, None];
    let mut counter_width = 0.0;
    let mut gap = 0.0;
    if let Some(counter) = &anchor.counter {
        for (index, text) in [&counter.prefix, &counter.value].into_iter().enumerate() {
            if index == 0 && text.is_empty() {
                continue;
            }
            let metrics = line_metrics(text, font, layouts)?;
            counter_width += metrics.width_px;
            runs[index] = Some(metrics);
        }
        let space = line_metrics(" ", font, layouts)?;
        gap = space.width_px;
    }
    let separate = anchor
        .counter
        .as_ref()
        .is_some_and(|counter| counter.separate);
    let before = anchor
        .counter
        .as_ref()
        .is_some_and(|counter| counter.before);
    let inline_width = if anchor.counter.is_some() && !separate {
        counter_width + gap
    } else {
        0.0
    };
    let badge_size = if anchor.cinnabar_badge {
        (name.ink_y_px[1] - name.ink_y_px[0]).max(0.0)
    } else {
        0.0
    };
    let badge_width = if badge_size > 0.0 {
        badge_size + 1.0
    } else {
        0.0
    };
    let first_width = name.width_px + badge_width + inline_width;
    let name_start = usize::from(separate && before);
    let mut bounds = Bounds { rect: None, style };
    for (index, text) in anchor.lines.iter().enumerate() {
        let metrics = line_metrics(text, font, layouts)?;
        let width = if index == 0 {
            first_width
        } else {
            metrics.width_px
        };
        let left = nametag_line_left(width)
            + if index == 0 {
                badge_width + if before { inline_width } else { 0.0 }
            } else {
                0.0
            };
        let top = (index + name_start) as f32 * NAMETAG_LINE_PITCH_PX;
        bounds.include(metrics.rect, [left, top], true);
        bounds.plate(width, top);
    }
    if let Some(counter) = &anchor.counter {
        let (mut left, index) = if separate {
            (
                nametag_line_left(counter_width),
                if before { 0 } else { anchor.lines.len() },
            )
        } else {
            (
                nametag_line_left(first_width)
                    + if before {
                        0.0
                    } else {
                        badge_width + name.width_px + gap
                    },
                0,
            )
        };
        let top = index as f32 * NAMETAG_LINE_PITCH_PX;
        for run in runs.into_iter().flatten() {
            bounds.include(run.rect, [left, top], true);
            left += run.width_px;
        }
        if counter.separate {
            bounds.plate(counter_width, top);
        }
    }
    if badge_size > 0.0 {
        let left = nametag_line_left(first_width)
            + if before && !separate {
                inline_width
            } else {
                0.0
            };
        bounds.include(
            [0.0, name.ink_y_px[0], badge_size, name.ink_y_px[1]],
            [left, name_start as f32 * NAMETAG_LINE_PITCH_PX],
            true,
        );
    }
    let scale = style.map_or(1.0, |style| style.scale);
    let lift = EXTRA_LINE_LIFT
        * (anchor.lines.len() + usize::from(separate)).saturating_sub(1) as f32
        * scale;
    Some(MeasuredBounds {
        rect: bounds.rect?,
        scale,
        lift,
    })
}

impl MeasuredBounds {
    /// Applies live sneak placement and projects the complete range of possible line lifts.
    fn intersects_view(self, anchor: &NametagAnchor, clip: Mat4, rotation: Quat) -> bool {
        let mut rect = self.rect;
        if anchor.sneaking {
            let drop = SNEAK_LABEL_DROP / render_model::NAMETAG_BLOCKS_PER_FONT_PIXEL * self.scale;
            rect[1] += drop;
            rect[3] += drop;
        }
        let record = NametagRecord {
            anchor: anchor.position.to_array(),
            rect,
            ..Default::default()
        };
        let Some(base) = record.world_corners(rotation) else {
            return true;
        };
        let Some(lifted) = (NametagRecord {
            line_lift: self.lift,
            ..record
        })
        .world_corners(rotation) else {
            return true;
        };
        let points: [Vec3; 8] = std::array::from_fn(|index| {
            Vec3::from_array(if index < 4 {
                base[index]
            } else {
                lifted[index - 4]
            })
        });
        render_model::visibility_bounds::points_intersect_view(clip, points)
    }
}

struct Bounds<'a> {
    rect: Option<[f32; 4]>,
    style: Option<&'a ui::mod_hud::NametagStyle>,
}

impl Bounds<'_> {
    /// Includes the shared plate geometry at a line's original vertical slot.
    fn plate(&mut self, width: f32, top: f32) {
        for rect in java_nametag_line_backgrounds(std::iter::once(width)) {
            self.include(rect, [0.0, top], false);
        }
    }

    /// Includes a scaled quad and every cosmetic extension that can paint outside it.
    fn include(&mut self, rect: [f32; 4], [x, y]: [f32; 2], text: bool) {
        let scale = self.style.map_or(1.0, |style| style.scale);
        let mut rect = [
            (rect[0] + x) * scale,
            (rect[1] + y) * scale,
            (rect[2] + x) * scale,
            (rect[3] + y) * scale,
        ];
        if let Some(style) = self.style {
            if text && style.shadow {
                rect[2] += scale;
                rect[3] += scale;
            } else if !text && style.border_color.is_some() {
                let width = style.border_width * scale;
                rect = [
                    rect[0] - width,
                    rect[1] - width,
                    rect[2] + width,
                    rect[3] + width,
                ];
            }
        }
        self.rect = Some(self.rect.map_or(rect, |old| {
            [
                old[0].min(rect[0]),
                old[1].min(rect[1]),
                old[2].max(rect[2]),
                old[3].max(rect[3]),
            ]
        }));
    }
}
