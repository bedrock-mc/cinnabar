//! Maps session ownership to the optional desktop Discord service.

use crate::{menu::MenuRuntime, runtime::world::ClientWorld};
use bevy::prelude::*;
use client_ui::ui_runtime::presentation::LoadingStage;
use launcher::menu::settings_options::DISCORD_PRESENCE_OPTION;
use rich_presence::{Presence, State};

#[derive(Resource)]
struct DiscordPresence {
    application_id: u64,
    /// Dropped while the Video setting is off, which closes the pipe and clears the activity.
    presence: Option<Presence>,
}

pub(crate) fn configure(app: &mut App) {
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
            .add_systems(Last, update);
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
    world: Res<ClientWorld>,
    ui: Res<client_ui::ui_runtime::presentation::UiPresentationRuntime>,
    session: Res<crate::session::SessionController>,
) {
    if menu
        .settings_snapshot()
        .0
        .value(DISCORD_PRESENCE_OPTION.name)
        == 0
    {
        if discord.presence.is_some() {
            discord.presence = None;
        }
        return;
    }
    let state = session_state(
        menu.is_connecting() || joining_screen(ui.loading_stage()),
        world.stream.is_some(),
        menu.is_launcher(),
        world.fatal_error.is_some(),
    );
    let application_id = discord.application_id;
    let presence = discord
        .presence
        .get_or_insert_with(|| Presence::start(application_id));
    presence.update(state, session.presence_target());
    // A direct `--address` session has no launcher to join through.
    if let Some(address) = presence.take_join()
        && menu.is_launcher()
        && crate::session::invite_joinable(&address)
        && !already_there(session.presence_target(), &address)
    {
        info!("joining {address} from a Discord invite");
        menu.request_connect(address);
    }
}

fn already_there(target: Option<&rich_presence::Target>, address: &str) -> bool {
    target.and_then(|target| target.join.as_deref()) == Some(address)
}

pub(crate) fn shutdown(app: &mut App) {
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
        };
        assert!(already_there(Some(&target), "gathering/1"));
        assert!(!already_there(Some(&target), "gathering/2"));
        assert!(!already_there(None, "gathering/1"));
    }
}
