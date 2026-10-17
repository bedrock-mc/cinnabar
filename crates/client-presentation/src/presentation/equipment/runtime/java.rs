//! Java 1.7 held-item placement: third-person grips on the posed arm and first-person items
//! under Java's hand stack.

use bevy::math::Mat4;
use render_model::java_animation::{
    self as java, JavaHand, JavaHeldItem, JavaItemMesh, is_java_sword,
};

use {
    super::*,
    render_model::equipment::{ItemDisplay, attach_to_bone},
};

const BOW: &str = assets::facts::VanillaItem::Bow.identifier();

fn mesh_kind(block: bool) -> JavaItemMesh {
    if block {
        JavaItemMesh::Block
    } else {
        JavaItemMesh::Sprite
    }
}

/// A bone at rest about `pivot` (rig blocks), leaving placement to the instance.
pub(super) fn rest_bone(pivot: [f32; 3]) -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [pivot[0], pivot[1], pivot[2], 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    }
}

impl EquipmentRuntime {
    /// The main-hand item in Java's third-person grip on the right arm; `false` when the body
    /// lacks the arm or the item has no drawable mesh.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_java_held(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        bones: &BodyBones,
        grip: JavaGrip,
        scale: f32,
        layers: &mut Vec<EquipmentPresentation>,
    ) -> bool {
        let Some(arm) = bones.right_arm else {
            return false;
        };
        let (Some(previous), Some(current)) = (
            body.input.previous_bones.get(arm),
            body.input.current_bones.get(arm),
        ) else {
            return false;
        };
        let Some((mesh, location, block, span)) = self.held_mesh(item, true) else {
            return false;
        };
        let grip_kind = if block {
            JavaHeldItem::Block
        } else if &*item.identifier == BOW {
            JavaHeldItem::Bow
        } else if item.grip.upright() == Some(true) {
            JavaHeldItem::Tool {
                rotate_around: item.grip.mirrored_art(),
                blocking: grip.blocking && is_java_sword(&item.identifier),
            }
        } else {
            JavaHeldItem::Flat
        };
        // The arm frame, not the hand bone, which vanilla animations still turn.
        let mut display =
            ItemDisplay::from_matrix(java::third_person_item(grip_kind, mesh_kind(block)));
        display.scale *= scale;
        let (Some(previous), Some(current)) = (
            attach_to_bone(*previous, display),
            attach_to_bone(*current, display),
        ) else {
            return false;
        };
        let poses = self
            .poses
            .share(body, LAYER_MAIN_HAND, [&[previous], &[current]]);
        layers.push(super::push::held_glint(
            layer_presentation(body, LAYER_MAIN_HAND, mesh, poses, location, 0),
            item,
            span,
        ));
        true
    }

    /// The main-hand item placed by Java's first-person stack in camera space.
    pub fn first_person_java_item(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        hand: JavaHand,
    ) -> Option<FirstPersonItem> {
        self.first_person_java_item_with_use(body, item, hand, Default::default())
    }

    /// Uses native stage artwork with Java's first-person swing and use transforms.
    pub fn first_person_java_item_with_use(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        hand: JavaHand,
        input: actor_animation::AttachableAnimationInput<'_>,
    ) -> Option<FirstPersonItem> {
        let (mesh, location, block, span) = self.held_mesh_with_use(item, input)?;
        let camera = java::first_person_item(hand, mesh_kind(block), item.grip.mirrored_art())
            * bevy::math::Mat4::from_scale(bevy::math::Vec3::splat(input.main_hand_scale()));
        let rest = [rest_bone([0.0; 3])];
        let poses = self
            .poses
            .share(body, FIRST_PERSON_ITEM_LAYER, [&rest, &rest]);
        Some(FirstPersonItem {
            presentation: super::push::held_glint(
                layer_presentation(body, LAYER_MAIN_HAND, mesh, poses, location, 0),
                item,
                span,
            ),
            camera_space: true,
            alpha_mode: self.first_person_alpha_mode(item, block),
            java_camera: camera.is_finite().then_some(camera),
            java_normal_axis: bevy::math::Vec3::Z,
        })
    }
}

/// Removes authored raster placement before scaling the image under Java's first-person stack.
pub(super) fn java_raster_camera(
    hand: JavaHand,
    image_to_rig: Mat4,
    width: u16,
    height: u16,
    held_scale: f32,
) -> Mat4 {
    java::first_person_item(hand, JavaItemMesh::Raster { width, height }, false)
        * Mat4::from_scale(bevy::math::Vec3::splat(held_scale))
        * image_to_rig.inverse()
}

/// Whether Java's first-person stack draws this attachable (its raster pull frames) itself.
pub fn java_draws_attachable(identifier: &str) -> bool {
    identifier == BOW
}

/// Server-pack attachables own their hand poses, including replacements for Java's bow.
pub(super) fn keep_authored_hand(
    identifier: &str,
    category: EquipmentCategory,
    from_pack: bool,
) -> bool {
    matches!(
        category,
        EquipmentCategory::Held | EquipmentCategory::Shield
    ) && (from_pack || !java_draws_attachable(identifier))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::{Vec3, Vec4};

    #[test]
    fn pack_bow_keeps_its_authored_hand_while_vanilla_bow_uses_java() {
        assert!(keep_authored_hand(BOW, EquipmentCategory::Held, true));
        assert!(!keep_authored_hand(BOW, EquipmentCategory::Held, false));
        for item in ["minecraft:crossbow", "minecraft:trident"] {
            assert!(keep_authored_hand(item, EquipmentCategory::Held, false));
        }
        assert!(keep_authored_hand(
            "minecraft:shield",
            EquipmentCategory::Shield,
            false
        ));
    }

    /// The raster camera undoes the image placement before Java's slab mapping.
    #[test]
    fn raster_camera_composes_through_the_image_frame() {
        let hand = JavaHand {
            swing: 0.0,
            equip: 1.0,
            using: None,
        };
        let image_to_rig = Mat4::from_translation(Vec3::new(0.1, 0.2, 0.3))
            * Mat4::from_rotation_y(1.0)
            * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
        let point = Vec4::new(3.5, 0.0, 9.5, 1.0);
        let through = java_raster_camera(hand, image_to_rig, 16, 16, 1.0) * (image_to_rig * point);
        let direct = java::first_person_item(
            hand,
            JavaItemMesh::Raster {
                width: 16,
                height: 16,
            },
            false,
        ) * point;
        assert!(through.abs_diff_eq(direct, 1e-5));
    }
}
