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
        .with_visual(UiVisual::Sprite {
            texture_page: self.textures.page,
            uv: sprite.uv,
            color: [255; 4],
        });
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    pub(super) fn player_preview(&mut self, preview: IconRef) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical([4.0, 4.0]);
        // The raster includes transparent breathing room around the model;
        // this display size leaves the avatar readable without competing with
        // Java-style chat or the sidebar.
        self.nodes.push(
            UiNode::new(
                UiNodeId::new(*self.next_id),
                None,
                rect(x, y, x + 64.0 * g.scale, y + 75.0 * g.scale)?,
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
}
