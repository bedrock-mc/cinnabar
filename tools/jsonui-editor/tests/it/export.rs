//! Pack export: overlays round-trip (the unedited pack plus the exported
//! overlay resolves exactly as the edited workspace), manifests, packaging and
//! the rule that only edited or authored files ship.

use std::io::Read;
use std::path::Path;

use json_ui::{Context, resolve};
use jsonui_editor::export::{self, Format, Mode, PackInfo};
use jsonui_editor::workspace::Workspace;

const HEADER: &str = "0f5b8c56-2a53-4a61-9d0e-6f2f3c7e1a10";
const MODULE: &str = "7c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";

fn base_files() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/base");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((relative, std::fs::read(&path).unwrap()));
            }
        }
    }
    out
}

fn workspace() -> Workspace {
    let mut workspace = Workspace::default();
    let layer = workspace.add_layer("vanilla stand-in");
    workspace.add_files(layer, base_files(), Vec::new());
    workspace
}

fn text(workspace: &Workspace, path: &str) -> String {
    workspace.layer(0).unwrap().text(path).unwrap()
}

/// Apply `edit` to a file of the bottom layer as an author would.
fn edit(workspace: &mut Workspace, path: &str, edit: impl Fn(&mut serde_json::Value)) {
    let mut value = export::parse(&text(workspace, path)).unwrap();
    edit(&mut value);
    workspace.edit(0, path, &serde_json::to_string_pretty(&value).unwrap());
}

fn children<'a>(value: &'a mut serde_json::Value, pointer: &str) -> &'a mut Vec<serde_json::Value> {
    value.pointer_mut(pointer).unwrap().as_array_mut().unwrap()
}

/// The unedited base plus the exported overlay must resolve every listed
/// control exactly as the edited workspace does.
fn assert_round_trip(mut edited: Workspace, references: &[&str]) -> export::Plan {
    let plan = export::plan(&mut edited, Mode::Overlay, None, false);
    let mut layered = workspace();
    let overlay = layered.add_layer("overlay");
    let files = plan
        .files
        .iter()
        .map(|(p, b)| (p.clone(), b.clone()))
        .collect();
    layered.add_files(overlay, files, Vec::new());
    let (want, got) = (edited.catalog(), layered.catalog());
    for reference in references {
        let context = Context::desktop();
        let want = resolve(&want, reference, &context).control;
        let got = resolve(&got, reference, &context).control;
        assert!(
            want.is_some(),
            "{reference} resolves in the edited workspace"
        );
        assert_eq!(
            want, got,
            "{reference} differs through the overlay:\n{:#?}",
            plan.files
        );
    }
    plan
}

const SCREEN: &str = "example.example_screen";
const CONTENT: &str =
    "/example_screen/controls/1/card@example_common.fill/controls/0/content/controls";

// Properties, a variable default, an inserted and a removed child, and a nested
// child's property land as partial overrides and `modifications`.
#[test]
fn property_and_child_edits_round_trip() {
    let mut edited = workspace();
    edit(&mut edited, "ui/example_screen.json", |v| {
        v["example_screen"]["$card_width|default"] = 300.into();
        let content = children(v, CONTENT);
        content.retain(|c| c.get("gap_2").is_none());
        content.insert(
            1,
            serde_json::json!({ "badge": { "type": "label", "text": "NEW" } }),
        );
        content
            .iter_mut()
            .find_map(|c| c.get_mut("status"))
            .unwrap()["text"] = "Online".into();
    });
    let plan = assert_round_trip(edited, &[SCREEN]);
    let overlay = export::parse(&String::from_utf8_lossy(
        &plan.files["ui/example_screen.json"],
    ))
    .unwrap();
    assert_eq!(overlay["namespace"], "example");
    let raw = String::from_utf8_lossy(&plan.files["ui/example_screen.json"]).into_owned();
    assert!(raw.starts_with("{\n  \"namespace\": \"example\","), "{raw}");
    assert_eq!(overlay["example_screen"]["$card_width|default"], 300);
    assert!(
        overlay["example_screen"].get("controls").is_none(),
        "unchanged lists are not restated"
    );
    let content = &overlay["example_screen/card/content"]["modifications"];
    assert_eq!(content[0]["operation"], "remove");
    assert_eq!(content[1]["operation"], "insert_after");
    assert_eq!(
        overlay["example_screen/card/content/status"],
        serde_json::json!({ "text": "Online" })
    );
    assert!(
        !plan.files.contains_key("ui/example_common.json"),
        "untouched files stay out"
    );
}

