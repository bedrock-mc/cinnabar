use super::{
    atlas::{ATLAS_SIDE, TILE_SLOT},
    draw::ParticleView,
    emitter::TileRequest,
    molang::{Rng, V_PARTICLE_RANDOM},
    system::ParticleSystem,
    triggers::block_break_request,
    world::EmptyWorld,
};

// The block_destruct contract from the pinned pack, using original fixture pixels.
const EFFECT: &str = r#"{"particle_effect":{"description":{"identifier":"minecraft:block_destruct",
"basic_render_parameters":{"material":"particles_alpha","texture":"atlas.terrain"}},"components":{
"minecraft:emitter_rate_instant":{"num_particles":"v.emitter_particles_count"},
"minecraft:emitter_lifetime_once":{},
"minecraft:particle_lifetime_expression":{"max_lifetime":"0.2/(math.random(0,1)*0.9+0.1)"},
"minecraft:particle_appearance_billboard":{"size":["v.particle_random_1*0.0375+0.0375","v.particle_random_1*0.0375+0.0375"],
"uv":{"uv":["v.emitter_texture_coordinate.u+v.emitter_texture_size.u/4*v.particle_random_1*3",
"v.emitter_texture_coordinate.v+v.emitter_texture_size.v/4*v.particle_random_2*3"],
"uv_size":["v.emitter_texture_size.u/4","v.emitter_texture_size.v/4"]}},
"minecraft:particle_appearance_lighting":{},
"minecraft:particle_appearance_tinting":{"color":["v.color.r","v.color.g","v.color.b","v.color.a"]}}}}"#;

