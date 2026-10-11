//! Chests: single and double, with the lid hinged at the back edge.
//!
//! Box sizes and UV origins follow the chest textures' unwraps (56x19 lid, 56x24 body,
//! 6x5 latch; 88 wide for the double); hinge height and latch placement need native measurement.

use glam::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{BoxSpec, Facing, Layer, MeshBuilder, WHITE, model_matrix},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopperAge {
    Unaffected,
    Exposed,
    Weathered,
    Oxidized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChestVariant {
    Normal,
    Trapped,
    Ender,
    Copper(CopperAge),
}

impl ChestVariant {
    fn texture(self, double: bool) -> String {
        let stem = match self {
            Self::Normal => "normal",
            Self::Trapped => "trapped",
            Self::Ender => "ender",
            Self::Copper(CopperAge::Unaffected) => "copper_default",
            Self::Copper(CopperAge::Exposed) => "copper_exposed",
            Self::Copper(CopperAge::Weathered) => "copper_weathered",
            Self::Copper(CopperAge::Oxidized) => "copper_oxidized",
        };
        let suffix = match (double, self) {
            (true, Self::Normal) => "double_normal".to_owned(),
            (true, Self::Ender) => stem.to_owned(),
            (true, _) => format!("{stem}_double"),
            (false, _) => stem.to_owned(),
        };
        format!("textures/entity/chest/{suffix}")
    }
}

/// How this block entity takes part in a double chest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChestPair {
    Single,
    /// Draws the whole double model spanning itself and `partner`.
    Lead {
        partner: [i32; 3],
    },
    /// Draws nothing; the lead draws the merged model.
    Follower,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChestModel {
    pub variant: ChestVariant,
    pub facing: Facing,
    pub pair: ChestPair,
    /// Raw lid openness in `0.0..=1.0`, before the easing curve.
    pub lid: f32,
}

/// Lid angle in radians for raw openness `raw`: eased out, up to a quarter turn.
#[must_use]
pub fn lid_angle_radians(raw: f32) -> f32 {
    let open = raw.clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - open).powi(3);
    eased * std::f32::consts::FRAC_PI_2
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &ChestModel,
) {
    let (double, half_width, center, logical) = match model.pair {
        ChestPair::Follower => return,
        ChestPair::Single => (false, 7.0, [0.5, 0.0, 0.5], [64.0, 64.0]),
        ChestPair::Lead { partner } => (
            true,
            15.0,
            [
                0.5 + (partner[0] - block[0]) as f32 * 0.5,
                0.0,
                0.5 + (partner[2] - block[2]) as f32 * 0.5,
            ],
            [128.0, 64.0],
        ),
    };
    let Some(texture) = atlas.texture(&model.variant.texture(double), logical) else {
        return;
    };
    let base = model_matrix(block, center, model.facing.yaw_degrees());
    let width = half_width * 2.0;
    builder.cuboid(
        Layer::Solid,
        &texture,
        base,
        BoxSpec::new([-half_width, 0.0, -7.0], [width, 10.0, 14.0], [0.0, 19.0]),
        WHITE,
    );
    // Lid and latch swing together about the back-bottom edge of the lid.
    let hinge = Vec3::new(0.0, 9.0, 7.0);
    let lid_matrix = base
        * Mat4::from_translation(hinge)
        * Mat4::from_rotation_x(lid_angle_radians(model.lid))
        * Mat4::from_translation(-hinge);
    builder.cuboid(
        Layer::Solid,
        &texture,
        lid_matrix,
        BoxSpec::new([-half_width, 9.0, -7.0], [width, 5.0, 14.0], [0.0, 0.0]),
        WHITE,
    );
    builder.cuboid(
        Layer::Solid,
        &texture,
        lid_matrix,
        BoxSpec::new([-1.0, 7.0, -8.0], [2.0, 4.0, 1.0], [0.0, 0.0]),
        WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_entity::atlas::{AtlasRect, TextureRef};

    #[test]
    fn lid_curve_is_eased_and_bounded() {
        assert_eq!(lid_angle_radians(0.0), 0.0);
        assert!((lid_angle_radians(1.0) - std::f32::consts::FRAC_PI_2).abs() < 1.0e-6);
        assert!(lid_angle_radians(0.5) > std::f32::consts::FRAC_PI_4);
        assert_eq!(lid_angle_radians(2.0), lid_angle_radians(1.0));
    }

    #[test]
    fn texture_names_cover_double_and_variant_forms() {
        assert_eq!(
            ChestVariant::Normal.texture(true),
            "textures/entity/chest/double_normal"
        );
        assert_eq!(
            ChestVariant::Trapped.texture(true),
            "textures/entity/chest/trapped_double"
        );
        assert_eq!(
            ChestVariant::Copper(CopperAge::Exposed).texture(false),
            "textures/entity/chest/copper_exposed"
        );
    }

    #[test]
    fn opening_swings_the_lid_above_its_closed_height() {
        let texture = TextureRef {
            rect: AtlasRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            },
            logical: [64.0, 64.0],
        };
        let top = |lid: f32| {
            let mut builder = MeshBuilder::new([64, 64]);
            let hinge = Vec3::new(0.0, 9.0, 7.0);
            let matrix = Mat4::from_translation(hinge)
                * Mat4::from_rotation_x(lid_angle_radians(lid))
                * Mat4::from_translation(-hinge);
            builder.cuboid(
                Layer::Solid,
                &texture,
                matrix,
                BoxSpec::new([-7.0, 9.0, -7.0], [14.0, 5.0, 14.0], [0.0, 0.0]),
                WHITE,
            );
            builder
                .solid
                .iter()
                .map(|vertex| vertex.position[1])
                .fold(f32::MIN, f32::max)
        };
        assert!(top(1.0) > top(0.0) + 5.0);
    }
}
