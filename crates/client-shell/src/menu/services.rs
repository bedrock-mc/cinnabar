//! Launcher services driven after the app supplies its session observations.
use super::*;
use launcher::menu::settings_options::CINNABAR_VISIBILITY_OPTION;
impl MenuRuntime {
    /// Advances the account core and local worlds using the current session state.
    #[allow(clippy::too_many_arguments)]
    pub fn drive_account_services(
        &mut self,
        commands: &mut Commands,
        client_cache_enabled: bool,
        player_runtime: &client_state::PlayerRuntime,
        in_session: bool,
        launcher: Option<ResMut<LauncherCoreSlot>>,
        launcher_account: Option<ResMut<launcher_account::LauncherAccount>>,
        mut local_worlds: Option<&mut crate::local_worlds::LocalWorlds>,
    ) {
        if let Some(mut slot) = launcher {
            // Remote direct sessions have a separate game core. Local worlds use the
            // account core, so sign-in must not restart it during local play.
            let idle = launcher_core::account_core_idle(
                self.is_launcher(),
                self.joining_past_sign_in(),
                in_session,
                self.local_world_joined,
            );
            slot.drive(
                commands,
                self,
                idle,
                client_cache_enabled,
                local_worlds.as_deref_mut(),
                std::time::Instant::now(),
            );
            // A held join waits for this core only while it may still start for that join.
            self.account_core = (idle && self.is_launcher()).then(|| slot.account_core());
        } else {
            self.account_core = None;
        }
        if std::mem::take(&mut self.accounts.skip_control) {
            self.forget_launcher_trust();
            return;
        }
        match launcher_account {
            Some(mut account) => {
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
                self.forget_launcher_trust();
                self.sign_out_locally();
            }
        }
        if let Some(worlds) = local_worlds {
            self.sync_local_worlds(worlds, in_session);
        }
    }
}

/// Sends roster changes to the Cinnabar lookup worker and badges the players it reports.
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
