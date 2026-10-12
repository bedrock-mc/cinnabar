//! Visual state for edit boxes whose text is owned by the host's editors.

use super::ScreenArt;

#[derive(Clone, Copy)]
pub struct Feedback {
    pub(super) selection: Option<[usize; 2]>,
}

impl Feedback {
    pub fn from_chat(editor: &ui::ChatEditor) -> Self {
        Self {
            selection: editor.selection().map(|range| [range.start, range.end]),
        }
    }

    /// A host editor's selection as byte offsets; its own caret state owns blink and placement.
    pub fn new(selection: Option<[usize; 2]>) -> Self {
        Self { selection }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Target<'a> {
    pub(super) text: &'a str,
    pub(super) placeholder: Option<&'a str>,
    pub(super) feedback: Feedback,
}

impl<'a> Target<'a> {
    /// Resolves authored text/placeholder targets from the actual focused edit component.
    pub(super) fn from_frame(frame: &'a json_ui::FormRender, art: ScreenArt<'_>) -> Option<Self> {
        let feedback = art.edit?;
        let key = art.view?.focused.as_deref()?;
        let edit = frame
            .hits
            .iter()
            .find(|hit| hit.key == key)?
            .widget
            .edit
            .as_ref()?;
        Some(Self {
            text: &edit.text_target.as_ref()?.0,
            placeholder: edit.placeholder.as_deref(),
            feedback,
        })
    }
}
