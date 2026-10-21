//! Skulls and heads: an 8x8x8 head cube, plus a hat layer for humanoid heads.
//!
//! Dragon and piglin heads are drawn from the entity geometry via [`HeadModels`].

use std::sync::Arc;

use crate::{ActorRigGeometry, EntityRigId, append_entity_cube_vertices};
use assets::block_entity_geometry::{
    BoxSpec, SKULL_HAT, SKULL_HAT_INFLATE, SKULL_HEAD, heads::skull_mob_head, skull_has_hat,
    skull_texture,
};
use assets::{EntityGeometryCube, EntityGeometryScalar, EntityGeometryUv};
use glam::Mat4;

use super::{
    atlas::{BlockEntityAtlas, TextureRef},
    heads::HeadModels,
    mesh::{Facing, Layer, MeshBuilder, WHITE, model_matrix},
};

/// Discriminants are the legacy `SkullType` ordinals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SkullKind {
    Skeleton = 0,
    WitherSkeleton = 1,
    Zombie = 2,
    Player = 3,
    Creeper = 4,
    Dragon = 5,
    Piglin = 6,
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

    /// Packed skull rigs exist exactly when the head has an atlas texture.
    pub const fn packed_rig_index(self) -> Option<u8> {
        if skull_texture(self as u8).is_some() {
            Some(self as u8)
        } else {
            None
        }
    }

    /// Iterates packed head models across the legacy byte ordinal domain.
    pub fn packed_kinds() -> impl Iterator<Item = Self> {
        Self::kinds().filter(|kind| kind.packed_rig_index().is_some())
    }

    /// Every supported head model, including heads built from entity geometry.
    pub fn kinds() -> impl Iterator<Item = Self> {
        (0..=u8::MAX).filter_map(|value| Self::from_nbt(i64::from(value)))
    }

    /// The head's packed texture and the size its UVs were authored against.
    #[must_use]
    pub fn texture(self, atlas: &BlockEntityAtlas) -> Option<TextureRef> {
        let (name, logical) = skull_texture(self as u8)?;
        atlas.texture(name, logical)
    }

    /// Whether the head carries a second, slightly larger hat layer.
    #[must_use]
    pub const fn has_hat_layer(self) -> bool {
        skull_has_hat(self as u8)
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
                * Mat4::from_translation(glam::Vec3::new(0.0, 0.0, 4.0))
        }
    };
    let geometry = match model.kind {
        SkullKind::Piglin => heads.piglin.as_ref(),
        SkullKind::Dragon => heads.dragon.as_ref(),
        _ => None,
    }
    .zip(skull_mob_head(model.kind as u8))
    .and_then(|(head, source)| {
        atlas
            .texture(source.texture, head.texture)
            .map(|texture| (head, texture))
    });
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
    let (origin, size, uv) = SKULL_HEAD;
    builder.cuboid(
        Layer::Solid,
        &texture,
        matrix,
        BoxSpec::new(origin, size, uv),
        WHITE,
    );
    if model.kind.has_hat_layer() {
        // Hat inflation needs native measurement.
        let (origin, size, uv) = SKULL_HAT;
        builder.cuboid(
            Layer::Solid,
            &texture,
            matrix,
            BoxSpec::new(origin, size, uv).inflated(SKULL_HAT_INFLATE),
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
