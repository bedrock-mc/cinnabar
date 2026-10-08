//! The authored example packs through the whole editor pipeline: layout boxes,
//! provenance across layers, painted pixels, diagnostics and edits.

use std::path::Path;

use jsonui_editor::mock::MockData;
use jsonui_editor::{Session, View};
use jsonui_editor::{api, export};

const SCREEN: &str = "example.example_screen";

fn read_dir(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            read_dir(root, &path, out);
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, std::fs::read(&path).unwrap()));
        }
    }
}

fn session(layers: &[&str]) -> Session {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut session = Session::default();
    for name in layers {
        let layer = session.workspace.add_layer(name);
        let mut files = Vec::new();
        read_dir(&examples.join(name), &examples.join(name), &mut files);
        session.workspace.add_files(layer, files, Vec::new());
    }
    session
}

fn view(size: [u32; 2]) -> View {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mock: MockData =
        serde_json::from_slice(&std::fs::read(examples.join("mock.json")).unwrap()).unwrap();
    View {
        reference: SCREEN.into(),
        size,
        gui_scale: Some(2),
        context: Default::default(),
        mock,
    }
}

fn rect(frame: &jsonui_editor::Frame, key: &str) -> [f64; 4] {
    let found = frame.boxes.iter().find(|laid| laid.key == key);
    found
        .unwrap_or_else(|| panic!("no box {key}"))
        .rect
        .map(|v| (v * 100.0).round() / 100.0)
}

// Golden: the example screen lays out to these virtual-pixel boxes (fallback text metrics).
#[test]
fn example_screen_lays_out_to_the_expected_boxes() {
    let mut session = session(&["base"]);
    let mut touch = view([960, 540]);
    touch.context.insert("touch".into(), true.into());
    let frame = session.frame(&touch);
    assert!(frame.diagnostics.is_empty(), "{:?}", frame.diagnostics);
    assert_eq!(frame.root, [480.0, 270.0]);
    assert_eq!(rect(&frame, "/example_screen"), [0.0, 0.0, 480.0, 270.0]);
    assert_eq!(
        rect(&frame, "/example_screen/card"),
        [120.0, 83.25, 240.0, 103.5]
    );
    assert_eq!(
        rect(&frame, "/example_screen/card/content"),
        [126.0, 89.25, 228.0, 91.5]
    );
    assert_eq!(
        rect(&frame, "/example_screen/card/content/title"),
        [126.0, 89.25, 228.0, 13.5]
    );
    assert_eq!(
        rect(&frame, "/example_screen/card/content/rows"),
        [126.0, 108.75, 228.0, 48.0]
    );
    assert_eq!(
        rect(&frame, "/example_screen/card/content/rows/row[1]"),
        [126.0, 124.75, 228.0, 16.0]
    );
    assert_eq!(
        rect(&frame, "/example_screen/card/content/rows/row[1]/row_label"),
        [132.0, 128.25, 48.0, 9.0]
    );
    assert_eq!(
        rect(&frame, "/example_screen/card/content/status"),
        [126.0, 162.75, 228.0, 9.0]
    );
    assert!(frame.boxes.iter().any(|laid| laid.name == "touch_hint"));
}

#[test]
fn context_flags_select_variables_and_ignored() {
    let mut session = session(&["base"]);
    let mut pocket = view([960, 540]);
    pocket.context.insert("pocket_screen".into(), true.into());
    let frame = session.frame(&pocket);
    assert_eq!(rect(&frame, "/example_screen/card")[2], 200.0);
    // Unset `$touch` reads as null, so `(not $touch)` drops the hint as vanilla does.
    assert!(!frame.boxes.iter().any(|laid| laid.name == "touch_hint"));
}

// An overlay's nested override and `modifications` land, and provenance names its file.
#[test]
fn overlay_properties_trace_to_the_overlay_file() {
    let mut session = session(&["base", "overlay"]);
    let frame = session.frame(&view([960, 540]));
    assert!(
        frame
            .boxes
            .iter()
            .any(|laid| laid.key.ends_with("/content/footer"))
    );
    let card = frame
        .boxes
        .iter()
        .position(|laid| laid.key == "/example_screen/card")
        .unwrap();
    let inspection = api::inspect(&mut session, SCREEN, &frame, card).unwrap();
    let color = inspection
        .properties
        .iter()
        .find(|p| p.key == "color")
        .unwrap();
    let source = serde_json::to_value(&color.source).unwrap();
    assert_eq!(source["kind"], "file");
    assert_eq!(source["location"]["path"], "ui/example_common.json");
    assert_eq!(source["via"]["variable"], "$fill");
    assert_eq!(source["via"]["declared"]["layer"], 1);
    assert_eq!(
        source["via"]["declared_site"],
        "example.example_screen/card"
    );
    let definition = inspection.definition.unwrap();
    assert_eq!(
        (definition.layer, definition.path.as_str()),
        (1, "ui/example_screen.json")
    );
}

