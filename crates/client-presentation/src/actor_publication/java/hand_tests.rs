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
fn equipment_fixture() -> (EquipmentRuntime, render_model::ActorArtworkPages) {
    equipment_fixture_with_native_bow(false)
}

/// Optionally installs a texture-only bow overlay on the original equipment fixture.
fn equipment_fixture_with_native_bow(
    native_bow: bool,
) -> (EquipmentRuntime, render_model::ActorArtworkPages) {
    equipment_fixture_with_bow_override(native_bow, None)
}

/// Installs an optional owner-conditional bow with distinct artwork over the startup fixture.
fn equipment_fixture_with_bow_override(
    native_bow: bool,
    condition: Option<&str>,
) -> (EquipmentRuntime, render_model::ActorArtworkPages) {
    let mut files: Vec<(Box<str>, Vec<u8>)> = vec![
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
                "textures": {"default": "textures/test0", "pull1": "textures/test1", "pull2": "textures/test2", "pull3": "textures/test3", "bow_pulling_0": "textures/test1", "bow_pulling_1": "textures/test2", "bow_pulling_2": "textures/test3"},
                "geometry": {"default": "geometry.test"},
                "render_controllers": ["controller.render.test"],
            }},
        });
        files.push((
            format!("attachables/{item}.json").into(),
            attachable.to_string().into_bytes(),
        ));
    }
    if native_bow {
        for (path, bytes) in &mut files {
            if path.ends_with(".json") {
                *bytes = String::from_utf8(std::mem::take(bytes))
                    .unwrap()
                    .replace("geometry.test", "geometry.bow_standby")
                    .into_bytes();
            }
        }
    }
    let mut compiled = pack_entity_compiler::compile_actor_pack(files.clone())
        .unwrap()
        .unwrap();
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
    let (mut equipment, mut pages, _) = EquipmentRuntime::build(
        Arc::clone(&entities),
        Some(Arc::clone(&catalog)),
        icons,
        None,
        None,
        render_model::ActorArtworkPages::default(),
    );
    if let Some(condition) = condition {
        files.retain(|(path, _)| !path.starts_with("attachables/") || path.ends_with("bow.json"));
        for (path, bytes) in &mut files {
            if path.ends_with(".json") {
                *bytes = String::from_utf8(std::mem::take(bytes))
                    .unwrap()
                    .replace("textures/test", "textures/override")
                    .into_bytes();
                if path.ends_with("bow.json") {
                    let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                    json["minecraft:attachable"]["description"]["item"] =
                        serde_json::json!({BOW: condition});
                    *bytes = serde_json::to_vec(&json).unwrap();
                }
            } else if path.starts_with("textures/") {
                let frame = path.strip_prefix("textures/test").unwrap().as_bytes()[0] - b'0';
                *bytes = fixture_png(frame + 1);
                *path = path.replace("textures/test", "textures/override").into();
            }
        }
        let compiled = pack_entity_compiler::compile_actor_pack(files)
            .unwrap()
            .unwrap();
        let catalog = Arc::new(
            assets::RuntimeEquipmentCatalog::from_parts(
                compiled.identity,
                compiled.equipment_bindings,
                compiled.equipment_textures,
            )
            .unwrap(),
        );
        let entities =
            Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
        let (next, locations) =
            pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        pages = next;
        equipment.set_pack_layer(Some((entities, Some(catalog), locations)));
        assert!(!equipment.uses_native_item_mesh(BOW));
    } else if native_bow {
        let (next, locations) =
            pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        pages = next;
        equipment.set_pack_layer(Some((entities, Some(catalog), locations)));
        assert!(equipment.uses_native_item_mesh(BOW));
    }
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
    source_for_use_and_swing(identifier, with_artwork, use_ticks, None, None)
}

/// Samples independent swing clocks and optional local use timing through Java publication.
fn source_for_use_and_swing(
    identifier: Option<&str>,
    with_artwork: bool,
    use_ticks: u32,
    swing: Option<actor_animation::LocalSwingProgress>,
    timing: Option<actor_animation::AttachableAnimationInput<'static>>,
) -> (HandSource, Vec<render_model::ActorRigGeometry>) {
    source_with_equipment(
        identifier,
        with_artwork,
        use_ticks,
        swing,
        timing,
        equipment_fixture(),
    )
}

