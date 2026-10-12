//! Real held meshes and authored hand placements, independent of GUI icon projection.

use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use assets::{
    EquipmentCategory, ItemDisplayScalar, ItemVisualDefinitionRoute, ItemVisualKey,
    RuntimeEntityAssets, RuntimeEquipmentCatalog, RuntimeIconCatalog,
};
use render_model::{RenderBoneTransform, held_sprite_vertices, textured_cube_vertices};

use player_preview::{PreviewHeldModel, PreviewHeldPlacement};
use {
    super::{UiPresentationError, atlas, player_preview},
    ui::IconRef,
};

/// One item's held model. A sprite item's extruded mesh is built the first time a preview holds
/// it, since every item has one and most are never held.
pub(super) struct HeldModel {
    model: PreviewHeldModel,
    /// The item sprite extruded on first use; `None` for models built up front.
    sprite: Option<usize>,
    extruded: OnceLock<Option<Arc<[render_model::ActorRigVertex]>>>,
}

impl HeldModel {
    const fn ready(model: PreviewHeldModel) -> Self {
        Self {
            model,
            sprite: None,
            extruded: OnceLock::new(),
        }
    }

    /// The model with its mesh, extruding a sprite item's sprite from `icons` on first use;
    /// `None` when that sprite has no mesh.
    pub(super) fn model(&self, icons: &RuntimeIconCatalog) -> Option<PreviewHeldModel> {
        let Some(sprite) = self.sprite else {
            return Some(self.model.clone());
        };
        let vertices = self
            .extruded
            .get_or_init(|| {
                let sprite = icons.sprites().get(sprite)?;
                held_sprite_vertices(
                    usize::from(sprite.width),
                    usize::from(sprite.height),
                    &sprite.rgba8,
                    [0.0, 0.0, 1.0, 1.0],
                )
                .map(Into::into)
            })
            .clone()?;
        Some(PreviewHeldModel {
            vertices,
            ..self.model.clone()
        })
    }
}

pub(super) fn prepare(
    atlas: &mut atlas::Atlas,
    entities: &RuntimeEntityAssets,
    icons: &RuntimeIconCatalog,
    icon_refs: &[IconRef],
    equipment: Option<&RuntimeEquipmentCatalog>,
    blocks: &BTreeMap<u32, IconRef>,
) -> Result<BTreeMap<ItemVisualKey, HeldModel>, UiPresentationError> {
    let Some(hand_pivots) = player_hands(entities) else {
        // A model without native item bones cannot safely manufacture a grip origin.
        return Ok(BTreeMap::new());
    };
    let mut models = BTreeMap::new();
    for entry in icons.entries() {
        let definition = entities.item_visuals().iter().find(|visual| {
            visual.key.identifier == entry.identifier && visual.key.metadata == entry.metadata
        });
        let model = match definition.map(|definition| definition.route) {
            Some(ItemVisualDefinitionRoute::BlockItem { block_visual })
                if blocks.contains_key(&block_visual.0) =>
            {
                Some((block(blocks[&block_visual.0], hand_pivots), None))
            }
            Some(ItemVisualDefinitionRoute::EmptyHand) => None,
            _ => {
                if icons.sprites().get(entry.sprite as usize).is_none() {
                    continue;
                }
                // Sprite texels already live in the item atlas; keep their original region.
                let source = *icon_refs
                    .get(entry.sprite as usize)
                    .ok_or(UiPresentationError::InvalidFontTexture)?;
                let model = PreviewHeldModel {
                    source,
                    hand_pivots,
                    vertices: Arc::from([]),
                    placements: [PreviewHeldPlacement::Sprite {
                        hand_equipped: definition
                            .and_then(|definition| definition.grip.upright())
                            .unwrap_or(false),
                    }; 2],
                };
                Some((model, Some(entry.sprite as usize)))
            }
        };
        if let Some((model, sprite)) = model {
            models.insert(
                ItemVisualKey {
                    identifier: entry.identifier.clone(),
                    metadata: entry.metadata,
                },
                HeldModel {
                    sprite,
                    ..HeldModel::ready(model.with_glint(
                        Some(
                            definition.is_some_and(|definition| {
                                definition.glint.enabled().unwrap_or(false)
                            }),
                        ),
                    ))
                },
            );
        }
    }
    if let Some(equipment) = equipment {
        for binding in equipment.bindings() {
            let Some(model) = authored(atlas, entities, equipment, binding, hand_pivots)? else {
                continue;
            };
            models.insert(
                ItemVisualKey {
                    identifier: binding.identifier.clone(),
                    metadata: 0,
                },
                HeldModel::ready(model.with_glint(Some(entities.item_visuals().iter().any(
                    |definition| {
                        definition.key.identifier == binding.identifier
                            && definition.key.metadata == 0
                            && definition.glint.enabled().unwrap_or(false)
                    },
                )))),
            );
        }
    }
    Ok(models)
}

