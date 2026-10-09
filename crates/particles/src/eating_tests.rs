use crate::{EmptyWorld, ParticleSystem, ParticleView, TileRequest, eating_item_request};

#[test]
fn eating_draw_keeps_native_quad_extent_and_quarter_sprite() {
    use crate::{atlas::ATLAS_SIDE, molang::V_PARTICLE_RANDOM};

    let mut system = ParticleSystem::default();
    assert!(system.register_effect(
        br#"{"particle_effect":{"description":{
        "identifier":"minecraft:breaking_item_icon","basic_render_parameters":{
        "material":"particles_alpha","texture":"atlas.items"}},"components":{
        "minecraft:emitter_lifetime_once":{},
        "minecraft:emitter_rate_instant":{"num_particles":"v.num_particles"},
        "minecraft:particle_lifetime_expression":{"max_lifetime":1},
        "minecraft:particle_appearance_billboard":{
        "size":["(v.particle_random_1*0.04+0.04)*v.size_modifier",
                "(v.particle_random_1*0.04+0.04)*v.size_modifier"],
        "facing_camera_mode":"lookat_xyz","uv":{
        "uv":["v.emittertexturecoord.u+v.emittertexturesize.u/4*v.particle_random_1*3",
              "v.emittertexturecoord.v+v.emittertexturesize.v/4*v.particle_random_2*3"],
        "uv_size":["v.emittertexturesize.u/4","v.emittertexturesize.v/4"]}}}}}"#
    ));
    system.spawn(&eating_item_request(
        [0.0, 0.0, -0.2],
        TileRequest {
            key: 42,
            size: 16,
            pixels: [220, 180, 20, 255].repeat(16 * 16).into(),
        },
    ));
    system.tick(0.001, &EmptyWorld);
    let emitter = &system.emitters_mut()[0];
    let placement = emitter.texture.normalized();
    let expected: Vec<_> = emitter
        .particles
        .iter()
        .map(|particle| {
            let r1 = particle.vars[V_PARTICLE_RANDOM as usize];
            let r2 = particle.vars[V_PARTICLE_RANDOM as usize + 1];
            (
                2.0 * (r1 * 0.04 + 0.04) * (2.0 / 3.0),
                [
                    placement[0] + placement[2] * r1 * 0.75,
                    placement[1] + placement[3] * r2 * 0.75,
                    placement[2] * 0.25,
                    placement[3] * 0.25,
                ],
            )
        })
        .collect();
    let draw = system.build_draw(
        &ParticleView {
            position: [0.0; 3],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.0,
        },
        &EmptyWorld,
    );
    for (instance, (extent, uv)) in draw.opaque.iter().zip(expected) {
        for axis in [instance.axis_x, instance.axis_y] {
            let full_extent = 2.0 * axis[..3].iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((full_extent - extent).abs() < 1e-6);
            assert!((0.053333..=0.106667).contains(&full_extent));
        }
        assert!(
            uv.iter()
                .zip(instance.uv)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
        assert_eq!(instance.uv[2] * ATLAS_SIDE as f32, 4.0);
        assert_eq!(instance.uv[3] * ATLAS_SIDE as f32, 4.0);
    }
    assert_eq!(draw.opaque.len(), 5);
}

#[test]
fn eating_emits_five_smaller_slower_fragments() {
    let request = eating_item_request(
        [1.0, 2.0, 3.0],
        TileRequest {
            key: 41,
            size: 1,
            pixels: vec![255; 4].into(),
        },
    );
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(
        br#"{"particle_effect":{"description":{
        "identifier":"minecraft:breaking_item_icon","basic_render_parameters":{
        "material":"particles_alpha","texture":"atlas.items"}},"components":{
        "minecraft:emitter_lifetime_once":{},
        "minecraft:emitter_rate_instant":{"num_particles":"v.num_particles"},
        "minecraft:particle_lifetime_expression":{"max_lifetime":1},
        "minecraft:particle_appearance_billboard":{"size":[0.05,0.05]}}}}"#
    ));
    system.spawn(&request);
    system.tick(0.001, &EmptyWorld);
    assert_eq!(system.live_particles(), 5);
    assert!(
        request
            .variables
            .contains(&("size_modifier".into(), 2.0 / 3.0))
    );
    assert!(request.variables.contains(&("speed_modifier".into(), 0.5)));
    assert!(request.variables.contains(&("emitter_radius".into(), 0.25)));
}
