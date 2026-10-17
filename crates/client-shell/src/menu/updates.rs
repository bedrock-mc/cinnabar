//! Local presentation state for verified release notices.

use super::{MenuDialog, MenuRuntime, MenuScreen};
use launcher::menu::updates::Ready;

impl MenuRuntime {
    /// Opens a required release once when the home screen can accept a popup.
    pub fn poll_updates(&mut self) {
        self.present_required_update(launcher_host::lifecycle::update::view().ready);
    }

    /// Keeps optional releases quiet and waits for other home-screen flows to finish.
    fn present_required_update(&mut self, ready: Option<Ready>) {
        let Some(ready) = ready else {
            return;
        };
        if !ready.protocol_required
            || self.update_notice_seen.as_ref() == Some(&ready)
            || self.screen != MenuScreen::Home
            || self.over_world()
            || self.dialog.is_some()
            || self.is_connecting()
            || self.disconnect_message.is_some()
            || self.sign_in_focus().is_some()
            || self.realm_membership.state.is_some()
            || self.join_request_prompted()
        {
            return;
        }
        self.update_notice_seen = Some(ready);
        self.dialog = Some(MenuDialog::UpdateNotice);
        self.focused = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::MenuAction;

    #[test]
    fn optional_release_stays_quiet_and_required_release_opens_only_once() {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        let mut ready = Ready {
            version: "2.0.0".into(),
            writable: true,
            protocol_required: false,
        };
        menu.present_required_update(Some(ready.clone()));
        assert_eq!(menu.dialog, None);
        ready.protocol_required = true;
        menu.present_required_update(Some(ready.clone()));
        assert_eq!(menu.dialog, Some(MenuDialog::UpdateNotice));
        menu.activate(MenuAction::DismissDialog);
        menu.present_required_update(Some(ready));
        assert_eq!(menu.dialog, None);
    }

    #[test]
    fn required_release_waits_for_an_existing_dialog() {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        let ready = Ready {
            version: "2.0.0".into(),
            writable: false,
            protocol_required: true,
        };
        menu.dialog = Some(MenuDialog::Exit);
        menu.present_required_update(Some(ready.clone()));
        assert_eq!(menu.dialog, Some(MenuDialog::Exit));
        menu.dialog = None;
        menu.present_required_update(Some(ready));
        assert_eq!(menu.dialog, Some(MenuDialog::UpdateNotice));
    }
}
