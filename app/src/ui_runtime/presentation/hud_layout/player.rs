use ui::{UiNode, UiNodeId, UiVisual};

use super::{HudLayout, HudTextureRole, IconRef, UiPresentationError, rect};

impl HudLayout<'_> {
    /// Crosshair-attached melee charge, drawn only below full charge. Bedrock
    /// does not publish a cooldown, so production keeps this hidden.
    pub(super) fn attack_indicator(
        &mut self,
        frame: &super::HudFrame,
    ) -> Result<(), UiPresentationError> {
        let Some(charge) = frame.attack_indicator_charge else {
            return Ok(());
        };
        if charge >= 1.0 {
            return Ok(());
        }
        let g = self.geometry;
        let left = (g.gui_width - 16.0) / 2.0;
        let top = g.gui_height / 2.0 + 9.0;
        self.solid_gui([left, top], [16.0, 2.0], [0, 0, 0, 170])?;
        let filled = (charge.clamp(0.0, 1.0) * 16.0).floor();
        if filled >= 1.0 {
            self.solid_gui([left, top], [filled, 2.0], [255, 255, 255, 255])?;
        }
        Ok(())
    }

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

    /// First-person item presentation. Skin-backed arm geometry is paired
    /// with cached, depth-rasterized item geometry instead of flat icon quads.
    pub(super) fn held_items(
        &mut self,
        frame: &super::HudFrame,
    ) -> Result<(), UiPresentationError> {
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
        if let Some(hand) = frame.right_hand {
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
