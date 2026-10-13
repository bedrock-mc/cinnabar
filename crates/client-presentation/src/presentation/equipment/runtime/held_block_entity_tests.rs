//! Block-entity camera materials and grips without installed carriers.

use super::*;
use render_model::{HandItemAlphaMode, java_animation::JavaHand};

#[test]
fn held_model_setup_preserves_texture_local_glint_coordinates() {
    let (base, body, mut item) = block_fixture();
    let placement = assets::BlockEntityPlacement {
        name: "textures/entity/chest/normal".into(),
        x: 32,
        y: 128,
        width: 64,
        height: 64,
    };
    let [width, height] = [256, 512];
    let encoded = assets::encode_block_entity_catalog(
        b"{}",
        width,
        height,
        &vec![255; (width * height * 4) as usize],
        std::slice::from_ref(&placement),
    )
    .unwrap();
    let (mut runtime, _, _) = EquipmentRuntime::build(
        Arc::clone(&base.assets),
        None,
        Arc::clone(&base.icons),
        None,
        Some(Arc::new(
            assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap(),
        )),
        ActorArtworkPages::default(),
    );
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into()]);
    let models = runtime.take_pending_geometries();
    item.identifier = Arc::from("minecraft:chest");
    item.kind = HeldKind::Sprite;
    item.enchanted = true;
    let first = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    let model = models
        .iter()
        .find(|model| model.id == first.presentation.submission.input.rig)
        .unwrap();
    let span = [
        placement.width as f32 / width as f32,
        placement.height as f32 / height as f32,
    ];
    assert_eq!(
        first.presentation.submission.material.glint,
        render_model::ActorGlint::item(0.0, span)
    );
    let origin = [
        placement.x as f32 / width as f32,
        placement.y as f32 / height as f32,
    ];
    assert!(model.vertices.iter().all(|vertex| {
        bevy::math::Vec2::from_array(vertex.glint_origin)
            .abs_diff_eq(bevy::math::Vec2::from_array(origin), 1.0e-5)
    }));
    assert!(runtime.take_pending_geometries().is_empty());
}

#[test]
fn held_block_entity_models_keep_cutout_alpha_and_block_grips() {
    let (mut runtime, body, mut item) = block_fixture();
    let animation = ItemAnimationState::default();
    let hand = JavaHand {
        swing: 0.0,
        equip: 1.0,
        using: None,
    };
    let block_main = runtime.first_person_item(&body, &item, animation).unwrap();
    let block_off = runtime.first_person_offhand(&body, &item).unwrap();
    let block_java = runtime.first_person_java_item(&body, &item, hand).unwrap();
    let location = block_main.presentation.location;
    item.enchanted = true;
    let span = [0.125, 0.0625];
    let expected_glint = render_model::ActorGlint::item(0.0, span);
    for (index, identifier) in [
        "minecraft:chest",
        "minecraft:red_shulker_box",
        "minecraft:decorated_pot",
        "minecraft:conduit",
    ]
    .into_iter()
    .enumerate()
    {
        item.identifier = Arc::from(identifier);
        let kind = HeldBlockEntity::from_item(identifier).unwrap();
        let rig = equipment_rig_id(HELD_BLOCK_ENTITY_RIG_INDEX_BASE + index as u32);
        runtime
            .held_block_entities
            .insert(kind, (rig, location, span));
        for held_kind in [HeldKind::Block(7), HeldKind::Sprite, HeldKind::Other] {
            item.kind = held_kind;
            let main = runtime.first_person_item(&body, &item, animation).unwrap();
            let off = runtime.first_person_offhand(&body, &item).unwrap();
            let java = runtime.first_person_java_item(&body, &item, hand).unwrap();
            for camera in [&main, &off, &java] {
                assert_eq!(camera.alpha_mode, HandItemAlphaMode::Cutout, "{identifier}");
                assert_eq!(camera.presentation.submission.input.rig, rig);
                assert_eq!(camera.presentation.location, location);
                assert_eq!(
                    camera.presentation.submission.material.glint,
                    expected_glint
                );
            }
            let third = third_person(&mut runtime, &body, &item);
            assert_eq!(third.submission.material.glint, expected_glint);
            assert_eq!(
                main.presentation.submission.input.current_bones,
                block_main.presentation.submission.input.current_bones
            );
            assert_eq!(
                off.presentation.submission.input.current_bones,
                block_off.presentation.submission.input.current_bones
            );
            assert_eq!(java.java_camera, block_java.java_camera);
        }
    }
}

#[test]
fn held_block_entity_alpha_uses_only_admitted_models_after_session_materials() {
    use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};

    let (mut runtime, body, mut item) = block_fixture();
    item.identifier = Arc::from("minecraft:decorated_pot");
    let animation = ItemAnimationState::default();
    let fallback = runtime.first_person_item(&body, &item, animation).unwrap();
    assert_eq!(fallback.alpha_mode, HandItemAlphaMode::Opaque);
    let kind = HeldBlockEntity::from_item(&item.identifier).unwrap();
    let rig = equipment_rig_id(HELD_BLOCK_ENTITY_RIG_INDEX_BASE);
    runtime
        .held_block_entities
        .insert(kind, (rig, fallback.presentation.location, [1.0; 2]));
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
    for (flags, expected) in [
        (0, HandItemAlphaMode::Opaque),
        (assets::MATERIAL_FLAG_ALPHA_BLEND, HandItemAlphaMode::Blend),
    ] {
        let items = crate::session_assets::SessionItems {
            components: Arc::new(Default::default()),
            icons: Some(Arc::new(SessionIcons {
                block_sheets: vec![SessionIcon {
                    identifier: Arc::clone(&item.identifier),
                    metadata: 0,
                    width,
                    height,
                    rgba8: vec![127; (width * height * 4) as usize].into(),
                }],
                block_material_flags: BTreeMap::from([(Arc::clone(&item.identifier), flags)]),
                ..Default::default()
            })),
        };
        let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
        let (_, locations) = ActorArtworkPages::default().with_equipment_rasters(staged.rasters());
        runtime.set_session_items(Some(&items), Some(staged), locations);
        let main = runtime.first_person_item(&body, &item, animation).unwrap();
        let off = runtime.first_person_offhand(&body, &item).unwrap();
        assert_eq!(main.alpha_mode, expected);
        assert_eq!(off.alpha_mode, expected);
        assert_ne!(main.presentation.submission.input.rig, rig);

        let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
        runtime.set_session_items(Some(&items), Some(staged), Vec::new());
        let main = runtime.first_person_item(&body, &item, animation).unwrap();
        let off = runtime.first_person_offhand(&body, &item).unwrap();
        assert_eq!(main.alpha_mode, HandItemAlphaMode::Cutout);
        assert_eq!(off.alpha_mode, HandItemAlphaMode::Cutout);
        assert_eq!(main.presentation.submission.input.rig, rig);
        assert_eq!(off.presentation.submission.input.rig, rig);
    }
}
