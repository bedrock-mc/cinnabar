use super::*;

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
    let icon = *presentation
        .gui_models
        .pack_equipment
        .textures
        .values()
        .next()
        .unwrap();
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
    assert!(presentation.gui_models.pack_equipment.textures.is_empty());
}
