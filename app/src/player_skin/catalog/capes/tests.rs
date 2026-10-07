use super::*;

fn layout() -> InstallLayout {
    super::super::tests::layout()
}

fn png(path: &Path, size: (u32, u32), color: [u8; 4]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(size.0, size.1, image::Rgba(color))
        .save(path)
        .unwrap();
}

#[test]
fn imported_cape_retains_texels_and_persists_independently_of_skin_choice() {
    let layout = layout();
    super::super::tests::native_fixture(&layout);
    let local = LocalPlayerSkin::generated_default("cape fixture");
    let mut view = super::super::load(&layout, &local);
    let source = layout.user_data_root.join("cape.png");
    let size = protocol::CAPE_DIMENSIONS[1];
    png(&source, size, [12, 41, 203, 56]);
    import_cape(&layout, &mut view, &source).unwrap();
    let selected = view.selected_cape().unwrap().clone();
    assert_eq!((selected.cape.width, selected.cape.height), size);
    assert!(
        selected
            .cape
            .rgba8
            .chunks_exact(4)
            .all(|pixel| pixel == [12, 41, 203, 56])
    );
    fs::remove_file(source).unwrap();
    super::super::select(&layout, &mut view, 1).unwrap();
    let reloaded = super::super::load(&layout, &local);
    assert_eq!(reloaded.selected_cape(), Some(&selected));
    assert_eq!(reloaded.selected_skin().unwrap().model, SkinModel::Slim);
    assert_eq!(
        reloaded.active_skin().unwrap().cape,
        Some(selected.cape.clone())
    );
    let login = LocalPlayerSkin::load(&layout, "cape fixture").to_client_skin();
    let cape = login.cape.unwrap();
    assert_eq!((cape.width, cape.height), size);
    assert_eq!(cape.rgba8, selected.cape.rgba8.as_ref());
    select_cape(&layout, &mut view, None).unwrap();
    assert!(
        super::super::load(&layout, &local)
            .active_skin()
            .unwrap()
            .cape
            .is_none()
    );
    assert!(
        LocalPlayerSkin::load(&layout, "cape fixture")
            .to_client_skin()
            .cape
            .is_none()
    );
}

#[test]
fn cape_rename_delete_remap_selection_and_preserve_originals() {
    let layout = layout();
    let local = LocalPlayerSkin::generated_default("cape edits");
    let mut view = super::super::load(&layout, &local);
    let first = layout.user_data_root.join("first-cape.png");
    let second = layout.user_data_root.join("second-cape.png");
    png(&first, protocol::CAPE_DIMENSIONS[0], [1, 2, 3, 255]);
    png(&second, protocol::CAPE_DIMENSIONS[0], [3, 2, 1, 255]);
    import_cape(&layout, &mut view, &first).unwrap();
    let private_first = view.selected_cape().unwrap().path.clone();
    import_cape(&layout, &mut view, &second).unwrap();
    let last = view.selected_cape().unwrap().clone();
    rename_cape(&layout, &mut view, 1, "  My cape  ").unwrap();
    assert_eq!(view.selected_cape().unwrap().name, "My cape");
    assert_eq!(view.selected_cape().unwrap().cape, last.cape);
    delete_cape(&layout, &mut view, 0).unwrap();
    assert_eq!(view.selected_cape, Some(0));
    assert!(!Path::new(&private_first).exists());
    assert!(first.exists() && second.exists());
    assert_eq!(
        super::super::load(&layout, &local).selected_cape(),
        view.selected_cape()
    );
    delete_cape(&layout, &mut view, 0).unwrap();
    assert!(view.selected_cape().is_none());
    assert!(super::super::load(&layout, &local).capes.is_empty());
}

#[test]
fn invalid_cape_and_failed_delete_leave_saved_library_unchanged() {
    let layout = layout();
    let local = LocalPlayerSkin::generated_default("cape validation");
    let mut view = super::super::load(&layout, &local);
    let source = layout.user_data_root.join("cape.png");
    png(&source, protocol::CAPE_DIMENSIONS[0], [0, 1, 2, 255]);
    import_cape(&layout, &mut view, &source).unwrap();
    let old = view.clone();
    let bad = layout.user_data_root.join("skin-is-not-cape.png");
    png(
        &bad,
        (
            protocol::CLASSIC_SKIN_SIDE as u32,
            protocol::CLASSIC_SKIN_SIDE as u32,
        ),
        [255; 4],
    );
    assert!(import_cape(&layout, &mut view, &bad).is_err());
    assert_eq!(view, old);
    let invalid_index = view.capes.len();
    assert!(select_cape(&layout, &mut view, Some(invalid_index)).is_err());
    let selected = old.selected_cape().unwrap();
    fs::remove_file(layout.skin_selection_file()).unwrap();
    fs::create_dir_all(layout.skin_selection_file()).unwrap();
    assert!(delete_cape(&layout, &mut view, 0).is_err());
    assert_eq!(view, old);
    assert!(Path::new(&selected.path).is_file());
    Arc::make_mut(&mut view.capes)[0].imported = false;
    assert!(rename_cape(&layout, &mut view, 0, "Immutable").is_err());
    assert!(delete_cape(&layout, &mut view, 0).is_err());
}

#[test]
fn cape_import_capacity_allows_duplicates_but_never_unrestorable_additions() {
    let layout = layout();
    let local = LocalPlayerSkin::generated_default("cape capacity");
    let mut view = super::super::load(&layout, &local);
    let source = layout.user_data_root.join("cape.png");
    png(&source, protocol::CAPE_DIMENSIONS[0], [10, 20, 30, 255]);
    import_cape(&layout, &mut view, &source).unwrap();
    let entry = view.selected_cape().unwrap().clone();
    let mut capes = vec![entry.clone(); MAX_IMPORTED_ITEMS];
    for (index, cape) in capes.iter_mut().enumerate().skip(1) {
        cape.id = format!("fixture:{index}");
    }
    view.capes = capes.into();
    import_cape(&layout, &mut view, &source).unwrap();
    let next = layout.user_data_root.join("next.png");
    png(&next, protocol::CAPE_DIMENSIONS[0], [30, 20, 10, 255]);
    assert!(import_cape(&layout, &mut view, &next).is_err());
    assert_eq!(view.capes.len(), MAX_IMPORTED_ITEMS);
    assert_eq!(
        super::super::load(&layout, &local).selected_cape(),
        Some(&entry)
    );
}
