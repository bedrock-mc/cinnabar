use super::*;
use std::sync::Arc;

/// Compiles an original player whose cape moves independently of the torso.
fn catalog() -> Arc<assets::RuntimeEntityAssets> {
    catalog_with_cape(Some([1.0; 3]))
}

/// Compiles a player with an optional cape bone and its authored animation scale.
fn catalog_with_cape(scale: Option<[f32; 3]>) -> Arc<assets::RuntimeEntityAssets> {
    let mut geometry = serde_json::json!({
        "format_version": "1.12.0",
        "minecraft:geometry": [
            {"description": {"identifier": "geometry.test_player", "texture_width":64, "texture_height":64},
             "bones": [
                {"name":"body", "pivot":[0,24,0], "cubes":[{"origin":[-4,12,-2], "size":[8,12,4], "uv":[16,16]}]},
                {"name":"cape", "parent":"body", "pivot":[0,24,3]}
             ]},
            {"description": {"identifier": assets::CAPE_GEOMETRY_IDENTIFIER, "texture_width":64, "texture_height":32},
             "bones": [
                {"name":"body", "pivot":[0,24,0]},
                {"name":"cape", "parent":"body", "pivot":[0,24,3], "bind_pose_rotation":[0,180,0],
                 "cubes":[{"origin":[-5,8,3], "size":[10,16,1], "uv":[0,0]}]}
             ]}
        ]
    });
    if scale.is_none() {
        geometry["minecraft:geometry"][0]["bones"]
            .as_array_mut()
            .unwrap()
            .retain(|bone| bone["name"] != "cape");
    }
    let entity = serde_json::json!({
        "format_version":"1.10.0", "minecraft:client_entity":{"description":{
            "identifier": assets::facts::VanillaEntity::Player.identifier(),
            "materials":{"default":"entity"}, "textures":{"default":"textures/test_player"},
            "geometry":{"default":"geometry.test_player"},
            "animations":{"motion":"animation.test.cape"}, "scripts":{"animate":["motion"]},
            "render_controllers":["controller.render.test_player"]
        }}
    });
    let mut animation = serde_json::json!({
        "format_version":"1.8.0", "animations":{"animation.test.cape":{"loop":true,"bones":{
            "body":{"rotation":["query.is_sneaking * 20",0,0]},
            "cape":{"rotation":["-10 - query.cape_flap_amount * 30", "query.life_time * 20", 0]}
        }}}
    });
    let bones = &mut animation["animations"]["animation.test.cape"]["bones"];
    match scale {
        Some(scale) => bones["cape"]["scale"] = serde_json::json!(scale),
        None => {
            bones.as_object_mut().unwrap().remove("cape");
        }
    }
    let render = serde_json::json!({
        "format_version":"1.8.0", "render_controllers":{"controller.render.test_player":{
            "geometry":"Geometry.default", "materials":[{"*":"Material.default"}], "textures":["Texture.default"]
        }}
    });
    let compiled = pack_entity_compiler::compile_entity_pack(vec![
        (
            "models/entity/player.geo.json".into(),
            geometry.to_string().into_bytes(),
        ),
        ("entity/player.json".into(), entity.to_string().into_bytes()),
        (
            "animations/player.json".into(),
            animation.to_string().into_bytes(),
        ),
        (
            "render_controllers/player.json".into(),
            render.to_string().into_bytes(),
        ),
    ])
    .unwrap()
    .unwrap();
    Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap())
}

/// Supplies a caped local player using the ordinary prediction-to-animation feed.
fn feed() -> client_world::LocalPlayerFeed {
    client_world::LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [0; 16],
        username: "Offline".into(),
        skin: protocol::PlayerSkin::Standard(render_api::StandardSkin {
            geometry: None,
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
            cape: Some(render_api::CapeImage {
                width: 64,
                height: 32,
                rgba8: vec![255; 64 * 32 * 4].into(),
            }),
        }),
        position: [0.0; 3],
        velocity: [0.0; 3],
        on_ground: true,
        flying: false,
        gliding: false,
        spinning: false,
        fall_fly_ticks: 0,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        equipment_count: 0,
        held_network_ids: [0; 2],
        held_variants: Default::default(),
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: true,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    }
}

