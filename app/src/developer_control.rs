//! Bevy glue for the developer control endpoint: drains authenticated commands each frame
//! and applies them through the same seams real input, chat and joins use.

mod actors;
mod camera;
mod capture;
mod input;
mod scoreboards;
mod state;

use std::path::PathBuf;

use bevy::{
    prelude::*,
    window::{PresentMode, WindowResolution},
};
use developer_control::{
    ENDPOINT_ENV, HIDDEN_WINDOW_ENV, WINDOW_SIZE_ENV,
    protocol::Command,
    server::{ControlServer, Pending},
};
use serde_json::{Value, json};

#[derive(Resource)]
struct Control {
    server: ControlServer,
}

/// Applies `CINNABAR_WINDOW_SIZE` and `CINNABAR_HIDDEN_WINDOW` to the primary window.
pub(crate) fn primary_window(mut window: Window) -> Window {
    if let Some(size) = std::env::var(WINDOW_SIZE_ENV)
        .ok()
        .and_then(|value| developer_control::parse_size(&value))
    {
        window.resolution = WindowResolution::new(size[0], size[1]).with_scale_factor_override(1.0);
        window.resizable = false;
    }
    if std::env::var_os(HIDDEN_WINDOW_ENV).is_some_and(|value| value == "1") {
        window.visible = false;
        // A hidden surface never reaches the display, so vsync would only throttle drawables.
        window.present_mode = PresentMode::AutoNoVsync;
    }
    window
}

/// Starts the endpoint when `CINNABAR_DEVELOPER_CONTROL` names its file.
pub(crate) fn configure(app: &mut App) {
    let Some(path) = std::env::var_os(ENDPOINT_ENV).map(PathBuf::from) else {
        return;
    };
    let server = match ControlServer::start(&path) {
        Ok(server) => server,
        Err(error) => {
            eprintln!(
                "developer control disabled: cannot publish {}: {error}",
                path.display()
            );
            return;
        }
    };
    eprintln!("developer control listening; endpoint {}", path.display());
    app.insert_resource(Control { server });
    input::configure(app);
    camera::configure(app);
    capture::configure(app);
    state::configure(app);
    app.add_systems(First, dispatch);
}

fn dispatch(world: &mut World) {
    let pending: Vec<Pending> = world.resource::<Control>().server.drain().collect();
    for Pending { command, reply } in pending {
        let outcome = match command {
            Command::Connect { address } => connect(world, address),
            Command::Disconnect => disconnect(world),
            Command::Input(command) => input::apply(world, &command),
            Command::Chat { text } => chat(world, &text),
            Command::CameraPath(path) => camera::start(world, path),
            Command::CameraRelease => camera::release(world),
            Command::State => Ok(state::snapshot(world)),
            Command::WaitFor {
                condition,
                timeout_ms,
            } => {
                state::wait(world, condition, timeout_ms, reply);
                continue;
            }
            Command::Screenshot { path } => {
                capture::screenshot(world, path, reply);
                continue;
            }
            Command::RecordStart(settings) => capture::start(world, &settings),
            Command::RecordStop => {
                capture::stop(world, reply);
                continue;
            }
            Command::Quit => {
                world.write_message(AppExit::Success);
                Ok(json!({ "quitting": true }))
            }
        };
        reply.send(outcome);
    }
}

fn connect(world: &mut World, address: String) -> Result<Value, String> {
    let mut menu = world
        .get_resource_mut::<crate::menu::MenuRuntime>()
        .ok_or("the launcher menu is unavailable")?;
    menu.request_connect(address.clone());
    if !menu.is_connecting() {
        return Err("the launcher refused the join (account change pending?)".into());
    }
    Ok(json!({ "joining": address }))
}

fn disconnect(world: &mut World) -> Result<Value, String> {
    let mut menu = world
        .get_resource_mut::<crate::menu::MenuRuntime>()
        .ok_or("the launcher menu is unavailable")?;
    menu.activate(crate::menu::MenuAction::PauseDisconnect);
    Ok(json!({ "disconnecting": true }))
}

/// Queues the line as Enter would, without touching any draft; a leading `/` makes it a command.
fn chat(world: &mut World, text: &str) -> Result<Value, String> {
    let now_millis = world
        .resource::<Time<Real>>()
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    world
        .resource_mut::<client_ui::ui_runtime::UiRuntime>()
        .queue_chat_message(text, now_millis)
        .map_err(|error| format!("{error:?}"))?;
    Ok(json!({ "sent": text }))
}

#[cfg(test)]
mod tests {
    use bevy::{prelude::*, time::TimePlugin};
    use client_ui::ui_runtime::UiRuntime;

    #[test]
    fn chat_sends_exactly_the_text_and_leaves_the_draft_alone() {
        let mut app = App::new();
        app.add_plugins(TimePlugin)
            .insert_resource(UiRuntime::new(1));
        app.update();
        let world = app.world_mut();
        world
            .resource_mut::<UiRuntime>()
            .insert_chat_text("half-typed ")
            .unwrap();
        super::chat(world, "/showcase souls").unwrap();
        let ui = world.resource::<UiRuntime>();
        let sent: Vec<&str> = ui
            .pending_chat_sends()
            .iter()
            .map(|request| &*request.message)
            .collect();
        assert_eq!(sent, ["/showcase souls"]);
        assert_eq!(ui.chat_editor().as_str(), "half-typed ");
        assert!(super::chat(world, "").is_err());
        assert_eq!(
            world.resource::<UiRuntime>().chat_editor().as_str(),
            "half-typed ",
            "a refused send leaves no editor changes behind"
        );
    }
}
