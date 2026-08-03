//! Java-inspired launcher/menu state and the small amount of input plumbing
//! needed before a Bedrock session exists.
//!
//! The game client remains the authority for rendering and input. The menu is
//! deliberately retained UI rather than a second windowing toolkit, so the
//! no-argument path is light, keyboard/controller friendly, and uses exactly
//! the same font, safe-area, and pointer coordinates as the gameplay HUD.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use bevy::{
    input::{ButtonState, keyboard::KeyboardInput},
    prelude::{
        AppExit, ButtonInput, KeyCode, MessageReader, MessageWriter, MouseButton, Res, ResMut,
        Resource, Single, With,
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use serde::{Deserialize, Serialize};
use ui::UiPoint;

use crate::{
    runtime::{endpoint::bridge_endpoint_exists, network::NetworkHandle},
    ui_runtime::presentation::UiPresentationRuntime,
};

const MAX_SERVER_NAME_BYTES: usize = 64;
const MAX_SERVER_ADDRESS_BYTES: usize = 128;
const DEFAULT_SERVER_FILE: &str = ".local/cinnabar/servers.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuScreen {
    Main,
    Play,
    Realms,
    Friends,
    Settings,
    AddServer,
    Pause,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuField {
    Name,
    Address,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuAction {
    MainPlay,
    MainRealms,
    MainFriends,
    MainSettings,
    MainExit,
    PlayAddServer,
    PlayBack,
    PlaySaved(usize),
    PlayFeatured(usize),
    PlayGathering(usize),
    AddName,
    AddAddress,
    AddSave,
    AddSaveConnect,
    AddBack,
    SettingsScale(u8),
    SettingsBack,
    RealmsBack,
    FriendsBack,
    PauseResume,
    PauseDisconnect,
    PauseSettings,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedServer {
    pub(crate) name: String,
    pub(crate) address: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MenuServerCard {
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) caption: String,
}

#[derive(Clone, Debug)]
pub(crate) struct MenuView {
    pub(crate) visible: bool,
    pub(crate) screen: MenuScreen,
    pub(crate) focused: usize,
    pub(crate) field: Option<MenuField>,
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) message: Option<String>,
    pub(crate) gui_scale: u8,
    pub(crate) servers: Vec<SavedServer>,
    pub(crate) featured: Vec<MenuServerCard>,
    pub(crate) gatherings: Vec<MenuServerCard>,
}

#[derive(Debug, Resource)]
pub(crate) struct MenuRuntime {
    visible: bool,
    screen: MenuScreen,
    focused: usize,
    field: Option<MenuField>,
    name: String,
    address: String,
    message: Option<String>,
    gui_scale: u8,
    display_name: String,
    servers: Vec<SavedServer>,
    config_path: PathBuf,
    pending_connect: Option<String>,
    connecting: bool,
    disconnect_requested: bool,
    exit_requested: bool,
    session_generation: u64,
}

impl MenuRuntime {
    pub(crate) fn new(visible: bool, gui_scale: u8, display_name: String) -> Self {
        let config_path = PathBuf::from(DEFAULT_SERVER_FILE);
        let servers = load_servers(&config_path);
        Self {
            visible,
            screen: MenuScreen::Main,
            focused: 0,
            field: None,
            name: String::new(),
            address: String::new(),
            message: None,
            gui_scale: gui_scale.clamp(1, 4),
            display_name,
            servers,
            config_path,
            pending_connect: None,
            connecting: false,
            disconnect_requested: false,
            exit_requested: false,
            session_generation: 1,
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn is_connecting(&self) -> bool {
        self.connecting
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if !visible {
            self.field = None;
        }
    }

    pub(crate) fn view(&self) -> MenuView {
        MenuView {
            visible: self.visible,
            screen: self.screen,
            focused: self.focused,
            field: self.field,
            name: self.name.clone(),
            address: self.address.clone(),
            message: self.message.clone(),
            gui_scale: self.gui_scale,
            servers: self.servers.clone(),
            featured: featured_servers(),
            gatherings: gathering_servers(),
        }
    }

    pub(crate) fn open_pause(&mut self) {
        if self.visible || self.connecting {
            return;
        }
        self.screen = MenuScreen::Pause;
        self.focused = 0;
        self.message = None;
        self.visible = true;
    }

    pub(crate) fn take_pending_connect(&mut self) -> Option<String> {
        self.pending_connect.take()
    }

    pub(crate) fn mark_connected(&mut self) {
        self.connecting = false;
        self.visible = false;
        self.screen = MenuScreen::Main;
        self.message = None;
        self.field = None;
    }

    pub(crate) fn mark_connecting(&mut self) {
        self.connecting = true;
        self.visible = true;
        self.screen = MenuScreen::Play;
        self.message = Some("Connecting…".to_owned());
    }

    pub(crate) fn take_disconnect_request(&mut self) -> bool {
        std::mem::take(&mut self.disconnect_requested)
    }

    pub(crate) fn take_exit_request(&mut self) -> bool {
        std::mem::take(&mut self.exit_requested)
    }

    pub(crate) fn next_session_generation(&mut self) -> u64 {
        self.session_generation = self.session_generation.saturating_add(1).max(1);
        self.session_generation
    }

    pub(crate) fn activate(&mut self, action: MenuAction) {
        self.message = None;
        match action {
            MenuAction::MainPlay => self.enter(MenuScreen::Play),
            MenuAction::MainRealms => self.enter(MenuScreen::Realms),
            MenuAction::MainFriends => self.enter(MenuScreen::Friends),
            MenuAction::MainSettings => self.enter(MenuScreen::Settings),
            MenuAction::MainExit => self.exit_requested = true,
            MenuAction::PlayAddServer => {
                self.name.clear();
                self.address.clear();
                self.field = Some(MenuField::Name);
                self.enter(MenuScreen::AddServer);
            }
            MenuAction::PlayBack => self.go_back(),
            MenuAction::PlaySaved(index) => {
                if let Some(server) = self.servers.get(index) {
                    self.request_connect(server.address.clone());
                }
            }
            MenuAction::PlayFeatured(index) => {
                if let Some(server) = featured_servers().get(index) {
                    self.request_connect(server.address.clone());
                }
            }
            MenuAction::PlayGathering(index) => {
                if let Some(server) = gathering_servers().get(index) {
                    self.request_connect(server.address.clone());
                }
            }
            MenuAction::AddName => self.field = Some(MenuField::Name),
            MenuAction::AddAddress => self.field = Some(MenuField::Address),
            MenuAction::AddSave => {
                if self.save_draft() {
                    self.enter(MenuScreen::Play);
                }
            }
            MenuAction::AddSaveConnect => {
                if self.save_draft() {
                    self.request_connect(self.address.clone());
                }
            }
            MenuAction::AddBack => self.go_back(),
            MenuAction::SettingsScale(scale) => self.gui_scale = scale.clamp(1, 4),
            MenuAction::SettingsBack => self.go_back(),
            MenuAction::RealmsBack | MenuAction::FriendsBack => self.go_back(),
            MenuAction::PauseResume => self.set_visible(false),
            MenuAction::PauseDisconnect => {
                self.disconnect_requested = true;
                self.set_visible(false);
            }
            MenuAction::PauseSettings => self.enter(MenuScreen::Settings),
        }
    }

    pub(crate) fn move_focus(&mut self, direction: i32) {
        let actions = self.focus_actions();
        if actions.is_empty() {
            self.focused = 0;
            return;
        }
        let length = actions.len() as i32;
        self.focused = (self.focused as i32 + direction).rem_euclid(length) as usize;
    }

    pub(crate) fn activate_focused(&mut self) {
        let Some(action) = self.focus_actions().get(self.focused).copied() else {
            return;
        };
        self.activate(action);
    }

    pub(crate) fn edit_text(&mut self, text: &str) {
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
        for character in text.chars().filter(|character| !character.is_control()) {
            if target.len().saturating_add(character.len_utf8()) > maximum {
                break;
            }
            target.push(character);
        }
    }

    pub(crate) fn backspace_text(&mut self) {
        let Some(field) = self.field else {
            return;
        };
        let target = match field {
            MenuField::Name => &mut self.name,
            MenuField::Address => &mut self.address,
        };
        let _ = target.pop();
    }

    fn focus_actions(&self) -> Vec<MenuAction> {
        match self.screen {
            MenuScreen::Main => vec![
                MenuAction::MainPlay,
                MenuAction::MainRealms,
                MenuAction::MainFriends,
                MenuAction::MainSettings,
                MenuAction::MainExit,
            ],
            MenuScreen::Play => {
                let mut actions = vec![MenuAction::PlayAddServer];
                actions.extend((0..self.servers.len()).map(MenuAction::PlaySaved));
                actions.extend((0..featured_servers().len()).map(MenuAction::PlayFeatured));
                actions.extend((0..gathering_servers().len()).map(MenuAction::PlayGathering));
                actions.push(MenuAction::PlayBack);
                actions
            }
            MenuScreen::Realms => vec![MenuAction::RealmsBack],
            MenuScreen::Friends => vec![MenuAction::FriendsBack],
            MenuScreen::Settings => vec![
                MenuAction::SettingsScale(1),
                MenuAction::SettingsScale(2),
                MenuAction::SettingsScale(3),
                MenuAction::SettingsScale(4),
                MenuAction::SettingsBack,
            ],
            MenuScreen::AddServer => vec![
                MenuAction::AddName,
                MenuAction::AddAddress,
                MenuAction::AddSave,
                MenuAction::AddSaveConnect,
                MenuAction::AddBack,
            ],
            MenuScreen::Pause => vec![
                MenuAction::PauseResume,
                MenuAction::PauseSettings,
                MenuAction::PauseDisconnect,
            ],
        }
    }

    fn enter(&mut self, screen: MenuScreen) {
        self.screen = screen;
        self.focused = 0;
        self.field = None;
        self.message = None;
        self.visible = true;
    }

    fn go_back(&mut self) {
        match self.screen {
            MenuScreen::Main => {}
            MenuScreen::Pause => self.set_visible(false),
            _ => self.enter(if self.screen == MenuScreen::AddServer {
                MenuScreen::Play
            } else {
                MenuScreen::Main
            }),
        }
    }

    fn save_draft(&mut self) -> bool {
        let name = self.name.trim();
        let address = self.address.trim();
        if name.is_empty() || address.is_empty() {
            self.message = Some("Enter a server name and address.".to_owned());
            return false;
        }
        let server = SavedServer {
            name: name.to_owned(),
            address: address.to_owned(),
        };
        if let Some(existing) = self
            .servers
            .iter_mut()
            .find(|existing| existing.address.eq_ignore_ascii_case(&server.address))
        {
            *existing = server;
        } else {
            self.servers.push(server);
        }
        if let Err(error) = save_servers(&self.config_path, &self.servers) {
            self.message = Some(format!("Could not save server: {error}"));
            return false;
        }
        true
    }

    fn request_connect(&mut self, address: String) {
        if address.trim().is_empty() {
            self.message = Some("That server has no address.".to_owned());
            return;
        }
        self.pending_connect = Some(address);
        self.mark_connecting();
    }
}

#[derive(Debug, Resource, Default)]
pub(crate) struct CoreProcessGuard {
    child: Option<Child>,
}

impl CoreProcessGuard {
    pub(crate) fn replace(&mut self, child: Child) {
        self.stop();
        self.child = Some(child);
    }

    pub(crate) fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for CoreProcessGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn spawn_core_for_address(socket_dir: &Path, address: &str) -> Result<Child> {
    let executable = core_executable().ok_or_else(|| {
        anyhow::anyhow!(
            "bedrock-core executable was not found beside the client or in target/debug/target/release"
        )
    })?;
    let mut command = Command::new(&executable);
    command
        .arg("-socket-dir")
        .arg(socket_dir)
        .arg("-upstream")
        .arg(address)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let auth_cache = std::env::current_dir()
        .ok()
        .map(|directory| directory.join(".local/auth/microsoft-token.json"))
        .filter(|path| path.is_file());
    if let Some(auth_cache) = auth_cache {
        command.arg("-auth-cache").arg(auth_cache);
    }
    let child = command
        .spawn()
        .with_context(|| format!("spawn {} for {address}", executable.display()))?;
    Ok(child)
}

pub(crate) fn wait_for_core(socket_dir: &Path) -> Result<()> {
    for _ in 0..100 {
        if bridge_endpoint_exists(socket_dir) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!(
        "bedrock-core did not publish its endpoint at {}",
        socket_dir.display()
    )
}

fn core_executable() -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        "bedrock-core.exe"
    } else {
        "bedrock-core"
    };
    let current = std::env::current_exe().ok();
    let working = std::env::current_dir().ok();
    let candidates = [
        current
            .as_ref()
            .and_then(|path| path.parent())
            .map(|path| path.join(filename)),
        working.as_ref().map(|path| path.join(filename)),
        working
            .as_ref()
            .map(|path| path.join("target/debug").join(filename)),
        working
            .as_ref()
            .map(|path| path.join("target/release").join(filename)),
    ];
    candidates.into_iter().flatten().find(|path| path.is_file())
}

fn load_servers(path: &Path) -> Vec<SavedServer> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_servers(path: &Path, servers: &[SavedServer]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(servers).context("encode saved servers")?;
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn featured_servers() -> Vec<MenuServerCard> {
    vec![
        MenuServerCard {
            name: "Zeqa Network".to_owned(),
            address: "zeqa.net:19132".to_owned(),
            caption: "Practice & minigames".to_owned(),
        },
        MenuServerCard {
            name: "Lunar Bedrock PvP".to_owned(),
            address: "pvp.lunarbedrock.com:19134".to_owned(),
            caption: "Vanilla combat lobby".to_owned(),
        },
    ]
}

fn gathering_servers() -> Vec<MenuServerCard> {
    vec![MenuServerCard {
        name: "Community Gatherings".to_owned(),
        address: "gatherings.lunarbedrock.com:19132".to_owned(),
        caption: "Community events".to_owned(),
    }]
}

pub(crate) fn drive_menu_input(
    mut keyboard_messages: MessageReader<KeyboardInput>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    presentation: Res<UiPresentationRuntime>,
    mut menu: ResMut<MenuRuntime>,
) {
    let (window, mut cursor) = window.into_inner();
    if !window.focused {
        return;
    }
    if !menu.is_visible() {
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
    if mouse_buttons.just_pressed(MouseButton::Left)
        && let Some(position) = window.cursor_position()
        && let Ok(position) = UiPoint::new(position.x, position.y)
        && let Some(action) = presentation.hit_test_menu(position)
    {
        menu.activate(action);
    }
    for input in keyboard_messages.read() {
        if input.state != ButtonState::Pressed {
            continue;
        }
        match input.key_code {
            KeyCode::Escape => menu.go_back_from_input(),
            KeyCode::ArrowUp => menu.move_focus(-1),
            KeyCode::ArrowDown | KeyCode::Tab => menu.move_focus(1),
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

pub(crate) fn drive_menu_connection(
    mut commands: bevy::prelude::Commands,
    mut exits: MessageWriter<AppExit>,
    mut menu: ResMut<MenuRuntime>,
    mut guard: ResMut<CoreProcessGuard>,
    mut network: ResMut<NetworkHandle>,
    mut runtime: ResMut<crate::ui_runtime::UiRuntime>,
    mut client_world: ResMut<crate::runtime::world::ClientWorld>,
) {
    if menu.is_connecting() && client_world.stream.is_some() {
        menu.mark_connected();
    }
    if let Some(address) = menu.take_pending_connect() {
        let generation = menu.next_session_generation();
        let socket_dir = PathBuf::from(format!(".local/cinnabar/connect-{generation}"));
        if let Err(error) = fs::create_dir_all(&socket_dir)
            .and_then(|_| {
                spawn_core_for_address(&socket_dir, &address).map_err(std::io::Error::other)
            })
            .and_then(|child| {
                guard.replace(child);
                wait_for_core(&socket_dir).map_err(std::io::Error::other)
            })
        {
            menu.message = Some(format!("Could not start {address}: {error}"));
            menu.connecting = false;
            return;
        }
        network.shutdown();
        match crate::runtime::network::spawn_network(crate::runtime::network::NetworkConfig {
            session_generation: generation,
            socket_dir,
            display_name: menu.display_name.clone(),
            client_blob_cache: protocol::ClientBlobCache::default(),
        }) {
            Ok(replacement) => {
                runtime.begin_session(generation);
                client_world.stream = None;
                client_world.pending_surface_spawn = None;
                client_world.fatal_error = None;
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
    if menu.take_disconnect_request() {
        network.shutdown();
        guard.stop();
        runtime.begin_session(menu.next_session_generation());
        client_world.stream = None;
        menu.visible = true;
        menu.screen = MenuScreen::Main;
        menu.connecting = false;
    }
    if menu.take_exit_request() {
        network.shutdown();
        guard.stop();
        exits.write(AppExit::Success);
    }
}
