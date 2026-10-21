//! Fits authored cloth and its mounts to the currently drawn torso surfaces.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct DepthFit {
    offset: f32,
    scale: f32,
}

impl DepthFit {
    /// Finds the rearmost torso surface at the pinned edge in the attachment's own frame.
    pub(super) fn resolve(
        model: &HatRig,
        bases: &[Arc<[RenderBoneTransform]>; 2],
        actor: [[f32; 4]; 3],
        alpha: f32,
        colliders: &[Collider],
    ) -> Option<Self> {
        let cloth = model.cloth.as_ref()?;
        let head = crate::actor::sampled_bone_offset(
            [0.0; 3],
            bases[0][model.head],
            bases[1][model.head],
            actor,
            alpha,
        )?;
        let frame = Mat4::from_cols(
            Vec3::from_array(head.basis[0]).extend(0.0),
            Vec3::from_array(head.basis[1]).extend(0.0),
            Vec3::from_array(head.basis[2]).extend(0.0),
            Vec3::from_array(head.position).extend(1.0),
        );
        if frame.determinant().abs() < 1e-8 {
            return None;
        }
        let inverse = frame.inverse();
        let pivot = Vec3::from_array(model.geometry.bone_pivots[model.head]);
        let pin = cloth.rest[0] - pivot;
        let mut front = None::<f32>;
        let mut outer_front = f32::INFINITY;
        let mut back = f32::NEG_INFINITY;
        for collider in colliders.iter().filter(|collider| collider.back_only) {
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for x in [collider.min.x, collider.max.x] {
                for y in [collider.min.y, collider.max.y] {
                    for z in [collider.min.z, collider.max.z] {
                        let point = inverse
                            .transform_point3(collider.world.transform_point3(Vec3::new(x, y, z)));
                        min = min.min(point);
                        max = max.max(point);
                    }
                }
            }
            if pin.y >= min.y && pin.y <= max.y {
                front.get_or_insert(min.z);
                outer_front = outer_front.min(min.z);
                back = back.max(max.z + CONTACT_SEPARATION);
            }
        }
        let front = front?;
        let span = pin.z - front;
        if span <= 1e-5 {
            return None;
        }
        let scale = (back - outer_front) / span;
        let offset = outer_front - front * scale;
        (scale.is_finite() && scale > 1e-5).then_some(Self { offset, scale })
    }

    /// Whether a depth change needs the simulated drape transferred to the new attachment frame.
    pub(super) fn differs(self, other: Self) -> bool {
        (self.offset - other.offset).abs() > 1e-4 || (self.scale - other.scale).abs() > 1e-4
    }

