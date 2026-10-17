use std::{collections::BTreeMap, sync::Arc};

use json_ui::Catalog;
use mod_ui::{
    experience_modal::ExperienceModal, template_screen::package_catalog as modal_catalog,
};
use server_experience::screen;
use ui_engine::text_metrics::TextMetrics;

use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

fn files(templates: &[(&str, &str)]) -> screen::Files {
    screen::Files {
        namespace: "demo".into(),
        templates: templates
            .iter()
            .map(|(path, text)| ((*path).to_owned(), text.as_bytes().to_vec()))
            .collect(),
        textures: Vec::new(),
    }
}

/// The carrier's vanilla catalog, as the engine keeps it below every server pack.
fn vanilla() -> Option<Catalog> {
    let carrier = ui_engine::test_support::carrier()?;
    Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .ok()
}

#[test]
fn templates_resolve_against_vanilla_and_their_own_namespace_only() {
    let Some(vanilla) = vanilla() else {
        return;
    };
    let terminal = r##"{"namespace": "demo",
        "terminal@common.empty_panel": {"controls": [{"row@demo.row": {}}]},
        "row": {"type": "label", "text": "#name"}}"##;
    let catalog = modal_catalog(&vanilla, &files(&[("ui/terminal.json", terminal)])).unwrap();
    assert!(catalog.lookup("demo", "terminal").is_some());
    let foreign = r#"{"namespace": "demo", "terminal@cinnabar_experience.indicator": {}}"#;
    assert!(modal_catalog(&vanilla, &files(&[("ui/terminal.json", foreign)])).is_err());
    let server = r#"{"namespace": "demo", "terminal@my_server_pack.panel": {}}"#;
    assert!(modal_catalog(&vanilla, &files(&[("ui/terminal.json", server)])).is_err());
    let rootless = r#"{"namespace": "demo", "panel": {}}"#;
    assert!(modal_catalog(&vanilla, &files(&[("ui/terminal.json", rootless)])).is_err());
    let mut vanilla_namespace = files(&[(
        "ui/terminal.json",
        r#"{"namespace": "common", "terminal": {}}"#,
    )]);
    vanilla_namespace.namespace = "common".into();
    assert!(modal_catalog(&vanilla, &vanilla_namespace).is_err());
}

/// A 1×1 PNG: signature, IHDR, IDAT and IEND.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[test]
fn bundle_textures_draw_and_nothing_outside_textures_resolves() {
    use json_ui::TextureSource;
    let Some(presentation) = super::super::super::pack_harness::engine_presentation() else {
        return;
    };
    let engine = presentation.engine.as_deref().unwrap();
    let files = vec![("textures/demo/panel.png".to_owned(), PIXEL.to_vec())];
    let set = engine
        .textures
        .confined(&files, 7, ui_engine::pages::MODAL_UI_PAGES);
    let atlas = set.lock();
    let view = ui_engine::textures::Textures {
        assets: engine.assets(),
        set: &set,
        atlas: &atlas,
        images: None,
    };
    assert_eq!(
        view.texture("textures/demo/panel").unwrap().pixels,
        [1.0, 1.0]
    );
    for path in [
        "https://example.com/panel.png",
        "textures/../ui/panel",
        "ui/demo/panel",
        "",
    ] {
        assert!(view.texture(path).is_none(), "{path}");
        assert!(view.missing(path), "{path}");
    }
}

