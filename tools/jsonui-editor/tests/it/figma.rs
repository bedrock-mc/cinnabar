//! Figma conversion: each fixture is a serialized frame as the plugin sends it;
//! its pack must validate, and laying it out at the frame's size must put
//! every layer where Figma drew it.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

use json_ui::Draw;
use jsonui_editor::export::{self, Format, Mode, PackInfo};
use jsonui_editor::figma::{self, Conversion, Node};
use jsonui_editor::{Frame, Session, View};
use serde_json::{Value, json};

fn fixture(name: &str) -> Node {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/figma/{name}.json"));
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
}

fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn solid_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    png(width, height, |_, _| rgba)
}

/// The generated UI file as JSON.
fn ui(conversion: &Conversion) -> Value {
    serde_json::from_slice(&conversion.files[&conversion.ui_path()]).unwrap()
}

/// The body of the control named `name` anywhere in `value`.
fn control<'a>(value: &'a Value, name: &str) -> &'a Value {
    find(value, name).unwrap_or_else(|| panic!("no control `{name}` in {value:#}"))
}

fn find<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map.iter().find_map(|(key, body)| {
            let own = key.split('@').next() == Some(name) && body.is_object();
            if own { Some(body) } else { find(body, name) }
        }),
        Value::Array(items) => items.iter().find_map(|item| find(item, name)),
        _ => None,
    }
}

/// Loads `conversion`, asserts it validates cleanly, and lays it out at `scale`.
fn laid_out(conversion: &Conversion, scale: u8) -> (Session, View, std::sync::Arc<Frame>) {
    let mut session = Session::default();
    let view = conversion.load(&mut session, scale);
    let diagnostics = conversion.validate(&mut session);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let frame = session.frame(&view);
    assert!(frame.diagnostics.is_empty(), "{:#?}", frame.diagnostics);
    (session, view, frame)
}

fn rect(frame: &Frame, name: &str) -> [f64; 4] {
    let found = frame.boxes.iter().find(|laid| laid.name == name);
    found
        .unwrap_or_else(|| panic!("no box {name}"))
        .rect
        .map(|v| (v * 100.0).round() / 100.0)
}

fn messages(conversion: &Conversion) -> Vec<(String, String)> {
    let warnings = conversion.warnings.iter();
    warnings
        .map(|w| (w.layer.clone(), w.message.clone()))
        .collect()
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    rgba[at..at + 4].try_into().unwrap()
}

fn near(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 4)
}

