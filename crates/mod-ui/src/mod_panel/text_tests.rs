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
        live: false,
        navigation_keys: false,
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

/// Drafts retain binding-like user text literally and never append a fake caret glyph.
#[test]
fn native_text_editor_renders_binding_like_drafts_literally() {
    for value in ["#FFFFFFFF", "$profile", "(#missing)", "##literal"] {
        let mut ui = text_field("Main", 0, 24);
        ui.mod_panel_key("TextInput", Some(value));
        let state = field_state(&ui);
        assert_eq!(state.display, value);
        assert_eq!(state.cursor, value.len());
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

/// Returns the focused field's retained editing state for behavior assertions.
fn field_state(ui: &ModUi) -> &super::field::Field {
    let Some(super::edit::Editor::Field { state, .. }) = &ui.mod_panel.as_ref().unwrap().edit
    else {
        panic!("expected focused text field");
    };
    state
}

/// Enables additive field capabilities while retaining the same focused draft.
fn live_field(value: &str, navigation: bool) -> ModUi {
    let mut ui = text_field(value, 0, 32);
    if let Control::Text {
        live,
        navigation_keys,
        ..
    } = &mut ui.mod_panel.as_mut().unwrap().panel.controls[0]
    {
        *live = true;
        *navigation_keys = navigation;
    }
    ui
}

/// Multiple edits in one input frame publish only the latest full value.
#[test]
fn native_live_field_changes_coalesce_and_submit_keeps_focus() {
    use ui::mod_panel::FieldEvent;
    let owner = PanelUiOwner::Environment(0);
    let mut ui = live_field("Main", false);
    ui.mod_panel_key("TextInput", Some("a"));
    ui.mod_panel_key("TextInput", Some("b"));
    ui.mod_panel_key("Backspace", None);
    assert!(
        ui.take_mod_panel_field_events(PanelUiOwner::Environment(1))
            .is_empty()
    );
    assert_eq!(
        ui.take_mod_panel_field_events(owner),
        vec![FieldEvent::Change(TextEvent {
            id: "name".into(),
            value: "a".into()
        })]
    );
    assert!(ui.take_mod_panel_text_events(owner).is_empty());
    ui.mod_panel_key("Paste", Some("é🙂"));
    assert_eq!(
        ui.take_mod_panel_field_events(owner),
        vec![FieldEvent::Change(TextEvent {
            id: "name".into(),
            value: "aé🙂".into()
        })]
    );
    ui.mod_panel_key("Enter", None);
    assert_eq!(
        ui.take_mod_panel_field_events(owner),
        vec![FieldEvent::Submit(TextEvent {
            id: "name".into(),
            value: "aé🙂".into()
        })]
    );
    assert!(ui.mod_panel_field_focused());
}

/// Selection replacement and word deletion keep all UTF-8 positions on character boundaries.
#[test]
fn native_live_field_word_movement_selection_and_paste() {
    use ui::mod_panel::FieldModifiers;
    let mut ui = live_field("héllo world 🙂", false);
    ui.mod_panel_key("End", None);
    ui.mod_panel_field_key(
        "ArrowLeft",
        None,
        FieldModifiers {
            alt: true,
            ..Default::default()
        },
    );
    assert_eq!(field_state(&ui).cursor, "héllo world ".len());
    ui.mod_panel_field_key(
        "ArrowLeft",
        None,
        FieldModifiers {
            control: true,
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(ui.mod_panel_field_selection(), Some("world "));
    ui.mod_panel_key("Paste", Some("earth "));
    assert_eq!(field_state(&ui).text, "héllo earth 🙂");
    ui.mod_panel_field_key(
        "Backspace",
        None,
        FieldModifiers {
            alt: true,
            ..Default::default()
        },
    );
    assert_eq!(field_state(&ui).text, "héllo 🙂");
    ui.mod_panel_field_key(
        "Delete",
        None,
        FieldModifiers {
            control: true,
            ..Default::default()
        },
    );
    assert_eq!(field_state(&ui).text, "héllo ");
    ui.mod_panel_key("Home", None);
    ui.mod_panel_field_key(
        "End",
        None,
        FieldModifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(ui.mod_panel_field_selection(), Some("héllo "));
}

/// Preedit is transient, replaces selection inline, and emits only when the platform commits.
#[test]
fn native_live_field_ime_composes_then_commits() {
    let owner = PanelUiOwner::Environment(0);
    let mut ui = live_field("old", true);
    ui.mod_panel_preedit("日本", Some(3));
    assert_eq!(field_state(&ui).text, "old");
    assert_eq!(field_state(&ui).display, "日本");
    assert!(ui.mod_panel_composing());
    assert_eq!(field_state(&ui).display_cursor(), 3);
    assert!(ui.take_mod_panel_field_events(owner).is_empty());
    ui.mod_panel_key("Unidentified", Some("leak"));
    assert_eq!(field_state(&ui).text, "old");
    ui.mod_panel_ime_commit("日本");
    assert_eq!(field_state(&ui).text, "日本");
    assert!(field_state(&ui).composition.is_empty());
    assert!(!ui.mod_panel_composing());
    assert!(
        matches!(&ui.take_mod_panel_field_events(owner)[0], ui::mod_panel::FieldEvent::Change(event) if event.value == "日本")
    );
    ui.mod_panel_key("Unidentified", Some("leak"));
    assert_eq!(field_state(&ui).text, "日本");
}

/// Navigation stays native unless opted in and horizontal motion reaches an unselected edge.
#[test]
fn native_live_field_navigation_retains_edge_modifiers() {
    use ui::mod_panel::{FieldEvent, FieldModifiers};
    let owner = PanelUiOwner::Environment(0);
    for navigation in [false, true] {
        let mut ui = live_field("ab", navigation);
        let modifiers = FieldModifiers {
            shift: true,
            alt: true,
            ..Default::default()
        };
        ui.mod_panel_field_key("ArrowDown", None, modifiers);
        assert_eq!(
            ui.take_mod_panel_field_events(owner).len(),
            usize::from(navigation)
        );
        ui.mod_panel_key("ArrowRight", None); // collapse the opening selection
        assert!(ui.take_mod_panel_field_events(owner).is_empty());
        ui.mod_panel_field_key("ArrowRight", None, modifiers);
        let events = ui.take_mod_panel_field_events(owner);
        assert_eq!(events.len(), usize::from(navigation));
        if navigation {
            assert!(
                matches!(&events[0], FieldEvent::Key(event) if event.key == "ArrowRight" && event.modifiers == modifiers)
            );
        }
        ui.mod_panel_key("Home", None);
        ui.mod_panel_field_key("Enter", None, modifiers);
        let events = ui.take_mod_panel_field_events(owner);
        assert!(matches!(&events[0], FieldEvent::Submit(_)));
        assert_eq!(events.len(), if navigation { 2 } else { 1 });
        if navigation {
            assert!(
                matches!(&events[1], FieldEvent::Key(event) if event.key == "Enter" && event.modifiers.shift)
            );
        }
    }
}

/// Paints native field feedback through the production engine without local game assets.
fn paint_field(
    ui: &mut ModUi,
    engine: &FormEngine,
    font: &assets::RuntimeFontCatalog,
    layouts: &mut ui::TextLayoutCache,
    now: f64,
) -> Vec<ui::UiNode> {
    paint_field_with_safe_area(ui, engine, font, layouts, now, ui::SafeArea::ZERO)
}

/// Paints with content insets while retaining window-relative input coordinates.
fn paint_field_with_safe_area(
    ui: &mut ModUi,
    engine: &FormEngine,
    font: &assets::RuntimeFontCatalog,
    layouts: &mut ui::TextLayoutCache,
    now: f64,
    safe_area: ui::SafeArea,
) -> Vec<ui::UiNode> {
    let mut nodes = Vec::new();
    ui.paint_mod_panel(
        Some(engine),
        &mut PaintFrame {
            layouts,
            font,
            metrics: ui_engine::text_metrics::TextMetrics::for_viewport(
                [1280, 720],
                ui::DpiScale::new(1.).unwrap(),
                Some(2),
            ),
            solid_page: 0,
            safe_area,
            content: [
                1280. - safe_area.left() - safe_area.right(),
                720. - safe_area.top() - safe_area.bottom(),
            ],
            translate: &|_| None,
            language: [0; 3],
            now,
            nodes: &mut nodes,
            next: &mut 1,
            renderers: &mut [],
        },
    );
    nodes
}

/// Creates the production painter with an empty redistributable UI catalog.
fn field_engine() -> FormEngine {
    use ui_engine::{engine::screen::ScreenRegistry, sources::ImageWork};
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    FormEngine::new(
        Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
        Catalog::default(),
        0,
        REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop())),
        Arc::new(NoImages),
        ImageWork::inline(),
    )
}

/// Blink, movement and selection retain the shaped glyphs and the resolved screen.
#[test]
fn native_field_blink_reuses_shaping_and_catalog() {
    let engine = field_engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = live_field("0000", false);
    paint_field(&mut ui, &engine, &font, &mut layouts, 0.);
    ui.mod_panel
        .as_mut()
        .unwrap()
        .open_editor(0, [40., 40., 180., 26.]);
    ui.mod_panel_key("End", None);
    let first = paint_field(&mut ui, &engine, &font, &mut layouts, 1.);
    let catalog = Arc::clone(ui.mod_panel.as_ref().unwrap().catalog.as_ref().unwrap());
    let passes = ui.mod_panel.as_ref().unwrap().screen.passes;
    let shaped = layouts.built_layout_count();
    let rectangles = field_state(&ui).feedback_updates;
    let layout = Arc::clone(field_state(&ui).layout.as_ref().unwrap());
    let second = paint_field(&mut ui, &engine, &font, &mut layouts, 1.6);
    let bindings = Arc::clone(&ui.mod_panel.as_ref().unwrap().data);
    ui.mod_panel_key("Enter", None);
    assert!(
        Arc::ptr_eq(&bindings, &ui.mod_panel.as_ref().unwrap().data),
        "submitting an unchanged live value retains its bindings"
    );

    assert_eq!(
        first.len(),
        second.len() + 1,
        "blink removes only the caret rectangle"
    );
    assert_eq!(layouts.built_layout_count(), shaped);
    assert_eq!(
        field_state(&ui).feedback_updates,
        rectangles,
        "blink reuses feedback rectangles"
    );
    assert_eq!(ui.mod_panel.as_ref().unwrap().screen.passes, passes);
    assert!(Arc::ptr_eq(
        &catalog,
        ui.mod_panel.as_ref().unwrap().catalog.as_ref().unwrap()
    ));
    ui.mod_panel_field_key(
        "ArrowLeft",
        None,
        ui::mod_panel::FieldModifiers {
            shift: true,
            ..Default::default()
        },
    );
    paint_field(&mut ui, &engine, &font, &mut layouts, 2.);
    assert_eq!(layouts.built_layout_count(), shaped);
    assert!(Arc::ptr_eq(
        &layout,
        field_state(&ui).layout.as_ref().unwrap()
    ));
    ui.mod_panel_key("TextInput", Some("2"));
    paint_field(&mut ui, &engine, &font, &mut layouts, 2.1);
    assert!(
        layouts.built_layout_count() > shaped,
        "changed values receive fresh glyph layouts"
    );
    // Pointer placement uses the same retained stops as the painted glyphs.
    let state = match ui.mod_panel.as_mut().unwrap().edit.as_mut().unwrap() {
        super::edit::Editor::Field { state, .. } => state,
        _ => unreachable!(),
    };
    let left = state.left;
    let stop = state.layout.as_ref().unwrap().first_line_caret_x_64(2) as f32 / 64.;
    state.place_cursor(left + stop, false);
    assert_eq!(state.cursor, 2);
    state.place_cursor(left + stop + 1000., true);
    assert_eq!(&state.text[state.selection()], "02");

    ui.mod_panel_preedit("00", Some(1));
    let composition = paint_field(&mut ui, &engine, &font, &mut layouts, 2.2);
    let composition_shapes = layouts.built_layout_count();
    ui.mod_panel_preedit("00", Some(2));
    paint_field(&mut ui, &engine, &font, &mut layouts, 2.3);
    assert_eq!(
        layouts.built_layout_count(),
        composition_shapes,
        "preedit cursor changes reuse the same glyphs"
    );

    assert!(
        composition
            .iter()
            .filter(|node| matches!(node.visual(), ui::UiVisual::Solid { .. }))
            .count()
            >= 2,
        "composition paints underline and caret"
    );
    let rect = [40., 40., 42., 26.];
    let panel = ui.mod_panel.as_mut().unwrap();
    panel.panel.controls[0] = Control::Slider {
        id: "number".into(),
        label: "Number".into(),
        value: 35.,
        min: 0.,
        max: 100.,
        step: 1.,
    };
    panel.refresh_data();
    panel.open_editor(0, rect);
    let number = "123456789012345678901234";
    ui.mod_panel_ime_commit(number);
    ui.mod_panel_key("TextInput", Some("1"));
    assert_eq!(
        field_state(&ui).text,
        number,
        "numeric character capacity is unchanged"
    );
    paint_field(&mut ui, &engine, &font, &mut layouts, 3.);
    assert!(
        field_state(&ui).scroll > 0.,
        "long numeric drafts scroll to their caret"
    );
    let frame = ui.mod_panel.as_ref().unwrap().frame.as_ref().unwrap();
    let bounds = [rect[0], rect[0] + rect[2]].map(|x| frame.origin[0] + x as f32 * frame.scale);
    assert!((bounds[0]..=bounds[1]).contains(&field_state(&ui).ime_position[0]));
    let layout = Arc::clone(field_state(&ui).layout.as_ref().unwrap());
    ui.mod_panel_key("Home", None);
    paint_field(&mut ui, &engine, &font, &mut layouts, 3.1);
    assert_eq!(field_state(&ui).scroll, 0.);
    assert!(
        Arc::ptr_eq(&layout, field_state(&ui).layout.as_ref().unwrap()),
        "scrolling reuses the shaped text"
    );
}

struct NoImages;
impl ui_engine::sources::RemoteSource for NoImages {
    /// Native text fields never need a network image fixture.
    fn state(&self, _: &str) -> ui_engine::sources::RemoteImage {
        ui_engine::sources::RemoteImage::Failed
    }
}

/// Dismissing a live draft must leave the saved row and any collection binding on its new value.
#[test]
fn native_live_field_dismissal_keeps_published_value() {
    let mut ui = live_field("Main", false);
    ui.mod_panel_key("TextInput", Some("Latest"));
    ui.mod_panel_key("Escape", None);
    ui.take_mod_panel_field_events(PanelUiOwner::Environment(0));
    let panel = ui.mod_panel.as_ref().unwrap().panel.clone();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    assert_eq!(
        ui.mod_panel
            .as_ref()
            .unwrap()
            .data
            .global_value("#row_0_value"),
        Some(&json_ui::Scalar::Text("Latest".into()))
    );
}

/// Guests can clear or replace a submitted search while the persistent editor keeps focus.
#[test]
fn native_live_field_guest_replaces_focused_value() {
    let mut ui = live_field("Main", true);
    ui.mod_panel_key("TextInput", Some("query"));
    ui.mod_panel_key("Enter", None);
    ui.take_mod_panel_field_events(PanelUiOwner::Environment(0));
    let mut panel = ui.mod_panel.as_ref().unwrap().panel.clone();
    if let Control::Text { value, .. } = &mut panel.controls[0] {
        value.clear();
    }
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    assert!(ui.mod_panel_field_focused());
    assert_eq!(field_state(&ui).text, "");
    ui.mod_panel_key("TextInput", Some("next"));
    ui.mod_panel_key("Enter", None);
    assert_eq!(
        ui.take_mod_panel_text_events(PanelUiOwner::Environment(0))
            .last()
            .unwrap()
            .value,
        "next"
    );
}

/// Explicit scripted/IME text commits also preserve the existing native number editor.
#[test]
fn native_numeric_field_accepts_explicit_text_commits() {
    let mut ui = text_field("Main", 0, 24);
    let panel = ui.mod_panel.as_mut().unwrap();
    panel.panel.controls[0] = Control::Slider {
        id: "number".into(),
        label: "Number".into(),
        value: 35.,
        min: 0.,
        max: 100.,
        step: 1.,
    };
    panel.open_editor(0, [40., 40., 100., 20.]);
    assert!(ui.mod_panel_field_focused());
    ui.mod_panel_preedit("7", Some(1));
    assert!(ui.mod_panel_composing());
    ui.mod_panel_ime_commit("73");
    assert_eq!(
        ui.mod_panel_key("Enter", None),
        vec![Event {
            id: "number".into(),
            value: 73.
        }]
    );
}

/// Every admitted navigation key must have a real focused-field route to the guest.
#[test]
fn native_field_navigation_contract_reaches_guest() {
    for key in ui::mod_panel::FIELD_NAVIGATION_KEYS {
        let mut ui = live_field("ab", true);
        ui.mod_panel_key(if *key == "ArrowLeft" { "Home" } else { "End" }, None);
        ui.mod_panel_key(key, None);
        assert!(ui.take_mod_panel_field_events(PanelUiOwner::Environment(0)).iter().any(|event|
            matches!(event, ui::mod_panel::FieldEvent::Key(event) if event.key == *key)), "missing navigation route for {key}");
    }
}

/// A host echo cannot replace a below-minimum draft after an intermediate valid edit coalesces away.
#[test]
fn native_live_field_coalescing_keeps_retained_values_in_sync() {
    let mut ui = text_field("Main", 1, 8);
    if let Control::Text { live, .. } = &mut ui.mod_panel.as_mut().unwrap().panel.controls[0] {
        *live = true;
    }
    let echo = ui.mod_panel.as_ref().unwrap().panel.clone();
    ui.mod_panel_key("TextInput", Some("a"));
    ui.mod_panel_key("Backspace", None);
    let events = ui.take_mod_panel_field_events(PanelUiOwner::Environment(0));
    assert!(
        matches!(&events[..], [ui::mod_panel::FieldEvent::Change(event)] if event.value.is_empty())
    );
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&echo))
        .unwrap();
    assert!(ui.mod_panel_field_focused());
    assert_eq!(field_state(&ui).text, "");
}

/// Mouse selection and IME candidates share the painted glyph's window position with insets.
#[test]
fn native_field_safe_area_preserves_pointer_and_ime_geometry() {
    let engine = field_engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    for safe_area in [
        ui::SafeArea::ZERO,
        ui::SafeArea::new(72., 44., 24., 16.).unwrap(),
    ] {
        let mut ui = live_field("0000", false);
        paint_field_with_safe_area(&mut ui, &engine, &font, &mut layouts, 0., safe_area);
        ui.mod_panel
            .as_mut()
            .unwrap()
            .open_editor(0, [40., 40., 180., 26.]);
        ui.mod_panel_key("End", None);
        let nodes =
            paint_field_with_safe_area(&mut ui, &engine, &font, &mut layouts, 1., safe_area);
        let layout = Arc::clone(field_state(&ui).layout.as_ref().unwrap());
        let text_id = nodes.iter().find(|node| matches!(node.visual(), ui::UiVisual::Text { layout: painted, .. } if Arc::ptr_eq(painted, &layout))).unwrap().id();
        let mut tree = ui::UiTree::new(nodes).unwrap();
        let frame = tree
            .layout(
                ui::UiRect::new(
                    ui::UiPoint::new(0., 0.).unwrap(),
                    ui::UiPoint::new(1280., 720.).unwrap(),
                )
                .unwrap(),
                ui::UiScale::default(),
                safe_area,
            )
            .unwrap();
        let text = frame.bounds(text_id).unwrap();
        let x = |at| text.min().x() + layout.first_line_caret_x_64(at) as f32 / 64.;
        let y = text.min().y() + text.height() * 0.5;
        assert_eq!(ui.mod_panel_ime_position(), Some([x(4), text.max().y()]));
        ui.mod_panel_events([x(1), y], true, true);
        assert_eq!(field_state(&ui).cursor, 1);
        ui.mod_panel_events([x(3), y], false, false);
        assert_eq!(
            &field_state(&ui).text[field_state(&ui).selection()],
            "00",
            "a move and release between frames completes the selection"
        );
        ui.mod_panel_events([x(1), y], true, true);
        ui.mod_panel_events([x(2), y], false, true);
        ui.mod_panel_events([x(4), y], false, false);
        assert_eq!(
            &field_state(&ui).text[field_state(&ui).selection()],
            "000",
            "release extends an existing drag to its final endpoint"
        );
    }
}

/// IME spelling can exceed a short field's committed limit before committing one character.
#[test]
fn native_field_ime_preedit_exceeds_commit_limit() {
    let mut ui = text_field("", 0, 1);
    if let Control::Text { live, .. } = &mut ui.mod_panel.as_mut().unwrap().panel.controls[0] {
        *live = true;
    }
    let owner = PanelUiOwner::Environment(0);
    ui.mod_panel_preedit("ni", Some(2));
    assert_eq!(field_state(&ui).display, "ni");
    assert_eq!(field_state(&ui).text, "");
    assert!(ui.take_mod_panel_field_events(owner).is_empty());
    ui.mod_panel_ime_commit("你好");
    assert_eq!(field_state(&ui).text, "");
    assert!(ui.take_mod_panel_field_events(owner).is_empty());
    ui.mod_panel_preedit("ni", Some(2));
    ui.mod_panel_ime_commit("你");
    assert_eq!(field_state(&ui).text, "你");
    assert!(matches!(&ui.take_mod_panel_field_events(owner)[..],
        [ui::mod_panel::FieldEvent::Change(event)] if event.value == "你"));
}
