use super::super::{
    paint::Canvas,
    theme::{self, BODY, CAPTION, HEADER5, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use super::command;
use launcher::dressing_room::{Action, SkinEditor, SkinEditorMode, SkinEditorTarget};
use launcher::menu::{MenuAction, MenuField, MenuView};
use ui_engine::engine::screen::PaintError;

const MARGIN: f32 = 1.6;
const PAD: f32 = 2.4;
const FIELD_BOTTOM: f32 = 13.2;
const DESCRIPTION_TOP: f32 = 9.0;
const BUTTON_HEIGHT: f32 = 4.4;
const ERROR_GAP: f32 = 1.6;
const BUTTON_GAP: f32 = 2.0;

struct Layout {
    width: f32,
    pad: f32,
    content_width: f32,
    error_top: f32,
    height: f32,
}

/// The removal explanation for the editor's selected collection.
fn description(editor: &SkinEditor) -> &'static str {
    match editor.target {
        SkinEditorTarget::Cape => {
            "This removes the imported cape from your collection. You can import it again later."
        }
        SkinEditorTarget::Skin => {
            "This removes the imported skin from your collection. You can import it again later."
        }
    }
}

impl Layout {
    /// Measures the complete editor, including wrapped removal and error text.
    fn new(
        canvas: &mut Canvas<'_>,
        editor: &SkinEditor,
        message: Option<&str>,
        size: [f32; 2],
    ) -> Result<Self, PaintError> {
        let width = canvas.r(48.0).min(size[0] - canvas.r(MARGIN * 2.0));
        let pad = canvas.r(PAD);
        let content_width = width - pad * 2.0;
        let body_bottom = if editor.mode == SkinEditorMode::Rename {
            canvas.r(FIELD_BOTTOM)
        } else {
            canvas.r(DESCRIPTION_TOP)
                + canvas.measure_height(description(editor), content_width, CAPTION)?
        };
        let error_top = body_bottom + canvas.r(ERROR_GAP);
        let error_height = message
            .map(|message| canvas.measure_height(message, content_width, CAPTION))
            .transpose()?
            .unwrap_or(0.0);
        let buttons_top = if error_height > 0.0 {
            error_top + error_height + canvas.r(BUTTON_GAP)
        } else {
            canvas.r(16.8).max(body_bottom + canvas.r(BUTTON_GAP))
        };
        Ok(Self {
            width,
            pad,
            content_width,
            error_top,
            height: buttons_top + canvas.r(BUTTON_HEIGHT) + pad,
        })
    }

    /// Keeps fitting layouts unchanged and reserves a margin around short-window dialogs.
    fn fitting_scale(&self, canvas: &Canvas<'_>, height: f32) -> f32 {
        ((height - canvas.r(MARGIN * 2.0)) / self.height).min(1.0)
    }
}

/// Fits all editor text and actions together before painting the modal.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), PaintError> {
    let Some(editor) = view.dressing_room.editor.as_ref() else {
        return Ok(());
    };
    let message = view.dressing_room.message.as_deref();
    let layout = Layout::new(canvas, editor, message, size)?;
    let scale = layout.fitting_scale(canvas, size[1]);
    canvas.with_scale(scale, |canvas| {
        let layout = if scale < 1.0 {
            Layout::new(canvas, editor, message, size)?
        } else {
            layout
        };
        draw_layout(canvas, view, size, layout)
    })
}

/// Paints a measured editor at one shared text, geometry and pointer scale.
fn draw_layout(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    layout: Layout,
) -> Result<(), PaintError> {
    let Some(editor) = view.dressing_room.editor.as_ref() else {
        return Ok(());
    };
    canvas.overlay(size, theme::OVERLAY_MODAL)?;
    let Layout {
        width,
        pad,
        content_width,
        error_top,
        height,
    } = layout;
    let cape = editor.target == SkinEditorTarget::Cape;
    let rename = editor.mode == SkinEditorMode::Rename;
    let x = (size[0] - width) * 0.5;
    let y = ((size[1] - height) * 0.5).max(canvas.r(MARGIN));
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
            [
                span[0],
                y + canvas.r(8.4),
                span[1],
                y + canvas.r(FIELD_BOTTOM),
            ],
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
            description(editor),
            [span[0], y + canvas.r(DESCRIPTION_TOP)],
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
    let buttons_y = b[3] - pad - canvas.r(BUTTON_HEIGHT);
    widgets::button(
        canvas,
        view,
        [
            span[0],
            buttons_y,
            mid - canvas.r(0.6),
            buttons_y + canvas.r(BUTTON_HEIGHT),
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
            buttons_y + canvas.r(BUTTON_HEIGHT),
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
