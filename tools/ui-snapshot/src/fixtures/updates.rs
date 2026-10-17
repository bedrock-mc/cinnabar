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
    if protocol_required || !writable {
        view.dialog = Some(launcher::menu::MenuDialog::UpdateNotice);
    }
    Fixture::Menu(Box::new(view))
}

/// Shows the writable release after the player opens its corner pill.
pub(super) fn expanded() -> Fixture {
    let Fixture::Menu(mut view) = home(true, false) else {
        unreachable!()
    };
    view.dialog = Some(launcher::menu::MenuDialog::UpdateNotice);
    Fixture::Menu(view)
}