// Constraints become anchors: the frame lays out as drawn, and resizing it
// moves and stretches each layer the way Figma's constraints do.
#[test]
fn constraints_place_frames_and_follow_resizes() {
    let conversion = figma::convert(&fixture("constraints"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    assert_eq!(conversion.screen, "layout_test.layout_test_screen");
    let ui = ui(&conversion);
    assert_eq!(control(&ui, "bottom_right")["anchor_from"], "bottom_right");
    assert_eq!(control(&ui, "inner")["clips_children"], true);
    let (mut session, mut view, frame) = laid_out(&conversion, 2);
    assert_eq!(
        view.gui_scale,
        Some(1),
        "below Bedrock's minimum window only scale 1 is 1:1"
    );
    for (name, expected) in [
        ("top_left", [10.0, 5.0, 40.0, 20.0]),
        ("bottom_right", [150.0, 70.0, 40.0, 20.0]),
        ("centered", [80.0, 40.0, 40.0, 20.0]),
        ("stretched", [10.0, 30.0, 180.0, 10.0]),
        ("scaled", [50.0, 50.0, 100.0, 10.0]),
        ("group", [20.0, 60.0, 30.0, 30.0]),
        ("inner", [25.0, 65.0, 10.0, 10.0]),
    ] {
        assert_eq!(rect(&frame, name), expected, "{name}");
    }
    view.size = [400, 200];
    let frame = session.frame(&view);
    for (name, expected) in [
        ("top_left", [10.0, 5.0, 40.0, 20.0]),
        ("bottom_right", [350.0, 170.0, 40.0, 20.0]),
        ("centered", [180.0, 90.0, 40.0, 20.0]),
        ("stretched", [10.0, 30.0, 380.0, 10.0]),
        ("scaled", [100.0, 100.0, 200.0, 20.0]),
    ] {
        assert_eq!(rect(&frame, name), expected, "{name} at 400x200");
    }
}

// Text keeps its font, size, colour and alignment; what labels cannot express warns.
#[test]
fn text_becomes_labels() {
    let conversion = figma::convert(&fixture("text"), &BTreeMap::new());
    let ui = ui(&conversion);
    let title = control(&ui, "title");
    assert_eq!(title["type"], "label");
    assert_eq!(title["text"], "Hello");
    assert_eq!(title["font_type"], "MinecraftTen");
    assert_eq!(title["font_scale_factor"], 2);
    assert_eq!(title["text_alignment"], "center");
    assert_eq!(title["color"], json!([1, 0.8, 0.2]));
    assert_eq!(title["size"], json!(["100%", "default"]));
    let body = control(&ui, "body");
    assert_eq!(body["type"], "panel", "vertical alignment wraps the label");
    let label = control(body, "text");
    assert_eq!(label["alpha"], 0.5);
    assert!(
        label.get("font_type").is_none(),
        "Minecraft is the default font"
    );
    assert_eq!(control(&ui, "fancy")["font_type"], "smooth");
    let warned: Vec<String> = messages(&conversion)
        .into_iter()
        .map(|(layer, message)| {
            assert_eq!(layer, "Text Test / Fancy");
            message
        })
        .collect();
    for expected in [
        "mixed fontSize",
        "font `Inter`",
        "font style `Bold`",
        "letter spacing",
    ] {
        assert!(
            warned.iter().any(|m| m.contains(expected)),
            "{expected}: {warned:?}"
        );
    }
    let (_, _, frame) = laid_out(&conversion, 1);
    assert_eq!(rect(&frame, "title")[..3], [0.0, 4.0, 160.0]);
    let text = rect(&frame, "text");
    assert_eq!(text[1] + text[3] / 2.0, 55.0, "centred in its 30 px box");
}

// Image fills and `@image` exports ship as PNG textures shared by key;
// scale modes map to `fill` and `keep_ratio`.
#[test]
fn image_fills_and_rasters_export_textures() {
    let mut jpeg = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(4, 4, image::Rgb([200, 10, 10]))
        .write_to(&mut jpeg, image::ImageFormat::Jpeg)
        .unwrap();
    let images = BTreeMap::from([
        ("backdrop".to_owned(), solid_png(4, 2, [255, 0, 0, 255])),
        ("photo".to_owned(), solid_png(2, 2, [0, 255, 0, 255])),
        ("node:3:4".to_owned(), solid_png(16, 16, [0, 0, 255, 255])),
        ("jpeg".to_owned(), jpeg.into_inner()),
    ]);
    let conversion = figma::convert(&fixture("images"), &images);
    let ui = ui(&conversion);
    let background = control(&ui, "background");
    assert_eq!(background["texture"], "textures/image_test/image_test");
    assert_eq!(background["keep_ratio"], true);
    let photo = control(&ui, "photo");
    assert_eq!(photo["texture"], "textures/image_test/photo");
    assert_eq!(
        (&photo["fill"], &photo["alpha"]),
        (&json!(true), &json!(0.5))
    );
    let copy = control(&ui, "photo_copy");
    assert_eq!(copy["texture"], photo["texture"], "one texture per image");
    assert_eq!(copy["keep_ratio"], false);
    let icon = control(&ui, "icon");
    assert_eq!(icon["texture"], "textures/image_test/icon");
    assert_eq!(control(&ui, "missing")["type"], "panel");
    let textures: Vec<&String> = conversion
        .files
        .keys()
        .filter(|path| path.starts_with("textures/"))
        .collect();
    assert_eq!(textures.len(), 4, "{textures:?}");
    for path in textures {
        assert!(
            conversion.files[path].starts_with(b"\x89PNG"),
            "{path} is a PNG"
        );
    }
    assert_eq!(
        messages(&conversion),
        [
            (
                "Image Test / Photo Copy".to_owned(),
                "crop image scaling is not supported; the image stretches".to_owned()
            ),
            (
                "Image Test / Missing".to_owned(),
                "the image's data is missing".to_owned()
            ),
        ],
        "the export already holds the vector's stroke"
    );
    let (mut session, _, frame) = laid_out(&conversion, 1);
    let rgba = session.paint(&frame, 0.0).unwrap();
    assert!(near(pixel(&rgba, 100, 18, 68), [0, 0, 255, 255]));
    assert!(near(pixel(&rgba, 100, 75, 25), [0, 255, 0, 255]));
}

// Solid rectangles tint a generated white texture; stacked fills and empty
// rectangles become panels.
#[test]
fn solid_rectangles_become_tinted_images() {
    let conversion = figma::convert(&fixture("solids"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let ui = ui(&conversion);
    let red = control(&ui, "red");
    assert_eq!(red["type"], "image");
    assert_eq!(red["texture"], "textures/solid_test/solid");
    assert_eq!(red["color"], json!([1, 0, 0]));
    assert_eq!(red["alpha"], 0.25, "fill and layer opacity multiply");
    assert_eq!(red["keep_ratio"], false);
    assert!(control(&ui, "spacer").get("controls").is_none());
    let layers = control(&ui, "layers");
    assert_eq!(layers["type"], "panel");
    assert_eq!(control(layers, "fill_2")["alpha"], 0.25);
    assert!(
        conversion
            .files
            .contains_key("textures/solid_test/solid.png")
    );
    let (mut session, _, frame) = laid_out(&conversion, 1);
    let rgba = session.paint(&frame, 0.0).unwrap();
    let [r, g, b, _] = pixel(&rgba, 100, 25, 25);
    assert!(
        (50..=80).contains(&r) && g < 10 && b < 10,
        "red at 25% over black: {r} {g} {b}"
    );
    let [r, g, b, _] = pixel(&rgba, 100, 80, 25);
    assert!(
        r < 10 && (50..=80).contains(&g) && (175..=205).contains(&b),
        "green over blue: {r} {g} {b}"
    );
}

// Auto-layout becomes stack panels with spacers, a padded wrapper and child
// anchors, laying out where Figma placed every child.
#[test]
fn auto_layout_becomes_stack_panels() {
    let conversion = figma::convert(&fixture("auto_layout"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let ui = ui(&conversion);
    let column = control(&ui, "column");
    assert_eq!(column["type"], "panel", "padding and a fill need a wrapper");
    let content = control(column, "content");
    assert_eq!(content["type"], "stack_panel");
    assert_eq!(content["use_child_anchors"], true);
    assert_eq!(content["size"], json!(["100% - 10px", "100% - 12px"]));
    let row = control(&ui, "row");
    assert_eq!(
        (&row["type"], &row["orientation"]),
        (&json!("stack_panel"), &json!("horizontal"))
    );
    assert_eq!(control(row, "e")["size"], json!(["fill", "100%"]));
    let (_, _, frame) = laid_out(&conversion, 2);
    for (name, expected) in [
        ("a", [15.0, 6.0, 30.0, 20.0]),
        ("b", [5.0, 30.0, 50.0, 10.0]),
        ("c", [20.0, 44.0, 20.0, 20.0]),
        ("badge", [40.0, 80.0, 10.0, 10.0]),
        ("d", [60.0, 0.0, 10.0, 20.0]),
        ("e", [72.0, 0.0, 48.0, 20.0]),
    ] {
        assert_eq!(rect(&frame, name), expected, "{name}");
    }
}

// `@button`, `$binding`, `$visible` and `@factory` add what Figma has no layer
// for, and the preview shows the design's own text through the bindings.
#[test]
fn naming_convention_adds_buttons_bindings_and_factories() {
    let conversion = figma::convert(&fixture("convention"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let ui = ui(&conversion);
    let title = control(&ui, "title");
    assert_eq!(title["text"], "#title");
    assert_eq!(title["bindings"], json!([{ "binding_name": "#title" }]));
    let play = control(&ui, "play");
    assert_eq!(play["type"], "button");
    assert_eq!(play["button_mappings"][0]["to_button_id"], "button.play");
    for state in ["default_control", "hover_control", "pressed_control"] {
        assert_eq!(play[state], "default", "{state}");
    }
    assert_eq!(control(play, "default")["size"], json!(["100%", "100%"]));
    let quit = control(&ui, "quit");
    assert_eq!(quit["button_mappings"][0]["to_button_id"], "button.quit");
    assert_eq!(
        (&quit["hover_control"], &quit["pressed_control"]),
        (&json!("hover"), &json!("default"))
    );
    assert_eq!(
        control(&ui, "badge")["bindings"],
        json!([{ "binding_name": "#has_badge", "binding_name_override": "#visible" }])
    );
    let servers = control(&ui, "servers");
    assert_eq!(servers["collection_name"], "servers");
    assert_eq!(
        servers["factory"]["control_ids"],
        json!({ "server_row": "@convention_test.server_row" })
    );
    let row_label = control(&ui["server_row"], "name");
    assert_eq!(row_label["bindings"][0]["binding_type"], "collection");
    assert_eq!(conversion.mock.globals["#title"], "Welcome");
    assert_eq!(conversion.mock.globals["#has_badge"], true);
    let items: Vec<&Value> = conversion.mock.collections["servers"]
        .iter()
        .map(|item| &item["#server_name"])
        .collect();
    assert_eq!(items, ["Lobby", "Survival", "Creative"]);
    let (_, _, frame) = laid_out(&conversion, 1);
    let rows = frame.boxes.iter().filter(|laid| laid.name == "server_row");
    assert_eq!(rows.count(), 3);
    let texts: Vec<&str> = frame
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    for text in ["Welcome", "Play", "Lobby", "Survival", "Creative"] {
        assert!(texts.contains(&text), "{text} in {texts:?}");
    }
}

// Every layer or property without a JSON-UI form is listed by layer, never dropped silently.
#[test]
fn unsupported_layers_and_effects_warn() {
    let conversion = figma::convert(&fixture("unsupported"), &BTreeMap::new());
    let warned = messages(&conversion);
    for (layer, message) in [
        (
            "Star",
            "star layers are not supported; name it `@image` to export it as a texture",
        ),
        ("Card", "rotation is not supported"),
        ("Card", "multiply blend mode is not supported"),
        ("Card", "drop shadow effect is not supported"),
        ("Card", "strokes are not supported"),
        ("Card", "corner radius is not supported"),
        ("Card", "gradient linear fills are not supported"),
        ("Odd @shiny", "`@shiny` is not a known directive"),
        ("Wrapped", "wrapping auto-layout is not supported"),
        (
            "Wrapped",
            "SpaceBetween distribution is not supported; children pack from the start",
        ),
    ] {
        let layer = format!("Unsupported Test / {layer}");
        assert!(
            warned.contains(&(layer.clone(), message.to_owned())),
            "{layer}: {message}\n{warned:#?}"
        );
    }
    assert!(
        !warned.iter().any(|(_, message)| message.contains("blur")),
        "hidden effects are fine"
    );
    assert_eq!(warned.len(), 10);
    let ui = ui(&conversion);
    assert!(find(&ui, "star").is_none());
    assert_eq!(control(&ui, "card")["type"], "panel");
    laid_out(&conversion, 1);
}

fn menu() -> Conversion {
    let backdrop = png(64, 36, |x, y| {
        let t = y as f32 / 35.0;
        let glow = 1.0 - ((x as f32 - 32.0).abs() / 32.0);
        [
            (40.0 + 60.0 * t + 30.0 * glow) as u8,
            (20.0 + 30.0 * t) as u8,
            (70.0 - 30.0 * t + 20.0 * glow) as u8,
            255,
        ]
    });
    figma::convert(
        &fixture("menu"),
        &BTreeMap::from([("backdrop".to_owned(), backdrop)]),
    )
}

// A typical menu screen converts without warnings, validates, renders through
// the editor's paint path, and exports as a resource pack.
#[test]
fn menu_screen_converts_validates_renders_and_exports() {
    let conversion = menu();
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let ui = ui(&conversion);
    assert_eq!(control(&ui, "buttons")["type"], "stack_panel");
    for button in ["play", "settings", "quit"] {
        assert_eq!(control(&ui, button)["type"], "button", "{button}");
    }
    let (mut session, view, frame) = laid_out(&conversion, 3);
    assert_eq!(view.size, [1440, 810]);
    assert_eq!(rect(&frame, "play"), [160.0, 100.0, 160.0, 28.0]);
    assert_eq!(rect(&frame, "quit"), [160.0, 168.0, 160.0, 28.0]);
    let rgba = session.paint(&frame, 0.0).unwrap();
    assert!(
        near(pixel(&rgba, 1440, 3 * 165, 3 * 103), [64, 64, 64, 255]),
        "button fill"
    );
    let image = image::RgbaImage::from_raw(1440, 810, rgba).unwrap();
    image
        .save(Path::new(env!("CARGO_TARGET_TMPDIR")).join("figma-menu.png"))
        .unwrap();

    let pack = PackInfo::new(
        "Main Menu",
        [
            "0f5b8c56-2a53-4a61-9d0e-6f2f3c7e1a10".into(),
            "7c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f".into(),
        ],
    );
    let plan = export::plan(&mut session.workspace, Mode::Full, Some(0), true);
    assert!(plan.skipped.is_empty() && plan.notes.is_empty(), "{plan:?}");
    let bytes = export::package(&plan, &pack, None, Format::Mcpack).unwrap();
    std::fs::write(
        Path::new(env!("CARGO_TARGET_TMPDIR")).join("figma-menu.mcpack"),
        &bytes,
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "manifest.json",
            "textures/main_menu/main_menu.png",
            "textures/main_menu/solid.png",
            "ui/_ui_defs.json",
            "ui/main_menu.json",
        ]
    );
    let mut shipped = String::new();
    archive
        .by_name("ui/main_menu.json")
        .unwrap()
        .read_to_string(&mut shipped)
        .unwrap();
    assert_eq!(shipped.as_bytes(), conversion.files["ui/main_menu.json"]);
}

// The same frame always converts to the same bytes.
#[test]
fn conversion_is_deterministic() {
    let (first, second) = (menu(), menu());
    assert_eq!(first.files, second.files);
    assert_eq!(first.warnings, second.warnings);
}

// A `@button` component set, or an instance of one, takes its states from the
// `State` variants (Disabled meaning locked); an instance keeps its other
// variant properties, and the button takes the default variant's size.
#[test]
fn state_variants_become_button_states() {
    let conversion = figma::convert(&fixture("variants"), &BTreeMap::new());
    assert_eq!(
        messages(&conversion),
        [(
            "Variant Test / Menu Button @button=play / State=Focused".to_owned(),
            "variant state `Focused` is not a button state".to_owned(),
        )],
        "the set's own stroke is canvas chrome"
    );
    let ui = ui(&conversion);
    let menu = control(&ui, "menu_button");
    assert_eq!(menu["type"], "button");
    assert_eq!(menu["button_mappings"][0]["to_button_id"], "button.play");
    for (property, state) in [
        ("default_control", "default"),
        ("hover_control", "hover"),
        ("pressed_control", "default"),
        ("locked_control", "locked"),
    ] {
        assert_eq!(menu[property], state, "{property}");
    }
    assert_eq!(menu["size"], json!([80, 20]));
    let states: Vec<&String> = menu["controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|control| control.as_object()?.keys().next())
        .collect();
    assert_eq!(states, ["default", "hover", "locked"]);
    assert_eq!(control(menu, "default")["size"], json!(["100%", "100%"]));
    assert_eq!(control(control(menu, "default"), "label")["text"], "Play");
    let quit = control(&ui, "quit");
    assert_eq!(
        (&quit["default_control"], &quit["hover_control"]),
        (&json!("default"), &json!("hover"))
    );
    let background = |state: &str| &control(control(quit, state), "background")["color"];
    assert_eq!(background("default"), &json!([0.5, 0, 0]));
    assert_eq!(
        background("hover"),
        &json!([0.8, 0, 0]),
        "the Large hover variant"
    );
    let (_, _, frame) = laid_out(&conversion, 1);
    assert_eq!(rect(&frame, "menu_button"), [0.0, 0.0, 80.0, 20.0]);
    assert_eq!(rect(&frame, "quit"), [100.0, 70.0, 80.0, 24.0]);
}

// A main-axis FILL child of a centred auto-layout keeps its share: the stack
// stays parent-sized instead of collapsing to its content.
#[test]
fn fill_children_keep_their_share_under_primary_alignment() {
    let conversion = figma::convert(&fixture("fill_center"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (_, _, frame) = laid_out(&conversion, 1);
    assert_eq!(rect(&frame, "g"), [0.0, 0.0, 20.0, 20.0]);
    assert_eq!(rect(&frame, "h"), [20.0, 0.0, 80.0, 20.0]);
}

// A `@button` instance's variants already carry its opacity; it applies once.
#[test]
fn instance_opacity_applies_once_to_button_states() {
    let conversion = figma::convert(&fixture("variant_opacity"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (mut session, _, frame) = laid_out(&conversion, 1);
    let rgba = session.paint(&frame, 0.0).unwrap();
    let [r, g, b, _] = pixel(&rgba, 100, 20, 10);
    assert!(
        (120..=136).contains(&r) && r == g && g == b,
        "white at 50% over black: {r} {g} {b}"
    );
}

// A `$visible` item takes its auto-layout gap with it, whether it starts shown
// or hidden; the first spaced item cannot, and says so.
#[test]
fn spacers_follow_their_items_visibility_bindings() {
    let conversion = figma::convert(&fixture("visibility"), &BTreeMap::new());
    assert_eq!(
        messages(&conversion),
        [(
            "Visibility Test / Lead / E $visible=#show_e".to_owned(),
            "the spacing after this layer does not follow `#show_e`".to_owned(),
        )]
    );
    let (mut session, mut view, frame) = laid_out(&conversion, 1);
    assert_eq!(rect(&frame, "c"), [0.0, 28.0, 50.0, 10.0]);
    view.mock.globals.insert("#show_b".into(), false.into());
    view.mock.globals.insert("#show_d".into(), true.into());
    let frame = session.frame(&view);
    assert_eq!(
        rect(&frame, "c"),
        [0.0, 14.0, 50.0, 10.0],
        "B's gap hides with B"
    );
    assert_eq!(
        rect(&frame, "d"),
        [0.0, 28.0, 50.0, 10.0],
        "D's gap shows with D"
    );
}
