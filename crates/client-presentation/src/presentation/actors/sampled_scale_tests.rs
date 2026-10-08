use super::*;
use bevy::math::{Mat4, Vec3};
use image::ImageEncoder;
use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, WorldBootstrap, WorldEvent,
};

/// A compiled creeper whose scale crosses an admission boundary within one tick.
fn world(scale: &str, axis: bool, position: [f32; 3]) -> client_world::WorldAuthority {
    let slot = if axis { "scaleX" } else { "scale" };
    let entity = format!(
        r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"minecraft:creeper","materials":{{"default":"entity"}},"textures":{{"default":"textures/entity/test"}},"geometry":{{"default":"geometry.test"}},"animations":{{"swell":"animation.test.swell"}},"scripts":{{"{slot}":"{scale}","animate":["swell"]}},"render_controllers":["controller.render.test"]}}}}}}"#
    );
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":64,"texture_height":64,"visible_bounds_width":2,"visible_bounds_height":2,"visible_bounds_offset":[0,1,0]},"bones":[{"name":"body","pivot":[0,0,0],"cubes":[{"origin":[-4,0,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#;
    let animation = br#"{"format_version":"1.8.0","animations":{"animation.test.swell":{"loop":true,"bones":{"body":{"scale":["1 + query.swell_amount",1,1]}}}}}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let mut texture = Vec::new();
    image::codecs::png::PngEncoder::new(&mut texture)
        .write_image(&[255; 4], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let compiled = pack_compiler::compile_entity_pack(vec![
        ("entity/creeper.json".into(), entity.into_bytes()),
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
                    identifier: "minecraft:creeper".into(),
                },
                position,
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([ActorMetadata {
                    key: 0,
                    value: ActorMetadataValue::Flags(1 << 10),
                }]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    world.advance_actor_interpolation_ticks(3);
    world
}

#[test]
fn sampled_scales_admit_a_zero_tick_scale_and_expanding_frustum_bounds() {
    assert_sampled_admission(false);
}

#[test]
fn sampled_scales_can_admit_a_frame_after_the_animation_tick_was_culled() {
    assert_sampled_admission(true);
}

/// Exercises scale sampling, both culling stages and body construction on compiled scripts.
fn assert_sampled_admission(cull_completed_tick: bool) {
    let camera = Vec3::new(0.0, 1.0, 0.0);
    let view = ActorCullView {
        camera_position: camera,
        clip_from_world: Mat4::perspective_infinite_reverse_rh(
            90_f32.to_radians(),
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * Mat4::look_to_rh(camera, -Vec3::Z, Vec3::Y),
        max_distance: 100.0,
    };
    let (artwork, locations) =
        ActorArtworkPages::default().with_equipment_rasters(&[render::EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([255; 4]),
        }]);
    let location = locations[0].unwrap();
    for (expression, axis, feet) in [
        ("query.swell_amount > 0.08 ? 2 : 0", false, [0.0, 0.0, -5.0]),
        ("query.swell_amount > 0.08 ? 4 : 1", false, [7.5, 0.0, -5.0]),
        ("query.swell_amount > 0.08 ? 4 : 1", true, [7.5, 0.0, -5.0]),
    ] {
        let mut world = world(expression, axis, feet);
        if cull_completed_tick {
            let view = client_world::ActorAnimationView {
                planes: [[0.0, 0.0, 0.0, -1.0]; 6],
                camera: [0.0; 3],
                player_distance: 100.0,
                entity_radius: 100.0,
            };
            world.set_actor_animation_view(Some(view));
            world.advance_actor_interpolation_ticks(1);
            world.set_actor_animation_view(Some(client_world::ActorAnimationView {
                planes: [[0.0, 0.0, 0.0, 1.0]; 6],
                ..view
            }));
        }
        let actor = world.actor(1).unwrap();
        let tick = world.actor_rig(1).unwrap();
        assert!(
            !tick.render.is_empty(),
            "fixture must select a drawable texture layer"
        );
        if feet[0] > 0.0 {
            assert!(!rig_may_be_visible(
                &tick,
                actor,
                0.5,
                Some(view),
                |_, _| false
            ));
        }
        let mut frame = world.actor_render_frame(0.5);
        let sampled = frame.sample_rig_scale(tick);
        assert!(
            rig_may_be_visible(&sampled, actor, 0.5, Some(view), |_, _| false),
            "sampled expanding bounds must survive early culling: {sampled:?}"
        );
        let mut presentation = entity_rig_presentation_cached(&sampled, actor, &artwork, 0.5, None)
            .expect("positive sampled scale must survive admission with zero tick scale");
        presentation.artwork = Some(location);
        presentation.submission.route = ActorRigRoute::Compiled;
        let batch =
            select_actor_presentations_for_view(999, false, None, [presentation], Some(view));
        assert_eq!(
            batch.submissions.len(),
            1,
            "sampled placement must survive final culling"
        );
        let layers = frame.layers(1).unwrap();
        let scale = layers[0].sampled_scale.unwrap();
        assert_eq!(sampled.scale, scale[0]);
        assert_eq!(sampled.axis_scale, [scale[1], scale[2], scale[3]]);
        assert_eq!(world.actor_rig(1).unwrap().scale, tick.scale);
    }
}
