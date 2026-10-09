//! Resolved death reasons retain literal item names through JSON-UI painting.
use super::screen_cache::{ScreenCache, ScreenKey};
use json_ui::{
    Catalog, Context, DataSource, Draw, LayoutEnv, Scalar, TextMeasure, TextureSource, ViewState,
};
use std::sync::Arc;

struct Measure;
impl TextMeasure for Measure {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64, 8.0]
    }
}
impl TextureSource for Measure {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

#[test]
fn death_reason_text_is_painted_without_a_second_translation() {
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/death_screen.json"]}"#.as_slice()),
        ("ui/death_screen.json", br##"{"namespace":"death","death_screen":{"type":"screen","controls":[
            {"reason":{"type":"label","size":[300,20],"text":"#death_reason_text","bindings":[{"binding_name":"#death_reason_text"}]}}
        ]}}"##.as_slice()),
    ]).unwrap();
    let catalog = Arc::new(super::pack_catalog::layer_pack_catalog(&catalog, &[]));
    let (context, view) = (Context::desktop(), ViewState::default());
    let reason = "Player was slain using 100% Power %entity.zombie.name";
    let mut data = DataSource::new();
    data.set_global("#death_reason_text", Scalar::Text(reason.into()));
    let rendered = ScreenCache::default()
        .render(
            ScreenKey {
                reference: "death.death_screen",
                catalog: &catalog,
                context: &context,
                data: &data,
                view: &view,
                root: [400.0, 300.0],
                px: 1.0,
                text: [0; 3],
            },
            &LayoutEnv {
                text: &Measure,
                textures: &Measure,
            },
        )
        .unwrap();
    let labels = rendered
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, localize, .. } => Some((text.as_str(), *localize)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        vec![(reason, false)],
        "formatted parameters must stay literal"
    );
}

#[test]
fn death_reason_variables_and_inherited_labels_remain_literal() {
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/death_screen.json"]}"#.as_slice()),
        ("ui/death_screen.json", br##"{"namespace":"death",
            "reason_base":{"type":"label","size":[300,20],"text":"$reason","bindings":[{"binding_name":"#death_reason_text"}]},
            "death_screen":{"type":"screen","$reason":"#death_reason_text","controls":[
                {"variable":{"type":"label","size":[300,20],"text":"$reason","bindings":[{"binding_name":"#death_reason_text"}]}},
                {"inherited@death.reason_base":{}}
            ]}
        }"##.as_slice()),
    ]).unwrap();
    let catalog = Arc::new(super::pack_catalog::layer_pack_catalog(&catalog, &[]));
    let (context, view) = (Context::desktop(), ViewState::default());
    let reason = "Player was slain using 100% Power %entity.zombie.name";
    let mut data = DataSource::new();
    data.set_global("#death_reason_text", Scalar::Text(reason.into()));
    let rendered = ScreenCache::default()
        .render(
            ScreenKey {
                reference: "death.death_screen",
                catalog: &catalog,
                context: &context,
                data: &data,
                view: &view,
                root: [400.0, 300.0],
                px: 1.0,
                text: [0; 3],
            },
            &LayoutEnv {
                text: &Measure,
                textures: &Measure,
            },
        )
        .unwrap();
    let labels = rendered
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, localize, .. } => Some((text.as_str(), *localize)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(labels, vec![(reason, false), (reason, false)]);
}

