use super::*;
use std::sync::Arc;

/// Compiles original one-cube player geometry without installed assets or a server.
fn player_catalog(size: u32) -> Arc<assets::RuntimeEntityAssets> {
    let geometry = format!(
        r#"{{"format_version":"1.12.0","minecraft:geometry":[{{"description":{{"identifier":"geometry.test_player","texture_width":64,"texture_height":64}},"bones":[{{"name":"body","pivot":[0,0,0],"cubes":[{{"origin":[0,0,0],"size":[{size},8,4],"uv":[0,0]}}]}}]}}]}}"#
    );
    player_catalog_geometry(geometry.into_bytes())
}

fn player_catalog_geometry(geometry: Vec<u8>) -> Arc<assets::RuntimeEntityAssets> {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","materials":{"default":"entity"},"textures":{"default":"textures/entity/test_player"},"geometry":{"default":"geometry.test_player"},"render_controllers":["controller.render.test_player"]}}}"#;
    let render = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test_player":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let compiled = pack_compiler::compile_entity_pack(vec![
        ("entity/player.json".into(), entity.to_vec()),
        ("models/entity/player.geo.json".into(), geometry),
        ("render_controllers/player.json".into(), render.to_vec()),
    ])
    .unwrap()
    .expect("original fixture compiles");
    assert_eq!(compiled.assets.rig_bindings.len(), 1, "player rig compiles");
    Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap())
}

#[test]
fn hud_local_emote_moves_vertices_and_equipment_without_replacing_geometry_or_native_pose() {
    let catalog = player_catalog_geometry(br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},"bones":[
        {"name":"root","pivot":[0,0,0]},
        {"name":"waist","parent":"root","pivot":[0,11,0]},
        {"name":"body","parent":"waist","pivot":[0,22,0],"cubes":[{"origin":[-3,11,-2],"size":[6,11,4],"uv":[0,0]}]},
        {"name":"head","parent":"body","pivot":[0,24,0]},
        {"name":"leftArm","parent":"body","pivot":[4,20,0]},
        {"name":"rightArm","parent":"body","pivot":[-4,20,0]},
        {"name":"leftLeg","parent":"root","pivot":[2,11,0]},
        {"name":"rightLeg","parent":"root","pivot":[-2,11,0]}]}]}"#.to_vec());
    let stream = player_stream(Arc::clone(&catalog), catalog);
    let native = stream
        .authority()
        .actor_ui_pose(stream.local_player_runtime_id())
        .unwrap()
        .to_vec();
    let mut presentation =
        UiPresentationRuntime::new(super::super::super::tests::fixture_font()).unwrap();
    presentation.capture_hud_player(Some(&stream), false);
    let rest = presentation.gui_models.live_player.vertices.clone();
    let geometry = presentation
        .gui_models
        .live_player
        .geometry
        .as_ref()
        .unwrap()
        .vertices
        .as_ptr();
    let emote = client_world::CustomEmote::Twerk;
    presentation.capture_hud_player_with_emote(Some(&stream), false, Some((emote, 0.0)));
    let first = presentation.gui_models.live_player.vertices.clone();
    assert_ne!(first, rest);
    assert!(
        presentation
            .gui_models
            .live_player
            .parts
            .iter()
            .all(Option::is_some)
    );
    presentation.capture_hud_player_with_emote(
        Some(&stream),
        false,
        Some((emote, emote.duration_seconds() / 4.0)),
    );
    assert_ne!(presentation.gui_models.live_player.vertices, first);
    assert_eq!(
        presentation
            .gui_models
            .live_player
            .geometry
            .as_ref()
            .unwrap()
            .vertices
            .as_ptr(),
        geometry
    );
    assert_eq!(
        stream
            .authority()
            .actor_ui_pose(stream.local_player_runtime_id())
            .unwrap(),
        native
    );
    presentation.capture_hud_player_with_emote(
        Some(&stream),
        false,
        Some((emote, emote.duration_seconds())),
    );
    assert_eq!(presentation.gui_models.live_player.vertices, first);
    presentation.capture_hud_player_with_emote(Some(&stream), false, None);
    assert_eq!(presentation.gui_models.live_player.vertices, rest);
}

/// Spawns an offline player whose rig comes from the supplied session catalog.
fn player_stream(
    vanilla: Arc<assets::RuntimeEntityAssets>,
    pack: Arc<assets::RuntimeEntityAssets>,
) -> chunk_pipeline::WorldStream {
    let mut stream = chunk_pipeline::WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        vanilla,
        [0.0; 3],
        None,
    );
    stream.set_pack_entities(Some((pack, vec![0])));
    stream
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: protocol::ActorKind::Player {
                    uuid: [0; 16],
                    username: "Offline".into(),
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
    stream
}

