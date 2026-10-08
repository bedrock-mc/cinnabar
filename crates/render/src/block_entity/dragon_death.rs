//! Untextured triangular rays around the dying dragon's body center.

use bevy::math::{Mat3, Vec3};
use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState};

use super::mesh::{BlockEntityVertex, MAX_BLOCK_ENTITY_VERTICES, MeshBuilder};

const RAY_COUNT_SCALE: f32 = 60.0;
const TRIANGLE_HALF_WIDTH: f32 = 0.866_025_4;
const VERTICES_PER_RAY: usize = 9;

/// Straight-alpha additive composition used by the untextured death effect.
pub const DRAGON_DEATH_BLEND: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::SrcAlpha,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::SrcAlpha,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

/// Frame-sampled dragon death state, independent of texture and actor rig geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragonDeathModel {
    pub center: [f32; 3],
    pub death_ticks: u32,
    pub partial_tick: f32,
    pub seed: u32,
    pub duration_ticks: f32,
}

fn vertices(model: &DragonDeathModel) -> Vec<BlockEntityVertex> {
    if model.death_ticks < 2
        || !model.center.iter().all(|value| value.is_finite())
        || !model.partial_tick.is_finite()
        || !model.duration_ticks.is_finite()
        || model.duration_ticks <= 0.0
    {
        return Vec::new();
    }
    let progress =
        (model.death_ticks as f32 + model.partial_tick.clamp(0.0, 1.0)) / model.duration_ticks;
    if !progress.is_finite() {
        return Vec::new();
    }
    let base = if progress > 0.8 {
        progress / 0.4 - 1.5
    } else {
        0.5
    };
    let strength = (base - (base - 0.95).max(0.0) * 20.0).max(0.0);
    let intensity = strength * strength;
    let alpha = (intensity * 255.0).clamp(0.0, 255.0) as u8;
    // Zero-alpha additive geometry cannot affect either color or depth.
    if alpha == 0 {
        return Vec::new();
    }
    let count = ((progress * progress + progress) * 0.5 * RAY_COUNT_SCALE)
        .ceil()
        .min(RAY_COUNT_SCALE) as usize;
    let mut random = assets::ClientRandom::new(model.seed);
    let mut rotation = Mat3::IDENTITY;
    let center = Vec3::from_array(model.center);
    let mut vertices = Vec::with_capacity(count * VERTICES_PER_RAY);
    for _ in 0..count {
        // The matrix and seeded stream continue from the preceding ray.
        for axis in 0..6 {
            let mut degrees = random.next_float() * 360.0;
            if axis == 5 {
                degrees += progress * 90.0;
            }
            let angle = degrees.to_radians();
            rotation *= match axis % 3 {
                0 => Mat3::from_rotation_x(angle),
                1 => Mat3::from_rotation_y(angle),
                _ => Mat3::from_rotation_z(angle),
            };
        }
        let length = random.next_float() * 20.0 + 5.0 + intensity * 10.0;
        let width = random.next_float() * 2.0 + 1.0 + intensity * 2.0;
        let corners = [
            Vec3::new(-TRIANGLE_HALF_WIDTH * width, length, -0.5 * width),
            Vec3::new(TRIANGLE_HALF_WIDTH * width, length, -0.5 * width),
            Vec3::new(0.0, length, width),
        ]
        .map(|point| (center + rotation * point).to_array());
        let normal = (rotation * Vec3::Y).normalize().to_array();
        let vertex = |position, color| BlockEntityVertex {
            position,
            color,
            normal,
            ..Default::default()
        };
        let origin = vertex(model.center, [1.0, 1.0, 1.0, f32::from(alpha) / 255.0]);
        let tips = corners.map(|position| vertex(position, [1.0, 0.0, 1.0, 0.0]));
        for side in 0..3 {
            vertices.extend([origin, tips[side], tips[(side + 1) % 3]]);
        }
    }
    vertices
}

