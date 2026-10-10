//! What the HUD's native renderers draw each frame: health, armor, hunger,
//! mount-health and air rows, status effects, the mount jump bar, and the
//! crosshair, laid out relative to each JSON-UI renderer's control. Heart
//! rows and absorption sprites follow the Bedrock HUD's display rules.

use crate::ui_runtime::gameplay_hud::HeartVariant;
use assets::HudTextureRole;

use super::{
    HudFrame, HudTexturePages, UiRuntime,
    pinned::{MAX_HEART_ROWS, MAX_MOUNT_HEARTS, effect_blink_alpha, effect_icon_role, heart_role},
    status_motion::{heart_lift, hunger_shake_offset, hunger_shakes},
};
use crate::ui_runtime::presentation::forms::hud_renderers::{
    Cell, EffectIcon, HudPaint, MobEffects, SheetSprite,
};

/// The `textures/ui` path a HUD role was compiled from.
fn path(role: HudTextureRole) -> &'static str {
    let source = role.source_path();
    source.strip_suffix(".png").unwrap_or(source)
}

/// The hardcore variant of a heart foreground, which lives in `textures/ui/hardcore/`.
fn hardcore_path(role: HudTextureRole) -> Option<&'static str> {
    Some(match role {
        HudTextureRole::HeartFull => "textures/ui/hardcore/heart",
        HudTextureRole::HeartHalf => "textures/ui/hardcore/heart_half",
        HudTextureRole::HeartFlashFull => "textures/ui/hardcore/heart_flash",
        HudTextureRole::HeartFlashHalf => "textures/ui/hardcore/heart_flash_half",
        HudTextureRole::PoisonHeartFull => "textures/ui/hardcore/poison_heart",
        HudTextureRole::PoisonHeartHalf => "textures/ui/hardcore/poison_heart_half",
        HudTextureRole::PoisonHeartFlashFull => "textures/ui/hardcore/poison_heart_flash",
        HudTextureRole::PoisonHeartFlashHalf => "textures/ui/hardcore/poison_heart_flash_half",
        HudTextureRole::WitherHeartFull => "textures/ui/hardcore/wither_heart",
        HudTextureRole::WitherHeartHalf => "textures/ui/hardcore/wither_heart_half",
        HudTextureRole::WitherHeartFlashFull => "textures/ui/hardcore/wither_heart_flash",
        HudTextureRole::WitherHeartFlashHalf => "textures/ui/hardcore/wither_heart_flash_half",
        HudTextureRole::AbsorptionHeartFull => "textures/ui/hardcore/absorption_heart",
        HudTextureRole::AbsorptionHeartHalf => "textures/ui/hardcore/absorption_heart_half",
        HudTextureRole::FreezeHeartFull => "textures/ui/hardcore/freeze_heart",
        HudTextureRole::FreezeHeartHalf => "textures/ui/hardcore/freeze_heart_half",
        HudTextureRole::FreezeHeartFlashFull => "textures/ui/hardcore/freeze_heart_flash",
        HudTextureRole::FreezeHeartFlashHalf => "textures/ui/hardcore/freeze_heart_flash_half",
        _ => return None,
    })
}

const HEART_COLUMNS: u32 = 10;
const HEART_ROW_PITCH: f32 = 10.0;

/// Health plus absorption in hearts, and the row pitch the stacked rows use.
struct HeartRows {
    current: u32,
    maximum: u32,
    absorption: u32,
    health_hearts: u32,
    total: u32,
    rows: u32,
    pitch: f32,
}

/// Quantizes health and absorption into the native renderer's heart rows.
fn heart_rows(runtime: &UiRuntime) -> Option<HeartRows> {
    let health = runtime.hud().health()?;
    let scale = u32::from(health.scale()).max(1);
    // Half-heart units on the reference 20-point scale.
    let current = u32::from(health.current()).div_ceil(scale);
    let maximum = u32::from(health.maximum()).div_ceil(scale);
    let absorption = runtime
        .hud()
        .absorption()
        .map(|stat| u32::from(stat.current()).div_ceil(u32::from(stat.scale()).max(1)))
        .unwrap_or(0);
    let health_hearts = maximum
        .div_ceil(2)
        .min(u32::from(MAX_HEART_ROWS) * HEART_COLUMNS);
    let total = (health_hearts + absorption.div_ceil(2)).max(1);
    let rows = total.div_ceil(HEART_COLUMNS).max(1);
    let pitch = HEART_ROW_PITCH;
    Some(HeartRows {
        current,
        maximum,
        absorption,
        health_hearts,
        total,
        rows,
        pitch,
    })
}

