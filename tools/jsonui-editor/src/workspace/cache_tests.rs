use super::*;

fn label(workspace: &mut Workspace) -> String {
    json_ui::resolve(
        &workspace.catalog(),
        "test.label",
        &json_ui::Context::desktop(),
    )
    .control
    .unwrap()
    .properties["text"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn assert_empty_prefix(workspace: &Workspace) {
    assert_eq!(workspace.prefix.len(), workspace.layers.len());
    assert!(
        workspace
            .prefix
            .iter()
            .all(|(generation, catalog)| { *generation == 0 && catalog.is_none() })
    );
}

#[test]
fn topology_changes_reset_initialized_prefix_entries_without_changing_resolution() {
    let mut workspace = Workspace::default();
    let base = workspace.add_layer("base");
    workspace.add_files(
        base,
        vec![
            (UI_DEFS.into(), br#"{"ui_defs":["ui/test.json"]}"#.to_vec()),
            (
                "ui/test.json".into(),
                br#"{"namespace":"test","label":{"type":"label","text":"base"}}"#.to_vec(),
            ),
        ],
        Vec::new(),
    );
    assert_empty_prefix(&workspace);
    let base_catalog = workspace.catalog();
    assert!(Arc::ptr_eq(&base_catalog, &workspace.catalog()));
    assert_eq!(label(&mut workspace), "base");

    let upper = workspace.add_layer("overlay");
    assert_empty_prefix(&workspace);
    workspace.edit(
        upper,
        "ui/test.json",
        r#"{"namespace":"test","label":{"text":"overlay"}}"#,
    );
    assert_eq!(label(&mut workspace), "overlay");
    assert!(!Arc::ptr_eq(&base_catalog, &workspace.catalog()));

    workspace.remove_layer(upper);
    assert_empty_prefix(&workspace);
    assert_eq!(label(&mut workspace), "base");
    let (scratch, _) = workspace.new_scratch_file("");
    assert_eq!(workspace.prefix[scratch].0, 0);
    assert!(workspace.prefix[scratch].1.is_none());
    assert_eq!(label(&mut workspace), "base");
    workspace.add_layer("below scratch");
    assert_empty_prefix(&workspace);
    assert_eq!(workspace.scratch_index(), Some(workspace.layers.len() - 1));
    assert_eq!(label(&mut workspace), "base");
}
