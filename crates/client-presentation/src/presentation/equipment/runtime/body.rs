//! Body-slot attachables retain their authored owner binding and controller output.

use super::*;

impl EquipmentRuntime {
    /// Draws the body slot with the same owner context and root placement as other attachables.
    pub(super) fn push_body(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        equipment: &ActorEquipmentInput,
        animation: EquipmentAnimation<'_>,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let variables = [(
            assets::entity::EngineVariable::IsEnchanted.name(),
            f32::from(item.enchanted),
        )];
        let input = actor_animation::AttachableAnimationInput {
            worn_slot: actor_animation::AttachableAnimationInput::BODY_SLOT,
            ..animation.worn_input(equipment, ArmorSlot::Helmet, &variables)
        };
        let Some(source) = self.selected_binding_source(item, animation, input) else {
            return;
        };
        let Some((_, bones)) = self.body_bones_for(body.input.rig) else {
            return;
        };
        let binding = &source.catalog.bindings()[source.index];
        let (assets, runtime) = if source.from_pack {
            let Some(pack) = self.pack.as_mut() else {
                return;
            };
            (Arc::clone(&pack.assets), &mut pack.attachables)
        } else {
            (Arc::clone(&self.assets), &mut self.attachables)
        };
        let registered = (binding.geometry.resolution == EntityDependencyResolution::Catalog)
            .then(|| find_geometry_index(&assets, &binding.geometry.identifier))
            .flatten();
        let Some(evaluated) = runtime.evaluate(
            binding.attachable_identifier(),
            animation.owner,
            animation.rig,
            input,
        ) else {
            return;
        };
        let Some(selected) = evaluated
            .render
            .iter()
            .find(|layer| layer.texture_slot == 0)
        else {
            return;
        };
        let geometry_index = selected.geometry.unwrap_or(evaluated.geometry);
        let local = if selected.pose.is_empty() {
            evaluated.pose
        } else {
            &selected.pose
        };
        let scale = evaluated.axis_scale.map(|axis| axis * evaluated.scale);
        let Some(poses) = self.attachment_poses.sample(
            body,
            (true, input.worn_slot),
            self.poses.frame,
            placement_cache::Inputs {
                assets: &assets,
                bones: &bones,
                owner_names: animation.rig.bone_names,
                geometry: (geometry_index, evaluated.geometry),
                local,
                paired: false,
                hidden: &selected.hidden_bones,
                scale,
                alpha: placement_cache::alpha(
                    body,
                    &evaluated,
                    geometry_index,
                    local.len(),
                    input.frame_alpha,
                ),
            },
            |_, index| {
                if selected.hidden_bones.contains(&(index as u32)) {
                    return Some(hidden_bone());
                }
                modern::compose_parent(
                    modern::parent_for_root(
                        &body.input.previous_bones,
                        &body.input.current_bones,
                        &bones,
                        input,
                        evaluated.bone_parent(geometry_index, index)?,
                        scale,
                    )?,
                    local[index],
                )
            },
        ) else {
            return;
        };
        let style = style::LayerStyle::from_layer(selected);
        let Some(texture_source) = assets.sources().get(selected.source as usize) else {
            return;
        };
        let texture = texture_source
            .path
            .strip_suffix(".png")
            .or_else(|| texture_source.path.strip_suffix(".tga"))
            .unwrap_or(&texture_source.path);
        let Some((_, _, location)) = self.draw_artwork(texture, &source) else {
            return;
        };
        let Some(geometry) = assets.geometries().get(geometry_index as usize) else {
            return;
        };
        let Some(geometry) = self.armor_geometry_for(&geometry.identifier, source.from_pack) else {
            return;
        };
        let registered = if source.from_pack {
            registered == Some(geometry_index)
        } else {
            self.startup_geometries.contains(&geometry.rig)
        };
        if !registered && !self.selected_geometries.contains(&geometry.rig) {
            let Some(mesh) =
                render_model::equipment_geometry(&assets, geometry_index as usize, geometry.rig)
            else {
                return;
            };
            self.pending.push(mesh);
            self.selected_geometries.insert(geometry.rig);
        }
        let mut layer = layer_presentation(
            body,
            super::super::BODY_EQUIPMENT_LAYER,
            geometry.rig,
            poses,
            location,
            0,
        );
        style.apply(&mut layer.submission);
        layers.push(layer);
    }
}
