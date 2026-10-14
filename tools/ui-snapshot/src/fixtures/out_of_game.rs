//! Anonymous session-boundary and launcher popup fixtures.

use super::Fixture;
use launcher::menu::{MenuAction, MenuDialog, MenuScreen, MenuView};

/// Creates a menu fixture with no account or network services.
fn menu() -> MenuView {
    MenuView::new(true, "Player".into())
}

/// Wraps a launcher dialog over settings.
fn dialog(value: MenuDialog) -> Fixture {
    let mut view = menu();
    view.screen = MenuScreen::Settings;
    view.dialog = Some(value);
    Fixture::Menu(Box::new(view))
}

/// Shows a retryable remote-session failure.
pub(super) fn disconnect() -> Fixture {
    let mut view = menu();
    view.disconnect_message = Some(launcher::menu::disconnect::from_server(
        "The server is restarting. Please try again in a moment.",
    ));
    view.can_reconnect = true;
    view.focused_action = Some(MenuAction::Reconnect);
    view.navigation_focus_visible = true;
    Fixture::Menu(Box::new(view))
}

/// Shows wrapping, paragraph breaks and server-supplied formatting.
pub(super) fn disconnect_ban() -> Fixture {
    let Fixture::Menu(mut view) = disconnect() else {
        unreachable!()
    };
    view.disconnect_message = Some(launcher::menu::disconnect::from_server(
        "§c§lYou are banned from this server.§r\n\n§eReason: §fRepeated violations of the community rules after multiple warnings. This restriction applies to all game modes on this server.\n\n§7Duration: §f7 days\n§7Reference: §fEXAMPLE-1234\n\n§aIf you believe this was a mistake, contact the moderation team with the reference above and a description of what happened.",
    ));
    Fixture::Menu(view)
}

/// Shows an acknowledgement when no reconnect destination exists.
pub(super) fn disconnect_no_retry() -> Fixture {
    let Fixture::Menu(mut view) = disconnect() else {
        unreachable!()
    };
    view.can_reconnect = false;
    view.focused_action = Some(MenuAction::DismissDialog);
    Fixture::Menu(view)
}

/// Shows an unreachable destination under the failed-connection title.
pub(super) fn connect_failed() -> Fixture {
    let Fixture::Menu(mut view) = disconnect() else {
        unreachable!()
    };
    view.disconnect_message =
        Some("bridge connection failed: Connection refused (os error 61)".into());
    Fixture::Menu(view)
}

/// Shows the first-join trust question over progress.
pub(super) fn server_trust() -> Fixture {
    let mut view = menu();
    view.connecting = true;
    view.feeds.server_trust = Some(launcher::menu::ServerTrustPrompt {
        id: 1,
        url: "https://example.invalid".into(),
        from_session_core: false,
    });
    Fixture::Menu(Box::new(view))
}

/// Shows a pending join request from an anonymous friend.
pub(super) fn join_request() -> Fixture {
    let mut view = menu();
    view.join_request = Some("Friend".into());
    Fixture::Menu(Box::new(view))
}

/// Shows the saved-server removal confirmation.
pub(super) fn remove_server() -> Fixture {
    let Fixture::Menu(mut view) = dialog(MenuDialog::RemoveSaved(0)) else {
        unreachable!()
    };
    view.servers.push(launcher::menu::SavedServer {
        name: "Example server".into(),
        address: "example.invalid".into(),
        favorite: false,
        last_joined_unix: 0,
    });
    Fixture::Menu(view)
}

/// Shows the cached-data deletion confirmation.
pub(super) fn storage_delete() -> Fixture {
    dialog(MenuDialog::StorageDelete)
}

/// Shows a local storage failure.
pub(super) fn storage_error() -> Fixture {
    let Fixture::Menu(mut view) = dialog(MenuDialog::StorageError) else {
        unreachable!()
    };
    std::sync::Arc::make_mut(&mut view.storage).error =
        Some("The selected files could not be removed. Please try again.".into());
    Fixture::Menu(view)
}

/// Shows a storage failure long enough that its body scrolls above pinned actions.
pub(super) fn storage_error_long() -> Fixture {
    let Fixture::Menu(mut view) = dialog(MenuDialog::StorageError) else {
        unreachable!()
    };
    std::sync::Arc::make_mut(&mut view.storage).error = Some(
        (1..=24)
            .map(|line| format!("Line {line}: example.invalid/cache/pack-{line} could not be removed because another process holds it open."))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    Fixture::Menu(view)
}

/// Shows the help-center question.
pub(super) fn settings_help() -> Fixture {
    dialog(MenuDialog::SettingsSupport(
        launcher::menu::settings_support::SupportDialog::Help,
    ))
}

/// Shows the bundled font notice.
pub(super) fn font_license() -> Fixture {
    dialog(MenuDialog::SettingsSupport(
        launcher::menu::settings_support::SupportDialog::FontLicense,
    ))
}

/// Shows the keyboard-binding reset confirmation.
pub(super) fn reset_bindings() -> Fixture {
    dialog(MenuDialog::SettingsResetBindings(false))
}

/// Shows the settings reset confirmation.
pub(super) fn reset_settings() -> Fixture {
    dialog(MenuDialog::SettingsResetGroup(
        launcher::menu::settings_options::SettingsGroup::Video,
    ))
}
