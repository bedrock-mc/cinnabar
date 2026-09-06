use bevy::{
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        touch::Touches,
    },
    prelude::{
        AppExit, ButtonInput, KeyCode, Local, MessageReader, MessageWriter, MouseButton, Query,
        Res, ResMut, Resource, Single, With,
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use ui::{ChatClipboard, UiPoint};

use crate::{
    runtime::{
        network::{NetworkConfig, NetworkHandle, ResourcePackAdmissionState},
        world::ClientWorld,
    },
    session_cleanup::SessionDirectoryGuard,
    ui_runtime::{PlatformClipboard, UiRuntime, presentation::UiPresentationRuntime},
};

use super::{
    CoreProcessGuard, MAX_SERVER_ADDRESS_BYTES, MAX_SERVER_NAME_BYTES, MenuField, MenuRuntime,
    spawn_core_for_address, wait_for_core,
};

#[derive(Resource)]
pub(crate) struct MenuClipboard(
    Box<dyn FnMut(usize) -> Option<String> + Send + Sync + 'static>,
    Box<dyn FnMut(String) + Send + Sync + 'static>,
);

impl MenuClipboard {
    pub(crate) fn with_access(
        reader: impl FnMut(usize) -> Option<String> + Send + Sync + 'static,
        writer: impl FnMut(String) + Send + Sync + 'static,
    ) -> Self {
        Self(Box::new(reader), Box::new(writer))
    }

    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Option<String> {
        (self.0)(maximum_bytes)
    }

    fn write_text(&mut self, text: String) {
        (self.1)(text);
    }
}

impl Default for MenuClipboard {
    fn default() -> Self {
        let mut reader = PlatformClipboard;
        let mut writer = PlatformClipboard;
        Self::with_access(
            move |maximum_bytes| {
                reader
                    .read_text_bounded(maximum_bytes)
                    .ok()
                    .flatten()
                    .map(|text| text.to_string())
            },
            move |text| {
                let _ = writer.write_text(text);
            },
        )
    }
}

#[derive(Default)]
pub(crate) struct MenuModifiers(u8);

impl MenuModifiers {
    const CONTROL_LEFT: u8 = 1 << 0;
    const CONTROL_RIGHT: u8 = 1 << 1;
    const SUPER_LEFT: u8 = 1 << 2;
    const SUPER_RIGHT: u8 = 1 << 3;
    const ALT_LEFT: u8 = 1 << 4;
    const ALT_RIGHT: u8 = 1 << 5;
    const SHIFT_LEFT: u8 = 1 << 6;
    const SHIFT_RIGHT: u8 = 1 << 7;

    fn capture_pressed(&mut self, keys: &ButtonInput<KeyCode>) {
        for key in [
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ] {
            if keys.pressed(key) {
                self.0 |= Self::mask(key);
            }
        }
    }

    fn observe(&mut self, input: &KeyboardInput) {
        let mask = Self::mask(input.key_code);
        if input.state == ButtonState::Pressed {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }
    }

    fn shortcut(&self) -> bool {
        self.0 & 0b0000_1111 != 0 && self.0 & 0b0011_0000 == 0
    }

    fn shift(&self) -> bool {
        self.0 & 0b1100_0000 != 0
    }

    const fn mask(key: KeyCode) -> u8 {
        match key {
            KeyCode::ControlLeft => Self::CONTROL_LEFT,
            KeyCode::ControlRight => Self::CONTROL_RIGHT,
            KeyCode::SuperLeft => Self::SUPER_LEFT,
            KeyCode::SuperRight => Self::SUPER_RIGHT,
            KeyCode::AltLeft => Self::ALT_LEFT,
            KeyCode::AltRight => Self::ALT_RIGHT,
            KeyCode::ShiftLeft => Self::SHIFT_LEFT,
            KeyCode::ShiftRight => Self::SHIFT_RIGHT,
            _ => 0,
        }
    }
}

impl MenuRuntime {
    pub(super) fn focus_field(&mut self, field: MenuField) {
        self.field = Some(field);
        self.text_selected = false;
    }

    fn has_focused_field(&self) -> bool {
        self.field.is_some()
    }

    fn selected_text_target(&self) -> Option<&str> {
        match self.field? {
            MenuField::Name => Some(&self.name),
            MenuField::Address => Some(&self.address),
        }
    }

    fn select_all_text(&mut self) {
        self.text_selected = self
            .selected_text_target()
            .is_some_and(|text| !text.is_empty());
    }

    fn selected_text(&self) -> Option<&str> {
        self.text_selected
            .then(|| self.selected_text_target())
            .flatten()
    }

    fn remaining_text_capacity(&self) -> usize {
        let Some(field) = self.field else {
            return 0;
        };
        let maximum = match field {
            MenuField::Name => MAX_SERVER_NAME_BYTES,
            MenuField::Address => MAX_SERVER_ADDRESS_BYTES,
        };
        if self.text_selected {
            maximum
        } else {
            maximum.saturating_sub(self.selected_text_target().map_or(0, str::len))
        }
    }

    fn edit_text(&mut self, text: &str) {
        let Some(field) = self.field else {
            return;
        };
        let target = match field {
            MenuField::Name => &mut self.name,
            MenuField::Address => &mut self.address,
        };
        let maximum = match field {
            MenuField::Name => MAX_SERVER_NAME_BYTES,
            MenuField::Address => MAX_SERVER_ADDRESS_BYTES,
        };
        let mut insertion = String::new();
        let base_length = if self.text_selected { 0 } else { target.len() };
        for character in text.chars().filter(|character| !character.is_control()) {
            if base_length
                .saturating_add(insertion.len())
                .saturating_add(character.len_utf8())
                > maximum
            {
                break;
            }
            insertion.push(character);
        }
        if insertion.is_empty() {
            return;
        }
        if self.text_selected {
            target.clear();
        }
        target.push_str(&insertion);
        self.text_selected = false;
    }

    fn backspace_text(&mut self) {
        let Some(field) = self.field else {
            return;
        };
        let target = match field {
            MenuField::Name => &mut self.name,
            MenuField::Address => &mut self.address,
        };
        if self.text_selected {
            target.clear();
        } else {
            let _ = target.pop();
        }
        self.text_selected = false;
    }
}

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
    // A replacement owns no route back into the old session, even when
    // provisioning the new endpoint fails before the connecting screen opens.
    menu.settings_return_to_pause = false;
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
    let child = match spawn_core_for_address(
        &menu.layout,
        &socket_dir,
        &address,
        auth_cache.as_deref(),
        // Advertise upstream cache capability exactly because this same
        // connect hands the verified blob cache to the new network
        // session below; that ownership is what makes the client answer
        // LoginSuccess with cache-enabled status downstream.
        client_blob_cache.enables_upstream_client_cache(),
    ) {
        Ok(child) => child,
        Err(error) => {
            drop(session_directory);
            menu.message = Some(format!("Could not start {address}: {error}"));
            menu.connecting = false;
            return;
        }
    };
    guard.replace(child);
    if let Err(error) = wait_for_core(&socket_dir) {
        super::core_process::stop_core_then(guard, |_| drop(session_directory));
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
            super::core_process::stop_core_then(guard, |_| menu.release_session_directory());
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
    mut clipboard: ResMut<MenuClipboard>,
    mut menu: ResMut<MenuRuntime>,
    mut modifiers: Local<MenuModifiers>,
) {
    let (window, mut cursor) = window.into_inner();
    menu.pressed = None;
    if !window.focused {
        *modifiers = MenuModifiers::default();
        keyboard_messages.clear();
        menu.pointer_down = false;
        return;
    }
    if !menu.is_visible() {
        // Gameplay/chat handled these messages already. In particular, do not
        // replay the Escape that opens pause as "back" on the following frame.
        *modifiers = MenuModifiers::default();
        keyboard_messages.clear();
        menu.hovered = None;
        menu.pointer_down = false;
        if keys.just_pressed(KeyCode::Escape) {
            modifiers.capture_pressed(&keys);
            menu.open_pause();
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
            keys.reset_all();
        }
        return;
    }

    modifiers.capture_pressed(&keys);
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    menu.hovered = window
        .cursor_position()
        .and_then(|position| UiPoint::new(position.x, position.y).ok())
        .and_then(|position| presentation.hit_test_menu(position));
    let pointer_pressed = mouse_buttons.pressed(MouseButton::Left);
    let pointer_just_pressed =
        mouse_buttons.just_pressed(MouseButton::Left) || (pointer_pressed && !menu.pointer_down);
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
        modifiers.observe(input);
        if input.state != ButtonState::Pressed {
            continue;
        }
        if modifiers.shortcut() && menu.has_focused_field() {
            match input.key_code {
                KeyCode::KeyA => {
                    menu.select_all_text();
                    continue;
                }
                KeyCode::KeyC => {
                    if let Some(text) = menu.selected_text() {
                        clipboard.write_text(text.to_owned());
                    }
                    continue;
                }
                KeyCode::KeyV => {
                    let maximum = menu.remaining_text_capacity();
                    if let Some(text) = clipboard.read_text_bounded(maximum) {
                        menu.edit_text(&text);
                    }
                    continue;
                }
                _ => {}
            }
            if input.text.is_some() {
                continue;
            }
        }
        match input.key_code {
            KeyCode::Escape => menu.go_back_from_input(),
            KeyCode::ArrowUp | KeyCode::ArrowLeft => menu.move_focus(-1),
            KeyCode::ArrowDown | KeyCode::ArrowRight => menu.move_focus(1),
            KeyCode::Tab => menu.move_focus(if modifiers.shift() { -1 } else { 1 }),
            KeyCode::Enter | KeyCode::NumpadEnter => menu.activate_focused(),
            KeyCode::Backspace if menu.field.is_some() => menu.backspace_text(),
            _ if menu.has_focused_field() && !modifiers.shortcut() => {
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
        menu.mark_disconnected();
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
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use bevy::prelude::{App, Update};

    use super::{
        super::{
            CoreProcessGuard, MAX_TRANSFER_CHAIN_HOPS, MenuAction, MenuRuntime, MenuScreen,
            format_transfer_address,
        },
        follow_server_transfer,
    };
    use crate::{
        app::ClientBlobCacheOwner,
        install_layout::{InstallEnvironment, InstallLayout, Platform},
        runtime::{
            network::{NetworkHandle, ResourcePackAdmissionState},
            world::{ClientWorld, TransferNotice},
        },
        ui_runtime::UiRuntime,
    };

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "cinnabar-menu-transfer-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create isolated transfer root");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn missing_core_layout(root: &Path) -> InstallLayout {
        InstallLayout::resolve(
            Platform::Linux,
            &InstallEnvironment {
                executable: root.join("target/debug/bedrock-client"),
                home: Some(root.join("home")),
                local_app_data: None,
                xdg_config_home: None,
                xdg_data_home: None,
                xdg_runtime_dir: None,
            },
        )
        .expect("isolated development layout")
    }

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
        assert_eq!(
            format_transfer_address("[2001:db8::10]", 25565),
            "[2001:db8::10]:25565",
            "an already-bracketed transfer literal must not be bracketed twice",
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

    #[test]
    fn failed_automatic_replacement_cannot_return_to_the_old_pause_menu() {
        let root = TempRoot::new();
        let mut menu = MenuRuntime::new_with_layout(
            true,
            2,
            "Player".to_owned(),
            missing_core_layout(root.path()),
        );
        menu.mark_connected();
        menu.open_pause();
        menu.activate(MenuAction::PauseSettings);

        let mut client_world = ClientWorld {
            transfer_notice: Some(TransferNotice {
                host: "transfer.example.net".to_owned(),
                port: 19132,
            }),
            ..ClientWorld::default()
        };
        client_world.stream = None;

        let mut app = App::new();
        app.insert_resource(menu)
            .insert_resource(CoreProcessGuard::default())
            .insert_resource(NetworkHandle::disconnected())
            .insert_resource(ClientBlobCacheOwner::default())
            .insert_resource(ResourcePackAdmissionState::default())
            .insert_resource(UiRuntime::new(1))
            .insert_resource(client_world)
            .add_systems(Update, follow_server_transfer);
        app.update();

        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        assert_eq!(menu.view().screen, MenuScreen::Settings);
        assert!(
            menu.view()
                .message
                .as_deref()
                .is_some_and(|message| message.starts_with("Could not start transfer.example.net"))
        );
        menu.go_back_from_input();
        assert_eq!(menu.view().screen, MenuScreen::Home);
    }
}
