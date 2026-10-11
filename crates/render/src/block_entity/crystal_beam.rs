//! Native end crystal beam, separate from the pack's animated cube rig.

use bevy::math::Vec3;

use super::{
    atlas::BlockEntityAtlas,
    mesh::{BlockEntityVertex, MAX_BLOCK_ENTITY_VERTICES, MeshBuilder},
};

// Vanilla beam constants are .75, .2, 1/8 and .0001.
const SIDES: usize = 8;
const CRYSTAL_RADIUS: f32 = 0.75;
const TARGET_RADIUS_RATIO: f32 = 0.2;
const NORMALIZE_EPSILON: f32 = 0.0001;
// Beam UVs scroll by .01 per tick.
const UV_SCROLL_PER_TICK: f32 = 0.01;

#[derive(Clone, Copy, Default)]
struct StripVertex {
    position: Vec3,
    uv: [f32; 2],
    shade: f32,
}

impl StripVertex {
    fn lerp(self, other: Self, alpha: f32) -> Self {
        Self {
            position: self.position.lerp(other.position, alpha),
            uv: std::array::from_fn(|axis| {
                self.uv[axis] + (other.uv[axis] - self.uv[axis]) * alpha
            }),
            shade: self.shade + (other.shade - self.shade) * alpha,
        }
    }
}

fn clip_triangle(triangle: [StripVertex; 3], below: bool) -> ([StripVertex; 4], usize) {
    let inside = |vertex: StripVertex| {
        if below {
            vertex.uv[1] <= 1.0
        } else {
            vertex.uv[1] >= 1.0
        }
    };
    let mut polygon = [StripVertex::default(); 4];
    let mut count = 0;
    let mut previous = triangle[2];
    for vertex in triangle {
        if inside(previous) != inside(vertex) {
            let alpha = (1.0 - previous.uv[1]) / (vertex.uv[1] - previous.uv[1]);
            polygon[count] = previous.lerp(vertex, alpha);
            polygon[count].uv[1] = 1.0;
            count += 1;
        }
        if inside(vertex) {
            polygon[count] = vertex;
            count += 1;
        }
        previous = vertex;
    }
    (polygon, count)
}

fn emit_triangle(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    rect: super::atlas::AtlasRect,
    triangle: [StripVertex; 3],
    wrap: f32,
) {
    if builder.solid.len() + triangle.len() > MAX_BLOCK_ENTITY_VERTICES {
        builder.rejected_quads = builder.rejected_quads.saturating_add(1);
        return;
    }
    let size = atlas.size();
    let light = builder.light;
    let actor_light = builder.actor_light;
    builder.solid.extend(triangle.map(|vertex| {
        let shade = vertex.shade * light;
        BlockEntityVertex {
            position: vertex.position.to_array(),
            uv: [
                (rect.x + rect.width * vertex.uv[0]) / size[0] as f32,
                (rect.y + rect.height * (vertex.uv[1] - wrap)) / size[1] as f32,
            ],
            color: [shade, shade, shade, 1.0],
            normal: Vec3::Y.to_array(),
            actor_light,
        }
    }));
}

/// A tapered, textured beam from the target block to the interpolated crystal origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrystalBeamModel {
    pub target: [f32; 3],
    pub crystal: [f32; 3],
    pub age_ticks: f32,
}

/// Normalizes a beam axis with the native tessellator's small-vector threshold.
fn normalized(vector: Vec3) -> Vec3 {
    let length = vector.length();
    if length >= NORMALIZE_EPSILON {
        vector / length
    } else {
        Vec3::ZERO
    }
}

