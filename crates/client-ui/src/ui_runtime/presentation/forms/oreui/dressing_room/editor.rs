use super::super::super::super::UiPresentationError;
use super::super::{
    paint::Canvas,
    theme::{self, BODY, CAPTION, HEADER5, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use super::command;
use launcher::dressing_room::{Action, SkinEditorMode, SkinEditorTarget};
use launcher::menu::{MenuAction, MenuField, MenuView};

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    let Some(editor) = view.dressing_room.editor.as_ref() else {
        return Ok(());
    };
    canvas.overlay(size, theme::OVERLAY_MODAL)?;
    let width = canvas.r(48.0).min(size[0] - canvas.r(3.2));
    let pad = canvas.r(2.4);
    let content_width = width - pad * 2.0;
    let cape = editor.target == SkinEditorTarget::Cape;
    let rename = editor.mode == SkinEditorMode::Rename;
    let description = if cape {
        "This removes the imported cape from your collection. You can import it again later."
    } else {
        "This removes the imported skin from your collection. You can import it again later."
    };
    let body_bottom = if rename {
        canvas.r(13.2)
    } else {
        canvas.r(9.0) + canvas.measure_height(description, content_width, CAPTION)?
    };
    let error_top = body_bottom + canvas.r(1.6);
    let error_height = view
        .dressing_room
        .message
        .as_deref()
        .map(|message| canvas.measure_height(message, content_width, CAPTION))
        .transpose()?
        .unwrap_or(0.0);
    let buttons_top = if error_height > 0.0 {
        error_top + error_height + canvas.r(2.0)
    } else {
        canvas.r(16.8).max(body_bottom + canvas.r(2.0))
    };
    let height = buttons_top + canvas.r(4.4) + pad;
    let x = (size[0] - width) * 0.5;
    let y = ((size[1] - height) * 0.5).max(canvas.r(1.6));
    let b = [x, y, x + width, y + height];
    let span = [x + pad, x + width - pad];
    let title = match (cape, rename) {
        (false, true) => "RENAME SKIN",
        (true, true) => "RENAME CAPE",
        (false, false) => "REMOVE SKIN?",
        (true, false) => "REMOVE CAPE?",
    };
    let entrance = canvas.begin_entrance(super::super::motion::Surface::Dialog(
        super::super::modal::dialog_id(title),
    ));
    widgets::panel(canvas, b)?;
    canvas.text_line(title, [span[0], y + pad], content_width, HEADER5, TEXT)?;
    if rename {
        canvas.text_line(
            "Give your look a name.",
            [span[0], y + canvas.r(5.6)],
            content_width,
            CAPTION,
            TEXT_DIMMER,
        )?;
        widgets::text_field(
            canvas,
            view,
            [span[0], y + canvas.r(8.4), span[1], y + canvas.r(13.2)],
            &editor.draft,
            "Name",
            !view.dressing_room.busy && view.field == Some(MenuField::SkinName),
            (!view.dressing_room.busy).then_some(MenuAction::EditSkinName),
        )?;
    } else {
        canvas.text_line(
            &editor.draft,
            [span[0], y + canvas.r(5.8)],
            content_width,
            BODY,
            TEXT,
        )?;
        canvas.text(
            description,
            [span[0], y + canvas.r(9.0)],
            content_width,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
    }
    if let Some(message) = view.dressing_room.message.as_deref() {
        canvas.text(
            message,
            [span[0], y + error_top],
            content_width,
            CAPTION,
            theme::DESTRUCTIVE_TINT,
            false,
        )?;
    }
    let mid = (span[0] + span[1]) * 0.5;
    let buttons_y = b[3] - pad - canvas.r(4.4);
    widgets::button(
        canvas,
        view,
        [
            span[0],
            buttons_y,
            mid - canvas.r(0.6),
            buttons_y + canvas.r(4.4),
        ],
        Variant::Secondary,
        "Cancel",
        Some(command(Action::Cancel)),
    )?;
    widgets::button(
        canvas,
        view,
        [
            mid + canvas.r(0.6),
            buttons_y,
            span[1],
            buttons_y + canvas.r(4.4),
        ],
        if rename {
            Variant::Primary
        } else {
            Variant::Destructive
        },
        if rename { "Save name" } else { "Remove" },
        (!view.dressing_room.busy && (!rename || !editor.draft.trim().is_empty())).then_some(
            command(if rename {
                Action::SaveRename
            } else {
                Action::ConfirmDelete
            }),
        ),
    )?;
    canvas.end_entrance(entrance, size)
}
