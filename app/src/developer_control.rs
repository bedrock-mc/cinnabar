//! Bevy glue for the developer control endpoint: drains authenticated commands each frame
//! and applies them through the same seams real input, chat and joins use.

mod actors;
mod camera;
mod cape;
mod capture;
mod input;
#[cfg(target_os = "macos")]
mod macos;
mod scoreboards;
mod state;

use std::path::PathBuf;

use bevy::{prelude::*, window::WindowResolution};
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
pub(crate) fn primary_window(window: Window) -> Window {
    let size = std::env::var(WINDOW_SIZE_ENV)
        .ok()
        .and_then(|value| developer_control::parse_size(&value));
    window_options(window, size, hidden_window_requested())
}

/// Only the explicit developer switch suppresses native application activation.
pub(crate) fn hidden_window_requested() -> bool {
    std::env::var_os(HIDDEN_WINDOW_ENV).is_some_and(|value| value == "1")
}

/// Installs native hidden-mode policy before any window backend creates its application.
pub(crate) fn prepare_native_application(connection_requested: bool) -> anyhow::Result<()> {
    if let Some(value) = std::env::var_os(developer_control::SIGN_IN_FIXTURE_ENV) {
        anyhow::ensure!(
            hidden_window_requested()
                && std::env::var_os(ENDPOINT_ENV).is_some()
                && !connection_requested,
            "sign-in fixtures require a hidden developer-control client without a world connection"
        );
        let state = value
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid sign-in fixture state"))?;
        serde_json::from_value::<developer_control::protocol::SignInFixtureState>(Value::String(
            state,
        ))
        .map_err(|_| anyhow::anyhow!("invalid sign-in fixture state"))?;
    }
    #[cfg(target_os = "macos")]
    if hidden_window_requested() {
        macos::prepare_hidden_application()?;
    }
    Ok(())
}

/// Hidden developer surfaces never request native focus; presentation policy owns pacing.
fn window_options(mut window: Window, size: Option<[u32; 2]>, hidden: bool) -> Window {
    if let Some(size) = size {
        window.resolution = WindowResolution::new(size[0], size[1]).with_scale_factor_override(1.0);
        window.resizable = false;
    }
    if hidden {
        window.visible = false;
        window.focused = false;
    }
    window
}

/// Starts the endpoint when `CINNABAR_DEVELOPER_CONTROL` names its file.
pub(crate) fn configure(app: &mut App) {
    #[cfg(target_os = "macos")]
    if hidden_window_requested() {
        macos::verify_after_launch(app);
    }
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
            Command::ImportSkin { path } => {
                if let Some(mut menu) = world.get_resource_mut::<crate::menu::MenuRuntime>() {
                    menu.import_skin_path(path);
                    Ok(json!({ "importing": true }))
                } else {
                    Err("the launcher menu is unavailable".into())
                }
            }
            Command::ImportCape { path } => {
                if let Some(mut menu) = world.get_resource_mut::<crate::menu::MenuRuntime>() {
                    menu.import_cape_path(path);
                    Ok(json!({ "importing": true }))
                } else {
                    Err("the launcher menu is unavailable".into())
                }
            }
            Command::Chat { text } => chat(world, &text),
            Command::TestCape { enabled } => cape::apply(world, enabled),
            Command::SignInFixture { state } => world
                .get_resource_mut::<crate::menu::MenuRuntime>()
                .ok_or_else(|| "the launcher menu is unavailable".to_owned())
                .and_then(|mut menu| menu.apply_sign_in_fixture(state))
                .map(|()| json!({ "fixture": state })),
            Command::TestAccounts { enabled } => {
                if let Some(mut menu) = world.get_resource_mut::<crate::menu::MenuRuntime>() {
                    if menu.set_presentation_accounts(enabled) {
                        Ok(json!({ "test_accounts": enabled }))
                    } else {
                        Err(
                            "test_accounts needs a signed-out install with no sign-in under way"
                                .into(),
                        )
                    }
                } else {
                    Err("the launcher menu is unavailable".into())
                }
            }
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
    menu.activate(launcher::menu::MenuAction::PauseDisconnect);
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
    fn hidden_surface_starts_without_native_focus() {
        let window = super::window_options(Window::default(), Some([1920, 1080]), true);
        assert!(!window.visible);
        assert!(!window.focused);
        assert!(!window.resizable);
        assert_eq!(window.resolution.physical_size(), UVec2::new(1920, 1080));
        assert_eq!(window.resolution.scale_factor(), 1.0);
    }

    #[test]
    fn normal_surface_retains_its_native_window_policy() {
        let original = Window::default();
        let window = super::window_options(original.clone(), None, false);
        assert_eq!(window.visible, original.visible);
        assert_eq!(window.focused, original.focused);
        assert_eq!(window.present_mode, original.present_mode);
        assert_eq!(window.resizable, original.resizable);
    }

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
