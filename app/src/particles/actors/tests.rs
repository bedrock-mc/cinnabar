use super::*;
use particles::{EmptyWorld, ParticleView};
use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue,
    ActorSpawnEvent, WorldEvent,
};
use std::sync::Arc;

const ENTITY: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:particle_actor","materials":{"default":"entity_alphatest"},
 "textures":{"default":"textures/entity/particle_actor"},"geometry":{"default":"geometry.particle_actor"},
 "animations":{"idle":"animation.test.idle","particle":"controller.animation.test.particle"},
 "particle_effects":{"idle":"test:orange"},"scripts":{"animate":["particle"]},
 "render_controllers":["controller.render.test.particle"]}}}"#;
const CONTROLLER: &str = r#"{"format_version":"1.10.0","animation_controllers":{
 "controller.animation.test.particle":{"states":{
 "default":{"animations":["idle"],"particle_effects":[{"effect":"idle"}],"transitions":[{"off":"query.variant != 0"}]},
 "off":{"transitions":[{"default":"query.variant == 0"}]}}}}}"#;
const EFFECT: &str = r##"{"particle_effect":{"description":{"identifier":"test:orange",
 "basic_render_parameters":{"material":"particles_alpha","texture":"textures/particle/test"}},
 "components":{"minecraft:emitter_rate_steady":{"spawn_rate":10,"max_particles":30},
 "minecraft:emitter_lifetime_expression":{"activation_expression":1},
 "minecraft:particle_lifetime_expression":{"max_lifetime":2},
 "minecraft:particle_appearance_billboard":{"size":[0.1,0.1]},
 "minecraft:particle_appearance_tinting":{"color":"#FFFF9200"}}}}"##;

/// Creates the authored actor and its default controller fixture.
fn stream() -> WorldStream {
    stream_with_controller(CONTROLLER)
}

/// Compiles an authored actor with an alternate controller and a fixed root locator.
fn stream_with_controller(controller: &str) -> WorldStream {
    stream_with_animation(controller, "0")
}

/// Builds the actor fixture with a root rotation expression sampled between ticks.
fn stream_with_animation(controller: &str, rotation: &str) -> WorldStream {
    stream_with_definition(ENTITY, controller, rotation)
}

/// Compiles an actor definition with its controllers and frame-dependent root rotation.
fn stream_with_definition(entity: &str, controller: &str, rotation: &str) -> WorldStream {
    let animation = format!(
        r#"{{"format_version":"1.8.0","animations":{{"animation.test.idle":{{"loop":true,"bones":{{"root":{{"rotation":[0,0,"{rotation}"]}}}}}}}}}}"#
    );
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let compiled = pack_entity_compiler::compile_actor_pack(vec![
        ("entity/test.json".into(), entity.as_bytes().to_vec()),
        ("animation_controllers/test.json".into(), controller.as_bytes().to_vec()),
        ("animations/test.json".into(), animation.into_bytes()),
        ("models/entity/test.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.particle_actor","texture_width":16,"texture_height":16},"bones":[{"name":"root","pivot":[0,0,0],"locators":{"tip":[16,16,0]},"cubes":[{"origin":[-4,0,-4],"size":[8,16,8],"uv":[0,0]}]}]}]}"#.to_vec()),
        ("render_controllers/test.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test.particle":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"],"overlay_color":{"a":"query.frame_alpha"}}}}"#.to_vec()),
        ("textures/entity/particle_actor.png".into(), png),
    ]).unwrap().unwrap();
    let candidates = compiled
        .bindings
        .iter()
        .map(|binding| binding.geometry_candidate)
        .collect();
    let assets = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
    let mut stream = WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(&assets),
        [0.0; 3],
        None,
    );
    stream.set_pack_entities(Some((assets, candidates)));
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: -42,
                runtime_id: 42,
                kind: ActorKind::Entity {
                    identifier: "test:particle_actor".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([ActorMetadata {
                    key: 2,
                    value: ActorMetadataValue::Int(0),
                }]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(1);
    stream
}

/// Publishes a variant update and completes its animation evaluation.
fn variant(stream: &mut WorldStream, sequence: u64, value: i32) {
    stream
        .submit(
            sequence,
            WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                tick: sequence,
                metadata: Arc::from([ActorMetadata {
                    key: 2,
                    value: ActorMetadataValue::Int(value),
                }]),
                properties: Arc::from([]),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(1);
}

#[test]
fn authored_controller_particles_reach_draws_once_and_stop_on_state_exit() {
    let mut stream = stream();
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(EFFECT.as_bytes()));
    system
        .actor_bindings
        .insert_entity(&serde_json::from_str(ENTITY).unwrap());
    system
        .actor_bindings
        .insert_controllers(&serde_json::from_str(CONTROLLER).unwrap());
    let mut queue = Vec::new();
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.0),
        &mut system,
        &mut queue,
    );
    route_actor_particles(&mut system, &mut queue);
    assert_eq!(system.emitter_count(), 1);
    system.tick(0.2, &EmptyWorld);
    let view = ParticleView {
        position: [0.0, 0.0, 4.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    let draws = system.build_draw(&view, &EmptyWorld);
    assert!(!draws.opaque.is_empty());
    assert_eq!(draws.opaque[0].color, [1.0, 146.0 / 255.0, 0.0, 1.0]);
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.0),
        &mut system,
        &mut queue,
    );
    assert!(queue.is_empty(), "holding a state starts no extra emitter");
    variant(&mut stream, 2, 1);
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.0),
        &mut system,
        &mut queue,
    );
    assert!(matches!(queue.as_slice(), [ActorParticleCommand::Stop(_)]));
    route_actor_particles(&mut system, &mut queue);
    let count = system.live_particles();
    system.tick(0.2, &EmptyWorld);
    assert_eq!(
        system.live_particles(),
        count,
        "state exit stops new emission"
    );
    variant(&mut stream, 3, 0);
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.0),
        &mut system,
        &mut queue,
    );
    assert!(matches!(
        queue.as_slice(),
        [ActorParticleCommand::Start(_, _)]
    ));
    route_actor_particles(&mut system, &mut queue);
    assert_eq!(
        system.emitter_count(),
        2,
        "reentry starts a new emitter while prior particles expire"
    );
}