/// A terminal with one vanilla-style edit box, `demo.search`, 100×20 GUI units at the top left,
/// whose label `display` shows its text; it needs nothing from the vanilla pack.
const SEARCH: &str = r#"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"search": {"type": "edit_box", "size": [100, 20],
            "anchor_from": "top_left", "anchor_to": "top_left",
            "text_box_name": "demo.search", "max_length": 10, "text_control": "display",
            "button_mappings": [
                {"from_button_id": "button.menu_select", "to_button_id": "button.text_edit_box_selected",
                 "handle_select": true, "handle_deselect": false, "mapping_type": "pressed"},
                {"from_button_id": "button.menu_select", "to_button_id": "button.text_edit_box_selected",
                 "handle_select": false, "handle_deselect": true, "mapping_type": "global",
                 "consume_event": false},
                {"from_button_id": "button.menu_cancel", "to_button_id": "button.text_edit_box_deselected",
                 "handle_select": false, "handle_deselect": true, "mapping_type": "global"}],
            "controls": [{"display": {"type": "label", "text": "", "size": [100, 20]}}]}}]}}"#;

/// The search terminal drawn open at `size` physical pixels, scale 1, over gameplay.
fn drawn(
    modal: &screen::Modal,
    files: &Arc<screen::Files>,
    size: [u32; 2],
) -> UiPresentationRuntime {
    let mut presentation = crate::test_support::mini_engine_presentation();
    redraw(&mut presentation, modal, files, size);
    presentation
}

fn redraw(
    presentation: &mut UiPresentationRuntime,
    modal: &screen::Modal,
    files: &Arc<screen::Files>,
    size: [u32; 2],
) {
    presentation.set_experience_modal(Some(ExperienceModal {
        bundle: "demo",
        files,
        modal,
    }));
    let player_runtime = player_state::PlayerState::new(1);
    presentation
        .build(
            &player_runtime,
            &UiRuntime::new(1),
            0,
            size,
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
}

fn open_search() -> (screen::Modal, Arc<screen::Files>) {
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    (modal, Arc::new(files(&[("ui/terminal.json", SEARCH)])))
}

/// The `#item_id_aux` keys a client part binds in its rows are the ones its item renderers draw.
#[test]
fn bound_item_keys_are_the_modal_icon_keys() {
    let (mut modal, files) = open_search();
    let row =
        |key: i64| screen::Row::from([("#item_id_aux".to_owned(), screen::Value::Integer(key))]);
    modal
        .collections
        .insert("items".into(), Arc::from([row(1 << 16), row(5 << 16 | 2)]));
    let presentation = drawn(&modal, &files, [1280, 720]);
    let keys = presentation.mod_ui().experience_modal_icon_keys().unwrap();
    assert_eq!(
        keys.iter().copied().collect::<Vec<_>>(),
        [1 << 16, 5 << 16 | 2]
    );
}

/// A terminal with the player's hotbar and inventory rows at the top left, 18 GUI units a slot,
/// each a button as vanilla's container cells are.
const SLOTS: &str = r#"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"hotbar": {"type": "grid", "size": [162, 18], "offset": [0, 0],
            "anchor_from": "top_left", "anchor_to": "top_left", "grid_dimensions": [9, 1],
            "collection_name": "hotbar_items", "grid_item_template": "demo.cell"}},
        {"inventory": {"type": "grid", "size": [162, 54], "offset": [0, 20],
            "anchor_from": "top_left", "anchor_to": "top_left", "grid_dimensions": [9, 3],
            "collection_name": "inventory_items", "grid_item_template": "demo.cell"}}]},
    "cell": {"type": "button", "size": [18, 18], "button_mappings": [
        {"from_button_id": "button.menu_select", "to_button_id": "button.container_slot", "mapping_type": "pressed"}],
        "controls": [
        {"item": {"type": "custom", "renderer": "inventory_item_renderer", "size": [16, 16]}}]}}"#;

/// A hotbar key acts on the modal's player slot under the pointer, as in a container screen: a
/// hotbar row is that hotbar slot and an inventory row the main inventory slot after it.
#[test]
fn modal_inventory_rows_hit_their_player_slots() {
    use ui_screens::container_actions::InventoryCellHit;
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    let files = Arc::new(files(&[("ui/terminal.json", SLOTS)]));
    let presentation = drawn(&modal, &files, [1280, 720]);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    let at = |x: f32, y: f32| presentation.experience_modal_inventory_hit([x * scale, y * scale]);
    assert_eq!(at(2.0 * 18.0 + 9.0, 9.0), Some(InventoryCellHit::Player(2)));
    assert_eq!(
        at(18.0 + 9.0, 20.0 + 18.0 + 9.0),
        Some(InventoryCellHit::Player(19))
    );
    assert_eq!(at(400.0, 400.0), None);
}