/// Prepares real GUI model sources with an original cape atlas region.
fn presentation(catalog: &assets::RuntimeEntityAssets) -> UiPresentationRuntime {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let encoded = assets::encode_icon_catalog(catalog.source_manifest_sha256(), &[], &[]).unwrap();
    presentation.icon_catalog = Some(Arc::new(
        assets::RuntimeIconCatalog::decode(&encoded).unwrap(),
    ));
    presentation.icon_refs = Some(Box::new([]));
    presentation
        .set_gui_models(&assets::RuntimeAssets::diagnostic(), catalog)
        .unwrap();
    presentation.set_player_preview_skin(None, Default::default());
    presentation.player_preview_view = super::super::super::player_preview::PreviewView::Hud;
    presentation.menu_preview_model.cape_key = Some("test-cape".into());
    presentation.menu_artwork.refs.insert(
        "test-cape".into(),
        ui::IconRef {
            page: 100,
            uv: [0, 0, 64, 32],
            glint: false,
        },
    );
    presentation
}

/// Reads only the cape's projected corners, excluding skin motion and shared depth normalization.
fn cape_points(presentation: &UiPresentationRuntime) -> Vec<[f32; 2]> {
    let mesh = presentation.gui_player_mesh().unwrap();
    let batch = mesh
        .batches()
        .iter()
        .find(|batch| batch.texture_page == 100)
        .expect("cape draw");
    mesh.vertices()[batch.index_range.start as usize..batch.index_range.end as usize]
        .iter()
        .map(|vertex| vertex.position)
        .collect()
}

#[test]
fn hud_cape_follows_motion_and_keeps_animating_with_unchanged_pixels() {
    let catalog = catalog();
    let mut stream = tests::player_stream(Arc::clone(&catalog), Arc::clone(&catalog));
    let mut presentation = presentation(&catalog);
    let mut feed = feed();
    stream.sync_local_player_pose(&feed);
    stream.advance_actor_interpolation_ticks(1);
    presentation.capture_hud_player(Some(&stream), false);
    let resting = cape_points(&presentation);
    let texture = presentation.gui_models.skin.as_ref().unwrap().identity();
    for (speed, sprinting, sneaking) in [
        (0.1, false, false),
        (0.2, true, false),
        (0.05, false, true),
        (0.0, false, false),
    ] {
        feed.velocity[2] = speed;
        feed.position[2] += speed;
        feed.sprinting = sprinting;
        feed.sneaking = sneaking;
        stream.sync_local_player_pose(&feed);
        stream.advance_actor_interpolation_ticks(1);
        presentation.capture_hud_player(Some(&stream), false);
        assert_ne!(
            cape_points(&presentation),
            resting,
            "motion must reach the cape mesh"
        );
        assert_eq!(
            presentation.gui_models.skin.as_ref().unwrap().identity(),
            texture
        );
    }
    let moving = cape_points(&presentation);
    stream.advance_actor_interpolation_ticks(1);
    presentation.capture_hud_player(Some(&stream), false);
    assert_ne!(
        cape_points(&presentation),
        moving,
        "stationary authored sway must remain live"
    );
    presentation.capture_hud_player(None, false);
    let mesh = presentation.gui_player_mesh().unwrap();
    assert!(mesh.batches().iter().all(|batch| batch.texture_page != 100));
}

