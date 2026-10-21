//! Worn wings use the pack's controller, movement queries and body attachment.

use super::*;

/// Vanilla's `elytra` material derives `entity_alphatest`: alpha-tested and double-sided.
const WING_MATERIAL_STATE: assets::EntityRenderMaterialState = assets::EntityRenderMaterialState {
    alpha_test: true,
    cull: false,
    blend: false,
    depth_write: true,
    emissive: false,
    additive: false,
    additive_alpha: false,
    disable_overlay: false,
    depth_always: false,
};

impl EquipmentRuntime {
    /// Samples the authored worn controller and places its model beneath the owner's body bone.
    pub(super) fn push_elytra(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        source: pack::BindingSource,
        equipment: &ActorEquipmentInput,
        animation: EquipmentAnimation<'_>,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let variables = [("variable.is_enchanted", f32::from(item.has_glint()))];
        let input = animation.worn_input(equipment, ArmorSlot::Chestplate, &variables);
        let pack::BindingSource {
            ref catalog,
            from_pack,
            index,
            ..
        } = source;
        let binding = &catalog.bindings()[index];
        let (assets, runtime) = if from_pack {
            let Some(pack) = self.pack.as_mut() else {
                return;
            };
            (Arc::clone(&pack.assets), &mut pack.attachables)
        } else {
            (Arc::clone(&self.assets), &mut self.attachables)
        };
        let Some(evaluated) = runtime.evaluate(
            binding.attachable_identifier(),
            animation.owner,
            animation.rig,
            input,
        ) else {
            return;
        };
        // The scene registers only each binding's catalog default model.
        let registered = (binding.geometry.resolution == EntityDependencyResolution::Catalog)
            .then(|| find_geometry_index(&assets, &binding.geometry.identifier))
            .flatten();
        // Each material group draws apart; later texture slots are samplers of that draw.
        let groups: Vec<_> = evaluated
            .render
            .iter()
            .filter(|layer| layer.texture_slot == 0)
            .map(|layer| {
                let pose = if layer.pose.is_empty() {
                    evaluated.pose
                } else {
                    &layer.pose
                };
                (
                    layer.geometry.unwrap_or(evaluated.geometry),
                    pose.to_vec(),
                    Arc::clone(&layer.hidden_bones),
                    layer.material,
                    layer.material_state,
                    layer.source,
                )
            })
            .collect();
        let model_scale = evaluated.axis_scale.map(|axis| axis * evaluated.scale);
        let Some((_, bones)) = self.body_bones_for(body.input.rig) else {
            return;
        };
        let Some(parent) = bones
            .names
            .iter()
            .position(|name| name.eq_ignore_ascii_case("body"))
        else {
            return;
        };
        let Some(mut parent) = body
            .input
            .previous_bones
            .get(parent)
            .zip(body.input.current_bones.get(parent))
            .and_then(|(previous, current)| {
                modern::interpolate_parent(*previous, *current, animation.frame_alpha)
            })
        else {
            return;
        };
        for (axis, scale) in model_scale.into_iter().enumerate() {
            parent.axis_scale[axis] *= scale;
        }
        let ids =
            std::iter::once(super::super::ELYTRA_LAYER).chain(super::super::ELYTRA_GROUP_LAYERS);
        for (layer_id, (index, pose, hidden, material, state, texture_source)) in ids.zip(groups) {
            let material = render_model::ActorMaterial {
                kind: if item.has_glint() {
                    assets::EntityRenderMaterial::Glint
                } else {
                    material
                },
                state: Some(state.unwrap_or(WING_MATERIAL_STATE)),
                glint: render_model::ActorGlint {
                    time_seconds: (animation.rig.completed_tick as f32 + animation.frame_alpha)
                        * simulation_clock::TICK_DURATION.as_secs_f32(),
                    ..Default::default()
                },
                ..Default::default()
            };
            let Some(texture_source) = assets.sources().get(texture_source as usize) else {
                continue;
            };
            let texture = texture_source
                .path
                .strip_suffix(".png")
                .or_else(|| texture_source.path.strip_suffix(".tga"));
            let Some((_, _, location)) =
                texture.and_then(|texture| self.draw_artwork(texture, &source))
            else {
                continue;
            };
            let Some(geometry) = assets.geometries().get(index as usize) else {
                continue;
            };
            let Some(geometry) = self.armor_geometry_for(&geometry.identifier, from_pack) else {
                continue;
            };
            if registered != Some(index) && !self.selected_geometries.contains(&geometry.rig) {
                let Some(mesh) =
                    render_model::equipment_geometry(&assets, index as usize, geometry.rig)
                else {
                    continue;
                };
                self.pending.push(mesh);
                self.selected_geometries.insert(geometry.rig);
            }
            let Some(pose) = pose
                .iter()
                .enumerate()
                .map(|(index, bone)| {
                    if hidden.contains(&(index as u32)) {
                        Some(hidden_bone())
                    } else {
                        modern::compose_parent(parent, *bone)
                    }
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let poses = self.poses.share(body, layer_id, [&pose, &pose]);
            let mut layer = layer_presentation(body, layer_id, geometry.rig, poses, location, 0);
            layer.submission.material = material;
            layers.push(layer);
        }
    }
}