// New top-level controls, appended bindings, changed globals and a child whose
// base changed; untouched definitions stay out of the overlay.
#[test]
fn common_file_and_global_edits_round_trip() {
    let mut edited = workspace();
    edit(&mut edited, "ui/example_common.json", |v| {
        v["heading"]["color"] = serde_json::json!([0.2, 0.8, 1.0]);
        v["chip"] =
            serde_json::json!({ "type": "image", "size": [8, 8], "color": "$example_accent" });
        let row = children(v, "/row/controls");
        let label = row.iter_mut().find_map(|c| c.get_mut("row_label")).unwrap();
        label["bindings"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({ "binding_name": "#row_color" }));
        let background = row.remove(0);
        row.insert(0, serde_json::json!({ "row_bg@example_common.heading": background["row_bg@example_common.fill"] }));
    });
    edit(&mut edited, "ui/_global_variables.json", |v| {
        v["$example_accent"] = serde_json::json!([0.1, 0.9, 0.3]);
    });
    let plan = assert_round_trip(
        edited,
        &[SCREEN, "example_common.row", "example_common.chip"],
    );
    let common = export::parse(&String::from_utf8_lossy(
        &plan.files["ui/example_common.json"],
    ))
    .unwrap();
    assert!(common.get("pulse").is_none() && common.get("fill").is_none());
    assert_eq!(
        common["row/row_label"]["modifications"][0]["operation"],
        "insert_back"
    );
    assert_eq!(common["row"]["modifications"][0]["operation"], "replace");
    let globals = export::parse(&String::from_utf8_lossy(
        &plan.files["ui/_global_variables.json"],
    ))
    .unwrap();
    assert_eq!(globals.as_object().unwrap().len(), 1);
}

// A reordered child list ships whole, and a scratch file ships as written and
// is listed in `_ui_defs.json`.
#[test]
fn reorders_and_scratch_files_round_trip() {
    let mut edited = workspace();
    edit(&mut edited, "ui/example_screen.json", |v| {
        children(v, CONTENT).swap(0, 1)
    });
    let scratch =
        r#"{ "namespace": "mine", "my_screen@example_common.fill": { "type": "screen" } }"#;
    edited.new_scratch_file(scratch);
    let plan = assert_round_trip(edited, &[SCREEN, "mine.my_screen"]);
    let overlay = export::parse(&String::from_utf8_lossy(
        &plan.files["ui/example_screen.json"],
    ))
    .unwrap();
    assert!(overlay["example_screen/card/content"]["controls"].is_array());
    assert_eq!(
        String::from_utf8_lossy(&plan.files["ui/scratch.json"]),
        scratch
    );
    let defs = export::parse(&String::from_utf8_lossy(&plan.files["ui/_ui_defs.json"])).unwrap();
    assert_eq!(defs, serde_json::json!({ "ui_defs": ["ui/scratch.json"] }));
}

// Deletions an overlay cannot express are reported, not silently dropped.
#[test]
fn inexpressible_deletions_are_noted() {
    let mut edited = workspace();
    edit(&mut edited, "ui/example_common.json", |v| {
        v.as_object_mut().unwrap().remove("pulse_back");
        v["heading"].as_object_mut().unwrap().remove("shadow");
    });
    let plan = export::plan(&mut edited, Mode::Overlay, None, false);
    assert!(
        plan.notes
            .iter()
            .any(|n| n.contains("`pulse_back` was deleted"))
    );
    assert!(plan.notes.iter().any(|n| n.contains("lost `shadow`")));
}

