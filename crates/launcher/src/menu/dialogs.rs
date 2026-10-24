//! Shared launcher popup wording and actions, independent of the renderer.

use super::{
    MenuAction, MenuDialog, MenuView,
    settings_options::SettingsGroup,
    settings_storage::StorageAction,
    settings_support::{SupportAction, SupportDialog, SupportLink},
};
use std::sync::Arc;

/// A two-button question and the actions each answer takes.
pub struct Popup {
    pub title: String,
    pub body: String,
    pub button1: String,
    pub button2: String,
    pub confirm: MenuAction,
    pub dismiss: MenuAction,
}

/// The active language lookup supplied by presentation.
type Translate<'a> = &'a dyn Fn(&str) -> Option<Arc<str>>;

/// Looks up a label and uses its readable fallback when the language lacks it.
fn translated(translate: Translate<'_>, key: &str, fallback: &str) -> String {
    translate(key).map_or_else(|| fallback.to_owned(), |text| text.to_string())
}

/// Builds a launcher's question with its existing answer actions.
pub fn dialog(view: &MenuView, dialog: MenuDialog, translate: Translate<'_>) -> Popup {
    let (title, body, button1, button2, confirm) = match dialog {
        MenuDialog::MinecraftImport => {
            let snapshot = &view.minecraft_import;
            let confirm = snapshot
                .actions()
                .first()
                .copied()
                .unwrap_or(crate::minecraft_import::Action::Close);
            (
                snapshot.title().into(),
                snapshot.body().to_owned(),
                snapshot.label(confirm).into(),
                "Close".into(),
                MenuAction::MinecraftImport(confirm),
            )
        }
        MenuDialog::Accounts => (
            "Accounts".into(),
            String::new(),
            "Close".into(),
            "Close".into(),
            MenuAction::DismissDialog,
        ),
        MenuDialog::Notice => (
            crate::PRODUCT_NAME.into(),
            view.message.clone().unwrap_or_default(),
            translated(translate, "gui.ok", "OK"),
            String::new(),
            MenuAction::DismissDialog,
        ),
        MenuDialog::SettingsResetGroup(group) => {
            return reset_group(group, translate);
        }
        MenuDialog::SettingsResetBindings(gamepad) => (
            translated(
                translate,
                "controllerLayoutScreen.resetAllBindings",
                "Reset to Default",
            ),
            translated(
                translate,
                "controllerLayoutScreen.confirmation.reset",
                "Reset all bindings to their defaults?",
            ),
            translated(translate, "options.continue", "Continue"),
            translated(translate, "controllerLayoutScreen.cancel", "Cancel"),
            MenuAction::SettingsConfirmResetBindings(gamepad),
        ),
        MenuDialog::SettingsSupport(dialog) => {
            return support(dialog, translate);
        }
        MenuDialog::StorageDelete | MenuDialog::StorageError => {
            return storage(view, dialog, translate);
        }
        MenuDialog::Exit => (
            translated(
                translate,
                "gui.warning.exitGameWarning",
                "Do you want to exit Minecraft?",
            ),
            String::new(),
            translated(translate, "gui.yes", "Yes"),
            translated(translate, "gui.no", "No"),
            MenuAction::ConfirmExit,
        ),
        MenuDialog::DeathQuit => (
            translated(
                translate,
                "deathScreen.quit.confirmToMainMenuTitleWarning",
                "Quit to Main Menu?",
            ),
            translated(
                translate,
                "deathScreen.quit.confirmToMainMenuWarning",
                "Are you sure you want to exit the game to the main menu?",
            ),
            translated(translate, "globalPauseScreen.quit", "Quit"),
            translated(translate, "gui.cancel", "Cancel"),
            MenuAction::ConfirmDeathQuit,
        ),
        MenuDialog::RemoveShaderPack(index, revision) => (
            "Remove shader pack?".into(),
            format!(
                "Remove {} from your shader packs? The imported copy and its saved shader settings will be deleted. The original file you imported is kept.",
                view.shader_packs
                    .get(usize::from(index))
                    .filter(|_| revision == view.shader_packs_revision)
                    .map_or("this pack", String::as_str)
            ),
            "Remove".into(),
            translated(translate, "gui.cancel", "Cancel"),
            MenuAction::ConfirmRemoveShaderPack(index, revision),
        ),
        // The popup's title is one line, so the server names it and the body asks.
        MenuDialog::RemoveSaved(index) => (
            view.servers
                .get(index)
                .map(|server| server.name.clone())
                .unwrap_or_default(),
            translated(
                translate,
                "addExternalServerScreen.removeConfirmation",
                "Are you sure you want to remove this server?",
            ),
            translated(
                translate,
                "addExternalServerScreen.removeButtonLabel",
                "Remove",
            ),
            translated(translate, "gui.cancel", "Cancel"),
            MenuAction::ConfirmRemoveSaved(index),
        ),
    };
    Popup {
        title,
        body,
        button1,
        button2,
        confirm,
        dismiss: MenuAction::DismissDialog,
    }
}

