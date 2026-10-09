use super::*;

pub(super) fn layout() -> InstallLayout {
    let mut layout = crate::install_layout::scratch("dressing-room-catalog");
    layout.resource_root = layout.user_data_root.join("runtime");
    layout
}

/// Decodes a PNG file as an imported classic skin.
fn read_png(path: &Path) -> Result<protocol::StandardSkin, String> {
    decode_png_with_model(&png_bytes(path)?, SkinModel::Classic)
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

/// A skin of one color, shaped like a starter skin.
pub(super) fn fixture_skin(color: [u8; 4]) -> protocol::StandardSkin {
    let side = assets::STARTER_SKIN_SIDE;
    standard_skin(side, side, color.repeat((side * side) as usize)).unwrap()
}

/// Writes a starter-skin carrier holding red Steve and blue Alex.
pub(super) fn native_fixture(layout: &InstallLayout) {
    let models = [SkinModel::Classic, SkinModel::Slim].map(|model| serde_json::json!({
        "description":{"identifier":model.geometry(), "texture_width":protocol::CLASSIC_SKIN_SIDE,"texture_height":protocol::CLASSIC_SKIN_SIDE},
        "bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]}]
    }));
    let side = assets::STARTER_SKIN_SIDE as usize;
    let carrier = assets::StarterSkins {
        geometry: serde_json::json!({"format_version":"1.12.0","minecraft:geometry":models})
            .to_string()
            .into(),
        skins: assets::STARTER_SKIN_SOURCES
            .iter()
            .zip([[255, 0, 0, 255], [0, 0, 255, 255]])
            .map(|(source, color)| assets::StarterSkin {
                name: source.name.into(),
                slim: source.slim,
                rgba8: color.repeat(side * side).into(),
            })
            .collect(),
    };
    let path = layout.starter_skins_asset();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, assets::encode_starter_skins(&carrier).unwrap()).unwrap();
}

#[test]
fn native_roster_uses_declared_models_and_matches_existing_default_without_duplicate() {
    let layout = layout();
    native_fixture(&layout);
    let skin = fixture_skin([255, 0, 0, 255]);
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
    native_fixture(&layout);
    fs::create_dir_all(&layout.user_config_root).unwrap();
    fs::write(
        layout.skin_selection_file(),
        r#"{"selected":"vanilla:Ari","imported":[]}"#,
    )
    .unwrap();
    let mut local = LocalPlayerSkin::generated_default("fixture");
    local.set_selection(&fixture_skin([0, 255, 0, 255]), SkinModel::Classic);
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

#[test]
fn importing_a_skin_with_geometry_keeps_the_custom_model_across_restarts() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("custom.png");
    png(&source, [20, 160, 230, 255]);
    let name = "geometry.import.fixture";
    let data = serde_json::json!({
        "format_version":"1.12.0",
        "minecraft:geometry":[{
            "description":{"identifier":name,"texture_width":64,"texture_height":64},
            "bones":[{"name":"body","cubes":[{"origin":[-12,0,-4],"size":[24,8,8],"uv":[0,0]}]}]
        }]
    });
    fs::write(source.with_extension("json"), data.to_string()).unwrap();
    import(&layout, &mut view, &source).unwrap();
    let selected = view.selected_skin().unwrap();
    assert_eq!(
        assets::skin_geometry_name(&selected.skin.geometry.as_ref().unwrap().resource_patch)
            .as_deref(),
        Some(name)
    );
    let restored = load(&layout, &local);
    assert_eq!(
        restored.selected_skin().unwrap().skin.geometry,
        selected.skin.geometry
    );
}

#[test]
fn invalid_loose_geometry_does_not_replace_the_selection_or_preferences() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    select(&layout, &mut view, 0).unwrap();
    let before = fs::read(layout.skin_selection_file()).unwrap();
    let source = layout.user_data_root.join("broken.png");
    png(&source, [20, 160, 230, 255]);
    fs::write(source.with_extension("json"), b"{broken").unwrap();
    assert!(import(&layout, &mut view, &source).is_err());
    assert_eq!(view.selected, Some(0));
    assert_eq!(fs::read(layout.skin_selection_file()).unwrap(), before);
}

#[test]
fn native_import_sizes_exclude_128_by_64_and_custom_alpha_preserves_transparency() {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::new(
        protocol::MAX_CLASSIC_SKIN_SIDE as u32,
        protocol::CLASSIC_SKIN_SIDE as u32,
    )
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    assert!(decode_png_with_model(bytes.get_ref(), SkinModel::Classic).is_err());
    assert!(decode_png_with_model(bytes.get_ref(), SkinModel::Custom).is_err());
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(
        protocol::CLASSIC_SKIN_SIDE as u32,
        protocol::CLASSIC_SKIN_SIDE as u32,
        image::Rgba([10, 20, 30, 0]),
    )
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    let skin = decode_png_with_model(bytes.get_ref(), SkinModel::Custom).unwrap();
    assert!(skin.rgba8.chunks_exact(4).all(|pixel| pixel[3] == 0));
}

