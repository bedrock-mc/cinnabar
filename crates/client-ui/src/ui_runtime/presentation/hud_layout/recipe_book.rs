//! The Java-styled recipe book: a toggle beside the crafting grid and a panel
//! of recipes the inventory can supply.

use super::{HudFrame, HudLayout, UiPresentationError, UiRuntime};
use crate::ui_runtime::presentation::inventory_pointer::{InventoryCellHit, InventoryScreen};
use crate::ui_runtime::presentation::screens::{
    self, BOOK_CELL_SIZE, BOOK_CELLS, BOOK_PANEL, Widget,
};

impl HudLayout<'_> {
    /// Draws the book toggle and, while open, the recipe panel.
    pub(super) fn recipe_book(
        &mut self,
        runtime: &UiRuntime,
        frame: &HudFrame,
        screen: InventoryScreen,
        main_origin: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let Some(toggle) = screens::book_toggle_origin(screen) else {
            return Ok(());
        };
        let state = runtime.screen_state();
        let hover = match state.hover {
            Some(InventoryCellHit::Widget(widget)) => Some(widget),
            _ => None,
        };
        let at = [main_origin[0] + toggle[0], main_origin[1] + toggle[1]];
        let shade = if hover == Some(Widget::BookToggle) {
            170
        } else {
            139
        };
        let size = screens::book_toggle_size();
        self.solid_gui(at, size, [0, 0, 0, 255])?;
        self.solid_gui(
            [at[0] + 1.0, at[1] + 1.0],
            [size[0] - 2.0, size[1] - 2.0],
            [shade, shade, shade, 255],
        )?;
        if let Some(icon) = frame.window_icons.book_button {
            self.icon_gui(icon, [at[0] + 2.0, at[1] + 1.0])?;
        }
        if !state.book_open {
            return Ok(());
        }
        let book = screens::book_origin(main_origin);
        self.panel(book, BOOK_PANEL)?;
        let title = frame.window_text.book_title.as_deref().unwrap_or("Recipes");
        self.inventory_label(title, [book[0] + 11.0, book[1] + 8.0])?;
        for index in 0..BOOK_CELLS {
            let cell = screens::book_cell_origin(index, book);
            let hot = hover == Some(Widget::BookRecipe(index as u8));
            let shade = if hot { 170 } else { 139 };
            self.solid_gui(cell, [BOOK_CELL_SIZE; 2], [55, 55, 55, 255])?;
            self.solid_gui(
                [cell[0] + 1.0, cell[1] + 1.0],
                [BOOK_CELL_SIZE - 2.0, BOOK_CELL_SIZE - 2.0],
                [shade, shade, shade, 255],
            )?;
            if let Some(icon) = frame.window_icons.book[index] {
                self.icon_gui(icon, [cell[0] + 4.0, cell[1] + 4.0])?;
            }
        }
        for next in [false, true] {
            let enabled = if next {
                frame.window_icons.book_more
            } else {
                state.book_page > 0
            };
            let at = screens::book_page_origin(next, book);
            let size = screens::book_page_size();
            let shade = if !enabled { 90 } else { 170 };
            self.solid_gui(at, size, [0, 0, 0, 255])?;
            self.solid_gui(
                [at[0] + 1.0, at[1] + 1.0],
                [size[0] - 2.0, size[1] - 2.0],
                [shade, shade, shade, 255],
            )?;
            self.ui_text(
                if next { ">" } else { "<" },
                [at[0] + 4.0, at[1] + 4.0],
                [255; 4],
                true,
            )?;
        }
        Ok(())
    }
}