/// Builds the trust question in the active language.
pub fn server_trust(url: &str, translate: Translate<'_>) -> Popup {
    let message = translated(
        translate,
        "permissions.servertrust.message",
        "You are connecting to %1$s for the first time. Only trust servers you recognize.",
    );
    Popup {
        title: translated(
            translate,
            "permissions.servertrust.title",
            "Trust this server?",
        ),
        body: message.replace("%1$s", url),
        button1: translated(
            translate,
            "permissions.servertrust.button.trust",
            "Trust and Join",
        ),
        button2: translated(
            translate,
            "permissions.servertrust.button.doNotTrust",
            "Don't Trust",
        ),
        confirm: MenuAction::ServerTrust(true),
        dismiss: MenuAction::ServerTrust(false),
    }
}

/// Builds the host's answer to a friend's join request.
pub fn join_request(name: &str, translate: Translate<'_>) -> Popup {
    Popup {
        title: crate::menu::join_requests::title(name),
        body: String::new(),
        button1: translated(translate, "gui.accept", "Accept"),
        button2: translated(translate, "gui.decline", "Decline"),
        confirm: MenuAction::JoinRequest(true),
        dismiss: MenuAction::JoinRequest(false),
    }
}

/// The native confirmation asks before applying any defaults.
fn reset_group(group: SettingsGroup, translate: Translate<'_>) -> Popup {
    let words = |key| translated(translate, key, key);
    Popup {
        title: words("options.resetSettings"),
        body: words("options.resetSettings.popUp"),
        button1: words("options.continue"),
        button2: words("gui.cancel"),
        confirm: MenuAction::SettingsConfirmResetGroup(group),
        dismiss: MenuAction::DismissDialog,
    }
}

/// Builds the bundled-font notice or the help-center question.
fn support(dialog: SupportDialog, translate: Translate<'_>) -> Popup {
    let words = |key| translated(translate, key, key);
    let (title, body, button1, button2, confirm) = match dialog {
        SupportDialog::Help => (
            words("feedbackPopup.title"),
            String::new(),
            words("gui.feedbackYes"),
            words("gui.no"),
            MenuAction::SettingsSupport(SupportAction::Open(SupportLink::Help)),
        ),
        SupportDialog::FontLicense => (
            words("options.font_license.name"),
            crate::menu::settings_support::font_licenses(),
            words("gui.close"),
            words("gui.close"),
            MenuAction::DismissDialog,
        ),
    };
    Popup {
        title,
        body,
        button1,
        button2,
        confirm,
        dismiss: MenuAction::DismissDialog,
    }
}

/// Uses the pinned delete prompt, or shows a concrete disk error without hiding failure.
fn storage(view: &MenuView, dialog: crate::menu::MenuDialog, translate: Translate<'_>) -> Popup {
    let words = |key, fallback| translated(translate, key, fallback);
    let (title, body, button1, confirm) = if dialog == crate::menu::MenuDialog::StorageError {
        (
            words("menu.storageManagement", "Storage"),
            view.storage.error.clone().unwrap_or_default(),
            words("gui.ok", "OK"),
            MenuAction::DismissDialog,
        )
    } else if view.storage.deleting_screenshots {
        (
            words(
                "options.dev_deleteLocalScreenshots",
                "Delete Local Screenshots",
            ),
            words(
                "storageManager.delete.content.screenshots",
                "Delete all local screenshots?",
            ),
            words("storageManager.delete.confirm", "Delete"),
            MenuAction::SettingsStorage(StorageAction::ConfirmDelete),
        )
    } else {
        (
            words("storageManager.delete.title", "Delete %s permanently?").replace(
                "%s",
                &words("storageManager.contentType.cachedData", "Cached Data"),
            ),
            words(
                "storageManager.delete.content",
                "Are you sure you want to delete the selected items? %s%s%s",
            )
            .replace("%s", ""),
            words("storageManager.delete.confirm", "Delete"),
            MenuAction::SettingsStorage(StorageAction::ConfirmDelete),
        )
    };
    Popup {
        title,
        body,
        button1,
        button2: words("storageManager.delete.cancel", "Cancel"),
        confirm,
        dismiss: MenuAction::DismissDialog,
    }
}
