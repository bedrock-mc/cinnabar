//! Home uses a centered action column with independent character and utility controls.

use super::super::paint::Bounds;
use ui::IconRef;

pub(super) const ROW_HEIGHT: f32 = 32.0;
pub(super) const COLUMN_WIDTH: f32 = 150.0;
pub(super) const LIVE_EVENT_HEIGHT: f32 = 26.0;
pub(super) const CHARACTER_ANCHOR: f32 = 0.81;
pub(super) const CHARACTER_WIDTH: f32 = 80.0;
pub(super) const CHARACTER_GAP: f32 = 8.0;

pub(super) struct Layout {
    pub scale: f32,
    pub logo: Bounds,
    pub actions: Bounds,
    pub footer: Bounds,
    pub version: Bounds,
    pub compact: bool,
    pub compact_footer: bool,
    pub gui: f32,
}

impl Layout {
    /// Fits full-height rows and utility controls without stretching the main column.
    pub(super) fn new(
        rem: f32,
        size: [f32; 2],
        title: Option<IconRef>,
        live_event: bool,
        update_ready: bool,
    ) -> Self {
        let gui = rem / super::super::theme::GUI_PIXELS_PER_REM;
        let header = if update_ready {
            super::super::updates::header_height(rem, size, size[0] * 0.55)
        } else {
            0.0
        };
        let character_fits = size[0] * (CHARACTER_ANCHOR - 0.5)
            >= gui * ((COLUMN_WIDTH + CHARACTER_WIDTH) * 0.5 + CHARACTER_GAP);
        let compact = size[0] < size[1] || size[1] < gui * 230.0 || !character_fits;
        let compact_footer = size[0] < super::footer::size(rem, false)[0] + gui * 16.0;
        let rows = (super::ACTIONS.len() + usize::from(compact)) as f32;
        let footer_size = super::footer::size(rem, compact_footer);
        let event = if live_event { LIVE_EVENT_HEIGHT } else { 0.0 };
        let ratio = title.map_or(5.0, |icon| {
            f32::from(icon.uv[2] - icon.uv[0]) / f32::from((icon.uv[3] - icon.uv[1]).max(1))
        });
        let logo_width = (size[0] * 0.55).min(size[1] * 0.22 * ratio);
        let logo_height = logo_width / ratio;
        let top = header + (size[1] - header) * 0.12;
        let available = (size[1] - top - logo_height).max(1.0);
        let required = gui * (rows * ROW_HEIGHT + event + 22.0) + footer_size[1];
        let scale = (available / required)
            .min((size[0] - 8.0) / (COLUMN_WIDTH * gui))
            .min(1.0);
        let gui = gui * scale;
        let footer_height = footer_size[1] * scale;
        let footer_width = (footer_size[0] * scale).min(size[0] - gui * 8.0);
        let footer_top = size[1] - gui * 14.0 - footer_height;
        let action_height = rows * ROW_HEIGHT * gui;
        let natural_top = top
            + logo_height
            + ((size[1] - top - logo_height - action_height - gui * 10.0) * 0.5).max(0.0);
        let action_top = natural_top
            .max(top + logo_height + gui * event)
            .min(footer_top - gui * 8.0 - action_height);
        Self {
            scale,
            logo: [
                (size[0] - logo_width) * 0.5,
                top,
                (size[0] + logo_width) * 0.5,
                top + logo_height,
            ],
            actions: [
                (size[0] - COLUMN_WIDTH * gui) * 0.5,
                action_top,
                (size[0] + COLUMN_WIDTH * gui) * 0.5,
                action_top + action_height,
            ],
            footer: [
                (size[0] - footer_width) * 0.5,
                footer_top,
                (size[0] + footer_width) * 0.5,
                footer_top + footer_height,
            ],
            version: [
                gui * 4.0,
                size[1] - gui * 12.0,
                size[0] - gui * 4.0,
                size[1],
            ],
            compact,
            compact_footer,
            gui,
        }
    }
}
