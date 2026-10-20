//! The HUD's native renderers: the `custom` controls `hud_screen.json` places
//! (hearts, armor, hunger, bubbles, mount hearts and jump bar, hotbar slot art,
//! status effects, the crosshair) draw at their control's position, as the
//! client's renderers do, so a pack that moves the control moves the art. What
//! each draws is captured once per frame into [`HudPaint`]; hunger motion is
//! updated per control when painted. Native cells obey
//! the control's inherited clip and viewport.

use std::collections::BTreeMap;

use serde_json::Value;
use ui::UiVisual;

use ui_engine::engine::{
    Canvas,
    screen::{CustomDraw, CustomRenderer, PaintError},
};

mod hunger_animation;
mod hunger_overlay;
pub use hunger_overlay::saturation_mask;
mod mob_effects;
mod status;
mod status_motion;
pub use hunger_animation::HungerAnimation;
pub use mob_effects::{EffectIcon, MobEffects};
pub use status::{HEART_COLUMNS, HEART_ROW_PITCH, HeartPaint, HungerPaint};

/// The pack's standalone crosshair art, used ahead of the HUD icons sheet.
pub const CROSSHAIR_TEXTURE: &str = "textures/ui/cross_hair";
/// The standalone crosshair's side in GUI px.
pub const CROSSHAIR_SIDE: f32 = 16.0;
pub const HUNGER_RENDERER: &str = "hunger_renderer";
/// The built-in Java pack's effect rows; vanilla's `mob_effects_renderer` draws the column.
const JAVA_MOB_EFFECTS_RENDERER: &str = "java_mob_effects_renderer";

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
    /// Straight RGB multiplier for cosmetic overlays; ordinary native cells are white.
    pub tint: [u8; 3],
}

impl Cell {
    pub fn icon(at: [f32; 2], texture: &'static str) -> Self {
        Self {
            at,
            size: [9.0, 9.0],
            texture,
            preferred: None,
            alpha: 255,
            tint: [255; 3],
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
    pub hunger: HungerPaint,
    pub bubbles: Vec<Cell>,
    pub mount_hearts: Vec<Cell>,
    /// The vanilla column `mob_effects_renderer` draws.
    pub effects: MobEffects,
    /// The Java rows `java_mob_effects_renderer` draws, relative to its control's top-right corner.
    pub java_effects: Vec<Cell>,
    /// Jump-bar background and fill (with its filled GUI width) over the XP bar.
    pub mount_jump: Option<(SheetSprite, SheetSprite, f32)>,
    pub hotbar_cooldowns: [f32; 9],
    pub crosshair: Option<SheetSprite>,
    pub crosshair_blend: ui::UiBlendMode,
}

impl HudPaint {
    /// Every texture path the renderers may draw this frame, pack overrides included.
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        [
            &self.armor,
            &self.bubbles,
            &self.mount_hearts,
            &self.java_effects,
        ]
        .into_iter()
        .flatten()
        .flat_map(|cell| cell.preferred.into_iter().chain([cell.texture]))
        .chain(self.effects.textures())
        .chain(self.hearts.textures())
        .chain(self.hunger.textures())
        .chain(SLOT_ART)
        .chain(self.crosshair.into_iter().flat_map(|_| {
            [
                CROSSHAIR_TEXTURE,
                assets::HudTextureRole::Crosshair.source_path(),
            ]
        }))
    }
}

/// Advance an admitted hunger control and return its render-update count.
pub type HungerUpdate<'a> = &'a dyn Fn(&str, Option<u64>) -> u64;

/// The HUD's native renderers over this frame's captured state.
pub struct HudRenderer<'a> {
    pub hud: &'a HudPaint,
    /// Per-control updates advance only when a hunger renderer reaches painting.
    pub hunger_update: Option<HungerUpdate<'a>>,
}

impl CustomRenderer for HudRenderer<'_> {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        Ok(paint(canvas, self.hud, self.hunger_update, node))
    }

    fn textures<'s>(&'s self, _drawn: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        for path in self.hud.textures() {
            texture(path);
        }
    }
}