/// Publishes the settled stack using the supplied startup and session equipment catalogs.
fn source_with_equipment(
    identifier: Option<&str>,
    with_artwork: bool,
    use_ticks: u32,
    swing: Option<actor_animation::LocalSwingProgress>,
    timing: Option<actor_animation::AttachableAnimationInput<'static>>,
    equipment: (EquipmentRuntime, render_model::ActorArtworkPages),
) -> (HandSource, Vec<render_model::ActorRigGeometry>) {
    source_with_equipment_and_map(
        identifier,
        with_artwork,
        use_ticks,
        swing,
        timing,
        equipment,
        None,
    )
}

/// Supplies optional map identity and received pixels while retaining missing-image controls.
fn source_with_equipment_and_map(
    identifier: Option<&str>,
    with_artwork: bool,
    use_ticks: u32,
    swing: Option<actor_animation::LocalSwingProgress>,
    timing: Option<actor_animation::AttachableAnimationInput<'static>>,
    (mut equipment, mut artwork): (EquipmentRuntime, render_model::ActorArtworkPages),
    map_data: Option<(i64, Option<&[u32]>)>,
) -> (HandSource, Vec<render_model::ActorRigGeometry>) {
    let mut stream = super::tests::head_stream();
    if identifier == Some(FILLED_MAP) {
        let assets = super::tests::head_assets();
        let catalog = super::tests::map_paper_catalog(assets.source_manifest_sha256());
        let (next, locations) =
            artwork.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        artwork = next;
        equipment.set_pack_layer(Some((assets, Some(catalog), locations)));
    }
    if let Some((map_id, Some(pixels))) = map_data {
        stream
            .submit(
                stream.committed_sequence() + 1,
                protocol::WorldEvent::MapData(protocol::MapDataEvent {
                    map_id,
                    start_x: 0,
                    start_y: 0,
                    width: pixels.len() as u32,
                    height: 1,
                    pixels: pixels.into(),
                }),
            )
            .unwrap();
    }
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
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(6);
    if use_ticks > 0 {
        feed.item_use = client_world::LocalItemUse::Using;
        stream.sync_local_player_pose(&feed);
        stream.prepare_actor_appearance_fixture();
        stream.advance_actor_interpolation_frame(use_ticks);
    }
    if let Some(swing) = swing {
        stream.sync_local_swing(swing);
        stream.advance_actor_interpolation_frame(0);
    }
    let artwork = if with_artwork {
        artwork
    } else {
        render_model::ActorArtworkPages::default()
    };
    let rig = stream.authority().actor_rig(1).unwrap();
    equipment.register_skin_rig(
        render_model::EntityRigId(rig.rig.0),
        rig.bone_names.to_vec(),
    );
    let input = ActorEquipmentInput {
        main: identifier.map(|identifier| WornItem {
            variant: Default::default(),
            map_id: map_data.map(|(id, _)| id),
            charged_projectile: None,
            use_facts: client_world::EffectiveItemUse::default(),
            grip: assets::item::grip::builtin_item_grip(identifier),
            identifier: identifier.into(),
            metadata: 0,
            damage: None,
            dye_rgb: None,
            enchanted: false,
            glint: Default::default(),
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
            item_animation: Some(actor_animation::AttachableAnimationInput {
                first_person: true,
                ..timing.unwrap_or_default()
            }),
            alpha: 0.5,
            artwork: &artwork,
            motion: Mat4::IDENTITY,
            sampling_camera: None,
            aspect: 1.0,
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
        assert!(source.body_camera.is_none());
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

/// Optionally exports the original bow fixture through the shared offline hand rasterizer.
fn write_bow_frame(name: &str, source: &HandSource, meshes: &[render_model::ActorRigGeometry]) {
    let Some(directory) = std::env::var_os("INSTA_OUTPUT_DIRECTORY") else {
        return;
    };
    use crate::presentation::equipment::frame_raster::{
        HEIGHT, Image, Target, WIDTH, bone_mat, draw_mesh, rows_to_mat,
    };
    let submissions = source.submissions();
    let draw = submissions[1].as_ref().unwrap();
    let mesh = meshes
        .iter()
        .find(|mesh| mesh.id == draw.input.rig)
        .unwrap();
    assert!(mesh.bone_pivots.iter().all(|pivot| *pivot == [0.0; 3]));
    let (item, atlas) = source.items[0].as_ref().unwrap();
    let layer_bytes = usize::from(atlas.width) * usize::from(atlas.height) * 4;
    let start = item.presentation.location.layer() as usize * layer_bytes;
    let texture = Image {
        width: usize::from(atlas.width),
        height: usize::from(atlas.height),
        rgba8: atlas.rgba8[start..start + layer_bytes].to_vec(),
    };
    let mut target = Target {
        color: [25, 32, 44, 255].repeat(WIDTH * HEIGHT),
        depth: vec![f32::INFINITY; WIDTH * HEIGHT],
    };
    let placement = rows_to_mat(draw.world_from_actor);
    draw_mesh(
        &mut target,
        &mesh.vertices,
        |vertex| {
            let bone = bone_mat(draw.input.current_bones[vertex.bone_index as usize]);
            placement.transform_point3(bone.transform_point3(Vec3::from(vertex.position)))
        },
        &texture,
    );
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        &target.color,
        WIDTH as u32,
        HEIGHT as u32,
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[test]
fn java_bow_hand_follows_the_owner_admitted_definition() {
    for use_ticks in [19, 0, 2, 15] {
        for swing in [0.4, 0.0] {
            let publish = |condition| {
                let (mut equipment, pages) = equipment_fixture_with_bow_override(true, condition);
                if condition.is_none() {
                    equipment.set_pack_layer(None);
                }
                source_with_equipment(
                    Some(BOW),
                    true,
                    use_ticks,
                    Some(actor_animation::LocalSwingProgress {
                        bedrock: [swing; 2],
                        java: [swing; 2],
                        frame_alpha: Some(0.5),
                    }),
                    None,
                    (equipment, pages),
                )
            };
            let (baseline, baseline_meshes) = publish(None);
            let (rejected, rejected_meshes) =
                publish(Some("q.owner_identifier != 'minecraft:player'"));
            let (admitted, admitted_meshes) =
                publish(Some("q.owner_identifier == 'minecraft:player'"));
            if use_ticks == 19 && swing == 0.4 {
                for (name, source, meshes) in [
                    ("baseline", &baseline, &baseline_meshes),
                    ("rejected", &rejected, &rejected_meshes),
                    ("admitted", &admitted, &admitted_meshes),
                ] {
                    write_bow_frame(name, source, meshes);
                }
            }
            let baseline_item = &baseline.items[0].as_ref().unwrap().0;
            let rejected_item = &rejected.items[0].as_ref().unwrap().0;
            let admitted_item = &admitted.items[0].as_ref().unwrap().0;
            assert!(baseline_item.java_camera.is_some());
            assert_eq!(
                rejected_item.java_camera, baseline_item.java_camera,
                "rejected bow changed the Java hand: use_ticks={use_ticks}, swing={swing}"
            );
            assert_eq!(rejected_item.camera_space, baseline_item.camera_space);
            assert_eq!(
                rejected_item.presentation.location,
                baseline_item.presentation.location
            );
            assert_eq!(
                rejected_item.presentation.submission.input.current_bones,
                baseline_item.presentation.submission.input.current_bones
            );
            assert!(admitted_item.java_camera.is_none());
            assert!(!admitted_item.camera_space);
            assert_ne!(
                admitted_item.presentation.location,
                baseline_item.presentation.location
            );
            for source in [&baseline, &rejected, &admitted] {
                assert!(source.body.is_none());
                assert_eq!(source.items.iter().flatten().count(), 1);
                assert_resolved_textures(source);
            }
        }
    }
}

#[test]
fn authored_pack_bow_keeps_native_frame_inputs_in_java_mode() {
    let (source, _) = source_with_equipment(
        Some(BOW),
        true,
        9,
        None,
        Some(actor_animation::AttachableAnimationInput {
            is_local_player: true,
            use_elapsed_ticks: Some(9),
            animation_frame: 2,
            ..Default::default()
        }),
        equipment_fixture_with_bow_override(true, Some("1")),
    );
    let (item, atlas) = source.items[0].as_ref().unwrap();
    assert!(item.java_camera.is_none());
    let layer_start = item.presentation.location.layer() as usize
        * usize::from(atlas.width)
        * usize::from(atlas.height)
        * 4;
    assert_eq!(
        atlas.rgba8[layer_start + 2],
        170,
        "the pack controller must receive native frame 2 at elapsed use tick 9"
    );
}

#[test]
fn published_java_bow_uses_local_draw_time_for_placement_and_artwork() {
    let mut previous = None;
    for alpha in [0.25, 0.75] {
        let timing = actor_animation::AttachableAnimationInput {
            is_local_player: true,
            frame_alpha: alpha,
            use_elapsed_ticks: Some(1),
            ..Default::default()
        };
        let (source, meshes) = source_for_use_and_swing(Some(BOW), true, 19, None, Some(timing));
        let (item, atlas) = source.items[0].as_ref().unwrap();
        let mesh = meshes
            .iter()
            .find(|mesh| mesh.id == item.presentation.submission.input.rig)
            .unwrap();
        let [u, v] = mesh.vertices[0]
            .uv
            .map(|coordinate| (coordinate * 16.0) as usize);
        let layer = item.presentation.location.layer() as usize;
        let texel = ((layer * 16 + v) * 16 + u) * 4;
        assert_eq!(atlas.rgba8[texel + 2], 70, "the first local bow pull frame");
        let camera = item.java_camera.unwrap();
        if let Some(previous) = previous {
            assert!(!camera.abs_diff_eq(previous, 1e-6));
        }
        previous = Some(camera);
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
        assert!(source.body_camera.is_none());
    }
}

#[test]
fn first_person_texture_lookup_rejects_a_layer_absent_from_its_page() {
    let mut source = source_for(Some(BOW), true);
    let (mut item, _) = source.items[0].take().unwrap();
    let raster = render_model::EquipmentRaster {
        width: 16,
        height: 16,
        rgba8: vec![255; 16 * 16 * 4].into(),
    };
    let (_, locations) = render_model::ActorArtworkPages::default()
        .with_equipment_rasters(&[raster.clone(), raster.clone()]);
    let (artwork, _) = render_model::ActorArtworkPages::default().with_equipment_rasters(&[raster]);
    item.presentation.location = locations[1].unwrap();
    assert!(item_atlas(&item, &artwork).is_none());
}

#[test]
fn empty_hand_and_maps_keep_their_intentional_textured_arms() {
    let empty = source_for(None, true);
    assert!(empty.body.is_some());
    assert!(empty.body_camera.is_some());
    assert!(empty.items.iter().all(Option::is_none));
    assert_resolved_textures(&empty);
    let pixels = [0xff33_2211];
    for data in [
        None,
        Some((266, None)),
        Some((266, Some(pixels.as_slice()))),
    ] {
        let map = source_with_equipment_and_map(
            Some(FILLED_MAP),
            true,
            0,
            None,
            None,
            equipment_fixture(),
            data,
        )
        .0;
        assert!(
            map.body.is_some(),
            "Java's map is the held-item arm exception"
        );
        assert_eq!(
            map.presentation.skin_rgba8.as_ref().unwrap().len(),
            render_model::STANDARD_SKIN_BYTES
        );
        assert!(
            map.all_submissions().any(|draw| {
                draw.texture_layer & render_model::hand::HAND_MAP_BACKGROUND_FLAG != 0
            }),
            "missing identity or pixels must still keep the paper"
        );
        let received = data.is_some_and(|(_, pixels)| pixels.is_some());
        assert_eq!(map.items[0].is_some(), received);
        assert!(map.items[1].is_none());
        if let Some((_, atlas)) = &map.items[0] {
            assert_eq!(atlas.width, protocol::MAP_IMAGE_SIDE as u16);
            assert_eq!(atlas.height, protocol::MAP_IMAGE_SIDE as u16);
            assert_eq!(atlas.layers, 1);
            assert_eq!(&atlas.rgba8[..4], &pixels[0].to_le_bytes());
            assert_eq!(
                atlas.rgba8.len(),
                usize::from(atlas.width) * usize::from(atlas.height) * 4
            );
        } else {
            assert_resolved_textures(&map);
        }
    }
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

#[test]
fn java_bare_arm_takes_precedence_over_the_legacy_attack_clock() {
    let swing = actor_animation::LocalSwingProgress {
        bedrock: [0.25, 0.5],
        java: [0.1, 0.2],
        frame_alpha: Some(0.75),
    };
    let source = source_for_use_and_swing(None, true, 0, Some(swing), None).0;
    let expected = render_model::java_animation::first_person_arm(0.175, 1.0);
    assert!(source.body_camera.unwrap().abs_diff_eq(expected, 1e-5));
    let legacy = render_model::legacy_combat::legacy_first_person_arm(0.4375, 1.0);
    assert!(!expected.abs_diff_eq(legacy, 1e-3));
}

#[test]
fn retained_native_bow_stays_idle_when_the_newly_selected_food_is_used() {
    let (mut equipment, artwork) = equipment_fixture_with_native_bow(true);
    let mut stream = super::tests::head_stream();
    let mut feed = super::tests::uploaded_skin_feed(false);
    feed.first_person = true;
    feed.main_hand = Some(BOW.into());
    stream.sync_local_player_pose(&feed);
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(6);
    let bow = WornItem {
        identifier: BOW.into(),
        metadata: 0,
        damage: None,
        map_id: None,
        charged_projectile: None,
        use_facts: client_world::EffectiveItemUse::default(),
        grip: assets::item::grip::builtin_item_grip(BOW),
        kind: HeldKind::Sprite,
        dye_rgb: None,
        enchanted: false,
        glint: Default::default(),
        variant: Default::default(),
    };
    let mut cache = HandCache::default();
    let rig = stream.authority().actor_rig(1).unwrap();
    equipment.register_skin_rig(
        render_model::EntityRigId(rig.rig.0),
        rig.bone_names.to_vec(),
    );
    cache.remember(&rig, Some(&bow));
    feed.main_hand = Some("minecraft:golden_apple".into());
    feed.item_use = client_world::LocalItemUse::Using;
    stream.sync_local_player_pose(&feed);
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let selected = ActorEquipmentInput {
        main: Some(WornItem {
            identifier: "minecraft:golden_apple".into(),
            ..bow.clone()
        }),
        ..Default::default()
    };
    let rig = stream.authority().actor_rig(1).unwrap();
    assert_eq!(rig.java_equipped.unwrap().identifier.as_ref(), BOW);
    cache.remember(&rig, selected.main.as_ref());
    let actor = stream.authority().actor(1).unwrap();
    let presentation =
        crate::presentation::actors::entity_rig_presentation(&rig, actor, &artwork, 0.5).unwrap();
    let idle = equipment
        .first_person_java_item_with_use(
            &presentation.submission,
            &bow,
            JavaHand {
                swing: 0.0,
                equip: 1.0,
                using: None,
            },
            actor_animation::AttachableAnimationInput::default(),
        )
        .unwrap();
    let pulling = equipment
        .first_person_java_item_with_use(
            &presentation.submission,
            &bow,
            JavaHand {
                swing: 0.0,
                equip: 1.0,
                using: None,
            },
            actor_animation::AttachableAnimationInput {
                use_elapsed_ticks: Some(19),
                max_use_ticks: 32,
                ..Default::default()
            },
        )
        .unwrap();
    assert_ne!(idle.presentation.location, pulling.presentation.location);
    let source = hand_source(
        HandInputs {
            stream: &stream,
            presentation,
            equipment_input: &selected,
            owner_equipment: &selected,
            consume_ticks: Some(32),
            item_animation: Some(actor_animation::AttachableAnimationInput {
                first_person: true,
                is_local_player: true,
                animation_frame: 3,
                use_elapsed_ticks: Some(19),
                max_use_ticks: 32,
                hand_charged: true,
                ..Default::default()
            }),
            alpha: 0.5,
            artwork: &artwork,
            motion: Mat4::IDENTITY,
            sampling_camera: None,
            aspect: 1.0,
        },
        &mut equipment,
        &mut cache,
    )
    .unwrap();
    let draw = &source.items[0].as_ref().unwrap().0;
    assert_eq!(draw.presentation.location, idle.presentation.location);
    assert_eq!(
        draw.presentation.submission.input.rig,
        idle.presentation.submission.input.rig
    );
    assert!(draw.java_camera.is_some());
}