#[test]
fn hud_cape_and_torso_share_the_swimming_translation_once() {
    let catalog = catalog();
    let stream = tests::player_stream(Arc::clone(&catalog), Arc::clone(&catalog));
    let mut presentation = presentation(&catalog);
    presentation.capture_hud_player(Some(&stream), false);
    let standing = cape_points(&presentation);
    presentation.capture_hud_player(Some(&stream), true);
    let swimming = cape_points(&presentation);
    let expected = -super::super::super::player_preview::HUD_SWIM_OFFSET
        * super::super::super::player_preview::PREVIEW_PIXELS_PER_BLOCK
        / super::super::super::player_preview::PREVIEW_HEIGHT as f32;
    for (standing, swimming) in standing.iter().zip(swimming) {
        assert!((swimming[0] - standing[0]).abs() < 1e-6);
        assert!((swimming[1] - standing[1] - expected).abs() < 1e-6);
    }
}

#[test]
fn missing_or_hidden_cape_keeps_the_hud_body_visible() {
    for scale in [None, Some([0.0; 3])] {
        let catalog = catalog_with_cape(scale);
        let mut stream = tests::player_stream(Arc::clone(&catalog), Arc::clone(&catalog));
        stream.advance_actor_interpolation_ticks(1);
        let mut presentation = presentation(&catalog);
        presentation.capture_hud_player(Some(&stream), false);
        let mesh = presentation
            .gui_player_mesh()
            .expect("a missing or hidden cape must not hide the player's body");
        assert!(!mesh.vertices().is_empty());
        assert!(
            mesh.batches().iter().all(|batch| batch.texture_page != 100),
            "cape faces must be omitted for scale {scale:?}"
        );
    }
}

#[test]
fn flattened_cape_keeps_its_visible_faces_and_hud_body() {
    let catalog = catalog_with_cape(Some([0.0, 1.0, 1.0]));
    let mut stream = tests::player_stream(Arc::clone(&catalog), Arc::clone(&catalog));
    stream.advance_actor_interpolation_ticks(1);
    let mut presentation = presentation(&catalog);
    presentation.capture_hud_player(Some(&stream), false);
    let mesh = presentation
        .gui_player_mesh()
        .expect("collapsed cape faces must not hide the body or the remaining cape faces");
    assert!(mesh.batches().iter().any(|batch| batch.texture_page == 100));
    assert!(mesh.batches().iter().any(|batch| batch.texture_page != 100));
}

#[test]
fn worn_wings_suppress_the_hud_cape_until_the_chest_slot_is_cleared() {
    let catalog = catalog();
    let stream = tests::player_stream(Arc::clone(&catalog), Arc::clone(&catalog));
    let mut presentation = presentation(&catalog);
    presentation.capture_hud_player(Some(&stream), false);
    let visible = cape_points(&presentation);
    let reference = |identifier: &str| assets::EquipmentReference {
        identifier: identifier.into(),
        resolution: assets::EntityDependencyResolution::Catalog,
    };
    let binding = assets::EquipmentBinding {
        identifier: "fixture:wings".into(),
        attachable: None,
        category: assets::EquipmentCategory::Elytra,
        geometry: reference("geometry.fixture.wings"),
        texture: reference("textures/fixture/wings"),
        resources: Default::default(),
        material: "entity".into(),
        color_mask: assets::EquipmentColorMask::NoMask,
        render_controller: "controller.render.fixture.wings".into(),
        first_person: assets::EquipmentTransform::NeedsMeasurement,
        third_person: assets::EquipmentTransform::NeedsMeasurement,
        dropped: assets::EquipmentTransform::NeedsMeasurement,
        poses: Box::new([]),
    };
    presentation.set_equipment_catalog(Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [1; 32],
            vec![binding],
            vec![assets::EquipmentTexture {
                identifier: "textures/fixture/wings".into(),
                width: 64,
                height: 32,
                rgba8: [255; 4].repeat(64 * 32).into(),
            }],
        )
        .unwrap(),
    )));
    presentation.set_player_preview_gear([None, Some(("fixture:wings", None)), None, None], None);
    let mesh = presentation.gui_player_mesh().unwrap();
    assert!(mesh.batches().iter().all(|batch| batch.texture_page != 100));
    presentation.set_player_preview_gear([None; 4], None);
    assert_eq!(cape_points(&presentation), visible);
}
