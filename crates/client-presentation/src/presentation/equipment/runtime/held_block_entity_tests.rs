//! Block-entity camera materials and grips without installed carriers.

use super::*;
use render_model::{HandItemAlphaMode, MIN_DYNAMIC_ATLAS_WIDTH, java_animation::JavaHand};

#[test]
fn startup_conduit_model_reaches_camera_hands_and_player_equipment() {
    let (base, body, mut item) = block_fixture();
    item.identifier = Arc::from("minecraft:conduit");
    item.kind = HeldKind::Sprite;
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog(
            base.assets.source_manifest_sha256(),
            &[IconSprite {
                width: 16,
                height: 16,
                rgba8: [80, 160, 200, 255].repeat(16 * 16).into(),
            }],
            &[assets::IconEntry {
                identifier: item.identifier.as_ref().into(),
                metadata: 0,
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let block_entities = assets::RuntimeBlockEntityAssets::decode(
        &assets::encode_block_entity_catalog(
            b"{}",
            MIN_DYNAMIC_ATLAS_WIDTH,
            64,
            &vec![255; MIN_DYNAMIC_ATLAS_WIDTH as usize * 64 * 4],
            &[assets::BlockEntityPlacement {
                name: assets::block_entity_geometry::CONDUIT_TEXTURE.0.into(),
                x: 0,
                y: 0,
                width: assets::block_entity_geometry::CONDUIT_TEXTURE.1[0] as u32,
                height: assets::block_entity_geometry::CONDUIT_TEXTURE.1[1] as u32,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let (mut runtime, startup_pages, _) = EquipmentRuntime::build(
        Arc::clone(&base.assets),
        None,
        Arc::new(icons),
        None,
        Some(Arc::new(block_entities)),
        ActorArtworkPages::default(),
    );
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into()]);
    let main = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    let off = runtime.first_person_offhand(&body, &item).unwrap();
    let java = runtime
        .first_person_java_item(
            &body,
            &item,
            JavaHand {
                swing: 0.0,
                equip: 1.0,
                using: None,
            },
        )
        .unwrap();
    let third = third_person(&mut runtime, &body, &item);
    let rig = main.presentation.submission.input.rig;
    for camera in [&main, &off, &java] {
        assert_eq!(camera.presentation.submission.input.rig, rig);
        assert_eq!(camera.alpha_mode, HandItemAlphaMode::Cutout);
    }
    assert_eq!(third.submission.input.rig, rig);
    let models = runtime.take_pending_geometries();
    let model = models.iter().find(|model| model.id == rig).unwrap();
    assert_eq!(model.vertices.len(), 36);
    for axis in 0..3 {
        let (min, max) =
            model
                .vertices
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), vertex| {
                    (
                        min.min(vertex.position[axis]),
                        max.max(vertex.position[axis]),
                    )
                });
        let half = assets::block_entity_geometry::CONDUIT_SHELL.1[axis] / 32.0;
        assert_eq!([min, max], [-half, half]);
    }
    assert!(model.vertices.iter().all(|vertex| {
        vertex
            .uv
            .iter()
            .all(|component| (0.0..=1.0).contains(component))
    }));
    let startup_uvs: Vec<_> = model.vertices.iter().map(|vertex| vertex.uv).collect();
    let original_location = main.presentation.location;
    let replacement = assets::RuntimeBlockEntityAssets::decode(
        &assets::encode_block_entity_catalog(
            b"{}",
            MIN_DYNAMIC_ATLAS_WIDTH,
            128,
            &vec![21; MIN_DYNAMIC_ATLAS_WIDTH as usize * 128 * 4],
            &[assets::BlockEntityPlacement {
                name: assets::block_entity_geometry::CONDUIT_TEXTURE.0.into(),
                x: 0,
                y: 64,
                width: assets::block_entity_geometry::CONDUIT_TEXTURE.1[0] as u32,
                height: assets::block_entity_geometry::CONDUIT_TEXTURE.1[1] as u32,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let mut scene = render_model::BlockEntityScene::default();
    let prepared = scene.artwork_preparer().prepare(&replacement).unwrap();
    scene.install_prepared_assets(&prepared);
    let (updated_pages, held) =
        EquipmentRuntime::prepare_block_entity_artwork(&startup_pages, &prepared);
    runtime.set_block_entity_artwork(Some(&held));
    let queued = runtime.take_pending_geometries();
    let updated = queued.iter().find(|model| model.id == rig).unwrap();
    assert_ne!(
        updated
            .vertices
            .iter()
            .map(|vertex| vertex.uv)
            .collect::<Vec<_>>(),
        startup_uvs
    );
    for camera in [
        runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap(),
        runtime.first_person_offhand(&body, &item).unwrap(),
    ] {
        assert_eq!(camera.presentation.submission.input.rig, rig);
        let location = camera.presentation.location;
        assert_ne!(location, original_location);
        let page = &updated_pages.pages()[usize::from(location.page()) - 1];
        let (width, height) = page.dimensions();
        let bytes = usize::from(width) * usize::from(height) * 4;
        let start = location.layer() as usize * bytes;
        assert!(
            page.pixels()[start..start + bytes]
                .iter()
                .all(|byte| *byte == 21)
        );
    }
    assert_eq!(
        third_person(&mut runtime, &body, &item)
            .submission
            .input
            .rig,
        rig
    );
    runtime.set_block_entity_artwork(Some(&held));
    assert!(
        runtime.take_pending_geometries().is_empty(),
        "unchanged artwork reuses prepared meshes"
    );
    runtime.set_block_entity_artwork(None);
    let restored = runtime.take_pending_geometries();
    assert_eq!(
        restored
            .iter()
            .find(|model| model.id == rig)
            .unwrap()
            .vertices
            .iter()
            .map(|vertex| vertex.uv)
            .collect::<Vec<_>>(),
        startup_uvs
    );
    assert_eq!(
        runtime
            .first_person_offhand(&body, &item)
            .unwrap()
            .presentation
            .location,
        original_location
    );
}

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
    let [width, height] = [MIN_DYNAMIC_ATLAS_WIDTH, 512];
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
    let heads = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .into_vec()
    .into_iter()
    .filter(|block| SkullKind::from_block_identifier(&block.name).is_some())
    .map(|block| block.name.to_string())
    .collect::<std::collections::BTreeSet<_>>();
    for (index, identifier) in [
        "minecraft:chest",
        "minecraft:red_shulker_box",
        "minecraft:decorated_pot",
        "minecraft:conduit",
    ]
    .into_iter()
    .map(String::from)
    .chain(heads)
    .enumerate()
    {
        item.identifier = Arc::from(identifier.as_str());
        let kind = HeldBlockEntity::from_item(&identifier).unwrap();
        let rig = equipment_rig_id(index as u32);
        runtime
            .held_block_entities
            .models
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
    let rig = equipment_rig_id(0);
    runtime
        .held_block_entities
        .models
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
