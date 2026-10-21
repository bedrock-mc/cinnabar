//! Maps session ownership to the optional desktop Discord service.

use std::time::Duration;

use crate::menu::MenuRuntime;
use crate::ui_resources::UiRuntimeResource;
use bevy::prelude::*;
use launcher::menu::settings_options::{
    DISCORD_ALLOW_JOINS_OPTION, DISCORD_PARTY_SIZE_OPTION, DISCORD_PRESENCE_OPTION,
    DISCORD_SERVER_DETAILS_OPTION, DISCORD_TIMESTAMP_OPTION,
};
use rich_presence::{Presence, PresenceSettings, State};
use ui::LoadingStage;

#[derive(Resource)]
struct DiscordPresence {
    application_id: u64,
    /// Dropped while the Discord setting is off, which closes the pipe and clears the activity.
    presence: Option<Presence>,
}

pub fn configure(app: &mut App) {
    let value = std::env::var(rich_presence::APPLICATION_ID_ENV);
    let application_id = match &value {
        Ok(value) => rich_presence::application_id(Some(value)),
        Err(std::env::VarError::NotPresent) => rich_presence::application_id(None),
        Err(std::env::VarError::NotUnicode(_)) => Err("application ID is not valid Unicode"),
    };
    match application_id {
        Ok(Some(application_id)) => {
            app.insert_resource(DiscordPresence {
                application_id,
                presence: None,
            })
            .add_systems(
                Last,
                update
                    .in_set(client_state::ShellFrameSet::Presence)
                    .run_if(resource_exists::<client_state::PresenceObservation>),
            )
            .add_systems(
                Update,
                crate::menu::open_join_requests_from_key
                    .after(client_state::ClientFrameSet::SemanticFinalize)
                    .before(client_state::ClientFrameSet::UiPreparation),
            );
        }
        Ok(None) => {}
        Err(error) => warn!("{}: {error}", rich_presence::APPLICATION_ID_ENV),
    }
}

fn session_state(connecting: bool, has_world: bool, launcher: bool, failed: bool) -> State {
    if failed {
        State::Menus
    } else if connecting {
        State::Joining
    } else if has_world {
        State::Playing
    } else if !launcher {
        State::Joining
    } else {
        State::Menus
    }
}

/// The join's loading screens; a dimension change happens inside a live session.
fn joining_screen(stage: Option<LoadingStage>) -> bool {
    matches!(
        stage,
        Some(LoadingStage::Connecting | LoadingStage::BuildingTerrain)
    )
}

fn update(
    mut discord: ResMut<DiscordPresence>,
    mut menu: ResMut<MenuRuntime>,
    mut runtime: ResMut<UiRuntimeResource>,
    time: Res<Time<Real>>,
    world: Res<client_state::PresenceObservation>,
    ui: Res<crate::ui_resources::UiPresentationResource>,
) {
    let now = time.elapsed();
    let settings = menu.settings_snapshot().0;
    if settings.value(DISCORD_PRESENCE_OPTION.id) == 0 {
        // Nothing new arrives while presence is off, so its requests go with it.
        if discord.presence.is_some() {
            discord.presence = None;
            menu.clear_join_requests();
            menu.sync_join_toast(&mut runtime, now);
        }
        return;
    }
    let state = session_state(
        menu.is_connecting() || joining_screen(ui.loading_stage()),
        world.has_world,
        menu.is_launcher(),
        world.failed,
    );
    let presence_settings = PresenceSettings {
        show_server_details: settings.value(DISCORD_SERVER_DETAILS_OPTION.id) != 0,
        show_party_size: settings.value(DISCORD_PARTY_SIZE_OPTION.id) != 0,
        allow_joins: settings.value(DISCORD_ALLOW_JOINS_OPTION.id) != 0,
        show_timestamp: settings.value(DISCORD_TIMESTAMP_OPTION.id) != 0,
    };
    let application_id = discord.application_id;
    let presence = discord
        .presence
        .get_or_insert_with(|| Presence::start(application_id));
    presence.update_with_settings(
        state,
        world.target.as_ref(),
        world.players,
        presence_settings,
    );
    if !presence_settings.allow_joins {
        menu.clear_join_requests();
    }
    relay_join_requests(presence, &mut menu, now);
    menu.sync_join_toast(&mut runtime, now);
    // A direct `--address` session has no launcher to join through.
    if let Some(address) = presence.take_join()
        && menu.is_launcher()
        && crate::presence_target::invite_joinable(&address)
        && !already_there(world.target.as_ref(), &address)
    {
        // The address stays out of the log, which crash reports copy.
        info!("joining a server from a Discord invite");
        menu.request_connect(address);
    }
}

/// Moves Discord's new join requests into the menu and sends the host's answers back.
fn relay_join_requests(presence: &Presence, menu: &mut MenuRuntime, now: Duration) {
    while let Some(request) = presence.take_join_request() {
        menu.push_join_request(request.user_id, request.name, now);
    }
    menu.expire_join_requests(now);
    while let Some((user_id, accept)) = menu.take_join_reply() {
        presence.reply(user_id, accept);
    }
}

fn already_there(target: Option<&rich_presence::Target>, address: &str) -> bool {
    target.and_then(|target| target.join.as_deref()) == Some(address)
}

pub fn shutdown(app: &mut App) {
    drop(app.world_mut().remove_resource::<DiscordPresence>());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_activity_follows_join_play_disconnect_and_direct_startup() {
        assert_eq!(session_state(false, false, true, false), State::Menus);
        assert_eq!(session_state(true, false, true, false), State::Joining);
        assert_eq!(session_state(true, true, true, false), State::Joining);
        assert_eq!(session_state(false, true, true, false), State::Playing);
        assert_eq!(session_state(false, false, true, false), State::Menus);
        assert_eq!(session_state(false, false, false, false), State::Joining);
        assert_eq!(session_state(false, true, false, false), State::Playing);
        assert_eq!(session_state(false, true, false, true), State::Menus);
    }

    #[test]
    fn dimension_changes_stay_in_game() {
        let state = |stage| session_state(joining_screen(stage), true, true, false);
        assert_eq!(state(Some(LoadingStage::ChangingDimension)), State::Playing);
        assert_eq!(state(Some(LoadingStage::BuildingTerrain)), State::Joining);
        assert_eq!(state(Some(LoadingStage::Connecting)), State::Joining);
        assert_eq!(state(None), State::Playing);
    }

    #[test]
    fn an_invite_to_the_current_destination_does_not_reconnect() {
        let target = rich_presence::Target {
            destination: rich_presence::Destination::Experience,
            join: Some("gathering/1".into()),
            badge: None,
            max_players: None,
        };
        assert!(already_there(Some(&target), "gathering/1"));
        assert!(!already_there(Some(&target), "gathering/2"));
        assert!(!already_there(None, "gathering/1"));
    }
}