/// The drawn modal's size is its root's, in GUI units, with the GUI scale that maps them onto the
/// window; a resize changes it, and a closed modal has none.
#[test]
fn a_drawn_modal_reports_its_gui_size() {
    let (mut modal, files) = open_search();
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let size = presentation
        .mod_ui()
        .experience_modal_size()
        .expect("drawn");
    assert!(size.valid(), "{size:?}");
    assert!((size.width * size.scale - 1280.0).abs() < 1.0, "{size:?}");
    assert!((size.height * size.scale - 720.0).abs() < 1.0, "{size:?}");
    redraw(&mut presentation, &modal, &files, [1600, 900]);
    let wider = presentation
        .mod_ui()
        .experience_modal_size()
        .expect("drawn");
    assert!(
        (wider.width * wider.scale - 1600.0).abs() < 1.0,
        "{wider:?}"
    );
    modal.open(None);
    redraw(&mut presentation, &modal, &files, [1600, 900]);
    assert_eq!(presentation.mod_ui().experience_modal_size(), None);
}

/// Pointer and keyboard drive the modal's edit box as vanilla's `text_edit_box`: a press selects
/// it, typing edits it and is reported by its `text_box_name`, Escape first deselects it, and only
/// an Escape with nothing selected is left to close the modal.
#[test]
fn edit_boxes_take_typing_and_escape_deselects_first() {
    let (modal, files) = open_search();
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    let inside = Some([10.0 * scale, 10.0 * scale]);
    let typed = |text: &str| vec![text.to_owned()];
    let none: Vec<String> = Vec::new();
    let pressed = presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, true, &none, false, 0.0);
    assert!(pressed.edits.is_empty() && !pressed.escape_consumed);
    let edited =
        presentation
            .mod_ui_mut()
            .edit_experience_modal(inside, false, &typed("iron"), false, 0.1);
    assert_eq!(
        edited.edits,
        [("demo.search".to_owned(), "iron".to_owned())]
    );
    let back =
        presentation
            .mod_ui_mut()
            .edit_experience_modal(inside, false, &typed("\u{8}"), false, 0.2);
    assert_eq!(back.edits, [("demo.search".to_owned(), "iro".to_owned())]);
    let escaped = presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, false, &none, true, 0.3);
    assert!(escaped.escape_consumed);
    let again = presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, false, &none, true, 0.4);
    assert!(!again.escape_consumed);
    // Unselected, typing goes nowhere.
    let ignored =
        presentation
            .mod_ui_mut()
            .edit_experience_modal(inside, false, &typed("x"), false, 0.5);
    assert!(ignored.edits.is_empty());
}

/// `set-text` reaches the box once, without being reported back; the user's later typing stands.
#[test]
fn host_text_reaches_the_box_once() {
    let (mut modal, files) = open_search();
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    modal.set_text("demo.search".into(), "gold".into());
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    let inside = Some([10.0 * scale, 10.0 * scale]);
    let none: Vec<String> = Vec::new();
    let pressed = presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, true, &none, false, 0.0);
    assert!(pressed.edits.is_empty());
    let typed = vec!["!".to_owned()];
    let edited = presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, false, &typed, false, 0.1);
    assert_eq!(
        edited.edits,
        [("demo.search".to_owned(), "gold!".to_owned())]
    );
    // A later unrelated change does not set the text again.
    modal.set_value("#title".into(), screen::Value::Text("ME".into()));
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let more = presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, false, &typed, false, 0.2);
    assert_eq!(
        more.edits,
        [("demo.search".to_owned(), "gold!!".to_owned())]
    );
}

