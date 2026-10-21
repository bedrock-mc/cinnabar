//! Block-entity items held in hand: chests, shulker boxes, decorated pots, conduits and heads drawn
//! as their placed models, closed and at rest, centred in the unit block the held cube uses.
//!
//! Provisional: the native item renderer's pose for these models (front facing and lid state)
//! is unmeasured; they face south like placed blocks with default facing.

use std::sync::Arc;

mod artwork;
pub use artwork::PreparedHeldBlockModels;

use super::{
    atlas::{AtlasRect, BlockEntityAtlas},
    chest::{self, ChestModel, ChestPair, ChestVariant},
    conduit::{self, ConduitModel},
    heads::HeadModels,
    mesh::{BlockEntityVertex, Facing, MeshBuilder},
    pot::{self, DecoratedPotModel},
    scene::SceneClock,
    shulker::{self, ShulkerModel},
    skull::{self, SkullKind, SkullModel, SkullMount},
};
use crate::{ActorRigGeometry, ActorRigSurface, ActorRigVertex, EntityRigId};

/// A held block-entity rig and the shared span that normalizes its texture-local glint UVs.
#[derive(Clone, Debug)]
pub struct HeldBlockEntityModel {
    pub geometry: ActorRigGeometry,
    pub glint_span: [f32; 2],
}

/// A block-entity item with a held model.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum HeldBlockEntity {
    Chest(ChestVariantKey),
    Shulker(&'static str),
    DecoratedPot,
    Conduit,
    Skull(SkullKind),
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
        use assets::facts::{BlockEntityFacts, BlockEntityRenderer};
        if let Some(kind) = SkullKind::from_block_identifier(identifier) {
            return Some(Self::Skull(kind));
        }
        let renderer = BlockEntityFacts::pinned()
            .by_block_identifier(identifier)
            .map(|entry| entry.renderer);
        if renderer == Some(BlockEntityRenderer::EnderChest) {
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
        match renderer? {
            BlockEntityRenderer::DecoratedPot => Some(Self::DecoratedPot),
            BlockEntityRenderer::Conduit => Some(Self::Conduit),
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
        all.extend(SkullKind::kinds().map(Self::Skull));
        all
    }

    /// Emits the placed model at the origin block.
    fn emit(self, builder: &mut MeshBuilder, atlas: &BlockEntityAtlas, heads: &HeadModels) {
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
            Self::Skull(kind) => skull::emit(
                builder,
                atlas,
                heads,
                block,
                &SkullModel {
                    kind,
                    mount: SkullMount::Floor {
                        rotation_degrees: 0.0,
                    },
                },
            ),
        }
    }
}

/// Builds a centered one-bone rig with static atlas UVs and texture-local glint coordinates.
/// Returns `None` when the atlas lacks the model's textures.
#[must_use]
pub fn held_block_entity_model(
    id: EntityRigId,
    atlas: &BlockEntityAtlas,
    kind: HeldBlockEntity,
) -> Option<HeldBlockEntityModel> {
    model_with_heads(id, atlas, kind, &HeadModels::default())
}