/// Capture the frame's native HUD art.
pub(in super::super) fn capture(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    sheet: Option<&HudTexturePages>,
    options: &crate::menu::settings_options::SettingsOptions,
) -> HudPaint {
    use crate::menu::settings_options::{INVERT_CROSSHAIR_OPTION, THIRD_PERSON_CROSSHAIR_OPTION};
    let now_tick = runtime.estimated_server_tick(frame.now_millis);
    let mode_allows_hotbar = player_runtime
        .facts
        .player_game_mode()
        .is_none_or(|mode| mode.shows_hotbar());
    let mut paint = HudPaint {
        effects: effects(runtime, now_tick),
        hotbar_cooldowns: frame.hotbar_cooldowns,
        // The third-person preference never overrides the spectator gate.
        crosshair: sheet
            .filter(|_| {
                (frame.first_person || options.value(THIRD_PERSON_CROSSHAIR_OPTION.name) != 0)
                    && mode_allows_hotbar
            })
            .map(|sheet| sheet_sprite(sheet, HudTextureRole::Crosshair)),
        crosshair_blend: if options.value(INVERT_CROSSHAIR_OPTION.name) != 0 {
            ui::UiBlendMode::Invert
        } else {
            ui::UiBlendMode::Alpha
        },
        ..HudPaint::default()
    };
    if !player_runtime.facts.survival_stats_visible() {
        return paint;
    }
    if let Some(rows) = heart_rows(runtime) {
        paint.hearts = hearts(runtime, frame, &rows, now_tick);
        paint.armor = armor(runtime, &rows);
        // The effect column's top band counts health and absorption at full maximum.
        paint.effects.status_rows = (rows.maximum + rows.absorption)
            .div_ceil(2)
            .div_ceil(HEART_COLUMNS)
            + u32::from(!paint.armor.is_empty());
    }
    match frame.mount_health {
        Some(health) => paint.mount_hearts = mount_hearts(health),
        None => paint.hunger = hunger(runtime, now_tick),
    }
    paint.bubbles = bubbles(runtime);
    if let (Some(charge), Some(sheet)) = (frame.mount_jump, sheet) {
        let filled = (charge.clamp(0.0, 1.0) * 183.0).floor().clamp(0.0, 182.0);
        paint.mount_jump = Some((
            sheet_sprite(sheet, HudTextureRole::MountJumpBackground),
            sheet_sprite(sheet, HudTextureRole::MountJumpProgress),
            filled,
        ));
    }
    paint
}

fn sheet_sprite(sheet: &HudTexturePages, role: HudTextureRole) -> SheetSprite {
    SheetSprite {
        page: sheet.page,
        uv: sheet.sprite(role).uv,
    }
}

/// Compact heart state; cells are generated only for rows a renderer can see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeartPaint {
    current: u32,
    previous: u32,
    absorption: u32,
    health_hearts: u32,
    total: u32,
    tick: u64,
    regenerating: bool,
    sprites: [Option<Cell>; 7],
}

impl HeartPaint {
    /// Texture candidates are independent of the number of heart rows.
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        self.sprites
            .iter()
            .flatten()
            .flat_map(|cell| cell.preferred.into_iter().chain([cell.texture]))
    }

    /// Enumerates cells lazily, preserving background-before-foreground order.
    #[cfg(test)]
    pub fn iter(&self) -> impl Iterator<Item = Cell> + '_ {
        self.cells(0..self.total)
    }

    /// Counts drawn containers and filled hearts without materializing them.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        (self.total
            + self.current.div_ceil(2).min(self.health_hearts)
            + self.previous.div_ceil(2).min(self.health_hearts)
            + self.absorption.div_ceil(2)) as usize
    }

    /// Visits only rows intersecting the viewport, including animated lift.
    pub fn visible_cells(
        &self,
        origin: [f32; 2],
        px: f32,
        bounds: [f32; 4],
    ) -> impl Iterator<Item = Cell> + '_ {
        let indices = if px.is_finite() && px > 0.0 {
            let pitch = HEART_ROW_PITCH * px;
            // Include the preceding row for shake and regeneration lift.
            let first =
                (((origin[1] - bounds[3]) / pitch).floor().max(0.0) as u32).saturating_sub(1);
            let height = self.sprites[0].as_ref().map_or(0.0, |cell| cell.size[1]);
            let end = ((origin[1] + height * px - bounds[1]) / pitch)
                .ceil()
                .max(0.0) as u32;
            first.saturating_mul(HEART_COLUMNS)..end.saturating_mul(HEART_COLUMNS).min(self.total)
        } else {
            0..0
        };
        self.cells(indices)
    }

    /// Produces background, previous-health flash and current fill for each selected container.
    fn cells(&self, indices: std::ops::Range<u32>) -> impl Iterator<Item = Cell> + '_ {
        indices.flat_map(|index| {
            let lift = heart_lift(
                index,
                self.health_hearts,
                self.current + self.absorption,
                self.regenerating,
                self.tick,
            );
            let at = [
                (index % HEART_COLUMNS) as f32 * 8.0,
                -((index / HEART_COLUMNS) as f32) * HEART_ROW_PITCH - lift,
            ];
            let (points, full, half) = if index < self.health_hearts {
                (self.current.saturating_sub(index * 2), 1, 2)
            } else {
                (
                    self.absorption
                        .saturating_sub((index - self.health_hearts) * 2),
                    3,
                    4,
                )
            };
            let foreground = match points {
                0 => None,
                1 => self.sprites[half].as_ref(),
                _ => self.sprites[full].as_ref(),
            };
            let previous = if index < self.health_hearts {
                match self.previous.saturating_sub(index * 2) {
                    0 => None,
                    1 => self.sprites[6].as_ref(),
                    _ => self.sprites[5].as_ref(),
                }
            } else {
                None
            };
            [self.sprites[0].as_ref(), previous, foreground]
                .into_iter()
                .flatten()
                .map(move |sprite| {
                    let mut cell = sprite.clone();
                    cell.at = at;
                    cell
                })
        })
    }
}