#[test]
fn hud_uses_the_session_player_geometry_and_refreshes_between_catalogs() {
    let vanilla = player_catalog(8);
    let mut presentation =
        UiPresentationRuntime::new(super::super::super::tests::fixture_font()).unwrap();
    for size in [24, 32] {
        let stream = player_stream(Arc::clone(&vanilla), player_catalog(size));
        let rig = stream.authority().actor_rig(1).expect("pack player rig");
        assert_eq!(rig.rig.0, assets::PACK_RIG_ID_BASE);
        assert!(rig.skin_geometry.is_none());
        presentation.capture_hud_player(Some(&stream), false);
        let live = &presentation.gui_models.live_player;
        assert!(
            !live.vertices.is_empty(),
            "server cube must replace the biped"
        );
        let width = live
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max)
            - live
                .vertices
                .iter()
                .map(|vertex| vertex.position[0])
                .fold(f32::INFINITY, f32::min);
        let expected = size as f32 / 16.0 / super::super::super::player_preview::PLAYER_MODEL_SCALE;
        assert!((width - expected).abs() < 1e-4, "{width} != {expected}");
    }
}

#[test]
fn hud_fire_follows_authoritative_burning_metadata_and_clears_after_session_end() {
    let catalog = player_catalog(8);
    let mut stream = player_stream(Arc::clone(&catalog), catalog);
    let mut presentation =
        UiPresentationRuntime::new(super::super::super::tests::fixture_font()).unwrap();
    for (sequence, flags, burning) in [(2, 1, true), (3, 2, false), (4, 1, true)] {
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                    protocol::ActorMetadataUpdateEvent {
                        dimension: 0,
                        runtime_id: 1,
                        metadata: Arc::from([protocol::ActorMetadata {
                            key: 0,
                            value: protocol::ActorMetadataValue::Flags(flags),
                        }]),
                        properties: Arc::from([]),
                        tick: sequence,
                    },
                )),
            )
            .unwrap();
        presentation.capture_hud_player(Some(&stream), false);
        assert_eq!(
            presentation.gui_models.live_player.fire_size.is_some(),
            burning
        );
    }
    presentation.capture_hud_player(Some(&stream), true);
    assert_eq!(
        presentation.gui_models.live_player.outer_y,
        super::super::super::player_preview::HUD_SWIM_OFFSET
    );
    presentation.capture_hud_player(None, false);
    assert!(presentation.gui_models.live_player.fire_size.is_none());
    assert_eq!(presentation.gui_models.live_player.outer_y, 0.0);
}

#[test]
fn hud_fire_color_uses_completed_actor_ticks_and_fades_after_extinguishing() {
    let catalog = player_catalog(8);
    let mut stream = player_stream(Arc::clone(&catalog), catalog);
    let mut presentation =
        UiPresentationRuntime::new(super::super::super::tests::fixture_font()).unwrap();
    let set_fire = |stream: &mut chunk_pipeline::WorldStream, sequence, flags| {
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                    protocol::ActorMetadataUpdateEvent {
                        dimension: 0,
                        runtime_id: 1,
                        metadata: Arc::from([protocol::ActorMetadata {
                            key: 0,
                            value: protocol::ActorMetadataValue::Flags(flags),
                        }]),
                        properties: Arc::from([]),
                        tick: sequence,
                    },
                )),
            )
            .unwrap();
    };
    set_fire(&mut stream, 2, 1);
    presentation.capture_hud_player(Some(&stream), false);
    assert_eq!(
        presentation.gui_models.live_player.overlay_color,
        [0.8, 0.3, 0.0, 0.0]
    );
    stream.advance_actor_interpolation_ticks(client_world::FIRE_FADE_TICKS);
    presentation.capture_hud_player(Some(&stream), false);
    assert_eq!(
        presentation.gui_models.live_player.overlay_color,
        [0.8, 0.15, 0.0, 0.7]
    );
    let steady = presentation.gui_models.live_player.overlay_color;
    presentation.capture_hud_player(Some(&stream), false);
    assert_eq!(
        presentation.gui_models.live_player.overlay_color, steady,
        "render frames do not advance on_fire_time"
    );
    stream.advance_actor_interpolation_ticks(client_world::FIRE_FADE_TICKS);
    presentation.capture_hud_player(Some(&stream), false);
    assert!((presentation.gui_models.live_player.overlay_color[1] - 0.3).abs() < 1e-6);
    set_fire(&mut stream, 3, 0);
    presentation.capture_hud_player(Some(&stream), false);
    assert!(presentation.gui_models.live_player.fire_size.is_none());
    assert_eq!(
        presentation.gui_models.live_player.overlay_color,
        [0.8, 0.3, 0.0, 0.7]
    );
    stream.advance_actor_interpolation_ticks(2);
    presentation.capture_hud_player(Some(&stream), false);
    assert!((presentation.gui_models.live_player.overlay_color[3] - 0.4536).abs() < 1e-6);
    stream.advance_actor_interpolation_ticks(client_world::FIRE_FADE_TICKS - 2);
    presentation.capture_hud_player(Some(&stream), false);
    assert_eq!(presentation.gui_models.live_player.overlay_color, [0.0; 4]);
    set_fire(&mut stream, 4, 1);
    stream.advance_actor_interpolation_ticks(client_world::FIRE_FADE_TICKS);
    stream
        .submit(
            5,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Status(protocol::ActorStatusEvent {
                runtime_id: 1,
                kind: protocol::ActorStatusKind::Hurt,
                data: 0,
            })),
        )
        .unwrap();
    presentation.capture_hud_player(Some(&stream), false);
    assert_eq!(
        presentation.gui_models.live_player.overlay_color,
        [1.0, 0.0, 0.0, 0.25]
    );
}
