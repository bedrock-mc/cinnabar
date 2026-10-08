//! Geometry of the recipe book panel beside the personal and crafting-table screens.

use super::{InventoryScreen, Widget};

pub const BOOK_PANEL: [f32; 2] = [147.0, 166.0];
/// Gap between the book panel and the main panel.
const BOOK_GAP: f32 = 5.0;
pub const BOOK_COLUMNS: usize = 5;
pub const BOOK_ROWS: usize = 4;
pub const BOOK_CELLS: usize = BOOK_COLUMNS * BOOK_ROWS;
pub const BOOK_CELL_SIZE: f32 = 25.0;
const BOOK_GRID: [f32; 2] = [11.0, 31.0];
const PAGE_BUTTON: [f32; 2] = [12.0, 17.0];
const PAGE_PREV: [f32; 2] = [38.0, 141.0];
const PAGE_NEXT: [f32; 2] = [93.0, 141.0];
const TOGGLE_SIZE: [f32; 2] = [20.0, 18.0];

/// Top-left of the book panel for a main panel at `main_origin`.
pub fn book_origin(main_origin: [f32; 2]) -> [f32; 2] {
    [main_origin[0] - BOOK_PANEL[0] - BOOK_GAP, main_origin[1]]
}

/// The toggle button's main-panel-relative corner; only these screens have a book.
pub fn toggle_origin(screen: InventoryScreen) -> Option<[f32; 2]> {
    match screen {
        InventoryScreen::Personal => Some([104.0, 22.0]),
        InventoryScreen::Workbench => Some([5.0, 35.0]),
        _ => None,
    }
}

pub const fn toggle_size() -> [f32; 2] {
    TOGGLE_SIZE
}

pub fn cell_origin(index: usize, book: [f32; 2]) -> [f32; 2] {
    [
        book[0] + BOOK_GRID[0] + (index % BOOK_COLUMNS) as f32 * BOOK_CELL_SIZE,
        book[1] + BOOK_GRID[1] + (index / BOOK_COLUMNS) as f32 * BOOK_CELL_SIZE,
    ]
}

pub fn page_origin(next: bool, book: [f32; 2]) -> [f32; 2] {
    let at = if next { PAGE_NEXT } else { PAGE_PREV };
    [book[0] + at[0], book[1] + at[1]]
}

pub const fn page_size() -> [f32; 2] {
    PAGE_BUTTON
}

fn inside(point: [f32; 2], at: [f32; 2], size: [f32; 2]) -> bool {
    point[0] >= at[0]
        && point[0] < at[0] + size[0]
        && point[1] >= at[1]
        && point[1] < at[1] + size[1]
}

/// The book control under `gui`, for a main panel at `main_origin`.
pub fn book_hit(
    screen: InventoryScreen,
    main_origin: [f32; 2],
    gui: [f32; 2],
    open: bool,
) -> Option<Widget> {
    let toggle = toggle_origin(screen)?;
    if inside(
        gui,
        [main_origin[0] + toggle[0], main_origin[1] + toggle[1]],
        TOGGLE_SIZE,
    ) {
        return Some(Widget::BookToggle);
    }
    if !open {
        return None;
    }
    let book = book_origin(main_origin);
    if let Some(index) =
        (0..BOOK_CELLS).find(|index| inside(gui, cell_origin(*index, book), [BOOK_CELL_SIZE; 2]))
    {
        return Some(Widget::BookRecipe(index as u8));
    }
    [false, true]
        .into_iter()
        .find(|next| inside(gui, page_origin(*next, book), PAGE_BUTTON))
        .map(|next| Widget::BookPage { next })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_controls_hit_only_when_open() {
        let origin = [200.0, 40.0];
        let toggle = [origin[0] + 105.0, origin[1] + 23.0];
        assert_eq!(
            book_hit(InventoryScreen::Personal, origin, toggle, false),
            Some(Widget::BookToggle)
        );
        let book = book_origin(origin);
        let cell = [book[0] + 12.0, book[1] + 32.0];
        assert_eq!(
            book_hit(InventoryScreen::Personal, origin, cell, false),
            None
        );
        assert_eq!(
            book_hit(InventoryScreen::Personal, origin, cell, true),
            Some(Widget::BookRecipe(0))
        );
        assert_eq!(
            book_hit(InventoryScreen::Creative, origin, toggle, true),
            None
        );
    }
}
