//! Block-entity items held in hand: chests, shulker boxes, decorated pots and conduits drawn
//! as their placed models, closed and at rest, centred in the unit block the held cube uses.
//!
//! Provisional: the native item renderer's pose for these models (front facing and lid state)
//! is unmeasured; they face south like placed blocks with default facing.

use std::sync::Arc;

use super::{
    atlas::BlockEntityAtlas,
    chest::{self, ChestModel, ChestPair, ChestVariant},
    conduit::{self, ConduitModel},
    mesh::{Facing, MeshBuilder},
    pot::{self, DecoratedPotModel},
    scene::SceneClock,
    shulker::{self, ShulkerModel},
};
use crate::{ActorRigGeometry, ActorRigSurface, ActorRigVertex, EntityRigId};

/// A block-entity item with a held model.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum HeldBlockEntity {
    Chest(ChestVariantKey),
    Shulker(&'static str),
    DecoratedPot,
    Conduit,
}

/// An orderable stand-in for [`ChestVariant`], so held models can key a map.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ChestVariantKey {
    Normal,
    Trapped,
    Ender,
    Copper(u8),
}

impl ChestVariantKey {
    /// The key of `variant`.
    const fn of(variant: ChestVariant) -> Self {
        match variant {
            ChestVariant::Normal => Self::Normal,
            ChestVariant::Trapped => Self::Trapped,
            ChestVariant::Ender => Self::Ender,
            ChestVariant::Copper(age) => Self::Copper(age as u8),
        }
    }

    /// The variant this key stands for.
    const fn variant(self) -> ChestVariant {
        use super::chest::CopperAge;
        match self {
            Self::Normal => ChestVariant::Normal,
            Self::Trapped => ChestVariant::Trapped,
            Self::Ender => ChestVariant::Ender,
            Self::Copper(0) => ChestVariant::Copper(CopperAge::Unaffected),
            Self::Copper(1) => ChestVariant::Copper(CopperAge::Exposed),
            Self::Copper(2) => ChestVariant::Copper(CopperAge::Weathered),
            Self::Copper(_) => ChestVariant::Copper(CopperAge::Oxidized),
        }
    }
}

impl HeldBlockEntity {
    /// The held model the item `identifier` draws, or `None` for items without one.
    #[must_use]
    pub fn from_item(identifier: &str) -> Option<Self> {
        if identifier == "minecraft:ender_chest" {
            return Some(Self::Chest(ChestVariantKey::Ender));
        }
        if let Some(variant) = ChestVariant::from_block_name(identifier) {
            return Some(Self::Chest(ChestVariantKey::of(variant)));
        }
        if let Some(color) =
            assets::block_entity_geometry::shulker_color_from_block_name(identifier)
        {
            return Some(Self::Shulker(color));
        }
        match identifier {
            "minecraft:decorated_pot" => Some(Self::DecoratedPot),
            "minecraft:conduit" => Some(Self::Conduit),
            _ => None,
        }
    }

    /// Every held model, for building them once per atlas.
    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut all = vec![
            Self::Chest(ChestVariantKey::Normal),
            Self::Chest(ChestVariantKey::Trapped),
            Self::Chest(ChestVariantKey::Ender),
            Self::DecoratedPot,
            Self::Conduit,
        ];
        all.extend((0..4).map(|age| Self::Chest(ChestVariantKey::Copper(age))));
        all.extend(
            assets::block_entity_geometry::SHULKER_COLORS
                .iter()
                .map(|color| Self::Shulker(color)),
        );
        all
    }

    /// Emits the placed model at the origin block.
    fn emit(self, builder: &mut MeshBuilder, atlas: &BlockEntityAtlas) {
        let block = [0; 3];
        match self {
            Self::Chest(key) => chest::emit(
                builder,
                atlas,
                block,
                &ChestModel {
                    variant: key.variant(),
                    facing: Facing::South,
                    pair: ChestPair::Single,
                    lid: 0.0,
                },
            ),
            Self::Shulker(color) => shulker::emit(
                builder,
                atlas,
                block,
                &ShulkerModel {
                    color,
                    facing: 1,
                    open: 0.0,
                },
            ),
            Self::DecoratedPot => pot::emit(
                builder,
                atlas,
                block,
                &DecoratedPotModel {
                    facing: Facing::South,
                    sherds: Default::default(),
                },
            ),
            Self::Conduit => conduit::emit(
                builder,
                atlas,
                block,
                &ConduitModel {
                    active: false,
                    hunting: false,
                    viewer_yaw_degrees: 0.0,
                },
                SceneClock { ticks: 0.0 },
            ),
        }
    }
}

