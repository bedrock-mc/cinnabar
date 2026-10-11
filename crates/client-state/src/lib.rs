//! Shared ECS contracts for the client plugins.
//!
//! This crate contains resource wrappers, frame sets and observations only.
//! Domain crates own state transitions; edge plugins own systems and publication.

use bevy::prelude::{Deref, DerefMut, Resource, SystemSet};

/// The ordered frame boundaries shared by the client plugins.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientFrameSet {
    RawInput,
    SemanticSample,
    UiAuthority,
    SemanticFinalize,
    Physics,
    Camera,
    Interaction,
    NetworkSend,
    WorldPublication,
    ActorPreparation,
    UiPreparation,
    ActorFinalization,
    ActorPublication,
    UiPublication,
}

/// Optional developer camera input, after physical look and before movement.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModCameraInputSet;

/// Shared player authority for network, UI and movement systems.
#[derive(Clone, Debug, Resource, Deref, DerefMut)]
pub struct PlayerRuntime(pub player_state::PlayerState);

/// The latest settings revision consumed by client subsystems.
#[derive(Clone, Debug, Default, Resource, Deref, DerefMut)]
pub struct RuntimeSettings(pub ui::settings_state::SettingsState);

/// The finalized semantic action frame consumed by gameplay and presentation.
#[derive(Clone, Debug, Default, Resource, Deref, DerefMut)]
pub struct SemanticInputSnapshot(pub semantic_input::finalized::FinalizedInput);

/// Screen ownership published by the shell for the current gameplay frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Resource)]
pub struct ScreenFocus {
    /// Whether a launcher or pause menu is visible.
    pub menu_visible: bool,
    /// Whether the UI owns player input independently of pack scene flags.
    pub ui_focused: bool,
    /// Whether the active screen stack absorbs gameplay input.
    pub absorbs_input: bool,
    /// The UI's cursor request, absent when there is no UI runtime.
    pub steals_mouse: Option<bool>,
}

/// Menu-configured controls sampled by the shell for gameplay consumers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Resource)]
pub struct GameplayScreenControls {
    pub pick_block_pressed: bool,
    pub spyglass_mouse_damping: f32,
    pub spyglass_gamepad_damping: f32,
}

/// Retained desktop input ownership shared by the shell and camera.
#[derive(Debug, Default, Resource, Deref, DerefMut)]
pub struct CursorFocus(pub semantic_input::cursor_focus::CursorFocus);

/// Present while a developer controller drives input without OS cursor capture.
#[derive(Debug, Default, Resource)]
pub struct DrivenInput;

/// Consent owns the whole input frame, including its dismissal frame.
#[derive(Default, Resource)]
pub struct ConsentInput(pub bool);

/// A join requested by the menu and consumed by the session controller.
#[derive(Debug)]
pub struct JoinIntent {
    pub address: String,
    pub auth_cache: Option<std::path::PathBuf>,
    /// Joins the launcher core's opened local world instead of the address.
    pub local_world: bool,
}

/// Session ownership presented by the menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionStatus {
    pub connecting: bool,
    pub owns_directory: bool,
}

/// Session facts consumed by the desktop presence driver.
#[derive(Default, Resource)]
pub struct PresenceObservation {
    pub has_world: bool,
    pub failed: bool,
    pub players: Option<u32>,
    pub target: Option<rich_presence::Target>,
}

/// Shell boundaries used to order session and gameplay adapters without plugin edges.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellFrameSet {
    MenuInput,
    Session,
    WindowSettings,
    Store,
    Focus,
    FinalFocus,
    GameplayControls,
    Presence,
}

/// Consumes consent input after the experience controller and before ordinary input.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExperienceInputSet;
