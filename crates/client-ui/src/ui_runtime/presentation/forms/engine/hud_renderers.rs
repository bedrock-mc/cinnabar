//! The HUD's native renderers: the `custom` controls `hud_screen.json` places
//! (hearts, armor, hunger, bubbles, mount hearts and jump bar, hotbar slot art,
//! status effects, the crosshair) draw at their control's position, as the
//! client's renderers do, so a pack that moves the control moves the art. What
//! each draws is captured once per frame into [`HudPaint`]. Native cells obey
//! the control's inherited clip and viewport.

use std::collections::BTreeMap;

use serde_json::Value;
use ui::UiVisual;

use super::Painter;
use crate::ui_runtime::presentation::hud_layout::HeartPaint;

const CROSSHAIR_TEXTURE: &str = "textures/ui/cross_hair";
const CROSSHAIR_SIDE: f32 = 16.0;

#[cfg(test)]
mod crosshair_tests;

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
    pub crosshair_blend: ui::UiBlendMode,
    pub custom_crosshair: Option<ui::mod_hud::Crosshair>,
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
        .chain(self.crosshair.into_iter().flat_map(|_| {
            [
                CROSSHAIR_TEXTURE,
                assets::HudTextureRole::Crosshair.source_path(),
            ]
        }))
    }
}

