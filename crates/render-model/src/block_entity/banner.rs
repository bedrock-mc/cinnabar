//! Banners: pole, crossbar and a cloth drawn once per layer (base color, then each pattern).
//!
//! Shared authoring boxes and the cloth pivot keep every artwork layer attached to the
//! crossbar. Mounting and animation use the same reflected model frame.

use assets::block_entity_geometry::{self as geometry, BoxSpec};
use glam::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Facing, Layer, MeshBuilder, model_matrix},
    scene::SceneClock,
};

const MODEL_SCALE: f32 = 0.64;
/// Coplanar layers are separated by this much inflation so they never fight.
const LAYER_INFLATE_STEP: f32 = 0.01;
const SWAY_PERIOD_TICKS: f64 = 100.0;
const SWAY_MEAN_DEGREES: f32 = 2.25;
const SWAY_AMPLITUDE_DEGREES: f32 = 1.8;
/// Pack path of a pattern texture, before the layer's pattern stem.
const BANNER_PATTERN_TEXTURE_PREFIX: &str = "textures/entity/banner/banner_";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BannerMount {
    /// On a pole; `rotation_degrees` follows the skull convention (0 faces south).
    Standing { rotation_degrees: f32 },
    /// Hanging from the top of a wall block, facing away from the wall.
    Wall(Facing),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BannerLayer {
    /// Texture stem after `banner_`, for example `stripe_bottom`.
    pub pattern: &'static str,
    /// Linear-space RGB dye color.
    pub color: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct BannerModel {
    pub mount: BannerMount,
    /// Linear-space RGB base dye color.
    pub base: [f32; 3],
    pub layers: Vec<BannerLayer>,
}

use assets::banner::MAX_BANNER_LAYERS;

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &BannerModel,
    clock: SceneClock,
) {
    let (base, has_pole) = match model.mount {
        BannerMount::Standing { rotation_degrees } => (
            model_matrix(block, [0.5, 0.48, 0.5], 180.0 - rotation_degrees)
                * Mat4::from_scale(Vec3::splat(MODEL_SCALE)),
            true,
        ),
        BannerMount::Wall(facing) => (
            model_matrix(block, [0.5, -0.16, 0.5], facing.yaw_degrees())
                * Mat4::from_translation(Vec3::new(0.0, -5.0, 7.0))
                * Mat4::from_scale(Vec3::splat(MODEL_SCALE)),
            false,
        ),
    };
    let Some(base_texture) = atlas.texture(geometry::BANNER_TEXTURE.0, geometry::BANNER_TEXTURE.1)
    else {
        return;
    };
    let white = [1.0; 4];
    if has_pole {
        builder.cuboid(
            Layer::Solid,
            &base_texture,
            base,
            world_box(geometry::BANNER_POLE),
            white,
        );
    }
    builder.cuboid(
        Layer::Solid,
        &base_texture,
        base,
        world_box(geometry::BANNER_BAR),
        white,
    );
    let sway = sway_degrees(block, clock);
    let hinge = Vec3::from(geometry::BANNER_CLOTH_PIVOT) * Vec3::new(1.0, -1.0, 1.0);
    let cloth = base * Mat4::from_translation(hinge) * Mat4::from_rotation_x(sway.to_radians());
    let tint = |color: [f32; 3]| [color[0], color[1], color[2], 1.0];
    let cloth_box = |inflate: f32| world_box(geometry::BANNER_CLOTH).inflated(inflate);
    builder.cuboid(
        Layer::Solid,
        &base_texture,
        cloth,
        cloth_box(0.0),
        tint(model.base),
    );
    // One name buffer serves every layer, so a swaying banner allocates at most once per frame.
    let mut name = String::new();
    for (index, layer) in model.layers.iter().take(MAX_BANNER_LAYERS).enumerate() {
        name.clear();
        name.push_str(BANNER_PATTERN_TEXTURE_PREFIX);
        name.push_str(layer.pattern);
        let Some(texture) = atlas.texture(&name, [64.0, 64.0]) else {
            continue;
        };
        builder.cuboid(
            Layer::Solid,
            &texture,
            cloth,
            cloth_box(LAYER_INFLATE_STEP * (index + 1) as f32),
            tint(layer.color),
        );
    }
}

/// Converts downward model-part Y coordinates into upward world model pixels.
fn world_box(([x, y, z], size, uv): geometry::ModelBox) -> BoxSpec {
    BoxSpec::new([x, -y - size[1], z], size, uv)
}

/// Cloth tilt in the upward-Y frame, varying with position so neighbors sway independently.
fn sway_degrees(block: [i32; 3], clock: SceneClock) -> f32 {
    let seed = (block[0].wrapping_mul(7))
        .wrapping_add(block[1].wrapping_mul(9))
        .wrapping_add(block[2].wrapping_mul(13));
    let phase = ((clock.ticks + f64::from(seed)) / SWAY_PERIOD_TICKS).fract() as f32;
    SWAY_MEAN_DEGREES - SWAY_AMPLITUDE_DEGREES * (std::f32::consts::TAU * phase).cos()
}

#[cfg(test)]
mod placement_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use assets::banner::pattern_texture;

    #[test]
    fn pattern_ids_resolve_and_unknown_ids_do_not() {
        assert_eq!(pattern_texture("bs"), Some("stripe_bottom"));
        assert_eq!(pattern_texture("lud"), Some("diagonal_left"));
        assert_eq!(pattern_texture("zzz"), None);
    }

    #[test]
    fn sway_stays_within_its_amplitude_band() {
        for tick in 0..200 {
            let angle = sway_degrees(
                [3, 64, -9],
                SceneClock {
                    ticks: f64::from(tick),
                },
            );
            assert!(
                (SWAY_MEAN_DEGREES - SWAY_AMPLITUDE_DEGREES - 1.0e-4
                    ..=SWAY_MEAN_DEGREES + SWAY_AMPLITUDE_DEGREES + 1.0e-4)
                    .contains(&angle)
            );
        }
    }
}
