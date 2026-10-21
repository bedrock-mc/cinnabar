//! Changing editor positions, feedback and contextual widget captions.

use super::*;

impl HudEditor {
    /// Binds changing values without regenerating the catalog.
    pub(super) fn data(&mut self) -> DataSource {
        let mut data = cards::data(&self.draft, self.viewport);
        if let Some(surface) = &self.draft.surface {
            crate::mod_panel::surface::bind(surface, &mut data);
            crate::mod_panel::surface::viewport(&mut data, self.viewport);
        }
        data.set_global("#editor_label", Scalar::Text(self.editor_label().into()));
        data.set_global("#grid_visible", Scalar::Bool(self.grid));
        data.set_global(
            "#grid_label",
            Scalar::Text(if self.grid { "Grid: ON" } else { "Grid: OFF" }.into()),
        );
        data.set_global(
            "#snap_label",
            Scalar::Text(if self.snap { "Snap: ON" } else { "Snap: OFF" }.into()),
        );
        for (name, value) in [
            ("#hud_grid_on", self.grid),
            ("#hud_snap_on", self.snap),
            ("#hud_can_undo", self.history.available()[0]),
            ("#hud_can_redo", self.history.available()[1]),
        ] {
            data.set_global(name, Scalar::Bool(value));
        }
        data.set_global("#hud_toast", Scalar::Text(self.toast.text.clone()));
        data.set_global("#hud_viewport_w", Scalar::Num(self.viewport[0]));
        data.set_global("#hud_viewport_h", Scalar::Num(self.viewport[1]));
        for &(index, fill) in &self.control_fills {
            data.set_global(format!("#row_{index}_fill"), Scalar::Num(fill));
        }
        self.write_geometry(&mut data);
        data
    }

    /// Shares displayed geometry and hidden-card filtering between paint and text measurement.
    pub(super) fn write_geometry(&mut self, data: &mut DataSource) {
        data.set_global("#hud_dragging", Scalar::Bool(self.drag.is_some()));
        for axis in 0..2 {
            let guide = self.drag.as_ref().and_then(|drag| drag.guides[axis]);
            data.set_global(
                format!("#guide_{axis}_visible"),
                Scalar::Bool(guide.is_some()),
            );
            let mut at = [0.; 2];
            at[axis] = guide.unwrap_or(0.);
            data.set_global(
                format!("#guide_{axis}_offset"),
                Scalar::Json(serde_json::json!(at)),
            );
        }
        let style = self.draft.editor_style.unwrap_or_default();
        let hovered = self.hovered_card();
        for (index, card) in self.draft.cards.iter().enumerate() {
            let visible = !self.hidden.contains(&card.id);
            data.set_global(format!("#card_{index}_visible"), Scalar::Bool(visible));
            data.set_global(format!("#hud_card_{index}_visible"), Scalar::Bool(visible));
            data.set_global(
                format!("#hud_card_{index}_name"),
                Scalar::Text(card.editor_name().to_owned()),
            );
            let at = self.card_origin(index);
            data.set_global(
                format!("#card_{index}_offset"),
                Scalar::Json(serde_json::json!(at)),
            );
            let size = cards::dimensions(card);
            let y = if at[1] >= 12. {
                at[1] - 12.
            } else {
                at[1] + size[1] + 2.
            };
            data.set_global(
                format!("#bounds_{index}_label_offset"),
                Scalar::Json(serde_json::json!([
                    at[0],
                    y.clamp(0., (self.viewport[1] - 12.).max(0.))
                ])),
            );
            data.set_global(
                format!("#bounds_{index}_color"),
                Scalar::Json(serde_json::json!(if self.selected == Some(index) {
                    style.selected
                } else if hovered == Some(index) {
                    style.outline
                } else {
                    [0_f32; 4]
                })),
            );
        }
        if let Some(drag) = &self.drag {
            for index in drag.targets.into_iter().flatten() {
                data.set_global(
                    format!("#bounds_{index}_color"),
                    Scalar::Json(serde_json::json!(style.guide)),
                );
            }
        }
        self.write_selection(data);
        let rect = self
            .selected
            .filter(|index| !self.hidden.contains(&self.draft.cards[*index].id))
            .map(|index| {
                (
                    self.card_origin(index),
                    cards::dimensions(&self.draft.cards[index]),
                )
            });
        feedback::measurements(data, self.viewport, rect);
    }