/// Draw `renderer` at `dest` when it is one of the HUD's; `false` otherwise.
pub(super) fn paint(
    painter: &mut Painter<'_>,
    hud: &HudPaint,
    renderer: &str,
    data: &BTreeMap<String, Value>,
    dest: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) -> bool {
    let top_left = [dest[0], dest[1]];
    let cells = match renderer {
        "heart_renderer" => {
            let visible = visible_bounds(painter);
            if visible[0] < visible[2] && visible[1] < visible[3] {
                for cell in hud.hearts.visible_cells(top_left, painter.px, visible) {
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
            slot_art(painter, data, dest, alpha);
            return true;
        }
        "horse_jump_renderer" => {
            if let Some((background, fill, filled)) = hud.mount_jump {
                let px = painter.px;
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
            let notches = data
                .get("#bar_notches")
                .and_then(Value::as_f64)
                .map_or(0, |notches| notches.clamp(0.0, 64.0) as u32);
            let width = dest[2] - dest[0];
            let px = painter.px;
            for notch in 1..notches {
                let x = dest[0] + width * notch as f32 / notches as f32;
                let _ = painter.solid([x, dest[1], x + px, dest[3]], alpha([0, 0, 0, 255]));
            }
            return true;
        }
        "cursor_renderer" => {
            if let Some(sprite) = hud.crosshair {
                if let Some(spec) = &hud.custom_crosshair {
                    if !painter.mod_crosshair(spec, dest) {
                        crosshair(painter, sprite, dest, hud.crosshair_blend);
                    }
                } else {
                    crosshair(painter, sprite, dest, hud.crosshair_blend);
                }
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
fn visible_bounds(painter: &Painter<'_>) -> [f32; 4] {
    let clip = painter.clip.map_or(painter.screen, |(bounds, _)| bounds);
    [
        clip[0].max(painter.screen[0]),
        clip[1].max(painter.screen[1]),
        clip[2].min(painter.screen[2]),
        clip[3].min(painter.screen[3]),
    ]
}

/// Emits one cell only when its animated bounds intersect the visible region.
fn paint_cell(
    painter: &mut Painter<'_>,
    cell: &Cell,
    origin: [f32; 2],
    visible: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let px = painter.px;
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
        .and_then(|path| painter.sprite(path, json_ui::UvRect::full(), color, Default::default()))
        .or_else(|| {
            painter.sprite(
                cell.texture,
                json_ui::UvRect::full(),
                color,
                Default::default(),
            )
        });
    if let Some(visual) = visual {
        let _ = painter.push(visual, bounds);
    }
}

/// The hotbar slot background for the cell the control's collection index names.
fn slot_art(
    painter: &mut Painter<'_>,
    data: &BTreeMap<String, Value>,
    dest: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let index = data
        .get("#collection_index")
        .and_then(Value::as_f64)
        .map_or(0, |index| index.clamp(0.0, 8.0) as usize);
    if let Some(visual) = painter.sprite(
        SLOT_ART[index],
        json_ui::UvRect::full(),
        alpha([255; 4]),
        Default::default(),
    ) {
        let _ = painter.push(visual, dest);
    }
}

fn sheet(
    painter: &mut Painter<'_>,
    sprite: SheetSprite,
    bounds: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let visual = UiVisual::Sprite {
        texture_page: sprite.page,
        uv: sprite.uv,
        color: alpha([255; 4]),
    };
    let _ = painter.push(visual, bounds);
}

/// Centers the pack crosshair or built-in art with the selected color blending.
fn crosshair(
    painter: &mut Painter<'_>,
    sprite: SheetSprite,
    dest: [f32; 4],
    blend: ui::UiBlendMode,
) {
    let role = assets::HudTextureRole::Crosshair;
    let standalone =
        painter
            .textures
            .sprite(CROSSHAIR_TEXTURE)
            .map(|(page, [x, y, width, height])| {
                (
                    SheetSprite {
                        page,
                        uv: [x, y, x + width, y + height].map(|value| value as u16),
                    },
                    CROSSHAIR_SIDE,
                )
            });
    let (sprite, gui_side) = standalone
        .or_else(|| {
            let (page, [x, y, width, height]) = painter.textures.sprite(role.source_path())?;
            let [crop_x, crop_y, crop_width, crop_height] = role.source_crop()?;
            let [native_width, native_height] = assets::HUD_ICONS_SHEET_SIZE;
            let scale_x = width / native_width as f32;
            let scale_y = height / native_height as f32;
            Some((
                SheetSprite {
                    page,
                    uv: [
                        x + crop_x as f32 * scale_x,
                        y + crop_y as f32 * scale_y,
                        x + (crop_x + crop_width) as f32 * scale_x,
                        y + (crop_y + crop_height) as f32 * scale_y,
                    ]
                    .map(|value| value.round() as u16),
                },
                role.expected_size()[0] as f32,
            ))
        })
        .unwrap_or((sprite, role.expected_size()[0] as f32));
    let side = gui_side * painter.px;
    let x = (dest[0] + dest[2] - side) * 0.5;
    let y = (dest[1] + dest[3] - side) * 0.5;
    let visual = match blend {
        ui::UiBlendMode::Invert => UiVisual::InvertedSprite {
            texture_page: sprite.page,
            uv: sprite.uv,
        },
        ui::UiBlendMode::Alpha => UiVisual::Sprite {
            texture_page: sprite.page,
            uv: sprite.uv,
            color: [255; 4],
        },
    };
    let _ = painter.push(visual, [x, y, x + side, y + side]);
}

/// Layers the built-in HUD and menu styling over vanilla before server packs.
pub(super) fn with_java_hud(vanilla: &json_ui::Catalog) -> json_ui::Catalog {
    let mut catalog = vanilla.clone();
    let hud_files = super::super::hud::JAVA_HUD_PACK
        .iter()
        .map(|(path, _, bytes)| (*path, *bytes));
    super::super::graphics_expander::install(&mut catalog);
    super::super::always_sprint_setting::install(&mut catalog);
    super::super::vsync_setting::install(&mut catalog);
    super::super::motion_blur_setting::install(&mut catalog);
    super::super::java_animations_setting::install(&mut catalog);
    super::super::discord_presence_setting::install(&mut catalog);
    super::super::crosshair_settings::install(&mut catalog);
    catalog.apply_pack(hud_files);
    catalog.apply_pack(
        [(
            "ui/ui_art_assets_common.json",
            super::menu_renderers::TITLE_PANEL_OVERLAY,
        )]
        .into_iter()
        .chain(super::menu_renderers::NO_COPYRIGHT_OVERLAYS),
    );
    super::super::loading_screen::install_brand_layout(&mut catalog);
    super::super::enhanced_setting::install(&mut catalog);
    super::super::chat_position::install(&mut catalog);
    catalog
}
