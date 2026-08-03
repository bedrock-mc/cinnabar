use ui::{UiNode, UiNodeId, UiVisual};

use super::{HudLayout, HudTextureRole, IconRef, UiPresentationError, rect};

impl HudLayout<'_> {
    /// 15x15 vanilla-alpha crosshair centered exactly on the framebuffer
    /// center: the fractional GUI remainder of a non-divisible viewport is
    /// kept rather than floored, so the quad's center equals width/2 and
    /// height/2 in physical pixels at every GUI scale, aspect, and DPI.
    pub(super) fn crosshair(&mut self) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let x = (g.gui_width - 15.0) / 2.0;
        let y = (g.gui_height - 15.0) / 2.0;
        let sprite = self.textures.sprite(HudTextureRole::Crosshair);
        let [left, top] = g.logical([x, y]);
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(left, top, left + 15.0 * g.scale, top + 15.0 * g.scale)?,
        )
        .with_visual(UiVisual::InvertedSprite {
            texture_page: self.textures.page,
            uv: sprite.uv,
        });
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    pub(super) fn player_preview(&mut self, preview: IconRef) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical([4.0, 4.0]);
        // The raster includes transparent breathing room around the model;
        // this keeps the corner avatar close to vanilla HUD proportions at
        // the Java GUI scale instead of dominating the gameplay view.
        self.nodes.push(
            UiNode::new(
                UiNodeId::new(*self.next_id),
                None,
                rect(x, y, x + 24.0 * g.scale, y + 28.0 * g.scale)?,
            )
            .with_visual(UiVisual::Sprite {
                texture_page: preview.page,
                uv: preview.uv,
                color: [255; 4],
            }),
        );
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    /// First-person item presentation. The skin-backed arm carriers give the
    /// view a proper hand silhouette even when the selected slot is empty;
    /// authoritative item sprites sit over that carrier until the render
    /// pipeline owns a true transformed 3-D viewmodel.
    pub(super) fn held_items(
        &mut self,
        frame: &super::HudFrame,
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        const HAND_SIZE: [f32; 2] = [80.0, 104.0];
        const ITEM_SIZE: f32 = 48.0;
        const HAND_Y: f32 = 92.0;
        const ITEM_Y: f32 = 92.0;
        let pitch = frame.viewmodel_pitch_degrees.to_radians().clamp(-1.2, 1.2);
        // LCE's ItemInHandRenderer applies a camera-facing tilt before its
        // 3-D item transform. The UI compatibility carrier cannot perform
        // the cuboid transform yet, but preserving this small 2-D component
        // keeps the hand/item pair moving in the same direction as the
        // native path rather than staying screen-upright.
        let main_item_angle = (-0.18 - pitch * 0.12).clamp(-0.45, 0.25);
        let offhand_item_angle = (0.18 + pitch * 0.12).clamp(-0.25, 0.45);
        // Java/Bedrock only expose the offhand carrier when there is an
        // offhand item to present. Keeping an empty left carrier hidden avoids
        // the two-block silhouette that made the earlier fallback visibly
        // non-vanilla.
        if frame.offhand_icon.is_some()
            && let Some(hand) = frame.left_hand
        {
            self.hand_sprite(hand, [8.0, g.gui_height - HAND_Y], HAND_SIZE)?;
        }
        if let Some(hand) = frame.right_hand {
            self.hand_sprite(
                hand,
                [g.gui_width - HAND_SIZE[0] - 8.0, g.gui_height - HAND_Y],
                HAND_SIZE,
            )?;
        }
        if let Some(icon) = frame.offhand_icon {
            self.item_sprite(
                icon,
                [10.0, g.gui_height - ITEM_Y],
                ITEM_SIZE,
                offhand_item_angle,
            )?;
        }
        if let Some(icon) = frame.held_item_icon {
            self.item_sprite(
                icon,
                [g.gui_width - ITEM_SIZE - 14.0, g.gui_height - ITEM_Y],
                ITEM_SIZE,
                main_item_angle,
            )?;
        }
        Ok(())
    }

    fn hand_sprite(
        &mut self,
        icon: IconRef,
        gui: [f32; 2],
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let bounds = rect(x, y, x + size[0] * g.scale, y + size[1] * g.scale)?;
        self.nodes.push(
            UiNode::new(UiNodeId::new(*self.next_id), None, bounds).with_visual(UiVisual::Sprite {
                texture_page: icon.page,
                uv: icon.uv,
                color: [255; 4],
            }),
        );
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    fn item_sprite(
        &mut self,
        icon: IconRef,
        gui: [f32; 2],
        size: f32,
        angle_radians: f32,
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let bounds = rect(x, y, x + size * g.scale, y + size * g.scale)?;
        self.nodes.push(
            UiNode::new(UiNodeId::new(*self.next_id), None, bounds).with_visual(
                UiVisual::RotatedSprite {
                    texture_page: icon.page,
                    uv: icon.uv,
                    color: [255; 4],
                    angle_radians,
                },
            ),
        );
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }
}
