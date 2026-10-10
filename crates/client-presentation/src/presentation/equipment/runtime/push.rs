//! Per-category equipment layer builders for a drawn body.

use super::*;

impl EquipmentRuntime {
    pub(super) fn push_held(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        layer: u8,
        hand: Option<usize>,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        if self.push_attachable(body, item, layer, hand, layer == LAYER_OFF_HAND, layers) {
            return;
        }
        self.push_attached(body, item, layer, hand, None, layers);
    }

    /// A held item whose attachable ships its own single-bone geometry and a literal
    /// third-person placement (trident, shield). `false` when the item is not one, or its
    /// placement is Molang-driven (then the sprite/cube path draws it).
    pub(super) fn push_attachable(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        layer: u8,
        hand: Option<usize>,
        off_hand: bool,
        layers: &mut Vec<EquipmentPresentation>,
    ) -> bool {
        let Some(hand) = hand else {
            return false;
        };
        let Some((catalog, from_pack)) = self.binding_source(&item.identifier) else {
            return false;
        };
        let Some(binding) = catalog.binding(&item.identifier) else {
            return false;
        };
        let channels = match self.effective_category(&item.identifier, binding.category) {
            EquipmentCategory::Held => {
                binding
                    .third_person
                    .literal()
                    .map(|transform| BoneChannels {
                        translation: transform.translation.map(|value| value.get()),
                        rotation: transform.rotation.map(|value| value.get()),
                        scale: transform.scale.map(|value| value.get()),
                    })
            }
            EquipmentCategory::Shield => {
                let slot = if off_hand { "off_hand" } else { "main_hand" };
                binding
                    .pose(&format!("wield_third_person@{slot}"))
                    .and_then(|pose| pose.bones.first())
                    .map(|bone| {
                        let channel = |value: Option<[assets::ItemDisplayScalar; 3]>, rest: f32| {
                            value.map_or([rest; 3], |value| value.map(|scalar| scalar.get()))
                        };
                        BoneChannels {
                            translation: channel(bone.translation, 0.0),
                            rotation: channel(bone.rotation, 0.0),
                            scale: channel(bone.scale, 1.0),
                        }
                    })
            }
            _ => None,
        };
        let Some(channels) = channels else {
            return false;
        };
        let Some(location) = self.texture_location(&binding.texture.identifier, from_pack) else {
            return false;
        };
        let Some(geometry) = self.armor_geometry_for(&binding.geometry.identifier, from_pack)
        else {
            return false;
        };
        // Only single-bone models are placed; a hierarchy needs its parent chain composed.
        let ([pivot], [has_binding_expression]) =
            (&geometry.pivots[..], &geometry.binding_expressions[..])
        else {
            return false;
        };
        let (Some(previous), Some(current)) = (
            body.input.previous_bones.get(hand),
            body.input.current_bones.get(hand),
        ) else {
            return false;
        };
        let (Some(previous), Some(current)) = (
            attachable::attach(*previous, *pivot, channels, *has_binding_expression),
            attachable::attach(*current, *pivot, channels, *has_binding_expression),
        ) else {
            return false;
        };
        let poses = self.poses.share(body, layer, [&[previous], &[current]]);
        layers.push(layer_presentation(
            body,
            layer,
            geometry.rig,
            poses,
            location,
            0,
        ));
        true
    }

    /// The held item's generated mesh and artwork, and whether the mesh is a block cube. A
    /// server pack's icon replaces the vanilla one, as it does in the inventory, and a custom
    /// block item with a cube sheet is held as that cube. With `icon_fallback`, a block with no
    /// plain cube sheet takes its icon sprite instead.
    pub(super) fn held_mesh(
        &mut self,
        item: &WornItem,
        icon_fallback: bool,
    ) -> Option<(EntityRigId, ActorArtworkLocation, bool)> {
        let session = match item.kind {
            HeldKind::Sprite | HeldKind::Other => {
                self.session_held(&item.identifier, item.metadata)
            }
            HeldKind::Block(_) => None,
        };
        let (index, key, placement, location) = if let Some(held) = session {
            held
        } else {
            let sheet = match item.kind {
                HeldKind::Block(visual) => self
                    .block_sheets
                    .get(&visual)
                    .map(|index| (*index, MeshKey::Block(visual))),
                _ => None,
            };
            let (index, key) = match (sheet, item.kind) {
                (Some(sheet), _) => sheet,
                (None, HeldKind::Other) => return None,
                (None, HeldKind::Block(_)) if !icon_fallback => return None,
                (None, _) => {
                    let index = self.icons.lookup_index(&item.identifier, item.metadata)?;
                    (index, MeshKey::Sprite(index))
                }
            };
            let placement = self.placements.get(index).copied().flatten()?;
            let location = self
                .atlas_locations
                .get(placement.layer)
                .copied()
                .flatten()?;
            (index, key, placement, location)
        };
        let mesh = self.mesh_for(key, index, placement)?;
        let block = matches!(key, MeshKey::Block(_) | MeshKey::SessionBlock(_));
        Some((mesh, location, block))
    }

