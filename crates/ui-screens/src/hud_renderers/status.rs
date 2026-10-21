//! The heart and hunger rows' captured state, expanded into cells only when painted.

use super::Cell;
use super::status_motion::{heart_lift, hunger_shake_offset, hunger_shakes};

/// Hearts per row.
pub const HEART_COLUMNS: u32 = 10;
/// GUI px between stacked heart rows.
pub const HEART_ROW_PITCH: f32 = 10.0;

/// Compact heart state; cells are generated only for rows a renderer can see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeartPaint {
    /// Health in half hearts.
    pub current: u32,
    /// Health before the flashing damage, in half hearts.
    pub previous: u32,
    /// Absorption in half hearts.
    pub absorption: u32,
    /// Heart containers health fills.
    pub health_hearts: u32,
    /// Containers drawn for health and absorption.
    pub total: u32,
    pub tick: u64,
    pub regenerating: bool,
    /// Background, full, half, absorption full and half, flash full and half.
    pub sprites: [Option<Cell>; 7],
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn iter(&self) -> impl Iterator<Item = Cell> + '_ {
        self.cells(0..self.total)
    }

    /// Counts drawn containers and filled hearts without materializing them.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(
        clippy::len_without_is_empty,
        reason = "counts drawn cells for assertions; a heart row is never empty"
    )]
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

/// Hunger artwork is shared across controls; each renderer supplies its own update count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HungerPaint {
    /// Food in half shanks.
    pub current: u32,
    pub saturation_empty: bool,
    pub saturation_overlay: Option<u32>,
    pub food_preview: Option<u32>,
    pub food_saturation_preview: Option<u32>,
    pub overlay_color: [u8; 4],
    pub inverse_overlay: bool,
    /// Background, full and half art; none hides the row.
    pub sprites: Option<[&'static str; 3]>,
}

impl HungerPaint {
    /// Applies known food values to the exact native hunger renderer, including pack relocation.
    pub fn set_overlay(
        &mut self,
        style: &ui::mod_hud::HungerOverlay,
        saturation: Option<f32>,
        food: Option<&protocol::FoodProperties>,
    ) {
        if self.sprites.is_none() {
            return;
        }
        let color = style.color.unwrap_or([1.; 4]);
        self.inverse_overlay = style.inverse_color;
        self.overlay_color = color.map(|component| (component.clamp(0., 1.) * 255.).round() as u8);
        self.saturation_overlay = style
            .saturation
            .then_some(saturation)
            .flatten()
            .filter(|value| value.is_finite() && *value >= 0.)
            .map(|value| value.ceil().min(20.) as u32);
        self.food_preview = style
            .potential_hunger
            .unwrap_or(style.held_food)
            .then_some(food)
            .flatten()
            .map(|food| self.current.saturating_add(food.nutrition()).min(20));
        self.food_saturation_preview = style
            .potential_saturation
            .unwrap_or(style.held_food)
            .then_some(food)
            .flatten()
            .zip(saturation)
            .filter(|(_, value)| value.is_finite() && *value >= 0.)
            .map(|(food, current)| {
                (current + food.nutrition() as f32 * food.saturation_modifier() * 2.)
                    .min(self.current.saturating_add(food.nutrition()).min(20) as f32)
                    .ceil() as u32
            });
    }
    /// Texture candidates do not depend on a control's animation phase.
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        self.sprites.iter().flatten().copied()
    }

    /// Generate both layers at the same icon offset without allocating a row.
    pub fn cells(&self, updates: u64) -> impl Iterator<Item = Cell> + '_ {
        let shaking = hunger_shakes(self.saturation_empty, self.current, updates);
        (0..10u32).flat_map(move |index| {
            let Some([background, full, half]) = self.sprites else {
                return [None, None, None].into_iter().flatten();
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
                self.food_preview
                    .filter(|preview| *preview > self.current && *preview > index * 2)
                    .map(|preview| {
                        let mut cell =
                            Cell::icon(at, if preview - index * 2 == 1 { half } else { full });
                        cell.alpha = 110;
                        cell
                    }),
            ]
            .into_iter()
            .flatten()
        })
    }

    /// Whether survival or mount visibility removed this row.
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_empty(&self) -> bool {
        self.sprites.is_none()
    }
}