/// Two 50×20 buttons: `cycle` maps a primary and a secondary press to `demo.cycle`, as vanilla's
/// slot buttons do; `plain` maps only a primary press to `demo.plain`.
const BUTTONS: &str = r#"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"cycle": {"type": "button", "size": [50, 20],
            "anchor_from": "top_left", "anchor_to": "top_left",
            "button_mappings": [
                {"from_button_id": "button.menu_select", "to_button_id": "demo.cycle", "mapping_type": "pressed"},
                {"from_button_id": "button.menu_secondary_select", "to_button_id": "demo.cycle", "mapping_type": "pressed"}]}},
        {"plain": {"type": "button", "size": [50, 20], "offset": [60, 0],
            "anchor_from": "top_left", "anchor_to": "top_left",
            "button_mappings": [
                {"from_button_id": "button.menu_select", "to_button_id": "demo.plain", "mapping_type": "pressed"}]}}]}}"#;

/// A secondary press released over the control it began on fires the action its
/// `button.menu_secondary_select` mapping names; a control without one takes no secondary press.
#[test]
fn secondary_presses_fire_the_secondary_mapping() {
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    let files = Arc::new(files(&[("ui/terminal.json", BUTTONS)]));
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    let cycle = Some([10.0 * scale, 10.0 * scale]);
    let plain = Some([70.0 * scale, 10.0 * scale]);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .secondary_press_experience_modal(cycle, true, false),
        None
    );
    assert_eq!(
        presentation
            .mod_ui_mut()
            .secondary_press_experience_modal(cycle, false, true),
        Some(("demo.cycle".to_owned(), None))
    );
    presentation
        .mod_ui_mut()
        .secondary_press_experience_modal(plain, true, false);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .secondary_press_experience_modal(plain, false, true),
        None
    );
    // Released elsewhere, nothing fires.
    presentation
        .mod_ui_mut()
        .secondary_press_experience_modal(cycle, true, false);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .secondary_press_experience_modal(plain, false, true),
        None
    );
}

/// What the user types draws in the box: the component's text reaches the label it targets.
#[test]
fn typed_text_draws_in_the_box() {
    let (modal, files) = open_search();
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    let inside = Some([10.0 * scale, 10.0 * scale]);
    let none: Vec<String> = Vec::new();
    presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, true, &none, false, 0.0);
    presentation.mod_ui_mut().edit_experience_modal(
        inside,
        false,
        &["iron".to_owned()],
        false,
        0.1,
    );
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let drawn: Vec<String> = presentation
        .last_frame
        .as_ref()
        .expect("a frame")
        .nodes
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
    assert!(
        drawn.iter().any(|text| text.starts_with("iron")),
        "{drawn:?}"
    );
}

/// The texts the last frame drew.
fn drawn_texts(presentation: &UiPresentationRuntime) -> Vec<String> {
    presentation
        .last_frame
        .as_ref()
        .expect("a frame")
        .nodes
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
        .collect()
}

/// A selected box draws vanilla's caret after its text, blinking on the edit component's
/// interval and shown again by each edit; a deselected box draws none.
#[test]
fn a_selected_box_draws_a_blinking_caret() {
    let (modal, files) = open_search();
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    let inside = Some([10.0 * scale, 10.0 * scale]);
    let none: Vec<String> = Vec::new();
    let caret = format!("iron{}", json_ui::CARET_GLYPH);
    let at = |presentation: &mut UiPresentationRuntime, typed: &[String], escape, now| {
        presentation
            .mod_ui_mut()
            .edit_experience_modal(inside, false, typed, escape, now);
        redraw(presentation, &modal, &files, [1280, 720]);
        drawn_texts(presentation)
    };
    presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, true, &none, false, 0.0);
    let typed = at(&mut presentation, &["iron".to_owned()], false, 0.05);
    assert!(typed.contains(&caret), "{typed:?}");
    let blinked = at(
        &mut presentation,
        &none,
        false,
        0.05 + json_ui::CARET_BLINK_SECONDS * 1.5,
    );
    assert!(blinked.iter().any(|text| text == "iron"), "{blinked:?}");
    let back = at(
        &mut presentation,
        &none,
        false,
        0.05 + json_ui::CARET_BLINK_SECONDS * 2.5,
    );
    assert!(back.contains(&caret), "{back:?}");
    let escaped = at(&mut presentation, &none, true, 1.0);
    assert!(!escaped.contains(&caret), "{escaped:?}");
    assert!(escaped.iter().any(|text| text == "iron"), "{escaped:?}");
}

