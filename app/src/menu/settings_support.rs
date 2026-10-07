//! Host support-link actions.
use super::{MenuDialog, MenuRuntime};
pub(crate) use launcher::menu::settings_support::*;

impl MenuRuntime {
    /// Opens the authored confirmation or launches its fixed browser destination.
    pub(super) fn activate_support(&mut self, action: SupportAction) {
        match action {
            SupportAction::Dialog(dialog) => {
                self.dialog = Some(MenuDialog::SettingsSupport(dialog))
            }
            SupportAction::Open(link) => {
                self.dialog = None;
                crate::desktop::open_url(link.url());
            }
        }
    }
}