fn pack() -> PackInfo {
    PackInfo::new("My UI", [HEADER.into(), MODULE.into()])
}

#[test]
fn manifest_follows_the_resource_pack_format() {
    let manifest = pack().manifest().unwrap();
    assert_eq!(manifest["format_version"], 2);
    let header = &manifest["header"];
    assert_eq!(
        (header["name"].as_str(), header["uuid"].as_str()),
        (Some("My UI"), Some(HEADER))
    );
    assert_eq!(header["version"], serde_json::json!([1, 0, 0]));
    let engine = export::default_min_engine_version();
    assert_eq!(header["min_engine_version"], serde_json::json!(engine));
    assert!(
        engine[0] >= 1 && engine[1] > 0,
        "derived from the pinned game version"
    );
    let module = &manifest["modules"][0];
    assert_eq!(
        (module["type"].as_str(), module["uuid"].as_str()),
        (Some("resources"), Some(MODULE))
    );
    assert_eq!(module["version"], header["version"]);
    let mut same = pack();
    same.module_uuid = HEADER.into();
    assert!(same.manifest().is_err());
    let mut bad = pack();
    bad.header_uuid = "not-a-uuid".into();
    assert!(bad.manifest().is_err());
}

fn zip_names(bytes: &[u8]) -> Vec<String> {
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    names.sort();
    names
}

// A .mcpack has manifest.json at its root; a .mcaddon wraps it; either reads
// back to the same pack info, so a re-export keeps its UUIDs.
#[test]
fn packages_keep_manifest_at_the_root_and_read_back() {
    let mut edited = workspace();
    edited.new_scratch_file("");
    let plan = export::plan(&mut edited, Mode::Overlay, None, false);
    let icon = {
        let mut png = Vec::new();
        let image = image::RgbaImage::new(4, 4);
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    };
    let mcpack = export::package(&plan, &pack(), Some(&icon), Format::Mcpack).unwrap();
    assert_eq!(
        zip_names(&mcpack),
        [
            "manifest.json",
            "pack_icon.png",
            "ui/_ui_defs.json",
            "ui/scratch.json"
        ]
    );
    let addon = export::package(&plan, &pack().bumped(), None, Format::Mcaddon).unwrap();
    assert_eq!(zip_names(&addon), ["My_UI.mcpack"]);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&addon)).unwrap();
    let mut inner = Vec::new();
    archive
        .by_index(0)
        .unwrap()
        .read_to_end(&mut inner)
        .unwrap();
    assert!(zip_names(&inner).contains(&"manifest.json".to_owned()));
    let back = export::read_pack_info(&addon).unwrap();
    assert_eq!(
        (back.header_uuid.as_str(), back.module_uuid.as_str()),
        (HEADER, MODULE)
    );
    assert_eq!(back.version, [1, 0, 1]);
    assert_eq!(export::read_pack_info(&mcpack).unwrap(), pack());
    assert!(export::package(&plan, &pack(), Some(b"GIF89a"), Format::Zip).is_err());
}

// A full copy of a loaded pack takes only its edited files unless the user
// confirms the pack is theirs; changed-files mode ships edits in full.
#[test]
fn only_edited_or_owned_files_ship() {
    let mut edited = workspace();
    edit(&mut edited, "ui/example_common.json", |v| {
        v["heading"]["color"] = "red".into()
    });
    let plan = export::plan(&mut edited, Mode::Full, Some(0), false);
    assert_eq!(
        plan.files.keys().collect::<Vec<_>>(),
        ["ui/example_common.json"]
    );
    assert!(plan.skipped.contains(&"ui/example_screen.json".to_owned()));
    let owned = export::plan(&mut edited, Mode::Full, Some(0), true);
    assert!(owned.files.contains_key("ui/example_screen.json"));
    assert!(
        !owned.files.contains_key("manifest.json"),
        "the export writes its own manifest"
    );
    let changed = export::plan(&mut edited, Mode::Changed, None, false);
    assert_eq!(
        changed.files.keys().collect::<Vec<_>>(),
        ["ui/example_common.json"]
    );
}

