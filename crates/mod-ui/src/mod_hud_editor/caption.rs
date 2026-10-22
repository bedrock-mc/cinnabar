//! Measured selection captions share placement obstacles with native inspector and guides.

use super::*;

pub(super) const CAPTION_HEIGHT: f64 = 14.;

impl HudEditor {
    /// Measures the selected name and scale with the same host face used to paint them.
    pub(super) fn measure_caption(&mut self, text: &dyn json_ui::TextMeasure) -> bool {
        let style = self.draft.editor_style.unwrap_or_default();
        let Some(card) = self
            .selected
            .and_then(|index| self.draft.cards.get(index))
            .filter(|card| style.selection_caption && !self.hidden.contains(&card.id))
        else {
            return false;
        };
        let shape = json_ui::LabelShape {
            scale: f64::from(style.text_scale),
            line_padding: 0.,
            hide_hyphen: true,
        };
        let scale = format!("{:.0}%", card.scale * 100.);
        let extents = [card.editor_name(), scale.as_str()].map(|word| {
            let width = text.named_label(word, style.font.name(), None, None, shape)[0];
            if width.is_finite() { width.max(0.) } else { 0. }
        });
        let width = (extents[0] + extents[1] + 16.).clamp(64., 104.);
        let scale_width = extents[1].min(width - 12.);
        if self.caption_width == width && self.caption_scale_width == scale_width {
            return false;
        }
        self.caption_width = width;
        self.caption_scale_width = scale_width;
        true
    }

    /// Places the caption near its widget, reserving its coverage before the inspector is placed.
    pub(super) fn write_caption(
        &mut self,
        data: &mut DataSource,
        selection: Option<[f64; 4]>,
    ) -> Option<[f64; 4]> {
        self.caption_bounds = selection
            .filter(|_| {
                self.draft
                    .editor_style
                    .unwrap_or_default()
                    .selection_caption
            })
            .map(|[x, y, _, h]| {
                let width = self.caption_width.min(self.viewport[0]);
                let clearance =
                    f64::from(self.draft.editor_style.unwrap_or_default().top_clearance);
                let top = if y >= clearance + CAPTION_HEIGHT + 4. {
                    y - CAPTION_HEIGHT - 4.
                } else {
                    (y + h + 4.).max(clearance)
                };
                let caption = [
                    x.clamp(0., (self.viewport[0] - width).max(0.)),
                    top.clamp(0., (self.viewport[1] - CAPTION_HEIGHT).max(0.)),
                    width,
                    CAPTION_HEIGHT,
                ];
                data.set_global(
                    "#hud_caption_offset",
                    Scalar::Json(serde_json::json!([caption[0], caption[1]])),
                );
                data.set_global("#hud_caption_width", Scalar::Num(width));
                data.set_global(
                    "#hud_caption_name_width",
                    Scalar::Num((width - self.caption_scale_width - 12.).max(0.)),
                );
                data.set_global(
                    "#hud_caption_scale_offset",
                    Scalar::Json(serde_json::json!([
                        width - self.caption_scale_width - 5.,
                        0.
                    ])),
                );
                data.set_global(
                    "#hud_caption_scale_width",
                    Scalar::Num(self.caption_scale_width),
                );
                caption
            });
        data.set_global(
            "#hud_caption_visible",
            Scalar::Bool(self.caption_bounds.is_some()),
        );
        let name = self
            .selected
            .and_then(|index| self.draft.cards.get(index))
            .map_or("", ui::mod_hud::Card::editor_name);
        data.set_global("#hud_caption_name", Scalar::Text(name.into()));
        selection.map(|rect| {
            self.caption_bounds.map_or(rect, |caption| {
                let x = rect[0].min(caption[0]);
                let y = rect[1].min(caption[1]);
                [
                    x,
                    y,
                    (rect[0] + rect[2]).max(caption[0] + caption[2]) - x,
                    (rect[1] + rect[3]).max(caption[1] + caption[3]) - y,
                ]
            })
        })
    }
}