/// Retains heart totals and sprite variants without allocating offscreen cells.
fn hearts(
    runtime: &UiRuntime,
    frame: &HudFrame,
    rows: &HeartRows,
    now_tick: Option<u64>,
) -> HeartPaint {
    let variant = runtime.gameplay_hud().heart_variant(now_tick);
    let damage = runtime
        .local_actor_damage
        .filter(|damage| damage.flash_active());
    let hardcore = runtime.gameplay_hud().hardcore();
    let sprite = |role: HudTextureRole| {
        let mut cell = Cell::icon([0.0; 2], path(role));
        cell.preferred = hardcore.then(|| hardcore_path(role)).flatten();
        Some(cell)
    };
    let (abs_full, abs_half) = if variant == HeartVariant::Withered {
        (
            HudTextureRole::WitherHeartFull,
            HudTextureRole::WitherHeartHalf,
        )
    } else {
        (
            HudTextureRole::AbsorptionHeartFull,
            HudTextureRole::AbsorptionHeartHalf,
        )
    };
    HeartPaint {
        current: rows.current,
        previous: damage.map_or(0, |damage| {
            (damage.previous_health.max(0.0).ceil() as u32).min(rows.maximum)
        }),
        absorption: rows.absorption,
        health_hearts: rows.health_hearts,
        total: rows.total,
        tick: now_tick.unwrap_or(frame.now_millis / 50),
        regenerating: runtime.gameplay_hud().regeneration_active(now_tick),
        sprites: [
            Some(Cell::icon(
                [0.0; 2],
                if damage.is_some() {
                    "textures/ui/heart_blink"
                } else {
                    path(HudTextureRole::HeartBackground)
                },
            )),
            heart_role(variant, None, 2).and_then(sprite),
            heart_role(variant, None, 1).and_then(sprite),
            sprite(abs_full),
            sprite(abs_half),
            damage
                .and_then(|_| heart_role(variant, Some(true), 2))
                .and_then(sprite),
            damage
                .and_then(|_| heart_role(variant, Some(true), 1))
                .and_then(sprite),
        ],
    }
}

/// Armor sits one row above the highest heart row, only while armor is worn.
fn armor(runtime: &UiRuntime, rows: &HeartRows) -> Vec<Cell> {
    let Some(armor) = runtime.hud().armor() else {
        return Vec::new();
    };
    let points = u32::from(armor.current()).div_ceil(u32::from(armor.scale()).max(1));
    if points == 0 {
        return Vec::new();
    }
    let y = -((rows.rows - 1) as f32) * rows.pitch - 10.0;
    (0..10u32)
        .map(|index| {
            let role = match points.saturating_sub(index * 2) {
                0 => HudTextureRole::ArmorEmpty,
                1 => HudTextureRole::ArmorHalf,
                _ => HudTextureRole::ArmorFull,
            };
            Cell::icon([index as f32 * 8.0, y], path(role))
        })
        .collect()
}

/// Hunger artwork is shared across controls; each renderer supplies its own update count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HungerPaint {
    current: u32,
    saturation_empty: bool,
    sprites: Option<[&'static str; 3]>,
}

