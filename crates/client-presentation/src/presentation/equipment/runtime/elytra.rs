//! Worn wings use the pack's controller, movement queries and body attachment.

use client_world::AttachableAnimationInput;

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
};

impl EquipmentRuntime {
    /// Samples the authored worn controller and places its model beneath the owner's body bone.
    pub(super) fn push_elytra(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        equipment: &ActorEquipmentInput,
        animation: EquipmentAnimation<'_>,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let Some((catalog, from_pack)) = self.binding_source(&item.identifier) else {
            return;
        };
        let assets = if from_pack {
            Arc::clone(&self.pack.as_ref().unwrap().assets)
        } else {
            Arc::clone(&self.assets)
        };
        // The scene registers only each binding's catalog default model.
        let registered = catalog
            .binding(&item.identifier)
            .filter(|binding| binding.geometry.resolution == EntityDependencyResolution::Catalog)
            .and_then(|binding| find_geometry_index(&assets, &binding.geometry.identifier));
        let runtime = if from_pack {
            &mut self.pack.as_mut().unwrap().attachables
        } else {
            &mut self.attachables
        };
        let variables = [("variable.is_enchanted", f32::from(item.enchanted))];
        let input = equipment.attachable_input(AttachableAnimationInput {
            worn: true,
            frame_alpha: animation.frame_alpha,
            owner_variables: &variables,
            ..Default::default()
        });
        let Some(evaluated) =
            runtime.evaluate(&item.identifier, animation.owner, animation.rig, input)
        else {
            return;
        };
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
        for (layer_id, (index, pose, hidden, material, state, source)) in ids.zip(groups) {
            let material = render::ActorMaterial {
                kind: if item.enchanted {
                    assets::EntityRenderMaterial::Glint
                } else {
                    material
                },
                state: Some(state.unwrap_or(WING_MATERIAL_STATE)),
                glint: render::ActorGlint {
                    time_seconds: (animation.rig.completed_tick as f32 + animation.frame_alpha)
                        * client_world::ACTOR_TICK_DURATION.as_secs_f32(),
                    ..Default::default()
                },
                ..Default::default()
            };
            let Some(source) = assets.sources().get(source as usize) else {
                continue;
            };
            let texture = source
                .path
                .strip_suffix(".png")
                .or_else(|| source.path.strip_suffix(".tga"));
            let Some(location) =
                texture.and_then(|texture| self.texture_location(texture, from_pack))
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
