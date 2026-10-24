//! Named, anonymous screen fixtures. Add a state here when a PR needs a new capture.

mod catalog;
mod chat;
mod collection_reveal;
#[cfg(feature = "test-support")]
mod cosmetics;
mod hud_card_fit;
mod hud_cards;
mod hud_editor;
mod local_identity;
mod minecraft_import;
mod mods;
mod out_of_game;
mod profile;
mod roster;
mod server_forms;
mod settings_screens;
mod shader_options;
mod shader_pack;
mod social_settings;
mod touch_editor;
#[cfg(feature = "test-support")]
pub(crate) use touch_editor::TouchUtilityView;
mod updates;

use std::sync::Arc;

use launcher::menu::{
    MenuAction, MenuScreen, MenuView, settings_options::SettingId,
    settings_sections::SettingSection,
};
use ui_screens::player_list::RosterRow;

/// A repeatable viewport and the runtime observations its screen paints.
pub struct State {
    pub name: &'static str,
    pub size: [u32; 2],
    pub gui_scale: u8,
    pub fixture: fn() -> Fixture,
}

/// Inputs borrowed by the same screen painters the client uses.
pub enum Fixture {
    /// Unequal mask edges and high-resolution sprite minification.
    MaskRegression,
    /// Original sprite samples before and after alpha-only outline shading.
    SpriteOutline {
        outline: bool,
    },
    /// Adjacent sparse saturation masks before and after overlap-aware exclusion.
    SaturationOverlap {
        crop: bool,
        source_alpha: u8,
    },
    /// A label whose unavailable primary face selects its installed backup.
    BackupFont,
    /// An original checker repeated beyond a small sprite's former draw cutoff.
    TiledImage {
        scale: f64,
    },
    /// A server form whose editable controls are locked during a session.
    #[cfg(feature = "test-support")]
    ControlLocked,
    /// An equipped original cosmetic rendered by the full production particle-preview path.
    #[cfg(feature = "test-support")]
    CosmeticParticles {
        view: client_ui::ui_runtime::presentation::player_preview::PreviewView,
        emitting: bool,
    },
    Menu(Box<MenuView>),
    /// Built-in touch editor interactions through production rendering and pointer routing.
    #[cfg(feature = "test-support")]
    TouchEditor {
        phase: u8,
    },
    /// Gameplay touch controls and their utility overlays.
    #[cfg(feature = "test-support")]
    TouchUtilities(TouchUtilityView),
    Loading(ui::LoadingStage),
    Chat {
        draft: &'static str,
        suggestions: Vec<Arc<str>>,
        messages: std::collections::VecDeque<ui::ChatMessage>,
        options: Arc<launcher::menu::settings_options::SettingsOptions>,
        server_hover: Option<bool>,
    },
    /// The held tab roster's rows.
    TabList(Vec<RosterRow>),
    /// Two original custom-form inputs after selection moves from the first to the second.
    EditSelection,
    /// An externally supplied bounded native mod panel.
    ModPanel(
        Box<ui::mod_panel::Panel>,
        Option<Box<ui::mod_hud::Hud>>,
        Option<(&'static str, render_model::UiTexturePage)>,
    ),
    /// A selected HUD card before or after retained placement and visibility edits.
    HudEditor {
        hud: Box<ui::mod_hud::Hud>,
        edited: bool,
    },
    /// Two same-ID requests arriving while the first menu is still open.
    #[cfg(feature = "test-support")]
    FormOverlap,
    /// A script form shown through data-driven UI.
    #[cfg(feature = "test-support")]
    DataDriven(protocol::ddui::ScriptedScreen),
    /// An edited settings draft after switching away from and back to its tab.
    #[cfg(feature = "test-support")]
    SettingsDraft,
    /// Retained Add Server focus after a simulated keyboard reduces the visible height.
    #[cfg(feature = "test-support")]
    MenuKeyboard,
    /// A damaged stack picked up from the survival inventory.
    #[cfg(feature = "test-support")]
    CursorDurability,
    /// Survival recipe-book inputs admitted by the inventory session and wire decoder.
    #[cfg(feature = "test-support")]
    CraftOverlap {
        selected: bool,
        block_identity: i32,
    },
    /// A client toast as the toast screen shows it once slid in.
    Toast {
        title: String,
        message: String,
    },
}

const SIZE: [u32; 2] = [1280, 720];
const GUI_SCALE: u8 = 2;
const SMALL_SIZE: [u32; 2] = [360, 240];
/// Fits an expanded card above two collapsed ones.
const TALL_SIZE: [u32; 2] = [1280, 900];
/// Keeps the first hotbar rows visible below the keyboard options.
const KEYBOARD_SIZE: [u32; 2] = [SIZE[0], 2000];
/// Reaches the Video pane's Renderer row below the general options.
const SHADER_PACK_ROW_SIZE: [u32; 2] = [SIZE[0], 1600];

/// Opens a settings category by its registered name rather than a copied index.
pub fn settings(section: &str) -> MenuView {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings.section = launcher::menu::settings_sections::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == section).then_some(*index))
        .expect("registered settings category");
    view
}

/// Enumerates every fixture, including the independently owned settings states.
pub fn states() -> impl Iterator<Item = &'static State> {
    catalog::STATES
        .iter()
        .chain(out_of_game::WORLD_FORM_STATES)
        .chain(updates::STATES)
        .chain(shader_pack::STATES)
        .chain(social_settings::STATES)
        .chain(settings_screens::SEARCH_STATES)
        .chain(settings_screens::WORLD_STATES)
        .chain(settings_screens::PAGE_STATES)
        .chain(server_forms::STATES)
        .chain(touch_editor::STATES)
}
