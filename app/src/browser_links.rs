//! Opens the Microsoft sign-in page from the menu's link button. Browsers block a tab a page
//! opens outside the click that asks for it, and the game reacts to a click a frame later, so
//! the page is opened inside the click's own event instead.

use std::cell::RefCell;

use bevy::prelude::{App, Res, Update};
use client_shell::menu::MenuRuntime;
use wasm_bindgen::{JsCast, closure::Closure};

thread_local! {
    /// The page a primary click on the game would open now.
    static LINK_ON_CLICK: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Opens the sign-in page inside each primary click on its link button.
pub(crate) fn configure(app: &mut App) {
    install_click_opener();
    app.add_systems(Update, publish_link);
}

/// Records the link a click would open, from the button under the pointer.
fn publish_link(menu: Option<Res<MenuRuntime>>) {
    let link = menu.and_then(|menu| menu.sign_in_link_hovered());
    LINK_ON_CLICK.with_borrow_mut(|slot| *slot = link);
}

fn install_click_opener() {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let listener = Closure::<dyn Fn(web_sys::MouseEvent)>::new(|event: web_sys::MouseEvent| {
        if event.button() != 0 {
            return;
        }
        if let (Some(window), Some(link)) =
            (web_sys::window(), LINK_ON_CLICK.with_borrow(Clone::clone))
        {
            let _ = window.open_with_url_and_target(&link, "_blank");
        }
    });
    // winit cancels the canvas's `pointerdown`, so browsers fire no `mousedown` after it; a
    // mouse's `pointerdown` still activates the page.
    let _ =
        document.add_event_listener_with_callback("pointerdown", listener.as_ref().unchecked_ref());
    // The listener lives as long as the page.
    listener.forget();
}
