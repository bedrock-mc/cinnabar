//! Account changes wait for the original per-session Core and cancel native session ownership.

use super::*;

/// Runs before menu services can dispatch signout or begin its original account storage job.
pub(crate) fn before_menu_services(mut menu: ResMut<MenuRuntime>, mut session: SessionResources) {
    menu.observe_session_core_exit(session.controller.core.exit_barrier());
    if !menu.account_operation_pending() {
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
