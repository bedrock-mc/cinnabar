//! OreUI icons as our own pixel art (one texel = 0.2rem), matched to the
//! originals' sizes; installed artwork supplies each icon's exact alpha shape.

use crate::ui_runtime::oreui_assets::ACTION_ICONS;

use super::super::super::UiPresentationError;
use super::paint::{Bounds, Canvas};
use super::theme::{EDGE, Rgba};

/// Native category sprites round their 24-texel squares down at odd GUI scales.
pub(super) fn native_side(canvas: &Canvas<'_>) -> f32 {
    24.0 * native_scale(canvas)
}

pub(super) fn native_scale(canvas: &Canvas<'_>) -> f32 {
    let dpi = canvas.metrics.dpi_scale.get();
    let gui_scale = canvas.rem * dpi / 5.0;
    (gui_scale * 0.5).floor().max(0.5) / dpi
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Icon {
    ArrowBack,
    Cross,
    Search,
    Player,
    Pencil,
    Check,
    Filter,
}

impl Icon {
    /// The install's atlas key for the local-originals mode.
    fn original(self) -> &'static str {
        match self {
            Self::ArrowBack => ACTION_ICONS[0],
            Self::Cross => ACTION_ICONS[1],
            Self::Search => ACTION_ICONS[2],
            Self::Player => ACTION_ICONS[3],
            Self::Pencil => ACTION_ICONS[4],
            Self::Check => ACTION_ICONS[5],
            Self::Filter => ACTION_ICONS[6],
        }
    }

    /// Our pixel art: one row per string, `#` for a lit texel.
    fn pixels(self) -> &'static [&'static str] {
        match self {
            Self::Filter => &[
                "......##....",
                "############",
                "......##....",
                "............",
                "..##........",
                "############",
                "..##........",
                "............",
                ".......##...",
                "############",
                ".......##...",
                "............",
            ],
            Self::ArrowBack => &[
                "....##......",
                "...##.......",
                "..##........",
                ".###########",
                "############",
                ".###########",
                "..##........",
                "...##.......",
                "....##......",
            ],
            Self::Cross => &[
                "##...##", ".##.##.", "..###..", "...#...", "..###..", ".##.##.", "##...##",
            ],
            Self::Search => &[
                "..####......",
                ".#....#.....",
                "#......#....",
                "#......#....",
                "#......#....",
                "#......#....",
                ".#....#.....",
                "..####.#....",
                ".......##...",
                "........##..",
                ".........##.",
                "..........##",
            ],
            Self::Player => &[
                "..####..", "..####..", "..####..", "..####..", "........", ".######.", "########",
                "########", "########",
            ],
            Self::Pencil => &[
                "............",
                "........###.",
                ".......#####",
                "......#####.",
                ".....#####..",
                "....#####...",
                "...#####....",
                "..#####.....",
                ".#####......",
                ".####.......",
                ".###........",
                "............",
            ],
            Self::Check => &[
                "......##", ".....##.", "#...##..", "##.##...", ".###....", "..#.....",
            ],
        }
    }

    /// Texel size of our art.
    pub(super) fn texels(self) -> [usize; 2] {
        if self == Self::Player {
            return [12, 12];
        }
        let rows = self.pixels();
        [rows.first().map_or(0, |row| row.len()), rows.len()]
    }
}

/// Draws `icon` with its top-left at `at`, one texel per 0.2rem (tinted by `color`).
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    icon: Icon,
    at: [f32; 2],
    color: Rgba,
) -> Result<(), UiPresentationError> {
    let color = canvas.appearance.ink(color);
    let texel = canvas.r(EDGE);
    let [width, height] = icon.texels();
    let bounds: Bounds = [
        at[0],
        at[1],
        at[0] + width as f32 * texel,
        at[1] + height as f32 * texel,
    ];
    if canvas.masked_sprite(icon.original(), bounds, color)? {
        return Ok(());
    }
    for (row, line) in icon.pixels().iter().enumerate() {
        // Runs of lit texels become one fill each.
        let bytes = line.as_bytes();
        let mut column = 0;
        while column < bytes.len() {
            if bytes[column] != b'#' {
                column += 1;
                continue;
            }
            let start = column;
            while column < bytes.len() && bytes[column] == b'#' {
                column += 1;
            }
            let top = at[1] + row as f32 * texel;
            canvas.fill(
                [
                    at[0] + start as f32 * texel,
                    top,
                    at[0] + column as f32 * texel,
                    top + texel,
                ],
                color,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_rows_are_rectangular() {
        for icon in [
            Icon::ArrowBack,
            Icon::Cross,
            Icon::Search,
            Icon::Player,
            Icon::Check,
            Icon::Filter,
        ] {
            let width = icon.pixels()[0].len();
            assert!(
                icon.pixels().iter().all(|row| row.len() == width),
                "{icon:?}"
            );
        }
    }
}