#[test]
fn controller_annotations_spawn_at_the_sampled_locator_and_initialize_billboard_size() {
    let controller = CONTROLLER.replace(
        r#"{"effect":"idle"}"#,
        r#"{"effect":"idle","locator":"tip","pre_effect_script":"v.size = query.variant + 2;"}"#,
    );
    let controller = controller.replace("query.variant != 0", "query.variant < 0");
    let mut stream = stream_with_controller(&controller);
    variant(&mut stream, 2, 4);
    let mut system = ParticleSystem::default();
    let effect = EFFECT.replace("[0.1,0.1]", r#"["v.size",0.1]"#);
    assert!(system.register_effect(effect.as_bytes()));
    system
        .actor_bindings
        .insert_entity(&serde_json::from_str(ENTITY).unwrap());
    system
        .actor_bindings
        .insert_controllers(&serde_json::from_str(&controller).unwrap());
    let mut queue = Vec::new();
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.5),
        &mut system,
        &mut queue,
    );
    let [ActorParticleCommand::Start(_, request)] = queue.as_slice() else {
        panic!("annotated controller effect must be admitted");
    };
    assert_eq!(request.position, [1.0, 1.0, 0.0]);
    assert!(request.initialization.is_some());
    assert!(request.actor_attachment.is_some());
    route_actor_particles(&mut system, &mut queue);
    system.tick(0.2, &EmptyWorld);
    let view = ParticleView {
        position: [0.0, 0.0, 4.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    let draws = system.build_draw(&view, &EmptyWorld);
    assert!(!draws.opaque.is_empty());
    assert_eq!(&draws.opaque[0].center_light[..3], &[1.0, 1.0, 0.0]);
    let width = draws.opaque[0].axis_x[..3]
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    assert!(
        (width - 6.0).abs() < 1e-5,
        "bound initialization must read the actor variant"
    );
}

#[test]
fn unbound_controller_initialization_uses_the_locator_without_actor_query_context() {
    let controller = CONTROLLER.replace(
        r#"{"effect":"idle"}"#,
        r#"{"effect":"idle","locator":"tip","bind_to_actor":false,"pre_effect_script":"v.size = query.variant + 2;"}"#,
    ).replace("query.variant != 0", "query.variant < 0");
    let mut stream = stream_with_controller(&controller);
    variant(&mut stream, 2, 4);
    let mut system = ParticleSystem::default();
    let effect = EFFECT.replace("[0.1,0.1]", r#"["v.size",0.1]"#);
    assert!(system.register_effect(effect.as_bytes()));
    system
        .actor_bindings
        .insert_entity(&serde_json::from_str(ENTITY).unwrap());
    system
        .actor_bindings
        .insert_controllers(&serde_json::from_str(&controller).unwrap());
    let mut queue = Vec::new();
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.5),
        &mut system,
        &mut queue,
    );
    let [ActorParticleCommand::Start(_, request)] = queue.as_slice() else {
        panic!("unbound annotated effect must be admitted");
    };
    assert_eq!(request.position, [1.0, 1.0, 0.0]);
    assert!(request.basis.is_none());
    assert!(request.actor_attachment.is_none());
    assert!(request.queries.actor.is_none());
    route_actor_particles(&mut system, &mut queue);
    system.tick(0.2, &EmptyWorld);
    let view = ParticleView {
        position: [0.0, 0.0, 4.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    let draws = system.build_draw(&view, &EmptyWorld);
    assert!(!draws.opaque.is_empty());
    let width = draws.opaque[0].axis_x[..3]
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    assert!(
        (width - 2.0).abs() < 1e-5,
        "unbound initialization must have no actor variant"
    );
}

/// Publishes metadata scale while retaining the actor and its controller state.
fn scale(stream: &mut WorldStream, sequence: u64, value: f32) {
    stream
        .submit(
            sequence,
            WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                tick: sequence,
                metadata: Arc::from([ActorMetadata {
                    key: client_world::SCALE_METADATA_KEY,
                    value: ActorMetadataValue::Float(value),
                }]),
                properties: Arc::from([]),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(1);
}

/// Registers a steady effect and its controller references.
fn particle_system(controller: &str) -> ParticleSystem {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(EFFECT.as_bytes()));
    system
        .actor_bindings
        .insert_entity(&serde_json::from_str(ENTITY).unwrap());
    system
        .actor_bindings
        .insert_controllers(&serde_json::from_str(controller).unwrap());
    system
}

#[test]
fn zero_scale_entry_retries_placement_without_reentering_the_controller() {
    let mut stream = stream();
    scale(&mut stream, 2, 0.0);
    let mut system = particle_system(CONTROLLER);
    let mut queue = Vec::new();
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.5),
        &mut system,
        &mut queue,
    );
    assert!(queue.is_empty());
    scale(&mut stream, 3, 1.0);
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.5),
        &mut system,
        &mut queue,
    );
    assert!(matches!(
        queue.as_slice(),
        [ActorParticleCommand::Start(_, _)]
    ));
    route_actor_particles(&mut system, &mut queue);
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.5),
        &mut system,
        &mut queue,
    );
    assert!(
        queue.is_empty(),
        "restoring scale starts the occurrence only once"
    );
}

