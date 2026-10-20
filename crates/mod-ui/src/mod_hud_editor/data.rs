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
        write_selection(&mut data, &self.draft, self.viewport, self.selected);
        for (name, value) in [
            ("#hud_can_undo", self.history.available()[0]),
            ("#hud_can_redo", self.history.available()[1]),
            ("#hud_dragging", self.drag.is_some()),
        ] {
            data.set_global(name, Scalar::Bool(value));
        }
        data.set_global("#hud_toast", Scalar::Text(self.toast.text.clone()));
        for (index, card) in self.draft.cards.iter().enumerate() {
            let visible = !self.hidden.contains(&card.id);
            data.set_global(format!("#hud_card_{index}_visible"), Scalar::Bool(visible));
            data.set_global(
                format!("#hud_card_{index}_name"),
                Scalar::Text(
                    if card.editor_label.is_empty() {
                        &card.id
                    } else {
                        &card.editor_label
                    }
                    .clone(),
                ),
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
        }
        if let Some(index) = self.selected {
            let at = self.card_origin(index);
            data.set_global("#hud_selected_x", Scalar::Num(at[0]));
            data.set_global("#hud_selected_y", Scalar::Num(at[1]));
        }
        if let Some(drag) = &self.drag {
            let color = self.draft.editor_style.unwrap_or_default().guide;
            for index in drag.targets.into_iter().flatten() {
                data.set_global(
                    format!("#bounds_{index}_color"),
                    Scalar::Json(serde_json::json!(color)),
                );
            }
        }
        feedback::measurements(
            &mut data,
            &self.draft,
            self.viewport,
            self.selected
                .filter(|index| !self.hidden.contains(&self.draft.cards[*index].id)),
        );
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

/// Publishes inspector geometry after layout or native text measurement changes.
pub(super) fn write_selection(
    data: &mut DataSource,
    hud: &Hud,
    viewport: [f64; 2],
    selected: Option<usize>,
) {
    let card = selected.and_then(|index| hud.cards.get(index));
    data.set_global(
        "#hud_selected",
        Scalar::Num(
            selected
                .filter(|_| card.is_some())
                .map_or(-1., |i| i as f64),
        ),
    );
    let at = card.map_or([0.; 2], |card| cards::origin(card, viewport));
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
        Scalar::Text(card.map_or_else(String::new, |card| format!("{:.0}%", card.scale * 100.))),
    );
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
