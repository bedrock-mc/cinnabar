use super::*;

pub(super) fn layout() -> InstallLayout {
    let mut layout = crate::install_layout::scratch("dressing-room-catalog");
    layout.resource_root = layout.user_data_root.join("runtime");
    layout
}

fn png(path: &Path, color: [u8; 4]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    image::RgbaImage::from_pixel(
        protocol::CLASSIC_SKIN_SIDE as u32,
        protocol::CLASSIC_SKIN_SIDE as u32,
        image::Rgba(color),
    )
    .save(path)
    .unwrap();
}

pub(super) fn native_fixture(layout: &InstallLayout) -> PathBuf {
    let root = layout.resource_root.join("skin_packs/vanilla");
    fs::create_dir_all(&root).unwrap();
    let models = [SkinModel::Classic, SkinModel::Slim].map(|model| serde_json::json!({
        "description":{"identifier":model.geometry(), "texture_width":protocol::CLASSIC_SKIN_SIDE,"texture_height":protocol::CLASSIC_SKIN_SIDE},
        "bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]}]
    }));
    fs::write(
        root.join("geometry.json"),
        serde_json::json!({"format_version":"1.12.0","minecraft:geometry":models}).to_string(),
    )
    .unwrap();
    fs::write(root.join("skins.json"), serde_json::json!({"skins":[
        {"localization_name":launcher::dressing_room::STARTER_SKIN_NAMES[0],"geometry":SkinModel::Classic.geometry(),"texture":"first.png","type":"free"},
        {"localization_name":"Ari","geometry":SkinModel::Classic.geometry(),"texture":"second.png","type":"free"},
        {"localization_name":launcher::dressing_room::STARTER_SKIN_NAMES[1],"geometry":SkinModel::Slim.geometry(),"texture":"third.png","type":"free"},
        {"localization_name":"Custom","geometry":SkinModel::Classic.geometry(),"texture":"custom.png","type":"custom"}
    ]}).to_string()).unwrap();
    for (file, color) in [
        ("first.png", [255, 0, 0, 255]),
        ("second.png", [0, 255, 0, 255]),
        ("third.png", [0, 0, 255, 255]),
    ] {
        png(&root.join(file), color);
    }
    root
}

#[test]
fn native_roster_uses_declared_models_and_matches_existing_default_without_duplicate() {
    let layout = layout();
    let root = native_fixture(&layout);
    let skin = read_png(&root.join("first.png")).unwrap();
    let mut local = LocalPlayerSkin::generated_default("fixture");
    local.set_selection(&skin, SkinModel::Classic);
    let view = load(&layout, &local);
    assert_eq!(
        view.skins.len(),
        launcher::dressing_room::STARTER_SKIN_NAMES.len()
    );
    assert_eq!(view.selected, Some(0));
    assert_eq!(view.skins[1].model, SkinModel::Slim);
    assert!(Arc::ptr_eq(
        &view.skins[0].skin.geometry.as_ref().unwrap().geometry_data,
        &view.skins[1].skin.geometry.as_ref().unwrap().geometry_data
    ));
    assert!(view.skins.iter().all(|skin| !skin.imported));
}

#[test]
fn removed_starter_selection_restores_steve_without_a_duplicate_current_entry() {
    let layout = layout();
    let root = native_fixture(&layout);
    fs::create_dir_all(&layout.user_config_root).unwrap();
    fs::write(
        layout.skin_selection_file(),
        r#"{"selected":"vanilla:Ari","imported":[]}"#,
    )
    .unwrap();
    let mut local = LocalPlayerSkin::generated_default("fixture");
    local.set_selection(
        &read_png(&root.join("second.png")).unwrap(),
        SkinModel::Classic,
    );
    let view = load(&layout, &local);
    assert_eq!(
        view.skins.len(),
        launcher::dressing_room::STARTER_SKIN_NAMES.len()
    );
    assert_eq!(
        view.selected_skin().unwrap().name,
        launcher::dressing_room::STARTER_SKIN_NAMES[0]
    );
    assert_eq!(
        LocalPlayerSkin::load(&layout, "fixture").standard_skin(),
        view.selected_skin().unwrap().skin
    );
}

#[test]
fn imported_rename_and_delete_keep_startup_skin_and_original_file_correct() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let first = layout.user_data_root.join("first-import.png");
    let second = layout.user_data_root.join("second-import.png");
    png(&first, [1, 25, 150, 255]);
    png(&second, [180, 50, 80, 255]);
    import(&layout, &mut view, &first).unwrap();
    let first_index = view.selected.unwrap();
    let private_first = view.selected_skin().unwrap().path.clone();
    let skin_before = view.selected_skin().unwrap().skin.clone();
    assert!(Arc::ptr_eq(
        skin_before.geometry.as_ref().unwrap(),
        view.skins[0].skin.geometry.as_ref().unwrap()
    ));
    rename(&layout, &mut view, first_index, "  My renamed skin  ").unwrap();
    assert_eq!(view.selected_skin().unwrap().name, "My renamed skin");
    assert_eq!(view.selected_skin().unwrap().skin, skin_before);
    assert_eq!(load(&layout, &local).selected_skin(), view.selected_skin());
    import(&layout, &mut view, &second).unwrap();
    let selected_id = view.selected_skin().unwrap().id.clone();
    delete(&layout, &mut view, first_index).unwrap();
    assert_eq!(view.selected_skin().unwrap().id, selected_id);
    assert_eq!(view.selected, Some(first_index));
    assert!(!Path::new(&private_first).exists());
    assert!(first.exists());
    let active = view.selected.unwrap();
    let private_second = view.selected_skin().unwrap().path.clone();
    delete(&layout, &mut view, active).unwrap();
    assert_eq!(
        view.selected_skin().unwrap().name,
        launcher::dressing_room::STARTER_SKIN_NAMES[0]
    );
    assert!(!Path::new(&private_second).exists());
    assert!(second.exists());
    assert_eq!(
        LocalPlayerSkin::load(&layout, "fixture").standard_skin(),
        view.selected_skin().unwrap().skin
    );
    assert!(
        load(&layout, &local)
            .skins
            .iter()
            .all(|entry| !entry.imported)
    );
}

