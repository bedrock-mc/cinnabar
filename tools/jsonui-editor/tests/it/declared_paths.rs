//! Pack indexes select editor documents from any pack-relative directory.

use std::io::{Cursor, Write};

use jsonui_editor::{Session, api, export};

/// Builds authored UI files with a custom directory and an unlisted decoy.
fn files() -> Vec<(String, Vec<u8>)> {
    [
        ("manifest.json", "{}"),
        ("ui/_global_variables.json", r#"{"$caption":"Custom folder"}"#),
        ("ui/_ui_defs.json", r#"{/* index */ "ui_defs":["@behub/nested/screen.json","@behub/common.uidx"]}"#),
        ("@behub/common.uidx", r#"{"namespace":"custom","label":{"type":"label","text":"$caption"}}"#),
        ("@behub/nested/screen.json", r#"{"namespace":"custom","main":{"type":"screen","controls":[{"title@custom.label":{}}]}}"#),
        ("ui/unlisted.json", r#"{"namespace":"decoy","main":{"type":"screen"}}"#),
    ]
    .into_iter()
    .map(|(path, text)| (path.to_owned(), text.as_bytes().to_vec()))
    .collect()
}

/// Checks the file picker, resolved controls and definition locations together.
fn assert_custom_files(session: &mut Session) {
    let paths: Vec<_> = session.workspace.layers()[0].ui_paths().collect();
    assert!(paths.contains(&"@behub/nested/screen.json"));
    assert!(paths.contains(&"@behub/common.uidx"));
    assert!(!paths.contains(&"ui/unlisted.json"));
    assert_eq!(
        api::pick_screen(session, 0, "@behub/nested/screen.json").as_deref(),
        Some("custom.main")
    );
    assert!(!api::has_control(session, "decoy.main"));
    let resolved = json_ui::resolve(
        &session.catalog(),
        "custom.main",
        &json_ui::Context::empty(),
    )
    .control
    .unwrap();
    assert_eq!(resolved.children[0].properties["text"], "Custom folder");
    assert_eq!(
        session.index().defs("custom.label")[0].path,
        "@behub/common.uidx"
    );
}

#[test]
fn folder_import_loads_and_lists_indexed_custom_directories() {
    let mut session = Session::default();
    let layer = session.workspace.add_layer("pack");
    let wrapped = files()
        .into_iter()
        .map(|(path, bytes)| (format!("Pack/{path}"), bytes))
        .collect();
    session.workspace.add_files(layer, wrapped, vec![]);
    assert_custom_files(&mut session);
}

#[test]
fn archive_import_loads_and_lists_indexed_custom_directories() {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in files() {
        archive
            .start_file(
                format!("Pack/{path}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    let mut session = Session::default();
    let layer = session.workspace.add_layer("archive");
    session
        .workspace
        .add_archive(layer, archive.finish().unwrap().into_inner())
        .unwrap();
    assert_custom_files(&mut session);
}

#[test]
fn inherited_custom_paths_keep_overrides_and_exports() {
    let mut session = Session::default();
    let base = session.workspace.add_layer("base");
    session.workspace.add_files(base, files(), vec![]);
    let overlay = session.workspace.add_layer("overlay");
    session.workspace.add_files(
        overlay,
        vec![(
            "@behub/common.uidx".into(),
            br#"{"label":{"text":"Overlay"}}"#.to_vec(),
        )],
        vec![],
    );
    assert!(
        session.workspace.layers()[overlay]
            .ui_paths()
            .any(|path| path == "@behub/common.uidx")
    );
    assert_eq!(
        session
            .index()
            .file_namespace(overlay, "@behub/common.uidx"),
        Some("custom")
    );
    session.workspace.edit(
        overlay,
        "@behub/common.uidx",
        r#"{"label":{"text":"Edited"}}"#,
    );
    let plan = export::plan(&mut session.workspace, export::Mode::Overlay, None, false);
    assert!(plan.files.contains_key("@behub/common.uidx"));
    let exported = session.workspace.add_layer("export");
    session
        .workspace
        .add_files(exported, plan.files.into_iter().collect(), vec![]);
    let root = json_ui::resolve(
        &session.catalog(),
        "custom.main",
        &json_ui::Context::empty(),
    )
    .control
    .unwrap();
    assert_eq!(root.children[0].properties["text"], "Edited");
}

#[test]
fn later_indexes_select_earlier_files_and_pending_folder_bytes() {
    let mut session = Session::default();
    let base = session.workspace.add_layer("base");
    session.workspace.add_files(
        base,
        vec![
            ("ui/_ui_defs.json".into(), br#"{"ui_defs":[]}"#.to_vec()),
            ("ui/_global_variables.json".into(), b"{}".to_vec()),
        ],
        vec!["@behub/late.json".into()],
    );
    assert!(session.workspace.wanted_ui_files().is_empty());
    let upper = session.workspace.add_layer("index");
    session.workspace.add_files(
        upper,
        vec![(
            "ui/_ui_defs.json".into(),
            br#"{"ui_defs":["@behub/late.json"]}"#.to_vec(),
        )],
        vec![],
    );
    assert_eq!(
        session.workspace.wanted_ui_files(),
        [(base, "@behub/late.json".into())]
    );
    assert!(!api::has_control(&mut session, "late.main"));
    session.workspace.supply(
        base,
        "@behub/late.json",
        br#"{"namespace":"late","main":{"type":"screen"}}"#.to_vec(),
    );
    assert!(api::has_control(&mut session, "late.main"));
    assert!(session.workspace.wanted_ui_files().is_empty());
    session.workspace.remove_layer(upper);
    assert!(!api::has_control(&mut session, "late.main"));
    assert!(
        !session.workspace.layers()[base]
            .ui_paths()
            .any(|path| path == "@behub/late.json")
    );
}

#[test]
fn definition_locations_follow_the_catalogs_sorted_path_order() {
    let mut session = Session::default();
    let layer = session.workspace.add_layer("pack");
    session.workspace.add_files(
        layer,
        vec![
            ("ui/_global_variables.json".into(), b"{}".to_vec()),
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["@behub/z.json","@behub/a.json","@behub/z.json"]}"#.to_vec(),
            ),
            (
                "@behub/a.json".into(),
                br#"{"namespace":"custom","label":{"type":"label","text":"A"}}"#.to_vec(),
            ),
            (
                "@behub/z.json".into(),
                br#"{"namespace":"custom","label":{"type":"label","text":"Z"}}"#.to_vec(),
            ),
        ],
        vec![],
    );
    assert_eq!(
        session.catalog().lookup("custom", "label").unwrap().props["text"],
        "Z"
    );
    assert_eq!(
        session.index().defs("custom.label")[0].path,
        "@behub/z.json"
    );
}

#[test]
fn editing_a_missing_declared_file_makes_it_available() {
    let mut session = Session::default();
    let layer = session.workspace.add_layer("pack");
    session.workspace.add_files(
        layer,
        vec![(
            json_ui::UI_DEFS_PATH.into(),
            br#"{"ui_defs":["@behub/new.json"]}"#.to_vec(),
        )],
        vec![],
    );
    assert!(!api::has_control(&mut session, "custom.main"));
    session.workspace.edit(
        layer,
        "@behub/new.json",
        r#"{"namespace":"custom","main":{"type":"screen"}}"#,
    );
    assert!(api::has_control(&mut session, "custom.main"));
    assert!(
        session.workspace.layers()[layer]
            .ui_paths()
            .any(|path| path == "@behub/new.json")
    );
}
