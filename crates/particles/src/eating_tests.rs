use crate::{EmptyWorld, ParticleSystem, TileRequest, eating_item_request};

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