/// Draw `node` when it is one of the HUD's renderers; `false` otherwise.
fn paint(
    canvas: &mut Canvas<'_>,
    hud: &HudPaint,
    hunger_update: Option<HungerUpdate<'_>>,
    node: &CustomDraw<'_>,
) -> bool {
    let (key, data, dest) = (node.key, node.data, node.dest);
    let alpha = &|color: [u8; 4]| node.alpha(color);
    let top_left = [dest[0], dest[1]];
    let cells = match node.renderer {
        "heart_renderer" => {
            let visible = visible_bounds(canvas);
            if visible[0] < visible[2] && visible[1] < visible[3] {
                for cell in hud.hearts.visible_cells(top_left, canvas.px(), visible) {
                    paint_cell(canvas, &cell, top_left, visible, alpha);
                }
            }
            return true;
        }
        "armor_renderer" => (&hud.armor, top_left),
        HUNGER_RENDERER => {
            let updates = hunger_update.map_or(1, |advance| {
                advance(
                    key,
                    data.get(json_ui::CUSTOM_CONTROL_INSTANCE_KEY)
                        .and_then(Value::as_u64),
                )
            });
            let visible = visible_bounds(canvas);
            for cell in hud.hunger.cells(updates) {
                paint_cell(canvas, &cell, top_left, visible, alpha);
            }
            hunger_overlay::paint(canvas, &hud.hunger, updates, node, visible);
            return true;
        }
        "bubbles_renderer" => (&hud.bubbles, top_left),
        "horse_heart_renderer" => (&hud.mount_hearts, top_left),
        JAVA_MOB_EFFECTS_RENDERER => (&hud.java_effects, [dest[2], dest[1]]),
        "mob_effects_renderer" => {
            mob_effects::paint(canvas, &hud.effects, dest, alpha);
            return true;
        }
        "hotbar_renderer" => {
            slot_art(canvas, data, dest, alpha);
            return true;
        }
        "horse_jump_renderer" => {
            if let Some((background, fill, filled)) = hud.mount_jump {
                let px = canvas.px();
                sheet(canvas, background, dest, alpha);
                let width = (filled * px).min(dest[2] - dest[0]);
                if width > 0.0 {
                    let mut uv = fill.uv;
                    let span = f32::from(uv[2] - uv[0]);
                    uv[2] = uv[0] + (span * filled / 182.0).round() as u16;
                    let clipped = SheetSprite { uv, ..fill };
                    sheet(
                        canvas,
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
            let px = canvas.px();
            for notch in 1..notches {
                let x = dest[0] + width * notch as f32 / notches as f32;
                let _ = canvas.solid([x, dest[1], x + px, dest[3]], alpha([0, 0, 0, 255]));
            }
            return true;
        }
        // A cosmetic cursor renderer supplied ahead of this one draws instead.
        "cursor_renderer" => {
            if let Some(sprite) = hud.crosshair {
                crosshair(canvas, sprite, dest, hud.crosshair_blend);
            }
            return true;
        }
        "hotbar_cooldown_renderer" => {
            let index = data
                .get("#collection_index")
                .and_then(Value::as_f64)
                .filter(|index| (0.0..9.0).contains(index) && index.fract() == 0.0)
                .map(|index| index as usize);
            if let Some(progress) = index.and_then(|index| hud.hotbar_cooldowns.get(index))
                && progress.is_finite()
                && *progress > 0.0
            {
                let px = canvas.px();
                let side = 16.0;
                let left = (dest[0] + dest[2] - side * px) * 0.5;
                let top = (dest[1] + dest[3] - side * px) * 0.5;
                let progress = progress.min(1.0);
                let start = (side * (1.0 - progress)).floor();
                let height = (side * progress).ceil();
                let _ = canvas.solid(
                    [
                        left,
                        top + start * px,
                        left + side * px,
                        top + (start + height) * px,
                    ],
                    alpha([255, 255, 255, 127]),
                );
            }
            return true;
        }
        // Renderers with no Cinnabar state draw nothing, as with no data.
        "dash_renderer"
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
    let visible = visible_bounds(canvas);
    if visible[0] < visible[2] && visible[1] < visible[3] {
        for cell in cells {
            paint_cell(canvas, cell, origin, visible, alpha);
        }
    }
    true
}

/// Intersects the inherited clip with the physical viewport.
fn visible_bounds(canvas: &Canvas<'_>) -> [f32; 4] {
    let screen = canvas.screen();
    let clip = canvas.clip().unwrap_or(screen);
    [
        clip[0].max(screen[0]),
        clip[1].max(screen[1]),
        clip[2].min(screen[2]),
        clip[3].min(screen[3]),
    ]
}

/// Emits one cell only when its animated bounds intersect the visible region.
fn paint_cell(
    canvas: &mut Canvas<'_>,
    cell: &Cell,
    origin: [f32; 2],
    visible: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let px = canvas.px();
    let x = origin[0] + cell.at[0] * px;
    let y = origin[1] + cell.at[1] * px;
    let bounds = [x, y, x + cell.size[0] * px, y + cell.size[1] * px];
    paint_sprite(
        canvas,
        cell.preferred,
        cell.texture,
        bounds,
        [cell.tint[0], cell.tint[1], cell.tint[2], cell.alpha],
        visible,
        alpha,
    );
}

/// Draws `texture` (or `preferred`, when the pack holds it) at logical `bounds` with the
/// straight `color`, when they intersect the visible region.
fn paint_sprite(
    canvas: &mut Canvas<'_>,
    preferred: Option<&str>,
    texture: &str,
    bounds: [f32; 4],
    color: [u8; 4],
    visible: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    if bounds[2] <= visible[0]
        || bounds[3] <= visible[1]
        || bounds[0] >= visible[2]
        || bounds[1] >= visible[3]
    {
        return;
    }
    let color = alpha(color);
    let visual = preferred
        .and_then(|path| canvas.sprite(path, json_ui::UvRect::full(), color, Default::default()))
        .or_else(|| canvas.sprite(texture, json_ui::UvRect::full(), color, Default::default()));
    if let Some(visual) = visual {
        let _ = canvas.push(visual, bounds);
    }
}

/// The hotbar slot background for the cell the control's collection index names.
fn slot_art(
    canvas: &mut Canvas<'_>,
    data: &BTreeMap<String, Value>,
    dest: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let index = data
        .get("#collection_index")
        .and_then(Value::as_f64)
        .map_or(0, |index| index.clamp(0.0, 8.0) as usize);
    if let Some(visual) = canvas.sprite(
        SLOT_ART[index],
        json_ui::UvRect::full(),
        alpha([255; 4]),
        Default::default(),
    ) {
        let _ = canvas.push(visual, dest);
    }
}

fn sheet(
    canvas: &mut Canvas<'_>,
    sprite: SheetSprite,
    bounds: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let visual = UiVisual::Sprite {
        texture_page: sprite.page,
        uv: sprite.uv,
        color: alpha([255; 4]),
    };
    let _ = canvas.push(visual, bounds);
}

/// Centers the pack crosshair or built-in art with the selected color blending.
fn crosshair(canvas: &mut Canvas<'_>, sprite: SheetSprite, dest: [f32; 4], blend: ui::UiBlendMode) {
    let role = assets::HudTextureRole::Crosshair;
    let standalone =
        canvas
            .textures()
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
            let (page, [x, y, width, height]) = canvas.textures().sprite(role.source_path())?;
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
    let side = gui_side * canvas.px();
    let x = (dest[0] + dest[2] - side) * 0.5;
    let y = (dest[1] + dest[3] - side) * 0.5;
    let visual = match blend {
        ui::UiBlendMode::Invert => UiVisual::InvertedSprite {
            texture_page: sprite.page,
            uv: sprite.uv,
        },
        ui::UiBlendMode::Alpha | ui::UiBlendMode::Backdrop | ui::UiBlendMode::Filter => {
            UiVisual::Sprite {
                texture_page: sprite.page,
                uv: sprite.uv,
                color: [255; 4],
            }
        }
    };
    let bounds = canvas.snap_sprite_bounds([x, y, x + side, y + side]);
    let _ = canvas.push(visual, bounds);
}
