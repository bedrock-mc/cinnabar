//! Experimental component host. Only the explicit WIT imports carry authority.

mod declarations;
mod drawing_policy;
use drawing_policy::DrawingRestart;
mod grants;
pub use declarations::{
    Declarations, REQUESTED_GRANTS_SECTION, declarations, declarations_snapshot,
};
pub mod helper;
pub use grants::{Capability, ModGrants};
mod callback_budget;
mod callbacks;
pub use callback_budget::{CallbackBudget, CallbackOutcome};
mod exclusive;
pub use exclusive::HudClaims;
mod component_assets;
mod execution;
mod load;
pub mod local_files;
pub mod module_observations;
mod outputs;
pub mod package;
mod runtime;
mod screens;
pub mod server;
mod settings;
mod terrain;
pub use settings::{SettingsSnapshot, settings_companion};

pub use screens::{DataSource, KeyModifiers, LoadedPackage, ModEvent, ModScreens};

pub use runtime::cinnabar::extension::gameplay::{
    CameraRig as GameplayCameraRig, Mob as GameplayMob,
    MovementSnapshot as GameplayMovementSnapshot, MovementToggles, Player as GameplayPlayer,
    Snapshot as GameplaySnapshot, Vector3 as GameplayVector3,
};
pub use runtime::cinnabar::extension::player_state::{
    Actor as ActorObservation, Bounds as ActorObservationBounds,
    ChatMessage as ObservedChatMessage, CombatEvent as CombatObservation, CombatKind,
    Effect as PlayerStateEffect, Food as FoodObservation, Item as PlayerStateItem,
    Motion as MotionObservation, ObservedActor, Sidebar as SidebarSnapshot, SidebarRow,
    Slot as PlayerStateSlot, Snapshot as PlayerStateSnapshot, Vitals as VitalsObservation,
    WorldObservation,
};
pub use runtime::cinnabar::extension::{
    events::Cue as ModCue, input::Controls as ControlFrame, panel::Event as ControlEvent,
};

pub use runtime::cinnabar::extension::terrain::{
    BlockState as TerrainBlockState, ChunkRevision as TerrainChunkRevision,
    Geometry as TerrainGeometry, Physics as TerrainPhysics, PhysicsFlags as TerrainPhysicsFlags,
    Position as TerrainPosition, ReadError as TerrainReadError, ReadResult as TerrainReadResult,
    Region as TerrainRegion, RegionQuery as TerrainQuery, ResolvedShape as TerrainShape,
    ShapeBinding as TerrainShapeBinding, ShapeOverride as TerrainShapeOverride,
    Stamp as TerrainStamp, SurfaceResponse as TerrainSurfaceResponse,
};
pub use terrain::{
    TerrainCheck, TerrainDescriptors, TerrainMetadata, TerrainRefresh, TerrainScheduler,
    TerrainSource, terrain_query_size, terrain_region_bytes,
};

/// Successfully committed local interaction requests, consumed once per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InteractionOutput {
    pub attack_reach: Option<f32>,
    pub attack_pulse: bool,
}

/// A vanilla inventory request a player mod committed, consumed once; the client makes it as
/// the vanilla screen it stands for would, and the server checks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryRequest {
    /// A creative catalog take of `count` of the item `identifier` with `aux`.
    CreativeTake {
        identifier: String,
        aux: u16,
        count: u8,
    },
    /// One recipe book craft of crafting recipe `network_id`.
    FillRecipe { network_id: u32 },
}

/// Committed local actor rotation; yaw turns left and pitch turns up, in radians.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraDelta {
    pub yaw: f32,
    pub pitch: f32,
}
use {
    runtime::Instance,
    server_experience::{screen::ScreenLayout, session_data::SessionData},
    std::{path::PathBuf, sync::Arc},
    wasmtime::Engine,
};

/// Maximum bytes accepted before compilation or allocation of a package buffer.
pub const MAX_COMPONENT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum local component envelope; executable bytes retain their separate original ceiling.
pub const MAX_COMPONENT_SOURCE_BYTES: usize = MAX_COMPONENT_BYTES
    + ui::mod_panel::MAX_PANEL_BUNDLE_BYTES
    + mod_api::MAX_PANEL_FONT_BYTES
    + mod_api::MAX_MENU_LOGO_BYTES
    + mod_api::MAX_MENU_TITLE_BYTES
    + mod_api::MAX_NOTICES_BYTES
    + mod_api::MAX_ICON_BYTES
    + component_assets::MAX_SECTION_FRAMING_BYTES;
/// Plain-text UI limit, checked before publishing any guest output.
pub const MAX_LABEL_BYTES: usize = 256;
pub(crate) const FRAME_FUEL: u64 = 100_000;
pub(crate) const MEMORY_BYTES: usize = 16 * 1024 * 1024;

/// Where a mod came from, which reload reads again.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Component(PathBuf),
    Package(PathBuf),
}
/// A developer-selected component with transactional reload and trap quarantine.
pub struct ModHost {
    engine: Engine,
    _ticker: execution::EpochTicker,
    instance: Instance,
    embedded_font: Option<Arc<[u8]>>,
    embedded_logo: Option<Arc<[u8]>>,
    embedded_title: Option<Arc<str>>,
    embedded_notices: Option<Arc<str>>,
    panel_bundle_override: Option<Arc<ui::mod_panel::PanelBundle>>,
    source: Source,
    attempted: [u8; 32],
    admitted_digest: [u8; 32],
    grants: ModGrants,
    /// The loader's authorization before component or manifest requests narrow it.
    authorized_grants: ModGrants,
    settings_writer: Option<settings::SettingsWriter>,
    settings_seed: Option<String>,
    pending_events: Vec<ModEvent>,
    package: Option<LoadedPackage>,
    layout: Option<ScreenLayout>,
    session: Arc<SessionData>,
    drawing_session: Option<mod_api::drawing::DrawingSession>,
    drawing_restart: DrawingRestart,
}

/// Builds an empty, unfocused control snapshot for a frame with no routed input.
pub fn empty_controls() -> ControlFrame {
    ControlFrame {
        seconds: 0.0,
        focused: false,
        gameplay: false,
        panel_open: false,
        keys_pressed: Vec::new(),
        keys_held: Vec::new(),
        events: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
