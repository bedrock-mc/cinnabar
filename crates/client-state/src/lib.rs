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

/// Interaction phases whose surrounding world and transport adapters share the frame.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientInteractionSet {
    HotbarSend,
    RetainPick,
}
