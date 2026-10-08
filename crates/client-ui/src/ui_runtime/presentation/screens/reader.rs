//! Geometry of the book reader and editor panel.

pub const READER_PANEL: [f32; 2] = [192.0, 192.0];
/// Page text box: origin and width in GUI pixels.
pub const PAGE_TEXT_ORIGIN: [f32; 2] = [36.0, 18.0];
pub const PAGE_TEXT_WIDTH: f32 = 114.0;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ReaderButton {
    Prev,
    Next,
    Done,
    Sign,
    Finalize,
    Cancel,
    /// The two-page spread's arrows.
    PrevSpread,
    NextSpread,
    /// Edits of the spread's left (`0`) or right (`1`) page; `EditPage`
    /// shows or hides that page's edit controls.
    EditPage(u8),
    InsertPage(u8),
    DeletePage(u8),
    SwapLeft(u8),
    SwapRight(u8),
    /// Typing goes to the spread's left (`0`) or right (`1`) page.
    FocusPage(u8),
}

/// The buttons the current mode shows, panel-relative `(button, position, size)`.
pub fn reader_buttons(editable: bool, signing: bool) -> Vec<(ReaderButton, [f32; 2], [f32; 2])> {
    let mut buttons = Vec::new();
    if !signing {
        buttons.push((ReaderButton::Prev, [43.0, 157.0], [23.0, 13.0]));
        buttons.push((ReaderButton::Next, [116.0, 157.0], [23.0, 13.0]));
    }
    if editable {
        if signing {
            buttons.push((ReaderButton::Finalize, [12.0, 175.0], [80.0, 13.0]));
            buttons.push((ReaderButton::Cancel, [100.0, 175.0], [80.0, 13.0]));
        } else {
            buttons.push((ReaderButton::Done, [12.0, 175.0], [80.0, 13.0]));
            buttons.push((ReaderButton::Sign, [100.0, 175.0], [80.0, 13.0]));
        }
    }
    buttons
}

/// The button under a panel-relative point.
pub fn reader_hit(local: [f32; 2], editable: bool, signing: bool) -> Option<ReaderButton> {
    reader_buttons(editable, signing)
        .into_iter()
        .find(|(_, pos, size)| {
            local[0] >= pos[0]
                && local[0] < pos[0] + size[0]
                && local[1] >= pos[1]
                && local[1] < pos[1] + size[1]
        })
        .map(|(button, _, _)| button)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_follow_the_mode() {
        assert_eq!(
            reader_hit([44.0, 158.0], false, false),
            Some(ReaderButton::Prev)
        );
        assert_eq!(reader_hit([13.0, 176.0], false, false), None);
        assert_eq!(
            reader_hit([13.0, 176.0], true, false),
            Some(ReaderButton::Done)
        );
        assert_eq!(
            reader_hit([13.0, 176.0], true, true),
            Some(ReaderButton::Finalize)
        );
        assert_eq!(reader_hit([44.0, 158.0], true, true), None);
    }
}
