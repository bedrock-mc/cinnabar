use super::*;
use bevy::math::Vec3;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};

/// Compiles an original owner whose ordinary held block has an authored scale.
fn owner_world(scale: Option<serde_json::Value>) -> client_world::WorldAuthority {
    let mut description = serde_json::json!({
        "identifier": "test:holder", "materials": {"default": "entity_alphatest"},
        "textures": {"default": "textures/entity/holder"},
        "geometry": {"default": "geometry.holder"},
        "render_controllers": ["controller.render.holder"]
    });
    if let Some(scale) = scale {
        description["held_item_scale"] = scale;
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let files = vec![
        ("entity/holder.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version": "1.10.0", "minecraft:client_entity": {"description": description}
        })).unwrap()),
        ("models/entity/holder.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.holder","texture_width":1,"texture_height":1},"bones":[{"name":"rightItem"},{"name":"leftItem"}]}]}"#.to_vec()),
        ("render_controllers/holder.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.holder":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#.to_vec()),
        ("textures/entity/holder.png".into(), png.into_inner()),
    ];
    let compiled = pack_entity_compiler::compile_entity_pack(files)
        .unwrap()
        .unwrap();
    let mut world = client_world::WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        Some(Arc::new(
            RuntimeEntityAssets::from_compiled(compiled.assets).unwrap(),
        )),
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Entity {
                    identifier: "test:holder".into(),
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
            Some(1),
        )
        .unwrap();
    world.advance_actor_interpolation_ticks(2);
    world
}

/// Measures the generated cube after its complete equipment bone transform.
fn extent(runtime: &mut EquipmentRuntime, draw: &EquipmentPresentation) -> Vec3 {
    let geometry = runtime
        .take_pending_geometries()
        .into_iter()
        .find(|geometry| geometry.id == draw.submission.input.rig)
        .unwrap();
    draw_extent(draw, &geometry)
}

/// Measures transformed vertices independently of artwork and geometry cache residency.
fn draw_extent(draw: &EquipmentPresentation, geometry: &ActorRigGeometry) -> Vec3 {
    let bone = draw.submission.input.current_bones[0];
    let matrix = bevy::math::Mat4::from_scale_rotation_translation(
        Vec3::from_array(bone.axis_scale[..3].try_into().unwrap()) * bone.translation_scale[3],
        Quat::from_array(bone.rotation),
        Vec3::from_array(bone.translation_scale[..3].try_into().unwrap()),
    );
    let (mut low, mut high) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for vertex in geometry.vertices.iter() {
        let point = matrix.transform_point3(Vec3::from_array(vertex.position));
        low = low.min(point);
        high = high.max(point);
    }
    high - low
}

#[test]
fn owner_scale_keeps_offhand_and_body_unchanged() {
    let bounds = [1.0, 2.0].map(|scale| {
        let world = owner_world(Some(serde_json::json!(scale)));
        let rig = world.actor_rig(2).unwrap();
        assert_eq!((rig.scale, rig.axis_scale), (1.0, [1.0; 3]));
        let (mut runtime, mut body, item) = block_fixture();
        runtime.register_skin_rig(body.input.rig, vec!["rightItem".into(), "leftItem".into()]);
        body.input.previous_bones = Arc::from([body.input.previous_bones[0]; 2]);
        body.input.current_bones = Arc::clone(&body.input.previous_bones);
        let original = Arc::clone(&body.input.current_bones);
        let layers = runtime
            .layers_for(
                &body,
                &ActorEquipmentInput {
                    main: Some(item.clone()),
                    off: Some(item),
                    ..Default::default()
                },
                Some(EquipmentAnimation {
                    owner: world.actor(2).unwrap(),
                    rig: &rig,
                    frame_alpha: 0.5,
                    delta_seconds: 0.016,
                }),
            )
            .to_vec();
        assert_eq!(layers.len(), 2);
        assert_eq!(body.input.current_bones, original);
        let mesh = runtime.take_pending_geometries().pop().unwrap();
        [
            draw_extent(&layers[0], &mesh),
            draw_extent(&layers[1], &mesh),
        ]
    });
    assert!((bounds[1][0] - bounds[0][0] * 2.0).abs().max_element() < 1e-5);
    assert_eq!(bounds[1][1], bounds[0][1]);
}

#[test]
fn first_person_block_uses_owner_scale_without_moving_the_hand() {
    let draws = [None, Some(serde_json::json!(1)), Some(serde_json::json!(2))].map(|scale| {
        let world = owner_world(scale);
        let rig = world.actor_rig(2).unwrap();
        let (mut runtime, body, item) = block_fixture();
        let draw = runtime
            .first_person_item_with_use(
                &body,
                &item,
                FirstPersonHand {
                    swing: 0.0,
                    equip: 1.0,
                    consume: None,
                    aspect: 1.0,
                },
                render_api::DEFAULT_COMBAT_VISUALS,
                actor_animation::AttachableAnimationInput {
                    first_person: true,
                    held_item_scale: rig.held_item_scale,
                    ..Default::default()
                },
            )
            .unwrap();
        let bone = draw.presentation.submission.input.current_bones[0];
        (
            extent(&mut runtime, &draw.presentation),
            bone.translation_scale[..3].to_vec(),
        )
    });
    assert_eq!(draws[0], draws[1]);
    assert_eq!(draws[1].1, draws[2].1);
    assert!((draws[2].0 - draws[1].0 * 2.0).abs().max_element() < 1e-5);
}

#[test]
fn equipment_reads_frame_sampled_owner_scale() {
    let world = owner_world(Some(serde_json::json!("1 + query.frame_alpha")));
    let completed = world.actor_rig(2).unwrap();
    assert_eq!(completed.held_item_scale, Some(1.0));
    let bounds = [0.25, 0.75].map(|alpha| {
        let mut frame = world.actor_render_frame(alpha);
        let rig = frame.sample_held_item_scale(completed);
        assert_eq!(rig.held_item_scale, Some(1.0 + alpha));
        let (mut runtime, body, item) = block_fixture();
        let layers = runtime.layers_for(
            &body,
            &ActorEquipmentInput {
                main: Some(item),
                ..Default::default()
            },
            Some(EquipmentAnimation {
                owner: world.actor(2).unwrap(),
                rig: &rig,
                frame_alpha: alpha,
                delta_seconds: 0.016,
            }),
        );
        let draw = layers[0].clone();
        extent(&mut runtime, &draw)
    });
    assert!((bounds[1] / 1.75 - bounds[0] / 1.25).abs().max_element() < 1e-5);
    assert_eq!(
        world.actor_rig(2).unwrap().held_item_scale,
        completed.held_item_scale
    );
}

#[test]
fn authored_main_hand_scale_changes_rendered_block_bounds() {
    let bounds = [1.0, 2.0].map(|scale| {
        let world = owner_world(Some(serde_json::json!(scale)));
        let rig = world.actor_rig(2).unwrap();
        let (mut runtime, body, item) = block_fixture();
        let layers = runtime.layers_for(
            &body,
            &ActorEquipmentInput {
                main: Some(item),
                ..Default::default()
            },
            Some(EquipmentAnimation {
                owner: world.actor(2).unwrap(),
                rig: &rig,
                frame_alpha: 0.5,
                delta_seconds: 0.016,
            }),
        );
        assert_eq!(layers.len(), 1);
        let draw = layers[0].clone();
        extent(&mut runtime, &draw)
    });
    assert!(
        (bounds[1] - bounds[0] * 2.0).abs().max_element() < 1e-5,
        "authored held-item scale must double every rendered extent: {bounds:?}"
    );
}
