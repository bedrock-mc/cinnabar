//! Cape composition consumes the body's controller visibility after equipment inheritance.
use super::*;
use assets::{RuntimeAssets, RuntimeEntityAssets};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};

#[path = "worn_category_tests.rs"]
mod worn_categories;

/// Supplies original body/cape sources and an optional visibility controller for layer fixtures.
fn fixture_files(controlled: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let geometry = serde_json::json!({
        "format_version":"1.12.0", "minecraft:geometry":[
            {"description":{"identifier":"geometry.fixture","texture_width":64,"texture_height":32},
             "bones":[{"name":"body","pivot":[0,24,0],
                 "cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[0,0]}]},
                 {"name":"cape","parent":"body","pivot":[0,24,3]}]},
            {"description":{"identifier":assets::CAPE_GEOMETRY_IDENTIFIER,"texture_width":64,"texture_height":32},
             "bones":[{"name":"body","pivot":[0,24,0]},
                 {"name":"cape","parent":"body","pivot":[0,24,3],
                  "cubes":[{"origin":[-5,8,3],"size":[10,16,1],"uv":[0,0]}]}]}
        ]
    });
    let mut controller = serde_json::json!({
        "geometry":"Geometry.default", "materials":[{"*":"Material.default"}],
        "textures":["Texture.default"]
    });
    if controlled {
        controller["part_visibility"] = serde_json::json!([{"cape":"!query.is_sneaking"}]);
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(64, 32, image::Rgba([180, 80, 20, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    vec![
        ("textures/fixture.png".into(), png.into_inner()),
        ("entity/player.json".into(), br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
            "identifier":"minecraft:player","materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/fixture"},"geometry":{"default":"geometry.fixture"},
            "render_controllers":["controller.render.fixture"]}}}"#.to_vec()),
        ("models/entity/fixture.json".into(), serde_json::to_vec(&geometry).unwrap()),
        ("render_controllers/fixture.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.8.0", "render_controllers":{"controller.render.fixture":controller}
        })).unwrap()),
    ]
}

/// Compiles an original body, cape and optional sneaking-dependent visibility controller.
fn fixture(controlled: bool) -> Arc<RuntimeEntityAssets> {
    let pack = pack_entity_compiler::compile_actor_pack(fixture_files(controlled))
        .unwrap()
        .unwrap();
    Arc::new(RuntimeEntityAssets::from_compiled(pack.entities).unwrap())
}

/// Publishes an original player profile and spawn into the same stream used by layer assembly.
fn stream(entities: Arc<RuntimeEntityAssets>) -> WorldStream {
    let mut stream = WorldStream::new_with_asset_sets(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        entities,
        [0.0; 3],
        None,
    );
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::PlayerList(protocol::PlayerListUpdateEvent {
                entries: Arc::from([protocol::PlayerListEntry::Add {
                    uuid: [2; 16],
                    unique_id: 2,
                    username: "fixture".into(),
                    xuid: "".into(),
                    verified: true,
                    skin: protocol::PlayerSkin::Standard(render_api::StandardSkin {
                        width: 64,
                        height: 64,
                        rgba8: vec![255; 64 * 64 * 4].into(),
                        geometry: None,
                        cape: Some(render_api::CapeImage {
                            width: 64,
                            height: 32,
                            rgba8: [50, 180, 90, 255].repeat(64 * 32).into(),
                        }),
                    }),
                }]),
            })),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Player {
                    uuid: [2; 16],
                    username: "fixture".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream.prepare_actor_appearance_fixture();
    stream
}