#[test]
fn death_reveal_preserves_the_reason_and_loading_retires_the_controls() {
    use crate::menu::{MenuScreen, MenuView};
    let catalog = Arc::new(Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/death_screen.json"]}"#.as_slice()),
        ("ui/death_screen.json", br##"{"namespace":"death","death_screen":{"type":"screen","controls":[
            {"content":{"type":"panel","bindings":[{"binding_name":"#buttons_and_deathmessage_visible","binding_name_override":"#visible"}],"controls":[
                {"reason":{"type":"label","text":"#death_reason_text","localize":false,"size":[300,20],"bindings":[{"binding_name":"#death_reason_text"}]}},
                {"respawn":{"type":"button","size":[100,20],"bindings":[{"binding_name":"#respawn_visible","binding_name_override":"#visible"},{"binding_name":"#respawn_enabled","binding_name_override":"#enabled"}],"controls":[{"label":{"type":"label","text":"Respawn","localize":false,"size":[100,20]}}]}},
                {"quit":{"type":"button","size":[100,20],"bindings":[{"binding_name":"#quit_visible","binding_name_override":"#visible"},{"binding_name":"#quit_enabled","binding_name_override":"#enabled"}],"controls":[{"label":{"type":"label","text":"Main menu","localize":false,"size":[100,20]}}]}}
            ]}},
            {"loading":{"type":"label","text":"Loading","localize":false,"size":[100,20],"bindings":[{"binding_name":"#loading_message_visible","binding_name_override":"#visible"}]}}
        ]}}"##.as_slice()),
    ]).unwrap());
    let mut view = MenuView::new(false, "Player".into());
    view.screen = MenuScreen::Death;
    view.death_reason = "Player fell".into();
    let (state, cache) = (ViewState::default(), ScreenCache::default());
    for (controls, loading, expected) in [
        (false, false, vec!["Player fell"]),
        (true, false, vec!["Player fell", "Respawn", "Main menu"]),
        (false, true, vec!["Loading"]),
    ] {
        view.death_controls_visible = controls;
        view.death_loading = loading;
        let screen = super::super::menu_screens::screen_data(&view, &|_| None).unwrap();
        let rendered = cache
            .render(
                ScreenKey {
                    reference: screen.reference,
                    catalog: &catalog,
                    context: &screen.context,
                    data: &screen.data,
                    view: &state,
                    root: [400.0, 300.0],
                    px: 1.0,
                    text: [0; 3],
                },
                &LayoutEnv {
                    text: &Measure,
                    textures: &Measure,
                },
            )
            .unwrap();
        let texts = rendered
            .nodes
            .iter()
            .filter_map(|node| match &node.draw {
                Draw::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(texts, expected);
        let buttons = rendered
            .hits
            .iter()
            .filter(|hit| hit.kind == json_ui::HitKind::Button)
            .count();
        assert_eq!(buttons, if controls { 2 } else { 0 });
    }
}

#[test]
fn death_quit_popup_retains_the_modern_overlay_underneath() {
    use crate::menu::{MenuDialog, MenuScreen, MenuView};
    use crate::ui_runtime::{
        UiRuntime,
        presentation::{TextMetrics, UiPresentationRuntime},
    };
    use assets::{RuntimeUiAssets, UiFile, encode_ui_catalog};
    let files = [
        ("ui/_global_variables.json", "{}"),
        ("ui/_ui_defs.json", r#"{"ui_defs":["ui/death_screen.json","ui/popup_dialog.json"]}"#),
        ("ui/death_screen.json", r#"{"namespace":"death","death_screen":{"type":"screen","render_only_when_topmost":true,"controls":[
            {"background":{"type":"label","text":"death background","localize":false,"size":[200,20]}}
        ]}}"#),
        ("ui/popup_dialog.json", r#"{"namespace":"popup_dialog","modal_dialog_popup":{"type":"panel","controls":[
            {"popup":{"type":"label","text":"modal foreground","localize":false,"size":[200,20]}}
        ]}}"#),
    ].map(|(path,text)| UiFile { path: path.into(), bytes: text.as_bytes().into() });
    let bytes = encode_ui_catalog([1; 32], &[], &[], &[], &files).unwrap();
    let carrier = Arc::new(RuntimeUiAssets::decode(&bytes).unwrap());
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    let mut view = MenuView::new(false, "Player".into());
    view.visible = true;
    view.screen = MenuScreen::Death;
    view.dialog = Some(MenuDialog::DeathQuit);
    presentation.set_menu_view(Some(view));
    let (mut nodes, mut next) = (Vec::new(), 1);
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), None);
    presentation
        .append_menu(
            &UiRuntime::new(1),
            &mut nodes,
            &mut next,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    let texts = super::super::pack_harness::drawn_texts(&nodes);
    assert!(texts.iter().any(|text| text == "YOU DIED!"), "{texts:?}");
    assert!(
        texts.iter().any(|text| text == "modal foreground"),
        "{texts:?}"
    );
}
