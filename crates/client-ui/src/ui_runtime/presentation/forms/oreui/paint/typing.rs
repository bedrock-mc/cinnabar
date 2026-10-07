use super::super::{motion::opacity, transitions::Insertion};
use super::{Bounds, Canvas, Rgba, Type, UiNode, UiNodeId, UiPresentationError};
use std::sync::Arc;

#[cfg(test)]
mod tests;

impl Canvas<'_> {
    /// Shares the final line layout while only new glyphs briefly fade and rise into place.
    pub(in super::super) fn text_line_typing(
        &mut self,
        value: &str,
        bounds: Bounds,
        style: Type,
        color: Rgba,
        insertion: Option<Insertion>,
    ) -> Result<(), UiPresentationError> {
        let width = bounds[2] - bounds[0];
        let Some(layout) = self.line_layout(value, width, style)? else {
            return Ok(());
        };
        let ink = layout
            .glyphs()
            .iter()
            .filter(|g| !g.codepoint.is_whitespace())
            .fold([f32::INFINITY, f32::NEG_INFINITY], |[top, bottom], g| {
                [
                    top.min(g.bounds_64[1] as f32 / 64.0),
                    bottom.max(g.bounds_64[3] as f32 / 64.0),
                ]
            });
        if !ink[0].is_finite() {
            return Ok(());
        }
        let y = (bounds[1] + bounds[3] - ink[0] - ink[1]) * 0.5;
        let insertion = insertion.filter(|i| {
            i.progress < 1.0
                && value
                    .chars()
                    .eq(layout.glyphs().iter().map(|g| g.codepoint))
        });
        let Some(insertion) = insertion else {
            return self
                .place_text(layout, [bounds[0], y], width + 1.0, color, false)
                .map(|_| ());
        };
        let Some(glyphs) = layout.glyphs().get(insertion.characters) else {
            return self
                .place_text(layout, [bounds[0], y], width + 1.0, color, false)
                .map(|_| ());
        };
        let span = glyphs
            .iter()
            .fold([f32::INFINITY, f32::NEG_INFINITY], |[left, right], g| {
                [
                    left.min(g.bounds_64[0] as f32 / 64.0),
                    right.max(g.bounds_64[2] as f32 / 64.0),
                ]
            });
        let start = (bounds[0] + span[0]).clamp(bounds[0], bounds[2]);
        let end = (bounds[0] + span[1]).clamp(start, bounds[2]);
        self.clipped_line(
            [bounds[0], bounds[1], start, bounds[3]],
            layout.clone(),
            [bounds[0], y],
            width,
            color,
        )?;
        self.clipped_line(
            [start, bounds[1], end, bounds[3]],
            layout.clone(),
            [bounds[0], y + self.r(0.12) * (1.0 - insertion.progress)],
            width,
            opacity(color, 0.55 + 0.45 * insertion.progress),
        )?;
        self.clipped_line(
            [end, bounds[1], bounds[2], bounds[3]],
            layout,
            [bounds[0], y],
            width,
            color,
        )
    }

    fn clipped_line(
        &mut self,
        mut bounds: Bounds,
        layout: Arc<ui::TextLayout>,
        at: [f32; 2],
        width: f32,
        color: Rgba,
    ) -> Result<(), UiPresentationError> {
        if let Some((_, clip)) = self.clip {
            bounds = [
                bounds[0].max(clip[0]),
                bounds[1].max(clip[1]),
                bounds[2].min(clip[2]),
                bounds[3].min(clip[3]),
            ];
        }
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
            return Ok(());
        }
        let id = UiNodeId::new(*self.next);
        self.nodes.push(
            UiNode::new(id, self.clip.map(|(id, _)| id), self.local(bounds)?)
                .with_clip_children(true),
        );
        *self.next = self.next.saturating_add(1);
        let previous = self.clip.replace((id, bounds));
        let result = self.place_text(layout, at, width + 1.0, color, false);
        self.clip = previous;
        result.map(|_| ())
    }
}
