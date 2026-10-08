//! Immutable blank pages for reserved font slots.

use std::sync::OnceLock;

use render_model::{UI_FALLBACK_FONT_PAGE_SIDE, UI_LOCAL_FONT_PAGE_SIDE, UiTexturePage};

pub fn blank_fallback_font_page() -> UiTexturePage {
    static BLANK: OnceLock<UiTexturePage> = OnceLock::new();
    BLANK
        .get_or_init(|| {
            UiTexturePage::coverage(
                [UI_FALLBACK_FONT_PAGE_SIDE; 2],
                vec![0; (UI_FALLBACK_FONT_PAGE_SIDE * UI_FALLBACK_FONT_PAGE_SIDE) as usize].into(),
            )
            .expect("bounded fallback font extent")
        })
        .clone()
}

pub fn blank_local_font_page() -> UiTexturePage {
    static BLANK: OnceLock<UiTexturePage> = OnceLock::new();
    BLANK
        .get_or_init(|| {
            let side = UI_LOCAL_FONT_PAGE_SIDE;
            UiTexturePage::owned([side; 2], vec![0; side as usize * side as usize * 4].into())
                .expect("bounded private-font page has a valid extent")
        })
        .clone()
}
