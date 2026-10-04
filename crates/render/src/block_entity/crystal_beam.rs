//! Native end crystal beam, separate from the pack's animated cube rig.

use bevy::math::Vec3;

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder},
};

// Vanilla beam constants are .75, .2, 1/8 and .0001.
const SIDES: usize = 8;
const CRYSTAL_RADIUS: f32 = 0.75;
const TARGET_RADIUS_RATIO: f32 = 0.2;
const NORMALIZE_EPSILON: f32 = 0.0001;
// Beam UVs scroll by .01 per tick.
const UV_SCROLL_PER_TICK: f32 = 0.01;

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

/// Emits the native eight-sided strip, splitting the scrolling UV at atlas boundaries.
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
    let split = 1.0 - scroll;
    let rect = texture.rect;
    for (low, high, v_low, v_high) in [(0.0, split, scroll, 1.0), (split, 1.0, 0.0, scroll)] {
        if high - low <= f32::EPSILON {
            continue;
        }
        for side in 0..SIDES {
            let point = |step: usize, along: f32| {
                let angle = (step % SIDES) as f32 * std::f32::consts::TAU / SIDES as f32;
                let (sine, cosine) = angle.sin_cos();
                let radius =
                    CRYSTAL_RADIUS * (TARGET_RADIUS_RATIO + (1.0 - TARGET_RADIUS_RATIO) * along);
                (target.lerp(crystal, along) + radius * (tangent * sine + normal * cosine))
                    .to_array()
            };
            let u_low = rect.x + rect.width * side as f32 / SIDES as f32;
            let u_high = rect.x + rect.width * (side + 1) as f32 / SIDES as f32;
            let v_low = rect.y + rect.height * v_low;
            let v_high = rect.y + rect.height * v_high;
            builder.quad_uv_colors(
                Layer::Solid,
                [
                    point(side, low),
                    point(side + 1, low),
                    point(side + 1, high),
                    point(side, high),
                ],
                [
                    [u_low, v_low],
                    [u_high, v_low],
                    [u_high, v_high],
                    [u_low, v_high],
                ],
                [
                    [low, low, low, 1.0],
                    [low, low, low, 1.0],
                    [high, high, high, 1.0],
                    [high, high, high, 1.0],
                ],
            );
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
        assert_eq!(scrolled.solid.len(), SIDES * 12);
        assert!(scrolled.solid.iter().all(
            |vertex| (0.0..=1.0).contains(&vertex.uv[0]) && (0.0..=1.0).contains(&vertex.uv[1])
        ));
        assert!(!Arc::ptr_eq(&first.solid, &scrolled.solid));
    }
}