    /// Publishes inspector values from the same displayed rectangle used by pointer bounds.
    fn write_selection(&mut self, data: &mut DataSource) {
        let selected = self
            .selected
            .filter(|index| *index < self.draft.cards.len());
        let card = selected.map(|index| &self.draft.cards[index]);
        data.set_global(
            "#hud_selected",
            Scalar::Num(selected.map_or(-1., |i| i as f64)),
        );
        let at = selected.map_or([0.; 2], |index| self.card_origin(index));
        let size = card.map_or([0.; 2], cards::dimensions);
        for (name, value) in [
            ("#hud_selected_x", at[0]),
            ("#hud_selected_y", at[1]),
            ("#hud_selected_w", size[0]),
            ("#hud_selected_h", size[1]),
        ] {
            data.set_global(name, Scalar::Num(value));
        }
        let anchor = card
            .filter(|card| card.position.is_none())
            .and_then(|card| {
                ui::mod_hud::Anchor::ALL
                    .iter()
                    .position(|anchor| *anchor == card.anchor)
            });
        data.set_global(
            "#hud_selected_anchor",
            Scalar::Num(anchor.map_or(-1., |i| i as f64)),
        );
        data.set_global(
            "#hud_selected_scale_text",
            Scalar::Text(
                card.map_or_else(String::new, |card| format!("{:.0}%", card.scale * 100.)),
            ),
        );
        data.set_global(
            "#hud_selected_scale_fill",
            Scalar::Num(card.map_or(0., |card| slider::scale_fill(card.scale))),
        );
        let [x, y, w, h] = [at[0], at[1], size[0], size[1]].map(|value| value.round() as i64);
        let (size_text, position_text) = if card.is_some() {
            (format!("{w}\u{d7}{h}"), format!("x {x} \u{b7} y {y}"))
        } else {
            Default::default()
        };
        data.set_global(
            "#hud_selected_name",
            Scalar::Text(card.map_or_else(String::new, |card| card.editor_name().to_owned())),
        );
        data.set_global("#hud_selected_size_text", Scalar::Text(size_text));
        data.set_global("#hud_selected_position_text", Scalar::Text(position_text));
        // Moving the inspector mid-gesture would move the slider under the pointer.
        if !self.scaling() {
            self.inspector.update(
                selected.map(|_| [at[0], at[1], size[0], size[1]]),
                self.viewport,
                self.draft.inspector_size,
            );
        }
        for (name, value) in [
            ("#hud_inspector_x", self.inspector.at[0]),
            ("#hud_inspector_y", self.inspector.at[1]),
        ] {
            data.set_global(name, Scalar::Num(value));
        }
    }

    /// Hovered card index, independent of the retained frame so rebuilds keep the outline.
    fn hovered_card(&self) -> Option<usize> {
        self.hovered.filter(|index| *index < self.draft.cards.len())
    }

    /// Names the hovered widget or current selection without placing labels over nearby cards.
    fn editor_label(&self) -> &str {
        self.hovered_card()
            .or(self.selected)
            .and_then(|index| self.draft.cards.get(index))
            .map(ui::mod_hud::Card::editor_name)
            .unwrap_or("Select a widget")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads emitted label text through the real binding pipeline without depending on geometry.
    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(serde_json::Value::as_str)
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    /// Default chrome names one selection, leaving crowded widget bounds free of labels.
    #[test]
    fn contextual_caption_follows_selection_and_uses_an_identity_fallback() {
        let mut ui = ModUi::default();
        let panel = crate::test_support::sample_panel();
        ui.set_mod_panel(crate::PanelUiOwner::Environment(0), Some(&panel))
            .unwrap();
        ui.set_mod_panel_open(true);
        let preview = Hud {
            cards: vec![ui::mod_hud::Card {
                id: "fps".into(),
                editor_label: "Frame rate".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        ui.open_mod_hud_editor(&preview).unwrap();
        let editor = ui.mod_hud_editor.as_mut().unwrap();
        assert_eq!(editor.editor_label(), "Select a widget");
        editor.selected = Some(0);
        assert_eq!(editor.editor_label(), "Frame rate");
        let catalog = template::catalog(&editor.draft, [400., 250.]).unwrap();
        let root = json_ui::resolve(&catalog, SCREEN, &Context::default())
            .control
            .unwrap();
        let bound = json_ui::bind(&root, &editor.data(), &json_ui::EmptyLibrary);
        assert!(
            has_text(&bound, "Frame rate"),
            "selection caption must reach the painted text"
        );
        editor.draft.cards[0].editor_label.clear();
        assert_eq!(editor.editor_label(), "fps");
        editor.selected = Some(1);
        assert_eq!(editor.editor_label(), "Select a widget");
    }
}
