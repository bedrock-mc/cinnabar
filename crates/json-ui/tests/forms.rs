//! End-to-end form rendering against the real vanilla `server_form.json` templates.
//! The `.local` pack is gitignored, so each test skips (not fails) when it is absent.
//! Assertions are structural (instance counts, order, presence of image/text nodes,
//! content sizing) — never pixels.

mod support;

use json_ui::{
    ActionElement, ActionForm, ButtonImage, Catalog, Context, CustomElement, CustomForm, Draw,
    DrawNode, FormButton, FormModel, HitKind, LaidOut, LayoutEnv, ModalForm, ResolvedControl,
    TextMeasure, TextureMeta, TextureSource, ViewState, layout, render_form, render_form_with,
};

struct ZeroText;
impl TextMeasure for ZeroText {
    fn extent(&self, _text: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}

struct NoTextures;
impl TextureSource for NoTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    }
}

fn catalog() -> Option<Catalog> {
    let dir = support::vanilla_pack().join("ui");
    dir.is_dir()
        .then(|| Catalog::load_dir(&dir).expect("index files load"))
}

/// Depth-first search for the first descendant (or self) with `name`.
fn find<'a>(control: &'a ResolvedControl, name: &str) -> Option<&'a ResolvedControl> {
    control.find(&|node| node.name == name)
}

fn find_laid<'a>(root: &'a LaidOut<'a>, name: &str) -> Option<&'a LaidOut<'a>> {
    if root.control.name == name {
        return Some(root);
    }
    root.children
        .iter()
        .find_map(|child| find_laid(child, name))
}

fn texts(nodes: &[DrawNode]) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Textures of every sprite emitted by a control instance named `name`.
fn sprite_textures(nodes: &[DrawNode], name: &str) -> Vec<String> {
    nodes
        .iter()
        .filter(|node| node.name == name)
        .filter_map(|node| match &node.draw {
            Draw::Sprite { texture, .. } => Some(texture.clone()),
            _ => None,
        })
        .collect()
}