/// A search field built on vanilla's `common.text_edit_box`, clipped and right-anchored as a
/// terminal's search is.
const VANILLA_SEARCH: &str = r#"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"search@common.text_edit_box": {"anchor_from": "top_left", "anchor_to": "top_left",
            "size": [89, 12], "max_length": 25,
            "$text_box_name": "demo.search",
            "$text_edit_clipping_panel_size": [79, "100%"],
            "$text_edit_box_label_anchor_point": "top_right"}}]}}"#;

/// A terminal's search field as a client part writes it: inside an offset `input_panel`, on
/// vanilla's `common.text_edit_box` with its own background, colours, offset and button mappings.
/// Vanilla's box clips its text to its height less 4px, which in a 12-unit box cuts off the caret
/// below the baseline, so the clipper is 2px taller and the label 1px lower, keeping the text put.
const TERMINAL_SEARCH: &str = r#"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"screen@demo.screen": {}}]},
    "screen": {"type": "input_panel", "size": [195, 152], "offset": [40, 30],
        "anchor_from": "top_left", "anchor_to": "top_left", "layer": 1,
        "controls": [{"search@demo.search": {}}]},
    "search@common.text_edit_box": {
        "anchor_from": "top_left", "anchor_to": "top_left", "offset": [80, 4], "size": [89, 12],
        "layer": 2, "max_length": 25,
        "$text_box_name": "demo.search",
        "$place_holder_text": "Search...",
        "$place_holder_text_color": [0.8706, 0.8745, 0.8902],
        "$text_box_text_color": [1, 1, 1],
        "$text_offset": [-1, 0],
        "$text_edit_clipping_panel_size": [79, "100% + 2px"],
        "$text_edit_box_label_offset": [0, 1],
        "$text_edit_box_label_anchor_point": "top_right",
        "$text_background_default": "demo.search_field",
        "$text_background_hover": "demo.search_field",
        "button_mappings": [
            {"from_button_id": "button.menu_select", "to_button_id": "$text_edit_box_selected_to_button_id", "handle_select": true, "handle_deselect": false, "mapping_type": "pressed"},
            {"from_button_id": "button.menu_select", "to_button_id": "$text_edit_box_selected_to_button_id", "handle_select": false, "handle_deselect": true, "mapping_type": "global", "consume_event": false},
            {"from_button_id": "button.menu_ok", "to_button_id": "$text_edit_box_selected_to_button_id", "handle_select": true, "handle_deselect": true, "mapping_type": "pressed"},
            {"from_button_id": "button.menu_ok", "to_button_id": "$text_edit_box_selected_to_button_id", "handle_select": false, "handle_deselect": true, "mapping_type": "global"},
            {"from_button_id": "button.menu_cancel", "to_button_id": "$text_edit_box_deselected_to_button_id", "handle_select": false, "handle_deselect": true, "mapping_type": "global"},
            {"to_button_id": "$text_edit_box_hovered_button_id", "mapping_type": "pressed"},
            {"from_button_id": "$text_edit_box_clear_from_button_id", "to_button_id": "$text_edit_box_clear_to_button_id", "handle_select": false, "handle_deselect": false, "mapping_type": "focused"},
            {"from_button_id": "button.menu_secondary_select", "to_button_id": "demo.search", "mapping_type": "pressed"}]},
    "search_field": {"type": "panel", "size": [89, 12]}}"#;

