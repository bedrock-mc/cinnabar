//! Skulls and heads: an 8x8x8 head cube, plus a hat layer for humanoid heads.
//!
//! Dragon and piglin heads are drawn from the entity geometry via [`HeadModels`].

use std::sync::Arc;

use assets::{EntityGeometryCube, EntityGeometryScalar, EntityGeometryUv};
use bevy::math::Mat4;
use render_model::{ActorRigGeometry, EntityRigId, append_entity_cube_vertices};

use super::{
    atlas::{BlockEntityAtlas, TextureRef},
    heads::HeadModels,
    mesh::{BoxSpec, Facing, Layer, MeshBuilder, WHITE, model_matrix},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkullKind {
    Skeleton,
    WitherSkeleton,
    Zombie,
    Player,
    Creeper,
    Dragon,
    Piglin,
}

impl SkullKind {
    /// Current native placed-head model selection follows the backing block type.
    #[must_use]
    pub fn from_block_identifier(identifier: &str) -> Option<Self> {
        assets::vanilla_skull_type(identifier).and_then(|kind| Self::from_nbt(i64::from(kind)))
    }

    /// The `SkullType` NBT byte.
    #[must_use]
    pub const fn from_nbt(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::Skeleton,
            1 => Self::WitherSkeleton,
            2 => Self::Zombie,
            3 => Self::Player,
            4 => Self::Creeper,
            5 => Self::Dragon,
            6 => Self::Piglin,
            _ => return None,
        })
    }

    /// The head's packed texture and the size its UVs were authored against.
    #[must_use]
    pub fn texture(self, atlas: &BlockEntityAtlas) -> Option<TextureRef> {
        match self {
            Self::Skeleton => atlas.texture("textures/entity/skulls/skeleton", [64.0, 32.0]),
            Self::WitherSkeleton => {
                atlas.texture("textures/entity/skulls/wither_skeleton", [64.0, 32.0])
            }
            Self::Zombie => atlas.texture("textures/entity/skulls/zombie", [64.0, 32.0]),
            Self::Creeper => atlas.texture("textures/entity/skulls/creeper", [64.0, 32.0]),
            Self::Player => atlas.texture("textures/entity/steve", [64.0, 64.0]),
            Self::Dragon | Self::Piglin => None,
        }
    }

    /// Whether the head carries a second, slightly larger hat layer.
    #[must_use]
    pub const fn has_hat_layer(self) -> bool {
        matches!(self, Self::Zombie | Self::Player)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkullMount {
    /// Standing on a block; `rotation_degrees` is the NBT `Rotation`, 0 facing south.
    Floor { rotation_degrees: f32 },
    /// Mounted on the wall opposite `Facing`, looking along it.
    Wall(Facing),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkullModel {
    pub kind: SkullKind,
    pub mount: SkullMount,
}

/// Yaw turning a head whose `Rotation` is 0 (facing south) to `rotation_degrees`.
#[must_use]
pub fn floor_yaw_degrees(rotation_degrees: f32) -> f32 {
    180.0 - rotation_degrees
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    heads: &HeadModels,
    block: [i32; 3],
    model: &SkullModel,
) {
    let matrix = match model.mount {
        SkullMount::Floor { rotation_degrees } => {
            model_matrix(block, [0.5, 0.0, 0.5], floor_yaw_degrees(rotation_degrees))
        }
        // Head bottom 4px up, back face flush with the wall.
        SkullMount::Wall(facing) => {
            model_matrix(block, [0.5, 0.25, 0.5], facing.yaw_degrees())
                * Mat4::from_translation(bevy::math::Vec3::new(0.0, 0.0, 4.0))
        }
    };
    let geometry = match model.kind {
        SkullKind::Piglin => heads
            .piglin
            .as_ref()
            .zip(atlas.texture("textures/entity/piglin/piglin", [64.0, 64.0])),
        SkullKind::Dragon => heads.dragon.as_ref().and_then(|head| {
            atlas
                .texture("textures/entity/dragon/dragon", head.texture)
                .map(|texture| (head, texture))
        }),
        _ => None,
    };
    if let Some((head, texture)) = geometry {
        for head_box in &head.boxes {
            builder.cuboid(
                Layer::Solid,
                &texture,
                matrix * head_box.matrix,
                head_box.spec,
                WHITE,
            );
        }
        return;
    }
    let Some(texture) = model.kind.texture(atlas) else {
        return;
    };
    builder.cuboid(
        Layer::Solid,
        &texture,
        matrix,
        BoxSpec::new([-4.0, 0.0, -4.0], [8.0; 3], [0.0, 0.0]),
        WHITE,
    );
    if model.kind.has_hat_layer() {
        // Hat inflation needs native measurement.
        builder.cuboid(
            Layer::Solid,
            &texture,
            matrix,
            BoxSpec::new([-4.0, 0.0, -4.0], [8.0; 3], [32.0, 0.0]).inflated(0.25),
            WHITE,
        );
    }
}

/// A worn skull: the player head cube (plus the hat layer for humanoid heads) on one bone, with
/// its UVs mapped into the block-entity atlas's static region. `None` when the kind has no
/// packed texture.
#[must_use]
pub fn skull_geometry(
    id: EntityRigId,
    atlas: &BlockEntityAtlas,
    kind: SkullKind,
) -> Option<ActorRigGeometry> {
    let texture = kind.texture(atlas)?;
    let [atlas_width, _] = atlas.size();
    let static_height = atlas.static_height();
    let rect = texture.rect_uv([0.0, 0.0, texture.logical[0], texture.logical[1]]);
    let region = [
        rect[0] / atlas_width as f32,
        rect[1] / static_height as f32,
        rect[2] / atlas_width as f32,
        rect[3] / static_height as f32,
    ];
    let logical = (texture.logical[0] as u16, texture.logical[1] as u16);
    let scalar =
        |value: f32| EntityGeometryScalar::new(value).unwrap_or(EntityGeometryScalar::ZERO);
    let cube = |uv: [f32; 2], inflate: f32| EntityGeometryCube {
        origin: [-4.0, 24.0, -4.0].map(scalar),
        size: [8.0; 3].map(scalar),
        pivot: [EntityGeometryScalar::ZERO; 3],
        rotation: [EntityGeometryScalar::ZERO; 3],
        uv: EntityGeometryUv::Box(uv.map(scalar)),
        inflate: scalar(inflate),
        mirror: false,
    };
    let mut vertices = Vec::new();
    let mut layers = vec![cube([0.0, 0.0], 0.0)];
    if kind.has_hat_layer() {
        layers.push(cube([32.0, 0.0], 0.25));
    }
    for layer in &layers {
        append_entity_cube_vertices(&mut vertices, layer, 0, logical, false, 0.0).ok()?;
    }
    for vertex in &mut vertices {
        for uv in [&mut vertex.uv, &mut vertex.back_uv] {
            uv[0] = region[0] + uv[0] * (region[2] - region[0]);
            uv[1] = region[1] + uv[1] * (region[3] - region[1]);
        }
    }
    ActorRigGeometry::new(id, Arc::from(vertices), Arc::from(vec![[0.0, 1.5, 0.0]])).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nbt_types_and_floor_yaw_follow_the_rotation_convention() {
        assert_eq!(SkullKind::from_nbt(3), Some(SkullKind::Player));
        assert_eq!(SkullKind::from_nbt(9), None);
        // Rotation 0 faces south (yaw 180); a quarter turn faces west (yaw 90).
        assert_eq!(floor_yaw_degrees(0.0), Facing::South.yaw_degrees());
        assert_eq!(floor_yaw_degrees(90.0), Facing::West.yaw_degrees());
    }
}
