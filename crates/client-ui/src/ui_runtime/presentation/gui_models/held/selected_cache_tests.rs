//! Atlas pressure must retain prepared geometry without requesting unsupported artwork.
use super::*;

#[test]
fn selection_consumers_unavailable_preview_artwork_reuses_prepared_geometry() {
    use crate::test_support::fixture_hud;
    use ui_engine::test_support::fixture_font;
    let item = "fixture:z_tool";
    let pack = fixture(item, "fixture:a_tool");
    let icons = Arc::new(
        RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                pack.assets.source_manifest_sha256(),
                &[assets::IconSprite {
                    width: 16,
                    height: 16,
                    rgba8: vec![255; 1024].into(),
                }],
                &[assets::IconEntry {
                    identifier: item.into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let mut ui =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), icons).unwrap();
    ui.set_gui_models(&assets::RuntimeAssets::diagnostic(), &pack.assets)
        .unwrap();
    ui.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    ui.set_preview_equipment_assets(None, Some(Arc::clone(&pack.assets)));
    ui.set_preview_pack_equipment(pack.equipment.clone());
    let world = owner(Arc::clone(&pack.assets));
    let context = PreviewEquipmentOwner {
        actor: world.authority().actor(1).unwrap(),
        rig: world.authority().actor_rig(1).unwrap(),
        input: Default::default(),
    };
    let side = render_model::UI_MODEL_ATLAS_SIDE;
    let page =
        render_model::UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into())
            .unwrap();
    ui.gui_models.pages = vec![page; super::super::super::MODEL_PAGES];
    ui.gui_models.required_pages = ui.gui_models.pages.len();
    ui.set_player_preview_gear_with_owner([None; 4], Some((item, 0)), context);
    assert!(ui.gui_models.pack_equipment.pages.is_empty());
    let fallback = ui.preview_held_models([true; 2])[0].clone().unwrap();
    assert!(
        fallback
            .placements
            .iter()
            .all(|placement| matches!(placement, PreviewHeldPlacement::Sprite { .. }))
    );
    assert!(ui.gui_player_mesh().is_some());
    let (_, unavailable) = test_alloc::measure(|| {
        ui.set_player_preview_gear_with_owner([None; 4], Some((item, 0)), context);
    });
    ui.gui_models.pages.clear();
    ui.gui_models.required_pages = 0;
    ui.place_pack_armor();
    ui.set_player_preview_gear_with_owner([None; 4], Some((item, 0)), context);
    assert_model(&ui, &pack, 0, "mapped");
    let (_, available) = test_alloc::measure(|| {
        ui.set_player_preview_gear_with_owner([None; 4], Some((item, 0)), context);
    });
    assert!(
        unavailable.bytes <= available.bytes,
        "missing artwork rebuilt geometry: {unavailable:?} vs drawable warm state {available:?}"
    );
    let catalog = pack.equipment.as_ref().unwrap();
    let mut bindings = catalog.bindings().to_vec();
    for binding in &mut bindings {
        binding.third_person = assets::EquipmentTransform::NeedsMeasurement;
    }
    ui.set_preview_pack_equipment(Some(Arc::new(
        RuntimeEquipmentCatalog::from_parts([1; 32], bindings, catalog.textures().to_vec())
            .unwrap(),
    )));
    ui.set_player_preview_gear_with_owner([None; 4], Some((item, 0)), context);
    assert!(
        ui.gui_models.pack_equipment.pages.is_empty(),
        "unsupported models cannot consume texture pages"
    );
    assert!(
        ui.preview_held_models([true; 2])[0]
            .as_ref()
            .unwrap()
            .placements
            .iter()
            .all(|placement| matches!(placement, PreviewHeldPlacement::Sprite { .. }))
    );
}
