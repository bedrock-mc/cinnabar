use ui::{UiNode, UiNodeId, UiVisual};

use {
    super::{HudLayout, UiPresentationError, rect},
    ui::IconRef,
};

impl HudLayout<'_> {
    /// First-person item presentation. Skin-backed arm geometry is paired
    /// with cached, depth-rasterized item geometry instead of flat icon quads.
    pub(super) fn held_items(
        &mut self,
        frame: &super::HudFrame,
    ) -> Result<(), UiPresentationError> {
        // The viewmodel omits these quads only when it drew the hand itself; the rig never does.
        if frame.hand_rig_active {
            return Ok(());
        }
        let g = self.geometry;
        const HAND_SIZE: [f32; 2] = [72.0, 88.0];
        const ITEM_SIZE: f32 = 64.0;
        const HAND_Y: f32 = 76.0;
        const ITEM_Y: f32 = 78.0;
        let pitch = frame.viewmodel_pitch_degrees.to_radians().clamp(-1.2, 1.2);
        let main_item_angle = (-pitch * 0.04).clamp(-0.08, 0.08);
        let offhand_item_angle = (pitch * 0.04).clamp(-0.08, 0.08);
        // Java/Bedrock only expose the offhand carrier when there is an
        // offhand item to present. Keeping an empty left carrier hidden avoids
        // the two-block silhouette that made the earlier fallback visibly
        // non-vanilla.
        if frame.offhand_viewmodel_icon.is_some()
            && let Some(hand) = frame.left_hand
        {
            self.hand_sprite(hand, [-10.0, g.gui_height - HAND_Y], HAND_SIZE)?;
        }
        // The player render controller shows the right arm only for an empty hand.
        if let Some(hand) = frame.right_hand.filter(|_| frame.held_item_icon.is_none()) {
            self.hand_sprite(
                hand,
                [g.gui_width - HAND_SIZE[0] + 10.0, g.gui_height - HAND_Y],
                HAND_SIZE,
            )?;
        }
        if let Some(icon) = frame.offhand_viewmodel_icon {
            self.item_sprite(
                icon,
                [-10.0, g.gui_height - ITEM_Y],
                ITEM_SIZE,
                offhand_item_angle,
            )?;
        }
        if let Some(icon) = frame.held_item_icon {
            self.item_sprite(
                icon,
                [g.gui_width - ITEM_SIZE + 10.0, g.gui_height - ITEM_Y],
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
