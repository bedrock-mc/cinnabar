//! Anonymous update notices rendered through the home screen.

use super::{Fixture, MenuView};

/// Shows a verified release without contacting the updater or using account data.
pub(super) fn home(writable: bool, protocol_required: bool) -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.updates.ready = Some(launcher::menu::updates::Ready {
        version: launcher::PRODUCT_VERSION.into(),
        writable,
        protocol_required,
    });
    Fixture::Menu(Box::new(view))
}