    /// A held or worn sprite/cube on `bone`, placed by `display` or the kind's held placement.
    pub(super) fn push_attached(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        layer: u8,
        bone: Option<usize>,
        override_display: Option<ItemDisplay>,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let Some(hand) = bone else {
            return;
        };
        let Some((mesh, location, block)) = self.held_mesh(item, override_display.is_none()) else {
            return;
        };
        let display = override_display.unwrap_or_else(|| {
            if block {
                held_block_display()
            } else {
                held_sprite_display(self.hand_equipped(&item.identifier))
            }
        });
        let (Some(previous), Some(current)) = (
            body.input.previous_bones.get(hand),
            body.input.current_bones.get(hand),
        ) else {
            return;
        };
        let (Some(previous), Some(current)) = (
            attach_to_bone(*previous, display),
            attach_to_bone(*current, display),
        ) else {
            return;
        };
        let poses = self.poses.share(body, layer, [&[previous], &[current]]);
        layers.push(layer_presentation(body, layer, mesh, poses, location, 0));
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_armor(
        &mut self,
        body: &ActorRigSubmission,
        bones: &BodyBones,
        body_geometry: u32,
        (slot, layer): (ArmorSlot, u8),
        item: &WornItem,
        (equipment, animation): (&ActorEquipmentInput, Option<EquipmentAnimation<'_>>),
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let Some((catalog, from_pack)) = self.binding_source(&item.identifier) else {
            return;
        };
        let Some(binding) = catalog.binding(&item.identifier) else {
            return;
        };
        // A custom item's `minecraft:wearable` slot names where its attachable is worn.
        let category = self.effective_category(&item.identifier, binding.category);
        if category != (EquipmentCategory::Armor { slot }) {
            return;
        }
        let selected = animation
            .filter(|_| from_pack)
            .and_then(|animation| self.worn_texture(item, slot, equipment, animation));
        let Some(location) =
            selected.or_else(|| self.texture_location(&binding.texture.identifier, from_pack))
        else {
            return;
        };
        let Some(geometry) = self.armor_geometry_for(&binding.geometry.identifier, from_pack)
        else {
            return;
        };
        let map = Arc::clone(
            self.armor_maps
                .entry((
                    body_geometry,
                    if from_pack {
                        format!(
                            "{}{}",
                            pack::ARMOR_CACHE_PREFIX,
                            binding.geometry.identifier
                        )
                        .into()
                    } else {
                        binding.geometry.identifier.clone()
                    },
                ))
                .or_insert_with(|| bone_map(&geometry.names, &bones.names).into()),
        );
        let tint = binding.color_mask_rgb(item.dye_rgb).map_or(0, pack_tint);
        let (previous, current) = (
            remap_pose(&map, &body.input.previous_bones),
            remap_pose(&map, &body.input.current_bones),
        );
        let poses = self.poses.share(body, layer, [&previous, &current]);
        layers.push(layer_presentation(
            body,
            layer,
            geometry.rig,
            poses,
            location,
            tint,
        ));
    }

    /// The texture a pack attachable's render controller selects for a worn piece, such as a
    /// team colour read from its owner; `None` keeps the binding's default texture.
    fn worn_texture(
        &mut self,
        item: &WornItem,
        slot: ArmorSlot,
        equipment: &ActorEquipmentInput,
        animation: EquipmentAnimation<'_>,
    ) -> Option<ActorArtworkLocation> {
        let pack = self.pack.as_mut()?;
        let variables = [("variable.is_enchanted", f32::from(item.enchanted))];
        let input = equipment.attachable_input(client_world::AttachableAnimationInput {
            worn: true,
            worn_slot: slot as u8,
            frame_alpha: animation.frame_alpha,
            owner_variables: &variables,
            ..Default::default()
        });
        let evaluated =
            pack.attachables
                .evaluate(&item.identifier, animation.owner, animation.rig, input)?;
        let layer = evaluated.render.iter().find(|layer| layer.texture_slot == 0)?;
        let path = &pack.assets.sources().get(layer.source as usize)?.path;
        let texture: Box<str> = path
            .strip_suffix(".png")
            .or_else(|| path.strip_suffix(".tga"))?
            .into();
        self.texture_location(&texture, true)
    }

    /// A worn head riding the body's head bone.
    pub(super) fn push_skull(
        &mut self,
        body: &ActorRigSubmission,
        kind: SkullKind,
        layer: u8,
        head: Option<usize>,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let Some(&(rig, location)) = self.skulls.get(&kind_index(kind)) else {
            return;
        };
        let Some(head) = head else {
            return;
        };
        let (Some(previous), Some(current)) = (
            body.input.previous_bones.get(head),
            body.input.current_bones.get(head),
        ) else {
            return;
        };
        let poses = self.poses.share(body, layer, [&[*previous], &[*current]]);
        layers.push(layer_presentation(body, layer, rig, poses, location, 0));
    }
}
