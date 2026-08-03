#![allow(clippy::too_many_arguments)]

use ui::{SafeArea, TextLayoutCache, UiNode, UiNodeId, UiRect, UiVisual};

use crate::menu::{MenuAction, MenuField, MenuView};
use crate::ui_runtime::presentation::IconRef;

use super::{
    ACCENT, BUTTON, BUTTON_FOCUSED, BUTTON_HOVERED, BUTTON_PRESSED, MUTED, PANEL_ALT, TEXT,
    TextMetrics, UiPresentationError, bounded_visible_text, rect,
};

pub(super) fn card(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    action: MenuAction,
    _focus_index: usize,
    server: &crate::menu::MenuServerCard,
    icon: Option<IconRef>,
    position: [f32; 2],
    width: f32,
    height: f32,
) -> Result<(), UiPresentationError> {
    let bounds = rect(
        position[0],
        position[1],
        position[0] + width,
        position[1] + height,
    )?;
    solid(
        nodes,
        next_id,
        solid_page,
        bounds,
        card_color(view, action, PANEL_ALT),
    );
    hits.push((action, bounds));
    if let Some(icon) = icon {
        let artwork = server.icon.is_some();
        let sprite_bounds = if artwork {
            rect(
                position[0] + 10.0,
                position[1] + 10.0,
                position[0] + 64.0,
                position[1] + 64.0,
            )?
        } else {
            rect(
                position[0] + 12.0,
                position[1] + 12.0,
                position[0] + 60.0,
                position[1] + 60.0,
            )?
        };
        nodes.push(
            UiNode::new(UiNodeId::new(*next_id), None, sprite_bounds).with_visual(
                UiVisual::Sprite {
                    texture_page: icon.page,
                    uv: icon.uv,
                    color: [255; 4],
                },
            ),
        );
        *next_id = next_id.saturating_add(1);
    }
    let content_left = if icon.is_some() { 76.0 } else { 16.0 };
    let content_width = (width - content_left - 16.0).max(1.0);
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        bounded_visible_text(&server.name),
        [position[0] + content_left, position[1] + 12.0],
        content_width,
        TEXT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        bounded_visible_text(if server.caption.is_empty() {
            "Featured server"
        } else {
            &server.caption
        }),
        [position[0] + content_left, position[1] + 36.0],
        content_width,
        MUTED,
    )?;
    if view.hovered == Some(action) || view.pressed == Some(action) {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            bounded_visible_text(&server.address),
            [
                position[0] + content_left,
                position[1] + (height - 24.0).max(52.0),
            ],
            content_width,
            ACCENT,
        )?;
    }
    Ok(())
}

pub(super) fn field(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    action: MenuAction,
    _focus_index: usize,
    field: MenuField,
    label: &str,
    value: &str,
    position: [f32; 2],
    width: f32,
) -> Result<(), UiPresentationError> {
    text(
        nodes, next_id, layouts, font, metrics, solid_page, label, position, width, MUTED,
    )?;
    let bounds = rect(
        position[0],
        position[1] + 22.0,
        position[0] + width,
        position[1] + 70.0,
    )?;
    solid(
        nodes,
        next_id,
        solid_page,
        bounds,
        if view.field == Some(field) || view.focused_action == Some(action) {
            BUTTON_FOCUSED
        } else {
            PANEL_ALT
        },
    );
    hits.push((action, bounds));
    let mut visible = value.to_owned();
    if view.field == Some(field) {
        visible.push('|');
    }
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        bounded_visible_text(&visible),
        [position[0] + 16.0, position[1] + 38.0],
        width - 32.0,
        TEXT,
    )
}

pub(super) fn button(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    action: MenuAction,
    _focus_index: usize,
    label: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    _safe_area: SafeArea,
) -> Result<(), UiPresentationError> {
    let bounds = rect(x, y, (x + width).max(x + 1.0), (y + height).max(y + 1.0))?;
    solid(
        nodes,
        next_id,
        solid_page,
        bounds,
        button_color(view, action, BUTTON),
    );
    hits.push((action, bounds));
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        label,
        [x + 18.0, y + (height - 18.0) * 0.5],
        (width - 36.0).max(1.0),
        TEXT,
    )
}

pub(super) fn button_color(view: &MenuView, action: MenuAction, idle: [u8; 4]) -> [u8; 4] {
    if view.pressed == Some(action) {
        BUTTON_PRESSED
    } else if view.hovered == Some(action) {
        BUTTON_HOVERED
    } else if view.focused_action == Some(action) {
        BUTTON_FOCUSED
    } else {
        idle
    }
}

fn card_color(view: &MenuView, action: MenuAction, idle: [u8; 4]) -> [u8; 4] {
    button_color(view, action, idle)
}

pub(super) fn solid(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    texture_page: u16,
    bounds: UiRect,
    color: [u8; 4],
) {
    nodes.push(
        UiNode::new(UiNodeId::new(*next_id), None, bounds).with_visual(UiVisual::Solid {
            texture_page,
            color,
        }),
    );
    *next_id = next_id.saturating_add(1);
}

pub(super) fn text(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    texture_page: u16,
    value: &str,
    position: [f32; 2],
    width: f32,
    color: [u8; 4],
) -> Result<(), UiPresentationError> {
    let layout = layouts
        .layout(metrics.request(value, (width.max(1.0) * 64.0) as u32, font))
        .map_err(UiPresentationError::Text)?;
    let height = (layout.size_64()[1] as f32 / 64.0).max(18.0);
    nodes.push(
        UiNode::new(
            UiNodeId::new(*next_id),
            None,
            rect(
                position[0],
                position[1],
                position[0] + width.max(1.0),
                position[1] + height,
            )?,
        )
        .with_visual(UiVisual::Text {
            layout,
            color,
            shadow: metrics.shadow(),
        }),
    );
    *next_id = next_id.saturating_add(1);
    let _ = texture_page;
    Ok(())
}
