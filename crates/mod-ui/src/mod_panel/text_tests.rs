//! Native fields preserve complete text values without converting them into numeric events.

use super::*;
use ui::mod_panel::{PanelPatch, TextEvent};

/// Starts a focused native text field without depending on a font or GPU fixture.
fn text_field(value: &str, min_chars: usize, max_chars: usize) -> ModUi {
    let mut panel = crate::test_support::sample_panel();
    panel.controls = vec![Control::Text {
        id: "name".into(),
        label: "Name".into(),
        value: value.into(),
        min_chars,
        max_chars,
    }];
    panel.sections.clear();
    let mut ui = ModUi::default();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    ui.set_mod_panel_open(true);
    ui.mod_panel.as_mut().unwrap().viewport = [400., 200.];
    ui.mod_panel
        .as_mut()
        .unwrap()
        .open_editor(0, [40., 40., 100., 20.]);
    ui
}

/// Drafts remain literal text even when they resemble JSON-UI bindings or template variables.
#[test]
fn native_text_editor_renders_binding_like_drafts_literally() {
    use json_ui::{Context, Draw, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState};
    use ui_engine::cached_screen::CachedScreen;
    struct Metrics;
    impl TextMeasure for Metrics {
        /// Supplies stable glyph extents without a local font carrier.
        fn extent(&self, text: &str) -> [f64; 2] {
            [text.chars().count() as f64 * 4., 8.]
        }
    }
    impl TextureSource for Metrics {
        /// Native editor chrome has no texture dependency.
        fn texture(&self, _: &str) -> Option<TextureMeta> {
            None
        }
    }
    for value in ["#FFFFFFFF", "$profile", "(#missing)", "##literal"] {
        let expected = format!("{value}|");
        let mut ui = text_field("Main", 0, 24);
        ui.mod_panel_key("TextInput", Some(value));
        ui.mod_panel
            .as_mut()
            .unwrap()
            .open_editor(0, [40., 40., 100., 40.]);
        ui.mod_panel_key("TextInput", Some(value));
        let panel = ui.mod_panel.as_ref().unwrap();
        let (catalog, _) =
            template::catalog(&panel.panel, [400., 200.], 0, 0, 4, panel.edit.as_ref()).unwrap();
        let catalog = std::sync::Arc::new(catalog);
        let mut screen = CachedScreen::default();
        let render = screen
            .render_shared_with(
                SCREEN,
                &catalog,
                &Context::default(),
                std::sync::Arc::clone(&panel.data),
                ([400., 200.], 1., [0; 3]),
                &LayoutEnv {
                    text: &Metrics,
                    textures: &Metrics,
                },
                &ViewState::default(),
            )
            .unwrap();
        let node = render
            .nodes
            .iter()
            .find(|node| matches!(&node.draw,Draw::Text {text,..} if text==&expected))
            .unwrap_or_else(|| panic!("native draft disappeared: {value}"));
        assert!(
            (node.dest.y + node.dest.h * 0.5 - 60.).abs() < 5.,
            "draft must cover the saved value in the focused row: {:?}",
            node.dest
        );
    }
}

/// Real keyboard edits retain UTF-8 characters and deliver only committed text events.
#[test]
fn native_text_fields_commit_unicode_and_keep_numeric_events_empty() {
    let mut ui = text_field("Main", 1, 8);
    assert!(ui.mod_panel_key("KeyE", Some("é🙂")).is_empty());
    assert!(ui.mod_panel_key("Enter", None).is_empty());
    assert!(
        ui.take_mod_panel_text_events(PanelUiOwner::Environment(1))
            .is_empty(),
        "another owner must not receive this component's text"
    );
    assert_eq!(
        ui.take_mod_panel_text_events(PanelUiOwner::Environment(0)),
        vec![TextEvent {
            id: "name".into(),
            value: "é🙂".into()
        }]
    );
    assert!(
        ui.take_mod_panel_text_events(PanelUiOwner::Environment(0))
            .is_empty()
    );
    ui.mod_panel
        .as_mut()
        .unwrap()
        .open_editor(0, [40., 40., 100., 20.]);
    for key in ["ArrowRight", "ArrowLeft", "Backspace", "Delete"] {
        assert!(ui.mod_panel_key(key, None).is_empty());
    }
    ui.mod_panel_key("Paste", Some("مرحبا"));
    ui.mod_panel_key("Enter", None);
    assert_eq!(
        ui.take_mod_panel_text_events(PanelUiOwner::Environment(0))[0].value,
        "مرحبا"
    );
}

