//! Per-category equipment layer builders for a drawn body.

use {
    super::*,
    render_model::equipment::{
        BoneChannels, ItemDisplay, attach_to_bone, held_block_display, held_sprite_display,
    },
};

impl EquipmentRuntime {
    /// Draws one admitted held definition, then uses the selected hand's ordinary mesh.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_held(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        layer: u8,
        bones: &BodyBones,
        source: Option<&pack::BindingSource>,
        (equipment, animation): (&ActorEquipmentInput, Option<EquipmentAnimation<'_>>),
        scale: f32,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        if let Some(source) = source {
            let binding = &source.catalog.bindings()[source.index];
            let authored = layer != LAYER_MAIN_HAND
                || equipment.java.is_none()
                || java::keep_authored_hand(&item.identifier, binding.category, source.from_pack);
            if authored
                && (equipment.java.is_none() || binding.third_person.literal().is_none())
                && let Some(animation) = animation
                && let Some(animated) = self.held_attachable_from_source(
                    body,
                    item,
                    source,
                    animation.owner,
                    animation.rig,
                    animation.held_input(equipment, layer == LAYER_OFF_HAND),
                    None,
                )
            {
                layers.push(animated.presentation);
                return;
            }
            let hand = if layer == LAYER_OFF_HAND {
                bones.left_item
            } else {
                bones.right_item
            };
            if self.push_attachable(body, item, layer, hand, source, scale, layers) {
                return;
            }
        }
        if let Some(grip) = equipment.java.filter(|_| layer == LAYER_MAIN_HAND) {
            self.push_java_held(body, item, bones, grip, scale, layers);
        } else {
            let hand = if layer == LAYER_OFF_HAND {
                bones.left_item
            } else {
                bones.right_item
            };
            self.push_attached(body, item, layer, hand, None, scale, layers);
        }
    }

