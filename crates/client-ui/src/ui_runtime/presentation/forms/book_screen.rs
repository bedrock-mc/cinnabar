//! The vanilla two-page book screen for writable, written and lectern books:
//! the spread showing the current page, the page edit controls, and the
//! signing cover. Its buttons press the existing reader actions.

use json_ui::{CollectionItem, Context, DataSource, HitKind, HitRegion, Scalar};
use protocol::MAX_BOOK_PAGES;
use serde_json::Value;

use crate::ui_runtime::book_screen::BookState;
use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;
use crate::ui_runtime::presentation::screens::{ReaderButton, Widget};

pub(super) const SCREEN: &str = "book.book_screen";
/// Characters a page takes, as the reader enforces.
const MAX_PAGE_LENGTH: u64 = 256;

pub(super) fn context(context: Context) -> Context {
    context.with_var("max_page_length", Value::from(MAX_PAGE_LENGTH))
}

/// The spread, its controls, and the signing cover's fields.
pub(super) fn book_data(data: &mut DataSource, book: &BookState) {
    let spread = book.spread();
    let editable = book.editable;
    for (name, value) in [
        ("#viewing", !book.signing),
        ("#signing", book.signing),
        ("#editable", editable),
        ("#author_editable", false),
        ("#prev_page_button_active", spread > 0),
        (
            "#next_page_button_active",
            spread + 2 < book.pages.len() || editable,
        ),
        ("#finalize_button_enabled", !book.title.trim().is_empty()),
    ] {
        data.set_global(name, Scalar::Bool(value));
    }
    // The edit buttons bind globally inside their per-page panels: they answer
    // for the page being edited.
    if let Some(at) = book.editing.filter(|at| *at < book.pages.len()) {
        for (name, value) in [
            ("#edit_controls_active", editable),
            (
                "#insert_page_active",
                editable && book.pages.len() < MAX_BOOK_PAGES,
            ),
            ("#swap_left_active", editable && at > 0),
            ("#swap_right_active", editable && at + 1 < book.pages.len()),
        ] {
            data.set_global(name, Scalar::Bool(value));
        }
    }
    data.set_global(
        "#title_text_box_item_name",
        Scalar::Text(book.title.clone()),
    );
    data.set_global(
        "#author_text_box_item_name",
        Scalar::Text(book.author.clone()),
    );
    let pages = (spread..spread + 2)
        .map(|index| {
            let Some(text) = book.pages.get(index) else {
                return CollectionItem::default()
                    .with("#page_visible", Scalar::Bool(false))
                    .with("#is_text_page", Scalar::Bool(false))
                    .with("#page_number", Scalar::Text(String::new()));
            };
            let flag = |value: bool| Scalar::Bool(editable && value);
            CollectionItem::default()
                .with("#page_visible", Scalar::Bool(true))
                .with("#is_text_page", Scalar::Bool(true))
                .with("#editable", Scalar::Bool(editable))
                .with("#text_box_item_name", Scalar::Text(text.clone()))
                .with("#page_number", Scalar::Text((index + 1).to_string()))
                .with("#edit_button_active", flag(book.editing != Some(index)))
                .with("#edit_controls_active", flag(book.editing == Some(index)))
                .with(
                    "#insert_page_active",
                    flag(book.pages.len() < MAX_BOOK_PAGES),
                )
                .with("#swap_left_active", flag(index > 0))
                .with("#swap_right_active", flag(index + 1 < book.pages.len()))
        })
        .collect();
    data.set_collection("book_pages", pages);
}

/// The reader action a book control presses.
pub(super) fn book_hit(region: &HitRegion) -> Option<InventoryCellHit> {
    let side = u8::try_from(region.collection_index.unwrap_or(0))
        .ok()?
        .min(1);
    let button = match region.pressed.as_deref() {
        Some("button.prev_page") => ReaderButton::PrevSpread,
        Some("button.next_page") => ReaderButton::NextSpread,
        Some("button.book_exit") => ReaderButton::Done,
        Some("button.sign_book") => ReaderButton::Sign,
        Some("button.edit_page") => ReaderButton::EditPage(side),
        Some("button.finalize") => ReaderButton::Finalize,
        Some("button.insert_text_page") => ReaderButton::InsertPage(side),
        Some("button.delete_page") => ReaderButton::DeletePage(side),
        Some("button.swap_page_left") => ReaderButton::SwapLeft(side),
        Some("button.swap_page_right") => ReaderButton::SwapRight(side),
        // A page's text field sits under its own factory; its page panel names the side.
        _ if region.kind == HitKind::EditBox && region.key.contains("page_panel_") => {
            ReaderButton::FocusPage(u8::from(region.key.contains("page_panel_right")))
        }
        _ => return None,
    };
    Some(InventoryCellHit::Widget(Widget::Reader(button)))
}

/// The text field typing reaches: the title while signing, else the current page.
pub(super) fn focused(regions: &[HitRegion], book: &BookState) -> Option<String> {
    if !book.editable {
        return None;
    }
    let side = book.page - book.spread();
    regions
        .iter()
        .filter(|region| region.kind == HitKind::EditBox)
        .find(|region| {
            if book.signing {
                region.control_name.as_deref() == Some("#title_text_box")
            } else {
                let right = region.key.contains("page_panel_right");
                region.key.contains("page_panel_") && usize::from(right) == side
            }
        })
        .map(|region| region.key.clone())
}
