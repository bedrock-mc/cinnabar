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

    /// First-person item presentation. This is intentionally a UI-layer
    /// fallback until the world renderer grows a held-item model: the sprite
    /// still comes from the authoritative item registry, so vanilla tools and
    /// blocks never turn into a guessed placeholder. The small shadow keeps
    /// bright items legible over sky and terrain without a backdrop panel.
    pub(super) fn held_items(
        &mut self,
        frame: &super::HudFrame,
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        if let Some(icon) = frame.offhand_icon {
            self.item_sprite(icon, [12.0, g.gui_height - 75.0], 48.0)?;
        }
        if let Some(icon) = frame.held_item_icon {
            self.item_sprite(icon, [g.gui_width - 60.0, g.gui_height - 75.0], 48.0)?;
        }
        Ok(())
    }

    fn item_sprite(
        &mut self,
        icon: IconRef,
        gui: [f32; 2],
        size: f32,
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let bounds = rect(x, y, x + size * g.scale, y + size * g.scale)?;
        self.nodes.push(
            UiNode::new(UiNodeId::new(*self.next_id), None, bounds).with_visual(UiVisual::Sprite {
                texture_page: icon.page,
                uv: icon.uv,
                color: [0, 0, 0, 105],
            }),
        );
        *self.next_id = self.next_id.saturating_add(1);
        let bounds = rect(
            x - g.scale,
            y - g.scale,
            x + size * g.scale - g.scale,
            y + size * g.scale - g.scale,
        )?;
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
}