    /// A held item whose attachable ships its own single-bone geometry and a literal
    /// third-person placement (trident, shield). `false` when the item is not one, or its
    /// placement is Molang-driven (then the sprite/cube path draws it).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_attachable(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        layer: u8,
        hand: Option<usize>,
        source: &pack::BindingSource,
        scale: f32,
        layers: &mut Vec<EquipmentPresentation>,
    ) -> bool {
        let Some(hand) = hand else {
            return false;
        };
        let off_hand = layer == LAYER_OFF_HAND;
        let pack::BindingSource {
            ref catalog,
            from_pack,
            index,
            ..
        } = *source;
        let binding = &catalog.bindings()[index];
        let Some((_, _, location)) = self.draw_artwork(&binding.texture.identifier, source) else {
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
            attachable::attach(
                scaled_parent(*previous, scale),
                *pivot,
                channels,
                *has_binding_expression,
            ),
            attachable::attach(
                scaled_parent(*current, scale),
                *pivot,
                channels,
                *has_binding_expression,
            ),
        ) else {
            return false;
        };
        let poses = self.poses.share(body, layer, [&[previous], &[current]]);
        layers.push(attachable_glint(
            layer_presentation(body, layer, geometry.rig, poses, location, 0),
            item,
        ));
        true
    }

    /// The held item's generated mesh and artwork, whether it holds like a block (a cube, a
    /// block's template or a block-entity model, placed by the held-block display), and the UV
    /// span of its atlas region that the item glint covers. A server pack's icon replaces the
    /// vanilla one, as it does in the inventory, and a custom block item with a cube sheet is
    /// held as that cube. With `icon_fallback`, a block with no plain cube sheet takes its icon
    /// sprite instead.
    pub(super) fn held_mesh(
        &mut self,
        item: &WornItem,
        icon_fallback: bool,
    ) -> Option<(EntityRigId, ActorArtworkLocation, bool, [f32; 2])> {
        let session =
            self.session_held(&item.identifier, item.metadata)
                .filter(|(_, key, _, _)| {
                    !matches!(item.kind, HeldKind::Block(_))
                        || matches!(key, MeshKey::SessionBlock(_))
                });
        if session.is_none()
            && let Some(&(rig, location, span)) = HeldBlockEntity::from_item(&item.identifier)
                .and_then(|kind| self.held_block_entities.models.get(&kind))
        {
            return Some((rig, location, true, span));
        }
        let (index, key, placement, location) = if let Some(held) = session {
            held
        } else {
            let sheet = match item.kind {
                HeldKind::Block(visual) => {
                    self.block_placement(visual).map(|sheet| (visual, sheet))
                }
                _ => None,
            };
            if let Some((visual, (index, placement, location))) = sheet {
                (index, MeshKey::Block(visual), placement, location)
            } else {
                match item.kind {
                    HeldKind::Other => return None,
                    HeldKind::Block(_) if !icon_fallback => return None,
                    _ => {}
                }
                let index = self.held_icon_index(item)?;
                let placement = self.placements.get(index).copied().flatten()?;
                let location = self
                    .atlas_locations
                    .get(placement.layer)
                    .copied()
                    .flatten()?;
                (index, MeshKey::Sprite(index), placement, location)
            }
        };
        let mesh = self.mesh_for(key, index, placement)?;
        let block = matches!(key, MeshKey::Block(_) | MeshKey::SessionBlock(_));
        let span = glint_span(placement.uv_rect(), block);
        Some((mesh, location, block, span))
    }

    /// A held or worn sprite/cube on `bone`, placed by `display` or the kind's held placement.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_attached(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        layer: u8,
        bone: Option<usize>,
        override_display: Option<ItemDisplay>,
        scale: f32,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let Some(hand) = bone else {
            return;
        };
        let Some((mesh, location, block, span)) = self.held_mesh(item, override_display.is_none())
        else {
            return;
        };
        let mut display = override_display.unwrap_or_else(|| {
            if block {
                held_block_display()
            } else {
                held_sprite_display(item.grip.upright().unwrap_or(false))
            }
        });
        display.scale *= scale;
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
        layers.push(held_glint(
            layer_presentation(body, layer, mesh, poses, location, 0),
            item,
            span,
        ));
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_armor(
        &mut self,
        body: &ActorRigSubmission,
        bones: &BodyBones,
        body_geometry: u32,
        (slot, layer): (ArmorSlot, u8),
        item: &WornItem,
        source: pack::BindingSource,
        (equipment, animation, profile): (
            &ActorEquipmentInput,
            Option<EquipmentAnimation<'_>>,
            render_api::CombatVisuals,
        ),
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let pack::BindingSource {
            ref catalog,
            from_pack,
            index,
            ..
        } = source;
        let binding = &catalog.bindings()[index];
        // A custom item's `minecraft:wearable` slot names where its attachable is worn.
        let category = self.effective_category(&item.identifier, binding.category);
        if category != (EquipmentCategory::Armor { slot }) {
            return;
        }
        let selected = match animation.map(|animation| {
            self.worn_render(
                item,
                slot,
                equipment,
                animation,
                from_pack,
                binding.attachable_identifier(),
                body,
                bones,
            )
        }) {
            Some(Ok(None)) => return,
            Some(Ok(selected)) => selected,
            _ => None,
        };
        let texture = selected
            .as_ref()
            .map(|selected| selected.texture.as_ref())
            .unwrap_or(&binding.texture.identifier);
        let Some((_, _, location)) = self.draw_artwork(texture, &source) else {
            return;
        };
        let geometry_identifier = selected
            .as_ref()
            .map_or(binding.geometry.identifier.as_ref(), |selected| {
                selected.geometry.as_ref()
            });
        let direct = self.armor_geometry_for(geometry_identifier, from_pack);
        let external = if direct.is_none()
            && binding.geometry.resolution == EntityDependencyResolution::External
        {
            if let Some(animation) = animation {
                match self.external_armor_model(
                    geometry_identifier,
                    (item, equipment, animation, slot),
                    body,
                    bones,
                ) {
                    Ok(None) => return,
                    Ok(model) => model,
                    Err(()) => None,
                }
            } else {
                None
            }
        } else {
            None
        };
        let geometry = direct
            .map(|geometry| (geometry_identifier, geometry))
            .or_else(|| {
                external
                    .as_ref()
                    .map(|model| (model.identifier.as_ref(), Arc::clone(&model.geometry)))
            })
            .or_else(|| {
                if from_pack
                    || selected.is_some()
                    || binding.category != (EquipmentCategory::Armor { slot })
                    || binding.geometry.resolution != EntityDependencyResolution::External
                {
                    return None;
                }
                // Native external models keep a static startup mesh without its owner scripts.
                catalog
                    .bindings_for(&item.identifier)
                    .iter()
                    .rev()
                    .find_map(|fallback| {
                        self.armor_geometry_for(&fallback.geometry.identifier, false)
                            .map(|geometry| (fallback.geometry.identifier.as_ref(), geometry))
                    })
            });
        let Some((geometry_identifier, geometry)) = geometry else {
            // An external family may have no model for this owner. Keep its admitted startup draw.
            if from_pack
                && let Some(index) = source.startup
                && let Some(catalog) = self.catalog.as_ref().map(Arc::clone)
            {
                self.push_armor(
                    body,
                    bones,
                    body_geometry,
                    (slot, layer),
                    item,
                    pack::BindingSource {
                        catalog,
                        from_pack: false,
                        index,
                        startup: Some(index),
                    },
                    (equipment, animation, profile),
                    layers,
                );
            }
            return;
        };
        if selected.is_some()
            && geometry_identifier != binding.geometry.identifier.as_ref()
            && self.selected_geometries.insert(geometry.rig)
        {
            let assets = if from_pack {
                &self.pack.as_ref().unwrap().assets
            } else {
                &self.assets
            };
            if let Some(index) = find_geometry_index(assets, geometry_identifier)
                && let Some(mesh) =
                    render_model::equipment_geometry(assets, index as usize, geometry.rig)
            {
                self.pending.push(mesh);
            }
        }
        let map = Arc::clone(
            self.armor_maps
                .entry((
                    body_geometry,
                    if from_pack {
                        format!("{}{}", pack::ARMOR_CACHE_PREFIX, geometry_identifier).into()
                    } else {
                        geometry_identifier.into()
                    },
                ))
                .or_insert_with(|| bone_map(&geometry.names, &bones.names).into()),
        );
        let tint = binding.color_mask_rgb(item.dye_rgb).map_or(0, pack_tint);
        let (mut previous, mut current) = (
            remap_pose(&map, &body.input.previous_bones),
            remap_pose(&map, &body.input.current_bones),
        );
        if let Some(selected) = selected
            .as_ref()
            .or_else(|| external.as_ref().and_then(|model| model.render.as_ref()))
        {
            for poses in [&mut previous, &mut current] {
                for (index, pose) in poses.iter_mut().enumerate() {
                    if selected.layer.hidden_bones.contains(&(index as u32)) {
                        *pose = hidden_bone();
                    } else if map[index].is_none()
                        && let Some(authored) = selected.pose.get(index)
                    {
                        *pose = *authored;
                    }
                }
            }
        }
        let poses = self.poses.share(body, layer, [&previous, &current]);
        let mut presentation = layer_presentation(body, layer, geometry.rig, poses, location, tint);
        let color_mask = profile == render_api::CombatVisuals::Legacy112
            && binding.color_mask == assets::EquipmentColorMask::Dye;
        presentation.submission.material.color_mask = color_mask;
        if profile == render_api::CombatVisuals::Legacy112 {
            presentation.submission.material.kind = if item.has_glint() {
                assets::EntityRenderMaterial::Glint
            } else {
                assets::EntityRenderMaterial::Default
            };
            presentation.submission.material.state = Some(assets::EntityRenderMaterialState {
                alpha_test: true,
                cull: false,
                ..Default::default()
            });
        }
        if profile == render_api::CombatVisuals::Legacy112
            && from_pack
            && let Some(selected) = selected
        {
            presentation.submission.material.glint = render_model::ActorGlint {
                profile,
                ..Default::default()
            };
            super::style::LayerStyle::from_layer(&selected.layer)
                .apply(&mut presentation.submission);
        }
        layers.push(presentation);
    }

    /// Returns selected worn artwork and shading; the binding supplies missing state.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn worn_render(
        &mut self,
        item: &WornItem,
        slot: ArmorSlot,
        equipment: &ActorEquipmentInput,
        animation: EquipmentAnimation<'_>,
        from_pack: bool,
        attachable_identifier: &str,
        body: &ActorRigSubmission,
        bones: &BodyBones,
    ) -> Result<Option<types::WornRender>, ()> {
        let variables = [(
            assets::entity::EngineVariable::IsEnchanted.name(),
            f32::from(item.has_glint()),
        )];
        let input = animation.worn_input(equipment, slot, &variables);
        let (assets, runtime) = if from_pack {
            let pack = self.pack.as_mut().ok_or(())?;
            (Arc::clone(&pack.assets), &mut pack.attachables)
        } else {
            (Arc::clone(&self.assets), &mut self.attachables)
        };
        let evaluated = runtime
            .evaluate(attachable_identifier, animation.owner, animation.rig, input)
            .ok_or(())?;
        let Some(layer) = evaluated
            .render
            .iter()
            .find(|layer| layer.texture_slot == 0)
        else {
            return Ok(None);
        };
        let geometry_index = layer.geometry.unwrap_or(evaluated.geometry);
        let geometry = assets
            .geometries()
            .get(geometry_index as usize)
            .ok_or(())?
            .identifier
            .clone();
        let local = if layer.pose.is_empty() {
            evaluated.pose
        } else {
            &layer.pose
        };
        let pose = self
            .attachment_poses
            .sample(
                body,
                (true, input.worn_slot),
                self.poses.frame,
                placement_cache::Inputs {
                    assets: &assets,
                    bones,
                    owner_names: animation.rig.bone_names,
                    geometry: (geometry_index, evaluated.geometry),
                    local,
                    hidden: &layer.hidden_bones,
                    scale: evaluated.axis_scale.map(|axis| axis * evaluated.scale),
                    alpha: placement_cache::alpha(
                        body,
                        &evaluated,
                        geometry_index,
                        local.len(),
                        input.frame_alpha,
                    ),
                },
                |_, index| {
                    let local = &local[index];
                    if layer.hidden_bones.contains(&(index as u32))
                        || (geometry_index == evaluated.geometry
                            && evaluated.bone_names.get(index).is_some_and(|name| {
                                bones
                                    .names
                                    .iter()
                                    .any(|owner| owner.eq_ignore_ascii_case(name))
                            }))
                    {
                        return Some(hidden_bone());
                    }
                    let Some(root) = evaluated.bone_parent(geometry_index, index) else {
                        return Some(hidden_bone());
                    };
                    if root == actor_animation::AttachableBoneParent::BindingExpression {
                        return Some(hidden_bone());
                    }
                    let Some(parent) = modern::parent_for_root(
                        &body.input.previous_bones,
                        &body.input.current_bones,
                        bones,
                        input,
                        root,
                        evaluated.axis_scale.map(|axis| axis * evaluated.scale),
                    ) else {
                        return Some(hidden_bone());
                    };
                    Some(modern::compose_parent(parent, *local).unwrap_or_else(hidden_bone))
                },
            )
            .ok_or(())?[0]
            .clone();
        let path = &assets.sources().get(layer.source as usize).ok_or(())?.path;
        let texture = path
            .strip_suffix(".png")
            .or_else(|| path.strip_suffix(".tga"))
            .ok_or(())?
            .into();
        Ok(Some(types::WornRender {
            texture,
            geometry,
            layer: layer.clone(),
            pose,
        }))
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

/// Scales a held model about its parent origin without moving the owner's hand.
fn scaled_parent(mut parent: RenderBoneTransform, scale: f32) -> RenderBoneTransform {
    parent.translation_scale[3] *= scale;
    parent
}

/// Returns one sprite's UV span, or one face's span within a block's six-face sheet.
fn glint_span([u0, v0, u1, v1]: [f32; 4], block: bool) -> [f32; 2] {
    let mut span = [u1 - u0, v1 - v0];
    if block {
        for (span, divisions) in span.iter_mut().zip(assets::BLOCK_ITEM_SHEET_GRID) {
            *span /= f32::from(divisions);
        }
    }
    span
}

/// Draws `presentation` with vanilla's item glint over its `span` of the atlas when the held
/// stack glints; the frame clock is applied at publication.
pub(super) fn held_glint(
    presentation: EquipmentPresentation,
    item: &WornItem,
    span: [f32; 2],
) -> EquipmentPresentation {
    with_glint(
        presentation,
        item,
        render_model::ActorGlint::item(0.0, span),
    )
}

/// An attachable's own model takes the item glint raster at the actor glint's unit UV scale,
/// as its render controller's glint material draws it.
pub(super) fn attachable_glint(
    presentation: EquipmentPresentation,
    item: &WornItem,
) -> EquipmentPresentation {
    with_glint(
        presentation,
        item,
        render_model::ActorGlint {
            profile: render_api::CombatVisuals::Modern,
            texture: render_model::GlintTexture::Item,
            ..Default::default()
        },
    )
}

/// Adds shine when actual enchantments or intrinsic facts request it.
fn with_glint(
    mut presentation: EquipmentPresentation,
    item: &WornItem,
    glint: render_model::ActorGlint,
) -> EquipmentPresentation {
    if item.has_glint() {
        presentation.submission.material = render_model::ActorMaterial {
            glint,
            ..presentation.submission.material
        };
    }
    presentation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_sheet_glint_covers_one_face_at_the_sprite_scale() {
        let face_span = 0.125;
        let [columns, rows] = assets::BLOCK_ITEM_SHEET_GRID.map(f32::from);
        let rect = [0.0, 0.0, face_span * columns, face_span * rows];
        let glint = render_model::ActorGlint::item(0.0, glint_span(rect, true));
        assert_eq!(
            glint.uv_scale,
            [render_model::ITEM_GLINT_UV_SCALE / face_span; 2]
        );
        assert_eq!(glint_span(rect, false), [rect[2], rect[3]]);
    }
}
