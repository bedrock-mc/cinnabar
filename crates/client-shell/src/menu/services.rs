//! Launcher services driven after the app supplies its session observations.
use super::*;
#[cfg(feature = "native-networking")]
use launcher::menu::settings_options::CINNABAR_VISIBILITY_OPTION;
impl MenuRuntime {
    /// Advances native account services and local worlds using the current session state.
    #[allow(clippy::too_many_arguments)]
    pub fn drive_account_services(
        &mut self,
        #[cfg(feature = "native-networking")] commands: &mut Commands,
        #[cfg(feature = "native-networking")] player_runtime: &client_state::PlayerRuntime,
        in_session: bool,
        #[cfg(feature = "native-networking")] launcher: Option<ResMut<LauncherServices>>,
        #[cfg(feature = "native-networking")] launcher_account: Option<
            ResMut<launcher_account::LauncherAccount>,
        >,
        local_worlds: Option<&mut crate::local_worlds::LocalWorlds>,
    ) {
        #[cfg(feature = "native-networking")]
        let mut local_worlds = local_worlds;
        let idle = !self.joining_past_sign_in()
            && (!in_session || !(self.is_launcher() || self.local_world_joined));
        #[cfg(not(feature = "native-networking"))]
        {
            self.sign_out_locally();
            if idle && self.accounts.operation.is_some() && self.auth_cleanup_complete() {
                self.start_account_operation();
            }
        }
        #[cfg(feature = "native-networking")]
        if let Some(mut slot) = launcher {
            slot.drive(commands, self, idle, local_worlds.as_deref_mut());
        }
        if std::mem::take(&mut self.accounts.skip_control) {
            return;
        }
        #[cfg(feature = "native-networking")]
        match launcher_account {
            Some(mut account) => {
                #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                account.0.set_native_host_intent(
                    local_worlds
                        .as_deref()
                        .and_then(|worlds| worlds.native_host_intent()),
                );
                let options = self.settings_snapshot().0;
                account.set_cinnabar_api(
                    options.update_preferences().channel,
                    options.value(CINNABAR_VISIBILITY_OPTION.id) != 0,
                );
                account.set_xbox_presence(xbox_presence::state(
                    in_session,
                    player_runtime.facts.world_default_game_mode(),
                    self.presence_address(),
                    self.presence_is_featured(),
                ));
                self.sync_account_control(&mut account.0);
            }
            None => {
                self.sign_out_locally();
            }
        }
        #[cfg(feature = "native-networking")]
        self.poll_native_catalog(std::time::Instant::now());
        if let Some(worlds) = local_worlds {
            self.sync_local_worlds(worlds, in_session);
        }
    }
}

/// Sends roster changes to the Cinnabar lookup worker and badges the players it reports.
#[cfg(feature = "native-networking")]
pub fn sync_cinnabar_players(
    account: &launcher_account::LauncherAccount,
    runtime: &mut client_ui::ui_runtime::UiRuntime,
) {
    account.0.set_cinnabar_roster(
        runtime.session_id(),
        runtime.player_roster.revision(),
        runtime.player_roster.lookup_ids(),
    );
    runtime
        .player_roster
        .set_cinnabar(account.0.cinnabar_players());
}