impl HungerPaint {
    /// Texture candidates do not depend on a control's animation phase.
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        self.sprites.iter().flatten().copied()
    }

    /// Generate both layers at the same icon offset without allocating a row.
    pub fn cells(&self, updates: u64) -> impl Iterator<Item = Cell> + '_ {
        let shaking = hunger_shakes(self.saturation_empty, self.current, updates);
        (0..10u32).flat_map(move |index| {
            let Some([background, full, half]) = self.sprites else {
                return [None, None].into_iter().flatten();
            };
            let y = if shaking {
                hunger_shake_offset(index, updates)
            } else {
                0.0
            };
            let at = [-8.0 - index as f32 * 8.0, y];
            let foreground = match self.current.saturating_sub(index * 2) {
                0 => None,
                1 => Some(half),
                _ => Some(full),
            };
            [
                Some(Cell::icon(at, background)),
                foreground.map(|texture| Cell::icon(at, texture)),
            ]
            .into_iter()
            .flatten()
        })
    }

    /// Whether survival or mount visibility removed this row.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.sprites.is_none()
    }
}

/// Capture hunger textures and authoritative food state independently of packet timing.
fn hunger(runtime: &UiRuntime, now_tick: Option<u64>) -> HungerPaint {
    let Some(hunger) = runtime.hud().hunger() else {
        return HungerPaint::default();
    };
    let current = u32::from(hunger.current()).div_ceil(u32::from(hunger.scale()).max(1));
    let roles = if runtime.gameplay_hud().hunger_effect_active(now_tick) {
        [
            HudTextureRole::HungerEffectBackground,
            HudTextureRole::HungerEffectFull,
            HudTextureRole::HungerEffectHalf,
        ]
    } else {
        [
            HudTextureRole::HungerBackground,
            HudTextureRole::HungerFull,
            HudTextureRole::HungerHalf,
        ]
    };
    HungerPaint {
        current,
        saturation_empty: runtime.gameplay_hud().saturation_empty(),
        sprites: Some(roles.map(path)),
    }
}

/// Mount hearts replace the hunger row while riding, capped at 30 over three rows.
fn mount_hearts((current, maximum): (f32, f32)) -> Vec<Cell> {
    let hearts = (((maximum + 0.5) / 2.0) as u16).clamp(1, MAX_MOUNT_HEARTS);
    let filled = current.clamp(0.0, maximum).ceil() as u32;
    let mut cells = Vec::new();
    for index in 0..u32::from(hearts) {
        let at = [
            -8.0 - (index % HEART_COLUMNS) as f32 * 8.0,
            -((index / HEART_COLUMNS) as f32) * 10.0,
        ];
        cells.push(Cell::icon(at, path(HudTextureRole::HeartBackground)));
        let role = match filled.saturating_sub(index * 2) {
            0 => None,
            1 => Some(HudTextureRole::MountHeartHalf),
            _ => Some(HudTextureRole::MountHeartFull),
        };
        if let Some(role) = role {
            cells.push(Cell::icon(at, path(role)));
        }
    }
    cells
}

/// Air bubbles while submerged (air below its maximum), with the popping tail.
fn bubbles(runtime: &UiRuntime) -> Vec<Cell> {
    let Some(air) = runtime.hud().air() else {
        return Vec::new();
    };
    let current = u32::from(air.current());
    let maximum = u32::from(air.maximum()).max(1);
    if current >= maximum {
        return Vec::new();
    }
    let full = (current.saturating_sub(2) * 10).div_ceil(maximum);
    let popping = (current * 10).div_ceil(maximum).saturating_sub(full);
    (0..(full + popping).min(10))
        .map(|index| {
            let role = if index < full {
                HudTextureRole::BubbleFull
            } else {
                HudTextureRole::BubblePop
            };
            Cell::icon([-8.0 - index as f32 * 8.0, 0.0], path(role))
        })
        .collect()
}

/// Visible effects in ascending effect-id order, the order vanilla's column uses.
fn effects(runtime: &UiRuntime, now_tick: Option<u64>) -> MobEffects {
    let mut effects: Vec<_> = runtime
        .gameplay_hud()
        .effects()
        .iter()
        .filter(|effect| effect.visible_at_tick(now_tick))
        .filter_map(|effect| Some((effect, effect_icon_role(effect.effect_id)?)))
        .collect();
    effects.sort_by_key(|(effect, _)| effect.effect_id);
    let icons = effects
        .into_iter()
        .map(|(effect, icon)| EffectIcon {
            background: path(if effect.ambient {
                HudTextureRole::EffectBackgroundAmbient
            } else {
                HudTextureRole::EffectBackground
            }),
            icon: path(icon),
            alpha: effect_blink_alpha(effect, now_tick),
        })
        .collect();
    MobEffects {
        icons,
        status_rows: 0,
    }
}

#[cfg(test)]
mod absorption_tests;
