//! Host support-link actions.
use launcher::menu::settings_support::*;
use {super::MenuRuntime, launcher::menu::MenuDialog};

impl MenuRuntime {
    /// Opens the authored confirmation or launches its fixed browser destination.
    pub(super) fn activate_support(&mut self, action: SupportAction) {
        match action {
            SupportAction::Dialog(dialog) => {
                self.dialog = Some(MenuDialog::SettingsSupport(dialog))
            }
            SupportAction::Open(link) => {
                self.dialog = None;
                launcher_host::desktop::open_url(link.url());
            }
        }
    }
}