/// Builds an original two-entry skin pack, with an optional broken second texture.
fn skin_pack(path: &Path, broken_second: bool) {
    use std::io::Write;
    let name = "geometry.pack.fixture";
    let geometry = serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":name,"texture_width":64,"texture_height":64},
        "bones":[{"name":"body","cubes":[{"origin":[-12,0,-4],"size":[24,8,8],"uv":[0,0]}]}]
    }]})
    .to_string();
    let catalog = serde_json::json!({"skins":[
        {"localization_name":"First","texture":"first.png","geometry":name,"type":"free"},
        {"localization_name":"Second","texture":"second.png","geometry":name,"type":"free"}
    ]})
    .to_string();
    let manifest = serde_json::json!({"modules":[{"type":"skin_pack"}],"header":{"min_engine_version":[1,21,0]}}).to_string();
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(64, 64, image::Rgba([20, 160, 230, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in [
        ("manifest.json", manifest.as_bytes()),
        ("skins.json", catalog.as_bytes()),
        ("geometry.json", geometry.as_bytes()),
        ("first.png", png.get_ref().as_slice()),
        (
            "second.png",
            if broken_second {
                b"broken".as_slice()
            } else {
                png.get_ref().as_slice()
            },
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn pack_import_keeps_custom_model_and_engine_version_for_startup_login_and_deletion() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("fixture.mcpack");
    skin_pack(&source, false);
    import(&layout, &mut view, &source).unwrap();
    let selected = view.selected_skin().unwrap().clone();
    assert_eq!(selected.model, SkinModel::Custom);
    assert_eq!(selected.engine_version.as_ref(), "1.21.0");
    assert!(set_model(&layout, &mut view, SkinModel::Slim).is_err());
    fs::remove_file(source).unwrap();
    let restored = LocalPlayerSkin::load(&layout, "fixture");
    assert_eq!(restored.model(), SkinModel::Custom);
    assert_eq!(restored.standard_skin(), selected.skin);
    let upload = restored.to_client_skin();
    let geometry = upload.geometry.unwrap();
    assert_eq!(geometry.engine_version, selected.engine_version.as_ref());
    assert_eq!(
        geometry.geometry_data,
        selected
            .skin
            .geometry
            .as_ref()
            .unwrap()
            .geometry_data
            .as_ref()
    );
    assert_eq!(
        geometry.resource_patch,
        selected
            .skin
            .geometry
            .as_ref()
            .unwrap()
            .resource_patch
            .as_ref()
    );
    let index = view.selected.unwrap();
    delete(&layout, &mut view, index).unwrap();
    assert!(!Path::new(&selected.path).exists());
    assert!(!Path::new(&selected.path).with_extension("json").exists());
}

#[test]
fn a_bad_pack_member_rolls_back_all_new_files_and_preserves_existing_imports() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("old.png");
    png(&source, [180, 50, 80, 255]);
    import(&layout, &mut view, &source).unwrap();
    let before = view.clone();
    let preferences = fs::read(layout.skin_selection_file()).unwrap();
    let source = layout.user_data_root.join("broken.mcpack");
    skin_pack(&source, true);
    assert!(import(&layout, &mut view, &source).is_err());
    assert_eq!(view, before);
    assert_eq!(fs::read(layout.skin_selection_file()).unwrap(), preferences);
    assert_eq!(fs::read_dir(layout.dressing_room_dir()).unwrap().count(), 1);
    assert!(Path::new(&before.selected_skin().unwrap().path).exists());
}

#[test]
fn saved_custom_entries_with_missing_model_metadata_or_invalid_engine_versions_are_skipped() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("fixture.mcpack");
    skin_pack(&source, false);
    import(&layout, &mut view, &source).unwrap();
    let mut preferences: serde_json::Value =
        serde_json::from_slice(&fs::read(layout.skin_selection_file()).unwrap()).unwrap();
    preferences["imported"][0]["engine_version"] = "invalid".into();
    fs::write(layout.skin_selection_file(), preferences.to_string()).unwrap();
    assert!(
        load(&layout, &local)
            .skins
            .iter()
            .all(|entry| !entry.imported)
    );
    preferences["imported"][0]["geometry"] = serde_json::Value::Null;
    fs::write(layout.skin_selection_file(), preferences.to_string()).unwrap();
    assert!(
        load(&layout, &local)
            .skins
            .iter()
            .all(|entry| !entry.imported)
    );
}

#[test]
fn custom_models_without_a_renderable_mesh_do_not_replace_the_selection() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    select(&layout, &mut view, 0).unwrap();
    let before = fs::read(layout.skin_selection_file()).unwrap();
    let source = layout.user_data_root.join("hidden.png");
    png(&source, [20, 160, 230, 255]);
    let geometry = serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.hidden.fixture","texture_width":64,"texture_height":64},
        "bones":[{"name":"body","neverRender":true,"cubes":[{"origin":[0,0,0],"size":[8,8,8],"uv":[0,0]}]}]
    }]}).to_string();
    fs::write(source.with_extension("json"), geometry).unwrap();
    assert!(import(&layout, &mut view, &source).is_err());
    assert_eq!(view.selected, Some(0));
    assert_eq!(fs::read(layout.skin_selection_file()).unwrap(), before);
}

