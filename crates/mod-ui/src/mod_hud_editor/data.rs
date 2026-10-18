//! Changing editor positions, feedback and contextual widget captions.

use super::*;

impl HudEditor {
    /// Binds changing positions and selection without regenerating the catalog.
    pub(super) fn data(&self) -> DataSource {
        let mut data = cards::data(&self.draft, self.viewport);
        data.set_global("#editor_label", Scalar::Text(self.editor_label().into()));
        data.set_global("#grid_visible", Scalar::Bool(self.snap));
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
        if let Some(surface) = &self.draft.surface {
            crate::mod_panel::surface::bind(surface, &mut data);
            crate::mod_panel::surface::viewport(&mut data, self.viewport);
        }
        data.set_global(
            "#grid_label",
            Scalar::Text(if self.snap { "Grid: ON" } else { "Grid: OFF" }.into()),
        );
        write_bounds(&mut data, &self.draft, self.viewport, self.selected);
        data
    }
    /// Names the hovered widget or current selection without placing labels over nearby cards.
    fn editor_label(&self) -> &str {
        let hovered = self.frame.as_ref().and_then(|frame| {
            let key = self.view.hovered.as_ref()?;
            let action = frame
                .hits
                .iter()
                .find(|hit| &hit.key == key)?
                .pressed
                .as_deref()?;
            action
                .strip_prefix("hud.card:")
                .and_then(|index| index.parse().ok())
                .or_else(|| {
                    action
                        .strip_prefix("hud.resize:")?
                        .split_once(':')?
                        .0
                        .parse()
                        .ok()
                })
        });
        hovered
            .or(self.selected)
            .and_then(|index| self.draft.cards.get(index))
            .map(|card| {
                if card.editor_label.is_empty() {
                    card.id.as_str()
                } else {
                    card.editor_label.as_str()
                }
            })
            .unwrap_or("Select a widget")
    }
}

/// Refreshes labels after native font measurement without replacing icon or transition bindings.
pub(super) fn write_bounds(
    data: &mut DataSource,
    hud: &Hud,
    viewport: [f64; 2],
    selected: Option<usize>,
) {
    let style = hud.editor_style.unwrap_or_default();
    for (index, card) in hud.cards.iter().enumerate() {
        let at = cards::origin(card, viewport);
        let size = cards::dimensions(card);
        let label_y = if at[1] >= 12. {
            at[1] - 12.
        } else {
            at[1] + size[1] + 2.
        };
        let label_y = label_y.clamp(0., (viewport[1] - 12.).max(0.));
        data.set_global(
            format!("#bounds_{index}_label_offset"),
            Scalar::Json(serde_json::json!([at[0], label_y])),
        );
        data.set_global(
            format!("#bounds_{index}_color"),
            Scalar::Json(serde_json::json!(if selected == Some(index) {
                style.selected
            } else {
                style.outline
            })),
        );
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
        let catalog = template::catalog(editor, [400., 250.]).unwrap();
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
