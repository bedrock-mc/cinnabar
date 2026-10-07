//! Decorated pots: a body with four sherd faces, a neck and a lip.
//!
//! Neck, lip and top/bottom plane regions follow the pot base texture's unwrap and the side
//! quads use the shared side tile plus the sherd pattern; wobble and which sherd
//! sits on which face need native measurement.

use assets::block_entity_geometry::{self as geometry, POT_BODY_HALF as BODY_HALF};
use bevy::math::Vec3;

use super::{
    atlas::{AtlasRect, BlockEntityAtlas},
    mesh::{BoxSpec, Facing, Layer, MeshBuilder, WHITE, model_matrix},
};

/// Sherd patterns sit this far off the side tile so they never fight it, in pixels.
const PATTERN_LIFT: f32 = 0.02;

#[derive(Clone, Debug, PartialEq)]
pub struct DecoratedPotModel {
    pub facing: Facing,
    /// Pattern texture stems (for example `archer_pottery_pattern`) for back, left, right,
    /// front; `None` shows the plain side.
    pub sherds: [Option<String>; 4],
}

/// The pattern texture stem for a sherd item name; the plain brick shows no pattern.
#[must_use]
pub fn sherd_pattern(item: &str) -> Option<String> {
    let name = item.strip_prefix("minecraft:")?;
    let pattern = name.strip_suffix("_pottery_sherd")?;
    Some(format!("{pattern}_pottery_pattern"))
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &DecoratedPotModel,
) {
    let matrix = model_matrix(block, [0.5, 0.0, 0.5], model.facing.yaw_degrees());
    if let Some(base) = atlas.texture(geometry::POT_BASE_TEXTURE.0, geometry::POT_BASE_TEXTURE.1) {
        builder.cuboid(
            Layer::Solid,
            &base,
            matrix,
            BoxSpec::from(geometry::POT_NECK).inflated(geometry::POT_NECK_INFLATE),
            WHITE,
        );
        builder.cuboid(
            Layer::Solid,
            &base,
            matrix,
            BoxSpec::from(geometry::POT_LIP).inflated(geometry::POT_LIP_INFLATE),
            WHITE,
        );
        let plane = |texels: [f32; 4], y: f32| {
            let uv = base.rect_uv(texels);
            let (near, far) = (-BODY_HALF, BODY_HALF);
            let corners = [
                [far, y, far],
                [near, y, far],
                [near, y, near],
                [far, y, near],
            ];
            let world =
                corners.map(|corner| matrix.transform_point3(Vec3::from_array(corner)).to_array());
            (world, uv)
        };
        for (texels, y) in geometry::POT_PLANES {
            let (corners, uv) = plane(texels, y);
            builder.quad(Layer::Solid, corners, uv, WHITE);
        }
    }
    let Some(side) = atlas.texture(geometry::POT_SIDE_TEXTURE.0, geometry::POT_SIDE_TEXTURE.1)
    else {
        return;
    };
    // Faces as outward direction, top-left first when seen from outside; index into `sherds`.
    let lifts = [
        ([0.0, 0.0, -PATTERN_LIFT], 3),
        ([0.0, 0.0, PATTERN_LIFT], 0),
        ([PATTERN_LIFT, 0.0, 0.0], 1),
        ([-PATTERN_LIFT, 0.0, 0.0], 2),
    ];
    for (corners, (lift, index)) in geometry::pot_sides().into_iter().zip(lifts) {
        let world = |offset: [f32; 3]| {
            corners.map(|corner| {
                matrix
                    .transform_point3(Vec3::from_array(corner) + Vec3::from_array(offset))
                    .to_array()
            })
        };
        builder.textured_quad(Layer::Solid, world([0.0; 3]), side.rect, WHITE);
        let pattern = model.sherds[index]
            .as_deref()
            .and_then(|stem| atlas.texture(&format!("textures/blocks/{stem}"), [16.0, 16.0]));
        if let Some(pattern) = pattern {
            let rect: AtlasRect = pattern.rect;
            builder.textured_quad(Layer::Solid, world(lift), rect, WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sherd_items_map_to_pattern_textures_and_bricks_show_none() {
        assert_eq!(
            sherd_pattern("minecraft:archer_pottery_sherd").as_deref(),
            Some("archer_pottery_pattern")
        );
        assert_eq!(sherd_pattern("minecraft:brick"), None);
        assert_eq!(sherd_pattern("archer_pottery_sherd"), None);
    }
    #[test]
    fn decorated_pot_neck_and_lip_preserve_the_authored_height_and_inflation() {
        let encoded = assets::encode_block_entity_catalog(
            b"{}",
            32,
            32,
            &vec![255; 32 * 32 * 4],
            &[assets::BlockEntityPlacement {
                name: "textures/blocks/decorated_pot_base".into(),
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            }],
        )
        .unwrap();
        let atlas = BlockEntityAtlas::from_assets(
            &assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap(),
        );
        let mut builder = MeshBuilder::new(atlas.size());
        emit(
            &mut builder,
            &atlas,
            [0; 3],
            &DecoratedPotModel {
                facing: Facing::North,
                sherds: Default::default(),
            },
        );
        let bounds = |vertices: &[super::super::mesh::BlockEntityVertex]| {
            vertices
                .iter()
                .map(|vertex| vertex.position[1] * 16.0)
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), y| {
                    (min.min(y), max.max(y))
                })
        };
        let (neck_min, neck_max) = bounds(&builder.solid[..36]);
        let (lip_min, lip_max) = bounds(&builder.solid[36..72]);
        assert!((neck_min - 14.1).abs() < 1.0e-4 && (neck_max - 16.9).abs() < 1.0e-4);
        assert!((lip_min - 15.8).abs() < 1.0e-4 && (lip_max - 17.2).abs() < 1.0e-4);
    }
}
