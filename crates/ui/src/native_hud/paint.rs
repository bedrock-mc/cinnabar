//! The HUD's native renderers: the `custom` controls `hud_screen.json` places
//! (hearts, armor, hunger, bubbles, mount hearts and jump bar, hotbar slot art,
//! status effects, the crosshair) draw at their control's position, as the
//! client's renderers do, so a pack that moves the control moves the art. What
//! each draws is captured once per frame into [`HudPaint`]. Native cells obey
//! the control's inherited clip and viewport.

use crate::UiVisual;
use super::HeartPaint;

pub const CROSSHAIR_TEXTURE: &str = "textures/ui/cross_hair";
pub const CROSSHAIR_SIDE: f32 = 16.0;

/// Resolves full sprites and emits retained geometry in the caller's active clip.
pub trait HudPaintTarget {
    fn gui_pixel_scale(&self) -> f32;
    fn visible_bounds(&self) -> [f32; 4];
    fn sprite(&self, path: &str, color: [u8; 4]) -> Option<UiVisual>;
    fn push(&mut self, visual: UiVisual, bounds: [f32; 4]);
    fn solid(&mut self, bounds: [f32; 4], color: [u8; 4]);
}

/// One sprite a renderer draws, relative to its control's origin, in GUI px.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub at: [f32; 2],
    pub size: [f32; 2],
    /// A `textures/ui/...` path, which a server pack may replace.
    pub texture: &'static str,
    /// Tried before `texture` (hardcore hearts), when the pack holds it.
    pub preferred: Option<&'static str>,
    pub alpha: u8,
}

impl Cell {
    pub fn icon(at: [f32; 2], texture: &'static str) -> Self {
        Self {
            at,
            size: [9.0, 9.0],
            texture,
            preferred: None,
            alpha: 255,
        }
    }
}

/// The hotbar cells' background art, by cell index.
const SLOT_ART: [&str; 9] = [
    "textures/ui/hotbar_0",
    "textures/ui/hotbar_1",
    "textures/ui/hotbar_2",
    "textures/ui/hotbar_3",
    "textures/ui/hotbar_4",
    "textures/ui/hotbar_5",
    "textures/ui/hotbar_6",
    "textures/ui/hotbar_7",
    "textures/ui/hotbar_8",
];

/// A sprite from the HUD carrier's own page (art the pack's `textures/ui` lacks).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetSprite {
    pub page: u16,
    pub uv: [u16; 4],
}

/// What the native renderers draw this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudPaint {
    /// Relative to the heart control's top-left; rows grow upward.
    pub hearts: HeartPaint,
    /// Relative to the armor control's top-left, above the heart rows.
    pub armor: Vec<Cell>,
    /// Relative to the hunger control's position, which is the row's right end.
    pub hunger: Vec<Cell>,
    pub bubbles: Vec<Cell>,
    pub mount_hearts: Vec<Cell>,
    /// Relative to the effects control's top-right corner.
    pub effects: Vec<Cell>,
    /// Jump-bar background and fill (with its filled GUI width) over the XP bar.
    pub mount_jump: Option<(SheetSprite, SheetSprite, f32)>,
    pub crosshair: Option<SheetSprite>,
    pub crosshair_blend: crate::UiBlendMode,
}

impl HudPaint {
    /// Every texture path the renderers may draw this frame, pack overrides included.
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        [
            &self.armor,
            &self.hunger,
            &self.bubbles,
            &self.mount_hearts,
            &self.effects,
        ]
        .into_iter()
        .flatten()
        .flat_map(|cell| cell.preferred.into_iter().chain([cell.texture]))
        .chain(self.hearts.textures())
        .chain(SLOT_ART)
        .chain(self.crosshair.map(|_| CROSSHAIR_TEXTURE))
    }
}

