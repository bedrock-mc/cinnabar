//! Home geometry fits the action, character and utility groups inside the viewport.

use ui::IconRef;

pub(super) const LIVE_EVENT_HEIGHT: f32 = 5.2;
pub(super) const PANEL_PADDING: f32 = 2.4;
const COMPACT_WIDTH: f32 = 44.0;
const DESKTOP_WIDTH: f32 = 86.0;

pub(super) struct Layout {
    pub scale: f32,
    pub width: f32,
    pub height: f32,
    pub outside: f32,
    pub header_height: f32,
    pub gap: f32,
    pub footer_height: f32,
    pub logo_width: f32,
    pub logo_height: f32,
    pub compact: bool,
    pub compact_footer: bool,
}

/// The action group's height in rem, including wardrobe controls on compact layouts.
pub(super) fn action_group_height(compact: bool) -> f32 {
    if compact { 29.2 } else { 23.6 }
}

/// The panel inset in logical pixels, limited by its width.
pub(super) fn action_padding(rem: f32, width: f32, compact: bool) -> f32 {
    (rem * if compact { 2.0 } else { PANEL_PADDING }).min(width * 0.08)
}

impl Layout {
    /// Keeps the normal layout unless the logo, actions and footer cannot fit.
    pub(super) fn new(
        rem: f32,
        size: [f32; 2],
        title: Option<IconRef>,
        live_event: bool,
        update_ready: bool,
    ) -> Self {
        let margin = (rem * 3.2).min(size[0] * 0.04).min(size[1] * 0.035);
        let available_width = size[0] - margin * 2.0;
        let width = (rem * DESKTOP_WIDTH).min(available_width);
        let compact = width < rem * 68.0 || size[1] < rem * 44.0 || size[0] < size[1] * 0.85;
        let column_width = if compact {
            COMPACT_WIDTH
        } else {
            DESKTOP_WIDTH
        };
        let width = (rem * column_width).min(available_width);
        let compact_footer = width < rem * 60.0;
        let footer = if compact_footer { 10.8 } else { 4.8 };
        let version = if compact_footer { 4.0 } else { 2.8 };
        let ratio = title.map(|icon| {
            f32::from(icon.uv[2] - icon.uv[0]) / f32::from((icon.uv[3] - icon.uv[1]).max(1))
        });
        let logo_width = (rem * 56.0).min(width * 0.78);
        let header_height = if update_ready {
            super::super::updates::header_height(rem, size, logo_width)
        } else {
            0.0
        };
        let available_height = size[1] - margin * 2.0 - header_height;
        let logo_height = ratio.map_or(rem * 6.4, |ratio| logo_width / ratio);
        let outside = logo_height + rem * (3.2 + footer + version);
        let action_width = width * if compact { 1.0 } else { 0.59 };
        let event = if live_event { LIVE_EVENT_HEIGHT } else { 0.0 };
        let group_height = action_group_height(compact);
        let minimum =
            rem * (group_height + event) + action_padding(rem, action_width, compact) * 2.0;
        let scale = if minimum + outside > available_height {
            // Full-width art and padding bound the content after the width is recalculated.
            let logo = ratio.map_or(6.4, |ratio| 56.0 / ratio);
            let pad = if compact { 2.0 } else { PANEL_PADDING };
            let required = logo + 3.2 + footer + version + group_height + event + pad * 2.0;
            (available_height / (rem * required)).min(1.0)
        } else {
            1.0
        };
        let rem = rem * scale;
        let width = (rem * column_width).min(available_width);
        let logo_width = (rem * 56.0).min(width * 0.78);
        let logo_height = ratio.map_or(rem * 6.4, |ratio| logo_width / ratio);
        let outside = logo_height + rem * (3.2 + footer + version);
        let action_width = width * if compact { 1.0 } else { 0.59 };
        let minimum =
            rem * (group_height + event) + action_padding(rem, action_width, compact) * 2.0;
        Self {
            scale,
            width,
            height: (rem * 34.4).min(available_height - outside).max(minimum),
            outside,
            header_height,
            gap: rem * 1.6,
            footer_height: rem * footer,
            logo_width,
            logo_height,
            compact,
            compact_footer,
        }
    }
}
