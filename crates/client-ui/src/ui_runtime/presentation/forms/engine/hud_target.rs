//! Native retained-tree adapter for shared HUD paint decisions.

use ui::{UiVisual, native_hud::HudPaintTarget};

use super::Painter;

impl HudPaintTarget for Painter<'_> {
    fn gui_pixel_scale(&self) -> f32 {
        self.px
    }

    fn visible_bounds(&self) -> [f32; 4] {
        let clip = self.clip.map_or(self.screen, |(bounds, _)| bounds);
        [
            clip[0].max(self.screen[0]),
            clip[1].max(self.screen[1]),
            clip[2].min(self.screen[2]),
            clip[3].min(self.screen[3]),
        ]
    }

    fn sprite(&self, path: &str, color: [u8; 4]) -> Option<UiVisual> {
        self.sprite(path, json_ui::UvRect::full(), color, Default::default())
    }

    fn push(&mut self, visual: UiVisual, bounds: [f32; 4]) {
        let _ = self.push(visual, bounds);
    }

    fn solid(&mut self, bounds: [f32; 4], color: [u8; 4]) {
        let _ = self.solid(bounds, color);
    }
}