/// The held model of `kind` as one-bone rig geometry: its solid faces centred on the origin,
/// with UVs in the atlas's static region. `None` when the atlas lacks its textures.
#[must_use]
pub fn held_block_entity_geometry(
    id: EntityRigId,
    atlas: &BlockEntityAtlas,
    kind: HeldBlockEntity,
) -> Option<ActorRigGeometry> {
    let [_, atlas_height] = atlas.size();
    let static_height = atlas.static_height();
    if static_height == 0 {
        return None;
    }
    if kind == HeldBlockEntity::DecoratedPot {
        use assets::block_entity_geometry as geometry;
        atlas.texture(geometry::POT_BASE_TEXTURE.0, geometry::POT_BASE_TEXTURE.1)?;
        atlas.texture(geometry::POT_SIDE_TEXTURE.0, geometry::POT_SIDE_TEXTURE.1)?;
    }
    let mut builder = MeshBuilder::new(atlas.size());
    // The actor rig needs face normals; its submission supplies the actual light.
    builder.actor_light = crate::pack_actor_light(0, 0);
    kind.emit(&mut builder, atlas);
    if builder.solid.is_empty() {
        return None;
    }
    let (min, max) = builder.solid.iter().fold(
        (
            glam::Vec3::splat(f32::INFINITY),
            glam::Vec3::splat(f32::NEG_INFINITY),
        ),
        |(min, max), vertex| {
            let position = glam::Vec3::from(vertex.position);
            (min.min(position), max.max(position))
        },
    );
    let center = (min + max) * 0.5;
    let v_scale = atlas_height as f32 / static_height as f32;
    let mut vertices = Vec::with_capacity(builder.solid.len());
    for vertex in &builder.solid {
        let uv = [vertex.uv[0], vertex.uv[1] * v_scale];
        if uv[1] > 1.0 {
            // Animated-region texels have no place in the static raster.
            return None;
        }
        vertices.push(ActorRigVertex {
            position: (glam::Vec3::from(vertex.position) - center).to_array(),
            normal: vertex.normal,
            uv,
            back_uv: uv,
            bone_index: 0,
            surface: ActorRigSurface::SINGLE_FACE,
            glint_origin: [0.0, 0.0],
        });
    }
    ActorRigGeometry::new(id, Arc::from(vertices), Arc::from(vec![[0.0, 0.0, 0.0]])).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorated_pot_requires_base_and_side_artwork() {
        use assets::block_entity_geometry as geometry;

        for names in [
            vec![geometry::POT_BASE_TEXTURE.0],
            vec![geometry::POT_SIDE_TEXTURE.0],
            vec![geometry::POT_BASE_TEXTURE.0, geometry::POT_SIDE_TEXTURE.0],
        ] {
            let mut placements = names
                .iter()
                .enumerate()
                .map(|(index, name)| assets::BlockEntityPlacement {
                    name: (*name).into(),
                    x: 0,
                    y: index as u32 * 64,
                    width: 64,
                    height: 64,
                })
                .collect::<Vec<_>>();
            placements.sort_by(|left, right| left.name.cmp(&right.name));
            let height = 64 * placements.len() as u32;
            let bytes = assets::encode_block_entity_catalog(
                b"{}",
                64,
                height,
                &vec![255; (64 * height * 4) as usize],
                &placements,
            )
            .unwrap();
            let atlas = BlockEntityAtlas::from_assets(
                &assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap(),
            );
            assert_eq!(
                held_block_entity_geometry(EntityRigId(1), &atlas, HeldBlockEntity::DecoratedPot)
                    .is_some(),
                names.len() == 2,
                "partial pot artwork must retain the sprite fallback: {names:?}"
            );
        }
    }

    #[test]
    fn held_models_have_finite_unit_face_normals() {
        use assets::block_entity_geometry as geometry;
        use glam::Vec3;

        let textures = [
            "textures/entity/chest/normal".to_owned(),
            geometry::POT_BASE_TEXTURE.0.to_owned(),
            geometry::POT_SIDE_TEXTURE.0.to_owned(),
            geometry::CONDUIT_TEXTURE.0.to_owned(),
        ]
        .into_iter()
        .chain(geometry::SHULKER_COLORS.map(geometry::shulker_texture));
        let side = 64;
        let mut placements = textures
            .enumerate()
            .map(|(index, name)| assets::BlockEntityPlacement {
                name: name.into(),
                x: 0,
                y: index as u32 * side,
                width: side,
                height: side,
            })
            .collect::<Vec<_>>();
        placements.sort_by(|left, right| left.name.cmp(&right.name));
        let height = side * placements.len() as u32;
        let encoded = assets::encode_block_entity_catalog(
            b"{}",
            side,
            height,
            &vec![255; (side * height * 4) as usize],
            &placements,
        )
        .unwrap();
        let atlas = BlockEntityAtlas::from_assets(
            &assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap(),
        );
        let kinds = [
            HeldBlockEntity::Chest(ChestVariantKey::Normal),
            HeldBlockEntity::DecoratedPot,
            HeldBlockEntity::Conduit,
        ]
        .into_iter()
        .chain(geometry::SHULKER_COLORS.map(HeldBlockEntity::Shulker));
        for kind in kinds {
            let model = held_block_entity_geometry(EntityRigId(1), &atlas, kind).unwrap();
            let (triangles, remainder) = model.vertices.as_chunks::<3>();
            assert!(remainder.is_empty());
            for triangle in triangles {
                let [a, b, c] = triangle.map(|vertex| Vec3::from_array(vertex.position));
                let face_normal = (c - a).cross(b - a).normalize();
                for vertex in triangle {
                    let normal = Vec3::from_array(vertex.normal);
                    assert!(normal.is_finite(), "{kind:?}");
                    assert!((normal.length_squared() - 1.0).abs() < 1.0e-5, "{kind:?}");
                    assert!(normal.abs_diff_eq(face_normal, 1.0e-5), "{kind:?}");
                }
            }
            if kind == HeldBlockEntity::DecoratedPot {
                let bottom = model
                    .vertices
                    .iter()
                    .map(|vertex| vertex.position[1])
                    .fold(f32::INFINITY, f32::min);
                let bottom_triangles = triangles
                    .iter()
                    .filter(|triangle| triangle.iter().all(|vertex| vertex.position[1] == bottom))
                    .collect::<Vec<_>>();
                assert!(!bottom_triangles.is_empty());
                for triangle in bottom_triangles {
                    assert!(triangle.iter().all(|vertex| {
                        Vec3::from_array(vertex.normal).abs_diff_eq(Vec3::NEG_Y, 1.0e-5)
                    }));
                }
            }
        }
    }

    #[test]
    fn items_resolve_their_held_models() {
        assert_eq!(
            HeldBlockEntity::from_item("minecraft:chest"),
            Some(HeldBlockEntity::Chest(ChestVariantKey::Normal))
        );
        assert_eq!(
            HeldBlockEntity::from_item("minecraft:waxed_oxidized_copper_chest"),
            Some(HeldBlockEntity::Chest(ChestVariantKey::Copper(3)))
        );
        assert_eq!(
            HeldBlockEntity::from_item("minecraft:ender_chest"),
            Some(HeldBlockEntity::Chest(ChestVariantKey::Ender))
        );
        assert_eq!(
            HeldBlockEntity::from_item("minecraft:red_shulker_box"),
            Some(HeldBlockEntity::Shulker("red"))
        );
        assert_eq!(HeldBlockEntity::from_item("minecraft:stone"), None);
        let items = [
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:ender_chest",
            "minecraft:copper_chest",
            "minecraft:exposed_copper_chest",
            "minecraft:weathered_copper_chest",
            "minecraft:oxidized_copper_chest",
            "minecraft:decorated_pot",
            "minecraft:conduit",
        ]
        .into_iter()
        .map(String::from)
        .chain(
            assets::block_entity_geometry::SHULKER_COLORS
                .iter()
                .map(|color| format!("minecraft:{color}_shulker_box")),
        );
        let reached: std::collections::BTreeSet<_> = items
            .filter_map(|identifier| HeldBlockEntity::from_item(&identifier))
            .collect();
        assert_eq!(
            reached,
            HeldBlockEntity::all().into_iter().collect(),
            "every held model is reached by its item"
        );
        for kind in HeldBlockEntity::all() {
            if let HeldBlockEntity::Chest(key) = kind {
                assert_eq!(ChestVariantKey::of(key.variant()), key);
            }
        }
    }
}
