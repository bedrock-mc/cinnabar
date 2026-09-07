use bevy::{
    ecs::system::SystemParam,
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
    local_player::{InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameReset},
    movement::{LocalPhysicsController, MovementTicker},
    runtime::{
        network::{NetworkConfig, NetworkHandle, ResourcePackAdmissionState},
        world::ClientWorld,
    },
    session_cleanup::SessionDirectoryGuard,
    ui_runtime::{PlatformClipboard, UiRuntime, presentation::UiPresentationRuntime},
};

#[derive(SystemParam)]
pub(crate) struct MenuSessionState<'w> {
    guard: ResMut<'w, CoreProcessGuard>,
    network: ResMut<'w, NetworkHandle>,
    resource_packs: ResMut<'w, ResourcePackAdmissionState>,
    runtime: ResMut<'w, UiRuntime>,
    client_world: ResMut<'w, ClientWorld>,
    movement: ResMut<'w, MovementTicker>,
    local_physics: ResMut<'w, LocalPhysicsController>,
    local_frame: ResMut<'w, LocalPlayerFrameCarrier>,
    interaction: ResMut<'w, InteractionOriginSnapshot>,
}

impl MenuSessionState<'_> {
    fn retire(&mut self, menu: &mut MenuRuntime) -> u64 {
        *self.network = NetworkHandle::disconnected();
        self.guard.stop();
        menu.release_session_directory();
        let generation = menu.next_session_generation();
        self.resource_packs.begin_generation(generation);
        self.runtime.begin_session(generation);
        self.client_world.stream = None;
        self.client_world.pending_surface_spawn = None;
        self.client_world.fatal_error = None;
        self.client_world.transfer_notice = None;
        self.movement.deactivate();
        self.local_physics.deactivate();
        self.local_frame.reset(LocalPlayerFrameReset::Session);
        self.interaction.invalidate();
        generation
    }
}

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
fn attempt_connect(
    commands: &mut bevy::prelude::Commands,
    menu: &mut MenuRuntime,
    client_blob_cache: &crate::app::ClientBlobCacheOwner,
    session: &mut MenuSessionState<'_>,
    address: String,
    auth_cache: Option<std::path::PathBuf>,
) {
    // A replacement owns no route back into the old session, even when
    // provisioning the new endpoint fails before the connecting screen opens.
    menu.mark_disconnected();
    let generation = session.retire(menu);
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
    session.guard.replace(child);
    if let Err(error) = wait_for_core(&socket_dir) {
        super::core_process::stop_core_then(&mut session.guard, |_| drop(session_directory));
        menu.message = Some(format!("Could not start {address}: {error}"));
        menu.connecting = false;
        return;
    }
    menu.bind_session_directory(session_directory);
    match crate::runtime::network::spawn_network(NetworkConfig {
        session_generation: generation,
        socket_dir,
        display_name: menu.display_name.clone(),
        client_blob_cache: client_blob_cache.cache(),
    }) {
        Ok(replacement) => {
            commands.insert_resource(replacement.movement_ticker());
            commands.insert_resource(replacement);
            menu.mark_connecting();
        }
        Err(error) => {
            super::core_process::stop_core_then(&mut session.guard, |_| {
                menu.release_session_directory();
            });
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
    client_blob_cache: Res<crate::app::ClientBlobCacheOwner>,
    mut session: MenuSessionState,
) {
    menu.poll_catalog();
    if menu.is_connecting() && session.client_world.stream.is_some() {
        menu.mark_connected();
    }
    if let Some(pending) = menu.take_pending_connect() {
        attempt_connect(
            &mut commands,
            &mut menu,
            &client_blob_cache,
            &mut session,
            pending.address,
            pending.auth_cache,
        );
    }
    if menu.take_disconnect_request() {
        // Drop the old event receivers as well as stopping their worker: a
        // queued transfer must not undo this explicit disconnect later this frame.
        session.retire(&mut menu);
        menu.mark_disconnected();
    }
    if menu.take_exit_request() {
        session.retire(&mut menu);
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
    mut session: MenuSessionState,
) {
    let Some(error) = session.client_world.fatal_error.clone() else {
        return;
    };
    if !menu.absorb_session_failure(&error) {
        return;
    }
    session.retire(&mut menu);
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
pub(crate) fn follow_server_transfer(
    mut commands: bevy::prelude::Commands,
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<crate::app::ClientBlobCacheOwner>,
    mut session: MenuSessionState,
) {
    let Some(notice) = session.client_world.transfer_notice.take() else {
        return;
    };
    let target = notice.host.clone();
    if !menu.is_launcher() {
        // No launcher exists to re-enter, so the one-session run ends with
        // the server-directed move named explicitly instead of followed.
        session.retire(&mut menu);
        crate::runtime::shutdown::record_fatal_error(
            &mut session.client_world.fatal_error,
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
            &mut session,
            format!("server sent an unusable transfer target ({target})"),
        );
        return;
    };
    if !menu.consume_transfer_chain_hop() {
        end_transfer_without_follow(
            &mut menu,
            &mut session,
            format!(
                "the transfer chain limit was reached at {}",
                crate::menu::format_transfer_address(&notice.host, notice.port)
            ),
        );
        return;
    }
    // The shared replacement path tears down old transport and world/UI state
    // before provisioning, so every early failure leaves no stale session.
    attempt_connect(
        &mut commands,
        &mut menu,
        &client_blob_cache,
        &mut session,
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
fn end_transfer_without_follow(
    menu: &mut MenuRuntime,
    session: &mut MenuSessionState<'_>,
    reason: String,
) {
    session.retire(menu);
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
        failed_automatic_replacement(true);
    }

    #[test]
    fn pointer_opened_dialogs_accept_keyboard_confirmation_and_navigation() {
        for remove_saved in [false, true] {
            let root = TempRoot::new();
            let mut menu = MenuRuntime::new_with_layout(
                true,
                2,
                "Player".to_owned(),
                missing_core_layout(root.path()),
            );
            menu.servers.push(super::super::SavedServer {
                name: "Local".to_owned(),
                address: "127.0.0.1:19132".to_owned(),
                favorite: false,
                last_joined_unix: 0,
            });
            menu.focused = 6;
            let (open, confirm) = if remove_saved {
                (
                    MenuAction::RemoveSavedDialog(0),
                    MenuAction::ConfirmRemoveSaved(0),
                )
            } else {
                (MenuAction::OpenExitDialog, MenuAction::ConfirmExit)
            };
            menu.activate(open);
            assert_eq!(menu.view().focused_action, Some(confirm));
            menu.move_focus(1);
            assert_eq!(menu.view().focused_action, Some(MenuAction::DismissDialog));
            menu.move_focus(-1);
            menu.activate_focused();
            assert!(menu.dialog.is_none());
            if remove_saved {
                assert!(menu.servers.is_empty());
            } else {
                assert!(menu.exit_requested);
            }
        }
    }

    #[test]
    fn failed_automatic_replacement_from_gameplay_reopens_the_launcher() {
        failed_automatic_replacement(false);
    }

    #[test]
    fn explicit_disconnect_discards_queued_terminal_events_and_closes_the_old_receiver() {
        use crate::runtime::network::NetworkControlEvent;
        use bevy::app::AppExit;
        use tokio::sync::mpsc;

        let root = TempRoot::new();
        let mut menu = MenuRuntime::new_with_layout(
            true,
            2,
            "Player".to_owned(),
            missing_core_layout(root.path()),
        );
        menu.catalog_started = true;
        menu.mark_connected();
        menu.open_pause();
        menu.activate(MenuAction::PauseDisconnect);
        let mut network = NetworkHandle::disconnected();
        let (old_controls, receiver) = mpsc::channel(2);
        *network.control_events_mut() = receiver;
        old_controls
            .try_send(NetworkControlEvent::Transferred {
                target: crate::runtime::network::SessionTransferTarget {
                    host: "old.example.net".to_owned(),
                    port: 19132,
                },
                decode_error_count: 0,
            })
            .unwrap();
        old_controls
            .try_send(NetworkControlEvent::Stopped {
                decode_error_count: 0,
            })
            .unwrap();

        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(menu)
            .insert_resource(CoreProcessGuard::default())
            .insert_resource(network)
            .insert_resource(ClientBlobCacheOwner::default())
            .insert_resource(ResourcePackAdmissionState::default())
            .insert_resource(UiRuntime::new(1))
            .insert_resource(ClientWorld::default())
            .insert_resource(crate::movement::MovementTicker::default())
            .insert_resource(crate::movement::LocalPhysicsController::default())
            .insert_resource(crate::local_player::LocalPlayerFrameCarrier::default())
            .insert_resource(crate::local_player::InteractionOriginSnapshot::default())
            .add_systems(Update, super::drive_menu_connection);
        app.update();

        let menu = app.world().resource::<MenuRuntime>();
        assert!(menu.is_visible());
        assert_eq!(menu.view().screen, MenuScreen::Home);
        assert!(!menu.is_connecting());
        assert_eq!(
            app.world()
                .resource::<NetworkHandle>()
                .pending_event_count(),
            0
        );
        assert!(
            app.world()
                .resource::<ClientWorld>()
                .transfer_notice
                .is_none()
        );
        assert!(old_controls.is_closed());
    }

    fn failed_automatic_replacement(from_settings: bool) {
        let root = TempRoot::new();
        let mut menu = MenuRuntime::new_with_layout(
            true,
            2,
            "Player".to_owned(),
            missing_core_layout(root.path()),
        );
        let old_generation = menu.next_session_generation();
        menu.mark_connected();
        if from_settings {
            menu.open_pause();
            menu.activate(MenuAction::PauseSettings);
        }

        let client_world = ClientWorld {
            stream: Some(client_world::WorldStream::new(protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.0, 64.0, 0.0],
                world_spawn_position: [0, 64, 0],
                air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
                block_network_ids_are_hashes: false,
            })),
            transfer_notice: Some(TransferNotice {
                host: "transfer.example.net".to_owned(),
                port: 19132,
            }),
            ..ClientWorld::default()
        };
        let mut runtime = UiRuntime::new(old_generation);
        let _ = runtime.open_chat();
        runtime.insert_chat_text("old session draft").unwrap();

        let mut app = App::new();
        app.insert_resource(menu)
            .insert_resource(CoreProcessGuard::default())
            .insert_resource(NetworkHandle::disconnected())
            .insert_resource(ClientBlobCacheOwner::default())
            .insert_resource(ResourcePackAdmissionState::default())
            .insert_resource(runtime)
            .insert_resource(client_world)
            .insert_resource(crate::movement::MovementTicker::default())
            .insert_resource(crate::movement::LocalPhysicsController::default())
            .insert_resource(crate::local_player::LocalPlayerFrameCarrier::default())
            .insert_resource(crate::local_player::InteractionOriginSnapshot::default())
            .add_systems(Update, follow_server_transfer);
        app.update();

        assert!(app.world().resource::<ClientWorld>().stream.is_none());
        let runtime = app.world().resource::<UiRuntime>();
        assert_ne!(runtime.session_id(), old_generation);
        assert!(!runtime.chat_focused());
        assert!(runtime.chat_editor().as_str().is_empty());

        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        assert!(menu.is_visible());
        assert_eq!(menu.view().screen, MenuScreen::Home);
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

#[cfg(test)]
#[path = "session_teardown_tests.rs"]
mod session_teardown_tests;