/// Builds the held model with the same entity-derived head geometry as placed skulls.
fn model_with_heads(
    id: EntityRigId,
    atlas: &BlockEntityAtlas,
    kind: HeldBlockEntity,
    heads: &HeadModels,
) -> Option<HeldBlockEntityModel> {
    let [atlas_width, atlas_height] = atlas.size();
    let static_height = atlas.static_height();
    if static_height == 0 {
        return None;
    }
    let mut builder = MeshBuilder::new(atlas.size());
    // The actor rig needs face normals; its submission supplies the actual light.
    builder.actor_light = crate::pack_actor_light(0, 0);
    kind.emit(&mut builder, atlas, heads);
    if builder.missing_required_texture || builder.solid.is_empty() {
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
    let (faces, remainder) = builder.solid.as_chunks::<6>();
    if !remainder.is_empty() {
        return None;
    }
    let first_texture = face_texture(atlas, faces.first()?)?;
    let glint_span = [
        first_texture.width / atlas_width as f32,
        first_texture.height / static_height as f32,
    ];
    let mut vertices = Vec::with_capacity(builder.solid.len());
    for face in faces {
        let texture = face_texture(atlas, face)?;
        let origin = [
            texture.x / atlas_width as f32,
            texture.y / static_height as f32,
        ];
        let span = [
            texture.width / atlas_width as f32,
            texture.height / static_height as f32,
        ];
        for vertex in face {
            let uv = [vertex.uv[0], vertex.uv[1] * v_scale];
            if uv[1] > 1.0 {
                // Animated-region texels have no place in the static raster.
                return None;
            }
            // Different-sized textures share one draw: subtract an offset that puts each
            // vertex's texture-local coordinates on the draw's common glint span.
            let glint_origin = std::array::from_fn(|axis| {
                let local = (uv[axis] - origin[axis]) / span[axis];
                uv[axis] - local * glint_span[axis]
            });
            vertices.push(ActorRigVertex {
                position: (glam::Vec3::from(vertex.position) - center).to_array(),
                normal: vertex.normal,
                uv,
                back_uv: uv,
                bone_index: 0,
                surface: ActorRigSurface::SINGLE_FACE,
                glint_origin,
            });
        }
    }
    let geometry =
        ActorRigGeometry::new(id, Arc::from(vertices), Arc::from(vec![[0.0; 3]])).ok()?;
    Some(HeldBlockEntityModel {
        geometry,
        glint_span,
    })
}

#[cfg(test)]
#[path = "held/residency_tests.rs"]
mod residency_tests;

/// Resolves an emitted face's texture from its interior UV point, before static UV rescaling.
fn face_texture(atlas: &BlockEntityAtlas, face: &[BlockEntityVertex; 6]) -> Option<AtlasRect> {
    let size = atlas.size().map(|side| side as f32);
    let pixel = std::array::from_fn(|axis| {
        face.iter().map(|vertex| vertex.uv[axis]).sum::<f32>() / face.len() as f32 * size[axis]
    });
    atlas.static_texture_at(pixel)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds synthetic model textures with independently varied packing and resolution.
    fn glint_atlas(
        size: [u32; 2],
        offset: [u32; 2],
        scale: [u32; 2],
        pot_side_scale: [u32; 2],
    ) -> BlockEntityAtlas {
        use assets::block_entity_geometry as geometry;

        let textures = [
            ("textures/entity/chest/normal".to_owned(), [64, 64]),
            (geometry::shulker_texture("red"), [64, 64]),
            (geometry::POT_BASE_TEXTURE.0.to_owned(), [32, 32]),
            (geometry::POT_SIDE_TEXTURE.0.to_owned(), [16, 16]),
            (geometry::CONDUIT_TEXTURE.0.to_owned(), [24, 12]),
        ];
        let mut placements = textures
            .into_iter()
            .enumerate()
            .map(|(index, (name, [width, height]))| {
                let texture_scale = if name == geometry::POT_SIDE_TEXTURE.0 {
                    pot_side_scale
                } else {
                    scale
                };
                assets::BlockEntityPlacement {
                    name: name.into(),
                    x: offset[0],
                    y: offset[1] + index as u32 * 64 * scale[1],
                    width: width * texture_scale[0],
                    height: height * texture_scale[1],
                }
            })
            .collect::<Vec<_>>();
        placements.sort_by(|left, right| left.name.cmp(&right.name));
        let bytes = assets::encode_block_entity_catalog(
            b"{}",
            size[0],
            size[1],
            &vec![255; (size[0] * size[1] * 4) as usize],
            &placements,
        )
        .unwrap();
        BlockEntityAtlas::from_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap())
    }

    #[test]
    fn glint_coordinates_are_independent_of_atlas_packing_and_texture_resolution() {
        use std::collections::BTreeMap;

        let mut reference = BTreeMap::new();
        for (size, offset, scale, pot_side_scale) in [
            ([128, 512], [0, 0], [1, 1], [1, 1]),
            ([1024, 1024], [128, 128], [1, 1], [3, 2]),
            ([1024, 2048], [384, 256], [2, 3], [4, 1]),
        ] {
            let atlas = glint_atlas(size, offset, scale, pot_side_scale);
            for kind in [
                HeldBlockEntity::Chest(ChestVariantKey::Normal),
                HeldBlockEntity::Shulker("red"),
                HeldBlockEntity::DecoratedPot,
                HeldBlockEntity::Conduit,
            ] {
                let model = held_block_entity_model(EntityRigId(1), &atlas, kind).unwrap();
                let glint = crate::ActorGlint::item(0.0, model.glint_span);
                let coordinates = model
                    .geometry
                    .vertices
                    .iter()
                    .map(|vertex| {
                        std::array::from_fn::<_, 2, _>(|axis| {
                            (vertex.uv[axis] - vertex.glint_origin[axis]) * glint.uv_scale[axis]
                        })
                    })
                    .collect::<Vec<_>>();
                let expected = reference.entry(kind).or_insert_with(|| coordinates.clone());
                assert_eq!(coordinates.len(), expected.len());
                for (actual, expected) in coordinates.iter().zip(expected.iter()) {
                    assert!(
                        glam::Vec2::from_array(*actual)
                            .abs_diff_eq(glam::Vec2::from_array(*expected), 1.0e-5),
                        "{kind:?}, size {size:?}, offset {offset:?}, scale {scale:?}"
                    );
                }
                if kind == HeldBlockEntity::Chest(ChestVariantKey::Normal) {
                    let span = [
                        64.0 * scale[0] as f32 / size[0] as f32,
                        64.0 * scale[1] as f32 / size[1] as f32,
                    ];
                    assert_eq!(model.glint_span, span);
                    for vertex in &*model.geometry.vertices {
                        let origin = [
                            offset[0] as f32 / size[0] as f32,
                            offset[1] as f32 / size[1] as f32,
                        ];
                        assert!(
                            glam::Vec2::from_array(vertex.glint_origin)
                                .abs_diff_eq(glam::Vec2::from_array(origin), 1.0e-5)
                        );
                    }
                }
            }
        }
    }

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
                held_block_entity_model(EntityRigId(1), &atlas, HeldBlockEntity::DecoratedPot)
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
            let model = held_block_entity_model(EntityRigId(1), &atlas, kind)
                .unwrap()
                .geometry;
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
    fn decorated_pots_require_both_base_and_side_textures() {
        use assets::block_entity_geometry::{POT_BASE_TEXTURE, POT_SIDE_TEXTURE};
        for names in [
            vec![POT_BASE_TEXTURE],
            vec![POT_SIDE_TEXTURE],
            vec![POT_BASE_TEXTURE, POT_SIDE_TEXTURE],
        ] {
            let complete = names.len() == 2;
            let mut placements: Vec<_> = names
                .into_iter()
                .enumerate()
                .map(|(index, (name, size))| assets::BlockEntityPlacement {
                    name: name.into(),
                    x: 0,
                    y: index as u32 * 64,
                    width: size[0] as u32,
                    height: size[1] as u32,
                })
                .collect();
            placements.sort_by(|left, right| left.name.cmp(&right.name));
            let encoded = assets::encode_block_entity_catalog(
                b"{}",
                64,
                128,
                &vec![255; 64 * 128 * 4],
                &placements,
            )
            .unwrap();
            let atlas = BlockEntityAtlas::from_assets(
                &assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap(),
            );
            let model =
                held_block_entity_model(EntityRigId(1), &atlas, HeldBlockEntity::DecoratedPot);
            assert_eq!(
                model.is_some(),
                complete,
                "partial pot art must retain the item fallback"
            );
            if let Some(model) = model {
                assert!(model.geometry.vertices.len() > 36);
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
        )
        .chain(
            assets::read_registry_for_protocol(
                assets::pinned_block_registry_bytes(),
                assets::active_content_registry_protocol(),
            )
            .unwrap()
            .into_iter()
            .filter(|block| SkullKind::from_block_identifier(&block.name).is_some())
            .map(|block| block.name.to_string()),
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