/// Whether `text`'s last glyph, the caret, lies inside the panel that clips it.
fn caret_in_clip(presentation: &UiPresentationRuntime, text: &str) -> bool {
    let nodes = &presentation.last_frame.as_ref().expect("a frame").nodes;
    nodes.iter().any(|node| {
        let ui::UiVisual::Text { layout, .. } = node.visual() else {
            return false;
        };
        let glyphs = layout.glyphs();
        if glyphs
            .iter()
            .map(|glyph| glyph.codepoint)
            .collect::<String>()
            != text
        {
            return false;
        }
        let Some(clip) = node
            .parent()
            .and_then(|id| nodes.iter().find(|n| n.id() == id))
        else {
            return false;
        };
        let caret = glyphs.last().expect("the caret glyph");
        let bottom = node.bounds().min().y() + caret.bounds_64[3] as f32 / 64.0;
        bottom <= clip.bounds().height()
    })
}

/// The terminal's search field draws the caret after what was typed, as the driver feeds it:
/// one edit pass per frame on an advancing clock.
#[test]
fn a_terminal_search_field_draws_its_caret() {
    let Some(mut presentation) = super::super::super::pack_harness::engine_presentation() else {
        return;
    };
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    let files = Arc::new(files(&[("ui/terminal.json", TERMINAL_SEARCH)]));
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    let inside = Some([(40.0 + 120.0) * scale, (30.0 + 9.0) * scale]);
    let none: Vec<String> = Vec::new();
    let caret = format!("ston{}", json_ui::CARET_GLYPH);
    let mut now = 0.0;
    let mut frame = |presentation: &mut UiPresentationRuntime,
                     modal: &screen::Modal,
                     pressed,
                     typed: &[String]| {
        now += 1.0 / 60.0;
        let edits = presentation
            .mod_ui_mut()
            .edit_experience_modal(inside, pressed, typed, false, now);
        redraw(presentation, modal, &files, [1280, 720]);
        (edits, drawn_texts(presentation))
    };
    frame(&mut presentation, &modal, true, &none);
    let (edits, texts) = frame(&mut presentation, &modal, false, &["ston".to_owned()]);
    assert_eq!(edits.edits, [("demo.search".to_owned(), "ston".to_owned())]);
    assert!(texts.contains(&caret), "{texts:?}");
    assert!(caret_in_clip(&presentation, &caret));
    // The client part answers the edit by binding new values, as a terminal filtering its list
    // does; the box keeps its selection and caret.
    modal.set_value("#shown".into(), screen::Value::Integer(3));
    let (_, texts) = frame(&mut presentation, &modal, false, &none);
    assert!(texts.contains(&caret), "{texts:?}");
}

/// Vanilla's own edit box draws the caret after the typed text too.
#[test]
fn a_selected_vanilla_box_draws_its_caret() {
    let Some(mut presentation) = super::super::super::pack_harness::engine_presentation() else {
        return;
    };
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    let files = Arc::new(files(&[("ui/terminal.json", VANILLA_SEARCH)]));
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    let inside = Some([40.0 * scale, 6.0 * scale]);
    let none: Vec<String> = Vec::new();
    presentation
        .mod_ui_mut()
        .edit_experience_modal(inside, true, &none, false, 0.0);
    presentation.mod_ui_mut().edit_experience_modal(
        inside,
        false,
        &["iron".to_owned()],
        false,
        0.05,
    );
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let texts = drawn_texts(&presentation);
    let caret = format!("iron{}", json_ui::CARET_GLYPH);
    assert!(texts.contains(&caret), "{texts:?}");
}

