//! Data-driven held attachables, sharing the entity Molang/animation/controller pipeline.

use bevy::math::{Quat, Vec3, Vec4};
use client_world::{ActorRigSnapshot, ActorSnapshot, AttachableAnimationInput, BoneTransform};

use super::*;

impl EquipmentRuntime {
    pub fn first_person_attachable(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        owner: &ActorSnapshot,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
    ) -> Option<FirstPersonItem> {
        let (catalog, from_pack) = self.binding_source(&item.identifier)?;
        let assets = if from_pack {
            Arc::clone(&self.pack.as_ref()?.assets)
        } else {
            Arc::clone(&self.assets)
        };
        let runtime = if from_pack {
            &mut self.pack.as_mut()?.attachables
        } else {
            &mut self.attachables
        };
        let evaluated = runtime.evaluate(&item.identifier, owner, owner_rig, input)?;
        // Each hand has its own item texture page. Multi-layer attachables remain
        // explicitly incomplete rather than accidentally displaying an enchantment layer.
        let selected = evaluated.render.first()?;
        let geometry_index = selected.geometry.unwrap_or(evaluated.geometry);
        let pose = if selected.pose.is_empty() {
            evaluated.pose
        } else {
            &selected.pose
        };
        let pose = pose.to_vec();
        let hidden = Arc::clone(&selected.hidden_bones);
        let model_scale =
            std::array::from_fn::<_, 3, _>(|axis| evaluated.scale * evaluated.axis_scale[axis]);
        let source = assets.sources().get(selected.source as usize)?;
        let texture_identifier = source
            .path
            .strip_suffix(".png")
            .or_else(|| source.path.strip_suffix(".tga"))
            .unwrap_or(&source.path);
        let texture = catalog
            .textures()
            .iter()
            .find(|t| &*t.identifier == texture_identifier)?;
        let location = self.texture_location(&texture.identifier, from_pack)?;
        let (_, body_bones) = self.body_bones_for(body.input.rig)?;
        let hand = if input.off_hand {
            body_bones.left_item?
        } else {
            body_bones.right_item?
        };
        let mut parent = interpolate_parent(
            *body.input.previous_bones.get(hand)?,
            *body.input.current_bones.get(hand)?,
            input.frame_alpha,
        )?;
        for (axis, scale) in model_scale.into_iter().enumerate() {
            parent.axis_scale[axis] *= scale;
        }
        let placed = pose
            .iter()
            .enumerate()
            .map(|(index, bone)| {
                if hidden.contains(&(index as u32)) {
                    return Some(hidden_bone());
                }
                compose_parent(parent, *bone)
            })
            .collect::<Option<Vec<_>>>()?;
        let key = (from_pack, geometry_index, texture.identifier.clone());
        let rig = if let Some(rig) = self.attachable_meshes.get(&key) {
            *rig
        } else {
            let rig = self.build_item_mesh(|rig| {
                render::attachable_geometry(&assets, geometry_index as usize, rig, texture).ok()
            })?;
            self.attachable_meshes.insert(key, rig);
            rig
        };
        Some(FirstPersonItem {
            presentation: layer_presentation(
                body,
                if input.off_hand {
                    LAYER_OFF_HAND
                } else {
                    LAYER_MAIN_HAND
                },
                rig,
                [Arc::from(placed.clone()), Arc::from(placed)],
                location,
                0,
            ),
            camera_space: false,
        })
    }
}

fn interpolate_parent(
    previous: RenderBoneTransform,
    current: RenderBoneTransform,
    alpha: f32,
) -> Option<RenderBoneTransform> {
    let alpha = if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let a = Quat::from_vec4(Vec4::from_array(previous.rotation).try_normalize()?);
    let b = Quat::from_vec4(Vec4::from_array(current.rotation).try_normalize()?);
    Some(RenderBoneTransform {
        rotation: a.slerp(b, alpha).to_array(),
        translation_scale: std::array::from_fn(|i| {
            previous.translation_scale[i]
                + (current.translation_scale[i] - previous.translation_scale[i]) * alpha
        }),
        axis_scale: std::array::from_fn(|i| {
            previous.axis_scale[i] + (current.axis_scale[i] - previous.axis_scale[i]) * alpha
        }),
    })
}

/// Native setupAttachableNoChecks copies the parent's complete matrix before the held
/// model's own channels. Poses and translations here already use the mirrored rig frame.
fn compose_parent(
    parent: RenderBoneTransform,
    local: BoneTransform,
) -> Option<RenderBoneTransform> {
    let parent_rotation = Quat::from_vec4(Vec4::from_array(parent.rotation).try_normalize()?);
    let rotation = Quat::from_vec4(Vec4::from_array(local.rotation).try_normalize()?);
    let parent_scale = Vec3::from_array(std::array::from_fn(|axis| {
        parent.axis_scale[axis] * parent.translation_scale[3]
    }));
    let offset = Vec3::new(
        local.translation_scale[0],
        local.translation_scale[1],
        local.translation_scale[2],
    ) / 16.0;
    let origin = Vec3::new(
        parent.translation_scale[0],
        parent.translation_scale[1],
        parent.translation_scale[2],
    ) + parent_rotation * (offset * parent_scale);
    let scale = parent_scale * Vec3::from_array(local.axis_scale) * local.translation_scale[3];
    let result = RenderBoneTransform {
        rotation: (parent_rotation * rotation).normalize().to_array(),
        translation_scale: [origin.x, origin.y, origin.z, 1.0],
        axis_scale: [scale.x, scale.y, scale.z, 1.0],
    };
    result.is_finite().then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachable_channels_follow_rotated_parent_not_third_person_grip() {
        let parent = RenderBoneTransform {
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array(),
            translation_scale: [1.0, 2.0, 3.0, 2.0],
            axis_scale: render::UNIT_AXIS_SCALE,
        };
        let local = BoneTransform {
            rotation: Quat::from_rotation_z(0.3).to_array(),
            translation_scale: [16.0, 0.0, 0.0, 1.0],
            axis_scale: [1.0, 2.0, 3.0],
        };
        let placed = compose_parent(parent, local).unwrap();
        assert!((placed.translation_scale[0] - 1.0).abs() < 1e-5);
        assert!((placed.translation_scale[1] - 2.0).abs() < 1e-5);
        assert!((placed.translation_scale[2] - 1.0).abs() < 1e-5);
        assert_eq!(placed.axis_scale, [2.0, 4.0, 6.0, 1.0]);
        assert!(Quat::from_array(placed.rotation).abs_diff_eq(
            Quat::from_array(parent.rotation) * Quat::from_array(local.rotation),
            1e-5
        ));
    }

    #[test]
    fn sampled_attachable_parent_does_not_wait_for_the_next_body_tick() {
        let a = RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render::UNIT_AXIS_SCALE,
        };
        let b = RenderBoneTransform {
            translation_scale: [4.0, 0.0, 0.0, 1.0],
            ..a
        };
        assert_eq!(
            interpolate_parent(a, b, 0.25).unwrap().translation_scale[0],
            1.0
        );
        assert_eq!(interpolate_parent(a, b, f32::NAN).unwrap(), a);
    }
}
