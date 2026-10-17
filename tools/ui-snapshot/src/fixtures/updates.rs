//! Anonymous update notices rendered through the home screen.

use super::{Fixture, MenuView};

/// Shows a verified release without contacting the updater or using account data.
pub(super) fn home(writable: bool, protocol_required: bool) -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.updates.ready = Some(launcher::menu::updates::Ready {
        version: next_version(),
        writable,
        protocol_required,
    });
    Fixture::Menu(Box::new(view))
}

/// Demonstrates a newer patch release without copying the current product version.
fn next_version() -> String {
    let core = launcher::PRODUCT_VERSION.split(['-', '+']).next().unwrap();
    let (prefix, patch) = core.rsplit_once('.').expect("product version has a patch");
    format!(
        "{prefix}.{}",
        patch.parse::<u64>().expect("numeric patch") + 1
    )
}

/// Shows the authenticated account placement without loading a real account or skin.
pub(super) fn signed_in() -> Fixture {
    let Fixture::Menu(mut view) = home(true, false) else {
        unreachable!()
    };
    view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    view.feeds.profile.gamertag = "ExamplePlayer".into();
    Fixture::Menu(view)
}