/// A list 100 GUI units wide whose 50-unit viewport scrolls 400 units of content, at the top
/// left, named `demo.list`, beside an unnamed one; it needs nothing from the vanilla pack.
const LIST: &str = r#"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"list@demo.view": {"scroll_view_name": "demo.list"}},
        {"other@demo.view": {"offset": [0, 100]}}]},
    "view": {"type": "scroll_view", "size": [100, 50],
        "anchor_from": "top_left", "anchor_to": "top_left",
        "scroll_speed": 18, "always_handle_pointer": true,
        "scroll_view_port": "viewport", "scroll_content": "content",
        "scrollbar_track": "track", "scrollbar_box": "box", "scroll_box_and_track_panel": "bar",
        "controls": [
            {"viewport": {"type": "panel", "size": [90, 50],
                "anchor_from": "top_left", "anchor_to": "top_left", "clips_children": true,
                "controls": [{"content": {"type": "panel", "size": [90, 400],
                    "anchor_from": "top_left", "anchor_to": "top_left"}}]}},
            {"bar": {"type": "panel", "size": [10, 50], "offset": [90, 0],
                "anchor_from": "top_left", "anchor_to": "top_left", "controls": [
                {"track": {"type": "scroll_track", "size": [10, 50],
                    "anchor_from": "top_left", "anchor_to": "top_left"}},
                {"box": {"type": "scrollbar_box", "size": [10, 10], "draggable": "vertical",
                    "anchor_from": "top_left", "anchor_to": "top_left"}}]}}]}}"#;

/// A named scroll view reports its range when first drawn and each time it changes, and only
/// then; an unnamed one reports nothing.
#[test]
fn named_scroll_views_report_their_range_on_change() {
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    let files = Arc::new(files(&[("ui/terminal.json", LIST)]));
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let first = presentation.mod_ui().experience_modal_scrolls();
    let [(view, range)] = first.as_slice() else {
        panic!("{first:?}");
    };
    assert_eq!(view, "demo.list");
    assert_eq!(
        (range.offset, range.viewport, range.content),
        (0.0, 50.0, 400.0)
    );
    presentation
        .mod_ui_mut()
        .accept_experience_modal_scroll(view, *range);
    assert!(presentation.mod_ui().experience_modal_scrolls().is_empty());
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    presentation
        .mod_ui_mut()
        .hover_experience_modal(Some([10.0 * scale, 10.0 * scale]));
    presentation.mod_ui_mut().scroll_experience_modal(2.0);
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let scrolled = presentation.mod_ui().experience_modal_scrolls();
    let [(view, range)] = scrolled.as_slice() else {
        panic!("{scrolled:?}");
    };
    assert_eq!((view.as_str(), range.offset), ("demo.list", 36.0));
}

/// A hotbar of item renderers over the host's `hotbar_items`.
const HOTBAR: &str = r##"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"hotbar": {"type": "grid", "size": [180, 20], "grid_dimensions": [9, 1],
            "anchor_from": "top_left", "anchor_to": "top_left",
            "collection_name": "hotbar_items", "grid_item_template": "demo.cell"}}]},
    "cell": {"type": "custom", "renderer": "inventory_item_renderer", "size": [20, 20],
        "bindings": [{"binding_type": "collection", "binding_collection_name": "hotbar_items",
            "binding_name": "#item_renderer_data"}]}}"##;

