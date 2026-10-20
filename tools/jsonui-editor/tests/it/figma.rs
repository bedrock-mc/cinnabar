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
        assert_eq!(play[state], "", "{state}: the content shows in every state");
    }
    assert_eq!(control(play, "content")["size"], json!(["100%", "100%"]));
    let quit = control(&ui, "quit");
    assert_eq!(quit["button_mappings"][0]["to_button_id"], "button.quit");
    assert_eq!(
        (&quit["hover_control"], &quit["pressed_control"]),
        (&json!("hover"), &json!("pressed"))
    );
    assert_eq!(
        control(&ui, "badge")["bindings"],
        json!([{ "binding_name": "#has_badge", "binding_name_override": "#visible" }])
    );
    let servers = control(&ui, "servers");
    assert_eq!(servers["collection_name"], "servers");
    assert_eq!(
        servers["factory"]["control_name"],
        "convention_test.server_row"
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
        ("pressed_control", "pressed"),
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
    assert_eq!(
        states,
        ["default", "hover", "pressed", "locked"],
        "pressed copies default"
    );
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
    assert_eq!(
        rect(&frame, "g2"),
        [40.0, 20.0, 20.0, 20.0],
        "a hidden fill child takes no share"
    );
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

// An `@image` export already has the layer's opacity baked in; it is not applied again.
#[test]
fn raster_opacity_applies_once() {
    // The plugin's export of a white layer at 50% opacity.
    let images = BTreeMap::from([(
        "node:14:2".to_owned(),
        solid_png(20, 20, [255, 255, 255, 128]),
    )]);
    let conversion = figma::convert(&fixture("raster_opacity"), &images);
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (mut session, _, frame) = laid_out(&conversion, 1);
    let rgba = session.paint(&frame, 0.0).unwrap();
    let [r, g, b, _] = pixel(&rgba, 40, 10, 10);
    assert!(
        (120..=136).contains(&r) && r == g && g == b,
        "50% white over black: {r} {g} {b}"
    );
}

// Absolute children of an auto-layout frame keep Figma's drawing order around
// the stacked items, reversed by `itemReverseZIndex`; one between items warns.
#[test]
fn absolute_children_keep_their_drawing_order() {
    let conversion = figma::convert(&fixture("absolute_order"), &BTreeMap::new());
    assert_eq!(
        messages(&conversion),
        [(
            "Order Test / Stack / Pin".to_owned(),
            "an absolute layer between auto-layout items draws above all of them".to_owned(),
        )]
    );
    let (mut session, _, frame) = laid_out(&conversion, 1);
    let rgba = session.paint(&frame, 0.0).unwrap();
    assert!(
        near(pixel(&rgba, 120, 30, 5), [0, 255, 0, 255]),
        "A above the backdrop"
    );
    assert!(
        near(pixel(&rgba, 120, 5, 35), [255, 255, 255, 255]),
        "the badge above B"
    );
    assert!(
        near(pixel(&rgba, 120, 90, 5), [255, 0, 255, 255]),
        "reversed: the cover above C"
    );
}

/// Fixed-advance text and no textures, for laying out the bound tree directly.
struct Plain;

impl json_ui::TextMeasure for Plain {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 6.0, 9.0]
    }
}