/// A fixed camera for the real particle draw-list path.
fn view() -> ParticleView {
    ParticleView {
        position: [0.5, 0.5, 3.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    }
}

#[test]
fn destroy_tiles_keep_their_pixels_tint_size_and_quarter_region() {
    for (key, texel, tint) in [
        (1, [125, 125, 125, 255], [1.0; 4]),
        (2, [200, 200, 200, 255], [0.4, 0.7, 0.2, 1.0]),
    ] {
        let mut system = ParticleSystem::default();
        assert!(system.register_effect(EFFECT.as_bytes()));
        let tile = TileRequest {
            key,
            size: TILE_SLOT,
            pixels: texel.repeat((TILE_SLOT * TILE_SLOT) as usize).into(),
        };
        system
            .spawn(&block_break_request(
                "minecraft:block_destruct",
                [0; 3],
                tile,
                tint,
            ))
            .unwrap();
        system.tick(0.001, &EmptyWorld);
        let emitter = &system.emitters_mut()[0];
        let placement = emitter.texture.normalized();
        let mut rng = Rng::new(0);
        let expected: Vec<_> = emitter
            .particles
            .iter()
            .map(|particle| {
                let r1 = particle.vars[V_PARTICLE_RANDOM as usize];
                let r2 = particle.vars[V_PARTICLE_RANDOM as usize + 1];
                let size = emitter.def.particle.billboard.size[0].eval(
                    &mut particle.vars.clone(),
                    &mut rng,
                    &emitter.queries,
                );
                assert!((size - (r1 * 0.0375 + 0.0375)).abs() < 1e-6);
                assert!((0.2..=2.0).contains(&particle.lifetime));
                [
                    placement[0] + placement[2] * r1 * 0.75,
                    placement[1] + placement[3] * r2 * 0.75,
                    placement[2] * 0.25,
                    placement[3] * 0.25,
                ]
            })
            .collect();
        let lists = system.build_draw(&view(), &EmptyWorld);
        assert_eq!(lists.opaque.len(), 100, "native default destroy count");
        for instance in lists.opaque {
            assert!(expected.iter().any(|uv| {
                uv.iter()
                    .zip(instance.uv)
                    .all(|(a, b)| (a - b).abs() < 1e-6)
            }));
            let half_extent = instance.axis_x[..3]
                .iter()
                .map(|x| x * x)
                .sum::<f32>()
                .sqrt();
            assert!((0.0375..=0.075).contains(&half_extent));
            assert_eq!(instance.center_light[3], f32::from(15_u8 << 4));
            assert_eq!(instance.axis_y[3], 1.0);
            assert_eq!(instance.color[3], tint[3]);
            if key == 2 {
                assert!(instance.color[1] > instance.color[0]);
                assert!(instance.color[0] > instance.color[2]);
            }
            let x = ((instance.uv[0] + instance.uv[2] * 0.5) * ATLAS_SIDE as f32) as usize;
            let y = ((instance.uv[1] + instance.uv[3] * 0.5) * ATLAS_SIDE as f32) as usize;
            let offset = (y * ATLAS_SIDE as usize + x) * 4;
            assert_eq!(&system.atlas().pixels()[offset..offset + 4], &texel);
        }
    }
}

#[test]
fn cracks_scatter_on_the_hit_face_and_legacy_terrain_emits_once() {
    use super::triggers::{block_crack_request, terrain_request};
    let tile = TileRequest {
        key: 1,
        size: TILE_SLOT,
        pixels: vec![255; (TILE_SLOT * TILE_SLOT * 4) as usize].into(),
    };
    for face in 0..6 {
        let normal_axis = [1, 1, 2, 2, 0, 0][face as usize];
        let plane = if face % 2 == 0 { -0.1 } else { 1.1 };
        let mut previous = None;
        for seed in 0..8 {
            let mut system = ParticleSystem::default();
            assert!(system.register_effect(EFFECT.as_bytes()));
            let mut request = block_crack_request(
                "minecraft:block_destruct",
                [0; 3],
                face,
                tile.clone(),
                [1.0; 4],
            );
            request.seed = seed;
            system.spawn(&request).unwrap();
            system.tick(0.001, &EmptyWorld);
            let emitter = &system.emitters_mut()[0];
            assert_eq!(emitter.particles.len(), 1);
            let position = emitter.particles[0].pos;
            assert!((position[normal_axis] - plane).abs() < 1e-6);
            for axis in (0..3).filter(|axis| *axis != normal_axis) {
                assert!((0.1..=0.9).contains(&position[axis]));
            }
            assert!(previous.is_none_or(|old| old != position));
            previous = Some(position);
            assert!(request.variables.contains(&("velocity_scalar".into(), 0.7)));
        }
    }
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(EFFECT.as_bytes()));
    system
        .spawn(&terrain_request(
            "minecraft:block_destruct",
            [0; 3],
            tile,
            [1.0; 4],
        ))
        .unwrap();
    system.tick(0.001, &EmptyWorld);
    assert_eq!(system.live_particles(), 1);
    assert_eq!(system.emitters_mut()[0].particles[0].pos, [0.5; 3]);
}

#[test]
fn review_render_live_terrain_particle_keeps_its_tile_during_recycling() {
    let mut system = ParticleSystem::default();
    let long = EFFECT
        .replace("minecraft:block_destruct", "minecraft:long")
        .replace("0.2/(math.random(0,1)*0.9+0.1)", "100");
    let short = EFFECT
        .replace("minecraft:block_destruct", "minecraft:short")
        .replace("0.2/(math.random(0,1)*0.9+0.1)", "0.001");
    assert!(system.register_effect(long.as_bytes()));
    assert!(system.register_effect(short.as_bytes()));
    system
        .spawn(&block_break_request(
            "minecraft:long",
            [0; 3],
            TileRequest {
                key: 1,
                size: 1,
                pixels: vec![7; 4].into(),
            },
            [1.0; 4],
        ))
        .unwrap();
    system.tick(0.001, &EmptyWorld);
    let first = system.emitters_mut()[0].texture;
    for key in 2..=super::atlas::MAX_SLOTS as u64 + 2 {
        system
            .spawn(&block_break_request(
                "minecraft:short",
                [0; 3],
                TileRequest {
                    key,
                    size: 1,
                    pixels: vec![9; 4].into(),
                },
                [1.0; 4],
            ))
            .unwrap();
        system.tick(0.002, &EmptyWorld);
    }
    let offset = ((first.y * ATLAS_SIDE + first.x) * 4) as usize;
    assert_eq!(&system.atlas().pixels()[offset..offset + 4], &[7; 4]);
    assert!(system.live_particles() > 0);
}
