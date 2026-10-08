use super::*;
use assets::{IconEntry, IconSprite, encode_icon_catalog};
use particles::EmptyWorld;

fn icons() -> RuntimeIconCatalog {
    let bytes = encode_icon_catalog(
        [0; 32],
        &[IconSprite {
            width: 1,
            height: 1,
            rgba8: vec![200, 230, 255, 255].into(),
        }],
        &[IconEntry {
            identifier: "minecraft:snowball".into(),
            metadata: 0,
            sprite: 0,
        }],
    )
    .unwrap();
    RuntimeIconCatalog::decode(&bytes).unwrap()
}

fn system() -> ParticleSystem {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(
        br#"{"particle_effect":{"description":{"identifier":"minecraft:breaking_item_icon",
        "basic_render_parameters":{"material":"particles_alpha","texture":"atlas.items"}},
        "components":{"minecraft:emitter_lifetime_once":{},
        "minecraft:emitter_rate_instant":{"num_particles":"v.num_particles"},
        "minecraft:particle_lifetime_expression":{"max_lifetime":1},
        "minecraft:particle_appearance_billboard":{"size":[0.05,0.05]}}}}"#
    ));
    system
}

#[test]
fn snowball_packets_each_emit_one_fragment_through_the_app_route() {
    let stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let registry = sim::CollisionRegistry::default();
    let world = StreamParticleWorld::new(&stream, &registry);
    let icons = icons();
    let routing = Routing {
        world: &world,
        stream: &stream,
        mode: NetworkIdMode::Sequential,
        icons: Some(&icons),
    };
    let mut system = system();
    for _ in 0..ITEM_ICON_PARTICLES {
        route_level_event(&mut system, &routing, 2009, [1.0, 2.0, 3.0], 15);
    }
    system.tick(0.001, &EmptyWorld);
    assert_eq!(system.live_particles(), ITEM_ICON_PARTICLES as usize);
}
