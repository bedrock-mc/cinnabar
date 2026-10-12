//! Original rig fixtures for the selected placement frame and projected snapshot.

use super::*;
use image::ImageEncoder;
use protocol::{ActorEvent, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render_model::{UiRenderBatch, UiRenderInput, UiRenderTextureArray, UiRenderVertex, UiScissor};

/// Compiles an original camera-facing plane, without installed game assets.
fn world(identifier: &str) -> client_world::WorldAuthority {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
        "identifier":"minecraft:xp_orb","materials":{"default":"entity"},
        "textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},
        "animations":{"view":"animation.test.view"},"scripts":{"animate":["view"]},
        "render_controllers":["controller.render.test"]}}}"#;
    let entity = String::from_utf8(entity.to_vec())
        .unwrap()
        .replace("minecraft:xp_orb", identifier);
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{
        "identifier":"geometry.test","texture_width":1,"texture_height":1},
        "bones":[{"name":"body","pivot":[0,0,0],"cubes":[{"origin":[-4,0,-0.01],
        "size":[8,8,0.02],"uv":[0,0]}]}]}]}"#;
    let animation = br#"{"format_version":"1.8.0","animations":{"animation.test.view":{
        "loop":true,"bones":{"body":{"rotation":["query.camera_rotation(0)",
        "query.camera_rotation(1)",0]}}}}}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{
        "geometry":"Geometry.default","materials":[{"*":"Material.default"}],
        "textures":["Texture.default"]}}}"#;
    let mut texture = Vec::new();
    image::codecs::png::PngEncoder::new(&mut texture)
        .write_image(&[230, 160, 45, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let compiled = pack_compiler::compile_entity_pack(vec![
        ("entity/test.json".into(), entity.into_bytes()),
        ("models/entity/test.geo.json".into(), geometry.to_vec()),
        ("animations/test.animation.json".into(), animation.to_vec()),
        ("render_controllers/test.json".into(), controller.to_vec()),
        ("textures/entity/test.png".into(), texture),
    ])
    .unwrap()
    .unwrap();
    let mut world = client_world::WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 999,
            local_player_unique_id: 999,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Some(Arc::new(
            assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap(),
        )),
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: ActorKind::Entity {
                    identifier: identifier.into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 75.0,
                head_yaw: 75.0,
                body_yaw: 75.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    world.set_actor_camera_rotation([0.0; 2]);
    world.advance_actor_interpolation_ticks(3);
    world
}

/// Projects the production placement transform through the offline UI raster harness.
fn snapshot(rows: [[f32; 4]; 3]) {
    let vertices = [
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ]
    .map(|point| {
        let transformed: [f32; 3] =
            rows.map(|row| row[0] * point[0] + row[1] * point[1] + row[2] * point[2] + row[3]);
        UiRenderVertex {
            position: [64.0 + transformed[0] * 80.0, 64.0 - transformed[1] * 80.0],
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [0.0; 2],
            color: [230, 160, 45, 255],
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        }
    });
    let input = UiRenderInput {
        revision: 1,
        viewport_size: [128; 2],
        safe_area: [0; 4],
        vertices: Arc::from(vertices),
        indices: Arc::from([0, 1, 2, 0, 2, 3]),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 128, 128),
            0,
            6,
            0,
        )]),
        textures: Arc::new(
            UiRenderTextureArray::new(
                vec![render_model::UiTexturePage::owned([1; 2], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    input.validate().unwrap();
    let pixels = client_ui::ui_runtime::presentation::forms::snapshot::rasterize(&input);
    assert!(pixels.pixels().any(|pixel| pixel.0 == [230, 160, 45, 255]));
    client_ui::ui_runtime::presentation::forms::snapshot::write(&input, "selected-rig-placement");
}

#[test]
fn selected_camera_rig_keeps_its_frame_with_an_arbitrary_display_identifier() {
    let world = world("minecraft:xp_orb");
    let rig = world.actor_rig(1).unwrap();
    let sampled = ActorRigSnapshot {
        previous_body_yaw: 75.0,
        body_yaw: 75.0,
        ..rig
    };
    let mut actor = world.actor(1).unwrap().clone();
    actor.kind = ActorKind::Entity {
        identifier: "fixture:arbitrary_display".into(),
    };
    let (rows, yaw) = sampled_rig_placement(&sampled, &actor, 0.5, [1.0; 4]).unwrap();
    snapshot(rows);
    assert_eq!(yaw, 0.0, "the selected rig owns the placement frame");
}

#[test]
fn camera_query_presence_does_not_assign_a_placement_frame() {
    let world = world("fixture:camera_only");
    assert_eq!(
        world.actor_rig(1).unwrap().placement_frame,
        assets::placement::EntityPlacementFrame::Unresolved
    );
}

#[test]
fn placement_frame_keeps_body_placement_independent_from_absolute_target_rotation() {
    let world = world("minecraft:arrow");
    let rig = world.actor_rig(1).unwrap();
    let sampled = ActorRigSnapshot {
        previous_body_yaw: 75.0,
        body_yaw: 75.0,
        placement_frame: assets::placement::EntityPlacementFrame::Provisional {
            placement: assets::placement::ActorYawFrame::Body,
            target_rotation: assets::placement::ActorYawFrame::Absolute,
        },
        ..rig
    };
    let mut actor = world.actor(1).unwrap().clone();
    actor.kind = ActorKind::Entity {
        identifier: "fixture:arbitrary_display".into(),
    };
    assert_eq!(
        sampled.placement_frame.target_rotation(),
        Some(assets::placement::ActorYawFrame::Absolute)
    );
    let (_, yaw) = sampled_rig_placement(&sampled, &actor, 0.5, [1.0; 4]).unwrap();
    assert_eq!(yaw, 75.0);
}