#[test]
fn bound_emission_survives_zero_scale_and_follows_the_restored_locator() {
    let controller = CONTROLLER.replace(
        r#"{"effect":"idle"}"#,
        r#"{"effect":"idle","locator":"tip"}"#,
    );
    let mut stream = stream_with_controller(&controller);
    let mut system = particle_system(&controller);
    let mut queue = Vec::new();
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.5),
        &mut system,
        &mut queue,
    );
    route_actor_particles(&mut system, &mut queue);
    system.tick(0.2, &EmptyWorld);
    scale(&mut stream, 2, 0.0);
    follow_actor_particles(&mut system, &mut ActorParticleFrame::new(&stream, 0.5));
    scale(&mut stream, 3, 2.0);
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.5),
        &mut system,
        &mut queue,
    );
    assert!(
        queue.is_empty(),
        "scale changes retain the original occurrence"
    );
    follow_actor_particles(&mut system, &mut ActorParticleFrame::new(&stream, 0.5));
    let count = system.live_particles();
    system.tick(0.2, &EmptyWorld);
    assert!(
        system.live_particles() > count,
        "restored attachments continue emitting"
    );
    let view = ParticleView {
        position: [0.0, 0.0, 4.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    assert!(
        system
            .build_draw(&view, &EmptyWorld)
            .opaque
            .iter()
            .any(|draw| draw.center_light[..3] == [2.0, 2.0, 0.0])
    );
}

#[test]
fn controllers_without_particle_work_do_not_allocate_sampled_poses() {
    let stream = stream_with_animation(CONTROLLER, "query.frame_alpha * 90");
    let sampled = stream
        .authority()
        .actor_render_frame(0.5)
        .layers(42)
        .unwrap();
    assert_eq!(
        sampled[0].overlay[3], 0.5,
        "the fixture evaluates a frame query"
    );
    let mut system = ParticleSystem::default();
    let mut queue = Vec::new();
    let ((), allocations) = test_alloc::count(|| {
        queue_actor_particles(
            &mut ActorParticleFrame::new(&stream, 0.5),
            &mut system,
            &mut queue,
        );
    });
    assert!(queue.is_empty());
    assert_eq!(
        allocations, 0,
        "controllers without effects must skip pose sampling"
    );
    system = particle_system(CONTROLLER);
    queue_actor_particles(
        &mut ActorParticleFrame::new(&stream, 0.5),
        &mut system,
        &mut queue,
    );
    route_actor_particles(&mut system, &mut queue);
    let ((), allocations) = test_alloc::count(|| {
        queue_actor_particles(
            &mut ActorParticleFrame::new(&stream, 0.5),
            &mut system,
            &mut queue,
        );
    });
    assert!(queue.is_empty());
    assert_eq!(allocations, 0, "admitted effects must skip pose sampling");
}

#[test]
fn multiple_controllers_and_attachments_reuse_one_actor_pose_sample() {
    let mut entity: serde_json::Value = serde_json::from_str(ENTITY).unwrap();
    let description = &mut entity["minecraft:client_entity"]["description"];
    description["animations"]["second"] = "controller.animation.test.second".into();
    description["scripts"]["animate"] = serde_json::json!(["particle", "second"]);
    let mut controllers: serde_json::Value = serde_json::from_str(CONTROLLER).unwrap();
    controllers["animation_controllers"]["controller.animation.test.second"] =
        controllers["animation_controllers"]["controller.animation.test.particle"].clone();
    let controllers = serde_json::to_string(&controllers).unwrap();
    let stream =
        stream_with_definition(&entity.to_string(), &controllers, "query.frame_alpha * 90");
    let mut system = particle_system(&controllers);
    let mut queue = Vec::new();
    let mut frame = ActorParticleFrame::new(&stream, 0.5);
    queue_actor_particles(&mut frame, &mut system, &mut queue);
    assert_eq!(
        queue.len(),
        2,
        "both controllers start their own occurrence"
    );
    route_actor_particles(&mut system, &mut queue);
    let ((), allocations) = test_alloc::count(|| {
        follow_actor_particles(&mut system, &mut frame);
    });
    assert_eq!(
        allocations, 0,
        "following must reuse the spawning pose sample"
    );
    system.tick(0.2, &EmptyWorld);
    assert!(system.live_particles() >= 2);
    let mut frame = ActorParticleFrame::new(&stream, 0.75);
    queue_actor_particles(&mut frame, &mut system, &mut queue);
    assert!(queue.is_empty());
    assert!(
        frame.samples.is_empty(),
        "admitted occurrences require no sampling"
    );
    follow_actor_particles(&mut system, &mut frame);
    assert_eq!(
        frame.samples.len(),
        1,
        "attachments share their actor's sample"
    );
}

#[test]
fn bound_spell_particles_use_actor_color_at_spawn_and_after_metadata_updates() {
    let mut stream = stream();
    let effect = EFFECT.replace(
        r##""#FFFF9200""##,
        r#"["query.spellcolor.r", "query.spellcolor.g", "query.spellcolor.b", "query.spellcolor.a"]"#,
    );
    let mut system = particle_system(CONTROLLER);
    assert!(system.register_effect(effect.as_bytes()));
    let mut queue = Vec::new();
    let view = ParticleView {
        position: [0.0, 0.0, 4.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    for (sequence, color, expected) in [
        (2, 0x3399cc, [0.2, 0.6, 0.8, 1.0]),
        (3, 0xcc6633, [0.8, 0.4, 0.2, 1.0]),
    ] {
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 42,
                    tick: sequence,
                    metadata: Arc::from([ActorMetadata {
                        key: client_world::SPELL_COLOR_METADATA_KEY,
                        value: ActorMetadataValue::Int(color),
                    }]),
                    properties: Arc::from([]),
                })),
            )
            .unwrap();
        stream.advance_actor_interpolation_ticks(1);
        let mut frame = ActorParticleFrame::new(&stream, 0.5);
        queue_actor_particles(&mut frame, &mut system, &mut queue);
        if sequence == 2 {
            let [ActorParticleCommand::Start(_, request)] = queue.as_slice() else {
                panic!("bound spell effect must start");
            };
            assert_eq!(request.queries.spell_color, expected);
            route_actor_particles(&mut system, &mut queue);
        } else {
            assert!(queue.is_empty(), "metadata refresh retains the emitter");
            follow_actor_particles(&mut system, &mut frame);
        }
        assert_eq!(system.emitter_count(), 1);
        system.tick(0.2, &EmptyWorld);
        let draws = system.build_draw(&view, &EmptyWorld);
        assert!(!draws.opaque.is_empty());
        assert!(draws.opaque.iter().all(|draw| draw.color == expected));
    }
}