    /// Fits mounts and cloth together between the torso front and the pinned back edge.
    pub(super) fn apply(self, model: &HatRig, poses: &mut [RenderBoneTransform]) {
        let head = poses[model.head];
        let head_scale = Vec3::from_slice(&head.axis_scale[..3]) * head.translation_scale[3];
        let frame = Mat4::from_scale_rotation_translation(
            head_scale,
            Quat::from_array(head.rotation),
            Vec3::from_slice(&head.translation_scale[..3]),
        );
        if frame.determinant().abs() < 1e-8 {
            return;
        }
        let fit = frame
            * Mat4::from_translation(Vec3::Z * self.offset)
            * Mat4::from_scale(Vec3::new(1.0, 1.0, self.scale))
            * frame.inverse();
        for pose in poses.iter_mut().take(model.cloth.as_ref().unwrap().first) {
            let scale = Vec3::from_slice(&pose.axis_scale[..3]) * pose.translation_scale[3];
            if scale.min_element() <= 1e-5 {
                continue;
            }
            let matrix = fit
                * Mat4::from_scale_rotation_translation(
                    scale,
                    Quat::from_array(pose.rotation),
                    Vec3::from_slice(&pose.translation_scale[..3]),
                );
            let (scale, rotation, position) = matrix.to_scale_rotation_translation();
            *pose = RenderBoneTransform {
                rotation: rotation.to_array(),
                translation_scale: [position.x, position.y, position.z, 1.0],
                axis_scale: [scale.x, scale.y, scale.z, 1.0],
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntityRigId;

    /// Creates drawn torso geometry with caller-controlled thickness, independent of catalog art.
    fn torso(depth: f32, root: RenderBoneTransform) -> Collider {
        let geometry = ActorRigGeometry::synthetic_cuboid(
            EntityRigId(1),
            [-0.25, -0.75, -depth],
            [0.25, 0.0, depth],
            1,
        )
        .unwrap();
        Collider::geometry("body", &geometry, 0, root, Mat4::IDENTITY).unwrap()
    }

    /// Samples a point in an attachment bone as the renderer does.
    fn point(local: Vec3, bone: RenderBoneTransform) -> Vec3 {
        Vec3::from_array(
            crate::actor::sampled_bone_offset(
                local.to_array(),
                bone,
                bone,
                [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
                1.0,
            )
            .unwrap()
            .position,
        )
    }

    #[test]
    fn pins_and_mounts_fit_bare_standard_and_thick_armor_after_rotation() {
        let item = crate::hats::fixture::cloth();
        let model = rig(item);
        let cloth = model.cloth.as_ref().unwrap();
        let root = RenderBoneTransform {
            rotation: (Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.15)).to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: [0.8, 1.2, 0.9, 1.0],
        };
        let base = model.pose(root, super::super::super::Motion::default());
        let actor = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        for depth in [2.0 / 16.0, 3.0 / 16.0, 5.0 / 16.0] {
            let body = torso(2.0 / 16.0, root);
            let armor = torso(depth, root);
            let mut state = ClothState::default();
            let (_, pose) = state
                .pose(
                    item,
                    [base.clone(), base.clone()],
                    actor,
                    (STEP, 1.0),
                    &[body, armor],
                )
                .unwrap();
            let inverse = Mat4::from_scale_rotation_translation(
                Vec3::new(0.8, 1.2, 0.9),
                Quat::from_array(root.rotation),
                Vec3::new(0.0, 1.5, 0.0),
            )
            .inverse();
            let pin = inverse.transform_point3(state.points[0]);
            assert!((pin.z - depth - CONTACT_MARGIN - CONTACT_SEPARATION / 0.9).abs() < 0.002);
            // A mount ending at the authored cloth plane must still meet the moved fabric pins.
            let pivot = Vec3::from_array(model.geometry.bone_pivots[model.head]);
            let mut mount = cloth.rest[0] - pivot;
            mount.x *= 0.89;
            assert!(point(mount, pose[model.head]).abs_diff_eq(state.points[0], 1e-5));
            let front = inverse.transform_point3(point(
                Vec3::new(0.0, 0.0, -2.0 / 16.0 - CONTACT_MARGIN),
                pose[model.head],
            ));
            assert!((front.z + depth + CONTACT_MARGIN).abs() < 1e-5);
        }
    }

    #[test]
    fn armor_changes_transfer_existing_drape_and_velocity_and_hidden_views_keep_fit() {
        let item = crate::hats::fixture::dense_cloth();
        let model = rig(item);
        let root = RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: UNIT_AXIS_SCALE,
        };
        let base = model.pose(root, super::super::super::Motion::default());
        let actor = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let mut state = ClothState::default();
        for depth in [2.0 / 16.0, 4.0 / 16.0, 2.0 / 16.0] {
            let points = state.points.clone();
            let old = state.old.clone();
            let frame = state.previous_frame;
            let contacts = [torso(2.0 / 16.0, root), torso(depth, root)];
            state
                .pose(
                    item,
                    [base.clone(), base.clone()],
                    actor,
                    (0.0, 1.0),
                    &contacts,
                )
                .unwrap();
            if let Some(frame) = frame {
                let transfer = state.previous_frame.unwrap() * frame.inverse();
                for (index, point) in state.points.iter().enumerate() {
                    assert!(point.abs_diff_eq(transfer.transform_point3(points[index]), 1e-5));
                    assert!(
                        (*point - state.old[index]).abs_diff_eq(
                            transfer.transform_vector3(points[index] - old[index]),
                            1e-5
                        )
                    );
                }
            }
            for index in model.cloth.as_ref().unwrap().columns + 1..state.points.len() {
                state.points[index].z += 0.04;
                state.old[index].z += 0.03;
            }
            let fitted = state.previous_frame.unwrap();
            state.retain_fitting();
            state
                .pose(
                    item,
                    [base.clone(), base.clone()],
                    actor,
                    (0.0, 1.0),
                    &[torso(2.0 / 16.0, root)],
                )
                .unwrap();
            assert!(state.previous_frame.unwrap().abs_diff_eq(fitted, 1e-6));
        }
    }
}
