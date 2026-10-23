//! Marketplace presentation fixtures with a fictional signed-in profile.

use super::{Arc, Fixture, GUI_SCALE, MenuAction, MenuView, SIZE, State, settings};
use launcher::{
    marketplace::{Action, catalog},
    menu::auth::AuthState,
    mods,
};

/// Opens the marketplace from Mods and focuses its selected navigation tab.
fn view(action: Action) -> MenuView {
    let mut view = settings(mods::SECTION);
    let account = catalog()
        .accounts
        .get("player-demo")
        .expect("marketplace player presentation profile");
    view.display_name.clone_from(&account.name);
    view.feeds.profile.gamertag.clone_from(&account.gamertag);
    view.auth_state = AuthState::Authenticated;
    Arc::make_mut(&mut view.mods).marketplace.apply(action);
    view.focused_action = Some(MenuAction::Mods(mods::Action::Marketplace(action)));
    view.navigation_focus_visible = true;
    view
}

/// Shows the published catalog with the player profile and an empty library.
fn browse() -> Fixture {
    Fixture::Menu(Box::new(view(Action::Browse)))
}

/// Opens the first published listing and focuses its library action.
fn details() -> Fixture {
    let mut view = view(Action::Browse);
    let id = view
        .mods
        .marketplace
        .listings()
        .first()
        .expect("published marketplace fixture listing")
        .id;
    Arc::make_mut(&mut view.mods)
        .marketplace
        .apply(Action::Details(id));
    view.focused_action = Some(MenuAction::Mods(mods::Action::Marketplace(Action::Save(
        id,
    ))));
    Fixture::Menu(Box::new(view))
}

/// Shows an empty library using presentation data without provider authentication.
fn library_empty() -> Fixture {
    Fixture::Menu(Box::new(view(Action::Library)))
}

pub(super) const STATES: &[State] = &[
    State {
        name: "marketplace-browse",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: browse,
    },
    State {
        name: "marketplace-details",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: details,
    },
    State {
        name: "marketplace-library-empty",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: library_empty,
    },
    State {
        name: "marketplace-small",
        size: [640, 360],
        gui_scale: GUI_SCALE,
        fixture: browse,
    },
];
