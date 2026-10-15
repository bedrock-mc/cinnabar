//! Import controls snapshot current preferences and publish completed worker results.

use super::MenuRuntime;
use launcher::menu::{MenuAction, MenuDialog};
use launcher::minecraft_import::{Action, Phase};
use launcher_host::minecraft_import::{Request, UserState};
use std::sync::Arc;

impl MenuRuntime {
    /// Queues an explicit operation and blocks repeat presses while its worker runs.
    pub(super) fn activate_minecraft_import(&mut self, action: Action) {
        if self.minecraft_import.phase == Phase::Busy {
            return;
        }
        if matches!(
            action,
            Action::Apply | Action::ConfirmApply | Action::ConfirmRevert
        ) && !self.saves.can_save()
        {
            self.message = Some(
                "Saved servers could not be recovered. Restore their file before importing."
                    .to_owned(),
            );
            return;
        }
        if matches!(
            action,
            Action::Apply | Action::ConfirmApply | Action::Revert | Action::ConfirmRevert
        ) && (self.over_world()
            || self.global_resources.busy
            || (matches!(action, Action::Apply | Action::ConfirmApply)
                && self.global_resources.has_pending_changes()))
        {
            let message =
                "Return to the launcher and finish resource pack changes before importing."
                    .to_owned();
            self.message = Some(message.clone());
            Arc::make_mut(&mut self.minecraft_import).body = message;
            return;
        }
        let state = UserState {
            settings: (*self.persistent_settings()).clone(),
            servers: self.servers.to_vec(),
            video: launcher_host::video_settings::SavedVideoSettings {
                fullscreen: self.fullscreen,
                gui_scale_offset: self.gui_scale_offset,
            },
            active: self.global_resources.selection.clone(),
        };
        self.minecraft_import_requests
            .push(Request { action, state });
        self.dialog = Some(MenuDialog::MinecraftImport);
        let snapshot = Arc::make_mut(&mut self.minecraft_import);
        snapshot.phase = Phase::Busy;
        snapshot.body = match action {
            Action::NotNow => "Saving your choice…",
            Action::Close => "Closing preview…",
            Action::Apply => "Reviewing your import…",
            Action::ConfirmApply => "Importing the previewed data…",
            Action::ConfirmRevert => "Restoring the previous preferences…",
            Action::Revert => "Reading the saved backup…",
            Action::Begin | Action::Select(_) => "Preparing the import preview…",
        }
        .to_owned();
        self.focused = 0;
        self.field = None;
    }

    /// Drains explicit user commands for the existing serial resource pack worker.
    pub fn take_minecraft_import_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.minecraft_import_requests)
    }

    /// Dismisses failed close requests locally and reports other requests to a stopped worker.
    pub fn fail_minecraft_import(&mut self, action: Action, message: &str) {
        let snapshot = Arc::make_mut(&mut self.minecraft_import);
        if action == Action::Close {
            snapshot.phase = Phase::Idle;
            snapshot.body.clear();
            snapshot.pack_status.clear();
            self.dialog = None;
        } else {
            snapshot.phase = Phase::Result;
            snapshot.body = message.to_owned();
            self.dialog = Some(MenuDialog::MinecraftImport);
        }
        self.focused = 0;
    }

    /// Keeps runtime pack progress and failures visible in the import result dialog.
    pub fn publish_minecraft_import_pack_status(&mut self, message: &str) {
        if self.minecraft_import.pack_status != message {
            Arc::make_mut(&mut self.minecraft_import).pack_status = message.to_owned();
        }
    }

    /// Applies restored/imported preferences through the existing persistence and runtime adapters.
    pub fn publish_minecraft_import(
        &mut self,
        snapshot: launcher::minecraft_import::Snapshot,
        state: Option<UserState>,
    ) {
        if let Some(mut state) = state {
            state
                .settings
                .set_anti_aliasing_support(self.settings_options.anti_aliasing_support());
            state
                .settings
                .set_render_distance_defaults(self.settings_options.render_distance_defaults());
            for preference in &mut self.session_overrides {
                let index = preference.index();
                let temporary = self
                    .settings_options
                    .option_preference(index)
                    .expect("registered session option");
                *preference = state
                    .settings
                    .option_preference(index)
                    .expect("registered session option");
                state.settings.restore_option_preference(temporary);
            }
            self.language_pending = state.settings.language() != self.settings_options.language();
            self.settings_options = Arc::new(state.settings);
            self.servers.replace(state.servers);
            self.fullscreen = state.video.fullscreen;
            self.fullscreen_change = Some(self.fullscreen);
            self.gui_scale_offset = state.video.gui_scale_offset;
            self.settings_dirty = true;
            self.settings_apply = true;
            self.settings_retry_at = None;
            self.save_servers();
            self.macro_commands =
                super::input::command_macros::macro_editors(&self.settings_options);
        }
        self.dialog = (snapshot.phase != Phase::Idle).then_some(MenuDialog::MinecraftImport);
        self.minecraft_import = Arc::new(snapshot);
        self.focused = 0;
    }

    /// Treats escape and the generic dialog close as persisted refusal for the first offer.
    pub(super) fn dismiss_minecraft_import(&mut self, action: MenuAction) -> bool {
        if self.dialog != Some(MenuDialog::MinecraftImport) || action != MenuAction::DismissDialog {
            return false;
        }
        self.activate_minecraft_import(if self.minecraft_import.phase == Phase::Offered {
            Action::NotNow
        } else {
            Action::Close
        });
        true
    }
}

#[cfg(test)]
mod tests;
