use super::super::super::UiPresentationError;
use super::{
    modal,
    paint::{Bounds, Canvas},
    theme::{self, CAPTION, HEADER5, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use crate::menu::{MenuAction, MenuView};

/// A server draft keeps its fields scrollable while the actions remain visible.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    canvas.overlay(size, theme::OVERLAY_MODAL)?;
    let entrance = canvas.begin_entrance(super::motion::Surface::Dialog(modal::dialog_id(
        "server draft",
    )));
    let margin = canvas.r(1.6).min(size[0] * 0.05).min(size[1] * 0.05);
    let width = canvas.r(56.0).min(size[0] - margin * 2.0);
    let pad = canvas.r(2.4).min(width * 0.06);
    let inner = width - pad * 2.0;
    let stacked = inner < canvas.r(36.0);
    let button_height = canvas.r(4.4);
    let gap = canvas.r(1.2);
    let footer_height = if stacked {
        button_height * 2.0 + gap
    } else {
        button_height
    };
    let caption = "Give this server a name and address.";
    let heading_width = inner - canvas.r(4.8);
    let caption_height = canvas.measure_height(caption, heading_width, CAPTION)?;
    let title_height = canvas.r(HEADER5.line);
    let header_height = pad + title_height + canvas.r(0.8) + caption_height + canvas.r(2.0);
    let field_height = canvas.r(CAPTION.line + 0.8 + 4.8);
    let hint = "Use a hostname or IP address.";
    let hint_height = canvas.measure_height(hint, inner, CAPTION)?;
    let mut content_height = field_height * if stacked { 3.0 } else { 2.0 }
        + canvas.r(if stacked { 3.2 } else { 1.6 })
        + canvas.r(0.8)
        + hint_height;
    let error_height = view
        .message
        .as_deref()
        .map(|message| canvas.measure_height(message, inner - canvas.r(2.4), CAPTION))
        .transpose()?
        .unwrap_or(0.0);
    if error_height > 0.0 {
        content_height += canvas.r(2.0) + error_height + canvas.r(2.4);
    }
    let height = (header_height + content_height + canvas.r(2.4) + footer_height + pad)
        .min(size[1] - margin * 2.0);
    let x = (size[0] - width) * 0.5;
    let y = (size[1] - height) * 0.5;
    let bounds = [x, y, x + width, y + height];
    widgets::panel(canvas, bounds)?;
    let span = [x + pad, x + width - pad];
    canvas.text_line(
        if view.editing.is_some() {
            "EDIT SERVER"
        } else {
            "ADD SERVER"
        },
        [span[0], y + pad],
        heading_width,
        HEADER5,
        TEXT,
    )?;
    canvas.text(
        caption,
        [span[0], y + pad + title_height + canvas.r(0.8)],
        heading_width,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    let close_size = canvas.r(4.0);
    let close_inset = canvas.r(1.2);
    modal::close_button(
        canvas,
        view,
        [
            bounds[2] - close_inset - close_size,
            y + close_inset,
            bounds[2] - close_inset,
            y + close_inset + close_size,
        ],
        MenuAction::AddBack,
    )?;
    let footer_top = bounds[3] - pad - footer_height;
    let viewport = [
        span[0],
        y + header_height,
        span[1],
        footer_top - canvas.r(2.4),
    ];
    let max = (content_height - (viewport[3] - viewport[1])).max(0.0);
    if let Some(offset) = canvas.offsets.get_mut("add_server_fields") {
        *offset = offset.clamp(0.0, max);
    }
    let field_right = span[1] - if max > 0.0 { canvas.r(1.6) } else { 0.0 };
    let scroll = canvas.begin_scroll("add_server_fields", viewport)?;
    let top = viewport[1] - scroll.offset;
    field(
        canvas,
        view,
        [span[0], top, field_right, top + field_height],
        "Server name",
        &view.name,
        "My server",
        MenuAction::AddName,
    )?;
    let address_top = top + field_height + canvas.r(1.6);
    let port_width = canvas.r(10.8);
    let address_right = if stacked {
        field_right
    } else {
        field_right - port_width - gap
    };
    field(
        canvas,
        view,
        [
            span[0],
            address_top,
            address_right,
            address_top + field_height,
        ],
        "Server address",
        &view.address,
        "Hostname or IP address",
        MenuAction::AddAddress,
    )?;
    let port_top = if stacked {
        address_top + field_height + canvas.r(1.6)
    } else {
        address_top
    };
    field(
        canvas,
        view,
        [
            if stacked {
                span[0]
            } else {
                field_right - port_width
            },
            port_top,
            field_right,
            port_top + field_height,
        ],
        "Port",
        &view.port,
        "Default",
        MenuAction::AddPort,
    )?;
    let hint_top = port_top + field_height + canvas.r(0.8);
    canvas.text(
        hint,
        [span[0], hint_top],
        inner,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    if let Some(message) = view.message.as_deref() {
        let error_top = hint_top + hint_height + canvas.r(2.0);
        let alert = [
            span[0],
            error_top,
            span[1],
            error_top + error_height + canvas.r(2.4),
        ];
        canvas.fill(alert, theme::NEUTRAL100)?;
        canvas.fill(
            [alert[0], alert[1], alert[0] + canvas.r(0.4), alert[3]],
            theme::DESTRUCTIVE_TINT,
        )?;
        canvas.text(
            message,
            [alert[0] + gap, alert[1] + gap],
            inner - gap * 2.0,
            CAPTION,
            theme::DESTRUCTIVE_TINT,
            false,
        )?;
    }
    canvas.end_scroll(scroll, content_height)?;
    let ready = !view.name.trim().is_empty() && !view.address.trim().is_empty();
    if stacked {
        widgets::button(
            canvas,
            view,
            [span[0], footer_top, span[1], footer_top + button_height],
            Variant::Secondary,
            "Save",
            ready.then_some(MenuAction::AddSave),
        )?;
        widgets::button(
            canvas,
            view,
            [
                span[0],
                footer_top + button_height + gap,
                span[1],
                bounds[3] - pad,
            ],
            Variant::Primary,
            "Save & join",
            ready.then_some(MenuAction::AddSaveConnect),
        )?;
    } else {
        let cell = (inner - gap) * 0.5;
        for (index, (label, variant, action)) in [
            (
                "Save",
                Variant::Secondary,
                ready.then_some(MenuAction::AddSave),
            ),
            (
                "Save & join",
                Variant::Primary,
                ready.then_some(MenuAction::AddSaveConnect),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let left = span[0] + index as f32 * (cell + gap);
            widgets::button(
                canvas,
                view,
                [left, footer_top, left + cell, footer_top + button_height],
                variant,
                label,
                action,
            )?;
        }
    }
    canvas.end_entrance(entrance, size)
}

fn field(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    label: &str,
    value: &str,
    placeholder: &str,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    canvas.text_line(
        label,
        [bounds[0], bounds[1]],
        bounds[2] - bounds[0],
        CAPTION,
        TEXT_DIMMER,
    )?;
    widgets::text_field(
        canvas,
        view,
        [
            bounds[0],
            bounds[1] + canvas.r(CAPTION.line + 0.8),
            bounds[2],
            bounds[3],
        ],
        value,
        placeholder,
        view.field == action.text_field(),
        Some(action),
    )
}
