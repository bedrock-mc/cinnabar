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

    /// Wraps fixed-width characters at the label's available width.
    fn wrapped(&self, text: &str, max_width: f64) -> [f64; 2] {
        let [width, height] = self.extent(text);
        [
            width.min(max_width),
            height * (width / max_width).ceil().max(1.0),
        ]
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
            local_player: "Alex",
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

/// Supplies only the history templates that the built-in pack inherits from its carrier.
fn history_catalog() -> Catalog {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/history_base.json",
        r##"{
            "namespace": "chat",
            "messages_scrolling_panel": {
                "type": "panel",
                "controls": [{"rows": {
                    "type": "stack_panel", "size": ["100%", "100%c"],
                    "factory": {"name": "messages_factory",
                        "control_ids": {"chat_screen_messages": "chat.messages_text"}}
                }}]
            },
            "messages_text": {
                "type": "panel", "size": ["100%", "100%c"],
                "controls": [{"text": {
                    "type": "label", "text": "#text", "localize": false,
                    "bindings": [{"binding_name": "#text"}]
                }}]
            }
        }"##,
    );
    catalog.overlay_text(
        "ui/hud_base.json",
        r##"{"namespace": "hud", "chat_label": {
            "type": "label", "text": "#text", "localize": false,
            "bindings": [{"binding_name": "#text"}]
        }}"##,
    );
    for (path, namespace, bytes) in crate::hud::JAVA_HUD_PACK {
        let text = std::str::from_utf8(bytes).unwrap();
        if namespace.is_empty() {
            catalog.overlay_globals_text(text);
        } else {
            catalog.overlay_text(path, text);
        }
    }
    catalog
}

#[test]
fn history_width_is_fixed_in_gui_units_for_hud_and_open_chat() {
    let catalog = history_catalog();
    let env = LayoutEnv {
        text: &Resources,
        textures: &Resources,
    };
    let text = "x".repeat(53);
    let mut data = DataSource::new();
    for name in ["#hud_visible", "#chat_visible", "#hud_propagate_alpha"] {
        data.set_global(name, Scalar::Bool(true));
    }
    data.set_global("#hud_alpha", Scalar::Num(1.0));
    data.set_global("#chat_background_opacity", Scalar::Num(0.5));
    for (factory, id) in [
        ("chat_item_factory", "chat_item"),
        ("messages_factory", "chat_screen_messages"),
    ] {
        data.set_factory(
            factory,
            ["short", text.as_str()]
                .map(|text| FactoryItem::new(id, 0.0).value("#text", Scalar::Text(text.into())))
                .into(),
        );
    }
    for (physical, preference, expected_width) in [
        ([360, 240], None, 320.0),
        ([854, 459], None, 320.0),
        ([1280, 720], None, 640.0),
        ([1920, 1080], None, 1280.0),
        ([2560, 1440], None, 1600.0),
        ([1280, 720], Some(1), 320.0),
        ([1920, 1080], Some(2), 640.0),
    ] {
        let scale = f64::from(ui::gui_scale(physical, preference));
        let viewport = physical.map(|side| f64::from(side) / scale);
        for top in [false, true] {
            let context = crate::chat_position::placed(crate::menu_screens::retail_context(), top);
            for reference in [
                if top { "hud.top_chat" } else { "hud.java_chat" },
                "chat.java_chat_content",
            ] {
                let root = json_ui::resolve(&catalog, reference, &context)
                    .control
                    .unwrap();
                let bound = json_ui::bind(
                    &root,
                    &data,
                    &CatalogLibrary {
                        catalog: &catalog,
                        context: &context,
                    },
                );
                let nodes = json_ui::emit(&json_ui::layout(&bound, viewport, &env), &env);
                let backgrounds: Vec<_> = nodes
                    .iter()
                    .filter(|node| {
                        node.name == "chat_background"
                            || node.name == "background" && node.key.contains("messages_panel")
                    })
                    .collect();
                assert_eq!(backgrounds.len(), 2, "{reference}");
                for node in &backgrounds {
                    assert_eq!(
                        node.dest.w * scale,
                        expected_width,
                        "{reference} at {physical:?}, {preference:?}"
                    );
                }
                let labels: Vec<_> = nodes
                    .iter()
                    .filter(|node| {
                        matches!(&node.draw, Draw::Text { text: value, .. } if value == &text)
                    })
                    .collect();
                assert_eq!(labels.len(), 1);
                assert_eq!(labels[0].dest.w * scale, expected_width - 8.0 * scale);
                assert!(labels[0].dest.h > 9.0, "the long message must wrap");
            }
        }
    }
}