/// Invalid drafts, cancellation and unchanged commits cannot create phantom setting updates.
#[test]
fn native_text_fields_respect_bounds_cancel_and_unchanged_values() {
    let mut ui = text_field("é🙂", 1, 2);
    ui.mod_panel_key("Paste", Some("abc"));
    ui.mod_panel_key("Enter", None);
    assert!(
        ui.take_mod_panel_text_events(PanelUiOwner::Environment(0))
            .is_empty()
    );
    ui.mod_panel
        .as_mut()
        .unwrap()
        .open_editor(0, [40., 40., 100., 20.]);
    ui.mod_panel_key("Backspace", None);
    ui.mod_panel_key("Enter", None);
    assert!(
        ui.mod_panel_editing(),
        "the minimum length keeps an invalid draft open"
    );
    assert!(
        ui.take_mod_panel_text_events(PanelUiOwner::Environment(0))
            .is_empty()
    );
    ui.mod_panel_key("Escape", None);
    assert!(!ui.mod_panel_editing());
    assert!(
        matches!(&ui.mod_panel.as_ref().unwrap().panel.controls[0],Control::Text {value,..} if value=="é🙂")
    );
    assert!(super::edit::visible_draft("é🙂مرحبا", false, "é🙂مرحبا".len(), 24.).ends_with('|'));
}

/// Text patches retain immutable character bounds and reject non-string or multiline values.
#[test]
fn text_schema_and_sparse_patches_preserve_their_value_domain() {
    let ui = text_field("Main", 1, 4);
    let panel = &ui.mod_panel.as_ref().unwrap().panel;
    let patch = PanelPatch::parse(r#"{"control_values":[[0,"é🙂"]]}"#).unwrap();
    let changed = patch.apply(panel, &Default::default()).unwrap();
    assert!(panel.controls[0].same_schema(&changed.controls[0]));
    for value in [
        serde_json::json!(42),
        serde_json::json!(""),
        serde_json::json!("abcde"),
        serde_json::json!("a\nb"),
    ] {
        let patch = PanelPatch::parse(
            &serde_json::json!({"controls":[{"id":"name","value":value}]}).to_string(),
        )
        .unwrap();
        assert!(patch.apply(panel, &Default::default()).is_err());
    }
    let mut different = changed.controls[0].clone();
    if let Control::Text { max_chars, .. } = &mut different {
        *max_chars = 8;
    }
    assert!(!different.same_schema(&panel.controls[0]));
}

/// A guest refresh cannot steal the keyboard from an already open native text draft.
#[test]
fn native_text_editing_survives_a_guest_capture_flag_refresh() {
    let mut ui = text_field("Main", 1, 24);
    ui.mod_panel_key("KeyP", Some("Draft"));
    let mut panel = ui.mod_panel.as_ref().unwrap().panel.clone();
    panel.capture_key = true;
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    assert!(ui.mod_panel_editing());
    ui.mod_panel_key("Enter", None);
    assert_eq!(
        ui.take_mod_panel_text_events(PanelUiOwner::Environment(0))[0].value,
        "Draft"
    );
}

/// Clipboard access requires a focused native field and exposes only its actual selection.
#[test]
fn native_clipboard_selection_follows_caret_and_focus() {
    let mut ui = text_field("é🙂", 0, 24);
    assert_eq!(ui.mod_panel_field_selection(), Some("é🙂"));
    ui.mod_panel_key("ArrowLeft", None);
    assert_eq!(ui.mod_panel_field_selection(), Some(""));
    ui.mod_panel_key("SelectAll", None);
    assert_eq!(ui.mod_panel_field_selection(), Some("é🙂"));
    ui.mod_panel_key("Escape", None);
    assert_eq!(ui.mod_panel_field_selection(), None);
    ui.set_mod_panel_open(false);
    assert_eq!(ui.mod_panel_field_selection(), None);
}
