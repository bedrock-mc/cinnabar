use {super::*, ui::IconRef};

fn catalog(color: [u8; 4]) -> Arc<assets::RuntimeEquipmentCatalog> {
    use assets::*;
    let reference = |identifier: &str| EquipmentReference {
        identifier: identifier.into(),
        resolution: EntityDependencyResolution::Catalog,
    };
    Arc::new(
        RuntimeEquipmentCatalog::from_parts(
            [1; 32],
            vec![EquipmentBinding {
                identifier: "fixture:chestplate".into(),
                category: EquipmentCategory::Armor {
                    slot: ArmorSlot::Chestplate,
                },
                geometry: reference("geometry.fixture"),
                texture: reference("textures/models/armor/fixture"),
                material: "armor".into(),
                render_controller: "controller.render.armor".into(),
                first_person: EquipmentTransform::NeedsMeasurement,
                third_person: EquipmentTransform::NeedsMeasurement,
                dropped: EquipmentTransform::NeedsMeasurement,
                poses: Box::new([]),
            }],
            vec![EquipmentTexture {
                identifier: "textures/models/armor/fixture".into(),
                width: 64,
                height: 32,
                rgba8: vec![color; 64 * 32]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .into(),
            }],
        )
        .unwrap(),
    )
}

/// Dresses the preview in the fixture chestplate, which places its pack texture.
fn wear_chestplate(presentation: &mut UiPresentationRuntime) {
    presentation
        .set_player_preview_gear([None, Some(("fixture:chestplate", None)), None, None], None);
}

#[test]
fn previews_use_pack_armor_reuse_unchanged_pages_and_restore_base_on_removal() {
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    let base = catalog([22, 33, 44, 255]);
    let pack = catalog([77, 88, 99, 255]);
    presentation.set_equipment_catalog(Some(base.clone()));
    presentation.gui_models.enabled = true;
    presentation.set_player_preview_skin(None, Default::default());
    presentation.set_preview_pack_equipment(Some(pack.clone()));
    let dress = |presentation: &mut UiPresentationRuntime| {
        presentation
            .set_player_preview_gear([None, Some(("fixture:chestplate", None)), None, None], None);
    };
    dress(&mut presentation);
    assert_eq!(
        &presentation.player_preview_gear.armor[1]
            .as_ref()
            .unwrap()
            .rgba[..4],
        &[77, 88, 99, 255]
    );
    let icon = presentation.gui_models.pack_equipment.region(1).unwrap();
    let mesh = presentation.gui_player_mesh().unwrap();
    assert!(
        mesh.batches()
            .iter()
            .any(|batch| batch.texture_page == icon.page)
    );
    let textures = Arc::clone(&presentation.textures);
    presentation.set_preview_pack_equipment(Some(pack));
    assert!(
        Arc::ptr_eq(&textures, &presentation.textures),
        "unchanged catalogs must not rebuild or upload"
    );
    presentation.set_preview_pack_equipment(None);
    dress(&mut presentation);
    assert_eq!(
        &presentation.player_preview_gear.armor[1]
            .as_ref()
            .unwrap()
            .rgba[..4],
        &[22, 33, 44, 255]
    );
    assert!(presentation.gui_models.pack_equipment.pages.is_empty());
    assert!(presentation.gui_models.pack_equipment.regions.is_empty());
}

#[test]
fn actor_flame_reclamation_keeps_session_armor_texels_addressable() {
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    let side = render_model::UI_MODEL_ATLAS_SIDE;
    let page =
        UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into()).unwrap();
    let first = (presentation.textures.dynamic_start() + MODEL_PAGE) as u16;
    presentation.gui_models.enabled = true;
    presentation.gui_models.pages = vec![page; MODEL_PAGES - 1];
    presentation.gui_models.required_pages = 1;
    presentation
        .gui_models
        .optional_models
        .insert((first + 1, [0, 0, 16, 16]));
    let color = [77, 88, 99, 255];
    presentation.set_preview_pack_equipment(Some(catalog(color)));
    wear_chestplate(&mut presentation);
    let previous = presentation
        .gui_models
        .pack_equipment
        .region(1)
        .unwrap()
        .page;
    let frame_side = side / 2;
    let fire = assets::ParticleTexture {
        path: assets::ACTOR_FLAME_TEXTURE.into(),
        width: frame_side,
        height: frame_side * 2,
        rgba8: (0..2)
            .flat_map(|frame| [frame, 20, 40, 255].repeat((frame_side * frame_side) as usize))
            .collect::<Vec<_>>()
            .into(),
    };
    presentation.set_gui_fire_texture(Some(&fire)).unwrap();
    let icon = presentation.gui_models.pack_equipment.region(1).unwrap();
    assert_ne!(icon.page, previous);
    let page = &presentation.textures.pages()[usize::from(icon.page)];
    let start = (usize::from(icon.uv[1]) * side as usize + usize::from(icon.uv[0])) * 4;
    assert_eq!(&page.pixels()[start..start + 4], &color);
    assert!(presentation.textures.plan().bytes() <= render_model::MAX_UI_TEXTURE_BYTES);
}

#[test]
fn review_session_armor_reclaims_optional_models_without_losing_flames() {
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    let side = render_model::UI_MODEL_ATLAS_SIDE;
    let page =
        UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into()).unwrap();
    let first = (presentation.textures.dynamic_start() + MODEL_PAGE) as u16;
    let optional = IconRef {
        page: first + 1,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    presentation.gui_models.enabled = true;
    presentation.gui_models.pages = vec![page; MODEL_PAGES - 1];
    presentation.gui_models.required_pages = 1;
    presentation
        .gui_models
        .optional_models
        .insert(icon_key(optional));
    presentation
        .gui_models
        .models
        .insert(icon_key(optional), item_gui::cube([optional; 6]).unwrap());
    let flame_side = assets::TILE_SIZE;
    let fire = assets::ParticleTexture {
        path: assets::ACTOR_FLAME_TEXTURE.into(),
        width: flame_side,
        height: flame_side * 2,
        rgba8: [11, 22, 33, 255]
            .repeat((flame_side * flame_side * 2) as usize)
            .into(),
    };
    presentation.set_gui_fire_texture(Some(&fire)).unwrap();
    assert_eq!(presentation.gui_models.pages.len(), MODEL_PAGES);
    let color = [77, 88, 99, 255];
    presentation.set_preview_pack_equipment(Some(catalog(color)));
    wear_chestplate(&mut presentation);
    assert!(
        presentation.gui_models.pack_equipment.region(1).is_some(),
        "optional icons must not disable session armor"
    );
    assert!(
        !presentation
            .gui_models
            .models
            .contains_key(&icon_key(optional))
    );
    assert_eq!(presentation.gui_models.pages.len(), 2);
    assert_eq!(presentation.gui_models.fire.frames.len(), 2);
    for (icon, expected) in [
        (presentation.gui_models.fire.frames[0], [11, 22, 33, 255]),
        (
            presentation.gui_models.pack_equipment.region(1).unwrap(),
            color,
        ),
    ] {
        let page = &presentation.textures.pages()[usize::from(icon.page)];
        let start = (usize::from(icon.uv[1]) * side as usize + usize::from(icon.uv[0])) * 4;
        assert_eq!(&page.pixels()[start..start + 4], &expected);
    }
    assert!(presentation.textures.plan().bytes() <= render_model::MAX_UI_TEXTURE_BYTES);
}
