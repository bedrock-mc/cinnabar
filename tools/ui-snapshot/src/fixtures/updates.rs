//! Anonymous update notices rendered through the home screen.

use super::{Fixture, GUI_SCALE, MenuView, SIZE, SMALL_SIZE, State};

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

/// Shows a downloaded update waiting for the original client to close.
pub(super) fn pending() -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.updates.active = false;
    view.updates.manual_pending = true;
    Fixture::Menu(Box::new(view))
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

/// Shows optional mod update discovery without app updates or a network request.
pub(super) fn mods(ready: bool, settings: bool) -> Fixture {
    let Fixture::Menu(mut view) = super::mods::declared() else {
        unreachable!()
    };
    if !settings {
        view.screen = launcher::menu::MenuScreen::Home;
    }
    if ready {
        std::sync::Arc::make_mut(&mut view.mods).library.installed[0]
            .metadata
            .update_url = Some(
            "https://github.com/example/zoom-plus/releases/latest/download/zoom-plus.wasm"
                .parse()
                .unwrap(),
        );
        std::sync::Arc::make_mut(&mut view.mods)
            .library
            .updates
            .push(launcher::mods::AvailableUpdate {
                id: 1,
                version: "v1.3.0".into(),
                release_url: "https://github.com/example/zoom-plus/releases/tag/v1.3.0".into(),
            });
    }
    Fixture::Menu(view)
}

/// Shows an explicitly requested in-app update while download or installation is pending.
pub(super) fn mod_installing() -> Fixture {
    let Fixture::Menu(mut view) = mods(true, true) else {
        unreachable!()
    };
    let library = &mut std::sync::Arc::make_mut(&mut view.mods).library;
    library.busy = true;
    library.update_progress = vec![launcher::mods::UpdateProgress {
        id: 1,
        phase: launcher::mods::UpdatePhase::Downloading,
        received_bytes: 512 * 1024,
        total_bytes: 1024 * 1024,
        bytes_per_sec: Some(256 * 1024),
        eta_secs: Some(2),
    }];
    Fixture::Menu(view)
}

/// Shows real post-download stages without fabricating byte percentages for local work.
pub(super) fn mod_stage(phase: launcher::mods::UpdatePhase) -> Fixture {
    let Fixture::Menu(mut view) = mod_installing() else {
        unreachable!()
    };
    let library = &mut std::sync::Arc::make_mut(&mut view.mods).library;
    library.update_progress = vec![launcher::mods::UpdateProgress::stage(1, phase)];
    if phase == launcher::mods::UpdatePhase::Reloading {
        library.installed[0].metadata.version = Some("1.3.0".into());
        library.updates.clear();
        library.active[0].status = launcher::mods::Status::Starting;
    }
    Fixture::Menu(view)
}

/// Shows update status with the selected sidebar tab scrolled into view.
pub(super) fn settings(status: launcher::menu::updates::Status) -> Fixture {
    let section = launcher::menu::settings_sections::SettingSection::Updates.definition();
    let mut view = super::settings(section.name);
    view.updates.status = status;
    view.focused_action = Some(launcher::menu::MenuAction::SettingsSection(section.index));
    Fixture::Menu(Box::new(view))
}

/// Home screens with each update notice, at desktop and compact sizes.
pub(super) const STATES: &[State] = &[
    State {
        name: "home-update-compact",
        size: [480, 640],
        gui_scale: 3,
        fixture: || home(true, false),
    },
    State {
        name: "home-signed-in-update",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: signed_in,
    },
    State {
        name: "home-update-ready",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || home(true, false),
    },
    State {
        name: "home-update-required",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || home(true, true),
    },
    State {
        name: "home-update-download",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || home(false, false),
    },
    State {
        name: "home-update-pending",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: pending,
    },
    State {
        name: "home-update-pending-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: pending,
    },
    State {
        name: "home-update-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: || home(true, false),
    },
];
