//! Banners: pole, crossbar and a cloth drawn once per layer (base color, then each pattern).
//!
//! Box sizes and UV origins follow the banner texture unwrap (20x40x1 cloth, 2x42x2 pole,
//! 20x2x2 bar). The 2/3 model scale, mounting heights and sway parameters need native
//! measurement.

use assets::block_entity_geometry as geometry;
use bevy::math::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{BoxSpec, Facing, Layer, MeshBuilder, model_matrix},
    scene::SceneClock,
};

const MODEL_SCALE: f32 = 2.0 / 3.0;
/// Coplanar layers are separated by this much inflation so they never fight.
const LAYER_INFLATE_STEP: f32 = 0.01;
const SWAY_PERIOD_TICKS: f64 = 100.0;
const SWAY_MEAN_DEGREES: f32 = -2.2;
const SWAY_AMPLITUDE_DEGREES: f32 = 1.8;

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

/// Maximum pattern layers drawn per banner, as in vanilla's crafting limit plus slack.
pub const MAX_BANNER_LAYERS: usize = 16;

/// The pattern texture stem for a Bedrock banner pattern id.
#[must_use]
pub fn pattern_texture(id: &str) -> Option<&'static str> {
    Some(match id {
        "bo" => "border",
        "bri" => "bricks",
        "mc" => "circle",
        "cre" => "creeper",
        "cr" => "cross",
        "cbo" => "curly_border",
        "lud" => "diagonal_left",
        "rd" => "diagonal_right",
        "ld" => "diagonal_up_left",
        "rud" => "diagonal_up_right",
        "flo" => "flower",
        "gra" => "gradient",
        "gru" => "gradient_up",
        "hh" => "half_horizontal",
        "hhb" => "half_horizontal_bottom",
        "vh" => "half_vertical",
        "vhr" => "half_vertical_right",
        "moj" => "mojang",
        "mr" => "rhombus",
        "sku" => "skull",
        "ss" => "small_stripes",
        "bl" => "square_bottom_left",
        "br" => "square_bottom_right",
        "tl" => "square_top_left",
        "tr" => "square_top_right",
        "sc" => "straight_cross",
        "bs" => "stripe_bottom",
        "cs" => "stripe_center",
        "dls" => "stripe_downleft",
        "drs" => "stripe_downright",
        "ls" => "stripe_left",
        "ms" => "stripe_middle",
        "rs" => "stripe_right",
        "ts" => "stripe_top",
        "bt" => "triangle_bottom",
        "tt" => "triangle_top",
        "bts" => "triangles_bottom",
        "tts" => "triangles_top",
        "glb" => "globe",
        "pig" => "piglin",
        "flw" => "flow",
        "gus" => "guster",
        _ => return None,
    })
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &BannerModel,
    clock: SceneClock,
) {
    let (base, has_pole) = match model.mount {
        BannerMount::Standing { rotation_degrees } => (
            model_matrix(block, [0.5, 0.0, 0.5], 180.0 - rotation_degrees)
                * Mat4::from_scale(Vec3::splat(MODEL_SCALE)),
            true,
        ),
        // Crossbar top flush with the block top, back against the wall.
        BannerMount::Wall(facing) => (
            model_matrix(block, [0.5, 0.0, 0.5], facing.yaw_degrees())
                * Mat4::from_translation(Vec3::new(0.0, 16.0 - 44.0 * MODEL_SCALE, 7.33))
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
            world_box(geometry::BANNER_POLE, 12.0),
            white,
        );
    }
    builder.cuboid(
        Layer::Solid,
        &base_texture,
        base,
        world_box(geometry::BANNER_BAR, 12.0),
        white,
    );
    let sway = sway_degrees(block, clock);
    let hinge = Vec3::new(0.0, 42.0, -1.0);
    let cloth = base
        * Mat4::from_translation(hinge)
        * Mat4::from_rotation_x(sway.to_radians())
        * Mat4::from_translation(-hinge);
    let tint = |color: [f32; 3]| [color[0], color[1], color[2], 1.0];
    let cloth_box = |inflate: f32| world_box(geometry::BANNER_CLOTH, hinge.y).inflated(inflate);
    builder.cuboid(
        Layer::Solid,
        &base_texture,
        cloth,
        cloth_box(0.0),
        tint(model.base),
    );
    for (index, layer) in model.layers.iter().take(MAX_BANNER_LAYERS).enumerate() {
        let Some(texture) = atlas.texture(
            &format!("textures/entity/banner/banner_{}", layer.pattern),
            [64.0, 64.0],
        ) else {
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
fn world_box(([x, y, z], size, uv): geometry::ModelBox, anchor: f32) -> BoxSpec {
    BoxSpec::new([x, anchor - y - size[1], z], size, uv)
}

/// Cloth tilt about the crossbar, varying with position so neighbors do not sway in step.
fn sway_degrees(block: [i32; 3], clock: SceneClock) -> f32 {
    let seed = (block[0].wrapping_mul(7))
        .wrapping_add(block[1].wrapping_mul(9))
        .wrapping_add(block[2].wrapping_mul(13));
    let phase = ((clock.ticks + f64::from(seed)) / SWAY_PERIOD_TICKS).fract() as f32;
    SWAY_MEAN_DEGREES + SWAY_AMPLITUDE_DEGREES * (std::f32::consts::TAU * phase).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

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
