//! The Java-styled book reader, writable-book editor and lectern page.

use ui::{TextLayoutRequest, TextStyle};

use super::{HudFrame, HudLayout, UiPresentationError, UiRuntime};
use crate::ui_runtime::presentation::inventory_pointer::{InventoryCellHit, InventoryScreen};
use crate::ui_runtime::presentation::screens::{
    self, PAGE_TEXT_ORIGIN, PAGE_TEXT_WIDTH, ReaderButton, Widget,
};

const PAGE_COLOR: [u8; 4] = [229, 213, 170, 255];
const PAGE_EDGE: [u8; 4] = [128, 96, 56, 255];
const INK: [u8; 4] = [32, 24, 16, 255];

fn button_label(button: ReaderButton) -> &'static str {
    match button {
        ReaderButton::Prev => "<",
        ReaderButton::Next => ">",
        ReaderButton::Done => "Done",
        ReaderButton::Sign => "Sign",
        ReaderButton::Finalize => "Sign and Close",
        ReaderButton::Cancel => "Cancel",
        // Only the vanilla two-page screen shows these.
        _ => "",
    }
}

impl HudLayout<'_> {
    /// Draws `text` wrapped to `width` GUI pixels.
    fn wrapped_text(
        &mut self,
        text: &str,
        position: [f32; 2],
        width: f32,
        color: [u8; 4],
    ) -> Result<(), UiPresentationError> {
        if text.is_empty() {
            return Ok(());
        }
        let scale = self.geometry.scale;
        let layout = self
            .layouts
            .layout(TextLayoutRequest {
                text,
                style: TextStyle::default(),
                width_64: (width * scale * 64.0) as u32,
                line_height_64: super::super::TEXT_LINE_HEIGHT_64,
                baseline_64: super::super::TEXT_BASELINE_64,
                scale: self.text_scale(9.0),
                font: self.font,
                wrap: Default::default(),
            })
            .map_err(UiPresentationError::Text)?;
        self.text_gui(layout, position, color)
    }

    pub(super) fn book_screen(
        &mut self,
        runtime: &UiRuntime,
        _frame: &HudFrame,
    ) -> Result<(), UiPresentationError> {
        let Some(book) = runtime.screen_state().book.as_ref() else {
            return Ok(());
        };
        let g = self.geometry;
        self.solid_gui([0.0, 0.0], [g.gui_width, g.gui_height], [0, 0, 0, 150])?;
        let origin = screens::panel_origin(InventoryScreen::Book, [g.gui_width, g.gui_height]);
        self.solid_gui(origin, screens::READER_PANEL, PAGE_EDGE)?;
        self.solid_gui(
            [origin[0] + 2.0, origin[1] + 2.0],
            [
                screens::READER_PANEL[0] - 4.0,
                screens::READER_PANEL[1] - 4.0,
            ],
            PAGE_COLOR,
        )?;
        let text_at = [
            origin[0] + PAGE_TEXT_ORIGIN[0],
            origin[1] + PAGE_TEXT_ORIGIN[1],
        ];
        if book.signing {
            self.ui_text("Enter Book Title:", [text_at[0], text_at[1]], INK, false)?;
            let title = format!("{}_", book.title);
            self.ui_text(&title, [text_at[0], text_at[1] + 24.0], INK, false)?;
            self.ui_text(
                "Note: You cannot edit a book after signing it.",
                [text_at[0], text_at[1] + 60.0],
                [96, 96, 96, 255],
                false,
            )?;
        } else {
            let counter = format!("Page {} of {}", book.page + 1, book.pages.len());
            self.ui_text(&counter, [origin[0] + 100.0, origin[1] + 8.0], INK, false)?;
            let mut page = book.pages[book.page].clone();
            if book.editable {
                page.push('_');
            }
            self.wrapped_text(&page, text_at, PAGE_TEXT_WIDTH, INK)?;
        }
        let hover = match runtime.screen_state().hover {
            Some(InventoryCellHit::Widget(Widget::Reader(button))) => Some(button),
            _ => None,
        };
        for (button, pos, size) in screens::reader_buttons(book.editable, book.signing) {
            let at = [origin[0] + pos[0], origin[1] + pos[1]];
            let shade = if hover == Some(button) { 200 } else { 160 };
            self.solid_gui(at, size, [0, 0, 0, 255])?;
            self.solid_gui(
                [at[0] + 1.0, at[1] + 1.0],
                [size[0] - 2.0, size[1] - 2.0],
                [shade, shade, shade, 255],
            )?;
            let label = button_label(button);
            let width = self.measure(label)?;
            self.ui_text(
                label,
                [at[0] + ((size[0] - width) * 0.5).floor(), at[1] + 2.0],
                [255; 4],
                true,
            )?;
        }
        Ok(())
    }
}