const ROOT: [f64; 2] = [512.0, 384.0];

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn action_form_renders_a_button_per_entry_with_present_images_only() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let button = |text: &str, image: Option<&str>| {
        ActionElement::Button(FormButton {
            text: text.into(),
            image: image.map(|path| ButtonImage::Path(path.into())),
        })
    };
    let model = FormModel::Action(ActionForm {
        title: "Shop".into(),
        body: "Choose an item".into(),
        elements: vec![
            button("Apple", Some("textures/items/apple")),
            button("Sword", Some("textures/items/sword")),
            button("Plain", None),
        ],
    });

    let render = render_form(&model, &catalog, &Context::desktop(), ROOT, &env())
        .expect("long_form resolves");

    // The factory instantiates one control per collection index.
    let panel = find(&render.bound, "long_form_dynamic_buttons_panel").expect("buttons panel");
    assert_eq!(panel.children.len(), 3, "one button instance per entry");

    // Every button label reaches the draw tree.
    let drawn = texts(&render.nodes);
    assert!(
        drawn.iter().any(|text| text == "Shop"),
        "missing form title"
    );
    for label in ["Apple", "Sword", "Plain"] {
        assert!(
            drawn.iter().any(|t| t == label),
            "missing button text {label}"
        );
    }

    // Only the two buttons with images emit an `image` sprite, and with their paths.
    let mut images = sprite_textures(&render.nodes, "image");
    images.sort();
    images.dedup();
    assert_eq!(
        images,
        vec![
            "textures/items/apple".to_owned(),
            "textures/items/sword".to_owned()
        ],
        "the imageless button emits no image sprite"
    );
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn modal_form_renders_through_the_two_button_popup() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let model = FormModel::Modal(ModalForm {
        title: "Confirm".into(),
        body: "Delete the world?".into(),
        button1: "Yes".into(),
        button2: "No".into(),
    });

    let render = render_form(&model, &catalog, &Context::desktop(), ROOT, &env())
        .expect("modal renders via the popup");

    let drawn = texts(&render.nodes);
    for text in ["Confirm", "Delete the world?", "Yes", "No"] {
        assert!(
            drawn.iter().any(|t| t == text),
            "missing {text} in {drawn:?}"
        );
    }
    // button1 routes to the left button, button2 to the right/cancel button.
    let pressed: Vec<&str> = render
        .hits
        .iter()
        .filter_map(|hit| hit.pressed.as_deref())
        .collect();
    assert!(pressed.contains(&"popup_dialog.left_button"));
    assert!(pressed.contains(&"popup_dialog.rightcancel_button"));
    assert_eq!(render.cancel_target.as_deref(), Some("popup_dialog.escape"));
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn action_form_buttons_report_their_collection_index() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let model = FormModel::Action(ActionForm {
        title: "Menu".into(),
        body: String::new(),
        elements: vec![
            ActionElement::Label("Intro".into()),
            ActionElement::Button(FormButton {
                text: "Go".into(),
                image: None,
            }),
        ],
    });
    let render = render_form(&model, &catalog, &Context::desktop(), ROOT, &env()).unwrap();
    let clicks: Vec<Option<usize>> = render
        .hits
        .iter()
        .filter(|hit| hit.pressed.as_deref() == Some("button.form_button_click"))
        .map(|hit| hit.collection_index)
        .collect();
    assert_eq!(
        clicks,
        [Some(1)],
        "the button is the second collection entry"
    );
    assert!(
        render
            .hits
            .iter()
            .any(|hit| hit.pressed.as_deref() == Some("button.menu_exit")),
        "the dialog close button exits"
    );
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn hovering_a_button_swaps_its_state_child() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let model = FormModel::Action(ActionForm {
        title: "Menu".into(),
        body: String::new(),
        elements: vec![ActionElement::Button(FormButton {
            text: "Go".into(),
            image: None,
        })],
    });
    let idle = render_form(&model, &catalog, &Context::desktop(), ROOT, &env()).unwrap();
    let button = idle
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("button.form_button_click"))
        .expect("form button hit")
        .clone();
    let state = ViewState {
        hovered: Some(button.key.clone()),
        ..ViewState::default()
    };
    let hovered =
        render_form_with(&model, &catalog, &Context::desktop(), ROOT, &env(), &state).unwrap();
    let under = |render: &json_ui::FormRender| -> Vec<String> {
        render
            .nodes
            .iter()
            .filter(|node| node.key.starts_with(&button.key))
            .filter_map(|node| match &node.draw {
                Draw::Sprite { texture, .. } => Some(texture.clone()),
                _ => None,
            })
            .collect()
    };
    assert_ne!(
        under(&idle),
        under(&hovered),
        "hover shows a different skin"
    );
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn custom_form_renders_elements_in_order_with_a_submit_button() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let model = FormModel::Custom(CustomForm {
        title: "Options".into(),
        elements: vec![
            CustomElement::Label {
                text: "Intro".into(),
            },
            CustomElement::Toggle {
                text: "Sound".into(),
                on: true,
                tooltip: String::new(),
            },
            CustomElement::Slider {
                text: "Volume: 5".into(),
                fraction: 0.5,
                tooltip: String::new(),
            },
            CustomElement::Dropdown {
                text: "Mode".into(),
                options: vec!["A".into(), "B".into()],
                index: 0,
                open: false,
                tooltip: String::new(),
            },
            CustomElement::Input {
                text: "Name".into(),
                value: String::new(),
                placeholder: "type".into(),
                tooltip: String::new(),
            },
        ],
        submit_text: "Submit".into(),
        submit_visible: true,
    });

    let render = render_form(&model, &catalog, &Context::desktop(), ROOT, &env())
        .expect("custom_form resolves");

    let generated = find(&render.bound, "generated_form").expect("generated form factory");
    assert!(
        texts(&render.nodes).iter().any(|text| text == "Options"),
        "missing custom title"
    );
    let names: Vec<&str> = generated
        .children
        .iter()
        .map(|child| child.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "custom_label",
            "custom_toggle",
            "custom_slider",
            "custom_dropdown",
            "custom_input"
        ],
        "the factory selects each element's control in wire order"
    );

    let submit = find(&render.bound, "submit_button").expect("submit button present");
    assert_eq!(
        submit.properties.get("visible"),
        Some(&serde_json::json!(true)),
        "#submit_button_visible drives the submit button"
    );
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn custom_form_hides_the_submit_button_when_not_visible() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let model = FormModel::Custom(CustomForm {
        title: "Options".into(),
        elements: vec![CustomElement::Label {
            text: "Intro".into(),
        }],
        submit_text: "Submit".into(),
        submit_visible: false,
    });
    let render = render_form(&model, &catalog, &Context::desktop(), ROOT, &env()).unwrap();
    let submit = find(&render.bound, "submit_button").expect("submit button present");
    assert_eq!(
        submit.properties.get("visible"),
        Some(&serde_json::json!(false))
    );
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn scroll_content_height_grows_with_the_button_collection() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let height = |count: usize| {
        let elements = (0..count)
            .map(|i| {
                ActionElement::Button(FormButton {
                    text: format!("Button {i}"),
                    image: None,
                })
            })
            .collect();
        let model = FormModel::Action(ActionForm {
            title: "Menu".into(),
            body: String::new(),
            elements,
        });
        let render = render_form(&model, &catalog, &Context::desktop(), ROOT, &env()).unwrap();
        let laid = layout(&render.bound, ROOT, &env());
        find_laid(&laid, "long_form_dynamic_buttons_panel")
            .expect("buttons panel laid out")
            .rect
            .h
    };

    // Each dynamic_button is 32 tall and the panel sizes to its `100%c` content.
    assert_eq!(height(1), 32.0);
    assert_eq!(height(3), 96.0);
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn long_forms_report_a_scrollable_viewport() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let elements = (0..20)
        .map(|i| {
            ActionElement::Button(FormButton {
                text: format!("Button {i}"),
                image: None,
            })
        })
        .collect();
    let model = FormModel::Action(ActionForm {
        title: "Menu".into(),
        body: String::new(),
        elements,
    });
    let render = render_form(&model, &catalog, &Context::desktop(), ROOT, &env()).unwrap();
    let scroll = render
        .report
        .scrolls
        .values()
        .next()
        .expect("the long form scrolls");
    assert!(scroll.content > scroll.viewport, "20 buttons overflow");
    assert!(scroll.thumb.is_some(), "an overflowing view shows its box");
    assert!(
        render
            .hits
            .iter()
            .any(|hit| hit.kind == HitKind::ScrollView),
        "the scroll view takes wheel input"
    );
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn custom_toggle_reports_its_name_and_index() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let model = FormModel::Custom(CustomForm {
        title: "Options".into(),
        elements: vec![
            CustomElement::Label {
                text: "Intro".into(),
            },
            CustomElement::Toggle {
                text: "Sound".into(),
                on: false,
                tooltip: String::new(),
            },
        ],
        submit_text: "Submit".into(),
        submit_visible: true,
    });
    let render = render_form(&model, &catalog, &Context::desktop(), ROOT, &env()).unwrap();
    let toggle = render
        .hits
        .iter()
        .find(|hit| hit.kind == HitKind::Toggle)
        .expect("toggle hit");
    assert_eq!(toggle.control_name.as_deref(), Some("custom_toggle"));
    assert_eq!(toggle.collection_index, Some(1));
    assert_eq!(toggle.checked, Some(false));
    assert!(
        render
            .hits
            .iter()
            .any(|hit| hit.pressed.as_deref() == Some("button.submit_custom_form"))
    );
}

// A pack's gamepad focus outline, bound to `#is_using_gamepad`, stays hidden in a
// pointer-driven form instead of framing every hovered button.
#[test]
fn gamepad_only_chrome_stays_hidden_in_forms() {
    let screen = br##"{
        "namespace": "s",
        "card": { "type": "panel", "size": [20, 20], "controls": [
            { "outline": { "type": "image", "texture": "textures/ui/outline",
                "bindings": [ { "binding_type": "global", "binding_name": "#is_using_gamepad",
                    "binding_name_override": "#visible" } ] } } ] }
    }"##;
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/s.json"]}"#.as_slice(),
        ),
        ("ui/s.json", screen.as_slice()),
    ])
    .unwrap();
    let card = json_ui::resolve(&catalog, "s.card", &Context::desktop())
        .control
        .unwrap();
    let model = FormModel::Action(ActionForm {
        title: "Menu".into(),
        body: String::new(),
        elements: Vec::new(),
    });
    let data = json_ui::form_data_source(&model);
    let bound = json_ui::bind(&card, &data, &json_ui::EmptyLibrary);
    let outline = find(&bound, "outline").unwrap();
    assert_eq!(
        outline.properties.get("visible"),
        Some(&serde_json::Value::Bool(false))
    );
}