#[test]
fn painted_pixels_follow_the_overlay_colour() {
    let mut session = session(&["base", "overlay"]);
    let frame = session.frame(&view([960, 540]));
    let pixels = session.paint(&frame, 0.0).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 960 + x) * 4..(y * 960 + x) * 4 + 4];
    // Card background, left gutter: `$fill` [0.16, 0.1, 0.22, 0.95] over the opaque backdrop.
    assert_eq!(at(245, 300), [39, 26, 54, 255]);
    // Outside the card: the backdrop.
    assert_eq!(at(10, 10), [8, 10, 15, 255]);
}

#[test]
fn animation_time_changes_only_the_paint() {
    let mut session = session(&["base"]);
    let frame = session.frame(&view([960, 540]));
    let status = frame
        .boxes
        .iter()
        .find(|laid| laid.name == "status")
        .unwrap();
    assert!(status.animated);
    let early = session.paint(&frame, 0.0).unwrap();
    let late = session.paint(&frame, 1.2).unwrap();
    assert_ne!(early, late);
    let again = session.frame(&view([960, 540]));
    assert!(
        std::sync::Arc::ptr_eq(&frame, &again),
        "an unchanged view reuses its frame"
    );
}

#[test]
fn syntax_errors_and_unknown_bases_are_located() {
    let mut session = session(&["base"]);
    session.workspace.edit(
        0,
        "ui/example_common.json",
        "{\n  \"namespace\": \"example_common\",\n  \"fill\": { \"type\": \"image\" }\n  \"oops\": 1\n}",
    );
    session.workspace.edit(
        0,
        "ui/example_screen.json",
        r#"{ "namespace": "example", "example_screen": { "type": "screen",
            "controls": [ { "x@example_common.missing": {} } ] } }"#,
    );
    let diagnostics = api::validate(&mut session, &[], &json_ui::Context::empty());
    let syntax = diagnostics.iter().find(|d| d.stage == "syntax").unwrap();
    let location = syntax.location.as_ref().unwrap();
    assert_eq!(
        (location.path.as_str(), location.line),
        ("ui/example_common.json", 3)
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("example_common.missing not found"))
    );
}

#[test]
fn screens_list_the_example_screen() {
    let mut session = session(&["base"]);
    let screens = api::screens(&mut session);
    assert_eq!(screens[0].reference, SCREEN);
    assert!(screens[0].screen);
    assert!(
        screens
            .iter()
            .any(|s| s.reference == "example_common.row" && !s.screen)
    );
}

// A file previews its first `screen` control, else its first top-level control;
// animations never qualify.
#[test]
fn pick_screen_prefers_screens_then_the_first_control() {
    let mut session = session(&["base"]);
    let pick = |session: &mut Session, path| api::pick_screen(session, 0, path);
    assert_eq!(
        pick(&mut session, "ui/example_screen.json").as_deref(),
        Some(SCREEN)
    );
    assert_eq!(
        pick(&mut session, "ui/example_common.json").as_deref(),
        Some("example_common.fill")
    );
    assert!(api::has_control(&mut session, SCREEN));
    assert!(!api::has_control(&mut session, "example.nope"));
    assert!(!api::has_control(&mut session, ""));
}

// Pasted text lands in a scratch layer that stays on top, is listed in its
// `_ui_defs.json`, previews its screen, and exports with the edits.
#[test]
fn pasted_text_becomes_a_scratch_file_on_top() {
    let mut session = session(&["base"]);
    let pasted = r#"// pasted
    { "namespace": "mine",
      "strip@example_common.fill": { "size": [10, 10] },
      "my_screen": { "type": "screen", "controls": [ { "s@mine.strip": {} } ] } }"#;
    let (layer, path) = session.workspace.new_scratch_file(pasted);
    assert_eq!((layer, path.as_str()), (1, "ui/scratch.json"));
    let defs = session
        .workspace
        .layer(1)
        .unwrap()
        .text("ui/_ui_defs.json")
        .unwrap();
    assert!(defs.contains("ui/scratch.json"));
    assert_eq!(
        api::pick_screen(&mut session, layer, &path).as_deref(),
        Some("mine.my_screen")
    );
    let mut shown = view([320, 180]);
    shown.reference = "mine.my_screen".into();
    let frame = session.frame(&shown);
    assert_eq!(rect(&frame, "/my_screen/s")[2..], [10.0, 10.0]);

    assert_eq!(session.workspace.add_layer("later pack"), 1);
    assert_eq!(session.workspace.scratch_index(), Some(2));
    let (_, second) = session.workspace.new_scratch_file("");
    assert_eq!(second, "ui/scratch_2.json");
    assert_eq!(
        api::pick_screen(&mut session, 2, &second).as_deref(),
        Some("scratch_2.main_screen")
    );

    let plan = export::plan(&mut session.workspace, export::Mode::Changed, None, false);
    let names: Vec<&str> = plan.files.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "ui/_global_variables.json",
            "ui/_ui_defs.json",
            "ui/scratch.json",
            "ui/scratch_2.json"
        ]
    );
}