#[test]
fn imported_edits_refuse_native_skins_and_restore_files_when_saving_fails() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    assert!(rename(&layout, &mut view, 0, "custom").is_err());
    assert!(delete(&layout, &mut view, 0).is_err());
    let source = layout.user_data_root.join("import.png");
    png(&source, [20, 40, 60, 255]);
    import(&layout, &mut view, &source).unwrap();
    let before = view.clone();
    let index = view.selected.unwrap();
    let private = view.selected_skin().unwrap().path.clone();
    assert!(rename(&layout, &mut view, index, " \n ").is_err());
    fs::remove_file(layout.skin_selection_file()).unwrap();
    fs::create_dir(layout.skin_selection_file()).unwrap();
    assert!(rename(&layout, &mut view, index, "new").is_err());
    assert_eq!(view, before);
    assert!(delete(&layout, &mut view, index).is_err());
    assert_eq!(view, before);
    assert!(Path::new(&private).exists());
    assert!(source.exists());
}

#[test]
fn imported_skin_and_model_survive_restart_and_deleted_original() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("original.png");
    png(&source, [120, 60, 240, 255]);
    import(&layout, &mut view, &source).unwrap();
    set_model(&layout, &mut view, SkinModel::Slim).unwrap();
    let expected = view.selected_skin().unwrap().clone();
    fs::remove_file(source).unwrap();
    let loaded = load(&layout, &local);
    assert_eq!(loaded.selected_skin(), Some(&expected));
    let startup = LocalPlayerSkin::load(&layout, "fixture");
    assert_eq!(startup.standard_skin(), expected.skin);
    assert_eq!(
        startup.to_client_skin().arm_size,
        SkinModel::Slim.arm_size()
    );
    let mut again = loaded.clone();
    import(&layout, &mut again, Path::new(&expected.path)).unwrap();
    assert_eq!(again.skins.len(), loaded.skins.len());
}

#[test]
fn invalid_import_and_native_model_edit_preserve_saved_selection() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    select(&layout, &mut view, 0).unwrap();
    let before = fs::read(layout.skin_selection_file()).unwrap();
    assert!(set_model(&layout, &mut view, SkinModel::Slim).is_err());
    let source = layout.user_data_root.join("invalid.png");
    fs::write(&source, b"not a PNG").unwrap();
    assert!(import(&layout, &mut view, &source).is_err());
    assert_eq!(view.selected, Some(0));
    assert_eq!(fs::read(layout.skin_selection_file()).unwrap(), before);
}

#[test]
fn skin_png_decode_rejects_images_outside_classic_upload_limit() {
    let layout = layout();
    let path = layout.user_data_root.join("large.png");
    fs::create_dir_all(&layout.user_data_root).unwrap();
    image::RgbaImage::new(
        protocol::MAX_CLASSIC_SKIN_SIDE as u32 + 1,
        protocol::CLASSIC_SKIN_SIDE as u32,
    )
    .save(&path)
    .unwrap();
    assert!(read_png(&path).is_err());
}

#[test]
fn imported_classic_alpha_matches_the_appearance_recipients_receive() {
    let layout = layout();
    let path = layout.user_data_root.join("partial.png");
    png(&path, [120, 60, 240, 56]);
    let skin = read_png(&path).unwrap();
    let side = protocol::CLASSIC_SKIN_SIDE;
    assert_eq!(
        skin.rgba8[(8 * side) * 4 + 3],
        255,
        "body sides become opaque"
    );
    assert_eq!(
        skin.rgba8[32 * 4 + 3],
        255,
        "hat alpha exceeds the native cutoff"
    );
    png(&path, [120, 60, 240, 0]);
    let transparent = read_png(&path).unwrap();
    assert_eq!(
        transparent.rgba8[(8 * side) * 4 + 3],
        255,
        "classic body coverage is protected"
    );
    assert_eq!(
        transparent.rgba8[32 * 4 + 3],
        0,
        "transparent hat remains transparent"
    );
}

#[test]
fn imported_skin_capacity_never_acknowledges_an_unrestorable_selection() {
    let layout = layout();
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("first.png");
    png(&source, [10, 20, 30, 255]);
    import(&layout, &mut view, &source).unwrap();
    let first = view.selected_skin().unwrap().clone();
    view.skins = (0..MAX_IMPORTED_ITEMS)
        .map(|index| {
            let mut entry = first.clone();
            if index != 0 {
                entry.id = format!("capacity:{index}");
            }
            entry
        })
        .collect();
    view.selected = Some(0);
    save(&layout, &view).unwrap();
    let before = fs::read(layout.skin_selection_file()).unwrap();
    let second = layout.user_data_root.join("second.png");
    png(&second, [40, 50, 60, 255]);
    assert!(
        import(&layout, &mut view, &second).is_err(),
        "full catalog rejects a new unique import"
    );
    assert_eq!(fs::read(layout.skin_selection_file()).unwrap(), before);
    assert_eq!(view.selected, Some(0));
    import(&layout, &mut view, &source).unwrap();
    assert_eq!(
        view.skins.len(),
        MAX_IMPORTED_ITEMS,
        "an existing import can still be selected"
    );
}