impl json_ui::TextureSource for Plain {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

/// Sprite tints drawn within the box named `name` under the interaction state
/// `state` makes from its key, through the engine's button state machinery.
fn tints(frame: &Frame, name: &str, state: impl Fn(String) -> json_ui::ViewState) -> Vec<[u8; 4]> {
    let laid = frame.boxes.iter().find(|laid| laid.name == name).unwrap();
    let env = json_ui::LayoutEnv {
        text: &Plain,
        textures: &Plain,
    };
    let (out, _) = json_ui::layout_with(&frame.bound, frame.root, &env, &state(laid.key.clone()));
    let [x, y, w, h] = laid.rect;
    json_ui::emit(&out, &env)
        .into_iter()
        .filter(|node| {
            node.dest.x >= x
                && node.dest.y >= y
                && node.dest.x + node.dest.w <= x + w
                && node.dest.y + node.dest.h <= y + h
        })
        .filter_map(|node| match node.draw {
            Draw::Sprite { color, .. } => Some(color),
            _ => None,
        })
        .collect()
}

fn rest(_: String) -> json_ui::ViewState {
    json_ui::ViewState::default()
}

fn hover(key: String) -> json_ui::ViewState {
    json_ui::ViewState {
        hovered: Some(key),
        ..Default::default()
    }
}

fn press(key: String) -> json_ui::ViewState {
    json_ui::ViewState {
        hovered: Some(key.clone()),
        pressed: Some(key),
        ..Default::default()
    }
}

// Buttons stay drawn when hovered and pressed: without states the content shows
// in every state, and a missing state shows the default's look.
#[test]
fn buttons_draw_in_every_interaction_state() {
    let conversion = figma::convert(&fixture("button_states"), &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (_, _, frame) = laid_out(&conversion, 1);
    let grey = [51, 51, 51, 255];
    for state in [rest, hover, press] {
        assert_eq!(tints(&frame, "plain", state), [grey]);
    }
    let (dark, light) = ([102, 0, 0, 255], [204, 0, 0, 255]);
    assert_eq!(tints(&frame, "pair", rest), [dark]);
    assert_eq!(tints(&frame, "pair", hover), [light]);
    assert_eq!(
        tints(&frame, "pair", press),
        [dark],
        "no pressed state: the default look"
    );
}

// States nest inside an auto-layout button's generated stack and still switch.
#[test]
fn auto_layout_button_states_are_found_in_figma_children() {
    let conversion = figma::convert(&fixture("button_states"), &BTreeMap::new());
    let ui = ui(&conversion);
    let auto = control(&ui, "auto");
    assert_eq!(
        (&auto["default_control"], &auto["hover_control"]),
        (&json!("default"), &json!("hover"))
    );
    let (_, _, frame) = laid_out(&conversion, 1);
    assert_eq!(tints(&frame, "auto", rest), [[0, 102, 0, 255]]);
    assert_eq!(tints(&frame, "auto", hover), [[0, 204, 0, 255]]);
}

/// Sprite tints and rects drawn within the box named `name` under `state`.
fn sprites(
    frame: &Frame,
    name: &str,
    state: impl Fn(String) -> json_ui::ViewState,
) -> Vec<([u8; 4], [f64; 4])> {
    let laid = frame.boxes.iter().find(|laid| laid.name == name).unwrap();
    let env = json_ui::LayoutEnv {
        text: &Plain,
        textures: &Plain,
    };
    let (out, _) = json_ui::layout_with(&frame.bound, frame.root, &env, &state(laid.key.clone()));
    json_ui::emit(&out, &env)
        .into_iter()
        .filter_map(|node| match node.draw {
            Draw::Sprite { color, .. } => {
                Some((color, [node.dest.x, node.dest.y, node.dest.w, node.dest.h]))
            }
            _ => None,
        })
        .filter(|(_, [x, ..])| *x >= laid.rect[0] && *x < laid.rect[0] + laid.rect[2])
        .collect()
}

// Every state of an auto-layout button overlays its flowing default's slot,
// including the pressed state copied from the default.
#[test]
fn button_states_overlay_the_default_slot() {
    let conversion = figma::convert(&fixture("button_states"), &BTreeMap::new());
    let (_, _, frame) = laid_out(&conversion, 1);
    let slot = [140.0, 0.0, 60.0, 20.0];
    let (dark, light) = ([0, 102, 0, 255], [0, 204, 0, 255]);
    assert_eq!(sprites(&frame, "auto", rest), [(dark, slot)]);
    assert_eq!(sprites(&frame, "auto", hover), [(light, slot)]);
    assert_eq!(sprites(&frame, "auto", press), [(dark, slot)]);
}

// Bound names keep the case the author wrote; JSON-UI lookups are case-sensitive.
#[test]
fn binding_names_keep_their_case() {
    let node: Node = serde_json::from_value(json!({
        "id": "16:1", "name": "Case Test", "type": "FRAME", "width": 100, "height": 20,
        "children": [{
            "id": "16:2", "name": "Name $binding=#ButtonName $visible=#ShowName", "type": "TEXT",
            "width": 100, "height": 10, "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
            "text": { "characters": "Hi", "font_family": "Minecraft" }
        }]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    let ui = ui(&conversion);
    let label = control(&ui, "name");
    assert_eq!(label["text"], "#ButtonName");
    assert_eq!(label["bindings"][0]["binding_name"], "#ButtonName");
    assert_eq!(label["bindings"][1]["binding_name"], "#ShowName");
    assert_eq!(conversion.mock.globals["#ButtonName"], "Hi");
    laid_out(&conversion, 1);
}

// A hidden fill child bound to `$visible` keeps its share once shown.
#[test]
fn visibility_bound_fill_children_keep_their_share_when_shown() {
    let conversion = figma::convert(&fixture("fill_center"), &BTreeMap::new());
    let (mut session, mut view, _) = laid_out(&conversion, 1);
    view.mock.globals.insert("#show_h3".into(), true.into());
    let frame = session.frame(&view);
    assert_eq!(rect(&frame, "g3"), [0.0, 40.0, 20.0, 20.0]);
    assert_eq!(rect(&frame, "h3"), [20.0, 40.0, 80.0, 20.0]);
}

fn drawn_texts(frame: &Frame) -> Vec<(String, bool)> {
    frame
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, localize, .. } => Some((text.clone(), *localize)),
            _ => None,
        })
        .collect()
}

// Design text draws as written: never localized, and never read as a `#`
// reference; only `$binding` labels read data.
#[test]
fn design_text_is_literal() {
    let text = |id: &str, name: &str, characters: &str, y: f64| {
        json!({
            "id": id, "name": name, "type": "TEXT", "y": y, "width": 100, "height": 10,
            "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
            "text": { "characters": characters, "font_family": "Minecraft" }
        })
    };
    let node: Node = serde_json::from_value(json!({
        "id": "17:1", "name": "Literal Test", "type": "FRAME", "width": 100, "height": 40,
        "children": [
            text("17:2", "Percent", "100% sure", 0.0),
            text("17:3", "Hash", "#1", 10.0),
            text("17:4", "Bound $binding=#Title", "Hello", 20.0),
        ]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    let ui = ui(&conversion);
    assert_eq!(control(&ui, "percent")["localize"], false);
    assert!(
        control(&ui, "bound").get("localize").is_none(),
        "bound text keeps the default"
    );
    let (_, _, frame) = laid_out(&conversion, 1);
    let texts = drawn_texts(&frame);
    assert!(
        texts.contains(&("100% sure".to_owned(), false)),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|(text, _)| text.ends_with("#1")),
        "#1 is drawn: {texts:?}"
    );
    assert!(texts.iter().any(|(text, _)| text == "Hello"), "{texts:?}");
}

// Two views of one collection each repeat their own template.
#[test]
fn factories_sharing_a_collection_coexist() {
    let list = |id: &str, x: f64, row: &str| {
        let item = |n: u32| json!({ "id": format!("{id}:{n}"), "name": row, "type": "FRAME", "y": f64::from(n) * 10.0, "width": 40, "height": 10 });
        json!({
            "id": id, "name": "List @factory=servers", "type": "FRAME", "x": x, "width": 40, "height": 20,
            "layout": { "mode": "VERTICAL" }, "children": [item(0), item(1)]
        })
    };
    let node: Node = serde_json::from_value(json!({
        "id": "18:1", "name": "Shared Test", "type": "FRAME", "width": 100, "height": 20,
        "children": [list("18:2", 0.0, "Row A"), list("18:3", 50.0, "Row B")]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (_, _, frame) = laid_out(&conversion, 1);
    for row in ["row_a", "row_b"] {
        assert_eq!(
            frame.boxes.iter().filter(|laid| laid.name == row).count(),
            2,
            "{row}"
        );
    }
}

// First-on-top stacking of flowing items and per-fill blend modes are not
// expressible, and say so.
#[test]
fn reverse_stacking_and_fill_blend_modes_warn() {
    let node: Node = serde_json::from_value(json!({
        "id": "19:1", "name": "Warn Test", "type": "FRAME", "width": 100, "height": 40,
        "children": [
            { "id": "19:2", "name": "Stack", "type": "FRAME", "width": 50, "height": 40,
              "layout": { "mode": "VERTICAL", "reverse_z_index": true },
              "children": [
                  { "id": "19:3", "name": "A", "type": "FRAME", "width": 50, "height": 20 },
                  { "id": "19:4", "name": "B", "type": "FRAME", "y": 20, "width": 50, "height": 20 }
              ] },
            { "id": "19:5", "name": "Tint", "type": "RECTANGLE", "x": 50, "width": 50, "height": 40,
              "fills": [{ "type": "SOLID", "color": [1, 0, 0], "blend_mode": "MULTIPLY" }] }
        ]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    assert_eq!(
        messages(&conversion),
        [
            (
                "Warn Test / Stack".to_owned(),
                "first-on-top stacking applies only to absolute layers; stacked items draw last on top".to_owned(),
            ),
            ("Warn Test / Tint".to_owned(), "multiply fill blend mode is not supported".to_owned()),
        ]
    );
}

// A text `@button` sized to its text hugs the label, as in Figma, instead of
// filling its parent.
#[test]
fn text_buttons_hug_their_label() {
    let node: Node = serde_json::from_value(json!({
        "id": "20:1", "name": "Hug Test", "type": "FRAME", "width": 200, "height": 100,
        "children": [{
            "id": "20:2", "name": "Play @button", "type": "TEXT", "x": 10, "y": 10, "width": 24, "height": 9,
            "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
            "text": { "characters": "Play", "font_family": "Minecraft" }
        }]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    let (_, _, frame) = laid_out(&conversion, 1);
    let label = rect(&frame, "content");
    assert_eq!(
        rect(&frame, "play"),
        label,
        "the button is the label's size"
    );
    assert!(label[2] < 100.0 && label[3] < 50.0, "{label:?}");
    assert_eq!(label[..2], [10.0, 10.0]);
}

// A missing state copied from the default keeps the default's directives.
#[test]
fn copied_states_keep_their_directives() {
    let node: Node = serde_json::from_value(json!({
        "id": "21:1", "name": "Copy Test", "type": "FRAME", "width": 100, "height": 40,
        "children": [{
            "id": "21:2", "name": "Go @button", "type": "FRAME", "width": 60, "height": 20,
            "children": [{
                "id": "21:3", "name": "Default $binding=#Title", "type": "TEXT", "width": 60, "height": 9,
                "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
                "text": { "characters": "Go", "font_family": "Minecraft" }
            }]
        }]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    let ui = ui(&conversion);
    let go = control(&ui, "go");
    for state in ["default", "hover", "pressed"] {
        assert_eq!(control(go, state)["text"], "#Title", "{state}");
    }
    laid_out(&conversion, 1);
}

// A rotated image paint has no JSON-UI form and says so.
#[test]
fn rotated_image_fills_warn() {
    let node: Node = serde_json::from_value(json!({
        "id": "22:1", "name": "Rotate Test", "type": "FRAME", "width": 40, "height": 40,
        "children": [{
            "id": "22:2", "name": "Photo", "type": "RECTANGLE", "width": 40, "height": 40,
            "fills": [{ "type": "IMAGE", "image": "photo", "scale_mode": "FILL", "rotation": 90 }]
        }]
    }))
    .unwrap();
    let images = BTreeMap::from([("photo".to_owned(), solid_png(2, 2, [0, 255, 0, 255]))]);
    let conversion = figma::convert(&node, &images);
    assert_eq!(
        messages(&conversion),
        [(
            "Rotate Test / Photo".to_owned(),
            "rotated image fills are not supported".to_owned()
        )]
    );
}

// A frame named `Namespace` cannot take the file's `namespace` key.
#[test]
fn the_namespace_key_is_reserved() {
    let node: Node = serde_json::from_value(json!({
        "id": "23:1", "name": "Namespace", "type": "FRAME", "width": 40, "height": 40,
        "children": [{ "id": "23:2", "name": "Namespace", "type": "FRAME", "width": 10, "height": 10 }]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    let ui = ui(&conversion);
    assert_eq!(ui["namespace"], "namespace");
    assert!(ui[conversion.screen.split_once('.').unwrap().1].is_object());
    laid_out(&conversion, 1);
}

// Text the property evaluator would read (a `#` reference, a `$` variable or a
// `(` expression) still draws exactly as written.
#[test]
fn evaluator_prefixed_text_stays_literal() {
    let samples = ["#1", "$shown", "(true)", "(1 + 1)"];
    let children: Vec<Value> = samples
        .iter()
        .enumerate()
        .map(|(index, characters)| {
            json!({
                "id": format!("24:{index}"), "name": format!("T{index}"), "type": "TEXT",
                "y": index as f64 * 10.0, "width": 80, "height": 9,
                "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
                "text": { "characters": characters, "font_family": "Minecraft" }
            })
        })
        .collect();
    let node: Node = serde_json::from_value(json!({
        "id": "24:9", "name": "Prefix Test", "type": "FRAME", "width": 80, "height": 40,
        "children": children
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    let mut session = Session::default();
    let mut view = conversion.load(&mut session, 1);
    let diagnostics = conversion.validate(&mut session);
    // A variable the screen defines must not replace the literal `$shown`.
    view.context.insert("shown".into(), "replaced".into());
    let frame = session.frame(&view);
    let texts = drawn_texts(&frame);
    let lost: Vec<&str> = samples
        .into_iter()
        .filter(|sample| !texts.iter().any(|(text, _)| text.ends_with(sample)))
        .collect();
    assert_eq!(lost, Vec::<&str>::new(), "drawn: {texts:?}");
    assert!(
        diagnostics.is_empty() && frame.diagnostics.is_empty(),
        "{diagnostics:?} {:?}",
        frame.diagnostics
    );
}

// Hugging auto-layout frames grow with wrapping text, padded or not, and the
// items after them move down.
#[test]
fn hugging_frames_grow_with_their_content() {
    let words = "aaaaaaaa bbbbbbbb cccccccc";
    let body = |id: &str, width: f64| {
        json!({
            "id": id, "name": "Body", "type": "TEXT", "width": width, "height": 9, "sizing": ["FILL", "HUG"],
            "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
            "text": { "characters": words, "font_family": "Minecraft", "auto_resize": "HEIGHT" }
        })
    };
    let node: Node = serde_json::from_value(json!({
        "id": "25:1", "name": "Hug Frames", "type": "FRAME", "width": 200, "height": 100,
        "layout": { "mode": "VERTICAL" },
        "children": [
            { "id": "25:2", "name": "Card", "type": "FRAME", "width": 200, "height": 9, "sizing": ["FILL", "HUG"],
              "layout": { "mode": "VERTICAL" }, "children": [body("25:3", 200.0)] },
            { "id": "25:4", "name": "Next", "type": "FRAME", "y": 9, "width": 200, "height": 10 },
            { "id": "25:5", "name": "Padded", "type": "FRAME", "y": 19, "width": 200, "height": 15, "sizing": ["FILL", "HUG"],
              "fills": [{ "type": "SOLID", "color": [0, 0, 0] }],
              "layout": { "mode": "VERTICAL", "padding": [2, 3, 2, 3] }, "children": [body("25:6", 196.0)] },
            { "id": "25:7", "name": "Last", "type": "FRAME", "y": 34, "width": 200, "height": 10 }
        ]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (mut session, mut view, frame) = laid_out(&conversion, 1);
    assert_eq!(rect(&frame, "card"), [0.0, 0.0, 200.0, 9.0]);
    assert_eq!(rect(&frame, "padded"), [0.0, 19.0, 200.0, 15.0]);
    assert_eq!(rect(&frame, "last")[1], 34.0);
    // At half the width the text wraps onto two lines.
    view.size = [100, 100];
    let frame = session.frame(&view);
    assert_eq!(rect(&frame, "card"), [0.0, 0.0, 100.0, 18.0]);
    assert_eq!(rect(&frame, "next")[1], 18.0);
    assert_eq!(rect(&frame, "padded"), [0.0, 28.0, 100.0, 24.0]);
    assert_eq!(rect(&frame, "last")[1], 52.0);
}

// A hugging frame sizes to its stacked items only; absolute layers overlay it.
#[test]
fn absolute_layers_do_not_grow_hugging_frames() {
    let node: Node = serde_json::from_value(json!({
        "id": "26:1", "name": "Overlay Test", "type": "FRAME", "width": 200, "height": 100,
        "layout": { "mode": "VERTICAL" },
        "children": [
            { "id": "26:2", "name": "Card", "type": "FRAME", "width": 100, "height": 20, "sizing": ["FIXED", "HUG"],
              "layout": { "mode": "VERTICAL" },
              "children": [
                  { "id": "26:3", "name": "Row", "type": "FRAME", "width": 100, "height": 20 },
                  { "id": "26:4", "name": "Tall", "type": "FRAME", "width": 10, "height": 200, "positioning": "ABSOLUTE" }
              ] },
            { "id": "26:5", "name": "After", "type": "FRAME", "y": 20, "width": 100, "height": 10 }
        ]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    assert!(conversion.warnings.is_empty(), "{:?}", conversion.warnings);
    let (_, _, frame) = laid_out(&conversion, 1);
    assert_eq!(rect(&frame, "card"), [0.0, 0.0, 100.0, 20.0]);
    assert_eq!(rect(&frame, "tall"), [0.0, 0.0, 10.0, 200.0]);
    assert_eq!(rect(&frame, "after")[1], 20.0);
}

// A `§` inside design text starts a formatting code that eats the next
// character; Bedrock text has no escape for it, so it warns.
#[test]
fn section_signs_in_design_text_warn() {
    let node: Node = serde_json::from_value(json!({
        "id": "27:1", "name": "Section Test", "type": "FRAME", "width": 100, "height": 20,
        "children": [{
            "id": "27:2", "name": "Terms", "type": "TEXT", "width": 100, "height": 9,
            "fills": [{ "type": "SOLID", "color": [1, 1, 1] }],
            "text": { "characters": "\u{a7}4 Terms", "font_family": "Minecraft" }
        }]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    assert_eq!(
        messages(&conversion),
        [(
            "Section Test / Terms".to_owned(),
            "`\u{a7}` starts a Bedrock formatting code, so it and the character after it do not draw".to_owned(),
        )]
    );
}

// A faded group fades each of its layers, so overlapping layers look stronger
// than Figma's flattened fade; that warns, while side-by-side layers stay exact.
#[test]
fn faded_groups_with_overlapping_layers_warn() {
    let square = |id: &str, x: f64| {
        json!({ "id": id, "name": "Square", "type": "RECTANGLE", "x": x, "width": 20, "height": 20,
                "fills": [{ "type": "SOLID", "color": [1, 1, 1] }] })
    };
    let node: Node = serde_json::from_value(json!({
        "id": "28:1", "name": "Fade Test", "type": "FRAME", "width": 100, "height": 40,
        "children": [
            { "id": "28:2", "name": "Faded", "type": "FRAME", "width": 50, "height": 20, "opacity": 0.5,
              "children": [square("28:3", 0.0), square("28:4", 10.0)] },
            { "id": "28:5", "name": "Spread", "type": "FRAME", "y": 20, "width": 50, "height": 20, "opacity": 0.5,
              "children": [square("28:6", 0.0), square("28:7", 25.0)] }
        ]
    }))
    .unwrap();
    let conversion = figma::convert(&node, &BTreeMap::new());
    assert_eq!(
        messages(&conversion),
        [(
            "Fade Test / Faded".to_owned(),
            "a faded group fades each layer, so overlapping layers look stronger than in Figma; name it `@image` to flatten it".to_owned(),
        )]
    );
}