#[test]
fn saved_custom_models_without_drawable_geometry_are_skipped() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("fixture.mcpack");
    skin_pack(&source, false);
    import(&layout, &mut view, &source).unwrap();
    let model_file = Path::new(&view.selected_skin().unwrap().path).with_extension("json");
    let mut geometry: serde_json::Value =
        serde_json::from_slice(&fs::read(&model_file).unwrap()).unwrap();
    geometry["minecraft:geometry"][0]["bones"][0]["neverRender"] = true.into();
    fs::write(model_file, geometry.to_string()).unwrap();
    assert!(
        load(&layout, &local)
            .skins
            .iter()
            .all(|entry| !entry.imported)
    );
}

/// Builds an original static pack with a built-in model and an explicit minimum engine version.
fn built_in_skin_pack(path: &Path, model: SkinModel, version: [u16; 3]) {
    use std::io::Write;
    let manifest = serde_json::json!({"modules":[{"type":"skin_pack"}],
        "header":{"min_engine_version":version}})
    .to_string();
    let catalog = serde_json::json!({"skins":[{"localization_name":"Built-in",
        "texture":"skin.png","geometry":model.geometry(),"type":"free"}]})
    .to_string();
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(
        protocol::CLASSIC_SKIN_SIDE as u32,
        protocol::CLASSIC_SKIN_SIDE as u32,
        image::Rgba([20, 160, 230, 255]),
    )
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in [
        ("manifest.json", manifest.as_bytes()),
        ("skins.json", catalog.as_bytes()),
        ("skin.png", png.get_ref().as_slice()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn built_in_pack_engine_version_survives_restart() {
    for model in [SkinModel::Classic, SkinModel::Slim] {
        let layout = layout();
        native_fixture(&layout);
        let local = LocalPlayerSkin::generated_default("fixture");
        let mut view = load(&layout, &local);
        let source = layout.user_data_root.join("built-in.mcpack");
        built_in_skin_pack(&source, model, [1, 21, 0]);
        import(&layout, &mut view, &source).unwrap();
        let selected = view.selected_skin().unwrap();
        assert_eq!(selected.model, model);
        assert_eq!(selected.engine_version.as_ref(), "1.21.0");
        fs::remove_file(source).unwrap();
        let restored = LocalPlayerSkin::load(&layout, "fixture");
        assert_eq!(restored.model(), model);
        assert_eq!(restored.engine_version.as_ref(), "1.21.0");
        assert_eq!(
            restored.to_client_skin().geometry.unwrap().engine_version,
            "1.21.0"
        );
    }
}

#[test]
fn built_in_pack_import_retains_distinct_engine_versions() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("built-in.mcpack");
    let mut ids = Vec::new();
    for (version, expected) in [([1, 21, 0], "1.21.0"), ([1, 22, 0], "1.22.0")] {
        built_in_skin_pack(&source, SkinModel::Classic, version);
        import(&layout, &mut view, &source).unwrap();
        let selected = view.selected_skin().unwrap();
        assert_eq!(selected.engine_version.as_ref(), expected);
        ids.push(selected.id.clone());
    }
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn legacy_png_preferences_restore_without_engine_metadata() {
    let layout = layout();
    native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("fixture");
    let mut view = load(&layout, &local);
    let source = layout.user_data_root.join("old.png");
    png(&source, [10, 20, 30, 255]);
    import(&layout, &mut view, &source).unwrap();
    let selected = view.selected_skin().unwrap().skin.clone();
    let path = layout.skin_selection_file();
    let mut preferences: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    preferences["imported"][0]
        .as_object_mut()
        .unwrap()
        .remove("engine_version");
    fs::write(path, preferences.to_string()).unwrap();
    let restored = LocalPlayerSkin::load(&layout, "fixture");
    assert_eq!(restored.standard_skin(), selected);
    assert_eq!(
        restored.engine_version.as_ref(),
        protocol::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION
    );
}
