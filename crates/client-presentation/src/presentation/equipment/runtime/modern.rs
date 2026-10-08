//! Data-driven held attachables, sharing the entity Molang/animation/controller pipeline.

use bevy::math::{Quat, Vec3, Vec4};
use client_world::{
    ActorRigSnapshot, ActorSnapshot, AttachableAnimationInput, AttachableBoneParent, BoneTransform,
};

use super::*;

impl EquipmentRuntime {
    pub fn first_person_attachable(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        owner: &ActorSnapshot,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        java_hand: Option<render_model::java_animation::JavaHand>,
    ) -> Option<FirstPersonItem> {
        let (catalog, from_pack) = self.binding_source(&item.identifier)?;
        let assets = if from_pack {
            Arc::clone(&self.pack.as_ref()?.assets)
        } else {
            Arc::clone(&self.assets)
        };
        let (_, body_bones) = self.body_bones_for(body.input.rig)?;
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
        let model_scale =
            std::array::from_fn::<_, 3, _>(|axis| evaluated.scale * evaluated.axis_scale[axis]);
        let placed: Arc<[RenderBoneTransform]> = pose
            .iter()
            .enumerate()
            .map(|(index, bone)| {
                if selected.hidden_bones.contains(&(index as u32)) {
                    return Some(hidden_bone());
                }
                compose_parent(
                    parent_for_root(
                        &body.input.previous_bones,
                        &body.input.current_bones,
                        &body_bones,
                        input,
                        evaluated.bone_parent(geometry_index, index)?,
                        model_scale,
                    )?,
                    *bone,
                )
            })
            .collect::<Option<Vec<_>>>()?
            .into();
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
        let key = (from_pack, geometry_index, texture.identifier.clone());
        // Java draws a raster attachable (the bow's pull frames) as its own flat item.
        let java = java_hand
            .filter(|_| super::java::java_draws_attachable(&item.identifier))
            .and_then(|hand| {
                let raster = self
                    .java_rasters
                    .entry(key.clone())
                    .or_insert_with(|| {
                        let (image_to_rig, pivots) = render_model::attachable_raster_frame(
                            &assets,
                            geometry_index as usize,
                            texture,
                        )?;
                        Some(JavaRasterFrame {
                            image_to_rig,
                            rest: pivots.into_iter().map(super::java::rest_bone).collect(),
                            normal_axis: image_to_rig
                                .transform_vector3(-bevy::math::Vec3::Y)
                                .normalize(),
                        })
                    })
                    .clone()?;
                let camera = super::java::java_raster_camera(
                    hand,
                    raster.image_to_rig,
                    texture.width,
                    texture.height,
                );
                camera
                    .is_finite()
                    .then_some((camera, raster.rest, raster.normal_axis))
            });
        let (java_camera, placed, java_normal_axis): (_, Arc<[RenderBoneTransform]>, _) = match java
        {
            Some((camera, rest, normal_axis)) => (Some(camera), rest, normal_axis),
            None => (None, placed, bevy::math::Vec3::Z),
        };
        let rig = if let Some(rig) = self.attachable_meshes.get(&key) {
            *rig
        } else {
            let rig = self.build_item_mesh(|rig| {
                render_model::attachable_geometry(&assets, geometry_index as usize, rig, texture)
                    .ok()
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
                [Arc::clone(&placed), placed],
                location,
                0,
            ),
            camera_space: java_camera.is_some(),
            alpha_mode: render::HandItemAlphaMode::Cutout,
            java_camera,
            java_normal_axis,
        })
    }
}