/// Clips the eight-sided strip at atlas wraps while preserving its texture interpolation.
pub(super) fn emit(builder: &mut MeshBuilder, atlas: &BlockEntityAtlas, model: &CrystalBeamModel) {
    if !model
        .target
        .iter()
        .chain(&model.crystal)
        .all(|value| value.is_finite())
        || !model.age_ticks.is_finite()
    {
        return;
    }
    let Some(texture) = atlas.texture(assets::CRYSTAL_BEAM_TEXTURE, [16.0; 2]) else {
        return;
    };
    let target = Vec3::from_array(model.target);
    let crystal = Vec3::from_array(model.crystal);
    let direction = normalized(crystal - target);
    // Native builds the tangent by projecting world up perpendicular to the beam.
    let tangent = normalized(Vec3::new(
        -direction.x * direction.y,
        direction.x * direction.x + direction.z * direction.z,
        -direction.z * direction.y,
    ));
    let normal = normalized(direction.cross(tangent));
    let scroll = (model.age_ticks * UV_SCROLL_PER_TICK).rem_euclid(1.0);
    let vertex = |step: usize, along: f32| {
        let angle = (step % SIDES) as f32 * std::f32::consts::TAU / SIDES as f32;
        let (sine, cosine) = angle.sin_cos();
        let radius = CRYSTAL_RADIUS * (TARGET_RADIUS_RATIO + (1.0 - TARGET_RADIUS_RATIO) * along);
        StripVertex {
            position: target.lerp(crystal, along) + radius * (tangent * sine + normal * cosine),
            uv: [step as f32 / SIDES as f32, along + scroll],
            shade: along,
        }
    };
    for side in 0..SIDES {
        let [target_first, crystal_first, target_second, crystal_second] = [
            vertex(side, 0.0),
            vertex(side, 1.0),
            vertex(side + 1, 0.0),
            vertex(side + 1, 1.0),
        ];
        // The strip diagonal joins the first crystal vertex to the next target vertex.
        for triangle in [
            [target_first, crystal_first, target_second],
            [target_second, crystal_first, crystal_second],
        ] {
            if scroll == 0.0 {
                emit_triangle(builder, atlas, texture.rect, triangle, 0.0);
                continue;
            }
            for (below, wrap) in [(true, 0.0), (false, 1.0)] {
                let (polygon, count) = clip_triangle(triangle, below);
                for index in 1..count.saturating_sub(1) {
                    emit_triangle(
                        builder,
                        atlas,
                        texture.rect,
                        [polygon[0], polygon[index], polygon[index + 1]],
                        wrap,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_entity::{
        BlockEntityKind, BlockEntityScene, BlockEntitySubmission, SceneClock,
    };
    use std::sync::Arc;

    /// Installs a synthetic beam texture without relying on local Mojang assets.
    fn scene() -> BlockEntityScene {
        let encoded = assets::encode_block_entity_catalog(
            b"{}",
            16,
            16,
            &[255; 16 * 16 * 4],
            &[assets::BlockEntityPlacement {
                name: assets::CRYSTAL_BEAM_TEXTURE.into(),
                x: 0,
                y: 0,
                width: 16,
                height: 16,
            }],
        )
        .unwrap();
        let mut scene = BlockEntityScene::default();
        scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap());
        scene
    }

    fn sample_uv(vertices: &[super::super::mesh::BlockEntityVertex], point: Vec3) -> [f32; 2] {
        for triangle in vertices.as_chunks::<3>().0 {
            let [a, b, c] = std::array::from_fn(|i| Vec3::from_array(triangle[i].position));
            let ab = b - a;
            let ac = c - a;
            let ap = point - a;
            let denominator = ab.length_squared() * ac.length_squared() - ab.dot(ac).powi(2);
            if denominator <= f32::EPSILON {
                continue;
            }
            let v = (ap.dot(ab) * ac.length_squared() - ap.dot(ac) * ab.dot(ac)) / denominator;
            let w = (ap.dot(ac) * ab.length_squared() - ap.dot(ab) * ab.dot(ac)) / denominator;
            let weights = [1.0 - v - w, v, w];
            if weights.iter().all(|weight| *weight >= -1e-5)
                && (ap - ab * v - ac * w).length() < 1e-5
            {
                return std::array::from_fn(|axis| {
                    (0..3).map(|i| triangle[i].uv[axis] * weights[i]).sum()
                });
            }
        }
        panic!("beam does not cover the native strip sample");
    }

    #[test]
    fn taper_texture_interpolation_keeps_the_native_strip_diagonal_when_scrolling() {
        let mut scene = scene();
        let angle = std::f32::consts::TAU / SIDES as f32;
        let (sine, cosine) = angle.sin_cos();
        let target_first = Vec3::new(-0.15, 0.0, 0.0);
        let crystal_first = Vec3::new(-CRYSTAL_RADIUS, 0.0, 4.0);
        let target_second = Vec3::new(-0.15 * cosine, 0.15 * sine, 0.0);
        let crystal_second = Vec3::new(-CRYSTAL_RADIUS * cosine, CRYSTAL_RADIUS * sine, 4.0);
        let samples = [
            (
                (target_first + crystal_first + target_second) / 3.0,
                [1.0 / (3 * SIDES) as f32, 1.0 / 3.0],
            ),
            (
                (target_second + crystal_first + crystal_second) / 3.0,
                [2.0 / (3 * SIDES) as f32, 2.0 / 3.0],
            ),
        ];
        let atlas_height = scene.atlas().unwrap().size()[1];
        for age_ticks in [0.0, 25.0, 50.0, 99.0, 100.0] {
            let submission = BlockEntitySubmission {
                block: [0; 3],
                light: 1.0.into(),
                kind: BlockEntityKind::CrystalBeam(CrystalBeamModel {
                    target: [0.0; 3],
                    crystal: [0.0, 0.0, 4.0],
                    age_ticks,
                }),
            };
            let frame = scene.update(SceneClock::default(), &[], &[submission]);
            for (point, expected) in samples {
                let uv = sample_uv(&frame.solid, point);
                assert!((uv[0] - expected[0]).abs() < 1e-5, "{age_ticks}: {uv:?}");
                let expected_v = (expected[1] + age_ticks * UV_SCROLL_PER_TICK).rem_euclid(1.0)
                    * 16.0
                    / atlas_height as f32;
                assert!((uv[1] - expected_v).abs() < 1e-5, "{age_ticks}: {uv:?}");
            }
        }
    }

    #[test]
    fn beam_keeps_owner_actor_light_and_an_up_normal_on_every_side() {
        let mut scene = scene();
        let frame = scene.update(
            SceneClock::default(),
            &[],
            &[BlockEntitySubmission {
                block: [0; 3],
                light: crate::block_entity::BlockEntityLight::Actor { block: 4, sky: 11 },
                kind: BlockEntityKind::CrystalBeam(CrystalBeamModel {
                    target: [0.0; 3],
                    crystal: [3.0, 2.0, 4.0],
                    age_ticks: 50.0,
                }),
            }],
        );
        assert!(!frame.solid.is_empty());
        assert!(frame.solid.iter().all(|vertex| {
            vertex.normal == Vec3::Y.to_array()
                && vertex.actor_light == crate::pack_actor_light(4, 11)
        }));
    }

    #[test]
    fn targeted_crystal_emits_eight_tapered_sides_with_native_gradient_and_scroll() {
        let mut scene = scene();
        let submission = |age_ticks| BlockEntitySubmission {
            block: [0; 3],
            light: 1.0.into(),
            kind: BlockEntityKind::CrystalBeam(CrystalBeamModel {
                target: [0.0; 3],
                crystal: [0.0, 0.0, 4.0],
                age_ticks,
            }),
        };
        let first = scene
            .update(SceneClock::default(), &[], &[submission(0.0)])
            .clone();
        assert_eq!(first.solid.len(), SIDES * 6);
        let target = first
            .solid
            .iter()
            .find(|vertex| vertex.position[2] == 0.0)
            .unwrap();
        let crystal = first
            .solid
            .iter()
            .find(|vertex| vertex.position[2] == 4.0)
            .unwrap();
        assert_eq!(target.color, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(crystal.color, [1.0; 4]);
        assert!((Vec3::from_array(target.position).length() - 0.15).abs() < 1e-6);
        assert!(
            (Vec3::from_array(crystal.position).distance(Vec3::new(0.0, 0.0, 4.0))
                - CRYSTAL_RADIUS)
                .abs()
                < 1e-6
        );
        let scrolled = scene.update(SceneClock::default(), &[], &[submission(50.0)]);
        assert_eq!(scrolled.solid.len(), SIDES * 18);
        assert!(scrolled.solid.iter().all(
            |vertex| (0.0..=1.0).contains(&vertex.uv[0]) && (0.0..=1.0).contains(&vertex.uv[1])
        ));
        assert!(!Arc::ptr_eq(&first.solid, &scrolled.solid));
    }
}
