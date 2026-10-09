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

fn stream() -> WorldStream {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let compiled = pack_compiler::compile_actor_pack(vec![
        ("entity/test.json".into(), ENTITY.as_bytes().to_vec()),
        ("animation_controllers/test.json".into(), CONTROLLER.as_bytes().to_vec()),
        ("animations/test.json".into(), br#"{"format_version":"1.8.0","animations":{"animation.test.idle":{"loop":true,"bones":{"root":{"rotation":[0,0,0]}}}}}"#.to_vec()),
        ("models/entity/test.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.particle_actor","texture_width":16,"texture_height":16},"bones":[{"name":"root","pivot":[0,0,0],"cubes":[{"origin":[-4,0,-4],"size":[8,16,8],"uv":[0,0]}]}]}]}"#.to_vec()),
        ("render_controllers/test.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test.particle":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#.to_vec()),
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
    queue_actor_particles(&stream, &mut system, &mut queue);
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
    queue_actor_particles(&stream, &mut system, &mut queue);
    assert!(queue.is_empty(), "holding a state starts no extra emitter");
    variant(&mut stream, 2, 1);
    queue_actor_particles(&stream, &mut system, &mut queue);
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
    queue_actor_particles(&stream, &mut system, &mut queue);
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
