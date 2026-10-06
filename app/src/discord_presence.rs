//! Maps session ownership to the optional desktop Discord service.

use crate::{menu::MenuRuntime, runtime::world::ClientWorld};
use bevy::prelude::*;
use rich_presence::{Presence, State};

#[derive(Resource)]
struct DiscordPresence(Presence);

pub(crate) fn configure(app: &mut App) {
    let value = std::env::var(rich_presence::APPLICATION_ID_ENV);
    let application_id = match value {
        Ok(ref value) => rich_presence::application_id(Some(value)),
        Err(std::env::VarError::NotPresent) => rich_presence::application_id(None),
        Err(std::env::VarError::NotUnicode(_)) => Err("application ID is not valid Unicode"),
    };
    match application_id {
        Ok(Some(id)) => {
            app.insert_resource(DiscordPresence(Presence::start(id)))
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

fn update(
    mut presence: ResMut<DiscordPresence>,
    menu: Res<MenuRuntime>,
    world: Res<ClientWorld>,
    ui: Res<client_ui::ui_runtime::presentation::UiPresentationRuntime>,
) {
    presence.0.update(session_state(
        menu.is_connecting() || ui.loading_stage().is_some(),
        world.stream.is_some(),
        menu.is_launcher(),
        world.fatal_error.is_some(),
    ));
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
}