/// A client part's modal reads the player's inventory as vanilla's container screens do: the
/// host fills `inventory_items` and `hotbar_items` read-only, replacing rows the client part
/// names the same, and their item renderers draw each occupied slot's icon.
#[test]
fn modal_item_renderers_draw_the_players_inventory() {
    use protocol::{
        ContainerIdentity, InventoryEvent, InventorySlotEvent, NetworkItemStack, SlotIdentity,
    };
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    // Rows the client part sends under the host's name point every cell at the first icon.
    modal.set_collection(
        "hotbar_items".into(),
        vec![BTreeMap::from([("#item_renderer_data".to_owned(), screen::Value::Integer(0),)]); 9],
    );
    let files = Arc::new(files(&[("ui/terminal.json", HOTBAR)]));
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.set_experience_modal(Some(ExperienceModal {
        bundle: "demo",
        files: &files,
        modal: &modal,
    }));
    let mut player_runtime = player_state::PlayerState::new(1);
    let stack = NetworkItemStack {
        network_id: 2,
        count: 5,
        stack_network_id: 7,
        ..NetworkItemStack::default()
    };
    player_runtime
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 3,
            },
            stack,
            storage_item: None,
        }));
    let icon = ui::IconRef {
        page: 3,
        uv: [16, 32, 48, 64],
        glint: false,
    };
    presentation.hud_frame.inventory_icons.0[3] = Some(icon);
    let runtime = UiRuntime::new(1);
    let size = [1280, 720];
    let dpi = ui::DpiScale::new(1.0).unwrap();
    let (mut nodes, mut next) = (Vec::new(), 1);
    presentation.append_experience_modal(
        &player_runtime,
        &runtime,
        &mut nodes,
        &mut next,
        TextMetrics::for_viewport(size, dpi, None),
        [1280.0, 720.0],
        true,
    );
    let drawn: Vec<_> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Sprite {
                texture_page, uv, ..
            } => Some((*texture_page, *uv)),
            _ => None,
        })
        .collect();
    assert_eq!(drawn, [(icon.page, icon.uv)]);
}

/// Undelivered ranges remain available, with only the latest drawn range retried.
#[test]
fn named_scroll_views_retry_the_latest_undelivered_range() {
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    let files = Arc::new(files(&[("ui/terminal.json", LIST)]));
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let first = presentation.mod_ui().experience_modal_scrolls();
    assert_eq!(first.len(), 1);
    assert_eq!(
        presentation.mod_ui().experience_modal_scrolls(),
        first,
        "an unaccepted initial range must be retried"
    );
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    presentation
        .mod_ui_mut()
        .hover_experience_modal(Some([10.0 * scale, 10.0 * scale]));
    presentation.mod_ui_mut().scroll_experience_modal(2.0);
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let latest = presentation.mod_ui().experience_modal_scrolls();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].1.offset, 36.0);
    assert_eq!(
        presentation.mod_ui().experience_modal_scrolls(),
        latest,
        "a rejected changed range must replace the initial range on retry"
    );
    presentation
        .mod_ui_mut()
        .accept_experience_modal_scroll(&first[0].0, first[0].1);
    assert_eq!(presentation.mod_ui().experience_modal_scrolls(), latest);
    presentation
        .mod_ui_mut()
        .accept_experience_modal_scroll(&latest[0].0, latest[0].1);
    assert!(presentation.mod_ui().experience_modal_scrolls().is_empty());
}

/// Repeated instances of one named view report their latest range without alternating callbacks.
#[test]
fn repeated_scroll_view_names_keep_one_stable_range() {
    let template = LIST.replace(
        r#""other@demo.view": {"offset": [0, 100]}"#,
        r#""other@demo.view": {"offset": [0, 100], "scroll_view_name": "demo.list"}"#,
    );
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    let files = Arc::new(files(&[("ui/terminal.json", &template)]));
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let first = presentation.mod_ui().experience_modal_scrolls();
    assert_eq!(first.len(), 1);
    presentation
        .mod_ui_mut()
        .accept_experience_modal_scroll(&first[0].0, first[0].1);
    let scale = presentation.mod_ui().experience_modal_size().unwrap().scale as f32;
    presentation
        .mod_ui_mut()
        .hover_experience_modal(Some([10.0 * scale, 10.0 * scale]));
    presentation.mod_ui_mut().scroll_experience_modal(2.0);
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    assert!(
        presentation.mod_ui().experience_modal_scrolls().is_empty(),
        "the latest instance still has its accepted range"
    );
}
