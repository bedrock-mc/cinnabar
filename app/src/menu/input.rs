use bevy::{
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        touch::Touches,
    },
    prelude::{
        AppExit, ButtonInput, KeyCode, MessageReader, MessageWriter, MouseButton, Query, Res,
        ResMut, Single, With,
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use ui::UiPoint;

use crate::{
    runtime::{
        network::{NetworkConfig, NetworkHandle, ResourcePackAdmissionState},
        world::ClientWorld,
    },
    session_cleanup::SessionDirectoryGuard,
    ui_runtime::{UiRuntime, presentation::UiPresentationRuntime},
};

use super::{CoreProcessGuard, MenuRuntime, MenuScreen, spawn_core_for_address, wait_for_core};

/// Spawns a fresh core and network session for one address, replacing any
/// previous session.
///
/// This is the single replacement-handoff path shared by user joins and
/// automatic server-transfer follows: identity-checked session directory,
/// bounded core start wait, old-network shutdown, and a fresh session
/// generation for every attempt.
#[allow(clippy::too_many_arguments)]
fn attempt_connect(
    commands: &mut bevy::prelude::Commands,
    menu: &mut MenuRuntime,
    guard: &mut CoreProcessGuard,
    network: &mut NetworkHandle,
    client_blob_cache: &crate::app::ClientBlobCacheOwner,
    resource_packs: &mut ResourcePackAdmissionState,
    runtime: &mut UiRuntime,
    client_world: &mut ClientWorld,
    address: String,
    auth_cache: Option<std::path::PathBuf>,
) {
    let generation = menu.next_session_generation();
    resource_packs.begin_generation(generation);
    // Namespaced by process id like the `--address` path: a bare
    // generation counter restarts at the same value every launch, so a
    // previous run's directory would be reused for this session.
    let socket_dir = menu
        .layout
        .connect_socket_dir(std::process::id(), generation);
    // The guard owns the directory across every teardown path below;
    // an identity conflict fails this connect loudly instead of
    // reusing another session's directory.
    let session_directory = match SessionDirectoryGuard::bind(socket_dir.clone()) {
        Ok(directory) => directory,
        Err(error) => {
            menu.message = Some(format!("Could not start {address}: {error}"));
            menu.connecting = false;
            return;
        }
    };
    if let Err(error) = spawn_core_for_address(
        &menu.layout,
        &socket_dir,
        &address,
        auth_cache.as_deref(),
        // Advertise upstream cache capability exactly because this same
        // connect hands the verified blob cache to the new network
        // session below; that ownership is what makes the client answer
        // LoginSuccess with cache-enabled status downstream.
        client_blob_cache.enables_upstream_client_cache(),
    )
    .map_err(std::io::Error::other)
    .and_then(|child| {
        guard.replace(child);
        wait_for_core(&socket_dir).map_err(std::io::Error::other)
    }) {
        drop(session_directory);
        menu.message = Some(format!("Could not start {address}: {error}"));
        menu.connecting = false;
        return;
    }
    menu.bind_session_directory(session_directory);
    network.shutdown();
    match crate::runtime::network::spawn_network(NetworkConfig {
        session_generation: generation,
        socket_dir,
        display_name: menu.display_name.clone(),
        client_blob_cache: client_blob_cache.cache(),
    }) {
        Ok(replacement) => {
            runtime.begin_session(generation);
            client_world.stream = None;
            client_world.pending_surface_spawn = None;
            client_world.fatal_error = None;
            client_world.transfer_notice = None;
            commands.insert_resource(replacement.movement_ticker());
            commands.insert_resource(replacement);
            menu.mark_connecting();
        }
        Err(error) => {
            menu.message = Some(format!("Could not connect: {error}"));
            menu.connecting = false;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_input(
    mut keyboard_messages: MessageReader<KeyboardInput>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    gamepads: Query<&Gamepad>,
    presentation: Res<UiPresentationRuntime>,
    mut menu: ResMut<MenuRuntime>,
) {
    let (window, mut cursor) = window.into_inner();
    menu.pressed = None;
    if !window.focused {
        menu.pointer_down = false;
        return;
    }
    if !menu.is_visible() {
        menu.hovered = None;
        menu.pointer_down = false;
        if keys.just_pressed(KeyCode::Escape) {
            menu.open_pause();
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
            keys.reset_all();
        }
        return;
    }

    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    menu.hovered = window
        .cursor_position()
        .and_then(|position| UiPoint::new(position.x, position.y).ok())
        .and_then(|position| presentation.hit_test_menu(position));
    let pointer_pressed = mouse_buttons.pressed(MouseButton::Left);
    let pointer_just_pressed = pointer_pressed && !menu.pointer_down;
    menu.pointer_down = pointer_pressed;
    if pointer_just_pressed && let Some(action) = menu.hovered {
        menu.activate(action);
    }
    for touch in touches.iter_just_pressed() {
        let position = touch.position();
        if let Ok(position) = UiPoint::new(position.x, position.y)
            && let Some(action) = presentation.hit_test_menu(position)
        {
            menu.activate(action);
        }
    }
    for gamepad in &gamepads {
        if gamepad.just_pressed(GamepadButton::DPadUp)
            || gamepad.just_pressed(GamepadButton::DPadLeft)
        {
            menu.move_focus(-1);
        }
        if gamepad.just_pressed(GamepadButton::DPadDown)
            || gamepad.just_pressed(GamepadButton::DPadRight)
        {
            menu.move_focus(1);
        }
        if gamepad.just_pressed(GamepadButton::South) {
            menu.activate_focused();
        }
        if gamepad.just_pressed(GamepadButton::East) {
            menu.go_back_from_input();
        }
    }
    for input in keyboard_messages.read() {
        if input.state != ButtonState::Pressed {
            continue;
        }
        match input.key_code {
            KeyCode::Escape => menu.go_back_from_input(),
            KeyCode::ArrowUp | KeyCode::ArrowLeft => menu.move_focus(-1),
            KeyCode::ArrowDown | KeyCode::ArrowRight | KeyCode::Tab => menu.move_focus(1),
            KeyCode::Enter | KeyCode::NumpadEnter => menu.activate_focused(),
            KeyCode::Backspace if menu.field.is_some() => menu.backspace_text(),
            _ if menu.field.is_some() => {
                if let Some(text) = input.text.as_deref() {
                    menu.edit_text(text);
                }
            }
            _ => {}
        }
    }
    // The menu owns the pointer and keyboard for this frame. This also keeps
    // the camera's recapture-on-click path from turning a menu click into a
    // gameplay attack or mouse grab.
    keys.reset_all();
    mouse_buttons.reset_all();
}

impl MenuRuntime {
    fn go_back_from_input(&mut self) {
        self.go_back();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_connection(
    mut commands: bevy::prelude::Commands,
    mut exits: MessageWriter<AppExit>,
    mut menu: ResMut<MenuRuntime>,
    mut guard: ResMut<CoreProcessGuard>,
    mut network: ResMut<NetworkHandle>,
    client_blob_cache: Res<crate::app::ClientBlobCacheOwner>,
    mut resource_packs: ResMut<ResourcePackAdmissionState>,
    mut runtime: ResMut<UiRuntime>,
    mut client_world: ResMut<ClientWorld>,
) {
    menu.poll_catalog();
    if menu.is_connecting() && client_world.stream.is_some() {
        menu.mark_connected();
    }
    if let Some(pending) = menu.take_pending_connect() {
        attempt_connect(
            &mut commands,
            &mut menu,
            &mut guard,
            &mut network,
            &client_blob_cache,
            &mut resource_packs,
            &mut runtime,
            &mut client_world,
            pending.address,
            pending.auth_cache,
        );
    }
    if menu.take_disconnect_request() {
        network.shutdown();
        guard.stop();
        // The core is gone, so its endpoint artifact is no longer open and
        // the identity-checked removal can proceed.
        menu.release_session_directory();
        let generation = menu.next_session_generation();
        resource_packs.begin_generation(generation);
        runtime.begin_session(generation);
        client_world.stream = None;
        menu.visible = true;
        menu.screen = MenuScreen::Home;
        menu.connecting = false;
    }
    if menu.take_exit_request() {
        network.shutdown();
        guard.stop();
        menu.release_session_directory();
        exits.write(AppExit::Success);
    }
}

/// Returns a failed launcher session to the menu instead of ending the process.
///
/// Runs late in the frame: the failure is recorded while network events are
/// drained, which is after the menu's own systems, so recovery has to happen
/// between that and the systems that exit on a fatal error.
pub(crate) fn recover_menu_session_failure(
    mut menu: ResMut<MenuRuntime>,
    mut guard: ResMut<CoreProcessGuard>,
    mut network: ResMut<NetworkHandle>,
    mut resource_packs: ResMut<crate::runtime::network::ResourcePackAdmissionState>,
    mut runtime: ResMut<crate::ui_runtime::UiRuntime>,
    mut client_world: ResMut<crate::runtime::world::ClientWorld>,
) {
    let Some(error) = client_world.fatal_error.clone() else {
        return;
    };
    if !menu.absorb_session_failure(&error) {
        return;
    }
    network.shutdown();
    guard.stop();
    menu.release_session_directory();
    let generation = menu.next_session_generation();
    resource_packs.begin_generation(generation);
    runtime.begin_session(generation);
    client_world.stream = None;
    client_world.pending_surface_spawn = None;
    client_world.fatal_error = None;
}

/// Consumes a latched server-transfer notice and performs the bounded
/// replacement handoff.
///
/// Runs late in the frame, after the network drain latched the notice and
/// before fatal-error exits: launcher sessions tear the old session down and
/// rejoin the transferred target through the exact user-join machinery,
/// while `--address` runs keep their historical single-session behavior and
/// end with the transfer named explicitly. The automatic chain is bounded so
/// a transfer loop ends in a visible menu state instead of reconnecting
/// forever.
#[allow(clippy::too_many_arguments)]
pub(crate) fn follow_server_transfer(
    mut commands: bevy::prelude::Commands,
    mut menu: ResMut<MenuRuntime>,
    mut guard: ResMut<CoreProcessGuard>,
    mut network: ResMut<NetworkHandle>,
    client_blob_cache: Res<crate::app::ClientBlobCacheOwner>,
    mut resource_packs: ResMut<ResourcePackAdmissionState>,
    mut runtime: ResMut<UiRuntime>,
    mut client_world: ResMut<ClientWorld>,
) {
    let Some(notice) = client_world.transfer_notice.take() else {
        return;
    };
    let target = notice.host.clone();
    if !menu.is_launcher() {
        // No launcher exists to re-enter, so the one-session run ends with
        // the server-directed move named explicitly instead of followed.
        crate::runtime::shutdown::record_fatal_error(
            &mut client_world.fatal_error,
            format!(
                "server transferred to {}",
                crate::menu::format_transfer_address(&notice.host, notice.port)
            ),
        );
        return;
    }
    let Some((address, auth_cache)) = menu.transfer_handoff_target(&notice.host, notice.port)
    else {
        end_transfer_without_follow(
            &mut menu,
            &mut guard,
            &mut network,
            &mut resource_packs,
            &mut runtime,
            &mut client_world,
            format!("server sent an unusable transfer target ({target})"),
        );
        return;
    };
    if !menu.consume_transfer_chain_hop() {
        end_transfer_without_follow(
            &mut menu,
            &mut guard,
            &mut network,
            &mut resource_packs,
            &mut runtime,
            &mut client_world,
            format!(
                "the transfer chain limit was reached at {}",
                crate::menu::format_transfer_address(&notice.host, notice.port)
            ),
        );
        return;
    }
    // Old-session teardown first: stop the network pump and the old core,
    // release the old session directory, then rejoin through the same
    // machinery as an ordinary user join (fresh generation, fresh core,
    // fresh session directory, shared verified blob cache).
    network.shutdown();
    guard.stop();
    menu.release_session_directory();
    attempt_connect(
        &mut commands,
        &mut menu,
        &mut guard,
        &mut network,
        &client_blob_cache,
        &mut resource_packs,
        &mut runtime,
        &mut client_world,
        address.clone(),
        auth_cache,
    );
    if menu.is_connecting() {
        menu.message = Some(format!("Transferring to {address}…"));
    }
}

/// Ends a transferred session in the explicit cannot-follow state: the old
/// session is torn down exactly like a failure recovery and the menu names
/// what happened instead of reconnecting again.
#[allow(clippy::too_many_arguments)]
fn end_transfer_without_follow(
    menu: &mut MenuRuntime,
    guard: &mut CoreProcessGuard,
    network: &mut NetworkHandle,
    resource_packs: &mut ResourcePackAdmissionState,
    runtime: &mut UiRuntime,
    client_world: &mut ClientWorld,
    reason: String,
) {
    network.shutdown();
    guard.stop();
    menu.release_session_directory();
    let generation = menu.next_session_generation();
    resource_packs.begin_generation(generation);
    runtime.begin_session(generation);
    client_world.stream = None;
    client_world.pending_surface_spawn = None;
    client_world.fatal_error = None;
    client_world.transfer_notice = None;
    menu.absorb_session_failure(&reason);
}

#[cfg(test)]
mod session_failure_message_tests {
    use super::super::MenuRuntime;

    const KICK: &str =
        "server disconnected: We've detected movement cheats (network read failed: closed)";

    #[test]
    fn launcher_renders_the_server_reason_in_the_menu_message() {
        let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
        assert!(menu.absorb_session_failure(KICK));
        assert_eq!(
            menu.view().message.as_deref(),
            Some(
                "Disconnected: server disconnected: We've detected movement cheats (network read failed: closed)"
            )
        );
    }

    #[test]
    fn launcher_falls_back_to_the_transport_failure_without_a_reason() {
        let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
        assert!(menu.absorb_session_failure("network session failed: closed"));
        assert_eq!(
            menu.view().message.as_deref(),
            Some("Disconnected: network session failed: closed")
        );
    }
}

#[cfg(test)]
mod transfer_follow_tests {
    use super::super::{MAX_TRANSFER_CHAIN_HOPS, MenuRuntime, format_transfer_address};

    #[test]
    fn transfer_addresses_bracket_ipv6_and_leave_ordinary_hosts_untouched() {
        assert_eq!(
            format_transfer_address("game.example.net", 19133),
            "game.example.net:19133"
        );
        assert_eq!(format_transfer_address("::1", 19132), "[::1]:19132");
        assert_eq!(
            format_transfer_address("2001:db8::10", 25565),
            "[2001:db8::10]:25565"
        );
    }

    #[test]
    fn handoff_targets_are_well_formed_without_any_host_allowlist() {
        let menu = MenuRuntime::new(true, 2, "Player".to_owned());

        let (address, _) = menu
            .transfer_handoff_target(" game.example.net ", 19133)
            .expect("a trimmed well-formed host is a valid target");
        assert_eq!(address, "game.example.net:19133");

        let (address, _) = menu
            .transfer_handoff_target("minigames.other-host.example", 19321)
            .expect("cross-host transfers are legitimate vanilla behavior");
        assert_eq!(address, "minigames.other-host.example:19321");

        assert!(menu.transfer_handoff_target("", 19132).is_none());
        assert!(menu.transfer_handoff_target("   ", 19132).is_none());
    }

    #[test]
    fn the_automatic_transfer_chain_is_bounded() {
        let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());

        // A user-initiated join always starts a fresh bounded chain.
        menu.begin_fresh_transfer_chain();
        for _ in 0..MAX_TRANSFER_CHAIN_HOPS {
            assert!(menu.consume_transfer_chain_hop());
        }
        assert!(
            !menu.consume_transfer_chain_hop(),
            "an exhausted chain must refuse to follow again"
        );

        // And another user join renews it after exhaustion.
        menu.begin_fresh_transfer_chain();
        assert!(menu.consume_transfer_chain_hop());
    }
}