fn parent_for_root(
    previous: &[RenderBoneTransform],
    current: &[RenderBoneTransform],
    bones: &BodyBones,
    input: AttachableAnimationInput<'_>,
    root: AttachableBoneParent<'_>,
    model_scale: [f32; 3],
) -> Option<RenderBoneTransform> {
    let owner = match root {
        AttachableBoneParent::Actor => None,
        AttachableBoneParent::OwnerNamed(name) => Some(
            bones
                .names
                .iter()
                .position(|owner| owner.eq_ignore_ascii_case(name))?,
        ),
        AttachableBoneParent::BindingExpression => Some(if input.off_hand {
            bones.left_item?
        } else {
            bones.right_item?
        }),
    };
    let mut parent = match owner {
        Some(index) => interpolate_parent(
            *previous.get(index)?,
            *current.get(index)?,
            input.frame_alpha,
        )?,
        None => RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        },
    };
    for (axis, scale) in model_scale.into_iter().enumerate() {
        parent.axis_scale[axis] *= scale;
    }
    Some(parent)
}

pub(super) fn interpolate_parent(
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

/// Vanilla attachable setup copies the parent's complete matrix before the held
/// model's own channels. Poses and translations here already use the mirrored rig frame.
pub(super) fn compose_parent(
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

    fn owner_parents() -> (BodyBones, [RenderBoneTransform; 3]) {
        let names = body_bones(["rightItem", "leftItem", "head"].map(Box::from).into());
        let poses = [
            ([3.0, 4.0, 5.0], Quat::from_rotation_x(0.7)),
            ([-2.0, 1.0, 3.0], Quat::from_rotation_z(0.4)),
            ([0.0, 2.0, 0.0], Quat::from_rotation_y(0.9)),
        ]
        .map(|(position, rotation)| RenderBoneTransform {
            rotation: rotation.to_array(),
            translation_scale: [position[0], position[1], position[2], 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        });
        (names, poses)
    }

    #[test]
    fn unbound_attachable_root_keeps_actor_frame_instead_of_either_hand() {
        let (names, poses) = owner_parents();
        let local = BoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 24.0, 20.0, 1.0],
            axis_scale: [1.0; 3],
        };
        for off_hand in [false, true] {
            let parent = parent_for_root(
                &poses,
                &poses,
                &names,
                AttachableAnimationInput {
                    off_hand,
                    ..Default::default()
                },
                AttachableBoneParent::Actor,
                [1.0, 2.0, 0.5],
            )
            .unwrap();
            let placed = compose_parent(parent, local).unwrap();
            assert_eq!(placed.translation_scale[..3], [0.0, 3.0, 0.625]);
            assert_eq!(placed.rotation, Quat::IDENTITY.to_array());
            assert_eq!(placed.axis_scale, [1.0, 2.0, 0.5, 1.0]);
        }
    }

    #[test]
    fn named_attachable_root_follows_its_matching_owner_bone() {
        let (names, poses) = owner_parents();
        let parent = parent_for_root(
            &poses,
            &poses,
            &names,
            AttachableAnimationInput::default(),
            AttachableBoneParent::OwnerNamed("HEAD"),
            [1.0; 3],
        )
        .unwrap();
        assert_eq!(parent, poses[2]);
    }

    #[test]
    fn slot_expression_attachable_keeps_the_selected_interpolated_hand() {
        let (names, poses) = owner_parents();
        let previous = poses.map(|pose| RenderBoneTransform {
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            ..pose
        });
        for off_hand in [false, true] {
            let parent = parent_for_root(
                &previous,
                &poses,
                &names,
                AttachableAnimationInput {
                    off_hand,
                    frame_alpha: 0.5,
                    ..Default::default()
                },
                AttachableBoneParent::BindingExpression,
                [1.0; 3],
            )
            .unwrap();
            let index = usize::from(off_hand);
            assert_eq!(
                parent,
                interpolate_parent(previous[index], poses[index], 0.5).unwrap()
            );
        }
    }

    #[test]
    fn attachable_channels_follow_rotated_parent_not_third_person_grip() {
        let parent = RenderBoneTransform {
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array(),
            translation_scale: [1.0, 2.0, 3.0, 2.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
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
            axis_scale: render_model::UNIT_AXIS_SCALE,
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