/// Reuses publication caches across controller changes and repeated draws of each sampled tick.
fn check_visibility(controlled: bool) {
    let entities = fixture(controlled);
    let mut stream = stream(Arc::clone(&entities));
    let mut state = ActorFrameState::default();
    let cape = match &stream.authority().actor_player_profile(2).unwrap().skin {
        protocol::PlayerSkin::Standard(skin) => skin.cape.clone().unwrap(),
        _ => unreachable!(),
    };
    let (_, preparation) = state.cape_state.rig(Some(&entities)).unwrap();
    preparation.request(&cape);
    preparation.submit();
    preparation.finish_fixture_batch();
    let mut scene = ActorRenderScene::default();
    for (step, sneaking) in [false, true, false, true, false].into_iter().enumerate() {
        stream
            .submit(
                3 + step as u64,
                WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                    runtime_id: 2,
                    dimension: 0,
                    tick: step as u64,
                    metadata: Arc::from([protocol::ActorMetadata {
                        key: protocol_ids::ActorDataKey::Flags.wire_value(),
                        value: protocol::ActorMetadataValue::Flags(if sneaking {
                            1 << protocol_ids::ActorFlag::Sneaking.wire_value()
                        } else {
                            0
                        }),
                    }]),
                    properties: Arc::from([]),
                })),
            )
            .unwrap();
        stream.advance_actor_interpolation_ticks(1);
        let authority = stream.authority();
        let rig = authority.actor_rig(2).unwrap();
        let cape_index = rig
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "cape")
            .unwrap();
        assert_eq!(
            rig.render[0].hidden_bones.contains(&(cape_index as u32)),
            controlled && sneaking
        );
        let body = crate::presentation::actors::actor_rig_presentation(
            &rig,
            authority.actor(2).unwrap(),
            None,
            1.0,
        )
        .unwrap();
        let original = Arc::clone(&body.submission.input.current_bones);
        let mut frame = authority.actor_render_frame(1.0);
        let mut previous = None;
        for _ in 0..2 {
            state.layer_poses.begin_frame();
            let mut batch = crate::presentation::actors::select_actor_presentations(
                2,
                true,
                Some(body.clone()),
                [],
            );
            layers::attach(
                &mut batch,
                Some(&stream),
                Some(&mut frame),
                layers::LayerPoses {
                    local_runtime_id: 2,
                    local_equipment: &Default::default(),
                    java_posed: &[],
                    combat_visuals: render_api::DEFAULT_COMBAT_VISUALS,
                    local_emote_pose: None,
                    remote_emote_poses: &Default::default(),
                    native_body_sampled: false,
                    local_coverage: false,
                    remote_skin_layers: &Default::default(),
                    native_pose: &state.java_hand.native_pose,
                },
                layers::LayerCaches {
                    equipment: None,
                    layer_poses: &mut state.layer_poses,
                    cape_state: &mut state.cape_state,
                    skin_layers: &mut state.skin_layers,
                    skin_rigs: &mut state.skin_rigs,
                },
                &Default::default(),
                Some(&entities),
                &mut scene,
                (1.0, 0.05, None),
                &mut Vec::new(),
            );
            let body = batch
                .submissions
                .iter()
                .find(|draw| draw.input.identity.layer == render_model::ACTOR_LAYER_BODY)
                .unwrap();
            let cape = batch
                .submissions
                .iter()
                .find(|draw| {
                    draw.input.identity.layer == crate::presentation::cape::ACTOR_LAYER_CAPE
                })
                .unwrap();
            let expected = if controlled && sneaking { 0.0 } else { 1.0 };
            assert_eq!(
                body.input.current_bones[cape_index].translation_scale[3],
                expected
            );
            assert_eq!(
                cape.input.current_bones[1].translation_scale[3], expected,
                "cape visibility must follow its body controller"
            );
            assert_eq!(body.input.current_bones[0].translation_scale[3], 1.0);
            assert_eq!(
                original[cape_index].translation_scale[3], 1.0,
                "masking must not mutate inherited poses"
            );
            if let Some((last_body, last_cape)) = previous {
                assert_eq!(body.input, last_body);
                assert_eq!(cape.input, last_cape);
            }
            previous = Some((body.input.clone(), cape.input.clone()));
        }
    }
}

#[test]
fn bedrock_cape_follows_controller_visibility_toggles() {
    check_visibility(true);
}

#[test]
fn ordinary_bedrock_cape_stays_visible_while_sneaking() {
    check_visibility(false);
}
