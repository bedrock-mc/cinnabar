use super::*;
use crate::global_resources::Action as PackAction;
use crate::menu::MenuView;
use crate::ui_runtime::presentation::forms::oreui::review_tests::paint;
use crate::ui_runtime::presentation::forms::oreui::theme;
use std::collections::HashMap;

#[test]
fn empty_pack_library_still_renders_the_always_active_vanilla_base() {
    let view = MenuView::new(true, "Player".into());
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        resources(&mut content).unwrap();
    });
    let labels: Vec<String> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    assert!(labels.iter().any(|label| label == "Vanilla Textures"));
    assert!(labels.iter().any(|label| label.contains("Always active")));
    assert!(!hits.iter().any(|(action, _)| matches!(
        action,
        MenuAction::GlobalResources(
            PackAction::Deactivate(_) | PackAction::MoveUp(_) | PackAction::MoveDown(_)
        )
    )));
}

#[test]
fn checked_language_radio_has_the_primary_diamond_face() {
    let mut view = MenuView::new(true, "Player".into());
    view.language_choices = vec![("en_US".into(), "English".into())].into();
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        language(&mut content).unwrap();
    });
    assert!(nodes.iter().any(|node| match node.visual() {
        ui::UiVisual::Solid { color, .. } | ui::UiVisual::RotatedSprite { color, .. } =>
            *color == theme::PRIMARY_ROLE.fill,
        _ => false,
    }));
}
