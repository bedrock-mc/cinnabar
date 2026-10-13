use super::*;

// Java crouching raised the first-person camera off the eye, which read as third person.
#[test]
fn first_person_bites_follow_a_raised_camera() {
    let stream = super::totem_tests::stream();
    let eye = bevy::math::Vec3::new(0.0, protocol::PLAYER_NETWORK_OFFSET, 0.0);
    let camera = Transform::from_translation(eye + bevy::math::Vec3::Y * 0.27);
    let first = local_bite_position(&stream, &camera, true).unwrap();
    let expected = camera.translation + camera.forward() * FIRST_PERSON_BITE_AHEAD
        - camera.up() * FIRST_PERSON_BITE_BELOW;
    assert!(first.distance(expected) < 1e-5, "{first:?}");
    let mouth = stream.authority().actor(1).unwrap().eating_position();
    assert_eq!(
        local_bite_position(&stream, &camera, false),
        Some(bevy::math::Vec3::from_array(mouth))
    );
}

#[test]
fn feed_notices_scatter_only_admitted_food_and_do_not_duplicate_local_bites() {
    use assets::item_use::ItemConsumption;
    use assets::{IconEntry, IconSprite, encode_icon_catalog};
    use particles::EmptyWorld;

    let identifier = "minecraft:bread";
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap()
        .network_id;
    let icons = RuntimeIconCatalog::decode(
        &encode_icon_catalog(
            [0; 32],
            &[IconSprite {
                width: 1,
                height: 1,
                rgba8: vec![200, 180, 100, 255].into(),
            }],
            &[IconEntry {
                identifier: identifier.into(),
                metadata: 0,
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    for consumption in [ItemConsumption::Eat, ItemConsumption::Drink] {
        let mut stream = super::totem_tests::stream();
        stream.set_item_use_overlays(Arc::new(std::collections::BTreeMap::from([(
            identifier.into(),
            assets::equipment::CompiledItemUseOverlay {
                identifier: identifier.into(),
                consumption: Some(consumption),
                ..Default::default()
            },
        )])));
        let registry = sim::CollisionRegistry::default();
        let world = StreamParticleWorld::new(&stream, &registry, None);
        let routing = Routing {
            world: &world,
            stream: &stream,
            mode: NetworkIdMode::Sequential,
            icons: Some(&icons),
        };
        for runtime_id in [stream.local_player_runtime_id(), 42] {
            let mut system = ParticleSystem::default();
            assert!(system.register_effect(
                br#"{"particle_effect":{"description":{"identifier":"minecraft:breaking_item_icon",
                "basic_render_parameters":{"material":"particles_alpha","texture":"atlas.items"}},
                "components":{"minecraft:emitter_lifetime_once":{},
                "minecraft:emitter_rate_instant":{"num_particles":"v.num_particles"},
                "minecraft:particle_lifetime_expression":{"max_lifetime":1},
                "minecraft:particle_appearance_billboard":{"size":[0.05,0.05]}}}}"#
            ));
            route_notice(
                &mut system,
                &routing,
                &ActorStatusNotice {
                    runtime_id,
                    kind: ActorStatusKind::Feed,
                    data: network_id << 16,
                    position: [0.0; 3],
                    height: None,
                    eating_position: [0.0; 3],
                },
            );
            system.tick(0.001, &EmptyWorld);
            assert_eq!(
                system.live_particles() > 0,
                consumption == ItemConsumption::Eat
                    && runtime_id != stream.local_player_runtime_id(),
                "{consumption:?} for actor {runtime_id}"
            );
        }
    }
}
