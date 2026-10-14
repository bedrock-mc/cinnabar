//! Which presses on an engine-drawn container drop the held stack.

use json_ui::{Catalog, Context, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState};
use ui_engine::frame::EngineFrame;

use super::engine_press_drops;

struct NoArt;
impl TextMeasure for NoArt {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}
impl TextureSource for NoArt {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// A screen shaped like vanilla's: the screen maps presses to a cursor drop,
/// two `common.root_panel`-style sections claim them, and a plain panel and a
/// `drop_item_panel` sit beside them.
const SCREEN: &str = r#"{
  "namespace": "drop",
  "section": {
    "type": "input_panel", "size": [40, 40],
    "anchor_from": "top_left", "anchor_to": "top_left",
    "button_mappings": [{"from_button_id": "button.menu_select",
      "to_button_id": "button.container_reset_held", "mapping_type": "pressed"}]
  },
  "screen": {
    "type": "screen",
    "button_mappings": [{"from_button_id": "button.menu_select",
      "to_button_id": "button.cursor_drop_all", "mapping_type": "pressed"}],
    "controls": [
      {"root_panel@drop.section": {"offset": [0, 0]}},
      {"recipe_book@drop.section": {"offset": [50, 0]}},
      {"background": {"type": "panel", "size": [40, 40],
        "anchor_from": "top_left", "anchor_to": "top_left", "offset": [100, 0]}},
      {"drop_item_panel": {"type": "input_panel", "size": [40, 40],
        "anchor_from": "top_left", "anchor_to": "top_left", "offset": [150, 0],
        "button_mappings": [{"from_button_id": "button.menu_select",
          "to_button_id": "button.cursor_drop_all", "mapping_type": "pressed"}]}}
    ]
  }
}"#;

fn frame() -> EngineFrame {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/drop.json", SCREEN);
    let screen = json_ui::resolve(&catalog, "drop.screen", &Context::desktop())
        .control
        .expect("the screen resolves");
    let (laid, report) = json_ui::layout_with(
        &screen,
        [200.0, 100.0],
        &LayoutEnv {
            text: &NoArt,
            textures: &NoArt,
        },
        &ViewState::default(),
    );
    EngineFrame {
        hits: json_ui::hit_regions(&laid).into(),
        report,
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    }
}

#[test]
fn presses_on_any_section_keep_the_held_stack() {
    let frame = frame();
    assert!(!engine_press_drops(&frame, [10.0, 10.0]), "root panel");
    assert!(!engine_press_drops(&frame, [60.0, 10.0]), "recipe book");
}

#[test]
fn presses_off_the_sections_drop_the_held_stack() {
    let frame = frame();
    assert!(engine_press_drops(&frame, [110.0, 10.0]), "plain panel");
    assert!(engine_press_drops(&frame, [160.0, 10.0]), "drop panel");
    assert!(engine_press_drops(&frame, [10.0, 60.0]), "empty screen");
}