fn player_hands(entities: &RuntimeEntityAssets) -> Option<[[f32; 3]; 2]> {
    let binding = entities.rig_bindings().iter().find(|binding| {
        entities
            .symbols()
            .get(binding.entity_symbol as usize)
            .is_some_and(|symbol| {
                symbol.kind == assets::EntityAssetKind::Entity
                    && &*symbol.identifier == "minecraft:player"
            })
    })?;
    let geometry = entities
        .rig_geometries()
        .get(binding.first_geometry as usize)?
        .geometry as usize;
    let names = render_model::geometry_bone_names(entities, geometry)?;
    let pivots = render_model::geometry_bone_pivots(entities, geometry)?;
    let hand = |name: &str| {
        let index = names
            .iter()
            .position(|bone| bone.eq_ignore_ascii_case(name))?;
        pivots.get(index).copied()
    };
    Some([hand("rightItem")?, hand("leftItem")?])
}

fn block(source: IconRef, hand_pivots: [[f32; 3]; 2]) -> PreviewHeldModel {
    // Share the same six-face cube and UV contract as the world held-item renderer.
    let faces = super::sheet_faces(source).map(|face| {
        [
            f32::from(face.uv[0] - source.uv[0]) / f32::from(source.uv[2] - source.uv[0]),
            f32::from(face.uv[1] - source.uv[1]) / f32::from(source.uv[3] - source.uv[1]),
            f32::from(face.uv[2] - source.uv[0]) / f32::from(source.uv[2] - source.uv[0]),
            f32::from(face.uv[3] - source.uv[1]) / f32::from(source.uv[3] - source.uv[1]),
        ]
    });
    PreviewHeldModel {
        source,
        hand_pivots,
        vertices: textured_cube_vertices(faces).into(),
        placements: [PreviewHeldPlacement::Block; 2],
    }
}

fn authored(
    atlas: &mut atlas::Atlas,
    entities: &RuntimeEntityAssets,
    equipment: &RuntimeEquipmentCatalog,
    binding: &assets::EquipmentBinding,
    hand_pivots: [[f32; 3]; 2],
) -> Result<Option<PreviewHeldModel>, UiPresentationError> {
    use render_model::equipment::{BoneChannels, attach};
    let channels = |off_hand: bool| -> Option<BoneChannels> {
        match binding.category {
            EquipmentCategory::Held => {
                let transform = binding.third_person.literal()?;
                Some(BoneChannels {
                    translation: transform.translation.map(ItemDisplayScalar::get),
                    rotation: transform.rotation.map(ItemDisplayScalar::get),
                    scale: transform.scale.map(ItemDisplayScalar::get),
                })
            }
            EquipmentCategory::Shield => {
                let slot = if off_hand { "off_hand" } else { "main_hand" };
                let pose = binding.pose(&format!("wield_third_person@{slot}"))?;
                let [bone] = &*pose.bones else { return None };
                let channel = |value: Option<[ItemDisplayScalar; 3]>, rest: f32| {
                    value.map_or([rest; 3], |value| value.map(ItemDisplayScalar::get))
                };
                Some(BoneChannels {
                    translation: channel(bone.translation, 0.0),
                    rotation: channel(bone.rotation, 0.0),
                    scale: channel(bone.scale, 1.0),
                })
            }
            _ => None,
        }
    };
    let (Some(main), Some(off)) = (channels(false), channels(true)) else {
        return Ok(None);
    };
    let Some(index) = entities
        .geometries()
        .iter()
        .position(|geometry| geometry.identifier == binding.geometry.identifier)
    else {
        return Ok(None);
    };
    let Ok(bones) = render_model::resolve_geometry_bones(entities, index) else {
        return Ok(None);
    };
    let [root] = &*bones else {
        return Ok(None);
    };
    let Some(texture) = equipment.texture(&binding.texture.identifier) else {
        return Ok(None);
    };
    let Ok(geometry) = render_model::attachable_geometry(
        entities,
        index,
        render_model::item_mesh_rig_id(0),
        texture,
    ) else {
        return Ok(None);
    };
    let [pivot] = &*geometry.bone_pivots else {
        return Ok(None);
    };
    let identity = RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0, 0.0, 0.0, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let (Some(main), Some(off)) = (
        attach(identity, *pivot, main, root.binding.is_some()),
        attach(identity, *pivot, off, root.binding.is_some()),
    ) else {
        return Ok(None);
    };
    let source = atlas.insert([texture.width, texture.height], &texture.rgba8)?;
    Ok(Some(PreviewHeldModel {
        source,
        hand_pivots,
        vertices: Arc::clone(&geometry.vertices),
        placements: [main, off].map(|bone| PreviewHeldPlacement::Authored {
            bone,
            pivot: *pivot,
        }),
    }))
}

#[cfg(test)]
mod tests;
