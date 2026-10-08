use crate::{LevelParticle, TileRequest, classify_level_event, item_icon_request};

// Item effects keep a supplied texture struct instead of executing its default assignments.
const ITEM_EFFECT: &[u8] = br#"{"particle_effect":{"description":{
"identifier":"minecraft:breaking_item_icon","basic_render_parameters":{
"material":"particles_alpha","texture":"atlas.items"}},"components":{
"minecraft:emitter_initialization":{"creation_expression":
"v.emittertexturecoord ?? {v.emittertexturecoord.u=0;v.emittertexturecoord.v=0;}; v.emittertexturesize ?? {v.emittertexturesize.u=0;v.emittertexturesize.v=0;};"},
"minecraft:emitter_lifetime_once":{},
"minecraft:emitter_rate_instant":{"num_particles":"v.num_particles"},
"minecraft:particle_lifetime_expression":{"max_lifetime":1},
"minecraft:particle_appearance_billboard":{"size":[0.05,0.05],"uv":{
"uv":["v.emittertexturecoord.u+v.emittertexturesize.u/4*(v.particle_random_1*3)",
"v.emittertexturecoord.v+v.emittertexturesize.v/4*(v.particle_random_2*3)"],
"uv_size":["v.emittertexturesize.u/4","v.emittertexturesize.v/4"]}}}}}"#;

#[test]
fn snowball_impact_selects_the_item_icon_instead_of_an_explosion() {
    for data in [15, (321 << 16) | 15, i32::MIN | 15] {
        assert_eq!(
            classify_level_event(2009, data),
            Some(LevelParticle::FixedItemIcon {
                identifier: "minecraft:snowball",
                count: 1,
            }),
            "the particle type occupies the low half of the event data"
        );
    }
}

#[test]
fn legacy_snowball_impacts_use_one_item_fragment() {
    assert_eq!(
        classify_level_event(super::triggers::LEVEL_EVENT_PARTICLE_FLAG | 15, 0),
        Some(LevelParticle::FixedItemIcon {
            identifier: "minecraft:snowball",
            count: 1,
        })
    );
}

#[test]
fn item_icon_emitter_uses_the_native_radius() {
    let request = item_icon_request(
        [1.0, 2.0, 3.0],
        TileRequest {
            key: 41,
            size: 1,
            pixels: vec![255; 4].into(),
        },
        1.0,
    );
    assert_eq!(request.effect, "minecraft:breaking_item_icon");
    assert!(request.variables.contains(&("emitter_radius".into(), 0.25)));
}

#[test]
fn item_icon_defaults_preserve_the_supplied_sprite_rectangle() {
    use crate::{EmptyWorld, ParticleSystem, ParticleView, atlas::ATLAS_SIDE};
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(ITEM_EFFECT));
    let texel = [220, 230, 255, 255];
    assert!(
        system
            .spawn(&item_icon_request(
                [0.0; 3],
                TileRequest {
                    key: 91,
                    size: 4,
                    pixels: texel.repeat(16).into(),
                },
                1.0,
            ))
            .is_some()
    );
    system.tick(0.001, &EmptyWorld);
    let draw = system.build_draw(
        &ParticleView {
            position: [0.0, 0.0, 3.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.0,
        },
        &EmptyWorld,
    );
    assert_eq!(draw.blend.len(), 1);
    let uv = draw.blend[0].uv;
    assert!(
        uv[2] > 0.0 && uv[3] > 0.0,
        "the supplied item UV must survive creation defaults"
    );
    let x = ((uv[0] + uv[2] * 0.5) * ATLAS_SIDE as f32) as usize;
    let y = ((uv[1] + uv[3] * 0.5) * ATLAS_SIDE as f32) as usize;
    let offset = (y * ATLAS_SIDE as usize + x) * 4;
    assert_eq!(&system.atlas().pixels()[offset..offset + 4], &texel);
}