pub(super) fn emit(builder: &mut MeshBuilder, model: &DragonDeathModel) {
    let available = MAX_BLOCK_ENTITY_VERTICES.saturating_sub(builder.additive.len());
    let geometry = vertices(model);
    let accepted = geometry
        .len()
        .min(available / VERTICES_PER_RAY * VERTICES_PER_RAY);
    builder.rejected_quads = builder
        .rejected_quads
        .saturating_add(((geometry.len() - accepted) / 3) as u64);
    let actor_light = builder.actor_light;
    let light = builder.light;
    builder
        .additive
        .extend(geometry.into_iter().take(accepted).map(|mut vertex| {
            vertex.actor_light = actor_light;
            for channel in &mut vertex.color[..3] {
                *channel *= light;
            }
            vertex
        }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_dragon_death_emits_white_centers_and_transparent_magenta_corners() {
        let model = DragonDeathModel {
            center: [10.0, 20.0, 30.0],
            death_ticks: 60,
            partial_tick: 0.5,
            seed: 17,
            duration_ticks: 120.0,
        };
        let rays = vertices(&model);
        assert!(!rays.is_empty(), "active dragon death must draw its rays");
        assert_eq!(rays.len() % 9, 0);
        for ray in rays.chunks_exact(9) {
            for triangle in ray.chunks_exact(3) {
                assert_eq!(triangle[0].position, model.center);
                assert_eq!(&triangle[0].color[..3], &[1.0; 3]);
                assert!(triangle[0].color[3] > 0.0);
                for corner in &triangle[1..] {
                    assert_ne!(corner.position, model.center);
                    assert_eq!(corner.color, [1.0, 0.0, 1.0, 0.0]);
                }
            }
        }
    }

    #[test]
    fn seeded_rays_repeat_at_the_same_frame_and_move_with_their_body() {
        let model = DragonDeathModel {
            center: [0.0; 3],
            death_ticks: 60,
            partial_tick: 0.0,
            seed: 17,
            duration_ticks: 120.0,
        };
        let original = vertices(&model);
        assert_eq!(original.len(), 23 * VERTICES_PER_RAY);
        assert_eq!(original[0].color[3], 63.0 / 255.0);
        assert_eq!(original, vertices(&model));
        let moved = vertices(&DragonDeathModel {
            center: [7.0, -11.0, 3.0],
            ..model
        });
        for (original, moved) in original.iter().zip(moved) {
            assert!(
                (Vec3::from_array(moved.position) - Vec3::from_array(original.position))
                    .abs_diff_eq(Vec3::new(7.0, -11.0, 3.0), 1e-5)
            );
            assert_eq!(original.color, moved.color);
        }
        assert_ne!(
            original,
            vertices(&DragonDeathModel {
                partial_tick: 0.5,
                ..model
            })
        );
        assert_ne!(original, vertices(&DragonDeathModel { seed: 18, ..model }));
    }

    #[test]
    fn every_ray_uses_a_unit_normal_along_its_fan_axis() {
        for seed in [17, 18, 99] {
            for death_ticks in [2, 60, 115] {
                let rays = vertices(&DragonDeathModel {
                    center: [0.0; 3],
                    death_ticks,
                    partial_tick: 0.5,
                    seed,
                    duration_ticks: 120.0,
                });
                assert!(!rays.is_empty());
                for (index, ray) in rays.chunks_exact(VERTICES_PER_RAY).enumerate() {
                    let normal = Vec3::from_array(ray[0].normal);
                    assert!(
                        (normal.length_squared() - 1.0).abs() <= 2.0 * f32::EPSILON,
                        "seed {seed}, counter {death_ticks}, ray {index}: normal {normal:?}"
                    );
                    assert!(ray.iter().all(|vertex| vertex.normal == ray[0].normal));
                    let axis = (Vec3::from_array(ray[1].position)
                        + Vec3::from_array(ray[2].position)
                        + Vec3::from_array(ray[5].position))
                    .normalize();
                    assert!(normal.abs_diff_eq(axis, 1e-5));
                }
            }
        }
    }

    #[test]
    fn initial_finished_and_malformed_death_states_emit_nothing() {
        let model = DragonDeathModel {
            center: [0.0; 3],
            death_ticks: 60,
            partial_tick: 0.0,
            seed: 17,
            duration_ticks: 120.0,
        };
        for invalid in [
            DragonDeathModel {
                death_ticks: 1,
                ..model
            },
            DragonDeathModel {
                death_ticks: 120,
                ..model
            },
            DragonDeathModel {
                death_ticks: u32::MAX,
                ..model
            },
            DragonDeathModel {
                duration_ticks: 0.0,
                ..model
            },
            DragonDeathModel {
                duration_ticks: f32::NAN,
                ..model
            },
            DragonDeathModel {
                partial_tick: f32::NAN,
                ..model
            },
            DragonDeathModel {
                center: [f32::INFINITY; 3],
                ..model
            },
        ] {
            assert!(vertices(&invalid).is_empty(), "{invalid:?}");
        }
        let mut builder = MeshBuilder::new([1; 2]);
        builder
            .additive
            .resize(MAX_BLOCK_ENTITY_VERTICES - 4, BlockEntityVertex::default());
        let before = builder.additive.len();
        emit(&mut builder, &model);
        assert_eq!(
            builder.additive.len(),
            before,
            "a partial ray is never submitted"
        );
        assert!(builder.rejected_quads > 0);
    }

    #[test]
    fn rays_need_no_texture_carrier_and_retire_with_the_next_empty_scene() {
        use crate::block_entity::{
            BlockEntityKind, BlockEntityLight, BlockEntityScene, BlockEntitySubmission, SceneClock,
        };
        let mut scene = BlockEntityScene::default();
        let submission = BlockEntitySubmission {
            block: [0; 3],
            light: BlockEntityLight::Actor { block: 4, sky: 11 },
            kind: BlockEntityKind::DragonDeath(DragonDeathModel {
                center: [0.0; 3],
                death_ticks: 60,
                partial_tick: 0.0,
                seed: 17,
                duration_ticks: 120.0,
            }),
        };
        let frame = scene
            .update(SceneClock::default(), &[], &[submission])
            .clone();
        assert!(!frame.additive.is_empty());
        assert!(frame.atlas.is_none() && frame.solid.is_empty() && frame.overlay.is_empty());
        assert!(
            frame
                .additive
                .iter()
                .all(|vertex| vertex.actor_light == crate::pack_actor_light(4, 11))
        );
        let retired = scene.update(SceneClock::default(), &[], &[]);
        assert!(retired.additive.is_empty());
        assert!(retired.revision > frame.revision);
    }
}
