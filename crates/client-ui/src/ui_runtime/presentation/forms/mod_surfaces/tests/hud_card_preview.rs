//! Shared card artwork uses the same runtime language inputs on every surface.

use std::sync::Arc;

use mod_ui::PanelUiOwner;
use serde_json::json;
use ui::{UiNode, UiVisual, mod_hud::Hud, mod_panel::Surface};
use ui_engine::text_metrics::TextMetrics;

use crate::ui_runtime::UiRuntime;

#[test]
fn hud_card_preview_localizes_shared_artwork_across_gameplay_and_panel_transitions() {
    const OWNER: PanelUiOwner = PanelUiOwner::Environment(0);
    for native_preview in [false, true] {
        let mut presentation = super::mod_widgets::presentation(false);
        let mut runtime = UiRuntime::new(1);
        set_translation(&mut runtime, "0");
        let player = player_state::PlayerState::new(1);
        let hud: Hud = serde_json::from_value(json!({"cards":[{
            "id":"preview","width":80,"background_opacity":0,
            "rows":[{"label":"preview.key","value":"","localized":true}]
        }]}))
        .unwrap();
        presentation.set_mod_hud(Some(&hud)).unwrap();
        presentation
            .mod_ui_mut()
            .set_mod_hud_sources([(OWNER, 1, &hud)].into_iter())
            .unwrap();
        super::mod_widgets::frame(&mut presentation, &runtime, [1280, 720], 1.);
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.).unwrap(), Some(2));
        let mut nodes = Vec::new();
        let mut next = 1;
        presentation.append_mod_widgets(
            &player,
            &runtime,
            &mut nodes,
            &mut next,
            metrics,
            [1280., 720.],
        );
        let gameplay = row_text(&nodes);
        assert_eq!(
            gameplay.glyphs().len(),
            1,
            "gameplay translates the native key"
        );

        let mut panel = mod_ui::test_support::sample_panel();
        panel.controls.clear();
        panel.surface = Some(Surface {
            screen: "preview.screen".into(),
            document: json!({"namespace":"preview","screen":{
                "type":"screen","size":["100%","100%"],"controls":[
                    {"card":{"type":"custom","renderer":"cinnabar_hud_card",
                        "card_id":"preview","size":[160,80]}}
                ]
            }})
            .to_string(),
            bindings: Default::default(),
            previews: Default::default(),
            transitions: Default::default(),
            collections: Default::default(),
        });
        if native_preview {
            panel.surface.as_mut().unwrap().previews.insert(
                "native".into(),
                ui::mod_panel::preview::ControlPreview {
                    reference: "hud.hud_screen".into(),
                    size: [100., 30.],
                    ..Default::default()
                },
            );
        }
        presentation
            .mod_ui_mut()
            .set_mod_panel(OWNER, Some(&panel))
            .unwrap();
        presentation.mod_ui_mut().set_mod_panel_open(true);
        nodes.clear();
        next = 1;
        presentation.append_mod_panel(
            &player,
            &runtime,
            &mut nodes,
            &mut next,
            metrics,
            [1280., 720.],
        );
        assert!(
            contains_layout(&nodes, &gameplay),
            "panel shares translated artwork"
        );

        set_translation(&mut runtime, "00");
        nodes.clear();
        next = 1;
        presentation.append_mod_panel(
            &player,
            &runtime,
            &mut nodes,
            &mut next,
            metrics,
            [1280., 720.],
        );
        let updated = row_text(&nodes);
        assert_eq!(
            updated.glyphs().len(),
            2,
            "language replacement updates the card"
        );
        presentation.mod_ui_mut().set_mod_panel_open(false);
        nodes.clear();
        next = 1;
        presentation.append_mod_widgets(
            &player,
            &runtime,
            &mut nodes,
            &mut next,
            metrics,
            [1280., 720.],
        );
        assert!(
            contains_layout(&nodes, &updated),
            "gameplay shares the updated artwork"
        );
    }
}

/// Replaces the runtime's language table so its normal generation key also changes.
fn set_translation(runtime: &mut UiRuntime, value: &str) {
    let bytes = assets::encode_lang_catalog(
        [9; 32],
        [10; 32],
        &[assets::LangEntry {
            key: "preview.key".into(),
            value: value.into(),
        }],
    )
    .unwrap();
    runtime.set_lang_catalog(Arc::new(
        assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
    ));
}

/// Reads the fixture's sole nonempty text run without depending on its layout tree.
fn row_text(nodes: &[UiNode]) -> Arc<ui::TextLayout> {
    let layouts: Vec<_> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } if !layout.glyphs().is_empty() => {
                Some(Arc::clone(layout))
            }
            _ => None,
        })
        .collect();
    assert_eq!(layouts.len(), 1, "one visible card label");
    Arc::clone(&layouts[0])
}

/// Checks that replay keeps the shared prepared text rather than shaping it again.
fn contains_layout(nodes: &[UiNode], expected: &Arc<ui::TextLayout>) -> bool {
    nodes.iter().any(|node| {
        matches!(node.visual(), UiVisual::Text { layout, .. } if Arc::ptr_eq(layout, expected))
    })
}
