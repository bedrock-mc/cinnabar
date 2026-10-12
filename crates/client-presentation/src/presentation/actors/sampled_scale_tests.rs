use super::*;
use bevy::math::{Mat4, Vec3};
use image::ImageEncoder;

use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, WorldBootstrap, WorldEvent,
};

/// A compiled creeper whose scale crosses an admission boundary within one tick.
fn world(scale: &str, axis: bool, position: [f32; 3]) -> client_world::WorldAuthority {
    world_with_frame_layer(scale, axis, position, false)
}

/// Builds the same actor with an optional depth layer selected only between completed ticks.
fn world_with_frame_layer(
    scale: &str,
    axis: bool,
    position: [f32; 3],
    frame_layer: bool,
) -> client_world::WorldAuthority {
    world_with_geometry_choice(scale, axis, position, frame_layer, None)
}

/// Adds an optional controller-selected mesh extending beyond the base visibility bounds.
fn world_with_geometry_choice(
    scale: &str,
    axis: bool,
    position: [f32; 3],
    frame_layer: bool,
    geometry_choice: Option<&str>,
) -> client_world::WorldAuthority {
    let slot = if axis { "scaleX" } else { "scale" };
    let entity = format!(
        r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"minecraft:creeper","materials":{{"default":"entity"}},"textures":{{"default":"textures/entity/test"}},"geometry":{{"default":"geometry.test"}},"animations":{{"swell":"animation.test.swell"}},"scripts":{{"{slot}":"{scale}","animate":["swell"]}},"render_controllers":["controller.render.test"]}}}}}}"#
    );
    let mut entity: serde_json::Value = serde_json::from_str(&entity).unwrap();
    if frame_layer {
        let description = &mut entity["minecraft:client_entity"]["description"];
        description["materials"]["default"] = "test_depth_marker".into();
        description["render_controllers"] =
            serde_json::json!([{"controller.render.test": "query.frame_alpha > 0.25"}]);
    }
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":64,"texture_height":64,"visible_bounds_width":2,"visible_bounds_height":2,"visible_bounds_offset":[0,1,0]},"bones":[{"name":"body","pivot":[0,0,0],"cubes":[{"origin":[-4,0,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#;
    let animation = br#"{"format_version":"1.8.0","animations":{"animation.test.swell":{"loop":true,"bones":{"body":{"scale":["1 + query.swell_amount",1,1]}}}}}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let mut geometry: serde_json::Value = serde_json::from_slice(geometry).unwrap();
    let mut controller: serde_json::Value = serde_json::from_slice(controller).unwrap();
    if let Some(choice) = geometry_choice {
        entity["minecraft:client_entity"]["description"]["geometry"]["wide"] =
            "geometry.wide".into();
        let mut wide = geometry["minecraft:geometry"][0].clone();
        wide["description"]["identifier"] = "geometry.wide".into();
        wide["description"]["visible_bounds_width"] = 20.into();
        wide["bones"][0]["cubes"][0]["origin"] = serde_json::json!([-80, 0, -4]);
        wide["bones"][0]["cubes"][0]["size"] = serde_json::json!([160, 8, 8]);
        geometry["minecraft:geometry"]
            .as_array_mut()
            .unwrap()
            .push(wide);
        controller["render_controllers"]["controller.render.test"]["geometry"] = choice.into();
    }
    let mut texture = Vec::new();
    image::codecs::png::PngEncoder::new(&mut texture)
        .write_image(&[255; 4], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let mut resources = vec![
        (
            "entity/creeper.json".into(),
            serde_json::to_vec(&entity).unwrap(),
        ),
        (
            "models/entity/test.geo.json".into(),
            serde_json::to_vec(&geometry).unwrap(),
        ),
        ("animations/test.animation.json".into(), animation.to_vec()),
        (
            "render_controllers/test.json".into(),
            serde_json::to_vec(&controller).unwrap(),
        ),
        ("textures/entity/test.png".into(), texture),
    ];
    if frame_layer {
        resources.push((
            "materials/depth.material".into(),
            br#"{"materials":{"version":"1.0.0","test_depth_marker:entity":{"depthFunc":"Always"}}}"#.to_vec(),
        ));
    }
    let compiled = pack_compiler::compile_entity_pack(resources)
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

#[test]
fn scale_independent_distance_gate_retains_edges_and_rejects_far_actors() {
    let radius = render_model::ACTOR_CANDIDATE_RADIUS_BLOCKS;
    let view = ActorCullView {
        camera_position: Vec3::Y,
        clip_from_world: Mat4::IDENTITY,
        max_distance: 100.0,
    };
    for (feet, maximum, expected) in [
        ([radius, 0.0, 0.0], radius + 10.0, true),
        ([radius + 1.0, 0.0, 0.0], radius + 10.0, false),
        ([0.0, 0.0, -10.0], 10.0, true),
        ([0.0, 0.0, -11.0], 10.0, false),
        ([7.5, 0.0, -5.0], 100.0, true),
    ] {
        let world = world("1000 + query.swell_amount", false, feet);
        let actor = world.actor(1).unwrap();
        let sampled = std::cell::Cell::new(false);
        let candidate = sample_candidate_scale(
            world.actor_rig(1).unwrap(),
            actor,
            0.5,
            Some(ActorCullView {
                max_distance: maximum,
                ..view
            }),
            |rig| {
                sampled.set(true);
                rig
            },
        );
        assert_eq!(
            sampled.get(),
            expected,
            "far actors must consume no scale-sampling work"
        );
        assert_eq!(candidate.is_some(), expected);
        assert_eq!(
            actor_within_render_distance(
                actor,
                0.5,
                Some(ActorCullView {
                    max_distance: maximum,
                    ..view
                })
            ),
            expected,
            "distance admission is independent of even a large authored scale: {feet:?}"
        );
        assert!(actor_within_render_distance(actor, 0.5, None));
    }
}

#[test]
fn always_depth_materials_bypass_terrain_occlusion_but_keep_view_culling() {
    let camera = Vec3::new(0.0, 1.0, 0.0);
    let view = ActorCullView {
        camera_position: camera,
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            90_f32.to_radians(),
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_to_mat4(camera, -Vec3::Z, Vec3::Y),
        max_distance: 100.0,
    };
    for (feet, maximum, visible) in [
        ([0.0, 0.0, -5.0], 100.0, true),
        ([30.0, 0.0, -5.0], 100.0, false),
        ([0.0, 0.0, -5.0], 4.0, false),
    ] {
        let world = world("1", false, feet);
        let actor = world.actor(1).unwrap();
        let rig = world.actor_rig(1).unwrap();
        let view = Some(ActorCullView {
            max_distance: maximum,
            ..view
        });
        assert!(!rig_may_be_visible(&rig, actor, 0.5, view, None, |_, _| {
            true
        }));
        let mut layers = rig.render.to_vec();
        assert!(!layers.is_empty(), "the fixture selects a material layer");
        layers[0].material_state = Some(assets::EntityRenderMaterialState {
            depth_always: true,
            ..Default::default()
        });
        let always = ActorRigSnapshot {
            render: &layers,
            ..rig
        };
        assert_eq!(
            rig_may_be_visible(&always, actor, 0.5, view, None, |_, _| true),
            visible,
            "through-wall materials retain distance and frustum checks: {feet:?}, {maximum}"
        );
    }
}

#[test]
fn frame_selected_always_depth_layers_survive_terrain_occlusion() {
    let world = world_with_frame_layer("1", false, [0.0, 0.0, -5.0], true);
    let actor = world.actor(1).unwrap();
    let rig = world.actor_rig(1).unwrap();
    assert!(
        rig.render.is_empty(),
        "the layer is inactive at the completed tick"
    );
    let camera = Vec3::new(0.0, 1.0, 0.0);
    let view = Some(ActorCullView {
        camera_position: camera,
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            90_f32.to_radians(),
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_to_mat4(camera, -Vec3::Z, Vec3::Y),
        max_distance: 100.0,
    });
    let through_wall = |layers: &[actor_animation::RenderTextureLayer]| {
        layers
            .iter()
            .any(|layer| layer.material_state.is_some_and(|state| state.depth_always))
    };
    let mut probe = world.actor_render_frame(0.5);
    assert!(
        through_wall(&probe.layers(1).unwrap()),
        "the frame selects the depth layer"
    );
    let mut frame = world.actor_render_frame(0.5);
    assert!(rig_may_be_visible(
        &rig,
        actor,
        0.5,
        view,
        Some(&mut frame),
        |_, _| true
    ));
    assert!(
        through_wall(&frame.layers(1).unwrap()),
        "the draw uses the admitted frame's layers"
    );
}

/// Exercises scale sampling, both culling stages and body construction on compiled scripts.
fn assert_sampled_admission(cull_completed_tick: bool) {
    let camera = Vec3::new(0.0, 1.0, 0.0);
    let view = ActorCullView {
        camera_position: camera,
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            90_f32.to_radians(),
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_to_mat4(camera, -Vec3::Z, Vec3::Y),
        max_distance: 100.0,
    };
    let (artwork, locations) =
        ActorArtworkPages::default().with_equipment_rasters(&[render_model::EquipmentRaster {
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
            let view = actor_animation::ActorAnimationView {
                planes: [[0.0, 0.0, 0.0, -1.0]; 6],
                camera: [0.0; 3],
                player_distance: 100.0,
                entity_radius: 100.0,
            };
            world.set_actor_animation_view(Some(view));
            world.advance_actor_interpolation_ticks(1);
            world.set_actor_animation_view(Some(actor_animation::ActorAnimationView {
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
                None,
                |_, _| false
            ));
        }
        let mut frame = world.actor_render_frame(0.5);
        let sampled = frame.sample_rig_scale(tick);
        assert!(
            rig_may_be_visible(&sampled, actor, 0.5, Some(view), None, |_, _| false),
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

/// Render occlusion never owns the clock that advances retained actor state.
#[test]
fn terrain_hidden_actors_keep_advancing_completed_ticks() {
    let mut world = world("1", false, [0.0, 0.0, -10.0]);
    let view = ActorCullView {
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            1.0, 1.0, 0.1,
        ),
        camera_position: Vec3::ZERO,
        max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    };
    let before = world.actor_rig(1).unwrap().completed_tick;
    for tick in 1..=4 {
        world.advance_actor_interpolation_ticks(1);
        let rig = world.actor_rig(1).unwrap();
        let actor = world.actor(1).unwrap();
        assert!(!rig_may_be_visible(
            &rig,
            actor,
            0.5,
            Some(view),
            None,
            |_, _| true
        ));
        assert_eq!(rig.completed_tick, before + tick);
    }
    let rig = world.actor_rig(1).unwrap();
    assert!(rig_may_be_visible(
        &rig,
        world.actor(1).unwrap(),
        0.5,
        Some(view),
        None,
        |_, _| false
    ));
}

#[test]
fn single_layer_replacements_survive_walls_covering_only_the_base_mesh() {
    let feet = [0.0, 0.0, -10.0];
    let camera = Vec3::Y;
    let view = Some(ActorCullView {
        camera_position: camera,
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            1.0, 1.0, 0.1,
        ) * glam::camera::rh::view::look_to_mat4(camera, -Vec3::Z, Vec3::Y),
        max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    });
    let hidden = |low, high| {
        meshing::occlusion::OcclusionBudget::default().hides_box(
            camera.to_array(),
            low,
            high,
            |cell, _| cell[2] == -5 && (-2..=1).contains(&cell[0]),
        )
    };
    for choice in [
        "Geometry.wide",
        "query.frame_alpha > 0.25 ? Geometry.wide : Geometry.default",
    ] {
        let world = world_with_geometry_choice("1", false, feet, false, Some(choice));
        let rig = world.actor_rig(1).unwrap();
        assert_eq!(rig.render.len(), 1);
        let (low, high) = rig.culling_bounds().at(feet, 1.0);
        assert!(hidden(low, high), "the wall covers the base mesh");
        if choice.starts_with("query.") {
            assert!(rig.render[0].geometry.is_none());
        }
        let mut frame = world.actor_render_frame(0.5);
        assert!(frame.may_replace_body_geometry(1));
        let mut sampled = world.actor_render_frame(0.5);
        let layers = sampled.layers(1).unwrap();
        assert_eq!(layers.len(), 1);
        let geometry = layers[0].geometry.unwrap();
        let (assets, _) = rig.geometry_source().unwrap();
        let (low, high) = assets
            .geometry_visible_bounds(geometry as usize)
            .unwrap()
            .at(feet, 1.0);
        assert!(!hidden(low, high), "the replacement extends past the wall");
        assert!(rig_may_be_visible(
            &rig,
            world.actor(1).unwrap(),
            0.5,
            view,
            Some(&mut frame),
            hidden,
        ));
    }
    let world = world("1", false, feet);
    let rig = world.actor_rig(1).unwrap();
    let mut frame = world.actor_render_frame(0.5);
    assert!(!frame.may_replace_body_geometry(1));
    assert!(!rig_may_be_visible(
        &rig,
        world.actor(1).unwrap(),
        0.5,
        view,
        Some(&mut frame),
        hidden,
    ));
}
