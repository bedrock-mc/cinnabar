use super::*;
use json_ui::{
    Catalog, CatalogLibrary, Draw, LayoutEnv, Rect, TextMeasure, TextureMeta, TextureSource,
};

struct Resources;

impl TextMeasure for Resources {
    /// Gives each visible character six pixels without requiring a font carrier.
    fn extent(&self, text: &str) -> [f64; 2] {
        let plain = ui::parse_bedrock_text(text, text.len())
            .unwrap()
            .plain_text();
        [plain.chars().count() as f64 * 6.0, 9.0]
    }
}

impl TextureSource for Resources {
    /// Missing artwork uses the renderer's full-rectangle diagnostic texture.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Binds the shipped chat layout to anonymous editor and completion data.
fn chat(
    draft: &str,
    cursor: usize,
    suggestions: &[&str],
    history_width: f64,
) -> json_ui::ResolvedControl {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/chat_base.json",
        r##"{
            "namespace": "chat",
            "messages_scrolling_panel": { "type": "panel" },
            "commands_panel": {
                "type": "panel", "anchor_from": "bottom_left", "anchor_to": "bottom_left",
                "controls": [{ "grid": {
                    "type": "grid", "collection_name": "auto_complete",
                    "grid_item_template": "chat.auto_complete_panel_contents_with_item",
                    "grid_rescaling_type": "horizontal", "size": ["100%", "default"],
                    "bindings": [{ "binding_name": "#collection_total_items",
                        "binding_name_override": "#maximum_grid_items", "binding_type": "collection",
                        "binding_collection_name": "auto_complete" }]
                }}]
            },
            "auto_complete_panel_contents_with_item": {
                "type": "panel", "size": ["100%", 10],
                "controls": [{ "text": {
                    "type": "label", "text": "#auto_complete_text", "localize": false,
                    "bindings": [{ "binding_name": "#auto_complete_text", "binding_type": "collection",
                        "binding_collection_name": "auto_complete" }]
                }}]
            }
        }"##,
    );
    let (path, _, bytes) = crate::hud::JAVA_HUD_PACK
        .iter()
        .find(|(_, namespace, _)| *namespace == "chat")
        .unwrap();
    catalog.overlay_text(path, std::str::from_utf8(bytes).unwrap());
    let context = crate::menu_screens::retail_context()
        .with_var("cinnabar_chat_width", Value::from(history_width));
    let root = json_ui::resolve(&catalog, "chat.java_chat_content", &context)
        .control
        .unwrap();
    let mut editor = ui::ChatEditor::new(ui::MAX_CHAT_INPUT_BYTES).unwrap();
    editor.set_text(draft);
    editor.place_cursor(cursor);
    let suggestions: Vec<_> = suggestions.iter().map(|text| Arc::from(*text)).collect();
    let data = chat_data(
        &ChatInput {
            editor: &editor,
            messages: &VecDeque::new(),
            suggestions: &suggestions,
            usage: None,
            selected_suggestion: None,
            annotations: &[],
            clocks: &BTreeMap::new(),
            now_millis: 0,
        },
        &ChatSettings::default(),
        &|_| None,
    );
    json_ui::bind(
        &root,
        &data,
        &CatalogLibrary {
            catalog: &catalog,
            context: &context,
        },
    )
}

/// Returns the painted black backdrops whose fixed opacity identifies their role.
fn backgrounds(control: &json_ui::ResolvedControl, viewport: [f64; 2], alpha: f32) -> Vec<Rect> {
    let env = LayoutEnv {
        text: &Resources,
        textures: &Resources,
    };
    json_ui::emit(&json_ui::layout(control, viewport, &env), &env)
        .into_iter()
        .filter_map(|node| {
            (matches!(&node.draw, Draw::Sprite { texture, .. } if texture == "textures/ui/Black")
                && (node.alpha - alpha).abs() < f32::EPSILON)
                .then_some(Rect::new(
                    node.dest.x,
                    node.dest.y,
                    node.dest.w,
                    node.dest.h,
                ))
        })
        .collect()
}

#[test]
fn input_background_spans_the_viewport_independently_of_history_width() {
    for viewport in [[360.0, 240.0], [640.0, 360.0], [960.0, 540.0]] {
        for history_width in [180.0, 330.0] {
            let root = chat("", 0, &[], history_width);
            assert_eq!(
                backgrounds(&root, viewport, 128.0 / 255.0),
                [Rect::new(2.0, viewport[1] - 14.0, viewport[0] - 4.0, 12.0)]
            );
        }
    }
}

#[test]
fn suggestion_rows_fit_the_longest_text_at_the_completion_start() {
    for (draft, cursor, prefix, longest) in [
        ("/", 1, "/", "@Charlotte"),
        ("/give @ tail", 7, "/give ", "@Charlotte"),
        ("hello @Al", 9, "hello ", "Charlotte"),
    ] {
        let root = chat(draft, cursor, &["@Alex", "@Charlotte", "@Bo"], 180.0);
        let rects = backgrounds(&root, [640.0, 360.0], 208.0 / 255.0);
        let width = Resources.extent(longest)[0] + 1.0;
        let left = Resources.extent(prefix)[0] + 3.0;
        assert_eq!(rects.len(), 3);
        for rect in &rects {
            assert_eq!((rect.x, rect.w, rect.h), (left, width, 12.0));
        }
        assert_eq!(rects.last().unwrap().y + 12.0, 360.0 - 15.0);
    }
}

#[test]
fn suggestion_rows_shift_left_to_fit_at_the_right_edge() {
    let draft = format!("/say {} ", "x".repeat(100));
    let root = chat(&draft, draft.len(), &["longest", "x"], 330.0);
    let rects = backgrounds(&root, [360.0, 240.0], 208.0 / 255.0);
    assert_eq!(rects.len(), 2);
    for rect in rects {
        assert_eq!(rect.w, Resources.extent("longest")[0] + 1.0);
        assert_eq!(rect.x + rect.w, 360.0);
    }
}