#[test]
fn review_generated_child_operations_preserve_authored_modifications() {
    let before = r#"{"namespace":"test","main":{"type":"panel","controls":[{"a":{"type":"label","text":"A"}}]}}"#;
    let after = r#"{"namespace":"test","main":{"type":"panel","controls":[{"a":{"type":"label","text":"A"}},{"b":{"type":"label","text":"B"}}],"modifications":[{"array_name":"controls","operation":"remove","control_name":"a"}]}}"#;
    let mut workspace = Workspace::default();
    let layer = workspace.add_layer("base");
    workspace.add_files(
        layer,
        vec![("ui/test.json".into(), before.as_bytes().to_vec())],
        vec![],
    );
    workspace.edit(layer, "ui/test.json", after);
    let plan = export::plan(&mut workspace, Mode::Overlay, None, false);
    let exported = export::parse(&String::from_utf8_lossy(&plan.files["ui/test.json"])).unwrap();
    let operations = exported["main"]["modifications"].as_array().unwrap();
    assert!(operations.iter().any(|operation| operation["operation"] == "remove" && operation["control_name"] == "a"));
    assert!(operations.iter().any(|operation| {
        operation["operation"]
            .as_str()
            .is_some_and(|operation| operation.starts_with("insert_"))
    }));
}

#[test]
fn review_changed_existing_modifications_round_trip_with_child_edits() {
    let before = r#"{"namespace":"test","main":{"controls":[{"a":{"type":"label","text":"A"}}],"modifications":[{"array_name":"controls","operation":"insert_back","value":[{"c":{"type":"label","text":"C"}}]}]}}"#;
    let after = before.replace("\"c\"", "\"d\"").replace("\"C\"", "\"D\"").replace(
        "\"controls\":[{\"a\":{\"type\":\"label\",\"text\":\"A\"}}]",
        "\"controls\":[{\"a\":{\"type\":\"label\",\"text\":\"A\"}},{\"b\":{\"type\":\"label\",\"text\":\"B\"}}]",
    );
    let make = || {
        let mut workspace = Workspace::default();
        let base = workspace.add_layer("base");
        workspace.add_files(
            base,
            vec![
                (
                    "ui/_ui_defs.json".into(),
                    br#"{"ui_defs":["ui/test.json"]}"#.to_vec(),
                ),
                ("ui/_global_variables.json".into(), b"{}".to_vec()),
                (
                    "ui/test.json".into(),
                    br#"{"namespace":"test","main":{"type":"panel"}}"#.to_vec(),
                ),
            ],
            vec![],
        );
        let upper = workspace.add_layer("pack");
        workspace.add_files(
            upper,
            vec![("ui/test.json".into(), before.as_bytes().to_vec())],
            vec![],
        );
        workspace
    };
    let mut edited = make();
    edited.edit(1, "ui/test.json", &after);
    let plan = export::plan(&mut edited, Mode::Overlay, None, false);
    let mut layered = make();
    let overlay = layered.add_layer("export");
    layered.add_files(overlay, plan.files.into_iter().collect(), vec![]);
    let context = Context::desktop();
    let want = resolve(&edited.catalog(), "test.main", &context)
        .control
        .unwrap();
    let got = resolve(&layered.catalog(), "test.main", &context)
        .control
        .unwrap();
    assert_eq!(
        want.children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "d"]
    );
    assert_eq!(want, got);
}
