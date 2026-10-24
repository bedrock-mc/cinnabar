//! Account changes cancel the original native session ownership before storage work.

use super::*;

/// Runs before menu services can dispatch signout or begin its original account storage job.
pub(crate) fn before_menu_services(
    menu: ResMut<MenuRuntime>,
    mut session: SessionResources,
    #[cfg(feature = "native-networking")] launcher: Option<
        ResMut<client_shell::menu::LauncherServices>,
    >,
) {
    #[cfg(feature = "native-networking")]
    if let Some(mut launcher) = launcher {
        launcher.set_session_workers(session.controller.network_workers.clone());
    }
    if !menu.account_change_pending() {
        session.controller.account_retirement_started = false;
        return;
    }
    if session.controller.account_retirement_started {
        return;
    }
    session.leave();
    session.controller.trust = None;
    #[cfg(feature = "native-networking")]
    {
        session.controller.native_account = None;
    }
    session.controller.account_retirement_started = true;
}

#[cfg(all(test, feature = "native-networking", not(target_arch = "wasm32")))]
mod tests;
