use super::*;
use crate::{ParticleView, emitter::TileRequest, world::EmptyWorld};

/// A one-particle flipbook on the cosmetic sprite, authored in 16-texel sprite space.
fn effect() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"particle_effect": {
        "description": {"identifier": "cinnabar_cosmetic:0123456789abcdef_0",
            "basic_render_parameters": {"material": "particles_alpha", "texture": "atlas.cosmetic"}},
        "components": {
            "minecraft:emitter_lifetime_once": {"active_time": 1},
            "minecraft:emitter_rate_instant": {"num_particles": 1},
            "minecraft:emitter_shape_point": {},
            "minecraft:particle_lifetime_expression": {"max_lifetime": 4},
            "minecraft:particle_appearance_billboard": {
                "size": [0.1, 0.1],
                "facing_camera_mode": "lookat_xyz",
                "uv": {"texture_width": 16, "texture_height": 16, "flipbook": {
                    "base_UV": [0, 8], "size_UV": [8, 8], "step_UV": [8, 0],
                    "frames_per_second": 1, "max_frame": 2, "loop": true
                }}
            }
        }
    }}))
    .unwrap()
}

/// Draws the system's particles from a camera looking down -Z at the origin.
fn uvs(system: &mut ParticleSystem) -> Vec<[f32; 4]> {
    let view = ParticleView {
        position: [0.0, 0.0, 2.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    system
        .build_draw(&view, &EmptyWorld)
        .opaque
        .iter()
        .map(|instance| instance.uv)
        .collect()
}

#[test]
fn cosmetic_flipbooks_sample_sub_rects_of_their_own_sprite_tile() {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&effect()));
    let request = SpawnRequest {
        effect: "cinnabar_cosmetic:0123456789abcdef_0".into(),
        tile: Some(TileRequest {
            key: 0xC05,
            size: 16,
            pixels: vec![200; 16 * 16 * 4].into(),
        }),
        ..SpawnRequest::default()
    };
    let id = system.spawn(&request).unwrap();
    let emitter = system.emitters.iter().find(|e| e.id == id).unwrap();
    let tile = emitter.texture;
    assert_ne!(tile, system.atlas.fallback());
    assert!(
        system.atlas.placement_references(tile) > 0,
        "live emitters pin their tile"
    );
    let mut resolved = std::borrow::Cow::Borrowed(&request);
    assert_eq!(
        system.resolve_texture(&crate::def::TextureSource::Cosmetic, &mut resolved),
        tile,
        "the cached sprite tile is reused"
    );
    assert!(
        resolved.variables.is_empty(),
        "cosmetic sprites add no terrain tile variables"
    );
    let [u, v, w, h] = tile.normalized();
    system.tick(0.05, &EmptyWorld);
    assert_eq!(uvs(&mut system), [[u, v + h / 2.0, w / 2.0, h / 2.0]]);
    // Frames are capped at a quarter second, so age the particle past one flipbook frame.
    for _ in 0..4 {
        system.tick(0.25, &EmptyWorld);
    }
    assert_eq!(
        uvs(&mut system),
        [[u + w / 2.0, v + h / 2.0, w / 2.0, h / 2.0]],
        "the second frame steps across the sprite"
    );

    let mut missing = ParticleSystem::default();
    assert!(missing.register_effect(&effect()));
    let id = missing
        .spawn(&SpawnRequest {
            tile: None,
            ..request
        })
        .unwrap();
    let fallback = missing.atlas.fallback();
    assert_eq!(
        missing
            .emitters
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .texture,
        fallback,
        "a spawn without its sprite draws the white texel"
    );
}