/// Draw `renderer` at `dest` when it is one of the HUD's; `false` otherwise.
pub fn paint(
    painter: &mut impl HudPaintTarget,
    hud: &HudPaint,
    renderer: &str,
    collection_index: usize,
    bar_notches: u32,
    dest: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) -> bool {
    let top_left = [dest[0], dest[1]];
    let cells = match renderer {
        "heart_renderer" => {
            let visible = visible_bounds(painter);
            if visible[0] < visible[2] && visible[1] < visible[3] {
                for cell in hud.hearts.visible_cells(top_left, painter.gui_pixel_scale(), visible) {
                    paint_cell(painter, &cell, top_left, visible, alpha);
                }
            }
            return true;
        }
        "armor_renderer" => (&hud.armor, top_left),
        "hunger_renderer" => (&hud.hunger, top_left),
        "bubbles_renderer" => (&hud.bubbles, top_left),
        "horse_heart_renderer" => (&hud.mount_hearts, top_left),
        "mob_effects_renderer" => (&hud.effects, [dest[2], dest[1]]),
        "hotbar_renderer" => {
            slot_art(painter, collection_index, dest, alpha);
            return true;
        }
        "horse_jump_renderer" => {
            if let Some((background, fill, filled)) = hud.mount_jump {
                let px = painter.gui_pixel_scale();
                sheet(painter, background, dest, alpha);
                let width = (filled * px).min(dest[2] - dest[0]);
                if width > 0.0 {
                    let mut uv = fill.uv;
                    let span = f32::from(uv[2] - uv[0]);
                    uv[2] = uv[0] + (span * filled / 182.0).round() as u16;
                    let clipped = SheetSprite { uv, ..fill };
                    sheet(
                        painter,
                        clipped,
                        [dest[0], dest[1], dest[0] + width, dest[3]],
                        alpha,
                    );
                }
            }
            return true;
        }
        // The built-in Java pack's notched boss-bar overlay.
        "java_boss_notches" => {
            let notches = bar_notches;
            let width = dest[2] - dest[0];
            let px = painter.gui_pixel_scale();
            for notch in 1..notches {
                let x = dest[0] + width * notch as f32 / notches as f32;
                painter.solid([x, dest[1], x + px, dest[3]], alpha([0, 0, 0, 255]));
            }
            return true;
        }
        "cursor_renderer" => {
            if let Some(sprite) = hud.crosshair {
                crosshair(painter, sprite, dest, hud.crosshair_blend);
            }
            return true;
        }
        // Renderers with no Cinnabar state draw nothing, as with no data.
        "hotbar_cooldown_renderer"
        | "dash_renderer"
        | "locator_bar"
        | "vignette_renderer"
        | "progress_indicator_renderer"
        | "camera_renderer"
        | "editor_gizmo_renderer"
        | "editor_compass_renderer"
        | "editor_volume_highlight_renderer" => return true,
        _ => return false,
    };
    let (cells, origin) = cells;
    let visible = visible_bounds(painter);
    if visible[0] < visible[2] && visible[1] < visible[3] {
        for cell in cells {
            paint_cell(painter, cell, origin, visible, alpha);
        }
    }
    true
}

/// Intersects the inherited clip with the physical viewport.
fn visible_bounds(painter: &impl HudPaintTarget) -> [f32; 4] {
    painter.visible_bounds()
}

/// Emits one cell only when its animated bounds intersect the visible region.
fn paint_cell(
    painter: &mut impl HudPaintTarget,
    cell: &Cell,
    origin: [f32; 2],
    visible: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let px = painter.gui_pixel_scale();
    let x = origin[0] + cell.at[0] * px;
    let y = origin[1] + cell.at[1] * px;
    let bounds = [x, y, x + cell.size[0] * px, y + cell.size[1] * px];
    if bounds[2] <= visible[0]
        || bounds[3] <= visible[1]
        || bounds[0] >= visible[2]
        || bounds[1] >= visible[3]
    {
        return;
    }
    let color = alpha([255, 255, 255, cell.alpha]);
    let visual = cell
        .preferred
        .and_then(|path| painter.sprite(path, color))
        .or_else(|| {
            painter.sprite(cell.texture, color)
        });
    if let Some(visual) = visual {
        painter.push(visual, bounds);
    }
}

/// The hotbar slot background for the cell the control's collection index names.
fn slot_art(
    painter: &mut impl HudPaintTarget,
    collection_index: usize,
    dest: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let index = collection_index.min(8);
    if let Some(visual) = painter.sprite(SLOT_ART[index], alpha([255; 4])) {
        painter.push(visual, dest);
    }
}

fn sheet(
    painter: &mut impl HudPaintTarget,
    sprite: SheetSprite,
    bounds: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let visual = UiVisual::Sprite {
        texture_page: sprite.page,
        uv: sprite.uv,
        color: alpha([255; 4]),
    };
    painter.push(visual, bounds);
}

/// Centers the pack crosshair or built-in art with the selected color blending.
fn crosshair(
    painter: &mut impl HudPaintTarget,
    sprite: SheetSprite,
    dest: [f32; 4],
    blend: crate::UiBlendMode,
) {
    let (sprite, gui_side) = match painter.sprite(CROSSHAIR_TEXTURE, [255; 4]) {
        Some(UiVisual::Sprite { texture_page, uv, .. }) => (
            SheetSprite { page: texture_page, uv }, CROSSHAIR_SIDE,
        ),
        _ => (sprite, assets::HudTextureRole::Crosshair.expected_size()[0] as f32),
    };
    let side = gui_side * painter.gui_pixel_scale();
    let x = (dest[0] + dest[2] - side) * 0.5;
    let y = (dest[1] + dest[3] - side) * 0.5;
    let visual = match blend {
        crate::UiBlendMode::Invert => UiVisual::InvertedSprite {
            texture_page: sprite.page,
            uv: sprite.uv,
        },
        crate::UiBlendMode::Alpha => UiVisual::Sprite {
            texture_page: sprite.page,
            uv: sprite.uv,
            color: [255; 4],
        },
    };
    painter.push(visual, [x, y, x + side, y + side]);
}
