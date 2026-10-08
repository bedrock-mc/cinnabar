use super::*;
use crate::presentation::equipment::HeldKind;
use bevy::math::Mat4;

const HELD_ITEMS: [&str; 9] = [
    "minecraft:diamond_sword",
    "minecraft:potion",
    "minecraft:golden_apple",
    "minecraft:stone",
    "minecraft:shield",
    "minecraft:crossbow",
    "minecraft:fishing_rod",
    FILLED_MAP,
    BOW,
];

/// Supplies original item pixels, including a carried cube, without installed assets.
fn equipment_fixture() -> (EquipmentRuntime, render::ActorArtworkPages) {
    let mut files = vec![
        ("models/entity/test.geo.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":16,"texture_height":16},"bones":[{"name":"item","pivot":[0,0,0],"texture_meshes":[{"texture":"default","position":[0,0,0],"rotation":[0,0,0]}]}]}]}"#.to_vec()),
        ("render_controllers/test.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"arrays":{"textures":{"Array.frames":["Texture.default","Texture.pull1","Texture.pull2","Texture.pull3"]}},"textures":["Array.frames[query.get_animation_frame]"]}}}"#.to_vec()),
    ];
    for frame in 0..4 {
        files.push((
            format!("textures/test{frame}.png").into(),
            fixture_png(frame),
        ));
    }
    for item in ["shield", "crossbow", "bow"] {
        let attachable = serde_json::json!({
            "format_version": "1.10.0", "minecraft:attachable": {"description": {
                "identifier": format!("minecraft:{item}"),
                "materials": {"default": "entity_alphatest"},
                "textures": {"default": "textures/test0", "pull1": "textures/test1", "pull2": "textures/test2", "pull3": "textures/test3"},
                "geometry": {"default": "geometry.test"},
                "render_controllers": ["controller.render.test"],
            }},
        });
        files.push((
            format!("attachables/{item}.json").into(),
            attachable.to_string().into_bytes(),
        ));
    }
    let mut compiled = pack_compiler::compile_actor_pack(files).unwrap().unwrap();
    compiled.entities.block_visual_count = 1;
    let catalog = Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            compiled.identity,
            compiled.equipment_bindings,
            compiled.equipment_textures,
        )
        .unwrap(),
    );
    let entities = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
    let sprite = assets::IconSprite {
        width: 16,
        height: 16,
        rgba8: vec![255; 16 * 16 * 4].into(),
    };
    let sheet = assets::compose_block_item_sheet(&std::array::from_fn(|_| sprite.clone())).unwrap();
    let mut entries = HELD_ITEMS
        .into_iter()
        .map(|identifier| assets::IconEntry {
            identifier: identifier.into(),
            metadata: 0,
            sprite: 0,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog_with_block_sheets(
                entities.source_manifest_sha256(),
                &[sprite, sheet],
                &entries,
                &[assets::IconBlockSheet {
                    visual: assets::BlockVisualId(0),
                    sprite: 1,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let (equipment, pages, _) = EquipmentRuntime::build(
        entities,
        Some(catalog),
        icons,
        None,
        None,
        render::ActorArtworkPages::default(),
    );
    (equipment, pages)
}

/// Encodes a small original checker texture for the attachable fixture.
fn fixture_png(frame: u8) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(16, 16, |x, y| {
        image::Rgba([
            if (x + y) % 2 == 0 { 255 } else { 100 },
            80,
            20 + frame * 50,
            if (x + y) % 3 == 0 { 0 } else { 255 },
        ])
    });
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    encoded.into_inner()
}

/// Uses the actual publication route with a settled equipped stack and uploaded player skin.
fn source_for(identifier: Option<&str>, with_artwork: bool) -> HandSource {
    source_for_use(identifier, with_artwork, 0).0
}

/// Samples the real use clock and retains each generated mesh for texture-coverage assertions.
fn source_for_use(
    identifier: Option<&str>,
    with_artwork: bool,
    use_ticks: u32,
) -> (HandSource, Vec<render_model::ActorRigGeometry>) {
    let mut stream = super::tests::head_stream();
    let mut feed = super::tests::uploaded_skin_feed(false);
    let protocol::PlayerSkin::Standard(skin) = &mut feed.skin else {
        unreachable!()
    };
    let geometry = Arc::make_mut(skin.geometry.as_mut().unwrap());
    let mut json: serde_json::Value = serde_json::from_str(&geometry.geometry_data).unwrap();
    let bones = json["minecraft:geometry"][0]["bones"]
        .as_array_mut()
        .unwrap();
    for (hand, arm, x) in [("rightItem", "rightarm", 4), ("leftItem", "leftarm", -4)] {
        bones.push(serde_json::json!({"name": hand, "parent": arm, "pivot": [x, 12, 0]}));
    }
    geometry.geometry_data = json.to_string().into();
    feed.first_person = true;
    feed.main_hand = identifier.map(Into::into);
    stream.sync_local_player_pose(&feed);
    stream.prepare_actor_appearances_for_test();
    stream.advance_actor_interpolation_frame(6);
    if use_ticks > 0 {
        feed.item_use = client_world::LocalItemUse::Using;
        stream.sync_local_player_pose(&feed);
        stream.prepare_actor_appearances_for_test();
        stream.advance_actor_interpolation_frame(use_ticks);
    }
    let (mut equipment, artwork) = equipment_fixture();
    let artwork = if with_artwork {
        artwork
    } else {
        render::ActorArtworkPages::default()
    };
    let rig = stream.authority().actor_rig(1).unwrap();
    equipment.register_skin_rig(
        render_model::EntityRigId(rig.rig.0),
        rig.bone_names.to_vec(),
    );
    let input = ActorEquipmentInput {
        main: identifier.map(|identifier| WornItem {
            identifier: identifier.into(),
            metadata: 0,
            damage: None,
            dye_rgb: None,
            enchanted: false,
            kind: if identifier == "minecraft:stone" {
                HeldKind::Block(0)
            } else {
                HeldKind::Sprite
            },
        }),
        ..Default::default()
    };
    let actor = stream.authority().actor(1).unwrap();
    let presentation =
        crate::presentation::actors::entity_rig_presentation(&rig, actor, &artwork, 0.5).unwrap();
    let mut cache = HandCache::default();
    cache.remember(&rig, input.main.as_ref());
    let source = hand_source(
        HandInputs {
            stream: &stream,
            presentation,
            equipment_input: &input,
            owner_equipment: &input,
            consume_ticks: None,
            item_animation: Some(client_world::AttachableAnimationInput {
                first_person: true,
                ..Default::default()
            }),
            alpha: 0.5,
            artwork: &artwork,
            motion: Mat4::IDENTITY,
            sampling_camera: None,
        },
        &mut equipment,
        &mut cache,
    )
    .unwrap();
    (source, equipment.take_pending_geometries())
}

#[test]
fn held_java_items_publish_only_the_resolved_item_layer() {
    for identifier in HELD_ITEMS.into_iter().filter(|item| *item != FILLED_MAP) {
        let source = source_for(Some(identifier), true);
        assert!(
            source.body.is_none(),
            "{identifier} must not publish a player arm"
        );
        assert!(source.java_body_camera.is_none());
        assert_eq!(source.items.iter().flatten().count(), 1, "{identifier}");
        if matches!(identifier, "minecraft:shield" | "minecraft:crossbow") {
            assert!(
                !source.items[0].as_ref().unwrap().0.camera_space,
                "{identifier} must exercise its attachable"
            );
        }
        assert_resolved_textures(&source);
    }
}

#[test]
fn every_java_bow_pull_frame_publishes_one_textured_raster_without_an_arm() {
    for (use_ticks, frame) in [(0, 0), (2, 1), (15, 2), (19, 3)] {
        let (source, meshes) = source_for_use(Some(BOW), true, use_ticks);
        assert!(source.body.is_none());
        assert_eq!(source.items.iter().flatten().count(), 1);
        assert_resolved_textures(&source);
        let (item, atlas) = source.items[0].as_ref().unwrap();
        let mesh = meshes
            .iter()
            .find(|mesh| mesh.id == item.presentation.submission.input.rig)
            .unwrap();
        assert!(item.java_camera.is_some());
        assert_eq!([atlas.width, atlas.height], [16, 16]);
        for vertex in mesh.vertices.iter() {
            let texel = vertex.uv.map(|coordinate| coordinate * 16.0);
            assert!(
                texel
                    .into_iter()
                    .all(|coordinate| (coordinate.fract() - 0.5).abs() < 1e-6),
                "bow must use the raster attachable's texel-centered mesh"
            );
            let layer = item.presentation.location.layer() as usize;
            let at = ((layer * 16 + texel[1] as usize) * 16 + texel[0] as usize) * 4;
            assert_eq!(atlas.rgba8[at + 2], 20 + frame * 50);
            assert_eq!(
                atlas.rgba8[at + 3],
                255,
                "no bow face samples an unresolved or transparent texel"
            );
        }
    }
}

#[test]
fn missing_held_item_artwork_does_not_publish_an_empty_arm() {
    for identifier in HELD_ITEMS.into_iter().filter(|item| *item != FILLED_MAP) {
        let source = source_for(Some(identifier), false);
        assert!(source.items.iter().all(Option::is_none));
        assert!(
            source.body.is_none(),
            "missing {identifier} artwork must not substitute a player arm"
        );
        assert!(source.java_body_camera.is_none());
    }
}

#[test]
fn first_person_texture_lookup_rejects_a_layer_absent_from_its_page() {
    let mut source = source_for(Some(BOW), true);
    let (mut item, _) = source.items[0].take().unwrap();
    let raster = render::EquipmentRaster {
        width: 16,
        height: 16,
        rgba8: vec![255; 16 * 16 * 4].into(),
    };
    let (_, locations) = render::ActorArtworkPages::default()
        .with_equipment_rasters(&[raster.clone(), raster.clone()]);
    let (artwork, _) = render::ActorArtworkPages::default().with_equipment_rasters(&[raster]);
    item.presentation.location = locations[1].unwrap();
    assert!(item_atlas(&item, &artwork).is_none());
}

#[test]
fn empty_hand_and_maps_keep_their_intentional_textured_arms() {
    let empty = source_for(None, true);
    assert!(empty.body.is_some());
    assert!(empty.java_body_camera.is_some());
    assert!(empty.items.iter().all(Option::is_none));
    assert_resolved_textures(&empty);
    let map = source_for(Some(FILLED_MAP), true);
    assert!(
        map.body.is_some(),
        "Java's map is the held-item arm exception"
    );
    assert!(map.items[0].is_some());
    assert!(map.items[1].is_none());
    assert_resolved_textures(&map);
}

/// Every item samples a present atlas layer; every visible arm has standard skin pixels.
fn assert_resolved_textures(source: &HandSource) {
    for (item, atlas) in source.items.iter().flatten() {
        assert!(item.presentation.location.page() > 0);
        assert!(item.presentation.location.layer() < atlas.layers);
        assert!(atlas.width > 0 && atlas.height > 0);
        assert_eq!(
            atlas.rgba8.len(),
            usize::from(atlas.width) * usize::from(atlas.height) * atlas.layers as usize * 4
        );
    }
    if source.body.is_some() {
        assert_eq!(
            source.presentation.skin_rgba8.as_ref().unwrap().len(),
            render_model::STANDARD_SKIN_BYTES
        );
    }
}
